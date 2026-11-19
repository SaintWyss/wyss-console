use wyss_console::distributors::{Distributor, SaveRequest};

use crate::t;

pub const SUGGESTED: [(&str, &str, &str); 3] = [
    ("Kimi CLI", "Moonshot AI", "kimi"),
    ("Gemini CLI", "Google", "gemini"),
    ("Aider", "Aider", "aider"),
];

pub const KNOWN: [(&str, Option<&str>); 3] = [
    ("Anthropic", Some("claude update")),
    ("OpenAI", Some("codex update")),
    ("OpenCode", None),
];

#[derive(Default)]
pub struct Draft {
    pub id: Option<String>,
    pub fields: [String; 5],
}

impl From<&Distributor> for Draft {
    fn from(value: &Distributor) -> Self {
        Self {
            id: Some(value.id.clone()),
            fields: [
                value.name.clone(),
                value.maker.clone().unwrap_or_default(),
                value.open.clone(),
                value.update.clone().unwrap_or_default(),
                value.note.clone().unwrap_or_default(),
            ],
        }
    }
}

impl Draft {
    pub fn problem(&self) -> Option<&'static str> {
        let [name, _, open, update, _] = &self.fields;
        if name.trim().is_empty() {
            return Some(t!("Enter a name."));
        }
        if !open
            .trim()
            .starts_with(|c: char| c.is_ascii_alphanumeric() || "/._-".contains(c))
        {
            return Some(t!(
                "Command to open must start with a command name (like kimi) or an absolute path."
            ));
        }
        if open.contains(['\r', '\n']) || update.contains(['\r', '\n']) {
            return Some(t!("Commands to open and update must each be on one line."));
        }
        None
    }

    pub fn request(&self) -> SaveRequest {
        let [name, maker, open, update, note] = self.fields.each_ref().map(|field| field.trim());
        let optional = |field: &str| (!field.is_empty()).then(|| field.to_owned());
        SaveRequest {
            id: self.id.clone(),
            name: name.to_owned(),
            maker: optional(maker),
            open: open.to_owned(),
            update: optional(update),
            note: optional(note),
        }
    }
}

pub fn version(raw: &str) -> &str {
    let bytes = raw.as_bytes();
    for (start, _) in raw.char_indices().filter(|(_, c)| c.is_ascii_digit()) {
        let digits = |offset| {
            bytes[offset..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
        };
        let major = start + digits(start);
        if bytes.get(major) != Some(&b'.') || digits(major + 1) == 0 {
            continue;
        }
        let mut end = major + 1 + digits(major + 1);
        if bytes.get(end) == Some(&b'.') && digits(end + 1) != 0 {
            end += 1 + digits(end + 1);
        }
        if matches!(bytes.get(end), Some(b'-' | b'+')) {
            let suffix = bytes[end + 1..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
                .count();
            if suffix != 0 {
                end += 1 + suffix;
            }
        }
        return &raw[start..end];
    }
    raw.trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_the_form_and_shows_the_reported_version() {
        let mut draft = Draft::default();
        assert_eq!(draft.problem(), Some("Enter a name."));
        draft.fields[0] = "Example".into();
        draft.fields[2] = "example".into();
        assert_eq!(draft.problem(), None);
        draft.fields[3] = "one\ntwo".into();
        assert!(draft.problem().is_some());
        assert_eq!(version("codex-cli 0.169.0-beta.1"), "0.169.0-beta.1");
        assert_eq!(version("  nightly  "), "nightly");
    }

    #[test]
    fn distributor_save_round_trip() {
        crate::personalization_round_trip(
            "distributor",
            "distributors::model::tests::distributor_save_child",
        );
    }

    #[test]
    fn distributor_save_child() {
        use wyss_console::distributors::{self, Places};
        let Some(root) = crate::personalization_child_root("distributor") else {
            return;
        };
        let places = Places::from_env().unwrap();
        assert_eq!(places.folder, root.join("config/wyss-console/distributors"));
        assert_eq!(places.home, root.join("home"));
        assert_eq!(places.launches, root.join("config/wyss-console/launch"));
        std::fs::create_dir_all(places.folder.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&places.home).unwrap();
        let mut draft = Draft {
            fields: [" Example ", " Maker ", "example", "", "note"].map(str::to_owned),
            ..Draft::default()
        };
        let saved = distributors::save(&places, &draft.request()).unwrap();
        assert_eq!(
            distributors::list(&places).unwrap().distributors.as_slice(),
            std::slice::from_ref(&saved)
        );
        assert_eq!(
            distributors::make_launch(&places, &saved.id).unwrap(),
            "example"
        );
        draft = Draft::from(&saved);
        draft.fields[3] = "example update".into();
        distributors::save(&places, &draft.request()).unwrap();
        assert_eq!(
            distributors::get(&places, &saved.id)
                .unwrap()
                .update
                .as_deref(),
            Some("example update")
        );
        distributors::delete(&places, &saved.id).unwrap();
        assert!(distributors::list(&places).unwrap().distributors.is_empty());
    }
}
