//! GTK named colors for each appearance theme.
pub use wyss_console::settings::AppearanceTheme as Theme;

pub const ALL: [Theme; 11] = [
    Theme::Dark,
    Theme::Light,
    Theme::HighContrast,
    Theme::CatppuccinMocha,
    Theme::Dracula,
    Theme::GruvboxDark,
    Theme::Monokai,
    Theme::Nord,
    Theme::RosePine,
    Theme::SolarizedLight,
    Theme::TokyoNight,
];

pub fn prefers_dark(theme: Theme) -> bool {
    !matches!(theme, Theme::Light | Theme::SolarizedLight)
}

type Tokens = [(&'static str, &'static str); 26];

const DARK: Tokens = [
    ("chrome", "#151515"),
    ("bg", "#0e0e0e"),
    ("surface", "#151515"),
    ("surface-control", "#1e1e1e"),
    ("surface-hover", "#232323"),
    ("surface-strong", "#262626"),
    ("surface-overlay", "#1e1e1e"),
    ("line", "#2a2a2a"),
    ("line-strong", "#808080"),
    ("text", "#ececec"),
    ("muted", "#a3a3a3"),
    ("faint", "#737373"),
    ("accent", "#e8e3d8"),
    ("accent-strong", "#f5f1e8"),
    ("accent-soft", "#232220"),
    ("accent-line", "#4a463f"),
    ("ok", "#5ee39a"),
    ("warn", "#f5c044"),
    ("warn-soft", "#2d2410"),
    ("warn-line", "#a7832a"),
    ("error", "#ff8080"),
    ("error-text", "#ffd2d2"),
    ("error-soft", "#341719"),
    ("error-line", "#b9525a"),
    ("terminal-bg", "#0e0e0e"),
    ("text-on-accent", "#121212"),
];

const LIGHT: Tokens = [
    ("chrome", "#ebebeb"),
    ("bg", "#f5f5f5"),
    ("surface", "#ffffff"),
    ("surface-control", "#f5f5f5"),
    ("surface-hover", "#ececec"),
    ("surface-strong", "#e0e0e0"),
    ("surface-overlay", "#ffffff"),
    ("line", "#d9d9d9"),
    ("line-strong", "#6b6b6b"),
    ("text", "#111111"),
    ("muted", "#3d3d3d"),
    ("faint", "#525252"),
    ("accent", "#4a4337"),
    ("accent-strong", "#2f2a22"),
    ("accent-soft", "#ebe6dc"),
    ("accent-line", "#4a4337"),
    ("ok", "#176b3a"),
    ("warn", "#6e4700"),
    ("warn-soft", "#fdf1cc"),
    ("warn-line", "#8a5a00"),
    ("error", "#a32622"),
    ("error-text", "#81201c"),
    ("error-soft", "#fde8e6"),
    ("error-line", "#a32622"),
    ("terminal-bg", "#ffffff"),
    ("text-on-accent", "#ffffff"),
];

const HIGH_CONTRAST: Tokens = [
    ("chrome", "#000000"),
    ("bg", "#000000"),
    ("surface", "#000000"),
    ("surface-control", "#000000"),
    ("surface-hover", "#111111"),
    ("surface-strong", "#000000"),
    ("surface-overlay", "#000000"),
    ("line", "#ffffff"),
    ("line-strong", "#ffffff"),
    ("text", "#ffffff"),
    ("muted", "#ffffff"),
    ("faint", "#ededed"),
    ("accent", "#ffffff"),
    ("accent-strong", "#ffff00"),
    ("accent-soft", "#1a1a1a"),
    ("accent-line", "#ffffff"),
    ("ok", "#73ff73"),
    ("warn", "#ffff00"),
    ("warn-soft", "#151500"),
    ("warn-line", "#ffff00"),
    ("error", "#ff6868"),
    ("error-text", "#ffb0b0"),
    ("error-soft", "#1b0000"),
    ("error-line", "#ff6868"),
    ("terminal-bg", "#000000"),
    ("text-on-accent", "#000000"),
];

