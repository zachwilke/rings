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

/// The default: Tokyo Night, the storm-lit navy that most terminals
/// already wear. Blue-violet ground, ten saturated hues, green accent.
pub const TOKYO_NIGHT: Theme = Theme {
    name: "tokyo-night",
    bg: Rgb(26, 27, 38),
    panel: Rgb(36, 40, 59),
    text: Rgb(192, 202, 245),
    muted: Rgb(86, 95, 137),
    accent: Rgb(158, 206, 106),
    warn: Rgb(224, 175, 104),
    danger: Rgb(247, 118, 142),
    select_bg: Rgb(40, 52, 87),
    smaller: Rgb(59, 66, 97),
    chip: Rgb(41, 46, 66),
    palette: [
        Rgb(122, 162, 247),
        Rgb(125, 207, 255),
        Rgb(187, 154, 247),
        Rgb(158, 206, 106),
        Rgb(255, 158, 100),
        Rgb(247, 118, 142),
        Rgb(115, 218, 202),
        Rgb(157, 124, 216),
        Rgb(224, 175, 104),
        Rgb(42, 195, 222),
    ],
    temp: Rgb(255, 158, 100),
    cache: Rgb(125, 207, 255),
    log: Rgb(157, 124, 216),
    journal: Rgb(187, 154, 247),
    crash: Rgb(247, 118, 142),
};

/// Tokyo Night with the lighter Storm ground, for screens where the
/// standard variant reads as flat black.
pub const TOKYO_NIGHT_STORM: Theme = Theme {
    name: "tokyo-night-storm",
    bg: Rgb(36, 40, 59),
    panel: Rgb(41, 46, 66),
    text: Rgb(192, 202, 245),
    muted: Rgb(86, 95, 137),
    accent: Rgb(158, 206, 106),
    warn: Rgb(224, 175, 104),
    danger: Rgb(247, 118, 142),
    select_bg: Rgb(46, 60, 100),
    smaller: Rgb(59, 66, 97),
    chip: Rgb(52, 58, 82),
    palette: [
        Rgb(122, 162, 247),
        Rgb(125, 207, 255),
        Rgb(187, 154, 247),
        Rgb(158, 206, 106),
        Rgb(255, 158, 100),
        Rgb(247, 118, 142),
        Rgb(115, 218, 202),
        Rgb(157, 124, 216),
        Rgb(224, 175, 104),
        Rgb(42, 195, 222),
    ],
    temp: Rgb(255, 158, 100),
    cache: Rgb(125, 207, 255),
    log: Rgb(157, 124, 216),
    journal: Rgb(187, 154, 247),
    crash: Rgb(247, 118, 142),
};

/// Tokyo Night Day: the light variant. The only pale built-in, and the
/// one that proves the fade, emphasis, and label-contrast helpers are not
/// quietly assuming a dark ground.
pub const TOKYO_NIGHT_DAY: Theme = Theme {
    name: "tokyo-night-day",
    bg: Rgb(225, 226, 231),
    panel: Rgb(208, 213, 227),
    text: Rgb(55, 96, 191),
    muted: Rgb(132, 140, 181),
    accent: Rgb(88, 117, 57),
    warn: Rgb(140, 108, 62),
    danger: Rgb(245, 42, 101),
    select_bg: Rgb(183, 193, 227),
    smaller: Rgb(168, 174, 203),
    chip: Rgb(196, 200, 218),
    palette: [
        Rgb(46, 125, 233),
        Rgb(0, 113, 151),
        Rgb(152, 84, 241),
        Rgb(88, 117, 57),
        Rgb(177, 92, 0),
        Rgb(245, 42, 101),
        Rgb(17, 140, 116),
        Rgb(120, 71, 189),
        Rgb(140, 108, 62),
        Rgb(97, 114, 176),
    ],
    temp: Rgb(177, 92, 0),
    cache: Rgb(0, 113, 151),
    log: Rgb(120, 71, 189),
    journal: Rgb(152, 84, 241),
    crash: Rgb(245, 42, 101),
};

