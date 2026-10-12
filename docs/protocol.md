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

The headset on its own USB-C cable enumerates as `10f5:2289` "Stealth Pro II PC Headset" **[seen]**, with the same
audio interfaces and the same two collections. It adds a standard Telephony collection (page `0x0B`, report
`0x05`: hook switch, phone mute, LED outputs) for softphones. It speaks the same protocol and serves the same
records as the transmitter, and both can be connected at the same time **[seen]**. After the battery is pulled
while the cable is in, the headset re-enumerates at full speed instead of high speed **[seen]**.

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
| `2c80` | 5C   | `00`, `01`, `03`, `1b` | Transmitter: `01` when the headset drops (battery pulled), `03` when it is back. Headset cable: `1b` a few seconds before `03`, also right before a wheel moved. **[guess]** link or activity state |
| `2cb1` | 5D   | `00 02 01 01 93 34 92 ed ca c7 80 01`, `00 02 00 01 93 34 92 ed ca c7 ff 00` | wireless link to the headset: byte 2 `01` up / `00` down, together with the headset's address `c7:ca:ed:92:34:93` (little-endian) **[seen]** on battery pull and reinsertion |
| `3103` | 5D   | `01` | sent right after the link comes up **[seen]** |
| `3110` | 5D   | `39`, `19`, `03` | |

## 4. Turtle Beach application layer: RACE cmd `9902`

Requests and responses are RACE cmd `0x9902`. The payload is a 12-byte header followed by a body **[capture]**:

```
request:  48 <op> <session> <counter> <seq> 00 00 00 00 00 00 00 <body>
response: 68 <op'> <session> <counter> <seq> 00 00 00 00 00 00 00 <body>
```

- `session`, `counter` and `seq` are chosen by the host and echoed. Swarm II picks a session byte per run (`aa`), a
  counter that runs on across sessions, and a sequence from 1. The device accepts any values **[seen]**: our own
  random tokens work.

| op | body | meaning |
|----|------|---------|
| `01` | `b5` `index` | list the records; the answer is a JSON array of record names |
| `01` | `61 00` `S` + name | subscribe to a record: answers it now and pushes `UP` changes later |
| `01` | `61 00` `R` + name | read a record once. Swarm II reads `BT` this way; `S` + `BT` gets no answer **[seen]** |
| `03` | `b7` `set_kvp` `ff` + JSON | write keys, e.g. `{"0x1210":"2"}`. Keys are written `0x`-prefixed, values as strings |

- Response `op'` is `45` for a reply carrying a document and `43` for the ack of a `set_kvp`. A document reply's
  body is `61 <x> 62 32 <y> 21 01 ff` followed by the JSON, or `c1 32 21 01 ff` followed by the array for `index`.
- After a `set_kvp` ack the device pushes the changed keys as an `UP` document, including keys that changed as a
  consequence (a preset change pushes its band values) **[seen]**.
- A frame must fit one 62-byte report: a `set_kvp` JSON of up to 32 characters. How Swarm II writes longer values
  (custom EQ with a name) is not captured yet.
- Swarm II also sends `9902` with payload `70 00 00 5c` right after its first `SInf` (no answer seen), and RACE cmd
  `1c07` (FOTA range; it answers with the firmware version). We don't need either.

JSON documents: `{"OR":"<record>","KVP":{...}}` answers a request; `{"UP":"<record>","KVP":{...}}` is pushed with
only the changed keys. Keys are hexadecimal strings; values are strings, or objects for `TX*` and the custom EQs.

### Record index [seen]

`Ver Inf GSI BT TX1 TX2 TX3 TX4 3DT Mic SAF Enc Btn AQG AQM CEC CG1..CG5 CM1..CM5`

Each record owns a key range: `Inf` 1xx, `GSI` 2xx, `BT` 3xx, `TX*` 4xx, `3DT` 5xx, `Mic` 6xx, `SAF` 7xx,
`Enc` axx, `Btn` bxx, `AQG` 12xx, `AQM` 13xx, `CEC` 16xx, `CG*` 17xx, `CM*` 18xx.

Writes marked ✎ were seen from Swarm II, with the values it used.

#### `Ver`
| key | seen | meaning |
|-----|------|---------|
| 0 | `2.0.0` | protocol version **[guess]** |

