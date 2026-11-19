use super::{markdown_blocks, short};

#[derive(Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
    Strong(String),
    Link { text: String, url: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Heading(Vec<Inline>),
    Paragraph(Vec<Inline>),
    List(Vec<Vec<Inline>>),
    Code(String),
    Table(Vec<Vec<Vec<Inline>>>),
}

fn inline(mut s: &str, summary: bool) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(at) = s[start..].find(['`', '[', '<', '!', '*']).map(|at| at + start) {
        let rest = &s[at..];
        let span = if summary && let Some(tail) = image_tail(rest) {
            Some((Inline::Text(String::new()), tail))
        } else if let Some(strong) = rest.strip_prefix("**") {
            strong
                .split_once("**")
                .filter(|(text, _)| !text.is_empty() && !text.contains('\n'))
                .map(|(text, tail)| {
                    let item = if summary {
                        Inline::Text(text.into())
                    } else {
                        Inline::Strong(text.into())
                    };
                    (item, tail)
                })
        } else if let Some(code) = rest.strip_prefix('`') {
            code.split_once('`')
                .map(|(code, tail)| (Inline::Code(code.into()), tail))
        } else if let Some(html) = rest.strip_prefix('<') {
            // Leave the whole HTML token literal, including link syntax in attributes.
            html.split_once('>').map(|(_, tail)| {
                let literal = if summary {
                    ""
                } else {
                    &rest[..rest.len() - tail.len()]
                };
                (Inline::Text(literal.into()), tail)
            })
        } else {
            let image = rest.starts_with("![");
            rest.strip_prefix("![")
                .or_else(|| rest.strip_prefix('['))
                .and_then(|link| {
                    let end = link.find([']', '[', '\n'])?;
                    let (url, tail) = link_destination(&link[end..], summary)?;
                    Some((&link[..end], url, tail))
                })
                .map(|(text, url, tail)| {
                    let item = if summary {
                        Inline::Text(text.into())
                    } else if !image && safe_http(url) {
                        Inline::Link {
                            text: text.into(),
                            url: url.into(),
                        }
                    } else {
                        Inline::Text(rest[..rest.len() - tail.len()].into())
                    };
                    (item, tail)
                })
        };
        if let Some((part, tail)) = span {
            if at > 0 {
                result.push(Inline::Text(s[..at].into()));
            }
            result.push(part);
            s = tail;
            start = 0;
        } else {
            start = at + 1;
        }
    }
    if !s.is_empty() {
        result.push(Inline::Text(s.into()));
    }
    result
}

fn link_destination(rest: &str, references: bool) -> Option<(&str, &str)> {
    if let Some(destination) = rest.strip_prefix("](") {
        let close = destination.find([')', '[', '\n'])?;
        return (destination.as_bytes()[close] == b')')
            .then(|| (&destination[..close], &destination[close + 1..]));
    }
    if references {
        return rest.strip_prefix("][")?.split_once(']');
    }
    None
}

fn image_tail(rest: &str) -> Option<&str> {
    let (image, linked) = match rest.strip_prefix('[') {
        Some(image) => (image, true),
        None => (rest, false),
    };
    let image = image.strip_prefix("![")?;
    let (_, tail) = link_destination(&image[image.find(']')?..], true)?;
    if linked {
        return link_destination(tail, true).map(|(_, tail)| tail);
    }
    Some(tail)
}

