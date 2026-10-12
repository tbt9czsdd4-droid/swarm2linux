# Windows harvest session

Goal: in one Windows session, collect (a) the Swarm II program files, which may contain the device protocol
definitions, and (b) USB captures of Swarm II changing every setting. Plan for 1–2 hours.

**Do not install a firmware update during this session.** If Swarm II offers one, decline. The firmware version
on Linux before the session was `4.107.703.0`.

## 0. Before rebooting into Windows

Choose where the results go:

- **Easiest:** save everything to `C:\swarm-harvest\` and copy it from Linux afterwards. This needs a clean
  shutdown of Windows: in step 5, use "Shut down" with Shift held, or run `shutdown /s /t 0`. Otherwise Fast
  Startup leaves the NTFS partition hibernated and Linux won't mount it read-write.
- **Or:** use a USB stick formatted exFAT.

## 1. Set up the tools (once)

1. Install Wireshark from wireshark.org. In the installer, **tick "Install USBPcap"**, then reboot Windows.
2. **Fully exit Swarm II.** Closing the window isn't enough: right-click its tray icon → Exit. Also check Task
   Manager. While it runs it holds the headset's channel, and we want every capture to include its startup.
3. Create the folder `C:\swarm-harvest\captures\`.

## 2. Copy the program files

Open PowerShell. This finds every Turtle Beach, Swarm or ROCCAT folder and writes the list to a file:

```powershell
$roots = 'C:\Program Files','C:\Program Files (x86)','C:\ProgramData',$env:APPDATA,$env:LOCALAPPDATA
Get-ChildItem -Path $roots -Directory -Recurse -Depth 3 -ErrorAction SilentlyContinue |
  Where-Object Name -match 'Turtle|Swarm|Roccat' | Select-Object -ExpandProperty FullName |
  Tee-Object C:\swarm-harvest\folders.txt
Get-Service | Where-Object DisplayName -match 'Turtle|Swarm|Roccat' |
  Format-List Name,DisplayName,Status,BinaryPathName | Out-File C:\swarm-harvest\services.txt
```

Copy each folder from the list into `C:\swarm-harvest\files\`, keeping the folder structure:

```powershell
Get-Content C:\swarm-harvest\folders.txt | ForEach-Object {
  $dest = 'C:\swarm-harvest\files\' + ($_ -replace ':','')
  robocopy $_ $dest /E /R:0 /W:0 /NFL /NDL | Out-Null
}
```

If Swarm II is a Microsoft Store app, its files live under `C:\Program Files\WindowsApps\…`, which is locked. In
that case, run `Get-AppxPackage *Turtle* | Format-List Name,InstallLocation` and tell me what it prints.

## 3. Captures

### How to capture

1. Start Wireshark. The interface list shows `USBPcap1`, `USBPcap2`, and so on. Click the small arrow or gear
   next to each one to see the devices attached to it, and **use the one that lists "Stealth Pro II PC"**.
2. Start capturing on it, do the actions for one capture below, stop, and use **File → Save As**
   `C:\swarm-harvest\captures\NN-name.pcapng`.
3. **Wait about 5 seconds between actions.** The pauses let me separate the actions in the capture.
4. In `C:\swarm-harvest\notes.txt`, write down each capture's actions **in the order you did them, with the
   exact values** shown in the app, e.g. `03: SHH on, level 2, level 3, intensity 0, intensity 100, off`.
5. Take a **screenshot of every Swarm II page** (Win+Shift+S) and save it to `C:\swarm-harvest\screens\`. These
   give the value ranges and labels for the Linux GUI.

Keep the headset on and connected through the USB-A dongle throughout, as on Linux.

### Capture list

| # | file | actions |
|---|------|---------|
| 01 | `01-launch` | Start capturing **before** launching Swarm II. Launch it, wait until the headset is shown, then click through every headset page/tab once **without changing anything**. Then exit Swarm II from the tray. |
| 02 | `02-eq-presets` | Launch Swarm II, then start capturing. Select each built-in EQ preset in turn, then back to the original one. |
| 03 | `03-eq-custom` | Custom EQ: lowest band to minimum, then maximum, then 0. Same for a middle band and the highest band. Save as a custom preset named `TEST`, select another preset, select `TEST` again, then delete `TEST`. |
| 04 | `04-shh` | Superhuman Hearing: on, each level, intensity at minimum, intensity at maximum, off. |
| 05 | `05-anc` | Noise control: each mode (ANC, Transparency, Off), ANC strength at min and max, transparency level at min and max if it exists. Create a custom ANC profile, rename it, delete it. |
| 06 | `06-mic` | Each mic setting at its minimum and maximum, in page order: sensitivity/volume, noise gate, mic EQ presets (each one), AI noise reduction off then on, mic monitoring/sidetone min, max and off. Any other mic toggles: off and on. |
| 07 | `07-controls` | Button/dial mapping: assign each available function to the remappable button(s), one at a time, then restore the original. |
| 08 | `08-device` | Rename the headset to `TEST1`, then back to `My Headset`. Voice prompts off/on if possible. Each auto-shutoff option. Any other device settings: change, then restore. |
| 09 | `09-connectivity` | Open the transmitter / CrossPlay / Bluetooth pages and toggle anything there that won't drop the connection. |
| 10 | `10-physical` | With Swarm II open: press the ANC button twice, Superhuman Hearing (power button) on and off, flip the mic up and down, roll the left (PC) wheel and the right (Bluetooth) wheel. This shows how the app learns about changes made on the headset. |
| 11 | `11-profiles` | Anything else Swarm II offers for the headset: profiles, Dolby/spatial toggle, hotkeys. Change, then restore. |
| 12 | `12-misc` | Any setting not covered above. |

## 4. Optional extras

- Swarm II log files. Look in the copied folders for `*.log`; they may contain protocol traces.
- `Get-PnpDevice -PresentOnly | Where-Object InstanceId -match 'VID_10F5' | Format-List FriendlyName,InstanceId`
  saved to `C:\swarm-harvest\pnp.txt`.

## 5. Back to Linux

Do a full shutdown (Shift + Shut down). Boot Linux and tell me where the files are, for example the Windows
partition or the USB stick. I'll copy them:

- program files → `harvest/` (gitignored; never committed)
- captures → `captures/windows/`
- notes and screenshots alongside them
