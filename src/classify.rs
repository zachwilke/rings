//! Tag common waste: temp, cache, logs, journals, crash dumps.

use std::path::Path;

/// Waste / content tag stored on each node and written to CSV.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    Normal,
    Temp,
    Cache,
    Log,
    Journal,
    Crash,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Normal => "normal",
            Category::Temp => "temp",
            Category::Cache => "cache",
            Category::Log => "log",
            Category::Journal => "journal",
            Category::Crash => "crash",
        }
    }

    pub fn is_waste(self) -> bool {
        self != Category::Normal
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Normal => "normal",
            Category::Temp => "temp",
            Category::Cache => "cache",
            Category::Log => "log",
            Category::Journal => "journal",
            Category::Crash => "crash dump",
        }
    }
}

impl std::fmt::Display for Category {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Longest-prefix first so `/var/log/journal` is a journal, not a log.
/// Rules are path-shaped, not host-shaped: a Windows tree mounted on Linux
/// still tags `$Recycle.Bin`, and a Time Machine volume still tags Caches.
const PREFIX_RULES: &[(&str, Category)] = &[
    ("/var/log/journal", Category::Journal),
    ("/run/log/journal", Category::Journal),
    ("/var/lib/systemd/coredump", Category::Crash),
    ("/var/lib/apport/coredump", Category::Crash),
    ("/var/lib/apt/lists", Category::Cache),
    ("/var/lib/snapd/cache", Category::Cache),
    ("/var/lib/docker/tmp", Category::Temp),
    ("/var/cache/snapd", Category::Cache),
    ("/var/crash", Category::Crash),
    ("/var/cache", Category::Cache),
    ("/Library/Logs/DiagnosticReports", Category::Crash),
    ("/Library/Logs/CrashReporter", Category::Crash),
    ("/private/var/log", Category::Log),
    ("/private/var/vm", Category::Temp),
    ("/private/var/tmp", Category::Temp),
    ("/private/var/folders", Category::Temp),
    ("/private/tmp", Category::Temp),
    ("/var/folders", Category::Temp),
    ("/var/log", Category::Log),
    ("/var/tmp", Category::Temp),
    ("/tmp", Category::Temp),
    ("/Library/Caches", Category::Cache),
    ("/Library/Logs", Category::Log),
];

/// Classify an absolute or relative path. Relative paths still match
/// well-known suffixes (`.cache`, `thumbnails`, package cache names).
pub fn classify(path: &Path) -> Category {
    let raw = path.to_string_lossy();
    let normalized = normalize_path(&raw);

    for (prefix, cat) in PREFIX_RULES {
        if path_is_or_under(&normalized, prefix) {
            return *cat;
        }
    }

    if macos_developer_cache(&normalized) {
        return Category::Cache;
    }
    if has_component(&normalized, "DiagnosticReports")
        || has_component(&normalized, "CrashReporter")
    {
        return Category::Crash;
    }
    if has_component(&normalized, ".cache") || has_component(&normalized, "Caches") {
        return Category::Cache;
    }
    if has_component(&normalized, "Logs") && has_component(&normalized, "Library") {
        return Category::Log;
    }
    if has_component(&normalized, "Logs") && has_component_ci(&normalized, "Windows") {
        return Category::Log;
    }
    if has_component(&normalized, "thumbnails") && has_component(&normalized, ".cache")
        || has_component(&normalized, "thumbnails") && normalized.contains("/.thumbnails")
    {
        return Category::Cache;
    }
    if normalized.contains("/.thumbnails/") || normalized.ends_with("/.thumbnails") {
        return Category::Cache;
    }
    if is_trash(&normalized) {
        return Category::Temp;
    }
    if has_component(&normalized, ".TemporaryItems")
        || has_component(&normalized, ".Spotlight-V100")
    {
        return if has_component(&normalized, ".Spotlight-V100") {
            Category::Cache
        } else {
            Category::Temp
        };
    }
    if has_component_ci(&normalized, "Temp")
        && (has_component_ci(&normalized, "AppData")
            || has_component_ci(&normalized, "Windows")
            || has_component_ci(&normalized, "Local"))
    {
        return Category::Temp;
    }
    if windows_update_cache(&normalized) {
        return Category::Cache;
    }
    if windows_crash(&normalized) {
        return Category::Crash;
    }

    // Package-manager cache dirs that sometimes live outside /var/cache.
    if has_component(&normalized, "apt") && has_component(&normalized, "archives") {
        return Category::Cache;
    }
    if ends_with_component(&normalized, "pacman") && normalized.contains("/cache") {
        return Category::Cache;
    }
    if has_component(&normalized, ".local") && has_component(&normalized, "Trash") {
        return Category::Temp;
    }

    if looks_like_core_dump(&normalized) {
        return Category::Crash;
    }

    Category::Normal
}

/// Xcode, simulators, and device support — the usual 20–80 GB on a Mac
/// that still looks like ordinary `Developer/` without these names.
fn macos_developer_cache(path: &str) -> bool {
    has_component(path, "DerivedData")
        || has_component(path, "CoreSimulator")
        || has_component(path, "iOS DeviceSupport")
        || has_component(path, "DeveloperDiskImages")
        || (has_component(path, "DocumentationCache") && has_component(path, "Developer"))
}

fn is_trash(path: &str) -> bool {
    has_component_ci(path, "$Recycle.Bin")
        || path
            .split('/')
            .any(|c| c == ".Trash" || c == ".Trashes" || c.starts_with(".Trash-"))
}

fn windows_update_cache(path: &str) -> bool {
    if has_component_ci(path, "Windows.old")
        || has_component_ci(path, "$Windows.~BT")
        || has_component_ci(path, "$Windows.~WS")
        || has_component_ci(path, "DeliveryOptimization")
        || has_component_ci(path, "INetCache")
        || has_component_ci(path, "Package Cache")
    {
        return true;
    }
    if has_component_ci(path, "Prefetch") && has_component_ci(path, "Windows") {
        return true;
    }
    if has_component_ci(path, "SoftwareDistribution") && has_component_ci(path, "Download") {
        return true;
    }
    false
}

fn windows_crash(path: &str) -> bool {
    if has_component_ci(path, "Minidump") || has_component_ci(path, "CrashDumps") {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or("");
    name.eq_ignore_ascii_case("MEMORY.DMP")
}

fn normalize_path(raw: &str) -> String {
    let mut s = raw.replace('\\', "/");
    if s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

fn path_is_or_under(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

fn has_component(path: &str, name: &str) -> bool {
    path.split('/').any(|c| c == name)
}

fn has_component_ci(path: &str, name: &str) -> bool {
    path.split('/').any(|c| c.eq_ignore_ascii_case(name))
}

fn ends_with_component(path: &str, name: &str) -> bool {
    path.rsplit('/').next().is_some_and(|c| c == name)
}

fn looks_like_core_dump(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or("");
    name == "core" || name.starts_with("core.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn well_known_temp_and_cache_paths() {
        assert_eq!(classify(Path::new("/tmp")), Category::Temp);
        assert_eq!(classify(Path::new("/tmp/x")), Category::Temp);
        assert_eq!(classify(Path::new("/var/tmp/foo")), Category::Temp);
        assert_eq!(classify(Path::new("/var/cache")), Category::Cache);
        assert_eq!(
            classify(Path::new("/var/cache/apt/archives")),
            Category::Cache
        );
        assert_eq!(classify(Path::new("/var/cache/dnf")), Category::Cache);
        assert_eq!(classify(Path::new("/var/cache/yum")), Category::Cache);
        assert_eq!(
            classify(Path::new("/var/cache/pacman/pkg")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new("/home/zach/.cache/thumbnails")),
            Category::Cache
        );
        assert_eq!(classify(Path::new("/root/.cache")), Category::Cache);
    }

    #[test]
    fn logs_journals_crashes() {
        assert_eq!(classify(Path::new("/var/log/syslog")), Category::Log);
        assert_eq!(
            classify(Path::new("/var/log/journal/machine/system.journal")),
            Category::Journal
        );
        assert_eq!(
            classify(Path::new("/var/lib/systemd/coredump/core.foo")),
            Category::Crash
        );
        assert_eq!(classify(Path::new("/var/crash/foo")), Category::Crash);
        assert_eq!(classify(Path::new("/home/zach/core.1234")), Category::Crash);
    }

    #[test]
    fn ordinary_paths_are_normal() {
        assert_eq!(classify(Path::new("/home/zach/src")), Category::Normal);
        assert_eq!(classify(Path::new("/opt/app")), Category::Normal);
        assert_eq!(classify(Path::new("/tmpfoo")), Category::Normal);
        assert_eq!(classify(Path::new("/var/lib/docker")), Category::Normal);
    }

    #[test]
    fn macos_and_windows_waste_paths() {
        assert_eq!(classify(Path::new("/private/tmp/x")), Category::Temp);
        assert_eq!(
            classify(Path::new("/private/var/folders/zz")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/Users/zach/Library/Caches/com.foo")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\Users\zach\AppData\Local\Temp\foo")),
            Category::Temp
        );
        assert_eq!(classify(Path::new(r"C:\Windows\Temp\bar")), Category::Temp);
        assert_eq!(
            classify(Path::new(r"C:\$Recycle.Bin\S-1-5-18")),
            Category::Temp
        );
    }

    #[test]
    fn macos_developer_and_crash_paths() {
        assert_eq!(
            classify(Path::new(
                "/Users/zach/Library/Developer/Xcode/DerivedData/rings-abc"
            )),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(
                "/Users/zach/Library/Developer/CoreSimulator/Devices"
            )),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(
                "/Users/zach/Library/Developer/Xcode/iOS DeviceSupport/18.0"
            )),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(
                "/Users/zach/Library/Logs/DiagnosticReports/rings.crash"
            )),
            Category::Crash
        );
        assert_eq!(
            classify(Path::new("/private/var/vm/sleepimage")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/Volumes/SSD/.Trashes/501")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/Volumes/SSD/.Spotlight-V100")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new("/Users/zach/Documents/project")),
            Category::Normal
        );
    }

    #[test]
    fn windows_update_and_crash_paths() {
        assert_eq!(
            classify(Path::new(r"C:\Windows.old\Windows")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\$Windows.~BT\Sources")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\SoftwareDistribution\Download\abc")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\Prefetch\RINGS.EXE-123.pf")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\Minidump\012345.dmp")),
            Category::Crash
        );
        assert_eq!(
            classify(Path::new(
                r"C:\Users\zach\AppData\Local\CrashDumps\rings.exe.dmp"
            )),
            Category::Crash
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\MEMORY.DMP")),
            Category::Crash
        );
        assert_eq!(
            classify(Path::new(
                r"C:\Users\zach\AppData\Local\Microsoft\Windows\INetCache\ie"
            )),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\Logs\CBS\cbs.log")),
            Category::Log
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\System32\drivers")),
            Category::Normal
        );
        assert_eq!(
            classify(Path::new(r"C:\Windows\WinSxS\manifests")),
            Category::Normal,
            "WinSxS is the component store, not waste"
        );
    }

    #[test]
    fn linux_package_and_trash_paths() {
        assert_eq!(
            classify(Path::new("/var/lib/apt/lists/deb.debian.org_dists")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new("/var/lib/snapd/cache/deadbeef")),
            Category::Cache
        );
        assert_eq!(
            classify(Path::new("/home/zach/.local/share/Trash/files/old")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/media/usb/.Trash-1000/files/x")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/var/lib/docker/tmp/buildkit")),
            Category::Temp
        );
        assert_eq!(
            classify(Path::new("/var/lib/docker/overlay2")),
            Category::Normal,
            "container layers are not waste"
        );
    }
}
