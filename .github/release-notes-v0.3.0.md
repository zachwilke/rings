rings now knows what a database is. Point it at a server and it tells you which files are load-bearing, which are garbage, and — for the ones in between — the command that actually reclaims the space.

```bash
# Linux x86_64 (Debian, Ubuntu, RHEL/Fedora, Arch — static musl)
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.3.0/rings-x86_64-linux-musl.xz | xz -d > rings
# 64-bit Raspberry Pi OS / aarch64 Linux
# …/rings-aarch64-linux-musl.xz
# 32-bit Raspberry Pi OS
# …/rings-armv7-linux-musleabihf.xz
# Pi 1 / Zero
# …/rings-arm-linux-musleabihf.xz
chmod +x rings && sudo ./rings /
```

**Databases, understood.** PostgreSQL clusters, MySQL/MariaDB data directories, SQL Server files, and SQLite databases are recognised while rings walks. Press `b` for the Databases view. Every row carries a role — data, wal, binlog, spill, log, backup — and the action that frees the space, because for a database the answer is almost never `rm`:

```
Databases · 6 · 63.5 GB reclaimable without removing data
 ● data     /var/lib/pgsql/16/data/base           31.4 GB
 ● wal      /var/lib/pgsql/16/data/pg_wal          1.0 GB
 ○ spill    /var/lib/pgsql/16/data/base/pgsql_tmp  3.2 GB   → 3.2 GB
 ● binlog   /var/lib/mysql (binlog)               60.0 GB   → 60.0 GB
     PURGE BINARY LOGS BEFORE NOW() - INTERVAL 7 DAY, then set
     binlog_expire_logs_seconds — deleting the files by hand breaks
     the .index and any replica
```

`●` means rings will refuse to delete it, `▸` means a command is the better move, `○` is ordinary waste. Nothing to enable: detection runs on every scan.

**Found by layout, not by guesswork.** A PostgreSQL cluster is `PG_VERSION` + `base/` + `global/`, so rings finds it wherever your packager put it — Debian, RHEL, a Docker volume, or a `pg_basebackup` on a backup disk. MySQL is `ibdata1`; MariaDB is told apart by its Aria control file. SQL Server is `.mdf`/`.ndf`/`.ldf`, and a `.ldf` several times its data file gets called out with the ratio, because that is a log that is not being truncated. SQLite is found by its 16-byte header magic — Chrome's `History` and Firefox's `places.sqlite` both get picked up, and the same 100-byte read reports exactly what `VACUUM` would hand back.

**It will not let you break a running server.** Live data files, write-ahead logs and binary logs are refused by the delete safeguard, and the refusal reaches the directories *above* them — `rm -rf` one level up destroys a cluster just as thoroughly as deleting `base/`. Guarded rows are marked in the browse list too, so you find out before you invest in a selection rather than at the confirm modal.

**Nothing talks to a server.** No client libraries, no credentials, no connections. That keeps the analysis working on the volumes where a disk tool is most useful: backup mounts, snapshots, detached disks and stopped containers. Header reads are bounded to 4 KB per candidate past a 1 MiB floor, capped per scan; `--no-app-probe` turns them off entirely and leaves the protection in place.

**A second way to see the tree.** `L` toggles the sunburst for an icicle — the same layout in Cartesian coordinates. A wedge cannot hold its own name, so the disc spends the one thing a terminal is good at on nothing; four icicle rows identify more of the tree than the whole disc does, and the child list gets the full width instead of 42% of it.

```
▏ /  178.0 GB
▏home  92.0 GB                          ▏var  46.0 GB      ▏usr  28.0 GB  ▏opt
▏zach  88.0 GB                       ▏  ▏lib  38.0 GB  ▏  ▏▏share  ▏lib ▏
▏Library  34.0 GB ▏Downloads▏src  ▏     ▏docker      ▏
```

It shrinks to the tree's real depth, and a terminal too short for a map falls back to list-only.

**Tokyo Night, and twelve more.** `tokyo-night` is the new default, with `-storm` and `-day` variants, plus `catppuccin`, `rose-pine`, `everforest` and `one-dark` alongside `rings`, `nord`, `gruvbox`, `dracula`, `solarized-dark` and `mono`. `--theme <name>`; `rings --help` lists them all. The icicle paints slice colours as backgrounds and `tokyo-night-day` is the first pale ground, which between them meant the fade, emphasis and label-contrast helpers all had to stop assuming a dark theme. Every built-in is now held to four legibility properties by test.

**Exports know too.** `--csv` gains `application`, `role` and `reclaim` columns; `--json` gains the matching fields; `--plain` gains `application` and `role`. So a fleet sweep is one line:

```bash
sudo rings --plain /var/lib | awk -F'\t' '$6=="mysql" && $7=="binlog"'
```

**Fixed.** A new scan now drops the delete collector, as the picker has always warned it would — it never did, which left stale references that could mark the wrong file. Each view's cursor is clamped against its own list rather than whichever list happened to be in front. Two `--help` lines wrapped on an 80-column terminal.
