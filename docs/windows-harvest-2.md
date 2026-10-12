# Windows harvest, round 2: the remaining settings

The first capture decoded everything it covered: EQ presets and bands, Superhuman Hearing, ANC, mic monitoring and
three mic settings. This round covers only what is still unknown. About 20 minutes.

Same setup as last time (Wireshark with USBPcap, headset on the USB-A dongle). One capture for everything is fine.
**Wait about 5 seconds between actions**, and write the order into `C:\swarm-harvest\notes2.txt`, for example
`rename to TEST1, rename back, mic EQ preset 2, ...`. No firmware updates.

## 0. Copy the device module

Swarm II downloads the Stealth Pro II module separately, and it wasn't installed yet last time. Copy this folder to
`C:\swarm-harvest\files2\`:

```
C:\Program Files\Turtle Beach Swarm II\Data\Devices\STEALTH_PRO_II
```

If it doesn't exist, run `Get-ChildItem 'C:\Program Files\Turtle Beach Swarm II\Data\Devices'` and note what is
there.

## 1. Capture: `capture2.pcapng`

Start capturing **before** launching Swarm II. Then, in this order, skipping anything that doesn't exist:

1. **A.I. noise reduction**: note whether it's on or off when the page opens, then toggle it once and toggle it back.
   This tells me which value means "on".
2. **Mic EQ presets**: select each one in turn, then the original again.
3. **Custom game EQ**: change one band, then **save it as a custom preset** named `TEST`. Select a built-in preset,
   select `TEST` again, then **delete** `TEST`.
4. **Custom mic EQ**: the same as step 3, if Swarm II offers it.
5. **Rename the headset** to `TEST1`, then back to `My Headset`.
6. **Button / dial mapping**: assign each available function to the remappable button(s) or wheel, one at a time,
   then restore the original.
7. **Everything else on the device and settings pages**: voice prompts, auto-off, sleep timer, spatial/3D audio,
   Bluetooth settings, LED, etc. Change each once and restore it, noting its name.
8. Exit Swarm II from the tray, then stop the capture.

## 2. Back on Linux

Put everything under `captures/swarm-harvest/` as before and tell me.
