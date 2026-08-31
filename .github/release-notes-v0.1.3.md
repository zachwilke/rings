Tighter sunburst and a launch-time GitHub Release update check.

```bash
# Linux x86_64 (Debian, Ubuntu, RHEL/Fedora, Arch — static musl)
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.3/rings-x86_64-linux-musl.xz | xz -d > rings
# 64-bit Raspberry Pi OS / aarch64 Linux
# …/rings-aarch64-linux-musl.xz
# 32-bit Raspberry Pi OS
# …/rings-armv7-linux-musleabihf.xz
# Pi 1 / Zero
# …/rings-arm-linux-musleabihf.xz
chmod +x rings && sudo ./rings /
```

- Sunburst rings scale with panel size; smaller real children stay visible
- Interactive TUI offers to install a newer GitHub Release (curl/wget, ~2s)
- `--offline` or `RINGS_NO_UPDATE=1` skips the check; pipes and `--plain` never prompt
- Still a tiny static musl binary — no HTTP crate
