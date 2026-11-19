//! The language of the window's own text. English is the source: every sentence is written in
//! English where it is used and is its own key. `tools/gen-i18n.py` generates `i18n_table.rs`
//! from `i18n/es.json`. A sentence with no entry is shown in English.
//!
//! Mark a sentence with `t!("…")` (a constant that is translated when drawn), `tr("…")` or
//! `trf("…", &[("name", value)])`: the generator finds the sentences by those three spellings.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// Fixes the language for the life of the process (the first call wins).
pub fn set(lang: Lang) {
    let _ = LANG.set(lang);
}

pub fn lang() -> Lang {
    LANG.get().copied().unwrap_or(Lang::En)
}

/// Marks a sentence that is stored as a constant and drawn through `tr`.
#[macro_export]
macro_rules! t {
    ($s:literal) => {
        $s
    };
}

/// The sentence in `lang`; the English one when the table has no Spanish for it.
pub fn translate(lang: Lang, text: &str) -> &str {
    match lang {
        Lang::En => text,
        Lang::Es => crate::i18n_table::ES.binary_search_by_key(&text, |(en, _)| en).map(|i| crate::i18n_table::ES[i].1).unwrap_or(text),
    }
}

/// `{name}` places replaced by their values; a name with no value stays as written.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if after[..close].chars().all(|c| c.is_alphanumeric() || c == '_') && close > 0 => {
                let name = &after[..close];
                match values.iter().find(|(n, _)| *n == name) {
                    Some((_, v)) => out.push_str(v),
                    None => out.push_str(&rest[open..open + close + 2]),
                }
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn tr(text: &str) -> String {
    translate(lang(), text).to_owned()
}

pub fn trf(text: &str, values: &[(&str, &str)]) -> String {
    fill(translate(lang(), text), values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_comes_from_the_consoles_table_and_english_is_the_key() {
        assert_eq!(translate(Lang::Es, "Stop"), "Parar");
        assert_eq!(translate(Lang::Es, "Restart"), "Reiniciar");
        assert_eq!(translate(Lang::Es, "Forget"), "Olvidar");
        assert_eq!(translate(Lang::Es, "Launch"), "Lanzar");
        assert_eq!(translate(Lang::En, "Stop"), "Stop");
        assert_eq!(translate(Lang::Es, "A sentence nobody translated"), "A sentence nobody translated");
    }

    #[test]
    fn the_table_is_sorted_for_the_binary_search_and_keeps_its_places() {
        let t = crate::i18n_table::ES;
        assert!(t.windows(2).all(|w| w[0].0 < w[1].0), "sorted by the English sentence");
        let names = |s: &str| {
            let mut v: Vec<String> = s.split('{').skip(1).filter_map(|p| p.split('}').next().map(str::to_owned)).collect();
            v.sort();
            v
        };
        for (en, es) in t {
            assert_eq!(names(en), names(es), "the {{places}} of {en:?} and its Spanish match");
        }
    }

    #[test]
    fn fill_replaces_named_places_only() {
        assert_eq!(fill("{current} of {total}{suffix}", &[("current", "2"), ("total", "7"), ("suffix", "")]), "2 of 7");
        assert_eq!(fill("Files in {folder}", &[]), "Files in {folder}");
        assert_eq!(fill("a { b } c", &[("b", "x")]), "a { b } c", "braces that are not a place stay");
        assert_eq!(fill("{x}{x}", &[("x", "ab")]), "abab");
    }

    #[test]
    fn every_sentence_the_window_uses_is_in_the_table() {
        // tools/gen-i18n.py --check fails when src/i18n_table.rs is stale; here the table is read
        // back for the sentences the sources mark, so a missing one fails `cargo test`.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/wyss-console-native");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            let skip = path.extension().is_none_or(|e| e != "rs") || ["i18n.rs", "i18n_table.rs"].iter().any(|n| path.ends_with(n));
            if skip {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for marker in ["t!(\"", "trf(\"", "tr(\""] {
                for (at, _) in text.match_indices(marker) {
                    // `str(` or `fn_tr(` are other names that merely end the same way.
                    if text[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_') {
                        continue;
                    }
                    let rest = &text[at + marker.len()..];
                    let Some(end) = rest.find('"') else { continue };
                    let sentence = &rest[..end];
                    if crate::i18n_table::ES.binary_search_by_key(&sentence, |(en, _)| en).is_err() {
                        missing.push(format!("{}: {sentence}", path.file_name().unwrap().to_string_lossy()));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "no Spanish for: {missing:#?}");
    }
}

pub fn incoming(text: &str) -> String {
    translate_incoming(lang(), text)
}

fn translate_incoming(lang: Lang, text: &str) -> String {
    if lang == Lang::Es {
        let table = crate::i18n_table::INCOMING_ES;
        if let Ok(index) = table.binary_search_by_key(&text, |(en, _)| en) {
            return table[index].1.into();
        }
        for (en, es) in table.iter().filter(|(en, _)| en.contains('{')) {
            if let Some(values) = incoming_parts(en, text) {
                return fill(es, &values);
            }
        }
    }
    translate(lang, text).into()
}

// Incoming templates from the desktop place literal delimiters between their substitutions.
fn incoming_parts<'a>(template: &'a str, text: &'a str) -> Option<Vec<(&'a str, &'a str)>> {
    let (prefix, mut template) = template.split_once('{')?;
    let mut text = text.strip_prefix(prefix)?;
    let mut values = Vec::new();
    loop {
        let (name, tail) = template.split_once('}')?;
        if let Some((delimiter, next)) = tail.split_once('{') {
            let (value, rest) = text.split_once(delimiter)?;
            values.push((name, value));
            text = rest;
            template = next;
        } else {
            values.push((name, text.strip_suffix(tail)?));
            return Some(values);
        }
    }
}

#[cfg(test)]
mod incoming_tests {
    use super::*;

    #[test]
    fn translates_dynamic_skill_blockers_like_workspace() {
        let reason = "The installed copy differs from revision 12: this session gets that revision from Arsenal. Your installed copy is not changed.";
        assert!(translate_incoming(Lang::Es, reason).starts_with("La copia instalada no coincide con la revisión 12: esta sesión usa esa revisión desde Arsenal."));
        let reason = "Claude Code reads this skill format, but it holds scripts whose own requirements are not checked. Wyss never runs them.";
        assert!(translate_incoming(Lang::Es, reason).starts_with("Claude Code lee este formato de skill"));
        assert_eq!(translate_incoming(Lang::En, reason), reason);
    }
}
