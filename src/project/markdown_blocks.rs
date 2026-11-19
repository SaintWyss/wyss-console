//! Markdown blocks and headings for Workspace guides. GTK labels display the content literally.

#[derive(Debug, PartialEq, Eq)]
pub enum Block {
    Heading(usize, String),
    Paragraph(String),
    List(Vec<String>),
    Code(String),
}

pub fn body(text: &str) -> &str {
    let mut lines = text.split_inclusive('\n');
    if lines.next().is_some_and(|line| line.trim_end() == "---") {
        let mut offset = text.find('\n').map_or(text.len(), |index| index + 1);
        for line in lines {
            offset += line.len();
            if line.trim_end() == "---" {
                return &text[offset..];
            }
        }
    }
    text
}

pub fn blocks(text: &str) -> Vec<Block> {
    let normalized = body(text).replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.split('\n');
    let mut paragraph = Vec::new();
    let mut items: Vec<String> = Vec::new();
    let mut out = Vec::new();
    fn flush(out: &mut Vec<Block>, paragraph: &mut Vec<&str>, items: &mut Vec<String>) {
        if !paragraph.is_empty() {
            out.push(Block::Paragraph(paragraph.join(" ")));
            paragraph.clear();
        }
        if !items.is_empty() {
            out.push(Block::List(std::mem::take(items)));
        }
    }
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            flush(&mut out, &mut paragraph, &mut items);
            let fence = &trimmed[..3];
            let code = lines
                .by_ref()
                .take_while(|line| !line.trim().starts_with(fence))
                .collect::<Vec<_>>();
            out.push(Block::Code(code.join("\n")));
            continue;
        }
        let rest = line.trim_start_matches('#');
        let level = line.len() - rest.len();
        if (1..=6).contains(&level) && rest.starts_with(char::is_whitespace) {
            flush(&mut out, &mut paragraph, &mut items);
            out.push(Block::Heading(level, rest.trim().into()));
            continue;
        }
        if let Some(item) = list_item(line.trim_start()) {
            if !paragraph.is_empty() {
                out.push(Block::Paragraph(paragraph.join(" ")));
                paragraph.clear();
            }
            items.push(item.trim().into());
        } else if trimmed.is_empty() {
            flush(&mut out, &mut paragraph, &mut items);
        } else if let Some(item) = items.last_mut()
            && line.starts_with(char::is_whitespace)
        {
            item.push(' ');
            item.push_str(trimmed);
        } else {
            flush(&mut out, &mut Vec::new(), &mut items);
            paragraph.push(trimmed);
        }
    }
    flush(&mut out, &mut paragraph, &mut items);
    out
}

fn list_item(line: &str) -> Option<&str> {
    let after = if let Some(after) = line.strip_prefix(['-', '*', '+']) {
        after
    } else {
        let rest = line.trim_start_matches(|ch: char| ch.is_ascii_digit());
        if line.len() == rest.len() {
            return None;
        }
        rest.strip_prefix(['.', ')'])?
    };
    after.starts_with(char::is_whitespace).then_some(after)
}

#[cfg(test)]
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Part {
    pub title: String,
    pub blocks: Vec<Block>,
    pub parts: Vec<Part>,
}

#[cfg(test)]
pub fn outline(blocks: Vec<Block>) -> Part {
    let mut counts = [0; 7];
    let mut first = None;
    for (index, block) in blocks.iter().enumerate() {
        if let Block::Heading(level, _) = block {
            counts[*level] += 1;
            first.get_or_insert((index, *level));
        }
    }
    let name = first.filter(|(_, level)| {
        counts[*level] == 1 && counts[..*level].iter().all(|count| *count == 0)
    });
    let mut levels =
        (1..=6).filter(|level| counts[*level] > 0 && name.is_none_or(|(_, own)| *level != own));
    let top = levels.next();
    let sub = levels.next();
    let mut document = Part::default();
    for (index, block) in blocks.into_iter().enumerate() {
        if name.is_some_and(|(own, _)| own == index) {
            continue;
        }
        if let Block::Heading(level, title) = &block {
            if Some(*level) == top {
                document.parts.push(Part {
                    title: title.clone(),
                    ..Part::default()
                });
                continue;
            }
            if Some(*level) == sub
                && let Some(part) = document.parts.last_mut()
            {
                part.parts.push(Part {
                    title: title.clone(),
                    ..Part::default()
                });
                continue;
            }
        }
        let target = if let Some(part) = document.parts.last_mut() {
            part.parts
                .last_mut()
                .map(|part| &mut part.blocks)
                .unwrap_or(&mut part.blocks)
        } else {
            &mut document.blocks
        };
        target.push(block);
    }
    document
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_arsenal_blocks_and_two_outline_levels() {
        let input = "---\r\ndescription: desc\r\n---\r\n# File\r\n\r\nIntro\r\ncontinued\r\n\r\n## Steps\r\n1. first\r\n  continued\r\n- second\r\n### Example\r\n~~~rust\r\n<literal>\r\n~~~";
        let document = outline(blocks(input));
        assert_eq!(
            document.blocks,
            [Block::Paragraph("Intro continued".into())]
        );
        assert_eq!(document.parts[0].title, "Steps");
        assert_eq!(
            document.parts[0].blocks,
            [Block::List(vec!["first continued".into(), "second".into()])]
        );
        assert_eq!(
            document.parts[0].parts[0].blocks,
            [Block::Code("<literal>".into())]
        );
    }
}
