//! GitHub Release check and optional self-update.
//!
//! The TUI starts immediately; a background thread probes GitHub and the
//! UI pops a modal if a newer tag exists. Ctrl+U downloads, replaces this
//! binary, and re-execs. No HTTP crate: curl/wget (Unix) or curl.exe /
//! PowerShell (Windows). Failures stay silent so a down GitHub never
//! blocks a disk scan. Asset names stay in lockstep with `rings_asset()`
//! in install.sh — see the table on `asset_for_uname`.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::cli::Cli;

/// GitHub repo that publishes rings binaries.
pub const REPO: &str = "zachwilke/rings";

/// Latest-release API. GitHub is the source of truth.
pub const LATEST_API: &str = "https://api.github.com/repos/zachwilke/rings/releases/latest";

/// Hard timeout for the version check. One shot, no retry.
pub const CHECK_TIMEOUT: Duration = Duration::from_millis(2000);

/// Asset download after the operator said yes. Longer than the probe.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// Set to `1` to skip the check (also `--offline`).
pub const NO_UPDATE_ENV: &str = "RINGS_NO_UPDATE";

const ACCEPT: &str = "application/vnd.github+json";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// major.minor.patch, compared as a triple. A leading `v` is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// Parse `1.2.3` / `v1.2.3`. Extra junk after the third number is rejected.
pub fn parse_semver(raw: &str) -> Option<Semver> {
    let s = raw.strip_prefix('v').unwrap_or(raw);
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some(Semver {
        major,
        minor,
        patch,
    })
}

/// `Some(true)` when latest is a newer semver than current.
/// `None` when either side is unparseable (treat as "stay silent").
pub fn latest_is_newer(latest_tag: &str, current: &str) -> Option<bool> {
    let latest = parse_semver(latest_tag)?;
    let current = parse_semver(current)?;
    Some(latest > current)
}

/// Tiny scrape of `"tag_name": "..."` from GitHub JSON (pretty or minified).
pub fn scrape_tag_name(json: &str) -> Option<&str> {
    let key = "\"tag_name\"";
    let start = json.find(key)?;
    let after = json[start + key.len()..].trim_start();
    let after = after.strip_prefix(':')?.trim_start();
    let after = after.strip_prefix('"')?;
    let end = after.find('"')?;
    let tag = &after[..end];
    if tag.is_empty() {
        return None;
    }
    Some(tag)
}

pub fn tag_is_safe(tag: &str) -> bool {
    !tag.is_empty()
        && tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Release asset for an OS/arch pair (`uname -s` / `uname -m` style).
/// Keep this table identical to `rings_asset()` in install.sh:
///   Linux    x86_64|amd64   rings-x86_64-linux-musl.xz
///   Linux    aarch64|arm64  rings-aarch64-linux-musl.xz
///   Linux    armv7l         rings-armv7-linux-musleabihf.xz
///   Linux    armv6l|armv6   rings-arm-linux-musleabihf.xz
///   Darwin   arm64          rings-aarch64-apple-darwin.xz
///   Darwin   x86_64         rings-x86_64-apple-darwin.xz
///   Windows  AMD64|x86_64   rings-x86_64-pc-windows-msvc.exe.zip
///   Windows  ARM64          (no asset)
pub fn asset_for_uname(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("Linux", "x86_64" | "amd64") => Some("rings-x86_64-linux-musl.xz"),
        ("Linux", "aarch64" | "arm64") => Some("rings-aarch64-linux-musl.xz"),
        ("Linux", "armv7l") => Some("rings-armv7-linux-musleabihf.xz"),
        ("Linux", "armv6l" | "armv6") => Some("rings-arm-linux-musleabihf.xz"),
        ("Darwin", "arm64") => Some("rings-aarch64-apple-darwin.xz"),
        ("Darwin", "x86_64") => Some("rings-x86_64-apple-darwin.xz"),
        ("Windows", "AMD64" | "amd64" | "x86_64") => Some("rings-x86_64-pc-windows-msvc.exe.zip"),
        _ => None,
    }
}

