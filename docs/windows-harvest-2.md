# Windows harvest, round 2: the remaining settings

The first capture decoded everything it covered: EQ presets and bands, Superhuman Hearing, ANC, mic monitoring and
three mic settings. This round covers what is still unknown. About 30 minutes.

Same setup as last time: Wireshark with USBPcap, headset on the USB-A dongle, one capture for everything.
**Wait about 5 seconds between actions.** No firmware updates.

What to bring back, all in `C:\swarm-harvest\round2\`:

| what | why |
|------|-----|
| `capture2.pcapng` | the writes for the settings below |
| `notes2.txt` | the order of your actions, with the values or option names you picked |
| `screens\` | a screenshot of **every** Swarm II page and every dropdown you open (Win+Shift+S) |
| `STEALTH_PRO_II\` | the device module folder (step 0) |

The screenshots matter as much as the capture: there are about 20 values the headset reports that I can't name
yet, and the pages show them with their labels.

## 0. Copy the device module

Before starting Swarm II, copy this folder to `C:\swarm-harvest\round2\STEALTH_PRO_II\`:

```
C:\Program Files\Turtle Beach Swarm II\Data\Devices\STEALTH_PRO_II
```

If it doesn't exist, run `Get-ChildItem 'C:\Program Files\Turtle Beach Swarm II\Data\Devices'` and put the output
into `notes2.txt`.

## 1. Capture

Start capturing **before** launching Swarm II. Then, in this order, skipping anything that doesn't exist. Take the
screenshots as you reach each page, before changing anything.

1. **Home / overview page**: screenshot only. Note every number shown (battery, a second battery, volumes, signal).
2. **A.I. noise reduction**: note whether it's on or off when the page opens, toggle it, toggle it back.
3. **Mic EQ presets**: select each one in turn, then the original again. Note their names.
4. **Custom game EQ**: change one band, then **save it as a custom preset** named `TEST`. Select a built-in preset,
   select `TEST` again, then **delete** `TEST`.
5. **Custom mic EQ**: the same as step 4, if Swarm II offers it.
6. **Rename the headset** to `TEST1`, then back to `My Headset`.
7. **Button and wheel mapping**: assign each available function to each remappable button or wheel, one at a time,
   then restore the original. Note the function names.
8. **CrossPlay / transmitter page**: change any setting per transmitter (volume, name, priority, ...) and restore it.
9. **Bluetooth page**: change any setting and restore it.
10. **Every remaining setting** on the device and settings pages: voice prompts, auto-off/sleep timer, spatial/3D
    audio, prompts volume, LED, etc. Change each once and restore it, noting its name.
11. Exit Swarm II from the tray, then stop the capture.

## 2. Back on Linux

Put `round2\` under `captures/swarm-harvest/` and tell me.

## Linux-only extras (no Windows needed, 2 minutes)

Run `./target/release/stealthctl watch` and, with about 10 seconds between steps:

1. Detach the mic boom, then reattach it.
2. Put the spare battery in the dock (or take it out), and plug or unplug the dock if it has its own cable.

Also tell me what the second laptop's transmitter (`10f5:2288`) physically is: the charging dock, or a second USB-A
dongle.