const CATPPUCCIN_MOCHA: Tokens = [
    ("chrome", "#181825"),
    ("bg", "#1e1e2e"),
    ("surface", "#181825"),
    ("surface-control", "#313244"),
    ("surface-hover", "#45475a"),
    ("surface-strong", "#585b70"),
    ("surface-overlay", "#313244"),
    ("line", "#45475a"),
    ("line-strong", "#7f849c"),
    ("text", "#cdd6f4"),
    ("muted", "#a6adc8"),
    ("faint", "#9399b2"),
    ("accent", "#cba6f7"),
    ("accent-strong", "#b4befe"),
    ("accent-soft", "#343048"),
    ("accent-line", "#715f8e"),
    ("ok", "#a6e3a1"),
    ("warn", "#f9e2af"),
    ("warn-soft", "#3a373f"),
    ("warn-line", "#877c6c"),
    ("error", "#f38ba8"),
    ("error-text", "#f2cdcd"),
    ("error-soft", "#3a2c3e"),
    ("error-line", "#845269"),
    ("terminal-bg", "#1e1e2e"),
    ("text-on-accent", "#11111b"),
];

const DRACULA: Tokens = [
    ("chrome", "#21222c"),
    ("bg", "#282a36"),
    ("surface", "#303241"),
    ("surface-control", "#383a48"),
    ("surface-hover", "#44475a"),
    ("surface-strong", "#4d5166"),
    ("surface-overlay", "#343746"),
    ("line", "#44475a"),
    ("line-strong", "#7181b2"),
    ("text", "#f8f8f2"),
    ("muted", "#b2bce0"),
    ("faint", "#8e9bc5"),
    ("accent", "#bd93f9"),
    ("accent-strong", "#ff79c6"),
    ("accent-soft", "#3b384f"),
    ("accent-line", "#705c94"),
    ("ok", "#50fa7b"),
    ("warn", "#f1fa8c"),
    ("warn-soft", "#424541"),
    ("warn-line", "#888e5f"),
    ("error", "#ff5555"),
    ("error-text", "#ff6e6e"),
    ("error-soft", "#44303a"),
    ("error-line", "#8f3f45"),
    ("terminal-bg", "#282a36"),
    ("text-on-accent", "#21222c"),
];

const GRUVBOX_DARK: Tokens = [
    ("chrome", "#1d2021"),
    ("bg", "#282828"),
    ("surface", "#32302f"),
    ("surface-control", "#3c3836"),
    ("surface-hover", "#504945"),
    ("surface-strong", "#665c54"),
    ("surface-overlay", "#3c3836"),
    ("line", "#504945"),
    ("line-strong", "#928374"),
    ("text", "#ebdbb2"),
    ("muted", "#bdae93"),
    ("faint", "#a89984"),
    ("accent", "#fabd2f"),
    ("accent-strong", "#fe8019"),
    ("accent-soft", "#433b29"),
    ("accent-line", "#8d702b"),
    ("ok", "#b8bb26"),
    ("warn", "#fabd2f"),
    ("warn-soft", "#433b29"),
    ("warn-line", "#8d702b"),
    ("error", "#fb4934"),
    ("error-text", "#fe8019"),
    ("error-soft", "#432c2a"),
    ("error-line", "#8d382e"),
    ("terminal-bg", "#282828"),
    ("text-on-accent", "#1d2021"),
];

const MONOKAI: Tokens = [
    ("chrome", "#20211c"),
    ("bg", "#272822"),
    ("surface", "#30312a"),
    ("surface-control", "#3e3d32"),
    ("surface-hover", "#49483e"),
    ("surface-strong", "#57584f"),
    ("surface-overlay", "#383930"),
    ("line", "#49483e"),
    ("line-strong", "#88846f"),
    ("text", "#f8f8f2"),
    ("muted", "#c0c1b5"),
    ("faint", "#a6a89d"),
    ("accent", "#a6e22e"),
    ("accent-strong", "#e6db74"),
    ("accent-soft", "#384024"),
    ("accent-line", "#648128"),
    ("ok", "#a6e22e"),
    ("warn", "#fd971f"),
    ("warn-soft", "#433622"),
    ("warn-line", "#8e5d21"),
    ("error", "#f92672"),
    ("error-text", "#ff9bbc"),
    ("error-soft", "#42282c"),
    ("error-line", "#8c2748"),
    ("terminal-bg", "#272822"),
    ("text-on-accent", "#272822"),
];

