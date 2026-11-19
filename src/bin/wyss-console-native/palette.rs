//! Colors of the terminal surface for the theme in force: default foreground and background, the
//! 256-color palette (base 16 + 6x6x6 cube + grayscale ramp), the selection, the cursor and the
//! search highlights. Each theme defines a base palette.

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

// ANSI order: black, red, green, yellow, blue, magenta, cyan, white, then bright variants.
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

// Terminal schemes and attribution are listed in THIRD-PARTY.md.
const CATPPUCCIN_MOCHA: [&str; 16] = [
    "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af",
    "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
    "#585b70", "#f7aec2", "#c2ecbf", "#fcd682",
    "#aeccfc", "#f398da", "#b1eae1", "#a6adc8",
];
const DRACULA: [&str; 16] = [
    "#21222c", "#ff5555", "#50fa7b", "#f1fa8c",
    "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
    "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5",
    "#d6acff", "#ff92df", "#a4ffff", "#ffffff",
];
const GRUVBOX_DARK: [&str; 16] = [
    "#282828", "#cc241d", "#98971a", "#d79921",
    "#458588", "#b16286", "#689d6a", "#a89984",
    "#928374", "#fb4934", "#b8bb26", "#fabd2f",
    "#83a598", "#d3869b", "#8ec07c", "#ebdbb2",
];
const MONOKAI: [&str; 16] = [
    "#272822", "#f92672", "#a6e22e", "#e6db74",
    "#fd971f", "#ae81ff", "#66d9ef", "#fdfff1",
    "#6e7066", "#f92672", "#a6e22e", "#e6db74",
    "#fd971f", "#ae81ff", "#66d9ef", "#fdfff1",
];
const NORD: [&str; 16] = [
    "#3b4252", "#bf616a", "#a3be8c", "#ebcb8b",
    "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
    "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b",
    "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4",
];
const ROSE_PINE: [&str; 16] = [
    "#26233a", "#eb6f92", "#31748f", "#f6c177",
    "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
    "#6e6a86", "#eb6f92", "#31748f", "#f6c177",
    "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
];
const SOLARIZED_LIGHT: [&str; 16] = [
    "#073642", "#dc322f", "#859900", "#b58900",
    "#268bd2", "#d33682", "#2aa198", "#eee8d5",
    "#002b36", "#cb4b16", "#586e75", "#657b83",
    "#839496", "#6c71c4", "#93a1a1", "#fdf6e3",
];
const TOKYO_NIGHT: [&str; 16] = [
    "#15161e", "#f7768e", "#9ece6a", "#e0af68",
    "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
    "#414868", "#f7768e", "#9ece6a", "#e0af68",
    "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
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
    let (ansi, selection, cursor) = match theme {
        Theme::Dark => return Colors {
            fg: tok("text"),
            bg: tok("bg"),
            sel_bg: hex("#3a4a63"),
            cursor: [0.77, 0.78, 0.78, 0.55],
            find: hex("#f5c044"),
            find_current: hex("#ff9f43"),
            table: table(DARK.map(|(r, g, b)| Rgb { r, g, b })),
        },
        Theme::Light => return Colors {
            fg: hex("#111111"),
            bg: hex("#ffffff"),
            sel_bg: hex("#b5d3f5"),
            cursor: [0.07, 0.07, 0.07, 0.45],
            find: hex("#ffd54a"),
            find_current: hex("#ff8a3d"),
            table: table(LIGHT.map(hex)),
        },
        Theme::HighContrast => return Colors {
            fg: hex("#ffffff"),
            bg: hex("#000000"),
            sel_bg: hex("#0047ab"),
            cursor: [1.0, 1.0, 1.0, 0.7],
            find: hex("#ffff00"),
            find_current: hex("#ff8c00"),
            table: table(HIGH_CONTRAST.map(hex)),
        },
        // The renderer retains the foreground on selection; choose a darker palette surface
        // in themes whose port pairs a light selection background with dark selection text.
        Theme::CatppuccinMocha => (CATPPUCCIN_MOCHA, "#45475a", "#f5e0dc"),
        Theme::Dracula => (DRACULA, "#44475a", "#f8f8f2"),
        Theme::GruvboxDark => (GRUVBOX_DARK, "#665c54", "#ebdbb2"),
        Theme::Monokai => (MONOKAI, "#57584f", "#c0c1b5"),
        Theme::Nord => (NORD, "#434c5e", "#eceff4"),
        Theme::RosePine => (ROSE_PINE, "#403d52", "#e0def4"),
        Theme::SolarizedLight => (SOLARIZED_LIGHT, "#f3edda", "#657b83"),
        Theme::TokyoNight => (TOKYO_NIGHT, "#283457", "#c0caf5"),
    };
    // The search overlay sits above the text, so these three themes need gentler tints.
    let (find, find_current) = match theme {
        Theme::GruvboxDark => (tok("warn-line"), hex("#765f2b")),
        Theme::Nord => (tok("warn-line"), hex("#4f6875")),
        Theme::SolarizedLight => (hex("#f4e8c5"), hex("#eaece1")),
        _ => (tok("warn-line"), tok("accent-line")),
    };
    let Rgb { r, g, b } = hex(cursor);
    Colors {
        fg: if theme == Theme::Monokai { hex("#fdfff1") } else { tok("text") },
        bg: tok("terminal-bg"),
        sel_bg: hex(selection),
        cursor: [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0, 0.55],
        find,
        find_current,
        table: table(ansi.map(hex)),
    }
}

/// The palette for the active theme, cached once per theme.
pub fn colors() -> &'static Colors {
    static CACHE: [OnceLock<Colors>; theme::ALL.len()] = [const { OnceLock::new() }; theme::ALL.len()];
    let theme = theme::current();
    CACHE[theme as usize].get_or_init(|| colors_for(theme))
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
        // WCAG relative luminance of an sRGB color.
        fn lum(c: Rgb) -> f64 {
            let f = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
        }
        let ratio = |a: Rgb, b: Rgb| {
            let (x, y) = (lum(a), lum(b));
            (x.max(y) + 0.05) / (x.min(y) + 0.05)
        };
        for theme in theme::ALL {
            let token = |name| hex(theme::token(theme, name).unwrap());
            let bg = token("bg");
            let colors = colors_for(theme);
            let text = ratio(token("text"), bg);
            let muted = ratio(token("muted"), bg);
            let accent = ratio(token("text-on-accent"), token("accent"));
            let terminal = ratio(colors.fg, colors.bg);
            println!("{theme:?}: text {text:.2}, muted {muted:.2}, accent {accent:.2}, terminal {terminal:.2}");
            assert!(text >= 4.5, "{theme:?} text: {text}");
            assert!(muted >= 3.0, "{theme:?} muted: {muted}");
            assert!(accent >= 4.5, "{theme:?} text-on-accent: {accent}");
            assert!(terminal >= 4.5, "{theme:?} terminal text: {terminal}");
            assert!(ratio(colors.fg, colors.sel_bg) >= 4.5, "{theme:?} terminal selection");
            match theme {
                Theme::Dark => assert!(terminal >= 7.0),
                Theme::Light | Theme::HighContrast => {
                    assert!(terminal >= 7.0);
                    assert!(ratio(colors.table[1], colors.bg) >= 4.5, "{theme:?} red");
                }
                _ => {}
            }
        }
    }
}
