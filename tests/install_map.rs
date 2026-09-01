//! Asset-name mapping for install.sh / install.ps1.
//! `--print-asset` is the stable interface; keep the table in lockstep with
//! the comment at the top of each installer.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[cfg(unix)]
fn print_asset(os: &str, arch: &str) -> std::process::Output {
    Command::new("sh")
        .arg(repo_root().join("install.sh"))
        .args(["--print-asset", os, arch])
        .output()
        .expect("run install.sh --print-asset")
}

#[cfg(unix)]
fn assert_asset(os: &str, arch: &str, want: &str) {
    let out = print_asset(os, arch);
    assert!(
        out.status.success(),
        "expected success for {os}/{arch}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        want,
        "{os}/{arch}"
    );
}

#[cfg(unix)]
fn assert_fail_contains(os: &str, arch: &str, needle: &str) {
    let out = print_asset(os, arch);
    assert!(
        !out.status.success(),
        "expected failure for {os}/{arch}, got {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains(needle),
        "stderr for {os}/{arch} should contain {needle:?}, got:\n{err}"
    );
}

#[cfg(unix)]
fn print_next(dest: &str) -> std::process::Output {
    Command::new("sh")
        .arg(repo_root().join("install.sh"))
        .args(["--print-next", dest])
        .output()
        .expect("run install.sh --print-next")
}

#[cfg(unix)]
fn assert_next(dest: &str, want: &str) {
    let out = print_next(dest);
    assert!(
        out.status.success(),
        "expected success for --print-next {dest}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        want,
        "--print-next {dest}"
    );
}

#[cfg(unix)]
#[test]
fn install_sh_syntax() {
    let status = Command::new("sh")
        .arg("-n")
        .arg(repo_root().join("install.sh"))
        .status()
        .expect("sh -n install.sh");
    assert!(status.success(), "sh -n install.sh failed");
}

#[cfg(unix)]
#[test]
fn install_sh_dash_syntax() {
    let result = Command::new("dash")
        .arg("-n")
        .arg(repo_root().join("install.sh"))
        .status();
    match result {
        Ok(status) => assert!(status.success(), "dash -n install.sh failed"),
        Err(_) => {
            // dash is optional (present on Debian/Ubuntu; often absent on macOS).
        }
    }
}

#[cfg(unix)]
#[test]
fn install_sh_next_uses_full_path_off_sudo_secure_path() {
    assert_next(
        "/home/zach/.local/bin/rings",
        "next: sudo /home/zach/.local/bin/rings /",
    );
    assert_next("/opt/rings/rings", "next: sudo /opt/rings/rings /");
    assert_next("/usr/local/bin/rings", "next: sudo rings /");
    assert_next("/usr/bin/rings", "next: sudo rings /");
    assert_next("/bin/rings", "next: sudo rings /");
}

#[cfg(unix)]
#[test]
fn install_sh_maps_uname_to_v0_1_2_assets() {
    assert_asset("Linux", "x86_64", "rings-x86_64-linux-musl.xz");
    assert_asset("Linux", "amd64", "rings-x86_64-linux-musl.xz");
    assert_asset("Linux", "aarch64", "rings-aarch64-linux-musl.xz");
    assert_asset("Linux", "arm64", "rings-aarch64-linux-musl.xz");
    assert_asset("Linux", "armv7l", "rings-armv7-linux-musleabihf.xz");
    assert_asset("Linux", "armv6l", "rings-arm-linux-musleabihf.xz");
    assert_asset("Linux", "armv6", "rings-arm-linux-musleabihf.xz");
    assert_asset("Darwin", "arm64", "rings-aarch64-apple-darwin.xz");
    assert_asset("Darwin", "x86_64", "rings-x86_64-apple-darwin.xz");
    assert_asset(
        "Windows",
        "AMD64",
        "rings-x86_64-pc-windows-msvc.exe.zip",
    );
    assert_asset(
        "Windows",
        "x86_64",
        "rings-x86_64-pc-windows-msvc.exe.zip",
    );
}

#[cfg(unix)]
#[test]
fn install_sh_rejects_unknown_os_arch() {
    assert_fail_contains("Linux", "riscv64", "riscv64");
    assert_fail_contains("FreeBSD", "x86_64", "FreeBSD");
    assert_fail_contains("Darwin", "powerpc", "powerpc");
    assert_fail_contains("Windows", "ARM64", "ARM64");
}

#[cfg(windows)]
fn ps_print_asset(os: &str, arch: &str) -> std::process::Output {
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-File",
            repo_root().join("install.ps1").to_str().unwrap(),
            "-PrintAsset",
            os,
            arch,
        ])
        .output()
        .expect("run install.ps1 -PrintAsset")
}

#[cfg(windows)]
#[test]
fn install_ps1_parses() {
    let script = format!(
        "$errs = $null; $null = [System.Management.Automation.Language.Parser]::ParseFile('{}', [ref]$null, [ref]$errs); if ($errs) {{ $errs | ForEach-Object {{ $_.ToString() }}; exit 1 }}",
        repo_root().join("install.ps1").display()
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .expect("parse install.ps1");
    assert!(status.success(), "install.ps1 failed to parse");
}

#[cfg(windows)]
#[test]
fn install_ps1_maps_x64_and_rejects_arm64() {
    let out = ps_print_asset("Windows", "AMD64");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "rings-x86_64-pc-windows-msvc.exe.zip"
    );

    let fail = ps_print_asset("Windows", "ARM64");
    assert!(!fail.status.success(), "ARM64 must not map to an asset");
    let err = format!(
        "{}{}",
        String::from_utf8_lossy(&fail.stderr),
        String::from_utf8_lossy(&fail.stdout)
    );
    assert!(err.contains("ARM64"), "error should mention ARM64:\n{err}");
}