/// Catppuccin Mocha: soft pastels on a warm charcoal.
pub const CATPPUCCIN: Theme = Theme {
    name: "catppuccin",
    bg: Rgb(30, 30, 46),
    panel: Rgb(49, 50, 68),
    text: Rgb(205, 214, 244),
    muted: Rgb(108, 112, 134),
    accent: Rgb(166, 227, 161),
    warn: Rgb(249, 226, 175),
    danger: Rgb(243, 139, 168),
    select_bg: Rgb(69, 71, 90),
    smaller: Rgb(88, 91, 112),
    chip: Rgb(49, 50, 68),
    palette: [
        Rgb(203, 166, 247),
        Rgb(137, 220, 235),
        Rgb(250, 179, 135),
        Rgb(166, 227, 161),
        Rgb(243, 139, 168),
        Rgb(137, 180, 250),
        Rgb(245, 194, 231),
        Rgb(148, 226, 213),
        Rgb(249, 226, 175),
        Rgb(180, 190, 254),
    ],
    temp: Rgb(250, 179, 135),
    cache: Rgb(137, 220, 235),
    log: Rgb(203, 166, 247),
    journal: Rgb(245, 194, 231),
    crash: Rgb(243, 139, 168),
};

/// Rose Pine: muted plum and foam, low saturation throughout.
pub const ROSE_PINE: Theme = Theme {
    name: "rose-pine",
    bg: Rgb(25, 23, 36),
    panel: Rgb(38, 35, 58),
    text: Rgb(224, 222, 244),
    muted: Rgb(110, 106, 134),
    accent: Rgb(156, 207, 216),
    warn: Rgb(246, 193, 119),
    danger: Rgb(235, 111, 146),
    select_bg: Rgb(64, 61, 82),
    smaller: Rgb(82, 79, 103),
    chip: Rgb(38, 35, 58),
    palette: [
        Rgb(196, 167, 231),
        Rgb(156, 207, 216),
        Rgb(246, 193, 119),
        Rgb(235, 188, 186),
        Rgb(235, 111, 146),
        Rgb(86, 148, 159),
        Rgb(144, 140, 170),
        Rgb(215, 130, 126),
        Rgb(170, 150, 220),
        Rgb(62, 143, 176),
    ],
    temp: Rgb(246, 193, 119),
    cache: Rgb(156, 207, 216),
    log: Rgb(196, 167, 231),
    journal: Rgb(235, 188, 186),
    crash: Rgb(235, 111, 146),
};

/// Everforest Dark: desaturated green-grey, easy over long sessions.
pub const EVERFOREST: Theme = Theme {
    name: "everforest",
    bg: Rgb(45, 53, 59),
    panel: Rgb(52, 63, 68),
    text: Rgb(211, 198, 170),
    muted: Rgb(133, 146, 137),
    accent: Rgb(167, 192, 128),
    warn: Rgb(219, 188, 127),
    danger: Rgb(230, 126, 128),
    select_bg: Rgb(61, 72, 77),
    smaller: Rgb(79, 88, 94),
    chip: Rgb(61, 72, 77),
    palette: [
        Rgb(167, 192, 128),
        Rgb(131, 192, 146),
        Rgb(219, 188, 127),
        Rgb(127, 187, 179),
        Rgb(230, 152, 117),
        Rgb(230, 126, 128),
        Rgb(214, 153, 182),
        Rgb(154, 166, 160),
        Rgb(200, 170, 110),
        Rgb(160, 180, 200),
    ],
    temp: Rgb(230, 152, 117),
    cache: Rgb(131, 192, 146),
    log: Rgb(214, 153, 182),
    journal: Rgb(127, 187, 179),
    crash: Rgb(230, 126, 128),
};

/// One Dark: the Atom palette, familiar from a decade of editors.
pub const ONE_DARK: Theme = Theme {
    name: "one-dark",
    bg: Rgb(40, 44, 52),
    panel: Rgb(49, 53, 63),
    text: Rgb(171, 178, 191),
    muted: Rgb(92, 99, 112),
    accent: Rgb(152, 195, 121),
    warn: Rgb(229, 192, 123),
    danger: Rgb(224, 108, 117),
    select_bg: Rgb(62, 68, 81),
    smaller: Rgb(75, 82, 99),
    chip: Rgb(58, 63, 75),
    palette: [
        Rgb(97, 175, 239),
        Rgb(86, 182, 194),
        Rgb(198, 120, 221),
        Rgb(152, 195, 121),
        Rgb(209, 154, 102),
        Rgb(224, 108, 117),
        Rgb(229, 192, 123),
        Rgb(130, 170, 255),
        Rgb(110, 200, 180),
        Rgb(190, 140, 200),
    ],
    temp: Rgb(209, 154, 102),
    cache: Rgb(86, 182, 194),
    log: Rgb(198, 120, 221),
    journal: Rgb(97, 175, 239),
    crash: Rgb(224, 108, 117),
};

