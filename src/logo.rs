//! Shared sunburst / nested-rings mark. One constant, four surfaces:
//! TUI first paint, help overlay, `rings help` / `--help`, and the README.

/// Compact nested-rings sunburst. 7 lines, dark-terminal friendly, not figlet.
pub const LOGO: &str = "\
       ╲    │    ╱
     ·  ╭───────╮  ·
  ─     │ ╭───╮ │     ─
        │ │ ◎ │ │
  ─     │ ╰───╯ │     ─
     ·  ╰───────╯  ·
       ╱    │    ╲
";

pub fn lines() -> impl Iterator<Item = &'static str> {
    LOGO.lines().filter(|l| !l.is_empty())
}

pub fn size() -> (u16, u16) {
    let mut w = 0u16;
    let mut h = 0u16;
    for line in lines() {
        w = w.max(line.chars().count() as u16);
        h += 1;
    }
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_is_compact_sunburst() {
        let n = lines().count();
        assert!((6..=10).contains(&n), "logo should be 6–10 lines, got {n}");
        let text: String = lines().collect();
        assert!(text.contains('◎'), "nested-ring center");
        assert!(text.contains('╭') && text.contains('╰'), "rings");
        assert!(text.contains('╲') && text.contains('╱'), "sunburst rays");
    }

    #[test]
    fn readme_embeds_the_same_logo() {
        let readme = include_str!("../README.md");
        for line in lines() {
            assert!(
                readme.contains(line),
                "README is missing logo line:\n{line}\n"
            );
        }
    }
}
