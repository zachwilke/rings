Braille-dot sunburst, a calmer footer, and an installer that puts rings on PATH for sudo.

```bash
# Linux x86_64 (Debian, Ubuntu, RHEL/Fedora, Arch — static musl)
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.4/rings-x86_64-linux-musl.xz | xz -d > rings
# 64-bit Raspberry Pi OS / aarch64 Linux
# …/rings-aarch64-linux-musl.xz
# 32-bit Raspberry Pi OS
# …/rings-armv7-linux-musleabihf.xz
# Pi 1 / Zero
# …/rings-arm-linux-musleabihf.xz
chmod +x rings && sudo ./rings /
```

- Sunburst cells are Braille 2×4 dots (btop-style); large panels show more rings
- Footer uses spaced chips, drops the repeated hit count, and shows a short keybind subset
- `install.sh` copies into `/usr/local/bin` with one sudo so `sudo rings /` works
