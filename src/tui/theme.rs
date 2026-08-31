//! Color themes. One `Theme` is active for the whole process; every draw
//! function reads it through `current()`. Built-ins live here so a
//! single static binary ships them all.

use std::cell::Cell;

use crate::term::Rgb;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Rgb,
    pub panel: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub warn: Rgb,
    pub danger: Rgb,
    pub select_bg: Rgb,
    /// "smaller objects" wedge and de-emphasised glyphs.
    pub smaller: Rgb,
    /// Footer chip face — distinct from `bg` so buttons read as buttons.
    pub chip: Rgb,
    /// Slice / list hues, cycled per sibling.
    pub palette: [Rgb; 10],
    pub temp: Rgb,
    pub cache: Rgb,
    pub log: Rgb,
    pub journal: Rgb,
    pub crash: Rgb,
}

/// The default look: deep navy, lime accent, ten saturated slice hues.
pub const RINGS: Theme = Theme {
    name: "rings",
    bg: Rgb(14, 20, 34),
    panel: Rgb(20, 28, 46),
    text: Rgb(228, 232, 240),
    muted: Rgb(118, 128, 148),
    accent: Rgb(126, 214, 92),
    warn: Rgb(230, 176, 72),
    danger: Rgb(224, 88, 88),
    select_bg: Rgb(36, 52, 80),
    smaller: Rgb(86, 92, 108),
    chip: Rgb(34, 48, 74),
    palette: [
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
    ],
    temp: Rgb(220, 170, 70),
    cache: Rgb(80, 200, 210),
    log: Rgb(160, 130, 220),
    journal: Rgb(210, 100, 180),
    crash: Rgb(220, 70, 70),
};

pub const NORD: Theme = Theme {
    name: "nord",
    bg: Rgb(46, 52, 64),
    panel: Rgb(59, 66, 82),
    text: Rgb(236, 239, 244),
    muted: Rgb(129, 140, 165),
    accent: Rgb(163, 190, 140),
    warn: Rgb(235, 203, 139),
    danger: Rgb(191, 97, 106),
    select_bg: Rgb(67, 76, 94),
    smaller: Rgb(76, 86, 106),
    chip: Rgb(67, 76, 94),
    palette: [
        Rgb(136, 192, 208),
        Rgb(163, 190, 140),
        Rgb(235, 203, 139),
        Rgb(180, 142, 173),
        Rgb(208, 135, 112),
        Rgb(129, 161, 193),
        Rgb(191, 97, 106),
        Rgb(143, 188, 187),
        Rgb(94, 129, 172),
        Rgb(216, 222, 233),
    ],
    temp: Rgb(235, 203, 139),
    cache: Rgb(136, 192, 208),
    log: Rgb(180, 142, 173),
    journal: Rgb(208, 135, 112),
    crash: Rgb(191, 97, 106),
};

pub const GRUVBOX: Theme = Theme {
    name: "gruvbox",
    bg: Rgb(40, 40, 40),
    panel: Rgb(60, 56, 54),
    text: Rgb(235, 219, 178),
    muted: Rgb(146, 131, 116),
    accent: Rgb(184, 187, 38),
    warn: Rgb(250, 189, 47),
    danger: Rgb(251, 73, 52),
    select_bg: Rgb(80, 73, 69),
    smaller: Rgb(102, 92, 84),
    chip: Rgb(80, 73, 69),
    palette: [
        Rgb(184, 187, 38),
        Rgb(131, 165, 152),
        Rgb(250, 189, 47),
        Rgb(211, 134, 155),
        Rgb(251, 73, 52),
        Rgb(142, 192, 124),
        Rgb(254, 128, 25),
        Rgb(152, 151, 26),
        Rgb(215, 153, 33),
        Rgb(204, 36, 29),
    ],
    temp: Rgb(250, 189, 47),
    cache: Rgb(131, 165, 152),
    log: Rgb(211, 134, 155),
    journal: Rgb(254, 128, 25),
    crash: Rgb(251, 73, 52),
};

pub const DRACULA: Theme = Theme {
    name: "dracula",
    bg: Rgb(40, 42, 54),
    panel: Rgb(52, 55, 70),
    text: Rgb(248, 248, 242),
    muted: Rgb(98, 114, 164),
    accent: Rgb(80, 250, 123),
    warn: Rgb(241, 250, 140),
    danger: Rgb(255, 85, 85),
    select_bg: Rgb(68, 71, 90),
    smaller: Rgb(77, 80, 102),
    chip: Rgb(68, 71, 90),
    palette: [
        Rgb(189, 147, 249),
        Rgb(139, 233, 253),
        Rgb(255, 184, 108),
        Rgb(255, 121, 198),
        Rgb(255, 85, 85),
        Rgb(80, 250, 123),
        Rgb(241, 250, 140),
        Rgb(214, 172, 255),
        Rgb(105, 255, 148),
        Rgb(98, 114, 164),
    ],
    temp: Rgb(255, 184, 108),
    cache: Rgb(139, 233, 253),
    log: Rgb(189, 147, 249),
    journal: Rgb(255, 121, 198),
    crash: Rgb(255, 85, 85),
};

