//! Redaction at the adapter boundary, before any projection exists (C-07, CTL-07).
//!
//! What never passes, from 06 §4.2: values of environment maps, headers and tokens of
//! HTTP handlers, credentials in a URL, fields named like a secret. A command line is
//! configuration and is shown, but the values of its variable assignments, of flags named
//! like a secret and of authorisation headers are not, and neither is anything shaped
//! like a token (11 §5.4). The rules are conservative: doubt redacts, and what was
//! redacted is said, so the page can mark an entry that may hold a secret.
//!
//! Text that reaches the page is a `Shown`, and only this module makes one.
use super::model::{Bounds, SourceFailure, TooLarge};
use serde::Serialize;
use std::{collections::BTreeSet, ops::Range};

/// What stands where a secret was. It says nothing of the length of what it replaced.
pub const MASK: &str = "[redacted]";

/// Text that has been through redaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Shown(String);

impl Shown {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretClass {
    /// The value of an environment variable, in a map or an assignment.
    EnvironmentValue,
    /// An HTTP header's value or an authorisation scheme's credential.
    Header,
    /// The user information of a URL, or a `user:password` argument.
    UrlCredentials,
    /// A field or flag whose name says it holds a secret.
    SecretField,
    /// Something shaped like a token, whatever its place.
    TokenShape,
}

/// A text with its secrets replaced, and the classes that were found in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Redacted {
    pub text: Shown,
    pub found: BTreeSet<SecretClass>,
}

impl Redacted {
    /// Whether redaction found anything: the entry may hold a secret.
    pub fn may_hold_secret(&self) -> bool {
        !self.found.is_empty()
    }
}

type Edit = (Range<usize>, SecretClass);

/// Applies byte ranges to mask; overlapping ranges are merged and the first class kept.
fn apply(text: &str, mut edits: Vec<Edit>) -> Redacted {
    edits.retain(|(range, _)| !range.is_empty());
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    let mut out = String::with_capacity(text.len());
    let mut found = BTreeSet::new();
    let mut at = 0;
    for (range, class) in edits {
        found.insert(class);
        if range.end <= at {
            continue;
        }
        let start = range.start.max(at);
        out.push_str(&text[at..start]);
        if start == range.start {
            out.push_str(MASK);
        }
        at = range.end;
    }
    out.push_str(&text[at..]);
    Redacted {
        text: Shown(out),
        found,
    }
}

fn normalized(name: &str) -> String {
    name.chars()
        .filter(|c| !matches!(c, '_' | '-' | '.' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

/// A name that says its value is a secret: `apiKey`, `API_TOKEN`, `client-secret`,
/// `Authorization`, `password`, `SERVICE_KEY` and the like. A word that merely ends in
/// the letters (`hotkey`, `monkey`) is not one: `key`, `pass` and `pwd` count as a last
/// word, set apart by a separator or a capital.
pub fn is_secret_name(name: &str) -> bool {
    let normalized = normalized(name);
    const CONTAINS: [&str; 15] = [
        "token",
        "oauth",
        "secret",
        "password",
        "passwd",
        "passphrase",
        "apikey",
        "authorization",
        "credential",
        "cookie",
        "privatekey",
        "accesskey",
        "sessionkey",
        "bearer",
        "signature",
    ];
    const EXACT: [&str; 7] = ["auth", "key", "pat", "pin", "otp", "pass", "pwd"];
    CONTAINS.iter().any(|part| normalized.contains(part))
        || EXACT.contains(&normalized.as_str())
        || ends_as_a_word(name, &["key", "pass", "pwd"])
}

/// Whether the last word of `name` is one of `words`: after a separator, or a capital
/// after a lower-case letter or digit.
fn ends_as_a_word(name: &str, words: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    words.iter().any(|word| {
        let Some(head) = lower.strip_suffix(word) else {
            return false;
        };
        let Some(before) = head.chars().last() else {
            return false;
        };
        let first = name.as_bytes()[head.len()];
        matches!(before, '_' | '-' | '.' | ' ')
            || (first.is_ascii_uppercase()
                && (before.is_ascii_lowercase() || before.is_ascii_digit()))
    })
}

/// A name that counts or limits something named like a secret: `max_tokens`,
/// `model_auto_compact_token_limit`. Its number is configuration, not a secret.
fn is_count_name(name: &str) -> bool {
    let name = normalized(name);
    name.ends_with("tokens")
        || [
            "tokenlimit",
            "maxtoken",
            "tokencount",
            "tokenbudget",
            "tokensused",
        ]
        .iter()
        .any(|part| name.contains(part))
}

/// A key whose value is a map of names to secret values.
fn secret_map(name: &str) -> Option<SecretClass> {
    let name = normalized(name);
    if name.contains("header") {
        Some(SecretClass::Header)
    } else if matches!(name.as_str(), "env" | "environment" | "envvars" | "setenv") {
        Some(SecretClass::EnvironmentValue)
    } else {
        None
    }
}

/// A key whose value is a command line or its words.
fn command_key(name: &str) -> bool {
    let name = normalized(name);
    matches!(
        name.as_str(),
        "command" | "cmd" | "args" | "arguments" | "notify"
    ) || name.ends_with("command")
        || name.ends_with("helper")
}

const fn word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'+' | b'=' | b'.' | b'~' | b'/')
}

const fn name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')
}

