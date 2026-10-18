# Linux port of Turtle Beach Swarm II for the Stealth Pro II

## Context

The Turtle Beach Stealth Pro II is configured with the vendor's Swarm II app, which only exists for Windows, iOS and Android. The goal is a feature-complete Linux replacement. The project directory `/home/patrick/projects/swarmII` is empty and not yet a git repo.

**Verdict: this is feasible, and the hard part is reverse engineering (RE), not porting.** Swarm II works as a remote control: every setting (EQ, ANC, mic, sidetone, buttons) is stored and processed on the headset and dongle. So we don't port the Windows code. We rebuild the same remote control against a protocol that turns out to be quite approachable.

### What's already established (checked on your machine, plus research)

- **Your dongle is `10f5:2287` "Stealth Pro II PC"**, using `hidraw9` and ALSA card 3. Other IDs seen in public sources: `10f5:229b` (the transmitter/charging dock) and `10f5:229e` (the headset itself on its USB-C cable). Xbox-SKU IDs are still to be collected.
- **Audio already works on Linux.** The dongle is a standard USB Audio Class 1 device: stereo out at 48/96 kHz in 16/24-bit, mono mic in at 16/48 kHz, and only master volume and mute. `snd-usb-audio` handles it. No kernel driver is needed.
- **Control goes over a vendor HID collection.** It sits on interface 3, usage page `0xFF13`. Output report `0x06` and input report `0x07` each carry 61 bytes after the report ID, and replies arrive on interrupt endpoint `0x82`. A second collection, report `0x0C`, carries the Consumer-page media keys (volume, mute, play/pause, next/previous).
- **The transport is Airoha RACE over HID.** Frames look like `06 <len> <target> | 05 5A <len16 LE> <cmd16 LE> <payload>`. The same framing is used by:
  - the Audeze Maxwell, already supported in HeadsetControl (`lib/devices/audeze_maxwell.hpp`, 60 ms pacing);
  - the ERNW `race-toolkit` (`librace/transport.py` `USBHIDTransport`, `librace/packets.py`).
- **Turtle Beach adds a JSON layer on top.** It's tunneled in a custom RACE command `0x9902`. HaloBattery PR #194 (`providers/turtlebeach.py`) decoded part of it from a Swarm II capture:
  - A session opener `SInf` makes the device dump its whole state.
  - Records are requested by name: `SGSI` asks for General Status Info.
  - Replies look like `{"OR":"GSI","KVP":{"220":"<name>","240":"<batt%>","250":"<charging>",...}}`.
  - The device also pushes changes by itself, e.g. `{"UP":"GSI","KVP":{...}}`.
  - Both captured frames (`ASK_SINF`, `ASK_GSI`) are published byte for byte. The frame header carries a per-session token (`14 04`, `11 01`) that we must understand.
  - Only one host session works at a time: while Swarm II is open it holds the channel.
- **Features we have to reproduce**, according to reviews and the store listings:
  - battery level and charging state, and the headset name
  - 4 onboard EQ presets plus a custom 10-band EQ
  - Superhuman Hearing: on/off, 3 levels and an intensity setting
  - noise control: ANC / Transparency / Off, ANC strength, up to 5 custom ANC profiles
  - mic: sensitivity, noise gate, mic EQ presets, AI noise reduction, monitoring (sidetone)
  - remapping the mode button and the dial
  - voice prompts
  - auto-shutoff (probably)
  - CrossPlay transmitter / Bluetooth status
  - firmware version
  - Windows-only, handled on the PC rather than the headset: Dolby Atmos, per-app profiles, global hotkeys
- **Decisions from you:**
  - Windows access: dual-boot, captured with USBPcap.
  - Stack: Rust with a Qt/QML Kirigami GUI (via cxx-qt).
  - Firmware updates: deferred; use the iOS app for those.
  - Control path: USB only for now.

## Approach

Two tracks run in parallel:
- **RE tooling in Python** (`tools/re/`). It's throwaway-friendly and fast to iterate.
- **The product in Rust** (Cargo workspace), plus a QML GUI.

The protocol spec in `docs/protocol.md` is the contract between the two tracks.

### Safety rules (apply in every phase)

1. Only send frames whose exact shape Swarm II has been seen sending. Read-only asks are the exception. Never send RACE storage commands (`0x04xx`), FOTA commands (`0x1Cxx`), or `0x9902` sub-opcodes we haven't seen.
2. Before the first write in any session, save a full `SInf` state dump to `captures/snapshots/`, so every setting can be restored.
3. Look up the headset's factory-reset button combo before the first write.
4. Don't redistribute Turtle Beach binaries or firmware. Name the project without the "Swarm" or "Turtle Beach" trademarks; the working name is `stealthctl`.