/// Release asset for a Rust target triple. GNU hosts still fetch the musl
/// (or MSVC) published file — that is the binary GitHub ships.
pub fn asset_for_triple(triple: &str) -> Option<&'static str> {
    match triple {
        "x86_64-unknown-linux-musl" | "x86_64-unknown-linux-gnu" => {
            Some("rings-x86_64-linux-musl.xz")
        }
        "aarch64-unknown-linux-musl" | "aarch64-unknown-linux-gnu" => {
            Some("rings-aarch64-linux-musl.xz")
        }
        "armv7-unknown-linux-musleabihf" | "armv7-unknown-linux-gnueabihf" => {
            Some("rings-armv7-linux-musleabihf.xz")
        }
        "arm-unknown-linux-musleabihf" | "arm-unknown-linux-gnueabihf" => {
            Some("rings-arm-linux-musleabihf.xz")
        }
        "aarch64-apple-darwin" => Some("rings-aarch64-apple-darwin.xz"),
        "x86_64-apple-darwin" => Some("rings-x86_64-apple-darwin.xz"),
        "x86_64-pc-windows-msvc" | "x86_64-pc-windows-gnu" => {
            Some("rings-x86_64-pc-windows-msvc.exe.zip")
        }
        _ => None,
    }
}

/// Asset for the binary that is running right now.
pub fn current_release_asset() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        asset_for_uname("Linux", "x86_64")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        asset_for_uname("Linux", "aarch64")
    } else if cfg!(all(target_os = "linux", target_arch = "arm")) {
        if cfg!(target_feature = "v7") {
            asset_for_uname("Linux", "armv7l")
        } else {
            asset_for_uname("Linux", "armv6l")
        }
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        asset_for_uname("Darwin", "arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        asset_for_uname("Darwin", "x86_64")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        asset_for_uname("Windows", "x86_64")
    } else {
        None
    }
}

/// A newer GitHub Release the TUI can offer to install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateOffer {
    /// Tag as published, e.g. `v0.4.0`.
    pub tag: String,
    /// Tag without a leading `v`, for the popup.
    pub version: String,
    pub asset: &'static str,
    /// Whether the running binary's directory can be replaced in place.
    pub writable: bool,
}

pub fn running_version() -> &'static str {
    CURRENT_VERSION
}

pub fn installer_hint() -> &'static str {
    if cfg!(windows) {
        "irm https://raw.githubusercontent.com/zachwilke/rings/main/install.ps1 | iex"
    } else {
        "curl -fsSL https://raw.githubusercontent.com/zachwilke/rings/main/install.sh | sh"
    }
}

/// Interactive TUI launch only. Scripting flags, pipes, help, version, and
/// the skip switches never check.
pub fn should_check_update(cli: &Cli, stdout_is_tty: bool, env_skip: bool) -> bool {
    if cli.offline || env_skip || cli.help || cli.version {
        return false;
    }
    cli.wants_tui(stdout_is_tty)
}

pub fn env_skips_update() -> bool {
    match std::env::var(NO_UPDATE_ENV) {
        Ok(v) => !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"),
        Err(_) => false,
    }
}

fn user_agent() -> String {
    format!("rings/{CURRENT_VERSION}")
}

fn download_url(tag: &str, asset: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/{asset}")
}

/// Probe GitHub on a background thread. The receiver yields at most one
/// [`UpdateOffer`]; a down network, unknown arch, or current version is
/// silence, never an error.
pub fn spawn_check() -> std::sync::mpsc::Receiver<UpdateOffer> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(offer) = probe_update() {
            let _ = tx.send(offer);
        }
    });
    rx
}

fn probe_update() -> Option<UpdateOffer> {
    let asset = current_release_asset()?;
    let json = fetch_latest_json()?;
    let tag = scrape_tag_name(&json)?;
    if !tag_is_safe(tag) {
        return None;
    }
    if !latest_is_newer(tag, CURRENT_VERSION).unwrap_or(false) {
        return None;
    }
    let writable = current_exe_path()
        .ok()
        .and_then(|exe| exe.parent().map(dir_is_writable))
        .unwrap_or(false);
    Some(UpdateOffer {
        tag: tag.to_string(),
        version: tag.strip_prefix('v').unwrap_or(tag).to_string(),
        asset,
        writable,
    })
}

/// Download, replace this binary, and re-exec. Does not return on success.
pub fn apply_and_reexec(tag: &str, asset: &str) -> Result<(), String> {
    apply_update(tag, asset).map_err(|e| match e {
        ApplyError::NotWritable(path) => format!(
            "{} is not writable — cannot self-update (no sudo).\ninstall the latest with:\n  {}",
            path.display(),
            installer_hint()
        ),
        ApplyError::Msg(msg) => msg,
    })
}

