This patch release makes rings remember how you like to work.

Press `M` anywhere in the TUI to open the new keyboard-first settings menu. Use
`j` / `k` to move, `h` / `l` to preview all 13 built-in themes, and Enter to
edit the folder used by interactive CSV exports. Changes persist between rings
sessions; an explicit `--theme` still overrides the saved theme for one launch.

Settings are stored in `$XDG_CONFIG_HOME/rings/config`, falling back to
`~/.config/rings/config` (and the corresponding user-profile path on Windows).
The menu is a real modal: controls beneath it cannot be clicked accidentally.

All database analysis, deletion safeguards, context menus, directory picker,
sunburst and icicle layouts from v0.3.0 remain intact.

```bash
# Linux x86_64
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.3.1/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
```
