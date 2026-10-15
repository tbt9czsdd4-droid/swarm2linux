# Windows harvest, round 3: Assignment and device pages

Round 2 decoded the mic EQ, custom presets and AI noise reduction, and the screenshots corrected several labels.
What's left is on two pages: the **Assignment** page (it didn't render in the round-2 screenshot) and the
**device page** (power options, charging hub). About 20 minutes.

Same setup: Wireshark with USBPcap, headset on the transmitter, one capture, **about 5 seconds between actions**,
no firmware updates. Bring back in `C:\swarm-harvest\round3\`:

- `capture3.pcapng`: start it **before** launching Swarm II
- `screens\`: screenshots as described below
- `notes3.txt`: every action in order, with the **exact option names** you picked. This matters most for the
  Assignment page: the capture only shows numbers like `b20 = 2`, and your notes give them their meaning.

## 1. Assignment page

1. Open it and wait until the headset picture and the controls appear; maximise the window if it stays empty.
   Screenshot it.
2. For **each** control that can be assigned (button or wheel): open its options, screenshot the list, then select
   every option one by one, writing down the order. Restore the original option at the end.

## 2. Device page

1. **Auto shut-off**: select each option (None, 5, 10, 20, 30 minutes), then restore 30 minutes.
2. **Wake on motion**: on, then off.
3. **Charging hub voice prompt**: open the dropdown, screenshot it, select each option, restore `None`.
4. **Charging hub LED 1**: slider to 0, then to 100. Then **LED 2** the same way.
5. If there is a transmitter section with its own voice prompt or LED setting: the same as steps 3 and 4.

## 3. Audio page

1. **Audio master volume** toggle (above the volume slider): off, then on.

## 4. Hotkeys

On the device page's hotkey list, assign a key to **MIC MONITORING ON/OFF** and to **VOLUME MUTE ON/OFF**. Press
each hotkey twice, 5 seconds apart. If MIC MONITORING ON/OFF isn't in the list, skip it.

## 5. Finish

Exit Swarm II from the tray, then stop the capture. Put `round3\` somewhere I can reach, as last time, and tell me
the path.