const NORD: Tokens = [
    ("chrome", "#292e39"),
    ("bg", "#2e3440"),
    ("surface", "#353c4a"),
    ("surface-control", "#3b4252"),
    ("surface-hover", "#434c5e"),
    ("surface-strong", "#4c566a"),
    ("surface-overlay", "#3b4252"),
    ("line", "#434c5e"),
    ("line-strong", "#78859b"),
    ("text", "#d8dee9"),
    ("muted", "#b7c2d4"),
    ("faint", "#9aa9be"),
    ("accent", "#88c0d0"),
    ("accent-strong", "#8fbcbb"),
    ("accent-soft", "#3a4653"),
    ("accent-line", "#597785"),
    ("ok", "#a3be8c"),
    ("warn", "#ebcb8b"),
    ("warn-soft", "#47484a"),
    ("warn-line", "#897c64"),
    ("error", "#bf616a"),
    ("error-text", "#e1a0a7"),
    ("error-soft", "#413a45"),
    ("error-line", "#744a54"),
    ("terminal-bg", "#2e3440"),
    ("text-on-accent", "#2e3440"),
];

const ROSE_PINE: Tokens = [
    ("chrome", "#1f1d2e"),
    ("bg", "#191724"),
    ("surface", "#1f1d2e"),
    ("surface-control", "#26233a"),
    ("surface-hover", "#403d52"),
    ("surface-strong", "#524f67"),
    ("surface-overlay", "#26233a"),
    ("line", "#403d52"),
    ("line-strong", "#797593"),
    ("text", "#e0def4"),
    ("muted", "#908caa"),
    ("faint", "#797593"),
    ("accent", "#c4a7e7"),
    ("accent-strong", "#ebbcba"),
    ("accent-soft", "#2f2a3d"),
    ("accent-line", "#6b5c82"),
    ("ok", "#9ccfd8"),
    ("warn", "#f6c177"),
    ("warn-soft", "#362d2f"),
    ("warn-line", "#83694c"),
    ("error", "#eb6f92"),
    ("error-text", "#ebbcba"),
    ("error-soft", "#342232"),
    ("error-line", "#7e4159"),
    ("terminal-bg", "#191724"),
    ("text-on-accent", "#191724"),
];

const SOLARIZED_LIGHT: Tokens = [
    ("chrome", "#eee8d5"),
    ("bg", "#fdf6e3"),
    ("surface", "#fffaed"),
    ("surface-control", "#f4eedb"),
    ("surface-hover", "#eee8d5"),
    ("surface-strong", "#e1dac6"),
    ("surface-overlay", "#fffaed"),
    ("line", "#ded7c1"),
    ("line-strong", "#839496"),
    ("text", "#586e75"),
    ("muted", "#657b83"),
    ("faint", "#758990"),
    ("accent", "#268bd2"),
    ("accent-strong", "#2075b1"),
    ("accent-soft", "#e1e8e1"),
    ("accent-line", "#96c3db"),
    ("ok", "#627200"),
    ("warn", "#805f00"),
    ("warn-soft", "#ede2c5"),
    ("warn-line", "#c1ae76"),
    ("error", "#dc322f"),
    ("error-text", "#a22724"),
    ("error-soft", "#f9ddcc"),
    ("error-line", "#ed988d"),
    ("terminal-bg", "#fdf6e3"),
    ("text-on-accent", "#001b22"),
];

const TOKYO_NIGHT: Tokens = [
    ("chrome", "#16161e"),
    ("bg", "#1a1b26"),
    ("surface", "#1f2335"),
    ("surface-control", "#24283b"),
    ("surface-hover", "#292e42"),
    ("surface-strong", "#3b4261"),
    ("surface-overlay", "#24283b"),
    ("line", "#3b4261"),
    ("line-strong", "#737aa2"),
    ("text", "#c0caf5"),
    ("muted", "#a9b1d6"),
    ("faint", "#9aa5ce"),
    ("accent", "#7aa2f7"),
    ("accent-strong", "#7dcfff"),
    ("accent-soft", "#262d41"),
    ("accent-line", "#485c8a"),
    ("ok", "#9ece6a"),
    ("warn", "#e0af68"),
    ("warn-soft", "#342e2f"),
    ("warn-line", "#796246"),
    ("error", "#f7768e"),
    ("error-text", "#ff9e9e"),
    ("error-soft", "#372734"),
    ("error-line", "#844758"),
    ("terminal-bg", "#1a1b26"),
    ("text-on-accent", "#15161e"),
];