### Phase 0: Project setup (Linux)
- Run `git init`. Create a Python venv for `tools/re` with `hidapi`, plus `pyshark` or tshark-JSON parsing.
- Install the packages: `wireshark-cli`, `qt6-qtdeclarative-devel`, `kf6-kirigami-devel`, `hidapi-devel`. This needs sudo, so you'll run it.
- Install a udev rule, `packaging/70-stealthctl.rules`, granting `TAG+="uaccess"` for `10f5:2287/229b/229e`. hidraw9 is already `0666` here, but other systems need the rule.

### Phase 1: Read-only first contact from Linux (no Windows needed)
- `tools/re/race_hid.py`: open the `0xFF13` collection and encode/decode the `06/07` framing and the RACE header. Reassemble the `07 <len> 00 …` payloads into a stream and pull JSON documents out of it, porting the brace scanner from HaloBattery.
- `tools/re/probe.py`: send the published `ASK_SINF` and `ASK_GSI` frames and save the full dump to `captures/linux/baseline-*.json`.
- `tools/re/monitor.py`: listen passively and log every pushed `UP` document with a timestamp.
- You then work the physical controls while I watch the log: ANC button, Superhuman Hearing (power button), EQ preset cycling, mic flip-to-mute, the volume wheels, battery swap, USB-C cable plug, BT pairing, CrossPlay switch, and taking the headset off.
- This should map most status keys before Windows is involved at all.
- Deliverable: `docs/protocol.md` v0 with a key table (key → meaning → values → source).