#### `Inf`: device information (subscribed with `SInf`)
| key | seen | meaning |
|-----|------|---------|
| 100 | `10F5` | USB vendor ID |
| 110 | `2289` | the headset's own USB product ID (its cable ID) |
| 120 | `00000000000` | serial number |
| 130 | `4.107.703.0` | firmware version |
| 140 | `1` | number of transmitters the headset is connected to: `0` with the battery out, `2` once a second laptop's transmitter came up **[seen]** |
| 150 | `1` | active transmitter (`1` = `TX1`, `2` = `TX2`); `0` while there is no link. Follows the CrossPlay button **[seen]** |
| 160 | `2` | |

#### `GSI`: general status
| key | seen | meaning |
|-----|------|---------|
| 200 | `1` | **[guess]** headset connected |
| 210 | `0` | |
| 220 | `My Headset` | headset name, user-editable in Swarm II **[capture]** |
| 230 | `2` | wireless link state **[guess]**: briefly `0` while CrossPlay switched transmitters, then `2` **[seen]**; HaloBattery saw `0` on cable |
| 240 | `65` | battery percent; `255` = unknown (battery pulled, fresh battery being measured) **[seen]** |
| 250 | `0` | charging `0`/`1` **[seen]** |
| 260 | `100` | stayed `100` across a battery swap; **[guess]** spare battery in the dock |
| 270 | `0` | |
| 280 | `4` | enum; HaloBattery saw `1` on battery, `2` on cable |
| 290 | `3` | power state: `3` on battery, `5` charging over USB **[seen]** |
| 2a0 | `42` | left wheel: 2.4 GHz volume `0`–`100`, uneven steps (0, 1, 3, 6, 8, 12, 16, 21, 26, 30, 36, 42, 48, 54, 62, 71, 76, 87, 93, 100). The wheel also makes KDE show its volume pop-up **[seen]** |
| 2b0 | `60` | |
| 2c0 | `50` | |
| 2d0 | `78` | |
| 2e0 | `0` | |

#### `BT`: Bluetooth (read with `RBT`; pushes `320` about every 10 s)
| key | seen | meaning |
|-----|------|---------|
| 300 | `1` | **[guess]** Bluetooth on / connected |
| 310 | `0` | |
| 320 | `195`, `-51`…`-64` | **[guess]** link quality: the record read gives `195`/`203`, the pushes give RSSI-like negative dBm |
| 330–350 | `0` | |

#### `TX1`..`TX4`: CrossPlay transmitter slots (keys 400, 420, 440, 460)
`{"info":[state,b,c,d,e,VID,PID,firmware,address],"control":[x,y,z]}` **[seen]**:

- `info[0]` state: `2` = active (audio from this transmitter), `1` = connected but not active, `0` = empty slot.
  Pressing CrossPlay swaps `2` and `1` between the slots and changes `Inf.150` to the active slot number.
- `info[1]`: `17` (0x11) for this PC's `2287` transmitter, `33` (0x21) for a second laptop's `2288` transmitter.
- `info[3]`: `100` for `TX1`, `0` for `TX2`. `info[4]`: `1` when present.
- `info[5..8]`: the transmitter's USB VID, PID, firmware and radio address.
- `control`: `["0","100","100"]` for both live transmitters, all `0` for an empty slot.

