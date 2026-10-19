//! A JSON reader that remembers where everything is, so that a change can be made to the
//! bytes of a file and to nothing else (07 §5.3).
//!
//! A change that goes through a parser and a printer rewrites the user's layout, key order
//! and spacing; this one removes or inserts exactly the bytes of the thing it changes, and
//! the rest of the file is the same bytes it was. Every change is checked afterwards
//! against the same change made to the parsed value (`mutate.rs`), so a mistake here is
//! caught by the next line, not by the user.
//!
//! It accepts strict JSON and refuses what makes a position ambiguous: a repeated key, a
//! document deeper than the bound, anything after the document.
use serde_json::Value;

const MOST_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub key: String,
    pub key_span: Span,
    pub value: Node,
}

impl Member {
    /// From the opening quote of the key to the end of the value.
    pub fn span(&self) -> Span {
        Span {
            start: self.key_span.start,
            end: self.value.span().end,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Object { span: Span, members: Vec<Member> },
    Array { span: Span, items: Vec<Node> },
    Scalar { span: Span },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanError {
    Invalid,
    DuplicateKey,
    TooDeep,
}

impl Node {
    pub fn span(&self) -> Span {
        match self {
            Self::Object { span, .. } | Self::Array { span, .. } | Self::Scalar { span } => *span,
        }
    }
    pub fn member(&self, key: &str) -> Option<&Member> {
        match self {
            Self::Object { members, .. } => members.iter().find(|m| m.key == key),
            _ => None,
        }
    }
    pub fn items(&self) -> Option<&[Node]> {
        match self {
            Self::Array { items, .. } => Some(items),
            _ => None,
        }
    }
    pub fn members(&self) -> Option<&[Member]> {
        match self {
            Self::Object { members, .. } => Some(members),
            _ => None,
        }
    }
    /// The spans of the children, in order: members span key to value.
    pub fn child_spans(&self) -> Vec<Span> {
        match self {
            Self::Object { members, .. } => members.iter().map(Member::span).collect(),
            Self::Array { items, .. } => items.iter().map(Node::span).collect(),
            Self::Scalar { .. } => Vec::new(),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    /// JSONC: comments count as blanks and a comma may precede a closing bracket.
    lenient: bool,
}

pub fn parse(bytes: &[u8]) -> Result<Node, SpanError> {
    read(bytes, false)
}

/// The same, for JSON with comments and trailing commas (OpenCode's `opencode.jsonc`). Spans
/// are of the file's own bytes, so an edit made from them changes those bytes and no others.
pub fn parse_jsonc(bytes: &[u8]) -> Result<Node, SpanError> {
    read(bytes, true)
}

fn read(bytes: &[u8], lenient: bool) -> Result<Node, SpanError> {
    let mut reader = Reader {
        bytes,
        at: 0,
        lenient,
    };
    reader.blanks();
    let node = reader.value(0)?;
    reader.blanks();
    if reader.at != bytes.len() {
        return Err(SpanError::Invalid);
    }
    Ok(node)
}

impl Reader<'_> {
    fn blanks(&mut self) {
        loop {
            while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                self.at += 1;
            }
            if !self.lenient || self.peek() != Some(b'/') {
                return;
            }
            match self.bytes.get(self.at + 1) {
                Some(b'/') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.at += 1;
                    }
                }
                Some(b'*') => {
                    // An unterminated comment is not a blank: the parse fails there.
                    match self.bytes[self.at + 2..]
                        .windows(2)
                        .position(|w| w == b"*/")
                    {
                        Some(end) => self.at += 2 + end + 2,
                        None => return,
                    }
                }
                _ => return,
            }
        }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }
    fn value(&mut self, depth: usize) -> Result<Node, SpanError> {
        if depth > MOST_DEPTH {
            return Err(SpanError::TooDeep);
        }
        match self.peek().ok_or(SpanError::Invalid)? {
            b'{' => self.object(depth),
            b'[' => self.array(depth),
            b'"' => {
                let span = self.string()?;
                Ok(Node::Scalar { span })
            }
            _ => self.literal(),
        }
    }
    fn string(&mut self) -> Result<Span, SpanError> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.peek().ok_or(SpanError::Invalid)? {
                b'"' => {
                    self.at += 1;
                    return Ok(Span {
                        start,
                        end: self.at,
                    });
                }
                b'\\' => self.at += 2,
                byte if byte < 0x20 => return Err(SpanError::Invalid),
                _ => self.at += 1,
            }
        }
    }
    fn literal(&mut self) -> Result<Node, SpanError> {
        let start = self.at;
        while matches!(self.peek(), Some(byte) if !matches!(byte, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.at += 1;
        }
        let span = Span {
            start,
            end: self.at,
        };
        // The text must be a JSON scalar, not merely something without a delimiter.
        match serde_json::from_slice::<Value>(&self.bytes[start..self.at]) {
            Ok(Value::Null | Value::Bool(_) | Value::Number(_)) => Ok(Node::Scalar { span }),
            _ => Err(SpanError::Invalid),
        }
    }
    fn array(&mut self, depth: usize) -> Result<Node, SpanError> {
        let start = self.at;
        self.at += 1;
        let mut items = Vec::new();
        self.blanks();
        if self.peek() == Some(b']') {
            self.at += 1;
        } else {
            loop {
                self.blanks();
                items.push(self.value(depth + 1)?);
                self.blanks();
                match self.peek().ok_or(SpanError::Invalid)? {
                    b',' => {
                        self.at += 1;
                        self.blanks();
                        if self.lenient && self.peek() == Some(b']') {
                            self.at += 1;
                            break;
                        }
                    }
                    b']' => {
                        self.at += 1;
                        break;
                    }
                    _ => return Err(SpanError::Invalid),
                }
            }
        }
        Ok(Node::Array {
            span: Span {
                start,
                end: self.at,
            },
            items,
        })
    }
    fn object(&mut self, depth: usize) -> Result<Node, SpanError> {
        let start = self.at;
        self.at += 1;
        let mut members: Vec<Member> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        self.blanks();
        if self.peek() == Some(b'}') {
            self.at += 1;
        } else {
            loop {
                self.blanks();
                if self.peek() != Some(b'"') {
                    return Err(SpanError::Invalid);
                }
                let key_span = self.string()?;
                let key: String = serde_json::from_slice(&self.bytes[key_span.start..key_span.end])
                    .map_err(|_| SpanError::Invalid)?;
                if !seen.insert(key.clone()) {
                    return Err(SpanError::DuplicateKey);
                }
                self.blanks();
                if self.peek() != Some(b':') {
                    return Err(SpanError::Invalid);
                }
                self.at += 1;
                self.blanks();
                let value = self.value(depth + 1)?;
                members.push(Member {
                    key,
                    key_span,
                    value,
                });
                self.blanks();
                match self.peek().ok_or(SpanError::Invalid)? {
                    b',' => {
                        self.at += 1;
                        self.blanks();
                        if self.lenient && self.peek() == Some(b'}') {
                            self.at += 1;
                            break;
                        }
                    }
                    b'}' => {
                        self.at += 1;
                        break;
                    }
                    _ => return Err(SpanError::Invalid),
                }
            }
        }
        Ok(Node::Object {
            span: Span {
                start,
                end: self.at,
            },
            members,
        })
    }
}