fn tokens(theme: Theme) -> &'static Tokens {
    match theme {
        Theme::Dark => &DARK,
        Theme::Light => &LIGHT,
        Theme::HighContrast => &HIGH_CONTRAST,
        Theme::CatppuccinMocha => &CATPPUCCIN_MOCHA,
        Theme::Dracula => &DRACULA,
        Theme::GruvboxDark => &GRUVBOX_DARK,
        Theme::Monokai => &MONOKAI,
        Theme::Nord => &NORD,
        Theme::RosePine => &ROSE_PINE,
        Theme::SolarizedLight => &SOLARIZED_LIGHT,
        Theme::TokyoNight => &TOKYO_NIGHT,
    }
}

pub fn token(theme: Theme, name: &str) -> Option<&'static str> {
    tokens(theme)
        .iter()
        .find_map(|(key, value)| (*key == name).then_some(*value))
}

/// The `@define-color` lines that go in front of `theme.css`.
pub fn defines(theme: Theme) -> String {
    let mut css: String = tokens(theme)
        .iter()
        .map(|(name, value)| format!("@define-color {} {};\n", name.replace('-', "_"), value))
        .collect();
    // Bars are told apart by tone; only high contrast keeps a line between them.
    let edge = if theme == Theme::HighContrast { token(theme, "line").unwrap_or("#ffffff") } else { "transparent" };
    css.push_str(&format!("@define-color edge {edge};\n"));
    css
}

thread_local! {
    static CURRENT: std::cell::Cell<Theme> = const { std::cell::Cell::new(Theme::Dark) };
}

pub fn set(theme: Theme) {
    CURRENT.set(theme);
}

pub fn current() -> Theme {
    CURRENT.get()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_token_matches_the_palette_fixture_in_all_three_themes() {
        let expected: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/theme-tokens.json"
        ))
        .unwrap();
        for (word, theme) in [
            ("dark", Theme::Dark),
            ("light", Theme::Light),
            ("high_contrast", Theme::HighContrast),
        ] {
            assert_eq!(expected[word].as_object().unwrap().len(), tokens(theme).len());
            for (name, value) in expected[word].as_object().unwrap() {
                assert_eq!(token(theme, name), value.as_str(), "{word}: {name}");
            }
        }
    }

    #[test]
    fn the_defines_name_every_token_with_underscores() {
        let d = defines(Theme::Light);
        assert!(d.contains("@define-color surface_hover #ececec;"), "{d}");
        assert!(d.contains("@define-color bg #f5f5f5;"));
        // Every token plus the `edge` color of the lines between bars.
        assert_eq!(d.lines().count(), tokens(Theme::Light).len() + 1);
        assert!(defines(Theme::HighContrast).contains("@define-color warn #ffff00;"));
    }

    #[test]
    fn only_high_contrast_draws_lines_between_bars() {
        assert!(defines(Theme::Dark).contains("@define-color edge transparent;"));
        assert!(defines(Theme::Light).contains("@define-color edge transparent;"));
        assert!(defines(Theme::HighContrast).contains("@define-color edge #ffffff;"));
    }

    #[test]
    fn gtk_variant_matches_the_app_background() {
        let light: Vec<_> = ALL.into_iter().filter(|theme| !prefers_dark(*theme)).collect();
        assert_eq!(light, [Theme::Light, Theme::SolarizedLight]);
    }

    #[test]
    fn every_theme_defines_the_same_tokens_once() {
        use std::collections::BTreeSet;

        let names = |theme| tokens(theme).iter().map(|(name, _)| *name).collect::<BTreeSet<_>>();
        let expected = names(Theme::Dark);
        for theme in ALL {
            assert_eq!(tokens(theme).len(), expected.len(), "{theme:?}");
            assert_eq!(names(theme), expected, "{theme:?}");
        }
    }
}
