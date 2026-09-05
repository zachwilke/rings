This release knows the disk it is standing on, and it no longer stops you at the door to ask about updates.

**Each OS, properly.** A scan of `/` on macOS was walking APFS firmlinks *and* `/System/Volumes/Data`, so every user file could be counted twice. rings now follows firmlinks once, skips Preboot/VM/Update and the autofs home map, and tags Xcode DerivedData, Core Simulator, DiagnosticReports, sleepimage, and Spotlight as waste. Windows no longer follows NTFS junctions (`Documents and Settings` cannot loop through `Users`); the Recycle Bin is scanned as temp; update leftovers and crash dumps show up in findings; sizes are on-disk allocation so OneDrive placeholders and NTFS compression stop looking like full files. Linux skips `/snap` and classifies apt lists, snapd cache, and Trash.

**Settings that look like a product.** Press `m` from the picker or a scan. The menu opens on a Delta Corps Priest **RINGS** wordmark, each letter a ring of the current theme, with a glint walking across it.

**Updates inside the TUI.** GitHub is checked in the background after launch. If a newer rings is out, a popup offers **Ctrl+U** to install in place and restart. Esc dismisses. `--offline` and `RINGS_NO_UPDATE=1` still skip the check.

```bash
# Linux x86_64
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.3.2/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
```
