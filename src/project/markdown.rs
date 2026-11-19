use super::{markdown_blocks, short};

#[derive(Debug, PartialEq, Eq)]
pub enum Inline {
    Text(String),
    Code(String),
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

fn inline(mut s: &str) -> Vec<Inline> {
    let mut result = Vec::new();
    let mut start = 0;
    while let Some(at) = s[start..].find(['`', '[', '<', '!']).map(|at| at + start) {
        let rest = &s[at..];
        let span = if let Some(code) = rest.strip_prefix('`') {
            code.split_once('`')
                .map(|(code, tail)| (Inline::Code(code.into()), tail))
        } else if let Some(html) = rest.strip_prefix('<') {
            // Leave the whole HTML token literal, including link syntax in attributes.
            html.split_once('>')
                .map(|(_, tail)| (Inline::Text(rest[..rest.len() - tail.len()].into()), tail))
        } else {
            let image = rest.starts_with("![");
            rest.strip_prefix("![")
                .or_else(|| rest.strip_prefix('['))
                .and_then(|link| {
                    let end = link.find([']', '[', '\n'])?;
                    let destination = link[end..].strip_prefix("](")?;
                    let close = destination.find([')', '[', '\n'])?;
                    Some((
                        &link[..end],
                        &destination[..close],
                        destination[close..].strip_prefix(')')?,
                    ))
                })
                .map(|(text, url, tail)| {
                    let item = if !image && safe_http(url) {
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
    use markdown_blocks::Block as Base;
    let mut output = Vec::new();
    let mut pending = String::new();
    let mut fence = None;
    let mut lines = text.lines().peekable();
    let flush = |pending: &mut String, output: &mut Vec<Block>| {
        output.extend(
            markdown_blocks::blocks(pending)
                .into_iter()
                .map(|block| match block {
                    Base::Heading(_, text) => Block::Heading(inline(&text)),
                    Base::Paragraph(text) => Block::Paragraph(inline(&text)),
                    Base::List(items) => {
                        Block::List(items.iter().map(|item| inline(item)).collect())
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
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
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
        pending.push_str(line);
        pending.push('\n');
    }
    flush(&mut pending, &mut output);
    output
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
        .map(|cell| inline(cell.trim()))
        .collect()
}

pub(super) fn plain_line(line: &str) -> String {
    let line = line
        .trim()
        .trim_start_matches(['#', '>', '-', '+', '*', ' ']);
    let plain: String = inline(line)
        .into_iter()
        .map(|part| match part {
            Inline::Code(code) => code,
            Inline::Link { text, .. } => text,
            Inline::Text(text) => text.replace(['*', '_', '~'], ""),
        })
        .collect();
    short(&plain)
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