### Phase 2: One Windows harvest session (you, following a checklist I write)
- `docs/windows-harvest-checklist.md` will cover:
  - **Copy the app files** to a partition both systems can read (Fedora's `ntfs3` reads NTFS) or a USB stick:
    - the whole Swarm II install folder
    - `%ProgramData%`, `%AppData%` and `%LocalAppData%` Turtle Beach folders (plugins, device definitions, logs, caches)
  - **Install USBPcap + Wireshark** and capture only the root hub the dongle is on.
  - **Make one `.pcapng` per action, named by an item ID**, for example `07-anc-strength-0-to-max.pcapng`. Wait 5 s between steps.
  - Items to capture:
    - app launch
    - each onboard EQ preset
    - each custom EQ band at its minimum and maximum
    - saving a custom preset
    - Superhuman Hearing on/off, each level, intensity at both ends
    - each ANC mode and ANC strength
    - creating, renaming and deleting a custom ANC profile
    - each mic setting at both ends, and each mic EQ preset
    - AI noise reduction on/off
    - sidetone at both ends
    - voice prompts, auto-off
    - each possible button/dial mapping
    - renaming the headset
    - the transmitter/Bluetooth pages
    - profile save/load
- **Do not run a firmware update during the session.**

### Phase 3: Static analysis of the harvested app (Linux)
- Find out what it's built with:
  - .NET (likely, given 1.0.0.x versioning): `ilspycmd` gives near-source.
  - Electron: unpack `app.asar`.
  - Native: Ghidra.
- Extract:
  - command and record names (`SInf`, `SGSI`, the index ask, set-commands)
  - the full KVP key catalogue with names, types and ranges
  - EQ band frequencies, gains and Q
  - preset tables, ANC profile and button-map encodings
  - how the frame header token and the target byte (`0x00` vs Maxwell's `0x80` relay) are generated
- Optional second source if the Windows binary is native/obfuscated: the Android APK (`com.turtlebeach.swarm2`) via jadx.

### Phase 4: Decoder, full spec, replay validation
- `tools/re/pcap_decode.py`: turn a pcapng into an annotated transcript (HID → RACE → 0x9902 tunnel → JSON), using `tshark -T json` on `usbhid.data` / `usb.capdata`.
- `tools/re/diff.py`: compare captures to isolate exactly what each setting writes.
- For each setting:
  1. Replay Swarm II's frame byte for byte from Linux.
  2. Generate the same frame with our encoder and check it matches.
  3. Read back via the GSI/record ask.
  4. Confirm by ear or physically (EQ, ANC, sidetone and prompts are all audible).
- Finish `docs/protocol.md`: framing, session/token rules, every record and key, every write.

### Phase 5: Rust core and CLI (`Cargo.toml` workspace)
- `crates/race-hid`:
  - finds hidraw devices through udev, keeping only the `0xFF13` collection of known PIDs
  - `06/07` framing, RACE packet encode/decode, the 60 ms send pacing, and frame reassembly
  - matching replies to requests by token
- `crates/tb-proto`:
  - the `0x9902` tunnel and its message types (session open, index, record get/set, push)
  - `serde_json` KVP documents and a typed `HeadsetState` and settings model
  - the device catalogue (PIDs and capabilities)
- Tests use golden frames taken from the Phase 2 captures as fixtures in `crates/*/tests/fixtures/`.
- `crates/stealthctl` (clap): `status`, `monitor`, `eq preset|set`, `anc mode|level|profile`, `shh`, `mic …`, `sidetone`, `buttons`, `prompts`, `autooff`, `rename`, `dump`, `restore <snapshot>`.
  - It talks directly to the device, or goes through the daemon when one is running.

### Phase 6: Daemon `stealthd`
- Holds the device exclusively and keeps the session open. It handles push updates, hotplug (udev monitor), dongle/dock/cable switching, and suspend/resume.
- D-Bus API via `zbus` on the session bus: interface `io.github.stealthctl.Headset1`, with state properties, set methods and `PropertiesChanged` signals.
- Profiles are TOML files in `~/.config/stealthctl/profiles/`. Re-applying the active profile on reconnect is optional.
- Low-battery and connection notifications via `org.freedesktop.Notifications`.
- Systemd user unit `packaging/stealthd.service`.

### Phase 7: GUI (`crates/stealth-gui`, Rust + Slint) — done
Changed from the first plan: Slint instead of cxx-qt + QML/Kirigami. It is pure Rust, needs no C++ bridge and only
the fontconfig headers, and suits Swarm II's custom dark look; since 1.18 it has a StatusNotifierItem tray icon.
- Pages after Swarm II's:
  - **Settings:** master volume (read-only), Superhuman Hearing, noise cancellation (AMB to ANC), microphone
  - **Equalizer:** game and mic EQ graph, onboard and custom presets, save and delete
  - **Controls:** mode button and assignable wheel functions, on a drawing of the left earcup
  - **Device:** status, power options, charging hub, notifications and startup, About
- Tray icon with the battery level and a menu (Slint 1.18 tray menus have no check marks, so labels say what is on).
- One instance at a time (`io.github.swarm2linux.Gui`), `--hidden` starts in the tray, optional autostart entry.
- The GUI is only a D-Bus client of `stealthd`. Not shown because Swarm II's writes were never captured: the
  master-volume mute toggle and the voice prompt. Swarm II's hotkeys belong to Phase 8.

### Release 1.0 — done
- `install.sh` installs the build tools (dnf, apt, pacman or zypper) and Rust (rustup) when they are missing, the
  udev rule, the three programs in `~/.local/bin`, the menu entry and icon, and the `stealthd` user service.
  `--uninstall` removes it again. Checked on clean Fedora 44 and Ubuntu 24.04 containers.
- Not covered: immutable systems (Bazzite, Kinoite, Silverblue, SteamOS) and prebuilt binaries or distribution
  packages (RPM, Flatpak).

### Release 1.1 — done
- The master volume is adjustable from the app and with `stealthctl volume`, through PipeWire (see Phase 8).
- `stealthd` ends a headset quirk: after a restart, AI noise reduction can keep working while the headset reports
  it off.
- Mic recordings from Linux showed that AI noise reduction, the noise gate and the mic EQ all act on what the PC
  records (`docs/protocol.md`).

### Release 1.2 — done
- `stealth-tui`, a terminal app with the desktop app's pages (ratatui 0.30 with crossterm), vim keys, arrows and
  the mouse. It is a client of `stealthd` like the desktop app; both share `stealth_dbus::link` (the daemon threads)
  and `stealth_dbus::view` (the preset name check, transmitter lines and EQ Cancel).
- `install.sh` installs it with the other programs.
- 1.2.1: `H`/`L` and `Shift-←`/`Shift-→` switch pages; the ±10 slider step moves to `Ctrl-←`/`Ctrl-→` (or a count).

### Release 1.3 — done
- The README has a step-by-step installation next to `install.sh`: the shared steps explained once, then a
  complete guide each for Fedora, Ubuntu/Debian (and Mint, Pop!_OS), Arch (and Manjaro, EndeavourOS) and
  openSUSE. Clean Fedora 44, Ubuntu 24.04, Debian 13, Arch and openSUSE Tumbleweed containers ran each guide as
  written, and each installs the same files as `install.sh`.
- The extra task bar icon after autostart on Plasma is recorded under Known issues.

### Phase 8: Replacements for Windows-only host features
- **Master volume — done:** the headset's volume (`2a0`) follows the USB playback volume and takes no writes Swarm II
  was seen making. So `stealthd` sets the headset's sink volume in PipeWire (`wpctl`) and serves it as
  `sink_volume`, read with `pw-dump` whenever `2a0` moves and every 5 s. The app's master volume slider and
  `stealthctl volume` use it.
- **Per-app profiles:** a KWin script reports the active window to `stealthd` over D-Bus. This is the reliable way to see the active window on Plasma Wayland.
- **Global hotkeys:** the `org.freedesktop.portal.GlobalShortcuts` portal (via `ashpd`).
- **Dolby Atmos can't be ported** (it's a licensed Windows component). Offer an optional PipeWire filter-chain virtual-surround preset as a substitute.
- **AI noise reduction:** if Phase 3 shows it runs on the PC rather than the headset, offer a PipeWire noise-suppression substitute (RNNoise or DeepFilterNet).
- **Out of scope:** other Turtle Beach and Roccat peripherals that Swarm II also manages.

### Known issues
- **A task bar icon after autostart (Plasma 6.7, Wayland).** When `stealth-gui --hidden` starts with the session,
  Plasma's task bar shows a swarm2linux icon next to the tray icon. Clicking it does nothing. It goes away once the
  window has been opened from the tray and closed again.
  - Found so far:
    - only that one process runs, started by `app-io.github.swarm2linux@autostart.service`
      (systemd-xdg-autostart-generator), and its environment has no `XDG_ACTIVATION_TOKEN` or `DESKTOP_STARTUP_ID`
    - KWin lists no swarm2linux window (WindowsRunner `Match`)
    - with the tray on, `--hidden` never shows the window; Slint creates the winit window only when it is shown,
      and the tray (ksni) has none
    - the behaviour fits Plasma's launch feedback: a startup task waiting for a window with the app id
      `io.github.swarm2linux`, which the real window later replaces. KWin makes such feedback for activation
      tokens that carry an app id and reads `X-KDE-StartupNotify`.
  - Not yet known: who asks for that feedback at login.
  - Next steps:
    - see whether `systemctl --user restart app-io.github.swarm2linux@autostart.service` brings the icon back, or
      only a login does
    - try `StartupNotify=false` in the autostart entry (`crates/stealth-gui/src/prefs.rs`) or the menu entry
    - find what requests the token

### Deferred
- Firmware updates: document the flow only (`race-toolkit` `librace/fota.py` is a reference). Keep using the iOS app.
- Bluetooth-only (BLE GATT) control.
- Upstreaming basic support (battery, sidetone, presets) to HeadsetControl.

## Files to create (representative)
- `tools/re/{race_hid,probe,monitor,pcap_decode,diff}.py`
- `docs/protocol.md`, `docs/windows-harvest-checklist.md`
- `crates/race-hid/src/lib.rs`, `crates/tb-proto/src/{lib,tunnel,records,model}.rs`, `crates/stealthctl/src/main.rs`, `crates/stealthd/src/main.rs`, `crates/stealth-gui/{src,qml}/`
- `packaging/70-stealthctl.rules`, `packaging/stealthd.service`, `.desktop` file, RPM spec

## Code to reuse or port
- **HaloBattery `providers/turtlebeach.py` (MIT):** the `ASK_SINF` and `ASK_GSI` frames, `strip_frames`, the `json_documents` brace scanner, and the GSI key meanings.
- **race-toolkit `librace/packets.py`, `transport.py`, `constants.py`:** the RACE header layout and the type bytes `5A` (command expecting a response), `5B` (response), `5C` (command without response), `5D` (indication). It's a reference only; check its license before copying code.
- **HeadsetControl `lib/devices/audeze_maxwell.hpp`:** the same HID/RACE transport with `hid_get_input_report` reads and 60 ms pacing, which confirms the transport details.

## Verification
- **Unit tests:** `cargo test` round-trips every golden frame from the captures, and every encoded write matches Swarm II byte for byte.
- **Decoder:** `pcap_decode.py` turns every Phase 2 capture into a transcript with no unknown frames left.
- **On the hardware:** for every setting, `stealthctl <set>`, then read the state back, then confirm by ear or physically. Afterwards `stealthctl restore` returns to the baseline snapshot.
- **Cross-check in Windows:** set things from Linux, boot Windows, and confirm Swarm II shows the same values. This works because the settings live on the headset.
- **Daemon:** unplug/replug the dongle, swap batteries, power-cycle the headset, suspend/resume, switch between dongle, dock and cable. The state must stay correct and the GUI and tray must update live.
- **GUI:** smoke test on KDE Plasma Wayland.

## References
- [HaloBattery PR #194 – Stealth Pro II protocol](https://github.com/HeyOkay/HaloBattery/pull/194)
- [ERNW race-toolkit (Airoha RACE)](https://github.com/auracast-research/race-toolkit)
- [HeadsetControl (Audeze Maxwell, same transport)](https://github.com/Sapd/HeadsetControl)
- Feature sources: [SoundGuys review](https://www.soundguys.com/turtle-beach-stealth-pro-ii-review-great-sound-bad-build-157082/), [CGMagazine review](https://www.cgmagonline.com/review/hardware/turtle-beach-stealth-pro-ii/), [Swarm II on the App Store](https://apps.apple.com/us/app/swarm-ii/id6468891260), [Turtle Beach support – Stealth Pro II Swarm II](https://support.turtlebeach.com/s/?language=en_US#/390/0/0/All-models/article/a32UR0000025kpyYAA)
