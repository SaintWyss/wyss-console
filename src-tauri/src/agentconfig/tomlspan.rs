//! A TOML reader that remembers where each statement is, so that one key can be set or
//! removed in the bytes of a file and nothing else changes: comments, blank lines, key
//! order, quoting and line endings stay the user's (F2b, AC-F2b-03).
//!
//! It is deliberately smaller than TOML. It understands what a Codex configuration needs
//! to be edited safely: table and array-of-tables headers, key/value statements (bare,
//! quoted and dotted keys), comments, basic and literal strings, multi-line strings, and
//! values that span lines (arrays, inline tables). A layout it cannot place with certainty
//! (a table defined through an inline table, say) is refused, not guessed at. Every edit is
//! checked afterwards by `codex_plan` against the same edit made to the parsed value.
use super::jsonspan::Edit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Full path: the header's path followed by the key's own (possibly dotted) path.
    pub path: Vec<String>,
    /// The value's bytes, without a trailing comment or the end of the line.
    pub value: (usize, usize),
    /// The whole statement: from the start of its first line to just after its line ending.
    pub line: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    /// `[a.b]` or `[[a.b]]`; empty for the part before the first header.
    pub path: Vec<String>,
    pub array: bool,
    /// The header's line, including its ending; `(0, 0)` for the root.
    pub header: (usize, usize),
    pub entries: Vec<Entry>,
    /// Where this table's body ends: just after its last statement, or after its header.
    pub body_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub tables: Vec<Table>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanError {
    /// Text this scanner does not place with certainty.
    Unsupported,
}

pub fn parse(text: &str) -> Result<Document, SpanError> {
    let bytes = text.as_bytes();
    let mut tables = vec![Table {
        path: Vec::new(),
        array: false,
        header: (0, 0),
        entries: Vec::new(),
        body_end: 0,
    }];
    let mut at = 0;
    while at < bytes.len() {
        let line_start = at;
        let mut i = skip_blank(bytes, at);
        let line_end = line_end(bytes, line_start);
        match bytes.get(i) {
            None | Some(b'\n' | b'\r') => {
                at = next_line(bytes, line_start);
                continue;
            }
            Some(b'#') => {
                at = next_line(bytes, line_start);
                continue;
            }
            Some(b'[') => {
                let array = bytes.get(i + 1) == Some(&b'[');
                i += if array { 2 } else { 1 };
                let (path, after) = key_path(bytes, i, if array { b"]]" } else { b"]" })?;
                let rest = skip_blank(bytes, after);
                if !matches!(bytes.get(rest), None | Some(b'#' | b'\n' | b'\r')) {
                    return Err(SpanError::Unsupported);
                }
                let end = next_line(bytes, line_start);
                tables.push(Table {
                    path,
                    array,
                    header: (line_start, end),
                    entries: Vec::new(),
                    body_end: end,
                });
                at = end;
            }
            Some(_) => {
                let (key, after_key) = key_path(bytes, i, b"=")?;
                let value_start = skip_blank(bytes, after_key);
                let value_end = value_extent(bytes, value_start)?;
                let rest = skip_blank(bytes, value_end);
                if !matches!(bytes.get(rest), None | Some(b'#' | b'\n' | b'\r')) {
                    return Err(SpanError::Unsupported);
                }
                let end = next_line(bytes, value_end.max(line_end));
                let table = tables.last_mut().ok_or(SpanError::Unsupported)?;
                let mut path = table.path.clone();
                path.extend(key);
                table.entries.push(Entry {
                    path,
                    value: (value_start, value_end),
                    line: (line_start, end),
                });
                table.body_end = end;
                at = end;
            }
        }
    }
    Ok(Document { tables })
}

fn skip_blank(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    i
}

fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |p| from + p)
}

/// Just after the line ending of the line that contains `from`.
fn next_line(bytes: &[u8], from: usize) -> usize {
    let end = line_end(bytes, from);
    (end + 1).min(bytes.len().max(end))
}

/// A (possibly dotted) key path, up to and not including `stop`. Returns the parts and the
/// index of the stop.
fn key_path(bytes: &[u8], mut i: usize, stop: &[u8]) -> Result<(Vec<String>, usize), SpanError> {
    let mut parts = Vec::new();
    loop {
        i = skip_blank(bytes, i);
        let (part, after) = match bytes.get(i) {
            Some(b'"') => quoted(bytes, i, true)?,
            Some(b'\'') => quoted(bytes, i, false)?,
            Some(b) if b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-' => {
                let end = bytes[i..]
                    .iter()
                    .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'))
                    .map_or(bytes.len(), |p| i + p);
                (String::from_utf8_lossy(&bytes[i..end]).into_owned(), end)
            }
            _ => return Err(SpanError::Unsupported),
        };
        parts.push(part);
        i = skip_blank(bytes, after);
        match bytes.get(i) {
            Some(b'.') => i += 1,
            _ if bytes[i..].starts_with(stop) => return Ok((parts, i + stop.len())),
            _ => return Err(SpanError::Unsupported),
        }
    }
}