/// One replacement of a stretch of bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub at: usize,
    pub remove: usize,
    pub insert: String,
}

impl Edit {
    pub fn apply(&self, bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + self.insert.len());
        out.extend_from_slice(&bytes[..self.at]);
        out.extend_from_slice(self.insert.as_bytes());
        out.extend_from_slice(&bytes[self.at + self.remove..]);
        out
    }
}

/// What a removal took and how it sat, for an exact restoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub edit: Edit,
    pub removed: String,
    pub anchor: super::restore::Anchor,
    /// The child on its own, without the separator that went with it.
    pub unit: String,
}

fn slice(bytes: &[u8], span: Span) -> String {
    String::from_utf8_lossy(&bytes[span.start..span.end]).into_owned()
}

/// Removes child `index` of `container`, with the separator that goes with it, so that what
/// is left is laid out as before: the next sibling takes its place, or, for the last, the
/// separator before it goes. The only child leaves the container empty.
pub fn remove_child(bytes: &[u8], container: &Node, index: usize) -> Option<Removal> {
    use super::restore::Anchor;
    let children = container.child_spans();
    let child = *children.get(index)?;
    let (at, end, anchor) = if children.len() == 1 {
        let open = container.span().start + 1;
        let close = container.span().end - 1;
        (open, close, Anchor::Alone)
    } else if index + 1 < children.len() {
        (child.start, children[index + 1].start, Anchor::BeforeNext)
    } else {
        (children[index - 1].end, child.end, Anchor::AfterPrevious)
    };
    Some(Removal {
        edit: Edit {
            at,
            remove: end - at,
            insert: String::new(),
        },
        removed: String::from_utf8_lossy(&bytes[at..end]).into_owned(),
        anchor,
        unit: slice(bytes, child),
    })
}

/// The whitespace that opens a line at `at`: the indentation of the line it is on.
fn indentation(bytes: &[u8], at: usize) -> String {
    let line = bytes[..at]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    bytes[line..at]
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .map(|&b| b as char)
        .collect()
}

