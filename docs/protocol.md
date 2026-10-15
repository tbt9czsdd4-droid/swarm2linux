# Stealth Pro II control protocol

Living spec. Every statement is tagged with where it comes from:

- **[seen]**: observed on our own hardware (`10f5:2287`, firmware `4.107.703.0`)
- **[capture]**: from a USB capture of Turtle Beach's Swarm II
- **[user]**: from the user's notes on what they changed in Swarm II
- **[ui]**: from screenshots of Swarm II
- **[dll]**: from setting names in Swarm II's Stealth Pro II device module (`STEALTH_PRO_II.dll` 1.0.0.4)
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
| `2cb1` | 5D   | `00 02 01 01 93 34 92 ed ca c7 80 01`, `00 02 00 01 93 34 92 ed ca c7 ff 00` | wireless link to the headset: byte 2 `01` up / `00` down, together with the headset's address `c7:ca:ed:92:34:93` (little-endian) **[seen]** on battery pull and reinsertion and on power-off/on. The transmitter sends no record push when the headset goes away; this indication is the only sign |
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

- Response `op'` is `45` for a reply carrying a document, `43` for the ack of a `set_kvp`, and `80` when the device
  refuses a write (seen for selecting an empty custom preset slot). A document reply's
  body is `61 <x> 62 32 <y> 21 01 ff` followed by the JSON, or `c1 32 21 01 ff` followed by the array for `index`.
- After a `set_kvp` ack the device pushes the changed keys as an `UP` document, including keys that changed as a
  consequence (a preset change pushes its band values) **[seen]**.
- **Subscriptions live in the headset.** After the headset restarts (power off/on), no push arrives any more, not
  even the periodic `BT` one, until the host subscribes again. Subscribing again ~1.5 s after the `2cb1` link-up
  indication restores them **[seen]**.
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
| 280 | `4` | auto shut-off (`Power_Options`): `0` never, `1` 5, `2` 10, `3` 20, `4` 30 minutes. Swarm II wrote 0…4 while the user picked None…30 minutes **[capture, user]** |
| 290 | `3` | power state: `3` on battery, `5` charging over USB **[seen]** |
| 2a0 | `42` | `Master_Volume_Level` **[dll, ui]**, also moved by the left wheel: 2.4 GHz volume `0`–`100`, uneven steps (0, 1, 3, 6, 8, 12, 16, 21, 26, 30, 36, 42, 48, 54, 62, 71, 76, 87, 93, 100). The wheel also makes KDE show its volume pop-up **[seen]** |
| 2b0 | `60` | |
| 2c0 | `50` | |
| 2d0 | `78` | |
| 2e0 | `0` | wake on motion, `0`/`1` **[capture, user]** | |

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
- `control`: `[voice prompt, LED 1, LED 2]`, `["0","100","100"]` for both live transmitters, all `0` for an empty
  slot. Swarm II's Charging Hub section writes single elements with a key one past the slot's key: `0x401` = LED 1,
  `0x402` = LED 2 (0–100) **[capture, ui]**, so `0x400` is presumably the voice prompt (its dropdown offers only
  "None"). TX1 (`10f5:2287`) is therefore the charging hub's built-in transmitter. The device module names these
  `Charging_Hub_Voice_Prompt` and `Charging_Hub_Led_Brightness_1/_2` **[dll]**.

Seen: `TX1` = `10f5:2287` `C5:43:B0:09:56:F3` (this PC), `TX2` = `10f5:2288` `CB:AF:2C:41:F2:F3` (a second
laptop's transmitter). `2288` is the third product ID listed in Swarm II's `Update.ini`.

#### `3DT`
| key | seen | meaning |
|-----|------|---------|
| 500–530 | `0` | **[guess]** 3D / spatial audio |

#### `Mic`

Swarm II's microphone panel, top to bottom: Mute microphone, Sensitivity, Monitoring (slider), Noise gate (toggle and
slider), AI noise reduction **[ui]**. The device module's setting names: `Mute`, `Sensitivity_Level`,
`Monitoring_Enable`, `Monitoring_Level`, `Noise_Gate_Enable`, `Noise_Gate_Level`, `AI_Noise_Reduction` **[dll]**.
Matched with the order of the round-1 writes:

| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 600 | `0` | | mute: flip-to-mute up `1`, down `0` **[seen]** |
| 610 | `51`, `84` | ✎ 100, 0, 84 | sensitivity **[ui]**. The device pushed the new value *before* Swarm II's write, so it probably follows the Windows mic level (UAC feature unit 6) **[guess]** |
| 620 | `70`, `85`, `65` | ✎ 0, 100, 85 | monitoring level (hearing your own voice): writing `0` from Linux silenced it, `65` brought it back **[seen]** |
| 630 | `0` | ✎ 1, 0 | AI noise reduction, `1` = on: the page showed "Off" with `0`, then Swarm II wrote `1` and `0` for on and off **[ui, capture]** |
| 640 | `1` | | **[guess]** monitoring enable: Swarm II only offers it as a hotkey, never wrote it |
| 650 | `0` | | |

#### `SAF`

| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 700 | `0` | ✎ 1, 0 | noise gate on/off **[ui, user]**. Round 1 labelled this "mic monitoring" because the user heard their own voice after `700 = 1` was written; that test had no off/on comparison and monitoring (`620`) was on all along |
| 710 | `0`, `100` | ✎ 100, 0, 100 | noise gate level **[ui, user]** |
| 720 | `0`, `1` | ✎ 1, 0 | Superhuman Hearing on/off: the power button's short press toggles it **[seen]**; selecting an EQ preset pushes `720 = 0` **[seen]** |
| 730 | `0` | ✎ 1, 2, 0 | Superhuman Hearing mode (`SHH_Preset`): `0` Legacy, `1` Footsteps, `2` Gunshots **[ui, dll]** |
| 740 | `60`, `100` | ✎ 0, 100 | Superhuman Hearing level (`SHH_Level`), slider 0–100 **[ui, dll]** |
| 750 | `0` | ✎ 1, 0 | active noise cancellation on/off: the ANC button toggles it, `1`/`0`/`1` for three presses **[seen]**. Each press only beeps; there are no voice prompts |
| 760 | `100` | ✎ 0, 100 | ANC level: the slider runs from `AMB` (ambient, outside sound let in) at `0` to `ANC` at `100`, with a centre mark **[ui]**. So "transparency" is the low end of this slider, not a separate mode |