enum ApplyError {
    NotWritable(PathBuf),
    Msg(String),
}

fn apply_update(tag: &str, asset: &str) -> Result<(), ApplyError> {
    let exe = current_exe_path()?;
    let dir = exe
        .parent()
        .ok_or_else(|| ApplyError::Msg("cannot find install directory".into()))?
        .to_path_buf();
    if !dir_is_writable(&dir) {
        return Err(ApplyError::NotWritable(dir));
    }

    let url = download_url(tag, asset);
    let archive = dir.join(format!(".rings-update-{}-{asset}", std::process::id()));
    let unpacked = dir.join(format!("{}.new", file_name_lossy(&exe)));

    let result = (|| -> Result<(), ApplyError> {
        download_file(&url, &archive)?;
        if !archive.is_file() || file_len(&archive) == 0 {
            return Err(ApplyError::Msg("download was empty".into()));
        }
        decompress_asset(asset, &archive, &unpacked)?;
        if file_len(&unpacked) == 0 {
            return Err(ApplyError::Msg("decompressed binary is empty".into()));
        }
        replace_and_reexec(&exe, &unpacked)
    })();

    let _ = fs::remove_file(&archive);
    if result.is_err() {
        let _ = fs::remove_file(&unpacked);
    }
    result
}

fn current_exe_path() -> Result<PathBuf, ApplyError> {
    let exe = std::env::current_exe().map_err(|e| ApplyError::Msg(e.to_string()))?;
    Ok(fs::canonicalize(&exe).unwrap_or(exe))
}

fn file_name_lossy(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rings".into())
}