const fn value_end(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'&' | b'#' | b';' | b'"' | b'\'' | b',' | b'>')
}

fn end_of_value(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| value_end(b))
        .map_or(bytes.len(), |n| from + n)
}

/// Prefixes of tokens issued by common services.
pub(super) const TOKEN_PREFIXES: [&str; 16] = [
    "sk-",
    "sk_",
    "pk_",
    "rk_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xox",
    "AKIA",
    "ASIA",
    "AIza",
    "eyJ",
];

/// A run that looks random: long, mixed, with digits, and spread over many symbols.
fn looks_random(run: &[u8]) -> bool {
    if run.len() < 24 {
        return false;
    }
    let digits = run.iter().filter(|b| b.is_ascii_digit()).count();
    let lower = run.iter().any(u8::is_ascii_lowercase);
    let upper = run.iter().any(u8::is_ascii_uppercase);
    if digits < 2 || !(lower || upper) {
        return false;
    }
    let mut counts = [0_u32; 256];
    for &b in run {
        counts[b as usize] += 1;
    }
    let len = run.len() as f64;
    let entropy: f64 = counts
        .iter()
        .filter(|&&n| n > 0)
        .map(|&n| {
            let p = f64::from(n) / len;
            -p * p.log2()
        })
        .sum();
    entropy >= 4.0
}

fn token_edits(bytes: &[u8], edits: &mut Vec<Edit>) {
    let mut i = 0;
    while i < bytes.len() {
        if !word_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && word_byte(bytes[i]) {
            i += 1;
        }
        // A path is judged by its parts, so a long path is not a token but a token in a
        // path (a webhook URL) is.
        let mut part = start;
        for end in (start..i).filter(|&n| bytes[n] == b'/').chain([i]) {
            let run = &bytes[part..end];
            let prefixed =
                run.len() >= 16 && TOKEN_PREFIXES.iter().any(|p| run.starts_with(p.as_bytes()));
            if prefixed || looks_random(run) {
                edits.push((part..end, SecretClass::TokenShape));
            }
            part = end + 1;
        }
    }
}

fn find_ignore_case(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|n| from + n)
}

/// Bytes that decorate a name or a value in quoted text: `"apiKey":"x"`, `\"apiKey\":\"x\"`.
const fn decoration(byte: u8) -> bool {
    matches!(byte, b'"' | b'\'' | b'\\')
}

/// Header values run to the end of the line (or the quote that closes the text around
/// them): `Cookie: a=1; b=2`.
fn line_valued(name: &str) -> bool {
    let name = normalized(name);
    name.contains("authorization") || name.contains("cookie")
}