fn safe_http(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    !host.is_empty()
        && !host.contains('@')
        && !url.contains('\\')
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn markdown(text: &str) -> Vec<Block> {
    parse(text, false)
}

fn parse(text: &str, summary: bool) -> Vec<Block> {
    use markdown_blocks::Block as Base;
    let mut output = Vec::new();
    let mut pending = String::new();
    let mut fence = None;
    let mut comment = false;
    let mut lines = text.lines().peekable();
    let flush = |pending: &mut String, output: &mut Vec<Block>| {
        output.extend(
            markdown_blocks::blocks(pending)
                .into_iter()
                .map(|block| match block {
                    Base::Heading(_, text) => Block::Heading(inline(&text, summary)),
                    Base::Paragraph(text) => Block::Paragraph(inline(&text, summary)),
                    Base::List(items) => {
                        Block::List(items.iter().map(|item| inline(item, summary)).collect())
                    }
                    Base::Code(code) => Block::Code(code),
                }),
        );
        pending.clear();
    };
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if let Some(mark) = fence {
            if trimmed.starts_with(mark) {
                fence = None;
            }
        } else if summary && (comment || trimmed.starts_with("<!--")) {
            comment = !trimmed.contains("-->");
            pending.push('\n');
            continue;
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
        } else if summary && skip_readme_line(line) {
            pending.push('\n');
            continue;
        } else if summary && !trimmed.is_empty() && lines.peek().is_some_and(|l| setext(l)) {
            lines.next();
            pending.push('\n');
            continue;
        } else if line.contains('|') && lines.peek().is_some_and(|l| table_rule(l)) {
            flush(&mut pending, &mut output);
            lines.next();
            let mut rows = vec![cells(line)];
            while lines.peek().is_some_and(|l| {
                l.contains('|') && !l.trim().starts_with("```") && !l.trim().starts_with("~~~")
            }) {
                rows.push(cells(lines.next().expect("peeked row")));
            }
            output.push(Block::Table(rows));
            continue;
        }
        pending.push_str(if summary && fence.is_none() {
            trimmed
        } else {
            line
        });
        pending.push('\n');
    }
    flush(&mut pending, &mut output);
    output
}

fn setext(line: &str) -> bool {
    let line = line.trim();
    line.starts_with(['=', '-']) && line.bytes().all(|b| b == line.as_bytes()[0])
}

fn skip_readme_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with(['<', '>'])
        || line.starts_with("    ")
        || line.starts_with('\t')
        || setext(trimmed)
        || (trimmed.len() >= 3 && trimmed.bytes().all(|b| matches!(b, b'*' | b'_' | b' ')))
        || (trimmed.starts_with('[') && trimmed.contains("]:"))
}

fn table_rule(line: &str) -> bool {
    line.contains('|')
        && line.trim().trim_matches('|').split('|').all(|c| {
            let dash = c.trim().trim_matches(':');
            !dash.is_empty() && dash.bytes().all(|b| b == b'-')
        })
}

fn cells(line: &str) -> Vec<Vec<Inline>> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| inline(cell.trim(), false))
        .collect()
}

pub(super) fn readme_excerpt(text: &str) -> Option<String> {
    let paragraph = parse(markdown_blocks::body(text), true)
        .into_iter()
        .find_map(|block| {
            let Block::Paragraph(parts) = block else {
                return None;
            };
            let plain: String = parts
                .into_iter()
                .map(|part| match part {
                    Inline::Code(code) | Inline::Strong(code) => code,
                    Inline::Link { text, .. } | Inline::Text(text) => {
                        text.replace(['*', '_', '~'], "")
                    }
                })
                .collect();
            let plain = plain.split_whitespace().collect::<Vec<_>>().join(" ");
            (!plain.is_empty()).then_some(plain)
        })?;
    let cap = paragraph
        .char_indices()
        .nth(200)
        .map_or(paragraph.len(), |(i, _)| i);
    let paragraph = &paragraph[..cap];
    let punctuation = ['.', '!', '?', '…'];
    let end = paragraph
        .match_indices(punctuation)
        .find_map(|(index, mark)| {
            let tail = &paragraph[index + mark.len()..];
            let after =
                tail.trim_start_matches(['.', '!', '?', '…', '"', '\'', '”', '’', '»', ')']);
            (after.is_empty() || after.starts_with(char::is_whitespace))
                .then_some(paragraph.len() - after.len())
        })
        .unwrap_or(paragraph.len());
    Some(paragraph[..end].trim_end().into())
}

pub(super) fn commands(text: &str) -> Vec<String> {
    let mut section = None;
    let mut fence = None;
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if let Some((mark, include)) = fence {
            if line.starts_with(mark) {
                fence = None;
            } else if include && !line.is_empty() {
                out.push(short(line));
                if out.len() == 10 {
                    break;
                }
            }
        } else if line.starts_with("```") || line.starts_with("~~~") {
            fence = Some((
                &line[..3],
                section.is_some() && matches!(line[3..].trim(), "sh" | "bash" | "shell"),
            ));
        } else if line.starts_with('#') {
            let heading = line.trim_start_matches('#');
            let level = line.len() - heading.len();
            if section.is_some_and(|parent| level <= parent) {
                section = None;
            }
            let heading = heading.to_lowercase();
            if ["compil", "ejecut", "test", "build", "run", "prueba"]
                .iter()
                .any(|word| heading.contains(word))
            {
                section = Some(level);
            }
        }
    }
    out
}
