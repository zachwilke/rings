//! Human-readable sizes and used vs apparent.

use crate::constants::BLOCK_BYTES;

const UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];

/// Format a byte count the way a server admin expects (1024-based, short).
pub fn human_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Allocated size from Unix `st_blocks` (512-byte units).
pub fn used_from_blocks(blocks: u64) -> u64 {
    blocks.saturating_mul(BLOCK_BYTES)
}

/// Group a count for progress text (`12481` → `12,481`).
pub fn group_u64(n: u64) -> String {
    let raw = n.to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_scales() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.00 KB");
        assert_eq!(human_bytes(10 * 1024), "10.0 KB");
        assert_eq!(human_bytes(1536), "1.50 KB");
    }

    #[test]
    fn used_multiplies_512() {
        assert_eq!(used_from_blocks(2), 1024);
    }
}
