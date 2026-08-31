Sunburst logo, full keybinding help, and a non-TUI mode for scripts.

```bash
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.1/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
sudo ./rings /
```

- Shared ASCII sunburst mark in the TUI, `rings help`, and the README
- `?` / `F1` overlay lists every key; `rings help` prints the same list
- `--plain` / `--no-tui` (and any pipe) prints a tab-separated table and never opens the TUI
- Still a tiny static musl binary, sudo scan, mouse, delete-with-confirm, temp/cache finder, CSV
