use crate::term::Rgb;

pub const BG: Rgb = Rgb(14, 20, 34);
pub const PANEL: Rgb = Rgb(20, 28, 46);
pub const TEXT: Rgb = Rgb(228, 232, 240);
pub const MUTED: Rgb = Rgb(118, 128, 148);
pub const ACCENT: Rgb = Rgb(126, 214, 92);
pub const WARN: Rgb = Rgb(230, 176, 72);
pub const DANGER: Rgb = Rgb(224, 88, 88);
pub const SELECT_BG: Rgb = Rgb(36, 52, 80);
pub const SMALLER: Rgb = Rgb(86, 92, 108);

pub const PALETTE: [Rgb; 10] = [
    Rgb(142, 214, 74),
    Rgb(78, 188, 198),
    Rgb(228, 178, 72),
    Rgb(186, 116, 214),
    Rgb(226, 104, 92),
    Rgb(74, 150, 226),
    Rgb(226, 132, 176),
    Rgb(158, 196, 92),
    Rgb(92, 206, 158),
    Rgb(218, 142, 62),
];

/// Fade so the outermost ring stays readable (~0.62) at 4 rings or 8.
pub fn dim_color(color: Rgb, ring: usize, rings: usize) -> Rgb {
    let Rgb(r, g, b) = color;
    let last = rings.saturating_sub(1).max(1) as f32;
    let keep = 1.0 - 0.38 * (ring as f32 / last).min(1.0);
    Rgb(
        (r as f32 * keep) as u8,
        (g as f32 * keep) as u8,
        (b as f32 * keep) as u8,
    )
}

/// 1-pixel-dark wedge edge: keep the hue, drop the value.
pub fn separator_color(color: Rgb) -> Rgb {
    let Rgb(r, g, b) = color;
    Rgb(
        (r as u16 * 42 / 100) as u8,
        (g as u16 * 42 / 100) as u8,
        (b as u16 * 42 / 100) as u8,
    )
}

pub fn brighten(color: Rgb) -> Rgb {
    let Rgb(r, g, b) = color;
    Rgb(
        r.saturating_add(40),
        g.saturating_add(40),
        b.saturating_add(40),
    )
}

pub fn category_color(cat: crate::classify::Category) -> Rgb {
    use crate::classify::Category::*;
    match cat {
        Normal => PALETTE[0],
        Temp => Rgb(220, 170, 70),
        Cache => Rgb(80, 200, 210),
        Log => Rgb(160, 130, 220),
        Journal => Rgb(210, 100, 180),
        Crash => Rgb(220, 70, 70),
    }
}
