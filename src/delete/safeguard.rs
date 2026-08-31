//! Refuse deletes that would take down the machine or this tool.

use std::path::{Path, PathBuf};

#[cfg(unix)]
use crate::constants::{SAFEGUARD_EXACT, SAFEGUARD_PREFIXES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeguardRefuse {
    pub path: PathBuf,
    pub reason: String,
}

pub fn refuse_reason(path: &Path) -> Option<SafeguardRefuse> {
    let canon = canonicalize_best_effort(path);
    let raw = display_abs(&canon);

    if is_system_root(&raw) {
        return Some(SafeguardRefuse {
            path: canon,
            reason: format!("{raw} is a safeguarded system path"),
        });
    }

    #[cfg(unix)]
    {
        for exact in SAFEGUARD_EXACT {
            if path_eq(&raw, exact) {
                return Some(SafeguardRefuse {
                    path: canon,
                    reason: format!("{exact} is a safeguarded system path"),
                });
            }
        }
        for prefix in SAFEGUARD_PREFIXES {
            if path_starts(&raw, prefix) {
                return Some(SafeguardRefuse {
                    path: canon.clone(),
                    reason: format!(
                        "{raw} is under safeguarded prefix {}",
                        prefix.trim_end_matches('/')
                    ),
                });
            }
        }
    }

    #[cfg(windows)]
    {
        if let Some(reason) = windows_refuse(&raw) {
            return Some(SafeguardRefuse {
                path: canon,
                reason,
            });
        }
    }

    if let Some(reason) = kernel_refuse(&raw) {
        return Some(SafeguardRefuse {
            path: canon.clone(),
            reason,
        });
    }

    if let Ok(exe) = std::env::current_exe() {
        let exe_c = canonicalize_best_effort(&exe);
        if paths_equal(&canon, &exe_c) {
            return Some(SafeguardRefuse {
                path: canon,
                reason: "refusing to delete the running rings binary".into(),
            });
        }
    }

    None
}

pub fn is_safeguarded(path: &Path) -> bool {
    refuse_reason(path).is_some()
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    fs_canonicalize(path).unwrap_or_else(|| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    })
}

fn fs_canonicalize(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok()
}

fn display_abs(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.is_empty() {
        return if cfg!(windows) {
            String::new()
        } else {
            "/".into()
        };
    }
    #[cfg(windows)]
    {
        let mut t = s.replace('/', "\\");
        if let Some(stripped) = t.strip_prefix(r"\\?\") {
            t = stripped.to_string();
        }
        if t.len() > 3 && t.ends_with('\\') {
            t.pop();
        }
        return t;
    }
    #[cfg(not(windows))]
    {
        if s != "/" && s.ends_with('/') {
            s.trim_end_matches('/').to_string()
        } else {
            s.into_owned()
        }
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b || path_eq(&display_abs(a), &display_abs(b))
}

fn path_eq(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn path_starts(a: &str, prefix: &str) -> bool {
    if cfg!(windows) {
        a.len() >= prefix.len() && a[..prefix.len()].eq_ignore_ascii_case(prefix)
    } else {
        a.starts_with(prefix)
    }
}

fn is_system_root(raw: &str) -> bool {
    if raw == "/" {
        return true;
    }
    // `C:` or `C:\` (any drive letter)
    let b = raw.as_bytes();
    matches!(b, [d, b':'] | [d, b':', b'\\' | b'/'] if d.is_ascii_alphabetic())
}

#[cfg(windows)]
fn windows_refuse(raw: &str) -> Option<String> {
    const EXACT: &[&str] = &[
        r"C:\Windows",
        r"C:\Program Files",
        r"C:\Program Files (x86)",
        r"C:\Users",
        r"C:\ProgramData",
    ];
    const PREFIXES: &[&str] = &[
        r"C:\Windows\",
        r"C:\Program Files\",
        r"C:\Program Files (x86)\",
    ];
    for exact in EXACT {
        if path_eq(raw, exact) {
            return Some(format!("{exact} is a safeguarded system path"));
        }
    }
    for prefix in PREFIXES {
        if path_starts(raw, prefix) {
            return Some(format!(
                "{raw} is under safeguarded prefix {}",
                prefix.trim_end_matches('\\')
            ));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn kernel_release() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(not(target_os = "linux"))]
fn kernel_release() -> Option<String> {
    None
}

fn kernel_refuse(raw: &str) -> Option<String> {
    let rel = kernel_release()?;
    let extra = [
        format!("/boot/vmlinuz-{rel}"),
        format!("/boot/vmlinux-{rel}"),
        format!("/boot/initrd.img-{rel}"),
        format!("/boot/initramfs-{rel}.img"),
        format!("/lib/modules/{rel}"),
    ];
    for p in extra {
        if raw == p || raw.starts_with(&format!("{p}/")) {
            return Some(format!("{raw} is the running kernel ({rel})"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn refuses_obvious_system_paths() {
        for p in ["/", "/boot", "/etc", "/usr", "/bin", "/sbin"] {
            let r = refuse_reason(Path::new(p)).expect(p);
            assert!(
                r.reason.contains("safeguarded") || r.reason.contains("kernel"),
                "{}: {}",
                p,
                r.reason
            );
        }
        assert!(refuse_reason(Path::new("/etc/passwd")).is_some());
        assert!(refuse_reason(Path::new("/usr/bin/bash")).is_some());
        assert!(refuse_reason(Path::new("/boot/grub")).is_some());
    }

    #[cfg(unix)]
    #[test]
    fn allows_typical_waste_paths() {
        assert!(refuse_reason(Path::new("/tmp/foo")).is_none());
        assert!(refuse_reason(Path::new("/var/cache/apt")).is_none());
        assert!(refuse_reason(Path::new("/var/log/old.log")).is_none());
        assert!(refuse_reason(Path::new("/home/zach/.cache")).is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_macos_system_roots() {
        assert!(refuse_reason(Path::new("/System")).is_some());
        assert!(refuse_reason(Path::new("/System/Library")).is_some());
        assert!(refuse_reason(Path::new("/private/etc/hosts")).is_some());
        assert!(refuse_reason(Path::new("/private/tmp/x")).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn refuses_windows_system_paths() {
        assert!(refuse_reason(Path::new(r"C:\")).is_some());
        assert!(refuse_reason(Path::new(r"C:\Windows")).is_some());
        assert!(refuse_reason(Path::new(r"C:\Windows\System32\cmd.exe")).is_some());
        assert!(refuse_reason(Path::new(r"C:\Program Files")).is_some());
        assert!(refuse_reason(Path::new(r"C:\Users")).is_some());
        assert!(refuse_reason(Path::new(r"C:\Users\zach\AppData\Local\Temp\x")).is_none());
        assert!(refuse_reason(Path::new(r"D:\scratch")).is_none());
    }

    #[test]
    fn refuses_own_binary() {
        let exe = std::env::current_exe().unwrap();
        let r = refuse_reason(&exe).expect("own binary");
        assert!(r.reason.contains("rings binary"), "{}", r.reason);
    }
}
