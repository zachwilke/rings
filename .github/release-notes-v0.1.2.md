macOS, Windows PowerShell, multi-arch Linux, and Raspberry Pi OS / Raspbian.

```bash
# Linux x86_64 (Debian, Ubuntu, RHEL/Fedora, Arch — static musl)
curl -fsSL https://github.com/zachwilke/rings/releases/download/v0.1.2/rings-x86_64-linux-musl.xz | xz -d > rings
# 64-bit Raspberry Pi OS / aarch64 Linux
# …/rings-aarch64-linux-musl.xz
# 32-bit Raspberry Pi OS
# …/rings-armv7-linux-musleabihf.xz
# Pi 1 / Zero
# …/rings-arm-linux-musleabihf.xz
chmod +x rings && sudo ./rings /
```

- Linux: one static musl binary per arch (no glibc, no distro repo)
- Raspberry Pi OS 64-bit (`aarch64`) and 32-bit (`armv7l` / `armv6l`)
- macOS Apple Silicon and Intel
- Windows `.exe` from PowerShell (`.\rings.exe C:\`, `--plain` / `--csv` / `--json`)
- Same tiny TUI, `--plain` for scripts, delete-with-confirm