/// Appends `text` (a value for an array, `"key": value` for an object) as the last child,
/// in the layout the container already has: the separator it puts between its children, or
/// the whitespace it puts before its first, or a new line a step in from its own.
pub fn append_child(bytes: &[u8], container: &Node, text: &str) -> Edit {
    let children = container.child_spans();
    let span = container.span();
    match children.as_slice() {
        [] => {
            let base = indentation(bytes, span.start);
            let unit = if base.contains('\t') { "\t" } else { "  " };
            Edit {
                at: span.start + 1,
                remove: span.end - 1 - (span.start + 1),
                insert: format!("\n{base}{unit}{text}\n{base}"),
            }
        }
        [only] => {
            let before = String::from_utf8_lossy(&bytes[span.start + 1..only.start]).into_owned();
            Edit {
                at: only.end,
                remove: 0,
                insert: format!(",{before}{text}"),
            }
        }
        [.., previous, last] => {
            let between = String::from_utf8_lossy(&bytes[previous.end..last.start]).into_owned();
            Edit {
                at: last.end,
                remove: 0,
                insert: format!("{between}{text}"),
            }
        }
    }
}

/// The member text for a new key: `"key": value`.
pub fn member_text(key: &str, value: &str) -> String {
    format!(
        "{}: {value}",
        serde_json::to_string(key).unwrap_or_default()
    )
}

/// Replaces the value of a scalar.
pub fn replace_value(node: &Node, text: &str) -> Edit {
    let span = node.span();
    Edit {
        at: span.start,
        remove: span.end - span.start,
        insert: text.to_owned(),
    }
}

/// Whether `bytes[from..to]` is only blanks and commas: nothing the user wrote (a comment)
/// sits in a stretch that is about to be removed or measured.
fn only_blanks_and_commas(bytes: &[u8], from: usize, to: usize) -> bool {
    bytes[from..to]
        .iter()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b','))
}

/// `remove_child` for a document that may have comments: refused (`None`) when anything but
/// the child, a separator and blanks is in the stretch that would go, so a comment is never
/// taken with an entry it was not part of.
pub fn remove_child_checked(bytes: &[u8], container: &Node, index: usize) -> Option<Removal> {
    let removal = remove_child(bytes, container, index)?;
    let (at, end) = (removal.edit.at, removal.edit.at + removal.edit.remove);
    let child = *container.child_spans().get(index)?;
    // A comment inside the entry (between a key and its value, say) would go with it.
    if has_comment(bytes, child) {
        return None;
    }
    // The stretch is the child plus its separator and blanks: outside the child, only those.
    let clean = if end <= child.start || at >= child.end {
        only_blanks_and_commas(bytes, at, end)
    } else {
        only_blanks_and_commas(bytes, at, child.start.max(at))
            && only_blanks_and_commas(bytes, child.end.min(end), end)
    };
    clean.then_some(removal)
}

/// `append_child` for a document that may have comments or a trailing comma. A comment
/// anywhere an empty container's insides would be replaced is refused (`None`).
pub fn append_child_lenient(bytes: &[u8], container: &Node, text: &str) -> Option<Edit> {
    let children = container.child_spans();
    let span = container.span();
    let Some(last) = children.last() else {
        return only_blanks_and_commas(bytes, span.start + 1, span.end - 1)
            .then(|| append_child(bytes, container, text));
    };
    let base = indentation(bytes, span.start);
    let multiline = bytes[span.start..span.end].contains(&b'\n');
    let unit = if base.contains('\t') { "\t" } else { "  " };
    let eol = if bytes[span.start..span.end].windows(2).any(|w| w == b"\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let lead = if multiline {
        let own = indentation(bytes, last.start);
        format!(
            "{eol}{}",
            if own.is_empty() {
                format!("{base}{unit}")
            } else {
                own
            }
        )
    } else {
        " ".to_owned()
    };
    // A comma already follows the last child: the new one goes after it, and keeps the style.
    let mut after = last.end;
    while bytes.get(after).is_some_and(u8::is_ascii_whitespace) {
        after += 1;
    }
    if bytes.get(after) == Some(&b',') {
        Some(Edit {
            at: after + 1,
            remove: 0,
            insert: format!("{lead}{text},"),
        })
    } else {
        Some(Edit {
            at: last.end,
            remove: 0,
            insert: format!(",{lead}{text}"),
        })
    }
}

/// Whether a `//` or `/*` opens anywhere in `span` outside a string.
pub fn has_comment(bytes: &[u8], span: Span) -> bool {
    let (mut i, end) = (span.start, span.end);
    while i < end {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < end && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if matches!(bytes.get(i + 1), Some(b'/' | b'*')) => return true,
            _ => i += 1,
        }
    }
    false
}
