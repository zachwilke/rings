//! SQLite recognition by file header.
//!
//! Extensions are useless here — real SQLite databases ship as `.db`,
//! `.sqlite`, `.sqlite3`, `.anki2`, and with no extension at all (Chrome's
//! `History`, `Cookies`). The 16-byte magic is the only reliable test, and
//! the same 100-byte read that identifies the file also reports its free
//! space, so detection and analysis cost one `read`.
//!
//! Header layout is fixed for every SQLite version ever released
//! (<https://sqlite.org/fileformat.html#the_database_header>), big-endian:
//!
//! ```text
//!   0..16  magic "SQLite format 3\0"
//!  16..18  page size in bytes (the value 1 means 65536)
//!  24..28  file change counter
//!  28..32  database size in pages ("in-header database size")
//!  36..40  total number of freelist pages
//!  92..96  version-valid-for number
//! ```
//!
//! The in-header page count is trustworthy only when the change counter and
//! the version-valid-for number agree; otherwise the file was last written by
//! a pre-3.7.0 library and the count is derived from the file length instead.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use super::{Guidance, Reclaim, Role};

/// A lone SQLite file is a file, not a service. Nothing here is protected:
/// a stale 4 GB Electron cache database is exactly the sort of thing a
/// person points rings at in order to delete, and the generic confirm flow
/// already covers it. What rings adds is the measurement and the advice.
pub fn guidance(role: Role) -> Guidance {
    match role {
        Role::Data => Guidance {
            reclaim: Reclaim::Command,
            protected: false,
            why: "database file; pages freed by DELETE stay in it until it is rebuilt",
            action: "VACUUM returns the freelist to the filesystem \u{2014} or remove the file outright if the database is disposable",
        },
        Role::Wal => Guidance {
            reclaim: Reclaim::Command,
            protected: false,
            why: "write-ahead log that has outgrown its checkpoint",
            action: "PRAGMA wal_checkpoint(TRUNCATE) folds it back into the database",
        },
        Role::Meta => Guidance {
            reclaim: Reclaim::Never,
            protected: false,
            why: "shared-memory or rollback sidecar for an open database",
            action: "disappears on its own once nothing has the database open",
        },
        Role::TempSpill | Role::Log | Role::Binlog | Role::Backup => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "SQLite scratch file",
            action: "safe to remove once nothing has the database open",
        },
    }
}

pub const MAGIC: &[u8; 16] = b"SQLite format 3\0";

/// Bytes that must be read to identify and measure a database.
pub const HEADER_BYTES: usize = 100;

/// Smallest and largest legal SQLite page.
const PAGE_MIN: u32 = 512;
const PAGE_MAX: u32 = 65_536;

/// What a SQLite header says about the file's size and free space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub page_size: u32,
    pub page_count: u32,
    pub freelist_pages: u32,
    /// False when the page count was derived from the file length because the
    /// in-header count could not be trusted.
    pub page_count_exact: bool,
}

impl Header {
    /// Exactly what `VACUUM` would return to the filesystem.
    pub fn freelist_bytes(&self) -> u64 {
        u64::from(self.freelist_pages) * u64::from(self.page_size)
    }

    /// Size the header accounts for. May trail the real file length when a
    /// hot journal has not been rolled back.
    pub fn total_bytes(&self) -> u64 {
        u64::from(self.page_count) * u64::from(self.page_size)
    }

    /// Share of the file that is freelist, 0.0–100.0.
    pub fn free_percent(&self) -> f64 {
        if self.page_count == 0 {
            return 0.0;
        }
        f64::from(self.freelist_pages) * 100.0 / f64::from(self.page_count)
    }

    /// Compact description for the databases view.
    pub fn summary(&self) -> String {
        format!(
            "{} B pages · {} pages · {:.0}% freelist",
            self.page_size,
            crate::size::group_u64(u64::from(self.page_count)),
            self.free_percent()
        )
    }
}

