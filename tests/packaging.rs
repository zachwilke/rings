//! Smoke tests for distro packaging (AUR metadata + debian/build-deb.sh).

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn cargo_version() -> String {
    let toml = std::fs::read_to_string(repo_root().join("Cargo.toml")).unwrap();
    toml.lines()
        .find_map(|line| {
            line.strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
                .map(str::to_string)
        })
        .expect("version in Cargo.toml")
}

#[test]
fn aur_pkgbuild_is_bin_and_matches_crate_version() {
    let pkgbuild = std::fs::read_to_string(repo_root().join("packaging/aur/rings-bin/PKGBUILD"))
        .expect("PKGBUILD");
    let srcinfo = std::fs::read_to_string(repo_root().join("packaging/aur/rings-bin/.SRCINFO"))
        .expect(".SRCINFO");
    let version = cargo_version();

    assert!(
        pkgbuild.contains("pkgname=rings-bin"),
        "pkgname must be rings-bin"
    );
    assert!(
        pkgbuild.contains(&format!("pkgver={version}")),
        "PKGBUILD pkgver must match Cargo.toml ({version})"
    );
    assert!(
        srcinfo.contains(&format!("pkgver = {version}")),
        ".SRCINFO pkgver must match Cargo.toml ({version})"
    );
    assert!(
        pkgbuild.contains("provides=('rings')") && pkgbuild.contains("conflicts=('rings')"),
        "provides/conflicts rings"
    );
    assert!(
        pkgbuild.contains("license=('MIT')"),
        "MIT license"
    );
    let has_rust_dep = pkgbuild.lines().any(|line| {
        let t = line.trim();
        !t.starts_with('#')
            && (t.starts_with("depends=") || t.starts_with("makedepends="))
            && (t.contains("rust") || t.contains("cargo"))
    });
    assert!(!has_rust_dep, "rings-bin must not depend on rust/cargo");
    assert!(
        pkgbuild.contains("source_x86_64=")
            && pkgbuild.contains("source_aarch64=")
            && pkgbuild.contains("source_armv7h="),
        "per-arch musl sources"
    );
    assert!(
        pkgbuild.contains("/usr/bin/rings")
            && pkgbuild.contains("/usr/share/licenses/$pkgname/LICENSE"),
        "install binary and LICENSE"
    );
}

#[cfg(unix)]
#[test]
fn build_deb_sh_syntax() {
    let status = Command::new("sh")
        .arg("-n")
        .arg(repo_root().join("packaging/debian/build-deb.sh"))
        .status()
        .expect("sh -n build-deb.sh");
    assert!(status.success(), "sh -n packaging/debian/build-deb.sh failed");
}

#[cfg(unix)]
#[test]
fn build_deb_wraps_dummy_binary_without_libc_depends() {
    let dpkg = Command::new("dpkg-deb").arg("--version").status();
    match dpkg {
        Ok(status) if status.success() => {}
        _ => return, // macOS CI has no dpkg-deb
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let dummy = tmp.path().join("rings-dummy");
    std::fs::write(&dummy, b"#!/bin/sh\necho rings-smoke\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dummy, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let version = cargo_version();
    let out = Command::new("sh")
        .arg(repo_root().join("packaging/debian/build-deb.sh"))
        .args([&version, "amd64"])
        .arg(&dummy)
        .env("OUTDIR", tmp.path())
        .output()
        .expect("run build-deb.sh");
    assert!(
        out.status.success(),
        "build-deb.sh failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let deb = tmp.path().join(format!("rings_{version}_amd64.deb"));
    assert!(deb.is_file(), "expected {}", deb.display());

    let info = Command::new("dpkg-deb")
        .args(["-I", deb.to_str().unwrap()])
        .output()
        .expect("dpkg-deb -I");
    assert!(info.status.success());
    let info_txt = String::from_utf8_lossy(&info.stdout);
    assert!(info_txt.contains("Package: rings"), "{info_txt}");
    assert!(info_txt.contains("Architecture: amd64"), "{info_txt}");
    assert!(info_txt.contains("Section: utils"), "{info_txt}");
    assert!(info_txt.contains("Priority: optional"), "{info_txt}");
    assert!(
        info_txt.contains("Zach Wilke <zach@pinefall.dev>"),
        "{info_txt}"
    );
    assert!(
        info_txt.contains("https://github.com/zachwilke/rings"),
        "{info_txt}"
    );
    assert!(
        !info_txt.to_ascii_lowercase().contains("libc"),
        "static musl .deb must not depend on libc:\n{info_txt}"
    );

    let depends = Command::new("dpkg-deb")
        .args(["-f", deb.to_str().unwrap(), "Depends"])
        .output()
        .expect("dpkg-deb -f Depends");
    assert!(
        String::from_utf8_lossy(&depends.stdout).trim().is_empty(),
        "Depends should be empty, got {:?}",
        String::from_utf8_lossy(&depends.stdout)
    );

    let listing = Command::new("dpkg-deb")
        .args(["-c", deb.to_str().unwrap()])
        .output()
        .expect("dpkg-deb -c");
    let listing_txt = String::from_utf8_lossy(&listing.stdout);
    assert!(
        listing_txt.contains("usr/bin/rings"),
        "missing binary:\n{listing_txt}"
    );
    assert!(
        listing_txt.contains("usr/share/doc/rings/copyright"),
        "missing copyright:\n{listing_txt}"
    );
}

#[cfg(unix)]
#[test]
fn build_deb_rejects_unknown_arch() {
    let tmp = tempfile::TempDir::new().unwrap();
    let dummy = tmp.path().join("rings-dummy");
    std::fs::write(&dummy, b"x").unwrap();
    let out = Command::new("sh")
        .arg(repo_root().join("packaging/debian/build-deb.sh"))
        .args(["0.2.0", "riscv64"])
        .arg(&dummy)
        .output()
        .expect("run build-deb.sh");
    assert!(!out.status.success(), "riscv64 must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("amd64") || err.contains("ARCH"),
        "stderr should mention valid arches:\n{err}"
    );
}
