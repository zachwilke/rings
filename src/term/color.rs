//! Color capability: what the terminal can show, and how to say it.
//!
//! Detected once from the environment at startup; `flush_diff` asks on
//! every paint so a later `set_color_depth` (options menu) takes effect.

use std::sync::atomic::{AtomicU8, Ordering};

use super::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorDepth {
    /// No color at all (`NO_COLOR`): bold and reverse video only.
    Mono,
    Ansi16,
    Ansi256,
    TrueColor,
}

static DEPTH: AtomicU8 = AtomicU8::new(3);

pub fn color_depth() -> ColorDepth {
    match DEPTH.load(Ordering::Relaxed) {
        0 => ColorDepth::Mono,
        1 => ColorDepth::Ansi16,
        2 => ColorDepth::Ansi256,
        _ => ColorDepth::TrueColor,
    }
}

pub fn set_color_depth(depth: ColorDepth) {
    let v = match depth {
        ColorDepth::Mono => 0,
        ColorDepth::Ansi16 => 1,
        ColorDepth::Ansi256 => 2,
        ColorDepth::TrueColor => 3,
    };
    DEPTH.store(v, Ordering::Relaxed);
}

/// Read the environment. `RINGS_COLORS` wins, then `NO_COLOR`, then the
/// usual `COLORTERM` / `TERM_PROGRAM` / `TERM` hints. Unknown terminals
/// get 256 colors: every xterm-alike parses those.
pub fn detect_color_depth() -> ColorDepth {
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    detect_from(
        &env("RINGS_COLORS"),
        std::env::var_os("NO_COLOR").is_some(),
        &env("COLORTERM"),
        &env("TERM_PROGRAM"),
        &env("TERM"),
        std::env::var_os("WT_SESSION").is_some(),
    )
}

fn detect_from(
    override_: &str,
    no_color: bool,
    colorterm: &str,
    term_program: &str,
    term: &str,
    windows_terminal: bool,
) -> ColorDepth {
    match override_.trim().to_ascii_lowercase().as_str() {
        "0" | "mono" | "none" => return ColorDepth::Mono,
        "16" | "ansi" => return ColorDepth::Ansi16,
        "256" => return ColorDepth::Ansi256,
        "truecolor" | "24bit" | "16m" => return ColorDepth::TrueColor,
        _ => {}
    }
    if no_color {
        return ColorDepth::Mono;
    }
    let ct = colorterm.to_ascii_lowercase();
    if ct == "truecolor" || ct == "24bit" {
        return ColorDepth::TrueColor;
    }
    let tp = term_program.to_ascii_lowercase();
    if windows_terminal
        || tp.contains("iterm")
        || tp.contains("wezterm")
        || tp.contains("ghostty")
        || tp.contains("vscode")
        || tp.contains("hyper")
        || tp.contains("tabby")
        || tp.contains("alacritty")
        || tp.contains("kitty")
    {
        return ColorDepth::TrueColor;
    }
    let t = term.to_ascii_lowercase();
    if t.contains("direct") || t.contains("truecolor") || t.contains("kitty") {
        return ColorDepth::TrueColor;
    }
    if t == "linux" || t == "vt100" || t == "vt220" || t == "ansi" || t == "xterm" {
        return ColorDepth::Ansi16;
    }
    if t == "dumb" {
        return ColorDepth::Mono;
    }
    ColorDepth::Ansi256
}

/// SGR parameters (without the leading `ESC[` and trailing `m`) that set
/// this color as foreground (`fg = true`) or background at `depth`.
/// `None` means "emit nothing" (mono).
pub fn sgr(depth: ColorDepth, color: Rgb, fg: bool) -> Option<String> {
    let Rgb(r, g, b) = color;
    match depth {
        ColorDepth::Mono => None,
        ColorDepth::TrueColor => Some(format!("{};2;{r};{g};{b}", if fg { 38 } else { 48 })),
        ColorDepth::Ansi256 => Some(format!(
            "{};5;{}",
            if fg { 38 } else { 48 },
            xterm256(color)
        )),
        ColorDepth::Ansi16 => {
            let i = ansi16(color);
            let base = match (fg, i >= 8) {
                (true, false) => 30,
                (true, true) => 90,
                (false, false) => 40,
                (false, true) => 100,
            };
            Some(format!("{}", base + (i % 8) as u16))
        }
    }
}