/// The original rings look: deep navy, lime accent. Was the default
/// through 0.2; kept because it is what the README screenshot shows.
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

pub const BUILTIN: [Theme; 13] = [
    TOKYO_NIGHT,
    TOKYO_NIGHT_STORM,
    TOKYO_NIGHT_DAY,
    CATPPUCCIN,
    ROSE_PINE,
    EVERFOREST,
    ONE_DARK,
    RINGS,
    NORD,
    GRUVBOX,
    DRACULA,
    SOLARIZED,
    MONO,
];

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

    /// True for a pale ground. Three helpers below have to know which way
    /// "recede" and "stand out" point; deriving it from `bg` means a new
    /// theme cannot forget to declare it.
    pub fn is_light(&self) -> bool {
        luma(self.bg) > 128.0
    }
}

/// Rec. 601 luminance. Cheap, and accurate enough to decide whether a colour
/// reads as light or dark.
pub fn luma(c: Rgb) -> f32 {
    0.2126 * f32::from(c.0) + 0.7152 * f32::from(c.1) + 0.0722 * f32::from(c.2)
}

/// Linear blend. Public so hot loops can pass a background they already
/// hold instead of re-reading the active theme per sample.
pub fn blend(a: Rgb, b: Rgb, t: f32) -> Rgb {
    mix(a, b, t.clamp(0.0, 1.0))
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Rgb(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// How far the deepest ring fades into the background.
pub const DIM_MAX: f32 = 0.38;

/// Fraction of the way to the background at `ring`.
pub fn dim_t(ring: usize, rings: usize) -> f32 {
    let last = rings.saturating_sub(1).max(1) as f32;
    DIM_MAX * (ring as f32 / last).min(1.0)
}

/// Blend `color` `t` of the way into the active background.
pub fn toward_bg(color: Rgb, t: f32) -> Rgb {
    mix(color, current().bg, t.clamp(0.0, 1.0))
}

/// Fade so the outermost ring recedes.
///
/// Blends toward the theme's *background* rather than toward black. On a
/// light theme darkening is what makes a wedge more prominent, so the old
/// multiply inverted the depth cue there; on a dark theme the two are
/// nearly the same thing.
pub fn dim_color(color: Rgb, ring: usize, rings: usize) -> Rgb {
    toward_bg(color, dim_t(ring, rings))
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

/// Pull a colour *away* from the ground so it reads as selected: lighter on
/// a dark theme, darker on a light one. Unconditionally brightening would
/// wash a selection out on a pale ground.
pub fn emphasize(color: Rgb) -> Rgb {
    const STEP: u8 = 40;
    let Rgb(r, g, b) = color;
    if current().is_light() {
        Rgb(
            r.saturating_sub(STEP),
            g.saturating_sub(STEP),
            b.saturating_sub(STEP),
        )
    } else {
        Rgb(
            r.saturating_add(STEP),
            g.saturating_add(STEP),
            b.saturating_add(STEP),
        )
    }
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

/// Colour by what can be done about the space rather than by what it is:
/// red for files rings will refuse to delete, amber for space a command
/// gives back, green for ordinary waste.
/// Readable ink for text written *on* a filled cell.
///
/// The icicle paints slice colours as backgrounds, which no view did before,
/// so a label has to pick a side: whichever of the theme's text and ground
/// sits further away in luminance. Choosing `text` unconditionally vanishes
/// on a pale wedge; choosing `bg` unconditionally vanishes on a light theme.
pub fn contrast_on(fill: Rgb) -> Rgb {
    let th = current();
    let target = luma(fill);
    if (luma(th.text) - target).abs() >= (luma(th.bg) - target).abs() {
        th.text
    } else {
        th.bg
    }
}

pub fn tone_color(tone: crate::apps::Tone) -> Rgb {
    use crate::apps::Tone::*;
    let th = current();
    match tone {
        Protected => th.danger,
        Advisory => th.warn,
        Reclaimable => th.accent,
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
            set(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert_eq!(names[0], "tokyo-night", "the default comes first");
        assert!(names.len() >= 13, "a variety, not a token second option");
        set("tokyo-night").unwrap();
    }

    #[test]
    fn names_are_stable_slugs() {
        for t in BUILTIN {
            assert!(
                t.name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'-' || b.is_ascii_digit()),
                "{} is not a typeable slug",
                t.name
            );
        }
    }

    #[test]
    fn unknown_theme_lists_the_choices() {
        let err = set("neon-hotdog").unwrap_err();
        assert!(err.contains("tokyo-night"), "{err}");
        assert!(err.contains("nord"), "{err}");
        assert!(err.contains("mono"), "{err}");
    }

    #[test]
    fn every_theme_keeps_text_readable_on_its_backgrounds() {
        for t in BUILTIN {
            for bg in [t.bg, t.panel, t.select_bg, t.hover_bg(), t.chip] {
                assert!(
                    (luma(t.text) - luma(bg)).abs() > 90.0,
                    "{}: text on {bg:?} too close",
                    t.name
                );
            }
            // Hover is a whisper of the selection: nearer the ground than
            // `select_bg` is, whichever direction that happens to be.
            assert!(
                (luma(t.hover_bg()) - luma(t.bg)).abs() < (luma(t.select_bg) - luma(t.bg)).abs(),
                "{}: hover is subtler than selection",
                t.name
            );
        }
    }

    #[test]
    fn every_theme_labels_its_own_fills_legibly() {
        // The icicle paints slice colours as *backgrounds*, which no view
        // did before. A palette that only ever had to work as foreground
        // text can fail this, so every hue of every theme is checked.
        for t in BUILTIN {
            set(t.name).unwrap();
            let mut fills = t.palette.to_vec();
            fills.extend_from_slice(&[
                t.temp, t.cache, t.log, t.journal, t.crash, t.accent, t.warn, t.danger,
            ]);
            for fill in fills {
                let ink = contrast_on(fill);
                assert!(
                    (luma(ink) - luma(fill)).abs() > 80.0,
                    "{}: {fill:?} labelled with {ink:?} would be unreadable",
                    t.name
                );
            }
        }
        set("tokyo-night").unwrap();
    }

    #[test]
    fn exactly_one_builtin_has_a_pale_ground() {
        let light: Vec<&str> = BUILTIN
            .iter()
            .filter(|t| t.is_light())
            .map(|t| t.name)
            .collect();
        assert_eq!(
            light,
            vec!["tokyo-night-day"],
            "the light variant is what proves the helpers are not assuming dark"
        );
    }

    #[test]
    fn dimming_recedes_toward_the_ground_on_light_and_dark_alike() {
        for name in ["tokyo-night", "tokyo-night-day"] {
            set(name).unwrap();
            let th = current();
            let c = th.palette[0];
            let near = dim_color(c, 0, 8);
            let far = dim_color(c, 7, 8);
            assert_eq!(near, c, "{name}: the innermost ring is undimmed");
            assert!(
                (luma(far) - luma(th.bg)).abs() < (luma(near) - luma(th.bg)).abs(),
                "{name}: depth must move a wedge toward the ground, not away"
            );
        }
        set("tokyo-night").unwrap();
    }

    #[test]
    fn emphasis_moves_away_from_the_ground_in_both_directions() {
        set("tokyo-night").unwrap();
        let c = current().palette[0];
        assert!(
            luma(emphasize(c)) > luma(c),
            "on a dark ground a selection lightens"
        );

        set("tokyo-night-day").unwrap();
        let c = current().palette[0];
        assert!(
            luma(emphasize(c)) < luma(c),
            "on a pale ground it has to darken instead"
        );
        set("tokyo-night").unwrap();
    }

    #[test]
    fn contrast_on_flips_with_the_ground() {
        // The same mid-tone fill wants opposite ink on opposite themes.
        let fill = Rgb(120, 120, 120);
        set("tokyo-night").unwrap();
        let dark_ink = contrast_on(fill);
        set("tokyo-night-day").unwrap();
        let light_ink = contrast_on(fill);
        assert_ne!(
            luma(dark_ink) > luma(fill),
            luma(light_ink) > luma(fill),
            "ink must pick opposite sides on opposite grounds"
        );
        set("tokyo-night").unwrap();
    }
}
