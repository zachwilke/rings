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
- **Theme engine** (M) — *done; 13 built-ins as of 2026-09-01*.
  `tokyo-night` is the default, with `-storm` and `-day` variants, plus
  `catppuccin`, `rose-pine`, `everforest`, `one-dark`, and the originals
  (`rings`, `nord`, `gruvbox`, `dracula`, `solarized-dark`, `mono`).
  `--help` lists them from the registry, so a new built-in cannot leave the
  docs stale. Detect true-color (`COLORTERM`), fall back to 256, then 16.
  `NO_COLOR`. See Decision 7 for what a theme now has to satisfy.
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

- **Application awareness** (M) — *first pass landed*. PostgreSQL clusters
  (by `PG_VERSION` + `base/` + `global/`, so packaging layout does not
  matter) and SQLite databases (by header magic, because the extension is
  useless). Every file gets a *role* — data, wal, spill, log, meta — and
  each role a *disposition*: `Never`, `Command`, or `Safe`. The point is
  that for a database the answer is almost never "delete this": it is
  VACUUM, or a checkpoint, or raising `work_mem`. Live data is refused by
  the delete safeguard, and the refusal propagates *up* the tree so the
  untagged parent a user actually selects is refused too.
  MySQL/MariaDB (datadir by `ibdata1`; binary logs are the headline —
  `PURGE BINARY LOGS`, never `rm`, which orphans the `.index` and breaks
  replicas) and SQL Server (`.mdf`/`.ndf`/`.ldf` by extension, since it has
  no canonical datadir; a `.ldf` several times its `.mdf` is the classic
  disk-full call) both landed alongside PostgreSQL and SQLite.
  *Next:* MongoDB, Elasticsearch, Redis, Docker overlay2.
  *Also next:* colour by disposition, not just by category.
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

- **Alternative layouts** (M) — *icicle landed 2026-09-01, see Decision 6*.
  `L` toggles sunburst ↔ icicle. The icicle takes the full width with the
  child list beneath it, writes names inside the bars, and shrinks to the
  tree's actual depth; bodies too short for a map degrade to list-only for
  free. A treemap remains possible but is a distant third — see the
  decision for why.
  *Next:* remember the layout in the config file, and pick the icicle
  automatically under ~70 columns where the disc stops being readable.
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
   only feature that reads *whole* file contents.
7. **A theme is a contract, not a colour list** (settled 2026-09-01).
   The icicle paints palette colours as *backgrounds*, which nothing did
   before it, and `tokyo-night-day` is the first pale ground. Between them
   they broke three helpers that had quietly assumed a dark theme, so those
   are now derived rather than hard-coded:
   - `dim_color` fades toward `bg`, not toward black. On a light theme,
     darkening makes a wedge *more* prominent, so the old multiply
     inverted the depth cue.
   - `brighten` became `emphasize`, moving *away* from the ground —
     lighter on dark, darker on light.
   - `contrast_on` picks whichever of `text`/`bg` is further away in
     luminance. Choosing either unconditionally makes a label vanish:
     `text` on a pale wedge, `bg` on a pale theme.
   Every built-in is held to four properties by test: text/surface
   luminance gap > 90, hover nearer the ground than selection, *every*
   palette and category hue legible when used as a label background
   (> 80), and slug-safe names. A new theme that fails any of them fails
   the build rather than shipping an unreadable view.

6. **The icicle is the second layout, not the treemap** (settled
   2026-09-01, after prototyping all three at 78 columns).
   - *The sunburst cannot show text.* Every wedge is anonymous, so the
     eye has to round-trip through the side list. In a terminal, text is
     the one thing we are unambiguously good at, and the current view
     spends none of it.
   - *An icicle is the sunburst unrolled.* `Slice { start, end, ring }`
     already holds 0–1 fractions and a depth — that is an icicle spec
     verbatim. `x = start × width`, `y = ring`. Hit testing needs no new
     code either: `slice_at(slices, ring, angle)` takes only those two
     values, so the polar transform in `polar()` is the *only*
     sunburst-specific line in the whole path.
   - *It costs a quarter of the space.* Four levels in four rows, versus
     ~45×24 for ten rings with the corners and the centre hole wasted.
     That buys back the room the details pane (0.2) needs.
   - *Squarified treemaps reorder under streaming.* A child crossing a
     strip boundary reshuffles its neighbours, which will flicker badly
     against the streaming scan in 0.2. Icicle ordering is stable.
     Bordered nested treemaps are also unreadable at 80 columns — the
     borders eat the content; a filled treemap needs colour to work at
     all, so it degrades worst on the 16-colour TTYs we support.
   - The sunburst stays the default and the README image. It is the
     product's face, and it is genuinely the best "shape at a glance"
     view. This is about having a second layout, not replacing it.

5. **Bounded header probes are exempt from (4)** (settled 2026-09-01).
   Application detection may read at most 4 KB from the head of a file,
   only for files past a size floor (1 MiB), capped at 4096 files per
   scan. That is a different order of cost from hashing every file ≥1 MiB,
   and it is the only way to report a real number: SQLite's 100-byte
   header gives exactly what `VACUUM` would return. `--no-app-probe` turns
   the reads off. Structural detection and the delete safeguards are *not*
   behind that flag — protection must not be something a performance
   switch can disable.