fn be16(b: &[u8], at: usize) -> u16 {
    (u16::from(b[at]) << 8) | u16::from(b[at + 1])
}

fn be32(b: &[u8], at: usize) -> u32 {
    (u32::from(b[at]) << 24)
        | (u32::from(b[at + 1]) << 16)
        | (u32::from(b[at + 2]) << 8)
        | u32::from(b[at + 3])
}

/// Parse a header. `file_len` is the fallback for an untrustworthy page count.
/// Returns `None` for anything that is not a SQLite database.
pub fn parse(bytes: &[u8], file_len: u64) -> Option<Header> {
    if bytes.len() < HEADER_BYTES || &bytes[..16] != MAGIC.as_slice() {
        return None;
    }
    // Every index below is < HEADER_BYTES, so the slicing is in bounds.
    let raw = be16(bytes, 16);
    let page_size = if raw == 1 { PAGE_MAX } else { u32::from(raw) };
    if page_size < PAGE_MIN || page_size > PAGE_MAX || !page_size.is_power_of_two() {
        return None;
    }

    let in_header = be32(bytes, 28);
    let change_counter = be32(bytes, 24);
    let valid_for = be32(bytes, 92);
    let page_count_exact = in_header != 0 && change_counter == valid_for;

    let from_len = (file_len / u64::from(page_size)).min(u64::from(u32::MAX)) as u32;
    let page_count = if page_count_exact { in_header } else { from_len };

    // A freelist longer than the database means a corrupt or truncated
    // header; clamp rather than report a reclaim larger than the file.
    let freelist_pages = be32(bytes, 36).min(page_count);

    Some(Header {
        page_size,
        page_count,
        freelist_pages,
        page_count_exact,
    })
}

