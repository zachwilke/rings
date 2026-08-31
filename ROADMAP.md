# rings roadmap — "btop for storage"

btop is the bar: launch it anywhere, it is instantly beautiful, everything
updates live, every pixel is clickable, and you can *act* on what you see.
rings today is a good disk-usage viewer. This is the path to it being the
tool you reach for on any machine to answer "what is eating my disk, and
what can I safely get rid of?"

## Principles (non-negotiable)

- **One tiny static binary, zero crates.** `std` + `libc` only. Themes,
  config, layout, persistence — all hand-rolled, like btop's own C++.
  ~500 KB, works over SSH on a Pi, no dependency churn.
- **Never delete without an explicit confirm.** Every new "act" feature
  goes through the collector → confirm modal → trash/typed-DELETE path.
- **80×24 over SSH is a first-class target.** Every layout must degrade;
  true-color with 256/16-color fallback; `NO_COLOR` respected.
- **Idle cost near zero.** Scans are threaded; the UI thread never blocks.
- **Verified by rendering.** Every view has buffer-snapshot tests.

## What btop does that rings does not (the gap)

| btop | rings today | needed |
|---|---|---|
| Live, continuously updating boxes | spinner, then a static tree | streaming scan, rescan, refresh |
| Whole-system overview on launch | picker or straight to one scan | mounts dashboard with capacity meters |
| Themes (dozens), config file, options menu | 12 hard-coded colors | theme engine, config, in-app options |
| Hover highlight, scroll wheel, click everything | click + right-click; wheel is decoded and discarded | motion tracking, wheel, hover |
| Process detail pane, tree, filter, sort | one child list, size-sorted only | details pane, `/` filter, sort modes, top files |
| Kill / signal from the UI | mark → confirm → trash | undo, projections, quick-mark rules, open/reveal, clipboard |
| Layout presets, box toggles | one fixed layout | treemap, list-only, dual-pane, responsive collapse |
| Polished distribution (brew, AUR, screenshots) | curl\|sh + GitHub Releases | tap, AUR, scoop, cargo install, demo GIF, man page |

## Phases

Each phase is a shippable release. Order matters: 0.2 lays the foundations
(theme engine, layout primitives, streaming scan, mouse) that every later
phase builds on. Sizes are rough: S = a day, M = a few days, L = a week+.

### 0.2 — Feels alive (foundations + polish)

Goal: the first 10 seconds feel like btop. Something is always moving,
the mouse does what you expect, and it looks deliberate.

- **Streaming scan** (L). Walker publishes partial tree snapshots; the
  sunburst and list fill in as top-level directories complete, DaisyDisk
  style. Esc cancels a scan. `r` rescans the directory in view.
  *Code:* `scan/walk.rs` sends `WalkEvent::Partial(Tree)` every N ms;
  `App` swaps trees while preserving cwd/selection by path.
- **Mouse, finished** (M). Wheel scrolls lists and the picker (SGR
  buttons 64/65 — currently masked out in `term/input.rs`). Motion
  tracking (`?1003h`) for hover highlight on slices, rows, and chips.
  Hover tooltip on a slice: name · size · % of parent.
- **Theme engine** (M). `Theme` struct replaces the consts in
  `tui/theme.rs`; a `--theme` flag; built-ins: `rings` (current), `nord`,
  `gruvbox`, `dracula`, `solarized-dark`, `mono` (for 16-color TTYs).
  Detect true-color (`COLORTERM`), fall back to 256, then 16. `NO_COLOR`.
- **Layout primitives** (M). `Rect::split_h/split_v`, a `Box` widget with
  btop-style rounded corners and titled border, used by every view so
  the picker, findings, collector, and details pane share one chrome.
- **Size bars in the list** (S). Each row gets a meter proportional to
  the parent, plus a `%` column. Theme-gradient fill.