/// A single-line quoted key or string starting at `i`; returns its text and the index after.
fn quoted(bytes: &[u8], i: usize, basic: bool) -> Result<(String, usize), SpanError> {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' if basic => j += 2,
            b if b == quote => {
                let raw = &bytes[i + 1..j];
                let text = if basic {
                    unescape(raw)?
                } else {
                    String::from_utf8_lossy(raw).into_owned()
                };
                return Ok((text, j + 1));
            }
            b'\n' => return Err(SpanError::Unsupported),
            _ => j += 1,
        }
    }
    Err(SpanError::Unsupported)
}

fn unescape(raw: &[u8]) -> Result<String, SpanError> {
    // A key with an escape is read by the real parser's rules.
    let wrapped = format!("\"{}\"", String::from_utf8_lossy(raw));
    let table: Result<toml::Value, _> = toml::from_str(&format!("k = {wrapped}"));
    match table {
        Ok(toml::Value::Table(mut t)) => match t.remove("k") {
            Some(toml::Value::String(s)) => Ok(s),
            _ => Err(SpanError::Unsupported),
        },
        _ => Err(SpanError::Unsupported),
    }
}

/// The end of the value that starts at `start`: a string, an array or inline table (which
/// may span lines), or a bare value up to a comment or the end of the line.
fn value_extent(bytes: &[u8], start: usize) -> Result<usize, SpanError> {
    let mut i = start;
    let mut depth = 0_usize;
    loop {
        match bytes.get(i) {
            None => break,
            Some(b'"') | Some(b'\'') => i = string_end(bytes, i)?,
            Some(b'[' | b'{') => {
                depth += 1;
                i += 1;
            }
            Some(b']' | b'}') => {
                depth = depth.checked_sub(1).ok_or(SpanError::Unsupported)?;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            Some(b'#') if depth > 0 => {
                i = line_end(bytes, i);
            }
            Some(b'#' | b'\n' | b'\r') if depth == 0 => break,
            Some(b' ' | b'\t') if depth == 0 && i > start => {
                // A bare value ends at its last non-blank byte.
                let rest = skip_blank(bytes, i);
                if matches!(bytes.get(rest), None | Some(b'#' | b'\n' | b'\r')) {
                    break;
                }
                i += 1;
            }
            Some(_) => i += 1,
        }
        // A scalar string or bare value is complete when nothing opened remains.
        if depth == 0 && matches!(bytes.get(start), Some(b'"' | b'\'')) && i > start {
            break;
        }
    }
    if depth != 0 || i == start {
        return Err(SpanError::Unsupported);
    }
    // Trailing blanks are not part of the value.
    let mut end = i;
    while end > start && matches!(bytes[end - 1], b' ' | b'\t') {
        end -= 1;
    }
    Ok(end)
}

/// The index after the string that starts at `i` (basic or literal, single or multi-line).
fn string_end(bytes: &[u8], i: usize) -> Result<usize, SpanError> {
    let quote = bytes[i];
    let multi = bytes[i..].starts_with(&[quote, quote, quote]);
    if multi {
        let mut j = i + 3;
        while j < bytes.len() {
            if quote == b'"' && bytes[j] == b'\\' {
                j += 2;
            } else if bytes[j..].starts_with(&[quote, quote, quote]) {
                // Up to two more quotes may belong to the string's own end.
                let mut k = j + 3;
                while bytes.get(k) == Some(&quote) && k < j + 5 {
                    k += 1;
                }
                return Ok(k);
            } else {
                j += 1;
            }
        }
        return Err(SpanError::Unsupported);
    }
    let mut j = i + 1;
    while j < bytes.len() {
        match bytes[j] {
            b'\\' if quote == b'"' => j += 2,
            b if b == quote => return Ok(j + 1),
            b'\n' => return Err(SpanError::Unsupported),
            _ => j += 1,
        }
    }
    Err(SpanError::Unsupported)
}

impl Document {
    /// The tables whose header path is `path`, in file order (an array of tables has many).
    pub fn tables_at(&self, path: &[&str]) -> Vec<&Table> {
        self.tables
            .iter()
            .filter(|t| t.path.iter().map(String::as_str).eq(path.iter().copied()))
            .collect()
    }
}

impl Table {
    /// The entry whose full path is `path` (header path then key path).
    pub fn entry(&self, path: &[String]) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == path)
    }
}

/// Where a new `key = value` line goes in `table`: after its last statement, or after its
/// header, and on a line of its own even if the file does not end with a newline.
pub fn insert_statement(text: &str, table: &Table, key: &str, value: &str) -> Edit {
    let line = if text.contains("\r\n") {
        format!("{key} = {value}\r\n")
    } else {
        format!("{key} = {value}\n")
    };
    let at = table.body_end;
    let needs_break = at > 0 && !text[..at].ends_with('\n');
    Edit {
        at,
        remove: 0,
        insert: if needs_break {
            format!(
                "{}{line}",
                if line.ends_with("\r\n") { "\r\n" } else { "\n" }
            )
        } else {
            line
        },
    }
}

/// Replaces the value of an existing statement, and nothing else of its line.
pub fn replace_value(entry: &Entry, value: &str) -> Edit {
    Edit {
        at: entry.value.0,
        remove: entry.value.1 - entry.value.0,
        insert: value.to_owned(),
    }
}

/// Removes a whole statement line, with its ending.
pub fn remove_statement(entry: &Entry) -> Edit {
    Edit {
        at: entry.line.0,
        remove: entry.line.1 - entry.line.0,
        insert: String::new(),
    }
}