/// Read and parse the header of `path`. Silent on any I/O error: an
/// unreadable file is simply not a database as far as the scan is concerned.
pub fn probe(path: &Path, file_len: u64) -> Option<Header> {
    let head = read_head(path)?;
    parse(&head, file_len)
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    let mut f = File::open(path).ok()?;
    let mut buf = vec![0u8; HEADER_BYTES];
    let mut filled = 0usize;
    while filled < HEADER_BYTES {
        match f.read(&mut buf[filled..]) {
            Ok(0) => return None, // shorter than a header
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
    Some(buf)
}

/// The three files SQLite parks beside a database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sidecar {
    Wal,
    Shm,
    Journal,
}

impl Sidecar {
    pub fn role(self) -> Role {
        match self {
            // A WAL that has outgrown its checkpoint is reclaimable — but by
            // checkpointing, never by unlinking.
            Sidecar::Wal => Role::Wal,
            // Removing either of these under a live database loses committed
            // transactions, so both are protected.
            Sidecar::Shm | Sidecar::Journal => Role::Meta,
        }
    }
}

/// Split `foo.db-wal` into (`foo.db`, [`Sidecar::Wal`]).
pub fn sidecar_of(name: &str) -> Option<(&str, Sidecar)> {
    const SUFFIXES: &[(&str, Sidecar)] = &[
        ("-wal", Sidecar::Wal),
        ("-shm", Sidecar::Shm),
        ("-journal", Sidecar::Journal),
    ];
    for (suffix, kind) in SUFFIXES {
        if let Some(base) = name.strip_suffix(suffix) {
            if !base.is_empty() {
                return Some((base, *kind));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal well-formed header. `exact` controls whether the in-header
    /// page count agrees with the change counter.
    fn header_bytes(page_size: u16, pages: u32, freelist: u32, exact: bool) -> Vec<u8> {
        let mut b = vec![0u8; HEADER_BYTES];
        b[..16].copy_from_slice(MAGIC.as_slice());
        b[16..18].copy_from_slice(&page_size.to_be_bytes());
        b[24..28].copy_from_slice(&7u32.to_be_bytes()); // change counter
        b[28..32].copy_from_slice(&pages.to_be_bytes());
        b[36..40].copy_from_slice(&freelist.to_be_bytes());
        b[92..96].copy_from_slice(&if exact { 7u32 } else { 6u32 }.to_be_bytes());
        b
    }

    #[test]
    fn rejects_anything_without_the_magic() {
        assert!(parse(&[0u8; HEADER_BYTES], 4096).is_none());
        assert!(parse(b"not a database", 4096).is_none(), "too short");
        let mut b = header_bytes(4096, 10, 0, true);
        b[3] = b'X';
        assert!(parse(&b, 40960).is_none(), "corrupted magic");
    }

    #[test]
    fn reads_page_size_count_and_freelist() {
        let b = header_bytes(4096, 1000, 380, true);
        let h = parse(&b, 4_096_000).expect("valid header");
        assert_eq!(h.page_size, 4096);
        assert_eq!(h.page_count, 1000);
        assert_eq!(h.freelist_pages, 380);
        assert!(h.page_count_exact);
        // The number the whole feature exists to report.
        assert_eq!(h.freelist_bytes(), 380 * 4096);
        assert_eq!(h.total_bytes(), 1000 * 4096);
        assert!((h.free_percent() - 38.0).abs() < 0.001);
    }

    #[test]
    fn page_size_one_means_65536() {
        let b = header_bytes(1, 4, 1, true);
        let h = parse(&b, 65_536 * 4).expect("64 KiB pages are legal");
        assert_eq!(h.page_size, 65_536);
        assert_eq!(h.freelist_bytes(), 65_536);
    }

    #[test]
    fn rejects_impossible_page_sizes() {
        for bad in [0u16, 3, 100, 511, 1000] {
            let b = header_bytes(bad, 4, 0, true);
            assert!(parse(&b, 4096).is_none(), "page size {bad} must be refused");
        }
    }

    #[test]
    fn falls_back_to_file_length_when_the_count_is_stale() {
        // Pre-3.7.0 writer: in-header count must not be believed.
        let b = header_bytes(4096, 999_999, 10, false);
        let h = parse(&b, 4096 * 50).expect("valid header");
        assert!(!h.page_count_exact);
        assert_eq!(h.page_count, 50, "derived from the file length");
    }

    #[test]
    fn freelist_is_clamped_to_the_database() {
        // A truncated file must never report reclaiming more than it holds.
        let b = header_bytes(4096, 10, 9_999, true);
        let h = parse(&b, 4096 * 10).expect("valid header");
        assert_eq!(h.freelist_pages, 10);
        assert!(h.freelist_bytes() <= h.total_bytes());
    }

    #[test]
    fn probe_reads_a_real_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("places.sqlite");
        let mut bytes = header_bytes(4096, 20, 5, true);
        bytes.resize(4096 * 20, 0);
        std::fs::write(&path, &bytes).unwrap();

        let h = probe(&path, bytes.len() as u64).expect("header round-trips through the fs");
        assert_eq!(h.page_size, 4096);
        assert_eq!(h.freelist_bytes(), 5 * 4096);

        // A file that merely looks database-shaped is not one.
        let decoy = tmp.path().join("notes.db");
        std::fs::write(&decoy, vec![b'z'; 8192]).unwrap();
        assert!(probe(&decoy, 8192).is_none());
    }

    #[test]
    fn recognises_sidecars() {
        assert_eq!(sidecar_of("places.sqlite-wal"), Some(("places.sqlite", Sidecar::Wal)));
        assert_eq!(sidecar_of("places.sqlite-shm"), Some(("places.sqlite", Sidecar::Shm)));
        assert_eq!(sidecar_of("app.db-journal"), Some(("app.db", Sidecar::Journal)));
        assert_eq!(sidecar_of("places.sqlite"), None);
        assert_eq!(sidecar_of("-wal"), None, "a bare suffix has no base");
        // Only the WAL is reclaimable, and only by checkpointing.
        assert_eq!(Sidecar::Wal.role(), Role::Wal);
        assert_eq!(Sidecar::Shm.role(), Role::Meta);
    }
}