/// `name=value` and `name: value`, bare or quoted, where the name says it is a secret.
/// Where a value is masked the scan resumes after it, so the work stays linear.
fn assignment_edits(text: &str, edits: &mut Vec<Edit>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !name_byte(bytes[i]) || (i > 0 && name_byte(bytes[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && name_byte(bytes[i]) {
            i += 1;
        }
        let name = &text[start..i];
        let mut at = i;
        while at < bytes.len() && decoration(bytes[at]) {
            at += 1;
        }
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        let Some(&separator) = bytes.get(at).filter(|b| matches!(b, b'=' | b':')) else {
            continue;
        };
        let secret_name = name.trim_start_matches('-');
        if !is_secret_name(secret_name) {
            continue;
        }
        let mut value = at + 1;
        while matches!(bytes.get(value), Some(b' ' | b'\t')) {
            value += 1;
        }
        let opening = value;
        while value < bytes.len() && decoration(bytes[value]) {
            value += 1;
        }
        let quote = bytes[opening..value]
            .iter()
            .copied()
            .find(|b| matches!(b, b'"' | b'\''));
        let end = match quote {
            Some(quote) => {
                // `"..."` honours a backslash escape; `\"...\"` ends at the first
                // backslash or quote.
                let escaping = !bytes[opening..value].contains(&b'\\');
                let mut end = value;
                while end < bytes.len() && bytes[end] != quote {
                    if bytes[end] == b'\\' {
                        if !escaping {
                            break;
                        }
                        end += 1;
                    }
                    end += 1;
                }
                end.min(bytes.len())
            }
            None if separator == b':' && line_valued(secret_name) => bytes[value..]
                .iter()
                .position(|b| matches!(b, b'\n' | b'\r' | b'"' | b'\''))
                .map_or(bytes.len(), |n| value + n),
            None => end_of_value(bytes, value),
        };
        let class = if separator == b':' && line_valued(secret_name) {
            SecretClass::Header
        } else {
            SecretClass::SecretField
        };
        edits.push((value..end, class));
        i = i.max(end);
    }
}

/// The rules for any text: URL credentials, secret query parameters and assignments,
/// authorisation schemes and headers, token shapes.
fn text_edits(text: &str) -> Vec<Edit> {
    let bytes = text.as_bytes();
    let mut edits = Vec::new();
    // scheme://user:password@host
    let mut from = 0;
    while let Some(at) = find_ignore_case(bytes, b"://", from) {
        let start = at + 3;
        let end = bytes[start..]
            .iter()
            .position(|&b| value_end(b) || matches!(b, b'/' | b'?'))
            .map_or(bytes.len(), |n| start + n);
        if let Some(user) = bytes[start..end].iter().rposition(|&b| b == b'@') {
            edits.push((start..start + user, SecretClass::UrlCredentials));
        }
        from = start;
    }
    assignment_edits(text, &mut edits);
    // Authorization: Bearer <credential>
    for scheme in [&b"bearer "[..], b"basic ", b"token "] {
        let mut from = 0;
        while let Some(at) = find_ignore_case(bytes, scheme, from) {
            let start = at + scheme.len();
            let boundary = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
            if boundary && (scheme != b"token " || looks_like_credential(bytes, start)) {
                edits.push((start..end_of_value(bytes, start), SecretClass::Header));
            }
            from = start;
        }
    }
    token_edits(bytes, &mut edits);
    edits
}

/// After `token `, a credential is one word with a digit in it: "token limit" is prose.
fn looks_like_credential(bytes: &[u8], start: usize) -> bool {
    bytes[start..end_of_value(bytes, start)]
        .iter()
        .any(u8::is_ascii_digit)
}

/// Redacts a text of no known kind.
pub fn text(text: &str) -> Redacted {
    apply(text, text_edits(text))
}

/// The words of a command line, as byte ranges, with their quotes. Unquoted shell
/// operators separate words.
fn words(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace()
            || matches!(bytes[i], b';' | b'|' | b'&' | b'(' | b')' | b'<' | b'>')
        {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => i += 2,
                b'\'' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'\'' {
                        i += 1;
                    }
                    i += 1;
                }
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                b if b.is_ascii_whitespace()
                    || matches!(b, b';' | b'|' | b'&' | b'(' | b')' | b'<' | b'>') =>
                {
                    break;
                }
                _ => i += 1,
            }
        }
        words.push(start..i.min(bytes.len()));
    }
    words
}