fn file_len(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".rings-write-test-{}", std::process::id()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(f) => {
            drop(f);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn fetch_latest_json() -> Option<String> {
    let ua = user_agent();
    let secs = CHECK_TIMEOUT.as_secs().max(1);
    #[cfg(unix)]
    {
        if let Some(body) = curl_get(LATEST_API, &ua, secs, None) {
            return Some(body);
        }
        wget_get(LATEST_API, &ua, secs, None)
    }
    #[cfg(windows)]
    {
        if let Some(body) = curl_get(LATEST_API, &ua, secs, None) {
            return Some(body);
        }
        powershell_get_text(LATEST_API, &ua, secs)
    }
}

fn download_file(url: &str, dest: &Path) -> Result<(), ApplyError> {
    let ua = user_agent();
    let secs = DOWNLOAD_TIMEOUT.as_secs().max(1);
    #[cfg(unix)]
    {
        if curl_get(url, &ua, secs, Some(dest)).is_some() {
            return Ok(());
        }
        if wget_get(url, &ua, secs, Some(dest)).is_some() {
            return Ok(());
        }
        Err(ApplyError::Msg(
            "need curl or wget to download the update".into(),
        ))
    }
    #[cfg(windows)]
    {
        if curl_get(url, &ua, secs, Some(dest)).is_some() {
            return Ok(());
        }
        powershell_download(url, dest, secs)
    }
}

fn curl_get(url: &str, ua: &str, timeout_secs: u64, dest: Option<&Path>) -> Option<String> {
    let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
    let mut cmd = Command::new(curl);
    cmd.arg("-fsSL")
        .arg("--max-time")
        .arg(timeout_secs.to_string())
        .arg("-A")
        .arg(ua)
        .arg("-H")
        .arg(format!("Accept: {ACCEPT}"))
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(path) = dest {
        cmd.arg("-o").arg(path);
        let status = cmd.status().ok()?;
        if status.success() {
            return Some(String::new());
        }
        return None;
    }
    let out = cmd.stdout(Stdio::piped()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(unix)]
fn wget_get(url: &str, ua: &str, timeout_secs: u64, dest: Option<&Path>) -> Option<String> {
    let mut cmd = Command::new("wget");
    cmd.arg("-q")
        .arg("--timeout")
        .arg(timeout_secs.to_string())
        .arg(format!("--user-agent={ua}"))
        .arg(format!("--header=Accept: {ACCEPT}"))
        .arg(url)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(path) = dest {
        cmd.arg("-O").arg(path);
        let status = cmd.status().ok()?;
        if status.success() {
            return Some(String::new());
        }
        return None;
    }
    cmd.arg("-O").arg("-");
    let out = cmd.stdout(Stdio::piped()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(windows)]
fn powershell_get_text(url: &str, ua: &str, timeout_secs: u64) -> Option<String> {
    let script = format!(
        "try {{ (Invoke-WebRequest -Uri $env:RINGS_UPDATE_URL -Headers @{{'User-Agent'=$env:RINGS_UPDATE_UA;'Accept'='{ACCEPT}'}} -TimeoutSec {timeout_secs} -UseBasicParsing).Content }} catch {{ exit 1 }}"
    );
    let out = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .env("RINGS_UPDATE_URL", url)
        .env("RINGS_UPDATE_UA", ua)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(windows)]
fn powershell_download(url: &str, dest: &Path, timeout_secs: u64) -> Result<(), ApplyError> {
    let script = format!(
        "try {{ Invoke-WebRequest -Uri $env:RINGS_UPDATE_URL -OutFile $env:RINGS_UPDATE_OUT -TimeoutSec {timeout_secs} -UseBasicParsing }} catch {{ exit 1 }}"
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .env("RINGS_UPDATE_URL", url)
        .env("RINGS_UPDATE_OUT", dest)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| ApplyError::Msg(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(ApplyError::Msg("download failed".into()))
    }
}

fn decompress_asset(asset: &str, archive: &Path, dest: &Path) -> Result<(), ApplyError> {
    if asset.ends_with(".zip") {
        decompress_zip(archive, dest)
    } else {
        decompress_xz(archive, dest)
    }
}

fn decompress_xz(archive: &Path, dest: &Path) -> Result<(), ApplyError> {
    let mut cmd = if command_ok("xz") {
        let mut c = Command::new("xz");
        c.args(["-d", "-c"]).arg(archive);
        c
    } else if command_ok("xzcat") {
        let mut c = Command::new("xzcat");
        c.arg(archive);
        c
    } else {
        return Err(ApplyError::Msg(
            "need xz to decompress the update (apt install xz-utils / brew install xz)".into(),
        ));
    };
    let out = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| ApplyError::Msg(e.to_string()))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(ApplyError::Msg("xz decompress failed".into()));
    }
    write_atomic_bytes(dest, &out.stdout)
}

#[cfg(unix)]
fn decompress_zip(_archive: &Path, _dest: &Path) -> Result<(), ApplyError> {
    Err(ApplyError::Msg("zip updates are Windows-only".into()))
}

#[cfg(windows)]
fn decompress_zip(archive: &Path, dest: &Path) -> Result<(), ApplyError> {
    let extract = dest.with_extension("extract");
    let _ = fs::remove_dir_all(&extract);
    fs::create_dir_all(&extract).map_err(|e| ApplyError::Msg(e.to_string()))?;
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Expand-Archive -Force -Path $env:RINGS_UPDATE_ZIP -DestinationPath $env:RINGS_UPDATE_DIR",
        ])
        .env("RINGS_UPDATE_ZIP", archive)
        .env("RINGS_UPDATE_DIR", &extract)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| ApplyError::Msg(e.to_string()))?;
    if !status.success() {
        let _ = fs::remove_dir_all(&extract);
        return Err(ApplyError::Msg("failed to unzip the update".into()));
    }
    let found = find_rings_exe(&extract);
    let result = match found {
        Some(exe) => {
            let bytes = fs::read(&exe).map_err(|e| ApplyError::Msg(e.to_string()))?;
            write_atomic_bytes(dest, &bytes)
        }
        None => Err(ApplyError::Msg("zip did not contain rings.exe".into())),
    };
    let _ = fs::remove_dir_all(&extract);
    result
}

#[cfg(windows)]
fn find_rings_exe(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(p) = stack.pop() {
        let entries = fs::read_dir(&p).ok()?;
        for ent in entries.flatten() {
            let path = ent.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case("rings.exe"))
            {
                return Some(path);
            }
        }
    }
    None
}