pub const SOLARIZED: Theme = Theme {
    name: "solarized-dark",
    bg: Rgb(0, 43, 54),
    panel: Rgb(7, 54, 66),
    text: Rgb(238, 232, 213),
    muted: Rgb(131, 148, 150),
    accent: Rgb(133, 153, 0),
    warn: Rgb(181, 137, 0),
    danger: Rgb(220, 50, 47),
    select_bg: Rgb(9, 73, 89),
    smaller: Rgb(88, 110, 117),
    chip: Rgb(9, 73, 89),
    palette: [
        Rgb(38, 139, 210),
        Rgb(42, 161, 152),
        Rgb(181, 137, 0),
        Rgb(108, 113, 196),
        Rgb(203, 75, 22),
        Rgb(133, 153, 0),
        Rgb(211, 54, 130),
        Rgb(147, 161, 161),
        Rgb(220, 50, 47),
        Rgb(238, 232, 213),
    ],
    temp: Rgb(181, 137, 0),
    cache: Rgb(42, 161, 152),
    log: Rgb(108, 113, 196),
    journal: Rgb(211, 54, 130),
    crash: Rgb(220, 50, 47),
};

/// Grays only: for 16-color consoles, e-ink, and screenshots in docs.
pub const MONO: Theme = Theme {
    name: "mono",
    bg: Rgb(0, 0, 0),
    panel: Rgb(40, 40, 40),
    text: Rgb(230, 230, 230),
    muted: Rgb(150, 150, 150),
    accent: Rgb(255, 255, 255),
    warn: Rgb(210, 210, 210),
    danger: Rgb(255, 255, 255),
    select_bg: Rgb(70, 70, 70),
    smaller: Rgb(90, 90, 90),
    chip: Rgb(50, 50, 50),
    palette: [
        Rgb(240, 240, 240),
        Rgb(120, 120, 120),
        Rgb(200, 200, 200),
        Rgb(150, 150, 150),
        Rgb(225, 225, 225),
        Rgb(105, 105, 105),
        Rgb(180, 180, 180),
        Rgb(135, 135, 135),
        Rgb(210, 210, 210),
        Rgb(165, 165, 165),
    ],
    temp: Rgb(220, 220, 220),
    cache: Rgb(190, 190, 190),
    log: Rgb(160, 160, 160),
    journal: Rgb(130, 130, 130),
    crash: Rgb(255, 255, 255),
};

pub const BUILTIN: [Theme; 6] = [RINGS, NORD, GRUVBOX, DRACULA, SOLARIZED, MONO];

// Per thread: only the UI thread draws, and it keeps each test isolated.
thread_local! {
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
}

/// The active theme for this thread. Cheap: a thread-local read and an index.
pub fn current() -> &'static Theme {
    &BUILTIN[ACTIVE.with(|a| a.get())]
}

/// Activate a built-in by name (case-insensitive) on the calling thread —
/// call it from the thread that will draw.
pub fn set(name: &str) -> Result<(), String> {
    let want = name.trim().to_ascii_lowercase();
    match BUILTIN.iter().position(|t| t.name == want) {
        Some(i) => {
            ACTIVE.with(|a| a.set(i));
            Ok(())
        }
        None => Err(format!(
            "unknown theme {name:?} (themes: {})",
            names().join(", ")
        )),
    }
}

pub fn names() -> Vec<&'static str> {
    BUILTIN.iter().map(|t| t.name).collect()
}

impl Theme {
    /// Hover highlight for rows and chips: a whisper of the selection color.
    pub fn hover_bg(&self) -> Rgb {
        mix(self.bg, self.select_bg, 0.4)
    }
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Rgb(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

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
    let th = current();
    match cat {
        Normal => th.palette[0],
        Temp => th.temp,
        Cache => th.cache,
        Log => th.log,
        Journal => th.journal,
        Crash => th.crash,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_resolves_by_name_and_names_are_unique() {
        let names = names();
        for (i, name) in names.iter().enumerate() {
            assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
            assert_eq!(BUILTIN[i].name, *name);
        }
        assert_eq!(names[0], "rings", "the default stays first");
    }

    #[test]
    fn unknown_theme_lists_the_choices() {
        let err = set("neon-hotdog").unwrap_err();
        assert!(err.contains("nord"), "{err}");
        assert!(err.contains("gruvbox"), "{err}");
        assert!(err.contains("mono"), "{err}");
    }

    #[test]
    fn every_theme_keeps_text_readable_on_its_backgrounds() {
        fn luma(c: Rgb) -> f32 {
            0.2126 * c.0 as f32 + 0.7152 * c.1 as f32 + 0.0722 * c.2 as f32
        }
        for t in BUILTIN {
            for bg in [t.bg, t.panel, t.select_bg, t.hover_bg(), t.chip] {
                assert!(
                    luma(t.text) - luma(bg) > 90.0,
                    "{}: text on {bg:?} too close",
                    t.name
                );
            }
            assert!(
                luma(t.hover_bg()) < luma(t.select_bg),
                "{}: hover is subtler",
                t.name
            );
        }
    }
}