/// Nearest xterm-256 index: grayscale ramp for near-grays, else 6×6×6 cube.
pub fn xterm256(color: Rgb) -> u8 {
    let Rgb(r, g, b) = color;
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let cube = |v: i32| -> i32 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let (cr, cg, cb) = (cube(r), cube(g), cube(b));
    let cube_idx = 16 + 36 * cr + 6 * cg + cb;
    let cube_val = |i: i32| if i == 0 { 0 } else { 55 + i * 40 };
    let cube_dist =
        (r - cube_val(cr)).pow(2) + (g - cube_val(cg)).pow(2) + (b - cube_val(cb)).pow(2);

    let avg = (r + g + b) / 3;
    let gray_i = ((avg - 8).max(0) / 10).min(23);
    let gray_val = 8 + gray_i * 10;
    let gray_dist = (r - gray_val).pow(2) + (g - gray_val).pow(2) + (b - gray_val).pow(2);

    if gray_dist < cube_dist {
        (232 + gray_i) as u8
    } else {
        cube_idx as u8
    }
}

/// Map to the 16 ANSI colors by hue, not RGB distance: a lime green must
/// land on green, not on the gray that happens to be numerically closer.
pub fn ansi16(color: Rgb) -> u8 {
    let Rgb(r, g, b) = color;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let luma = (r as u32 * 30 + g as u32 * 59 + b as u32 * 11) / 100;
    if max - min < 40 {
        // Near gray: black, bright black, white, bright white.
        return match luma {
            0..=63 => 0,
            64..=159 => 8,
            160..=223 => 7,
            _ => 15,
        };
    }
    let mid = (max as u16 + min as u16) / 2;
    let on = |c: u8| (c as u16 > mid) as u8;
    let idx = on(r) | on(g) << 1 | on(b) << 2;
    if max >= 170 {
        idx + 8
    } else {
        idx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_prefers_override_then_no_color_then_hints() {
        assert_eq!(
            detect_from("16", false, "truecolor", "", "xterm-256color", false),
            ColorDepth::Ansi16
        );
        assert_eq!(
            detect_from("", true, "truecolor", "", "xterm-256color", false),
            ColorDepth::Mono
        );
        assert_eq!(
            detect_from("", false, "truecolor", "", "xterm-256color", false),
            ColorDepth::TrueColor
        );
        assert_eq!(
            detect_from("", false, "", "iTerm.app", "xterm-256color", false),
            ColorDepth::TrueColor
        );
        assert_eq!(
            detect_from("", false, "", "", "xterm-256color", true),
            ColorDepth::TrueColor,
            "Windows Terminal"
        );
        assert_eq!(
            detect_from("", false, "", "Apple_Terminal", "xterm-256color", false),
            ColorDepth::Ansi256,
            "Terminal.app has no truecolor"
        );
        assert_eq!(
            detect_from("", false, "", "", "linux", false),
            ColorDepth::Ansi16,
            "the Linux console"
        );
        assert_eq!(
            detect_from("", false, "", "", "screen", false),
            ColorDepth::Ansi256,
            "unknown terminals get 256"
        );
    }

    #[test]
    fn quantization_hits_the_expected_indices() {
        assert_eq!(xterm256(Rgb(0, 0, 0)), 16);
        assert_eq!(xterm256(Rgb(255, 255, 255)), 231);
        assert_eq!(xterm256(Rgb(128, 128, 128)), 244, "mid gray uses the ramp");
        assert_eq!(xterm256(Rgb(255, 0, 0)), 196);
        assert_eq!(xterm256(Rgb(0, 0, 255)), 21);
        assert_eq!(ansi16(Rgb(0, 0, 0)), 0);
        assert_eq!(ansi16(Rgb(255, 255, 255)), 15);
        assert_eq!(ansi16(Rgb(128, 128, 128)), 8, "mid gray is bright black");
        assert_eq!(ansi16(Rgb(250, 30, 30)), 9, "bright red");
        assert_eq!(ansi16(Rgb(0, 120, 0)), 2, "plain green");
        assert_eq!(
            ansi16(Rgb(126, 214, 92)),
            10,
            "lime is bright green, not gray"
        );
        assert_eq!(ansi16(Rgb(78, 188, 198)), 14, "teal is bright cyan");
        assert_eq!(ansi16(Rgb(186, 116, 214)), 13, "violet is bright magenta");
        assert_eq!(ansi16(Rgb(14, 20, 34)), 0, "the rings background is black");
    }

    #[test]
    fn sgr_strings_per_depth() {
        let c = Rgb(126, 214, 92);
        assert_eq!(
            sgr(ColorDepth::TrueColor, c, true).unwrap(),
            "38;2;126;214;92"
        );
        assert_eq!(
            sgr(ColorDepth::TrueColor, c, false).unwrap(),
            "48;2;126;214;92"
        );
        assert!(sgr(ColorDepth::Ansi256, c, true)
            .unwrap()
            .starts_with("38;5;"));
        assert_eq!(sgr(ColorDepth::Ansi16, c, false).unwrap(), "102");
        assert_eq!(sgr(ColorDepth::Ansi16, c, true).unwrap(), "92");
        assert_eq!(sgr(ColorDepth::Mono, c, true), None);
    }
}
