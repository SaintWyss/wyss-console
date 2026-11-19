//! The three GTK themes use named colors generated from the palette below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    HighContrast,
}

impl Theme {
    /// The `interface.theme` word of `preferences.json`.
    #[cfg(test)]
    pub fn from_word(word: &str) -> Option<Theme> {
        match word {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            "high_contrast" => Some(Theme::HighContrast),
            _ => None,
        }
    }

    fn column(self) -> usize {
        match self {
            Theme::Dark => 0,
            Theme::Light => 1,
            Theme::HighContrast => 2,
        }
    }

    pub fn prefers_dark(self) -> bool {
        matches!(self, Self::Dark | Self::HighContrast)
    }

}

/// `(css name, dark, light, high contrast)`. The CSS name becomes a GTK named color.
pub const TOKENS: &[(&str, [&str; 3])] = &[
    ("chrome", ["#151515", "#ebebeb", "#000000"]),
    ("bg", ["#0e0e0e", "#f5f5f5", "#000000"]),
    ("surface", ["#151515", "#ffffff", "#000000"]),
    ("surface-control", ["#1e1e1e", "#f5f5f5", "#000000"]),
    ("surface-hover", ["#232323", "#ececec", "#111111"]),
    ("surface-strong", ["#262626", "#e0e0e0", "#000000"]),
    ("surface-overlay", ["#1e1e1e", "#ffffff", "#000000"]),
    ("line", ["#2a2a2a", "#d9d9d9", "#ffffff"]),
    ("line-strong", ["#808080", "#6b6b6b", "#ffffff"]),
    ("text", ["#ececec", "#111111", "#ffffff"]),
    ("muted", ["#a3a3a3", "#3d3d3d", "#ffffff"]),
    ("faint", ["#737373", "#525252", "#ededed"]),
    ("accent", ["#e8e3d8", "#4a4337", "#ffffff"]),
    ("accent-strong", ["#f5f1e8", "#2f2a22", "#ffff00"]),
    ("accent-soft", ["#232220", "#ebe6dc", "#1a1a1a"]),
    ("accent-line", ["#4a463f", "#4a4337", "#ffffff"]),
    ("ok", ["#5ee39a", "#176b3a", "#73ff73"]),
    ("warn", ["#f5c044", "#6e4700", "#ffff00"]),
    ("warn-soft", ["#2d2410", "#fdf1cc", "#151500"]),
    ("warn-line", ["#a7832a", "#8a5a00", "#ffff00"]),
    ("error", ["#ff8080", "#a32622", "#ff6868"]),
    ("error-text", ["#ffd2d2", "#81201c", "#ffb0b0"]),
    ("error-soft", ["#341719", "#fde8e6", "#1b0000"]),
    ("error-line", ["#b9525a", "#a32622", "#ff6868"]),
    ("terminal-bg", ["#0e0e0e", "#ffffff", "#000000"]),
    ("text-on-accent", ["#121212", "#ffffff", "#000000"]),
];

pub fn token(theme: Theme, name: &str) -> Option<&'static str> {
    TOKENS.iter().find(|(n, _)| *n == name).map(|(_, v)| v[theme.column()])
}

/// The `@define-color` lines that go in front of `theme.css`.
pub fn defines(theme: Theme) -> String {
    TOKENS.iter().map(|(name, v)| format!("@define-color {} {};\n", name.replace('-', "_"), v[theme.column()])).collect()
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
            assert_eq!(expected[word].as_object().unwrap().len(), TOKENS.len());
            for (name, _) in TOKENS {
                assert_eq!(token(theme, name), expected[word][name].as_str(), "{word}: {name}");
            }
        }
    }

    #[test]
    fn the_defines_name_every_token_with_underscores() {
        let d = defines(Theme::Light);
        assert!(d.contains("@define-color surface_hover #ececec;"), "{d}");
        assert!(d.contains("@define-color bg #f5f5f5;"));
        assert_eq!(d.lines().count(), TOKENS.len());
        assert!(defines(Theme::HighContrast).contains("@define-color warn #ffff00;"));
    }

    #[test]
    fn gtk_variant_matches_the_app_background() {
        assert!(Theme::Dark.prefers_dark());
        assert!(!Theme::Light.prefers_dark());
        assert!(Theme::HighContrast.prefers_dark());
    }

    #[test]
    fn words_of_the_preference_map_to_themes() {
        assert_eq!(Theme::from_word("dark"), Some(Theme::Dark));
        assert_eq!(Theme::from_word("light"), Some(Theme::Light));
        assert_eq!(Theme::from_word("high_contrast"), Some(Theme::HighContrast));
        assert_eq!(Theme::from_word("sepia"), None);
        assert_eq!(Theme::from_word(""), None);
    }
}
