//! Refuse deletes that would take down the machine or this tool.

use std::path::{Path, PathBuf};

use crate::constants::{SAFEGUARD_EXACT, SAFEGUARD_PREFIXES};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeguardRefuse {
    pub path: PathBuf,
    pub reason: String,
}

pub fn refuse_reason(path: &Path) -> Option<SafeguardRefuse> {
    let canon = canonicalize_best_effort(path);
    let raw = display_abs(&canon);

    for exact in SAFEGUARD_EXACT {
        if raw == *exact {
            return Some(SafeguardRefuse {
                path: canon,
                reason: format!("{exact} is a safeguarded system path"),
            });
        }
    }
    for prefix in SAFEGUARD_PREFIXES {
        if raw.starts_with(prefix) {
            return Some(SafeguardRefuse {
                path: canon.clone(),
                reason: format!("{raw} is under safeguarded prefix {}", prefix.trim_end_matches('/')),
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
        return "/".into();
    }
    if s != "/" && s.ends_with('/') {
        s.trim_end_matches('/').to_string()
    } else {
        s.into_owned()
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b || display_abs(a) == display_abs(b)
}

fn kernel_release() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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

    #[test]
    fn allows_typical_waste_paths() {
        assert!(refuse_reason(Path::new("/tmp/foo")).is_none());
        assert!(refuse_reason(Path::new("/var/cache/apt")).is_none());
        assert!(refuse_reason(Path::new("/var/log/old.log")).is_none());
        assert!(refuse_reason(Path::new("/home/zach/.cache")).is_none());
    }

    #[test]
    fn refuses_own_binary() {
        let exe = std::env::current_exe().unwrap();
        let r = refuse_reason(&exe).expect("own binary");
        assert!(r.reason.contains("rings binary"), "{}", r.reason);
    }
}