- **Details pane** (M). Wide terminals (≥120 cols) show a third column
  for the selection: full path, used vs apparent, file/dir counts, mtime,
  owner/perms, category, % of parent, % of scan, top 5 children.
  *Requires* capturing `mtime`, `uid`, `mode` in `Node` (cheap: already
  stat'ing every inode).
- **Config file** (S). `~/.config/rings/rings.conf` (`key = value`, no
  parser deps): theme, units (binary/SI), show hidden, default layout,
  exclude globs. `--config` to point elsewhere.
- **Keymap reconciliation** (S). One table drives key handling, the help
  overlay, and the footer hints. Resolve conflicts now (`s` scan vs sort;
  use `o` for order). Add `gg`/`G`, `Ctrl-d`/`Ctrl-u`.

### 0.3 — Overview (the btop launch moment)

Goal: `sudo rings` shows the whole machine's storage at a glance, then
lets you dive in.

- **Mounts dashboard** (L). New start view: every mounted filesystem as a
  btop-style meter — mountpoint, device, fstype, used/free/%, inode use,
  read-only flag. Sorted by fullest. Enter/click on a mount opens the
  picker there; `s` scans it. Virtual/pseudo mounts filtered.
  *Platform:* `getmntinfo` (macOS/BSD), `/proc/self/mounts` + `statvfs`
  (Linux), `GetLogicalDrives` + `GetDiskFreeSpaceEx` (Windows). Add to
  `sys.rs`; it already has the per-OS split.
- **Scan cache** (M). Persist finished scans to `~/.cache/rings/` (root:
  `/var/cache/rings/`, mode 0600) in a compact binary format. Dashboard
  lists recent scans with age; reopening a root is instant, tagged
  *stale*, with `r` to refresh. `--no-cache` and a config key opt out.
- **Top files** (S). `t` lists the largest N files anywhere under the
  current directory, not just direct children.
- **Filter** (M). `/` opens an nvim-style fuzzy filter over the current
  list / findings / top files; Esc clears. Match highlighting.
- **Sort modes** (S). `o` cycles size → name → count → mtime; header
  shows the active sort.
- **Breadcrumb polish** (S). Truncate from the left, `~` for home,
  clickable ancestors already exist — make them hover-highlight.

### 0.4 — Find the waste

Goal: rings knows what is safe to reclaim on a developer machine, a
server, and a Pi, and shows you *why*.

- **Extended detectors** (M). Beyond temp/cache/log: `node_modules`,
  `target/`, `.venv`/`venv`, `__pycache__`, `.gradle`, `.m2`, Xcode
  `DerivedData` + simulators, Homebrew cache, npm/yarn/pnpm/pip/cargo
  registry caches, Docker overlay (`/var/lib/docker`, macOS
  `Docker.raw`), old kernels, apt lists, Trash contents, `*.dmg`/`*.iso`
  downloads, crash reports. Each carries a *why* and a *safe to remove
  when* note shown in the details pane and findings row.
- **Color modes** (M). `v` cycles the sunburst/list coloring: by depth
  (current), by category (waste tags), by file type (video / image /
  archive / code / build / other, via extension tables), by age (mtime
  heatmap: cold = untouched for a year).
- **Age filter** (S). "Not touched in 30d / 6mo / 1y" toggle on the
  findings and top-files views.
- **What grew** (M). Diff the current scan against the cached previous
  one: a *Δ* column, a "grew the most" view, and a dashboard sparkline
  per mount once a few scans exist. This is the "activity" half of the
  activity-manager analogy.
- **Empty dirs, broken symlinks, hard-link groups** (S). Small findings
  sections; hard-link groups explain why `used` ≠ `apparent`.
- **Duplicates** (L, opt-in). `D` hashes files ≥ 1 MiB (size-bucketed
  first, then a fast 64-bit hash of head/tail, then full hash only on
  collision) and lists groups. Never on by default; progress bar.

### 0.5 — Act, safely and quickly

Goal: going from "found it" to "reclaimed it" is one confirmed motion,
and mistakes are recoverable.

- **Undo** (M). Trash moves record source → trash path in the delete
  log; `u` in the collector view restores the last batch. Typed-DELETE
  unlinks stay irreversible and say so.
- **Delete progress** (S). Per-item progress and result in the confirm
  modal for big batches; failures listed with reasons and re-tryable.
- **Projections** (S). Collector header and dashboard meter preview the
  outcome: "after: 61% used, 38 GB free (+12 GB)".
- **Quick-mark rules** (M). From findings: mark all of a category, all
  older than N days, all `node_modules` not touched in 90 days. Each is
  a review-then-confirm, never auto.
- **Clipboard** (S). `y` yanks the selected path via OSC 52 — works over
  SSH. `Y` yanks the collector as a shell-quoted list.
- **Open / reveal** (S). `O` reveals in Finder / file manager
  (`open -R`, `xdg-open`, `explorer /select`), dropping privileges when
  under sudo.
- **Exclude patterns** (S). Config + `--exclude` globs; excluded subtrees
  drawn hatched, not hidden.

### 0.6 — Everywhere, beautifully

Goal: the README screenshot sells it, and `brew install rings` works.

- **Treemap layout** (L). `L` toggles sunburst ↔ squarified treemap;
  same hit-testing contract as slices.
- **Layout presets** (M). Sunburst+list (default), list-only (auto under
  70 cols), dual-pane (two directories side by side for moving/compare).
  Boxes toggleable like btop's `1`–`4`.
- **In-app options menu** (M). btop-style `Esc` → Options: theme, units,
  layout, color mode, cache, with live preview and save-to-config.
- **Distribution** (M). Homebrew tap, AUR (`rings-bin`), scoop/winget,
  `cargo install rings`, `.deb` in the release. Shell completions
  (bash/zsh/fish/pwsh), a man page generated from `cli.rs`.
- **Show it** (S). VHS tape → animated GIF in the README; per-theme
  screenshots; a `scripts/preview` that turns the PPM test dumps into
  PNGs for PR review.
- **Benchmarks** (S). Scan speed vs `du`, `dust`, `ncdu`, `gdu` on a
  fixed fixture, published in the README, guarded by a CI perf test.

### 1.0

Stability pass, docs site, and a hard look at every key binding and
every color with fresh eyes. Nothing new.

## Engineering foundations (cross-cutting, start in 0.2)

- **Streaming tree.** `Tree` gains a `by_path` index so partial snapshots
  can preserve cursor/cwd; `recompute` becomes incremental per subtree.
- **Widget layer.** `tui/widgets/{box,list,meter,menu}.rs` replace the
  hand-laid rects in `draw.rs`. Every view is `fn draw(&Theme, Rect,
  &App) -> Hits`.
- **Theme + capability detection** in `term/`: `Caps { truecolor,
  colors: 16|256|16M, mouse_motion }`, resolved once at `Term::enter`.
- **Persistence format.** One versioned little-endian binary writer/reader
  in `cache.rs`, used by scan cache and diff. No serde.
- **Snapshot tests.** Golden text renders per view under
  `tests/snapshots/`, updated with `UPDATE_SNAPSHOTS=1`. The PPM dump
  becomes a helper, not a test.

## Decisions (settled 2026-08-30)

1. **Zero crates — kept.** It is the product's identity and every item
   above is doable in `std`. Cost: more code (config parser, hash,
   treemap) that we own.
2. **Launch screen is the mounts dashboard**, with the picker one
   keypress away. The picker stays the no-mount fallback and the `-`
   target.
3. **Scan cache on by default**, 0600 files, a one-line first-run
   notice, `--no-cache` to opt out. Root scans describe the whole
   filesystem, so the cache location is root-only.
4. **Duplicates finder is opt-in** and behind a progress bar; it is the
   only feature that reads file contents.
