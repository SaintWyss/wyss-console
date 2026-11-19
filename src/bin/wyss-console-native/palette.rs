//! Colors of the terminal surface for the theme in force: default foreground and background, the
//! 256-color palette (base 16 + 6x6x6 cube + grayscale ramp), the selection, the cursor and the
//! search highlights. Dark, light and high contrast each define a base palette.

use std::sync::OnceLock;

use crate::ghostty::Rgb;
use crate::theme::{self, Theme};

pub struct Colors {
    pub fg: Rgb,
    pub bg: Rgb,
    pub sel_bg: Rgb,
    /// Red, green, blue, alpha of the cursor block.
    pub cursor: [f32; 4],
    /// Overlay of the matches of a search, and of the selected one.
    pub find: Rgb,
    pub find_current: Rgb,
    pub table: [Rgb; 256],
}

pub fn hex(h: &str) -> Rgb {
    let n = u32::from_str_radix(h.trim_start_matches('#'), 16).expect("a #rrggbb color");
    Rgb { r: (n >> 16) as u8, g: (n >> 8) as u8, b: n as u8 }
}

/// The Console's ANSI palette for light and high contrast: black, red, green, yellow, blue, magenta,
/// cyan, white, then the same eight bright.
const LIGHT: [&str; 16] = [
    "#111827", "#a40000", "#075a2c", "#644200", "#004f9e", "#6b2292", "#00636b", "#475569", "#343f50", "#7f0000", "#004820", "#503400", "#003d7a", "#571578", "#004b52", "#1f2937",
];
const HIGH_CONTRAST: [&str; 16] = [
    "#c5c5c5", "#ff5c5c", "#73ff73", "#fff45c", "#78b5ff", "#f0aaff", "#55ffff", "#ffffff", "#f0f0f0", "#ff8585", "#a3ffa3", "#fffaa8", "#aad1ff", "#ffccff", "#a6ffff", "#ffffff",
];
const DARK: [(u8, u8, u8); 16] = [
    (0x1d, 0x1f, 0x21), (0xcc, 0x66, 0x66), (0xb5, 0xbd, 0x68), (0xf0, 0xc6, 0x74),
    (0x81, 0xa2, 0xbe), (0xb2, 0x94, 0xbb), (0x8a, 0xbe, 0xb7), (0xc5, 0xc8, 0xc6),
    (0x66, 0x66, 0x66), (0xd5, 0x4e, 0x53), (0xb9, 0xca, 0x4a), (0xe7, 0xc5, 0x47),
    (0x7a, 0xa6, 0xda), (0xc3, 0x97, 0xd8), (0x70, 0xc0, 0xb1), (0xea, 0xea, 0xea),
];

/// The 256 entries: `base` for the first 16, then the cube and the ramp.
fn table(base: [Rgb; 16]) -> [Rgb; 256] {
    let mut t = [Rgb { r: 0, g: 0, b: 0 }; 256];
    for (i, e) in t.iter_mut().enumerate() {
        *e = if i < 16 {
            base[i]
        } else if i < 232 {
            let i = i - 16;
            let v = |x: usize| if x == 0 { 0 } else { (x * 40 + 55) as u8 };
            Rgb { r: v(i / 36), g: v((i / 6) % 6), b: v(i % 6) }
        } else {
            let g = (8 + (i - 232) * 10) as u8;
            Rgb { r: g, g, b: g }
        };
    }
    t
}

pub fn colors_for(theme: Theme) -> Colors {
    let tok = |name: &str| hex(theme::token(theme, name).expect("a token"));
    match theme {
        Theme::Dark => Colors {
            fg: tok("text"),
            bg: tok("bg"),
            sel_bg: hex("#3a4a63"),
            cursor: [0.77, 0.78, 0.78, 0.55],
            find: hex("#f5c044"),
            find_current: hex("#ff9f43"),
            table: table(DARK.map(|(r, g, b)| Rgb { r, g, b })),
        },
        Theme::Light => Colors {
            fg: hex("#111111"),
            bg: hex("#ffffff"),
            sel_bg: hex("#b5d3f5"),
            cursor: [0.07, 0.07, 0.07, 0.45],
            find: hex("#ffd54a"),
            find_current: hex("#ff8a3d"),
            table: table(LIGHT.map(hex)),
        },
        Theme::HighContrast => Colors {
            fg: hex("#ffffff"),
            bg: hex("#000000"),
            sel_bg: hex("#0047ab"),
            cursor: [1.0, 1.0, 1.0, 0.7],
            find: hex("#ffff00"),
            find_current: hex("#ff8c00"),
            table: table(HIGH_CONTRAST.map(hex)),
        },
    }
}

/// The palette for the active theme, cached once per theme.
pub fn colors() -> &'static Colors {
    static DARK: OnceLock<Colors> = OnceLock::new();
    static LIGHT: OnceLock<Colors> = OnceLock::new();
    static HIGH_CONTRAST: OnceLock<Colors> = OnceLock::new();
    let theme = theme::current();
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
        Theme::HighContrast => &HIGH_CONTRAST,
    }.get_or_init(|| colors_for(theme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_layout() {
        let t = colors_for(Theme::Dark).table;
        assert_eq!(t[1], Rgb { r: 0xcc, g: 0x66, b: 0x66 });
        assert_eq!(t[16], Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(t[231], Rgb { r: 255, g: 255, b: 255 });
        assert_eq!(t[232], Rgb { r: 8, g: 8, b: 8 });
        assert_eq!(t[255], Rgb { r: 238, g: 238, b: 238 });
    }

    #[test]
    fn dark_keeps_the_consoles_surface_and_the_others_change_it() {
        let d = colors_for(Theme::Dark);
        assert_eq!((d.fg, d.bg), (hex("#ececec"), hex("#0e0e0e")));
        let l = colors_for(Theme::Light);
        assert_eq!((l.fg, l.bg), (hex("#111111"), hex("#ffffff")), "--terminal-fg and --terminal-bg of the light theme");
        assert_eq!(l.table[1], hex("#a40000"), "light red is the Console's");
        let h = colors_for(Theme::HighContrast);
        assert_eq!((h.fg, h.bg), (hex("#ffffff"), hex("#000000")));
        assert_eq!(h.table[2], hex("#73ff73"));
        assert_eq!(h.table[15], hex("#ffffff"));
    }

    #[test]
    fn the_text_of_every_theme_is_readable_on_its_background() {
        // WCAG contrast of the default foreground over the background and of the red used for errors.
        fn lum(c: Rgb) -> f64 {
            let f = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
        }
        let ratio = |a: Rgb, b: Rgb| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for theme in [Theme::Dark, Theme::Light, Theme::HighContrast] {
            let c = colors_for(theme);
            assert!(ratio(c.fg, c.bg) >= 7.0, "{theme:?} text");
            if theme != Theme::Dark {
                assert!(ratio(c.table[1], c.bg) >= 4.5, "{theme:?} red");
            }
        }
    }
}
