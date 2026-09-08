//! macOS `getattrlistbulk` directory reader.
//!
//! One syscall returns many names plus file sizes. Directories, firmlink-ish
//! gaps, and anything missing a returned attribute are `lstat`'d so used-byte
//! accounting matches the portable walker. Set `RINGS_SCAN_NO_BULK=1` to
//! force the std `readdir` + `fstatat` path.

use std::fs::{self, File};
use std::io::{self, Error, ErrorKind};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use crate::scan::entry::{note_error, DirEntryInfo};
use crate::scan::tree::ScanStats;

const ATTR_BIT_MAP_COUNT: u16 = 5;
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_DEVID: u32 = 0x0000_0002;
const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
const ATTR_CMN_FILEID: u32 = 0x0200_0000;
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;

const ATTR_FILE_LINKCOUNT: u32 = 0x0000_0001;
const ATTR_FILE_TOTALSIZE: u32 = 0x0000_0002;
const ATTR_FILE_ALLOCSIZE: u32 = 0x0000_0004;

const VREG: u32 = 1;
const VDIR: u32 = 2;
const VLNK: u32 = 5;

const BUF_SIZE: usize = 64 * 1024;

#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AttributeSet {
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

extern "C" {
    fn getattrlistbulk(
        dirfd: i32,
        alist: *mut AttrList,
        attr_buf: *mut libc::c_void,
        buf_size: usize,
        options: u64,
    ) -> i32;
}

pub fn bulk_enabled() -> bool {
    !matches!(
        std::env::var("RINGS_SCAN_NO_BULK").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

pub fn read_dir_bulk(dir: &Path, stats: &mut ScanStats) -> io::Result<Vec<DirEntryInfo>> {
    let file = File::open(dir)?;
    let fd = file.as_raw_fd();
    let mut alist = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS
            | ATTR_CMN_NAME
            | ATTR_CMN_ERROR
            | ATTR_CMN_OBJTYPE
            | ATTR_CMN_DEVID
            | ATTR_CMN_FILEID,
        volattr: 0,
        dirattr: 0,
        fileattr: ATTR_FILE_LINKCOUNT | ATTR_FILE_TOTALSIZE | ATTR_FILE_ALLOCSIZE,
        forkattr: 0,
    };
    let mut buf = vec![0u8; BUF_SIZE];
    let mut out = Vec::new();

    loop {
        let n = unsafe {
            getattrlistbulk(
                fd,
                &mut alist,
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
            )
        };
        if n < 0 {
            let err = Error::last_os_error();
            if err.kind() == ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if n == 0 {
            break;
        }
        if !parse_buffer(&buf, n as usize, dir, stats, &mut out) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "getattrlistbulk parse failed",
            ));
        }
    }
    Ok(out)
}

struct Cursor<'a> {
    buf: &'a [u8],
    start: usize,
    end: usize,
    off: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8], start: usize, len: usize) -> Option<Self> {
        let end = start.checked_add(len)?;
        if end > buf.len() || len < 4 {
            return None;
        }
        Some(Self {
            buf,
            start,
            end,
            off: start + 4,
        })
    }

    fn align(&mut self, align: usize) {
        let rel = self.off - self.start;
        let padded = (rel + align - 1) & !(align - 1);
        self.off = self.start + padded;
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.off + n > self.end {
            return None;
        }
        let s = &self.buf[self.off..self.off + n];
        self.off += n;
        Some(s)
    }

    fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let b = self.take(4)?;
        Some(u32::from_ne_bytes(b.try_into().ok()?))
    }

    fn u64(&mut self) -> Option<u64> {
        self.align(8);
        let b = self.take(8)?;
        Some(u64::from_ne_bytes(b.try_into().ok()?))
    }

    fn attr_set(&mut self) -> Option<AttributeSet> {
        Some(AttributeSet {
            commonattr: self.u32()?,
            volattr: self.u32()?,
            dirattr: self.u32()?,
            fileattr: self.u32()?,
            forkattr: self.u32()?,
        })
    }

    fn name(&mut self) -> Option<Vec<u8>> {
        self.align(4);
        let ref_off = self.off;
        let data_offset = i32::from_ne_bytes(self.take(4)?.try_into().ok()?);
        let data_len = u32::from_ne_bytes(self.take(4)?.try_into().ok()?) as usize;
        let start = ref_off.checked_add_signed(data_offset as isize)?;
        let end = start.checked_add(data_len)?;
        if end > self.buf.len() {
            return None;
        }
        let mut bytes = &self.buf[start..end];
        if let Some(z) = bytes.iter().position(|&b| b == 0) {
            bytes = &bytes[..z];
        }
        if bytes.is_empty() || bytes == b"." || bytes == b".." {
            return Some(Vec::new());
        }
        Some(bytes.to_vec())
    }
}

struct BulkFields {
    name: Vec<u8>,
    obj_type: Option<u32>,
    dev: Option<u64>,
    ino: Option<u64>,
    apparent: Option<u64>,
    used: Option<u64>,
    nlink: Option<u64>,
}

