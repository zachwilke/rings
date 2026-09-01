A directory picker, right-click menus, themes, and a mouse that behaves — the first half of the "feels alive" work.

```bash
# Linux x86_64 (Debian, Ubuntu, RHEL/Fedora, Arch — static musl)
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-x86_64-linux-musl.xz | xz -d > rings
# 64-bit Raspberry Pi OS / aarch64 Linux
# …/rings-aarch64-linux-musl.xz
# 32-bit Raspberry Pi OS
# …/rings-armv7-linux-musleabihf.xz
# Pi 1 / Zero
# …/rings-arm-linux-musleabihf.xz
chmod +x rings && sudo ./rings /
```

**Start anywhere.** `rings` with no PATH opens a vim-style directory picker in the current directory: `j`/`k` to move, `Enter` to open, `h` to go up, `s` to scan. A PATH argument still scans straight away, and piped or scripted runs are unchanged. From a finished scan, `-` reopens the picker where you are browsing; `Esc` drops back into the scan you left.

**Right-click anything.** Slices, list rows, findings, collector entries, and picker rows all open a context menu: open, mark for delete, or delete this file or directory. Delete marks the target and opens the usual confirm modal — the typed-`DELETE` rule, the trash fallback, and the path safeguards all still apply, and nothing unlinks on the click.

**Themes and honest color.** `--theme nord` (also `gruvbox`, `dracula`, `solarized-dark`, `mono`; default `rings`). Color depth follows the terminal — 24-bit where `COLORTERM`/`TERM_PROGRAM`/`WT_SESSION` say so, 256 for other xterm-alikes, 16 for the Linux console with hue-preserving quantization. `NO_COLOR` drops to bold and reverse video; `RINGS_COLORS=16|256|truecolor` overrides.

**The mouse, finished.** The wheel scrolls every list. Hover highlights rows, chips, breadcrumbs, and slices; hovering a slice shows its path, size, and share of its parent in the footer.

**A keys page worth reading.** `?` / `F1` shows five titled groups in two columns — Navigate, Views, Delete, Picker, Mouse — with the logo above when the terminal has room and every binding still visible on 80×24. `rings help` prints the same table.

**Delete, clearer.** Marking now says what happens next (`marked foo (1.2 GB) · 3 in collector · c review · x delete`), the context menu says when Delete would take other staged items with it, and the picker warns before a new scan drops your marks.

**Fixes.** The delete log no longer echoes to stderr and paints over the sunburst. The confirm modal swallows clicks that miss its buttons instead of passing them to the ring behind it.

Still one static binary with no dependencies: 506 KB on x86_64 Linux.
