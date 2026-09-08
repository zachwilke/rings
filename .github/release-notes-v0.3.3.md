This release walks the disk on every core, and `/` finds the hog without hunting through the sunburst.

**Faster scans.** Directories are walked in parallel — one worker per CPU, default cap 32. Fewer stats per file (`DirEntry` metadata instead of a second lstat), and on macOS `getattrlistbulk` pulls a batch of names and sizes in one syscall. `RINGS_SCAN_THREADS=1` keeps the original single-thread walk. Same skip rules, same hardlink dedupe, same junction safety.

**Find the hog.** After a scan, `/` opens a scan-wide fuzzy finder. Type a name, an extension (`.mp4`, `.iso`), or a waste category (`cache`, `node_modules`). Live results show size and path, ranked so the space hogs bubble up. An empty query lists the largest items under the current scope. Enter jumps the sunburst there; Space marks for the collector; Tab toggles whole-scan vs the directory you drilled into. Capped at 200 hits.

**Still tiny.** One static binary, `std` + `libc` only — zero extra crates. The finder and the parallel walker add about 20 KB.

```bash
# Linux x86_64
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.3.3/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
```
