# P1. Paths, Mapped Mount, Media Storage preferences

Questions from section 12 of the plan:

1. Can the Banc volume be created as section 4 plans: APFS volume `/Volumes/Banc` on Mac, persistent `B:` letter on Windows?
2. Is a clip imported on the Mac into a project online on the PC thanks to the Mapped Mount, without relinking by hand? And the other way round?
3. In which file does Resolve store Media Storage and the Mapped Mount, in what format, and can Banc write to it?

You need a Mac and a Windows PC with the same version of Resolve Studio, and a USB stick or a network share to move files. Allow half a day.

The scripts are in `docs/prototypes/scripts/`. Read its `README.md` first (Python, "External scripting" setting). The commands below are run from that folder.

## 1. The volume on Mac

```bash
diskutil apfs list
```

Find the APFS container of the internal disk: the one holding the `Macintosh HD - Data` volume, often `disk3` on Apple Silicon.

```bash
sudo diskutil apfs addVolume disk3 APFS Banc
```

Observe and record:

- [ ] The volume is mounted on `/Volumes/Banc` without any other command.
- [ ] `touch /Volumes/Banc/test` works without `sudo`, with the editor's account.
- [ ] `diskutil info /Volumes/Banc`: record the "Owners" and "Mount Point" lines.
- [ ] After a restart, the volume is mounted again at the same place before Resolve opens.
- [ ] The free space shown is that of the internal disk (the volume shares the container).
- [ ] Does the volume appear on the desktop and in the Finder? Does Spotlight index it (`mdutil -s /Volumes/Banc`)?

## 2. The `B:` letter on Windows

In PowerShell run as administrator. Replace `D:\Banc` with `C:\Banc` if there is only one disk.

```powershell
New-Item -ItemType Directory -Force D:\Banc
```

```powershell
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\DOS Devices" /v B: /t REG_SZ /d "\??\D:\Banc" /f
```

Restart the PC.

Observe and record:

- [ ] `B:` appears in Explorer, for the editor's account.
- [ ] The editor, without administrator rights, can create a file in `B:\`.
- [ ] The free space shown for `B:` is that of `D:`.
- [ ] A system service also sees `B:` (the Banc agent will be a service). Test, as administrator:

  ```powershell
  schtasks /create /tn banc-p1 /tr "cmd /c dir B:\ > C:\banc-p1.txt" /sc once /st 23:59 /ru SYSTEM /f
  ```

  ```powershell
  schtasks /run /tn banc-p1
  ```

  Read `C:\banc-p1.txt`, then delete the task: `schtasks /delete /tn banc-p1 /f`.

## 3. Finding the preferences file

On each workstation, in order. Close Resolve before each snapshot: it often writes its preferences when it quits.

```bash
python3 preferences.py snapshot 0-start
```

1. Open Resolve. Preferences > System > Media Storage: add `/Volumes/Banc` (Mac) or `B:\` (PC). Save, quit Resolve.

   ```bash
   python3 preferences.py snapshot 1-volume
   ```

2. Open Resolve. In the same list, on the Banc volume's row, fill in the Mapped Mount column with the other OS's path: `B:\` on the Mac, `/Volumes/Banc` on the PC. Save, quit.

   ```bash
   python3 preferences.py snapshot 2-mapped
   ```

3. Compare:

   ```bash
   python3 preferences.py diff 0-start 1-volume
   ```

   ```bash
   python3 preferences.py diff 1-volume 2-mapped
   ```

Snapshots are written to `docs/prototypes/p1/snapshots/`, which git ignores: they may contain project database credentials. Copy only the useful excerpt into the answer.

Record:

- [ ] The exact path of the modified file on each OS, and whether it is per user or shared by the workstation.
- [ ] Its format (XML, text, binary) and the excerpt that describes the volume and its Mapped Mount.
- [ ] Other files modified at the same time.
- [ ] The form the interface accepts: `B:`, `B:\` or `B:/`; `/Volumes/Banc` with or without a trailing `/`.

## 4. Can Banc write this file?

1. Open Resolve, delete the Banc volume's row in Media Storage, save, quit.
2. Rewrite the file from the snapshot that contains the Mapped Mount, with Resolve closed. The script refuses if Resolve is running and keeps a `.banc-<date>` copy next to the file:

   ```bash
   python3 preferences.py restore 2-mapped "<file path, copied from the diff output>"
   ```

   On Windows, if the file is under `C:\ProgramData`, run the command as administrator and record that it was needed.
3. Open Resolve. Are the volume's row and its Mapped Mount back?
4. If the file is text: with Resolve closed, change the Mapped Mount by hand in an editor (for example `B:\` becomes `C:\`), reopen Resolve and check that it shows the new value. Then put the right value back.

Record:

- [ ] Does Resolve read the rewritten file without reporting anything?
- [ ] Does the file carry a checksum, a version or a signature that would prevent a modification?
- [ ] Are administrator rights needed to write it?

## 5. A clip imported on the Mac, opened on the PC

1. On the Mac, make two test clips, or copy two real rushes to the same place:

   ```bash
   python3 test_media.py /Volumes/Banc/P1/Rushes/D01/CamA --count 2 --no-proxies
   ```

2. In Resolve on the Mac, create the `P1` project in the local library, import these two clips, put them in a timeline. Then:

   ```bash
   python3 inventory.py --output ../p1/mac.json
   ```

3. File > Export Project: `P1.drp`.
4. Copy the `/Volumes/Banc/P1` folder to `B:\P1` on the PC, with the same tree, and copy `P1.drp`.
5. On the PC, import `P1.drp` into the local library and open it. Do not relink anything by hand.

   ```bash
   python inventory.py --output ../p1/pc.json
   ```

If a shared library (PostgreSQL or Project Server) already exists between the two workstations, repeat the test with it instead of the `.drp`, and record whether the result differs.

Observe and record:

- [ ] On the PC, are the clips online (no red thumbnail), and does the timeline play?
- [ ] In `pc.json`, the clips' `File Path` field: `/Volumes/Banc/...` or `B:\...`? This is the path consolidation will read (P5). `file_present_here` is `true` only if the API returns the translated path.
- [ ] Do the clips stay online after quitting Resolve and reopening the project on the PC?
- [ ] Without the Mapped Mount (remove it, reopen the project), do the clips go offline? This proves that it is the Mapped Mount doing the work.

## 6. The other way round

On the PC, import a third clip placed in `B:\P1\Rushes\D01\CamA`, add it to the timeline, export the `.drp`, copy it with the clip to the Mac, open it.

- [ ] Is the clip added on the PC online on the Mac?
- [ ] `inventory.py` on the Mac: which `File Path` for this clip?

## Where to record the answer

In `docs/prototypes.md`, section "P1. Paths", using the answer template in that file: date, Resolve version, macOS and Windows versions, answer to each question, and what it changes in the plan.

Add to `docs/prototypes/p1/`: `mac.json`, `pc.json`, the excerpt of the preferences file, and the useful screenshots (Media Storage, clips online or offline).

Update section 4 of `docs/PLAN.md` if an answer contradicts an assumption: volume creation command, registry value, Mapped Mount form, file to write.
