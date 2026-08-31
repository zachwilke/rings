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

A DaisyDisk-style disk map in one tiny static binary. Scan a path, see a colorful sunburst of what's eating the disk, drill in, and clear waste — with nothing deleted until you explicitly confirm.

Works on Linux (Debian, Ubuntu, RHEL/Fedora/CentOS, Arch), Raspberry Pi OS / Raspbian (64-bit and 32-bit), macOS, and Windows PowerShell.

## Install

Detect OS/arch and install the matching binary from the latest GitHub Release:

```bash
curl -fsSL https://raw.githubusercontent.com/zachwilke/rings/main/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/zachwilke/rings/main/install.ps1 | iex
```

The one-liner lives on `main` (so the platform map stays current) and fetches the latest published release, not a pinned tag. Pin with `RING_VERSION=v0.2.0`; install to a chosen directory with `RING_PREFIX`. Unix needs `curl` or `wget`, plus `xz` (`apt install xz-utils` / `brew install xz`).

The Unix installer writes `/usr/local/bin/rings` (may ask for sudo once) so `sudo rings /` works. If it cannot write there, it falls back to `~/.local/bin` and prints the full-path next command (`sudo ~/.local/bin/rings /`).

Linux downloads are one static musl binary per arch — the same file for Debian/Ubuntu, RHEL/Fedora/CentOS, and Arch. No glibc version soup, no distro repo.

Manual per-platform download (fallback):

### Linux

```bash
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
sudo ./rings /
```

On aarch64 machines (servers, 64-bit Pi OS) use `rings-aarch64-linux-musl.xz` instead.

### Raspberry Pi / Raspbian

`uname -m` picks the asset: `aarch64` is 64-bit Raspberry Pi OS (Pi 3/4/5). `armv7l` is 32-bit Raspberry Pi OS (Pi 2/3/4). `armv6l` is Pi 1 / Zero.

```bash
# 64-bit Raspberry Pi OS
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-aarch64-linux-musl.xz | xz -d > rings
# 32-bit Raspberry Pi OS (Pi 2/3/4)
# curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-armv7-linux-musleabihf.xz | xz -d > rings
# Pi 1 / Zero (armv6)
# curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-arm-linux-musleabihf.xz | xz -d > rings
chmod +x rings
sudo ./rings /
```

### macOS

```bash
# Apple Silicon
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-aarch64-apple-darwin.xz | xz -d > rings
# Intel: rings-x86_64-apple-darwin.xz
chmod +x rings
sudo ./rings /
```

### Windows (PowerShell)

```powershell
irm https://github.com/zachwilke/rings/releases/download/v0.2.0/rings-x86_64-pc-windows-msvc.exe.zip -OutFile rings.zip
Expand-Archive -Force rings.zip .
.\rings.exe C:\
.\rings.exe --plain C:\
```

Or `Invoke-WebRequest` if you prefer the long name. `--plain`, `--csv`, `--json`, and `--help` work from any PowerShell; the sunburst TUI uses the Windows console (Windows Terminal looks best).

Or build from source (Rust 1.88+; `libc` on Unix only):

```bash
cargo build --release --target x86_64-unknown-linux-musl
cargo build --release
```

We deliberately skip UPX: it shrinks the download but needs extra memory and disk to unpack at every launch — the wrong trade on a full machine.

## Usage

```bash
sudo rings /           # full-disk scan (Linux, Raspberry Pi OS, macOS)
rings.exe C:\          # Windows PowerShell (Administrator for the whole disk)
rings /var/log         # any path
sudo rings             # no path: pick the directory to scan first
rings help             # logo, usage, and every key binding
rings --plain /        # parseable table to stdout, no TUI
rings --offline /      # TUI without the GitHub Release update check
rings --csv out.csv /  # findings CSV for scripts, then exit
rings --json /srv      # analyzed tree as JSON
```

Given a path, rings scans it straight away. Started with no path in a terminal, it opens a vim-style directory picker in the current directory instead: `j` / `k` to move, `Enter` (or `l`) to open a directory, `h` / Backspace to go up, `s` to scan the highlighted directory. Piped or scripted runs with no path still scan the current directory, unchanged.

