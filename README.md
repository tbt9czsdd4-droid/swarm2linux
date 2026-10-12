# swarm2linux

A Linux replacement for Turtle Beach's Swarm II app, starting with the **Stealth Pro II** headset.

This is an independent interoperability project and is not affiliated with Turtle Beach.

## Status

Reverse engineering, phase 1 (read-only first contact). See [docs/PLAN.md](docs/PLAN.md) for the roadmap and
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
| `10f5:229e` | Stealth Pro II headset on its USB-C cable |

## Reverse-engineering tools

`tools/re/` holds dependency-free Python scripts that talk to `/dev/hidrawN` directly:

```sh
python3 tools/re/probe.py            # read-only: open a session and dump the device state
python3 tools/re/monitor.py --session  # log every packet the device pushes
```

The hidraw node must be readable and writable by your user. Install `packaging/70-swarm2linux.rules` if it is not.

## Safety

The tools only send frames that Turtle Beach's own app has been observed sending. They never send RACE storage
(`0x04xx`) or firmware-update (`0x1Cxx`) commands. Firmware updates are out of scope; use the official mobile app.