/// What the word after a flag is.
#[derive(Clone, Copy)]
enum Pending {
    None,
    Secret,
    Header,
    User,
}

/// The rules for the words of a command: assignments, flags named like a secret, header
/// and user arguments. `word` is one word as written; `range` places it in the text.
fn word_edits(word: &str, range: Range<usize>, pending: Pending, edits: &mut Vec<Edit>) -> Pending {
    let bytes = word.as_bytes();
    // A quoted word keeps its closing quote: the mask goes inside it.
    let closing = usize::from(
        bytes.len() > 1 && matches!(bytes[0], b'\'' | b'"') && bytes[bytes.len() - 1] == bytes[0],
    );
    let after = |from: usize| {
        let mut start = range.start + from;
        while start < range.end - closing && word.as_bytes()[start - range.start] == b' ' {
            start += 1;
        }
        start..range.end - closing
    };
    match pending {
        Pending::Secret => {
            edits.push((range, SecretClass::SecretField));
            return Pending::None;
        }
        Pending::Header => {
            let at = bytes.iter().position(|&b| b == b':').map_or(0, |n| n + 1);
            edits.push((after(at), SecretClass::Header));
            return Pending::None;
        }
        Pending::User => {
            if let Some(at) = bytes.iter().position(|&b| b == b':') {
                edits.push((after(at + 1), SecretClass::UrlCredentials));
            }
            return Pending::None;
        }
        Pending::None => (),
    }
    let assignment = bytes
        .first()
        .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        && bytes.iter().position(|&b| b == b'=').is_some_and(|eq| {
            bytes[..eq]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        });
    if assignment {
        let eq = bytes.iter().position(|&b| b == b'=').unwrap_or(0);
        edits.push((after(eq + 1), SecretClass::EnvironmentValue));
        return Pending::None;
    }
    if !word.starts_with('-') {
        return Pending::None;
    }
    let (flag, value) = match word.split_once('=') {
        Some((flag, _)) => (flag, Some(flag.len() + 1)),
        None => (word, None),
    };
    let name = flag.trim_start_matches('-');
    let next = if matches!(flag, "-H" | "--header") {
        Pending::Header
    } else if matches!(flag, "-u" | "--user") {
        Pending::User
    } else if is_secret_name(name) {
        Pending::Secret
    } else {
        return Pending::None;
    };
    match value {
        Some(at) => {
            word_edits(&word[at..], range.start + at..range.end, next, edits);
            Pending::None
        }
        None => next,
    }
}

/// Redacts a command line.
pub fn command(text: &str) -> Redacted {
    let mut edits = text_edits(text);
    let mut pending = Pending::None;
    for range in words(text) {
        pending = word_edits(&text[range.clone()], range, pending, &mut edits);
    }
    apply(text, edits)
}

/// Redacts the words of a command given as a list, as `args` and `notify` are.
pub fn arguments<S: AsRef<str>>(words: &[S]) -> Vec<Redacted> {
    let mut pending = Pending::None;
    words
        .iter()
        .map(|word| {
            let word = word.as_ref();
            let mut edits = text_edits(word);
            pending = word_edits(word, 0..word.len(), pending, &mut edits);
            apply(word, edits)
        })
        .collect()
}

/// What the page may see of a parsed value. Strings are redacted; a value whose place
/// says it is a secret is replaced by the class it belongs to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Projection {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    Text(Redacted),
    Hidden(SecretClass),
    List(Vec<Projection>),
    Map(Vec<(Shown, Projection)>),
}

impl Projection {
    /// Whether any part of this value was redacted.
    pub fn may_hold_secret(&self) -> bool {
        match self {
            Self::Null | Self::Bool(_) | Self::Number(_) => false,
            Self::Text(text) => text.may_hold_secret(),
            Self::Hidden(_) => true,
            Self::List(items) => items.iter().any(Self::may_hold_secret),
            Self::Map(entries) => entries
                .iter()
                .any(|(key, value)| key.as_str().contains(MASK) || value.may_hold_secret()),
        }
    }
}

