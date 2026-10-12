# Stealth Pro II control protocol

Living spec. Every statement is tagged with where it comes from:

- **[seen]**: observed on our own hardware (`10f5:2287`, firmware `4.107.703.0`)
- **[capture]**: from a USB capture of Turtle Beach's Swarm II
- **[guess]**: hypothesis, not yet tested

## 1. USB device

The transmitter `10f5:2287` "Stealth Pro II PC" has four interfaces **[seen]**:

| Interface | Class                 | Contents |
|-----------|-----------------------|----------|
| 0         | Audio Control (UAC1)  | Speaker feature unit (master mute + volume), mic feature unit (mute + volume) |
| 1         | Audio Streaming       | Stereo out, 16 or 24 bit, 48 or 96 kHz, EP 0x01 isochronous |
| 2         | Audio Streaming       | Mono mic in, 16 bit, 16 or 48 kHz, EP 0x81 isochronous |
| 3         | HID                   | EP 0x82 interrupt IN, no OUT endpoint |

The HID report descriptor (68 bytes) has two collections **[seen]**:

- Consumer Control, report `0x0C`: 8 one-bit inputs for Volume Up, Volume Down, Mute, Play/Pause, Next, Previous,
  Fast Forward, Rewind.
- Vendor page `0xFF13` usage `0x01`: output report `0x06` and input report `0x07`, 61 data bytes each.

## 2. HID framing

```
out: 06 <n> <target> <n bytes of RACE stream> <zero padding to 62 bytes>
in:  07 <n> <source> <n bytes of RACE stream> <padding>
```

- Output reports go out with a plain `write()` on hidraw. Without an OUT endpoint, the kernel sends a SET_REPORT
  control transfer **[seen]**.
- **Replies come only on GET_REPORT(Input, 0x07)** (hidraw `HIDIOCGINPUT`). Nothing arrives on the interrupt
  endpoint **[seen]**. Poll at about 60 ms **[seen]**. An idle report has `n = 0`.
- The input side is a **byte stream**. A RACE packet can start in the middle of a report and span several reports,
  so cut packets by their own length field **[seen]**.
- The transmitter **queues its output while nobody polls**: a fresh poll first returns old events. That queue is
  bounded, so its oldest bytes can be cut off and the reader must resync **[seen]**.
- `target` in our asks is `00`. On the Audeze Maxwell, which uses the same transport, `80` relays to the headset
  **[guess]** for this device.

## 3. RACE packets

Airoha RACE; the same as `race-toolkit`'s `librace/packets.py`:

```
05 <type> <length u16 LE> <cmd u16 LE> <payload: length - 2 bytes>
```

| type | meaning |
|------|---------|
| `5A` | command expecting a response |
| `5B` | response |
| `5C` | command without response (the transmitter also sends these on its own) |
| `5D` | indication |

### Unsolicited packets from the transmitter [seen]

| cmd    | type | payload examples | notes |
|--------|------|------------------|-------|
| `2c80` | 5C   | `00`, `01`, `03` | **[guess]** link/connection state |
| `2cb1` | 5D   | `00 02 01 01 93 34 92 ed ca c7 80 01`, `00 02 00 01 93 34 92 ed ca c7 ff 00` | contains a Bluetooth address `c7:ca:ed:92:34:93` (little-endian); byte 2 is `01` or `00` **[guess]** link up/down |
| `3103` | 5D   | `01` | |
| `3110` | 5D   | `39`, `19` | |

## 4. Turtle Beach application layer: RACE cmd `9902`

Requests and responses are RACE cmd `0x9902`. The payload has a 20-byte header (bytes 0–19 below) followed by
either a record name (request) or a JSON document (response).

```
request  (5A): 48 01 <op> <tok0> <tok1> 00 00 00 00 00 00 00 61 00 <ASCII action + record, e.g. "SInf">
response (5B): 68 45 <op> <tok0> <tok1> 00 00 00 00 00 00 00 61 <x> 62 32 <y> 21 01 ff <JSON document>
```

- Byte 0: `48` request, `68` response; bit 5 is the response flag **[guess]**.
- Bytes 2–4 are echoed in the response **[seen]**. Asks seen: `c0 4a 01` for `SInf` and `44 14 04` for `SGSI`
  **[capture]**. Whether the device checks these bytes is unknown.
- Byte 13 of the response: `02` for `Inf`, `09` for `GSI` **[seen]**.
- Byte 16 of the response: `61` or `20` across replies to the same ask **[seen]**.
- A record request is `S` + the record name **[capture]**. `S` is probably a get or subscribe action **[guess]**.

JSON documents **[seen]**:

- `{"OR":"<record>","KVP":{...}}` answers a request (OR = object response **[guess]**).
- `{"UP":"<record>","KVP":{...}}` is pushed with only the changed keys **[capture]**.

All keys are hexadecimal strings and all values are strings.

### Record `Inf` (asked with `SInf`, the session opener) [seen]

| key | value seen      | meaning |
|-----|-----------------|---------|
| 100 | `10F5`          | USB vendor ID |
| 110 | `2289`          | product ID; not the transmitter's `2287`, **[guess]** the headset's |
| 120 | `00000000000`   | serial number |
| 130 | `4.107.703.0`   | firmware version |
| 140 | `1`             | |
| 150 | `1`             | |
| 160 | `2`             | |

### Record `GSI`, general status info (asked with `SGSI`)

| key | seen | meaning |
|-----|------|---------|
| 200 | `1`  | **[guess]** headset connected |
| 210 | `0`  | |
| 220 | `My Headset` | headset name, user-editable in Swarm II **[capture]** |
| 230 | `2`  | `2` on battery, `0` on cable **[capture]** |
| 240 | `65` | battery percent **[capture]** |
| 250 | `0`  | charging `0`/`1` **[capture]** |
| 260 | `100` | **[guess]** spare battery in the dock |
| 270 | `0`  | |
| 280 | `4`  | enum, `1` on battery / `2` on cable in HaloBattery's run **[capture]** |
| 290 | `3`  | enum, `2` / `4` in HaloBattery's run **[capture]** |
| 2a0 | `42` | **[guess]** a volume or level |
| 2b0 | `60` | **[guess]** a volume or level |
| 2c0 | `50` | **[guess]** a volume or level |
| 2d0 | `78` | **[guess]** a volume or level |
| 2e0 | `0`  | |

## 5. Open questions

1. What do bytes 1–4 of the `9902` header mean (op, token), and does the device check them?
2. What are the write commands? Needs Swarm II captures (see `windows-harvest-checklist.md`).
3. Which other records exist (EQ, ANC, mic, buttons)? The capture's "index" ask (packet 280) probably lists them.
4. What do the unsolicited `2c80` / `2cb1` / `3103` / `3110` packets mean?
5. Does `target = 80` reach the headset directly?

## Sources

- HaloBattery `providers/turtlebeach.py` and `docs/protocols.md` (MIT): the Swarm II capture frames and the GSI
  keys 220/240/250. https://github.com/HeyOkay/HaloBattery/pull/194
- race-toolkit `librace/`: the RACE header and type values. https://github.com/auracast-research/race-toolkit
- HeadsetControl `lib/devices/audeze_maxwell.hpp`: the same transport on the Audeze Maxwell.
  https://github.com/Sapd/HeadsetControl
