# swarm2linux

A Linux replacement for Turtle Beach's Swarm II app, starting with the **Stealth Pro II** headset.

This is an independent interoperability project and is not affiliated with Turtle Beach.

![The equalizer page](docs/screenshots/equalizer.png)

## Status

The desktop app `stealth-gui` covers the same settings as Swarm II's pages, with a tray icon for the battery.
`stealthctl` reads all 26 records and changes both EQs (presets, bands, saving and deleting custom presets), Superhuman Hearing, ANC, mic monitoring, noise gate, sensitivity and AI noise reduction. It also sets the button and wheel functions, auto shut-off, wake on motion and the charging hub LEDs; that covers every headset setting Swarm II offers on PC except the master-volume mute toggle. See [docs/PLAN.md](docs/PLAN.md) for the roadmap and
[docs/protocol.md](docs/protocol.md) for what is known about the protocol.

## How it works

On Linux, audio already works through `snd-usb-audio`: the transmitter is a standard USB Audio Class 1 device.
All headset settings (EQ, ANC, mic, sidetone, buttons) live on the headset and the transmitter and are changed over
a vendor HID collection (usage page `0xFF13`). That collection carries Airoha RACE packets, and Turtle Beach
tunnels a JSON key/value protocol through them. This project speaks that protocol from Linux.

## Supported hardware

| USB ID      | Device                                   |
|-------------|------------------------------------------|
| `10f5:2287` | Stealth Pro II PC, USB-A transmitter     |
| `10f5:229b` | Stealth Pro II transmitter dock          |
| `10f5:2289` | Stealth Pro II PC headset on its USB-C cable (seen) |
| `10f5:229e` | Stealth Pro II headset on its USB-C cable (other SKU, from HaloBattery) |

## stealth-gui (desktop app)

A Slint app in the style of Swarm II, with four pages:

- **Settings:** master volume, Superhuman Hearing (mode and level), noise cancellation (ambient to full), and the
  microphone (sensitivity, monitoring, noise gate, AI noise reduction).
- **Equalizer:** game and mic EQ as a draggable 10-band graph, the onboard presets, and saving and deleting custom
  presets.
- **Controls:** what the mode button and the assignable wheel do.
- **Device:** battery, transmitters and firmware, auto shut-off and wake on motion, the charging hub's LEDs,
  battery pop-ups, the tray icon and starting with the session.

![The settings page](docs/screenshots/settings.png)

It is a client of `stealthd` and needs it running. The tray icon shows the battery as a headset whose ear cups
fill up; its menu switches noise cancellation, Superhuman Hearing and the game EQ preset. Closing the window keeps
the app in the tray (turn the tray off on the Device page to quit on close), and `stealth-gui --hidden` starts in
the tray. `packaging/install-user.sh` installs it with a desktop entry.

Built with [Slint](https://slint.dev) under its royalty-free licence; the Device page carries the credit.

## stealthd (background service)

`stealthd` owns the headset: it finds it (preferring the transmitter over the headset's own cable), keeps a session
with every record subscribed, follows unplugging, and serves everything on the session bus. It also shows a
notification when the battery drops to 15 % and 5 % (the `BatteryNotifications` property turns that off; it is
kept in `~/.config/swarm2linux/stealthd.toml`).

```sh
packaging/install-user.sh      # installs stealthctl, stealthd and stealth-gui to ~/.local/bin, the desktop entry,
                               # and enables the user service
```

D-Bus: `io.github.swarm2linux.Stealthd`, object `/io/github/swarm2linux/Headset`, interface
`io.github.swarm2linux.Headset1` with `Values() -> a{sv}`, `Set(s, v)`, `SetBand(s, u, i)`, `SavePreset(s, s)`,
`DeletePreset(s, s)` and the signal `Changed(a{sv})`. Value names are listed in `crates/tb-proto/src/values.rs`.

```sh
busctl --user call io.github.swarm2linux.Stealthd /io/github/swarm2linux/Headset \
    io.github.swarm2linux.Headset1 Set sv anc b true
```

When `stealthd` runs, `stealthctl` goes through it; the raw commands (`dump`, `get`, `set`, `snapshot`, `restore`)
need direct access and ask you to stop the service first.

## stealthctl

```sh
cargo build --release
B=./target/release/stealthctl
$B status                      # battery, transmitters, volume, both EQs, Superhuman Hearing, ANC, mic
$B eq list                     # game EQ: bands with frequencies, built-in and custom presets
$B eq preset bass              # signature | bass | bass-treble | vocal | <custom preset name>
$B eq band 1 -3.5              # dB in steps of 0.5
$B eq save MyPreset            # save the current bands as a custom preset (5 slots); eq delete MyPreset
$B mic-eq preset clarity       # signature | full | clarity | smooth; band/save/delete as for eq
$B shh on                      # Superhuman Hearing; off, mode legacy|footsteps|gunshots, level 0-100
$B anc on                      # off, level 0-100 (0 = ambient, 100 = full noise cancellation)
$B monitor level 60            # how loud you hear your own voice
$B mic gate on                 # mic: sensitivity N, gate on|off, gate-level N, ai on|off
$B button cycle-eq             # top left button: anc | cycle-eq | noise-gate
$B wheel chat-mix              # assignable wheel: monitoring | chat-mix | bass | treble | noise-gate
$B power auto-off 20           # never | 5 | 10 | 20 | 30; power wake-on-motion on|off
$B hub led1 30                 # charging hub LED brightness: led1 | led2 0-100
$B watch                       # print changes as they happen (buttons, wheel, battery, CrossPlay)
$B snapshot my.json            # save everything; `restore my.json` writes the settings back
$B dump                        # every record as JSON
```

Set `RACE_HID_TRACE=1` to print every HID report.

## Reverse-engineering tools

`tools/re/` holds dependency-free Python scripts that talk to `/dev/hidrawN` directly:

```sh
python3 tools/re/probe.py            # read-only: open a session and dump the device state
python3 tools/re/monitor.py --session  # log every packet the device pushes (survives replugging)
python3 tools/re/decode_log.py --changes captures/linux/monitor-*.jsonl  # replay a saved log
python3 tools/re/pcap2jsonl.py capture.pcapng -o capture.jsonl  # Windows USBPcap capture -> same log format
python3 tools/re/kvp.py dump --save    # read all 26 records, save a snapshot
python3 tools/re/kvp.py watch 60       # subscribe to everything and print pushed changes
python3 tools/re/kvp.py set 0x1210 2   # write a key (only keys/values Swarm II was seen writing)
```

The hidraw node must be readable and writable by your user. Install `packaging/70-swarm2linux.rules` if it is not.

## Safety

The tools only send frames that Turtle Beach's own app has been observed sending. They never send RACE storage
(`0x04xx`) or firmware-update (`0x1Cxx`) commands. Firmware updates are out of scope; use the official mobile app.
