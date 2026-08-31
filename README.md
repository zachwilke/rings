```
       ╲    │    ╱
     ·  ╭───────╮  ·
  ─     │ ╭───╮ │     ─
        │ │ ◎ │ │
  ─     │ ╰───╯ │     ─
     ·  ╰───────╯  ·
       ╱    │    ╲
```

# rings

A DaisyDisk-style disk map for Linux servers, in one tiny static binary. Scan a path over SSH, see a colorful sunburst of what's eating the disk, drill in, and clear waste — with nothing deleted until you explicitly confirm.

## Install

Built for the machine that is already almost full: the download is a ~245 KB `.xz`, the binary ~585 KB, fully static (musl), zero runtime dependencies.

```bash
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.0/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
sudo ./rings /
```

Or build from source (Rust 1.88+, the only dependency is `libc`):

```bash
cargo build --release --target x86_64-unknown-linux-musl
cargo build --release
```

We deliberately skip UPX: it shrinks the download but needs extra memory and disk to unpack at every launch — the wrong trade on a full machine.

## Usage

```bash
sudo rings /           # full-disk scan (the normal server move)
rings /var/log         # any path; default is .
rings help             # logo, usage, and every key binding
rings --plain /        # parseable table to stdout, no TUI
rings --csv out.csv /  # findings CSV for scripts, then exit
rings --json /srv      # analyzed tree as JSON
```

When stdout is not a terminal (a pipe or redirect), rings prints the plain table automatically — no TUI, no spinner. `--plain` / `--no-tui`, `--csv`, and `--json` never enter the TUI, even in a terminal.

rings stays on one filesystem (`--all-filesystems` to cross), skips `/proc` `/sys` `/dev` `/run`, never follows symlinked directories, counts hard-linked inodes once, and counts permission errors instead of crashing. Without root it scans what it can read and reminds you `sudo rings /` sees everything.

Press `?` or `F1` in the TUI for the full key list (the footer always hints `? help`). `rings help` and `rings --help` print the same list.

| Keys | Mouse |
| --- | --- |
| `j` `k` / arrows — move | click a slice or row — select |
| Enter — drill in | double-click — drill in |
| `h` / Backspace — go up | click the footer buttons |
| Space — mark for delete · `f` — temp & cache · `c` — collector | |
| `x` — confirm delete · `e` — export CSV · `?` `F1` — help · `q` — quit | |

## Delete, carefully

Mark items with Space, review the **collector** (`c`) — full list, total size — then confirm with `x`. As root you must type `DELETE`; as a user, items go to trash when available. Every deletion is logged (stderr plus `/var/log/rings-delete.log` or `~/.local/share/rings/delete.log`). rings refuses to touch `/`, `/boot`, `/etc`, `/usr`, `/bin`, `/sbin`, `/lib`, the running kernel, and its own binary.

## Temp & cache

`f` surfaces the usual server waste — `/tmp`, `/var/tmp`, `/var/cache` (apt/dnf/yum/pacman), systemd journal, old logs, crash dumps, `~/.cache` — tagged by color in the sunburst and the CSV. Inspect, then decide. Nothing is ever auto-deleted.

## CSV and plain

`rings --csv findings.csv /` writes `path,type,size_bytes,size_human,category,in_delete_collector` — one row per directory, per temp/cache/log/journal/crash hit, and per file ≥ 1 MiB (tiny ordinary files are omitted so a root scan stays readable). Paths are RFC 4180 quoted; the file is written to a temp name then renamed, so a failed write never truncates an old export.

`rings --plain /` (or a pipe) writes the same rows, largest-first, as a tab-separated table: `path`, `type`, `size_bytes`, `size_human`, `category`. No color, no spinner, exit 0 on success.

## License

MIT
