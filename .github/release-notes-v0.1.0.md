Tiny DaisyDisk-style disk map for Linux servers. One static musl binary, no runtime deps.

Download is about 245 KB xz (binary about 585 KB). Built for a machine that is already almost full.

```bash
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.0/rings-x86_64-linux-musl.xz | xz -d > rings
chmod +x rings
sudo ./rings /
```

Colorful sunburst TUI, mouse and keyboard. sudo full-disk scan. Temp and cache finder (never auto-deletes). Delete collector with confirm (type DELETE as root). CSV export: `rings --csv findings.csv /`.