/// Where a value is, which decides how its strings are treated.
#[derive(Clone, Copy)]
enum Place {
    Plain,
    Command,
    /// Inside a field named like a secret: every string is hidden.
    Secret(SecretClass),
    /// The value of a map of names to secrets: names stay, values go.
    SecretMap(SecretClass),
}

fn child_place(place: Place, key: &str) -> Place {
    match place {
        Place::Secret(class) | Place::SecretMap(class) => Place::Secret(class),
        Place::Plain | Place::Command => {
            if let Some(class) = secret_map(key) {
                Place::SecretMap(class)
            } else if command_key(key) {
                // `apiKeyHelper` names a program, not a key: shown, with its own
                // arguments redacted.
                Place::Command
            } else if is_secret_name(key) {
                Place::Secret(SecretClass::SecretField)
            } else {
                Place::Plain
            }
        }
    }
}

/// Whether an object is a name/value pair whose name says the value is a secret.
fn names_a_secret(entries: &serde_json::Map<String, serde_json::Value>) -> bool {
    ["name", "key", "header"].iter().any(|field| {
        entries
            .get(*field)
            .and_then(serde_json::Value::as_str)
            .is_some_and(is_secret_name)
    })
}

fn project_at(
    value: &serde_json::Value,
    place: Place,
    key: Option<&str>,
    depth: usize,
    bounds: &Bounds,
) -> Result<Projection, SourceFailure> {
    use serde_json::Value;
    if depth > bounds.depth {
        return Err(SourceFailure::TooLarge {
            bound: TooLarge::Depth,
        });
    }
    Ok(match value {
        Value::Null => Projection::Null,
        Value::Bool(b) => Projection::Bool(*b),
        // A number under a secret's name is hidden too (a PIN, a numeric token), unless
        // the name counts something: `max_tokens` is configuration.
        Value::Number(n) => match place {
            Place::Secret(class) if !key.is_some_and(is_count_name) => Projection::Hidden(class),
            Place::SecretMap(class) => Projection::Hidden(class),
            _ => Projection::Number(n.clone()),
        },
        Value::String(s) => match place {
            Place::Plain => Projection::Text(text(s)),
            Place::Command => Projection::Text(command(s)),
            Place::Secret(class) => Projection::Hidden(class),
            // `NAME=value` in a list form of an environment.
            Place::SecretMap(class) => match s.split_once('=') {
                Some((name, _)) => {
                    let mut shown = text(name);
                    shown.text.0.push('=');
                    shown.text.0.push_str(MASK);
                    shown.found.insert(class);
                    Projection::Text(shown)
                }
                None => Projection::Hidden(class),
            },
        },
        Value::Array(items) => match place {
            Place::Command if items.iter().all(Value::is_string) => Projection::List(
                arguments(&items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
                    .into_iter()
                    .map(Projection::Text)
                    .collect(),
            ),
            _ => Projection::List(
                items
                    .iter()
                    .map(|item| project_at(item, place, None, depth + 1, bounds))
                    .collect::<Result<_, _>>()?,
            ),
        },
        Value::Object(entries) => Projection::Map(
            entries
                .iter()
                .map(|(key, value)| {
                    // `{"name": "X-Api-Key", "value": "..."}`: the secret's name is in
                    // one field and its value in the next.
                    let child_place = if key == "value" && names_a_secret(entries) {
                        Place::Secret(SecretClass::SecretField)
                    } else {
                        child_place(place, key)
                    };
                    let child = project_at(value, child_place, Some(key), depth + 1, bounds)?;
                    Ok((text(key).text, child))
                })
                .collect::<Result<_, _>>()?,
        ),
    })
}

/// The projection of a parsed document, or of one entry of it, for the page.
pub fn project(value: &serde_json::Value, bounds: &Bounds) -> Result<Projection, SourceFailure> {
    project_at(value, Place::Plain, None, 0, bounds)
}