fn write_atomic_bytes(dest: &Path, bytes: &[u8]) -> Result<(), ApplyError> {
    let mut f = File::create(dest).map_err(|e| ApplyError::Msg(e.to_string()))?;
    f.write_all(bytes)
        .map_err(|e| ApplyError::Msg(e.to_string()))?;
    f.sync_all().map_err(|e| ApplyError::Msg(e.to_string()))?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(dest)
            .map_err(|e| ApplyError::Msg(e.to_string()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(dest, perms).map_err(|e| ApplyError::Msg(e.to_string()))?;
    }
    Ok(())
}

fn replace_and_reexec(exe: &Path, new_bin: &Path) -> Result<(), ApplyError> {
    #[cfg(unix)]
    {
        fs::rename(new_bin, exe).map_err(|e| {
            if e.kind() == io::ErrorKind::PermissionDenied {
                ApplyError::NotWritable(exe.to_path_buf())
            } else {
                ApplyError::Msg(e.to_string())
            }
        })?;
        if let Some(dir) = exe.parent() {
            if let Ok(dirf) = File::open(dir) {
                let _ = dirf.sync_all();
            }
        }
        reexec(exe)
    }
    #[cfg(windows)]
    {
        // A running Windows exe cannot be overwritten, but it can be renamed.
        // rings.exe → rings.exe.old, rings.exe.new → rings.exe, then spawn
        // the new process. The new process deletes the leftover .old file.
        // `rings.exe` → `rings.exe.old`. Rename works on a running image.
        let old = {
            let mut name = file_name_lossy(exe);
            name.push_str(".old");
            exe.with_file_name(name)
        };
        let _ = fs::remove_file(&old);
        fs::rename(exe, &old).map_err(|e| ApplyError::Msg(e.to_string()))?;
        if let Err(e) = fs::rename(new_bin, exe) {
            let _ = fs::rename(&old, exe);
            return Err(ApplyError::Msg(e.to_string()));
        }
        reexec(exe)
    }
}

fn reexec(exe: &Path) -> Result<(), ApplyError> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(exe).args(&args).env(NO_UPDATE_ENV, "1").exec();
        Err(ApplyError::Msg(format!("exec failed: {err}")))
    }
    #[cfg(windows)]
    {
        Command::new(exe)
            .args(&args)
            .env(NO_UPDATE_ENV, "1")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ApplyError::Msg(e.to_string()))?;
        std::process::exit(0);
    }
}

/// Drop a leftover `rings.exe.old` from a previous Windows self-update.
pub fn cleanup_replaced_exe() {
    #[cfg(windows)]
    {
        if let Ok(exe) = std::env::current_exe() {
            let mut name = file_name_lossy(&exe);
            name.push_str(".old");
            if let Some(dir) = exe.parent() {
                let old = dir.join(name);
                let _ = fs::remove_file(old);
            }
        }
    }
}