#### `Enc`: wheels
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| a00 | `1` | | |
| a10 | `2` | | |
| a20 | `1` | ✎ 2, 3, 4, 5, 1 | function of the second wheel on the left earcup (the other one is the volume wheel, `2a0`; the right earcup's wheel is Bluetooth volume) **[ui, user, capture]**: `1` mic monitoring volume (default), `2` game and chat mix, `3` bass boost level, `4` treble boost level, `5` noise gate volume |
| a30 | `50` | | **[guess]** game/chat mix position |

#### `Btn`: buttons
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| b00 | `0` | | |
| b10 | `0` | | |
| b20 | `0` | ✎ 1, 2, 0 | function of the top button on the left earcup **[ui, capture]**: `0` active noise cancellation on/off (default), `1` cycle game presets, `2` noise gate on/off |

#### `AQG`: game audio EQ
| key | seen | ✎ written | meaning |
|-----|------|-----------|---------|
| 1200 | `0` | | |
| 1210 | `1` | ✎ 1–4, 16 | preset: `0` unsaved custom bands, `1` Signature Sound, `2` Bass Boost, `3` Bass & Treble Boost, `4` Vocal Boost **[seen]**, `16`–`20` the custom presets in slots `CG1`–`CG5` **[seen]** for 16 and 17 |
| 1220–12b0 | `0` | ✎ −90…90 | the 10 bands in tenths of a dB, at 32, 63, 125, 250, 500 Hz, 1, 2, 4, 8, 16 kHz, ±9 dB **[ui]**. Values snap to steps of 5 (−86 → −85, 64 → 65, 2 → 0). Writing a band sets `1210` to `0` and `12c0` to `""` **[seen]** |
| 12c0 | `Signature Sound` | ✎ `TEST` | preset name; writing it saves the current bands as a custom preset (below) |

Built-in preset bands (bands 1–10, tenths of a dB). A push lists only the bands that changed, so these follow from
applying the pushes in the order the presets were selected (Signature → Bass → Bass & Treble → Vocal) **[seen]**:

| `1210` | name | bands 1–10 |
|--------|------|------------|
| 1 | Signature Sound | all 0 |
| 2 | Bass Boost | `50 50 35 0 0 0 0 0 0 0` |
| 3 | Bass & Treble Boost | `50 50 35 0 0 0 30 50 50 50` |
| 4 | Vocal Boost | `0 0 20 35 35 35 20 0 0 0` |

#### `AQM`: mic EQ
Same layout as `AQG` with keys 1300–13c0. Bands at 100, 160, 250, 400, 630 Hz, 1, 1.6, 2.5, 4, 6.3 kHz **[ui]**.
Presets **[seen]**:

| `1310` | name | bands 1–10 (tenths of a dB) |
|--------|------|-----------------------------|
| 1 | Signature Sound | all 0 |
| 2 | Full | `-30 50 50 30 0 -11 0 0 0 0` |
| 3 | Clarity | `-30 -30 -20 0 0 0 30 40 40 0` |
| 4 | Smooth | `0 0 0 0 -20 -20 0 20 20 0` |

Derived the same way, from pushes in the order 1 → 2 → 3 → 4 (seen twice, identical both times).

#### Custom presets: `CEC`, `CG1`..`CG5`, `CM1`..`CM5`

- `CG1`..`CG5` (keys 1700, 1720, …, 1780) and `CM1`..`CM5` (1800, …, 1880) hold `{"name":"","bands":[10 values]}`;
  an empty name is a free slot.
- `CEC.1600` counts the saved game presets, `CEC.1620` the mic presets **[seen]**. `1610` and `1630` hold the name of
  the last deleted game and mic preset; `1640`/`1650` stay `0`/`""` **[seen]**.
- **Save** (Swarm II's sequence, reproduced from Linux) **[capture, seen]**:
  1. select the first free slot's preset number (`1210 = 16`). The device refuses with reply op `80`, because the
     slot is empty.
  2. write all 10 bands (`1220`…`12b0`).
  3. write the name (`12c0 = "TEST"`). The device stores the bands under that name in the first free slot, pushes
     the slot record, raises the count in `CEC` and selects the preset (`1210 = 16`).

  A second preset went to `CG2` as preset `17` **[seen]**.
- **Delete**: write the preset's name to `1610` (game) or `1630` (mic). The device empties the slot and lowers the
  count **[capture, seen]**.
- Select a saved preset like a built-in one: `1210 = 16`…`20`.

## 5. Open questions

0. Does the left wheel really send Consumer Volume Up/Down? If so, KDE changes the *default* sink's volume, which may
   not be the headset, while the headset changes its own volume (`2a0`) as well.
1. Not captured: the "Audio master volume" toggle (`Master_Volume_Enable`, also the "volume mute" hotkey) and
   monitoring enable (`640`?). Both duplicate other controls (system mute, monitoring level 0).
2. Unknown keys: `GSI` 200, 210, 260, 270, 2b0, 2c0, 2d0; `Mic` 650; `Enc` a00, a10, a30; `Btn` b00, b10;
   `BT` 300–350; `3DT` 500–530
   (the device module also has settings for Waves 3D, chat boost and game/chat mix, which this headset's pages do not
   show).
3. How are writes longer than one report sent? (None seen so far; preset names keep requests short.)
4. Does `target = 80` reach the headset directly?

## Sources

- Our own Swarm II 1.0.0.59 captures, rounds 1 to 3 (device module 1.0.0.4, firmware 4.107.703.0), screenshots and
  the device module's strings: `captures/` (not committed).

- HaloBattery `providers/turtlebeach.py` and `docs/protocols.md` (MIT): the Swarm II capture frames and the GSI
  keys 220/240/250. https://github.com/HeyOkay/HaloBattery/pull/194
- race-toolkit `librace/`: the RACE header and type values. https://github.com/auracast-research/race-toolkit
- HeadsetControl `lib/devices/audeze_maxwell.hpp`: the same transport on the Audeze Maxwell.
  https://github.com/Sapd/HeadsetControl
