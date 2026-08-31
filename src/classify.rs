//! Tag common Linux server waste: temp, cache, logs, journals, crash dumps.

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
const PREFIX_RULES: &[(&str, Category)] = &[
    ("/var/log/journal", Category::Journal),
    ("/run/log/journal", Category::Journal),
    ("/var/lib/systemd/coredump", Category::Crash),
    ("/var/lib/apport/coredump", Category::Crash),
    ("/var/crash", Category::Crash),
    ("/var/cache", Category::Cache),
    ("/var/log", Category::Log),
    ("/var/tmp", Category::Temp),
    ("/tmp", Category::Temp),
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

    if has_component(&normalized, ".cache") {
        return Category::Cache;
    }
    if has_component(&normalized, "thumbnails") && has_component(&normalized, ".cache")
        || has_component(&normalized, "thumbnails") && normalized.contains("/.thumbnails")
    {
        return Category::Cache;
    }
    if normalized.contains("/.thumbnails/") || normalized.ends_with("/.thumbnails") {
        return Category::Cache;
    }

    // Package-manager cache dirs that sometimes live outside /var/cache.
    if has_component(&normalized, "apt") && has_component(&normalized, "archives") {
        return Category::Cache;
    }
    if ends_with_component(&normalized, "pacman") && normalized.contains("/cache") {
        return Category::Cache;
    }

    if looks_like_core_dump(&normalized) {
        return Category::Crash;
    }

    Category::Normal
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
        assert_eq!(classify(Path::new("/var/cache/apt/archives")), Category::Cache);
        assert_eq!(classify(Path::new("/var/cache/dnf")), Category::Cache);
        assert_eq!(classify(Path::new("/var/cache/yum")), Category::Cache);
        assert_eq!(classify(Path::new("/var/cache/pacman/pkg")), Category::Cache);
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
}