fn command_ok(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_vs_current_equal_newer_older_junk() {
        assert_eq!(parse_semver("0.1.2"), parse_semver("v0.1.2"));
        assert_eq!(
            parse_semver("v0.1.3"),
            Some(Semver {
                major: 0,
                minor: 1,
                patch: 3
            })
        );
        assert_eq!(latest_is_newer("v0.1.2", "0.1.2"), Some(false));
        assert_eq!(latest_is_newer("0.1.2", "0.1.2"), Some(false));
        assert_eq!(latest_is_newer("v0.1.3", "0.1.2"), Some(true));
        assert_eq!(latest_is_newer("v0.2.0", "0.1.9"), Some(true));
        assert_eq!(latest_is_newer("v0.1.1", "0.1.2"), Some(false));
        assert_eq!(latest_is_newer("v0.0.9", "0.1.0"), Some(false));
        assert_eq!(latest_is_newer("not-a-version", "0.1.2"), None);
        assert_eq!(latest_is_newer("v0.1.2", "nope"), None);
        assert_eq!(latest_is_newer("v1.2", "0.1.2"), None);
        assert_eq!(latest_is_newer("v0.1.2-beta", "0.1.2"), None);
        assert_eq!(latest_is_newer("", "0.1.2"), None);
    }

    #[test]
    fn scrape_tag_name_pretty_and_minified() {
        let pretty = "{\n  \"url\": \"https://example\",\n  \"tag_name\": \"v0.1.3\",\n  \"name\": \"v0.1.3\"\n}";
        assert_eq!(scrape_tag_name(pretty), Some("v0.1.3"));
        let mini = r#"{"url":"x","tag_name":"v0.1.4","draft":false}"#;
        assert_eq!(scrape_tag_name(mini), Some("v0.1.4"));
        assert_eq!(scrape_tag_name("{\"name\":\"nope\"}"), None);
        assert_eq!(scrape_tag_name(""), None);
        assert_eq!(scrape_tag_name("{\"tag_name\": \"\"}"), None);
    }

    #[test]
    fn tag_safety() {
        assert!(tag_is_safe("v0.1.3"));
        assert!(tag_is_safe("0.1.3"));
        assert!(!tag_is_safe(""));
        assert!(!tag_is_safe("v0.1.3\ncurl evil"));
        assert!(!tag_is_safe("../x"));
    }

    #[test]
    fn asset_name_for_each_target_triple() {
        assert_eq!(
            asset_for_triple("x86_64-unknown-linux-musl"),
            Some("rings-x86_64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_triple("x86_64-unknown-linux-gnu"),
            Some("rings-x86_64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_triple("aarch64-unknown-linux-musl"),
            Some("rings-aarch64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_triple("armv7-unknown-linux-musleabihf"),
            Some("rings-armv7-linux-musleabihf.xz")
        );
        assert_eq!(
            asset_for_triple("arm-unknown-linux-musleabihf"),
            Some("rings-arm-linux-musleabihf.xz")
        );
        assert_eq!(
            asset_for_triple("aarch64-apple-darwin"),
            Some("rings-aarch64-apple-darwin.xz")
        );
        assert_eq!(
            asset_for_triple("x86_64-apple-darwin"),
            Some("rings-x86_64-apple-darwin.xz")
        );
        assert_eq!(
            asset_for_triple("x86_64-pc-windows-msvc"),
            Some("rings-x86_64-pc-windows-msvc.exe.zip")
        );
        assert_eq!(asset_for_triple("riscv64gc-unknown-linux-gnu"), None);
        assert_eq!(asset_for_triple("aarch64-pc-windows-msvc"), None);
    }

    #[test]
    fn asset_name_mirrors_install_sh_uname_table() {
        assert_eq!(
            asset_for_uname("Linux", "x86_64"),
            Some("rings-x86_64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "amd64"),
            Some("rings-x86_64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "aarch64"),
            Some("rings-aarch64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "arm64"),
            Some("rings-aarch64-linux-musl.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "armv7l"),
            Some("rings-armv7-linux-musleabihf.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "armv6l"),
            Some("rings-arm-linux-musleabihf.xz")
        );
        assert_eq!(
            asset_for_uname("Linux", "armv6"),
            Some("rings-arm-linux-musleabihf.xz")
        );
        assert_eq!(
            asset_for_uname("Darwin", "arm64"),
            Some("rings-aarch64-apple-darwin.xz")
        );
        assert_eq!(
            asset_for_uname("Darwin", "x86_64"),
            Some("rings-x86_64-apple-darwin.xz")
        );
        assert_eq!(
            asset_for_uname("Windows", "AMD64"),
            Some("rings-x86_64-pc-windows-msvc.exe.zip")
        );
        assert_eq!(
            asset_for_uname("Windows", "x86_64"),
            Some("rings-x86_64-pc-windows-msvc.exe.zip")
        );
        assert_eq!(asset_for_uname("Windows", "ARM64"), None);
        assert_eq!(asset_for_uname("Linux", "riscv64"), None);
        assert_eq!(asset_for_uname("FreeBSD", "x86_64"), None);
    }

    #[test]
    fn current_host_maps_to_a_published_asset() {
        let asset = current_release_asset().expect("CI hosts are published release targets");
        assert!(
            asset.starts_with("rings-") && (asset.ends_with(".xz") || asset.ends_with(".zip")),
            "{asset}"
        );
    }

    #[test]
    fn skip_when_not_tty_or_plain_or_offline() {
        let interactive = Cli::default();
        assert!(
            should_check_update(&interactive, true, false),
            "TTY + no flags should check"
        );
        assert!(
            !should_check_update(&interactive, false, false),
            "piped stdout must not check"
        );

        let mut plain = Cli::default();
        plain.plain = true;
        assert!(!should_check_update(&plain, true, false));

        let mut json = Cli::default();
        json.json = true;
        assert!(!should_check_update(&json, true, false));

        let mut csv = Cli::default();
        csv.csv = Some(PathBuf::from("out.csv"));
        assert!(!should_check_update(&csv, true, false));

        let mut offline = Cli::default();
        offline.offline = true;
        assert!(!should_check_update(&offline, true, false));

        assert!(
            !should_check_update(&interactive, true, true),
            "RINGS_NO_UPDATE must skip"
        );

        let mut help = Cli::default();
        help.help = true;
        assert!(!should_check_update(&help, true, false));

        let mut version = Cli::default();
        version.version = true;
        assert!(!should_check_update(&version, true, false));
    }
}