Seen: `TX1` = `10f5:2287` `C5:43:B0:09:56:F3` (this PC), `TX2` = `10f5:2288` `CB:AF:2C:41:F2:F3` (a second
laptop's transmitter). `2288` is the third product ID listed in Swarm II's `Update.ini`.

#### `3DT`
| key | seen | meaning |
|-----|------|---------|
| 500–530 | `0` | **[guess]** 3D / spatial audio |

#### `Mic`
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 600 | `0` | | mic muted: flip-to-mute up `1`, down `0` **[seen]** |
| 610 | `51`, `84` | ✎ 100, 0, 84 | **[guess]** mic volume. The device pushed the new value *before* Swarm II's write, so it probably follows the Windows mic level (UAC feature unit 6) |
| 620 | `70`, `85` | ✎ 0, 100, 85 | **[guess]** noise gate or sensitivity |
| 630 | `0` | ✎ 1, 0 | **[guess]** a mic toggle: A.I. noise reduction or noise gate |
| 640 | `1` | | |
| 650 | `0` | | |

#### `SAF`
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 700 | `0` | ✎ 1, 0 | **[guess]** mic monitoring on/off |
| 710 | `0`, `100` | ✎ 100, 0, 100 | **[guess]** mic monitoring volume |
| 720 | `0`, `1` | ✎ 1, 0 | Superhuman Hearing on/off: the power button's short press toggles it **[seen]**; selecting an EQ preset pushes `720 = 0` **[seen]** |
| 730 | `0` | ✎ 1, 2, 0 | Superhuman Hearing level (3 levels) **[guess, strong]** |
| 740 | `60`, `100` | ✎ 0, 100 | Superhuman Hearing volume **[guess, strong]** |
| 750 | `0` | ✎ 1, 0 | active noise cancellation on/off: the ANC button toggles it, `1`/`0`/`1` for three presses **[seen]**. Each press only beeps; there are no voice prompts |
| 760 | `100` | ✎ 0, 100 | **[guess]** active noise cancellation level |

Swarm II's hotkey labels name these pairs: "SUPERHUMAN HEARING ON/OFF / VOLUME", "ACTIVE NOISE CANCELLATION ON/OFF /
LEVEL", "MIC MONITORING ON/OFF / VOLUME", "NOISE GATE ON/OFF / VOLUME", "A.I. NOISE REDUCTION".

#### `Enc`: encoder / wheels **[guess]**
`a00 = 1`, `a10 = 2`, `a20 = 1`, `a30 = 50`.

#### `Btn`: button mapping **[guess]**
`b00`, `b10`, `b20` = `0`.

#### `AQG`: game audio EQ
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 1200 | `0` | | |
| 1210 | `1` | ✎ 2, 3, 4, 1 | preset: `0` custom, `1` Signature Sound, `2` Bass Boost, `3` Bass & Treble Boost, `4` Vocal Boost **[seen]** |
| 1220–12b0 | `0` | ✎ −90…90 | the 10 bands, in tenths of a dB **[guess]**; values snap to steps of 5 (−86 → −85, 2 → 0, −1 → 0). Writing a band sets `1210` to `0` and `12c0` to `""` **[seen]** |
| 12c0 | `Signature Sound` | | preset name |

Preset band values pushed by the device (bands 1–10; omitted = 0):
- Bass Boost: `50 50 35`
- Bass & Treble Boost: bands 7–10 `30 50 50 50` (bands 1–3 kept from Bass Boost in the capture's sequence; to check)
- Vocal Boost: `0 0 20 35 35 35 20 0 0 0`

#### `AQM`: mic EQ
Same layout as `AQG` with keys 1300–13c0: `1310` preset, `1320`–`13b0` bands, `13c0` name.

#### `CEC`, `CG1`..`CG5`, `CM1`..`CM5`: custom EQ slots
`CEC`: `1600 = 0, 1610 = "", 1620 = 0, 1630 = "", 1640 = 0, 1650 = ""` (**[guess]** active custom presets and their
names). `CG*` (game) and `CM*` (mic): `{"name":"","bands":[10 values]}`. Saving a custom EQ has not been captured.

## 5. Open questions

0. Does the left wheel really send Consumer Volume Up/Down? If so, KDE changes the *default* sink's volume, which may
   not be the headset, while the headset changes its own volume (`2a0`) as well. The Linux app may need to keep
   the two in step.

1. Confirm the meaning of the `SAF` and `Mic` keys by listening (mic monitoring, ANC, noise gate, AI noise reduction).
2. Not captured yet: ANC/Transparency mode, button mapping (`Btn`), wheel settings (`Enc`), renaming (`220`), mic EQ
   presets (`AQM`), saving custom EQs (`CG*`/`CM*`/`CEC`), voice prompts, auto-off.
3. How are writes longer than one report sent?
4. What do the unsolicited `2c80` / `2cb1` / `3103` / `3110` packets mean?
5. Does `target = 80` reach the headset directly?

## Sources

- Our own Swarm II 1.0.0.59 capture (device module 1.0.0.4, firmware 4.107.703.0): `captures/windows/` (not committed).

- HaloBattery `providers/turtlebeach.py` and `docs/protocols.md` (MIT): the Swarm II capture frames and the GSI
  keys 220/240/250. https://github.com/HeyOkay/HaloBattery/pull/194
- race-toolkit `librace/`: the RACE header and type values. https://github.com/auracast-research/race-toolkit
- HeadsetControl `lib/devices/audeze_maxwell.hpp`: the same transport on the Audeze Maxwell.
  https://github.com/Sapd/HeadsetControl