fn parse_buffer(
    buf: &[u8],
    count: usize,
    dir: &Path,
    stats: &mut ScanStats,
    out: &mut Vec<DirEntryInfo>,
) -> bool {
    let mut pos = 0usize;
    for _ in 0..count {
        if pos + 4 > buf.len() {
            return false;
        }
        let len = u32::from_ne_bytes(buf[pos..pos + 4].try_into().unwrap()) as usize;
        if len < 4 || pos + len > buf.len() {
            return false;
        }
        match parse_entry(buf, pos, len) {
            Some(fields) => {
                if !fields.name.is_empty() {
                    push_fields(dir, fields, stats, out);
                }
            }
            None => return false,
        }
        pos += len;
    }
    true
}

fn parse_entry(buf: &[u8], start: usize, len: usize) -> Option<BulkFields> {
    let mut c = Cursor::new(buf, start, len)?;
    let returned = c.attr_set()?;

    let mut error = 0u32;
    if returned.commonattr & ATTR_CMN_ERROR != 0 {
        error = c.u32()?;
    }
    if error != 0 {
        return Some(BulkFields {
            name: Vec::new(),
            obj_type: None,
            dev: None,
            ino: None,
            apparent: None,
            used: None,
            nlink: None,
        });
    }

    let mut name = Vec::new();
    let mut obj_type = None;
    let mut dev = None;
    let mut ino = None;
    // Remaining common attrs in bit order, ERROR / RETURNED already consumed.
    if returned.commonattr & ATTR_CMN_NAME != 0 {
        name = c.name()?;
    }
    if returned.commonattr & ATTR_CMN_DEVID != 0 {
        dev = Some(c.u32()? as u64);
    }
    if returned.commonattr & ATTR_CMN_OBJTYPE != 0 {
        obj_type = Some(c.u32()?);
    }
    if returned.commonattr & ATTR_CMN_FILEID != 0 {
        ino = Some(c.u64()?);
    }

    let mut nlink = None;
    let mut apparent = None;
    let mut used = None;
    if returned.fileattr & ATTR_FILE_LINKCOUNT != 0 {
        nlink = Some(c.u32()? as u64);
    }
    if returned.fileattr & ATTR_FILE_TOTALSIZE != 0 {
        apparent = Some(c.u64()?);
    }
    if returned.fileattr & ATTR_FILE_ALLOCSIZE != 0 {
        used = Some(c.u64()?);
    }

    Some(BulkFields {
        name,
        obj_type,
        dev,
        ino,
        apparent,
        used,
        nlink,
    })
}

fn push_fields(
    dir: &Path,
    fields: BulkFields,
    stats: &mut ScanStats,
    out: &mut Vec<DirEntryInfo>,
) {
    let os = std::ffi::OsStr::from_bytes(&fields.name);
    let path = dir.join(os);
    let is_dir = fields.obj_type == Some(VDIR);
    let complete_file = matches!(fields.obj_type, Some(VREG) | Some(VLNK))
        && fields.used.is_some()
        && fields.apparent.is_some()
        && fields.dev.is_some();

    if is_dir || !complete_file {
        match fs::symlink_metadata(&path) {
            Ok(meta) => out.push(DirEntryInfo::from_meta(path, &meta)),
            Err(e) => note_error(stats, &e),
        }
        return;
    }

    out.push(DirEntryInfo {
        is_walkable_dir: false,
        used: fields.used.unwrap_or(0),
        apparent: fields.apparent.unwrap_or(0),
        nlink: fields.nlink.unwrap_or(1),
        ino: fields.ino.unwrap_or(0),
        dev: fields.dev.unwrap_or(0),
        path,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::entry::read_dir_entries_std;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn bulk_matches_std_on_mixed_tree() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("a.bin"), vec![b'x'; 2000]).unwrap();
        fs::write(root.join("sub").join("b.bin"), vec![b'y'; 4000]).unwrap();
        std::os::unix::fs::symlink(root.join("a.bin"), root.join("link")).unwrap();

        let mut a = ScanStats::default();
        let mut b = ScanStats::default();
        let bulk = read_dir_bulk(root, &mut a).expect("bulk");
        let std = read_dir_entries_std(root, &mut b);
        assert_eq!(a.errors, 0);
        assert_eq!(b.errors, 0);

        let mut bulk: Vec<_> = bulk.into_iter().map(|e| (e.path.clone(), e)).collect();
        let mut std: Vec<_> = std.into_iter().map(|e| (e.path.clone(), e)).collect();
        bulk.sort_by(|x, y| x.0.cmp(&y.0));
        std.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(bulk.len(), std.len());
        for ((_, b), (_, s)) in bulk.iter().zip(std.iter()) {
            assert_eq!(b.is_walkable_dir, s.is_walkable_dir, "{:?}", b.path);
            assert_eq!(b.apparent, s.apparent, "{:?}", b.path);
            assert_eq!(b.used, s.used, "{:?}", b.path);
            assert_eq!(b.nlink, s.nlink, "{:?}", b.path);
            assert_eq!(b.ino, s.ino, "{:?}", b.path);
            assert_eq!(b.dev, s.dev, "{:?}", b.path);
        }
    }
}