From a finished scan, `-` (or the **Picker** button) reopens the picker at the directory you are browsing, so you can scan somewhere else without restarting rings. `Esc` — or **Back to scan** — returns to the scan you left, untouched; scanning a new directory replaces it.

When stdout is not a terminal (a pipe or redirect), rings prints the plain table automatically — no TUI, no spinner. `--plain` / `--no-tui`, `--csv`, and `--json` never enter the TUI, even in a terminal.

On an interactive TUI launch, rings asks GitHub Releases whether a newer version is out (about two seconds, then gives up) and offers to install it in place. `--plain`, `--json`, `--csv`, pipes, `--help`, and `--version` never check and never prompt. Skip with `--offline` or `RINGS_NO_UPDATE=1`. The check uses `curl` or `wget` (PowerShell / `curl.exe` on Windows) — no extra libraries. If the install path is not writable, rings prints the installer one-liner instead of sudoing.

rings stays on one filesystem (`--all-filesystems` to cross), skips Linux virtual mounts (`/proc` `/sys` `/dev` `/run`; `/dev` on macOS), never follows symlinked directories, counts hard-linked inodes once, and counts permission errors instead of crashing. Without root it scans what it can read and reminds you `sudo rings /` (or Administrator on Windows) sees everything.

Press `?` or `F1` in the TUI for the full key list (the footer always hints `? help`). `rings help` and `rings --help` print the same list.

| Keys | Mouse |
| --- | --- |
| `j` `k` / arrows — move | click a slice or row — select |
| Enter — drill in | double-click — drill in |
| `h` / Backspace — go up · `-` — picker | right-click — context menu |
| Space — mark for delete · `f` — temp & cache · `c` — collector | click the footer buttons |
| `x` — confirm delete · `e` — export CSV · `?` `F1` — help · `q` — quit | click a breadcrumb — jump |

In the picker: `j` `k` move, `Enter` opens a directory, `h` goes up, `s` scans the highlighted one, `Esc` goes back to the scan you came from.

Hovering highlights rows, slices, and buttons; hovering a slice shows its path, size, and share of its parent in the footer. The scroll wheel moves the cursor. Right-click any slice, row, or picker entry for a context menu — open, mark for delete, or delete that file or directory. Deleting from the menu marks the item and opens the same confirm modal as `x`; it never unlinks on the click.

## Themes and color

`--theme nord` (also `gruvbox`, `dracula`, `solarized-dark`, `mono`; default `rings`). Color depth follows the terminal: 24-bit where `COLORTERM`, `TERM_PROGRAM`, or `WT_SESSION` say so, 256 colors for other xterm-alikes, 16 for the Linux console. `NO_COLOR` turns color off (bold and reverse video only); `RINGS_COLORS=16|256|truecolor` overrides detection.

## Delete, carefully

Mark items with Space, review the **collector** (`c`) — full list, total size — then confirm with `x`. As root/Administrator you must type `DELETE`; as a user, items go to trash (XDG Trash on Linux, `~/.Trash` on macOS, Recycle Bin on Windows). Every deletion is logged. rings refuses to touch system roots (`/`, `/boot`, `/etc`, `/System`, `C:\`, `C:\Windows`, …), the running kernel, and its own binary.

## Temp & cache

`f` surfaces the usual waste — `/tmp`, `/var/cache` (apt/dnf/yum/pacman), systemd journal, old logs, crash dumps, `~/.cache`, macOS `Library/Caches`, Windows `%TEMP%` — tagged by color in the sunburst and the CSV. Inspect, then decide. Nothing is ever auto-deleted.

## CSV and plain

`rings --csv findings.csv /` writes `path,type,size_bytes,size_human,category,in_delete_collector` — one row per directory, per temp/cache/log/journal/crash hit, and per file ≥ 1 MiB (tiny ordinary files are omitted so a root scan stays readable). Paths are RFC 4180 quoted; the file is written to a temp name then renamed, so a failed write never truncates an old export.

`rings --plain /` (or a pipe) writes the same rows, largest-first, as a tab-separated table: `path`, `type`, `size_bytes`, `size_human`, `category`. No color, no spinner, exit 0 on success.

## License

MIT
