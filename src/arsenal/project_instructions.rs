//! The bytes reviewed for a single launch, and the persistent selection (ids and hashes only).
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::role_instructions::{Choice, MAX_ROLE};
use crate::native::store;

const FILE: &str = "project-instructions.json";
const MAX_STATE: u64 = 1024 * 1024;
const MAX_CHOICES: usize = 1024;
const MAX_SECTIONS: usize = 256;
const MAX_INPUT: usize = 512 * 1024;
const MAX_FILE: usize = 128 * 1024;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Section {
    pub id: String,
    pub file: String,
    #[serde(skip)]
    pub heading_path: Vec<String>,
    pub text: String,
    #[serde(skip)]
    pub hash: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Omission {
    UnsafeImport,
    Unreadable,
    TooLarge,
    Limit,
}

#[derive(Debug)]
pub struct Source {
    pub file: String,
    pub sections: Vec<Section>,
    pub omitted: Vec<(String, Omission)>,
}

#[derive(Debug)]
pub struct Snapshot {
    root: PathBuf,
    pub sources: Vec<Source>,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Snapshot {
    /// Read only the two root instruction files and explicit, contained Claude imports.
    pub fn read(directory: &Path) -> io::Result<Self> {
        let root = directory.canonicalize()?;
        let mut budget = MAX_INPUT;
        let mut remaining = MAX_SECTIONS;
        let mut sources = Vec::new();
        for file in ["CLAUDE.md", "AGENTS.md"] {
            if fs::symlink_metadata(root.join(file))
                .is_err_and(|e| e.kind() == io::ErrorKind::NotFound)
            {
                continue;
            }
            let mut source = Source {
                file: file.to_owned(),
                sections: Vec::new(),
                omitted: Vec::new(),
            };
            match read_file(&root, Path::new(file), &mut budget) {
                Ok(text) => source.split(&root, &text, &mut remaining, &mut budget),
                Err(reason) => source.omitted.push((file.to_owned(), reason)),
            }
            sources.push(source);
        }
        Ok(Self { root, sources })
    }

    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.sources.iter().flat_map(|source| &source.sections)
    }

    pub fn remembered(&self, directory: &Path, preset: &str) -> io::Result<Remembered> {
        Ok(Remembered(
            read(directory)?
                .choices
                .remove(&self.key(preset)?)
                .unwrap_or_default(),
        ))
    }

    pub fn remember(
        &self,
        directory: &Path,
        preset: &str,
        selected: &BTreeSet<String>,
    ) -> io::Result<()> {
        let key = self.key(preset)?;
        let _lock =
            crate::agentconfig::vault::exclusive(&directory.join("project-instructions.lock"))?;
        let mut state = read(directory)?;
        state.choices.insert(
            key.clone(),
            self.sections()
                .map(|section| {
                    (
                        section.id.clone(),
                        SavedSection {
                            hash: section.hash.clone(),
                            selected: selected.contains(&section.id),
                        },
                    )
                })
                .collect(),
        );
        // ponytail: evicts by key order, an access time if losing a live choice ever matters.
        // A lost choice only means its sections start unchecked; a full store never blocks a launch.
        let bytes = loop {
            let bytes = serde_json::to_vec(&state).map_err(io::Error::other)?;
            if state.choices.len() <= MAX_CHOICES && bytes.len() as u64 <= MAX_STATE {
                break bytes;
            }
            let Some(oldest) = state.choices.keys().find(|k| **k != key).cloned() else {
                return Err(io::Error::other("project instructions store full"));
            };
            state.choices.remove(&oldest);
        };
        store::write_atomic(&directory.join(FILE), &bytes)
    }

    fn key(&self, preset: &str) -> io::Result<String> {
        if !super::role_instructions::id_ok(preset) {
            return Err(io::Error::other("invalid preset"));
        }
        Ok(format!(
            "{preset}:{}",
            hash(self.root.as_os_str().as_bytes())
        ))
    }

    /// This exact string is both the preview and the role payload.
    pub fn compose(&self, base: Option<&Choice>, selected: &BTreeSet<String>) -> Composition {
        let mut text = match base {
            Some(Choice::Role { role }) => role.clone(),
            _ => String::new(),
        };
        for section in self
            .sections()
            .filter(|section| selected.contains(&section.id))
        {
            if section.text.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push_str("\n\n");
            }
            text.push_str(&section.text);
        }
        Composition { text }
    }
}

pub struct Composition {
    pub text: String,
}

impl Composition {
    pub fn excess(&self) -> usize {
        self.text.len().saturating_sub(MAX_ROLE)
    }

    pub fn into_choice(self) -> Result<Choice, usize> {
        let excess = self.excess();
        if excess > 0 {
            return Err(excess);
        }
        Ok(if self.text.is_empty() {
            Choice::Exclude { role: self.text }
        } else {
            Choice::Role { role: self.text }
        })
    }
}

fn read_file(root: &Path, relative: &Path, budget: &mut usize) -> Result<String, Omission> {
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| Omission::Unreadable)?;
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Omission::UnsafeImport)?;
    if relative.as_os_str().is_empty() {
        return Err(Omission::Unreadable);
    }
    // Open under the root again, without following a symlink inserted after canonicalization.
    let file = crate::project::open_relative(root, relative).map_err(|_| Omission::Unreadable)?;
    let max = MAX_FILE.min(*budget);
    let mut bytes = Vec::new();
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Omission::Unreadable)?;
    if bytes.len() > max {
        return Err(Omission::TooLarge);
    }
    *budget -= bytes.len();
    String::from_utf8(bytes)
        .ok()
        .filter(|text| !text.contains('\0'))
        .ok_or(Omission::Unreadable)
}

#[derive(Clone, Copy)]
struct Line<'a> {
    text: &'a str,
    fenced: bool,
}

fn fence(line: &str) -> Option<(u8, usize, &str)> {
    let line = markdown_line(line)?;
    let marker = *line.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let n = line.bytes().take_while(|byte| *byte == marker).count();
    (n >= 3).then(|| (marker, n, &line[n..]))
}

fn markdown_line(line: &str) -> Option<&str> {
    let line = line.trim_end_matches(['\n', '\r']);
    let stripped = line.trim_start_matches(' ');
    (line.len() - stripped.len() <= 3).then_some(stripped)
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut open = None;
    text.split_inclusive('\n')
        .map(|text| {
            let fenced = open.is_some()
                || fence(text)
                    .is_some_and(|(marker, _, tail)| marker == b'~' || !tail.contains('`'));
            if let Some((marker, n, tail)) = fence(text) {
                match open {
                    Some((old, count)) if marker == old && n >= count && tail.trim().is_empty() => {
                        open = None;
                    }
                    None if fenced => open = Some((marker, n)),
                    _ => {}
                }
            }
            Line { text, fenced }
        })
        .collect()
}

fn atx(line: Line<'_>) -> Option<(usize, &str)> {
    let line = (!line.fenced).then(|| markdown_line(line.text)).flatten()?;
    let n = line.bytes().take_while(|byte| *byte == b'#').count();
    let rest = line[n..].trim_end();
    if !(1..=6).contains(&n) || (!rest.is_empty() && !rest.starts_with([' ', '\t'])) {
        return None;
    }
    let stripped = rest.trim_end_matches('#');
    let title = if stripped.ends_with([' ', '\t']) {
        stripped
    } else {
        rest
    };
    Some((n, title.trim()))
}

fn setext(line: Line<'_>) -> Option<usize> {
    let line = (!line.fenced)
        .then(|| markdown_line(line.text))
        .flatten()?
        .trim_end();
    let marker = *line.as_bytes().first()?;
    match marker {
        b'=' | b'-' if line.bytes().all(|b| b == marker) => {
            Some(if marker == b'=' { 1 } else { 2 })
        }
        _ => None,
    }
}

struct Chunk<'a> {
    path: Vec<(String, usize)>,
    lines: &'a [Line<'a>],
}

fn paragraph_line(line: Line<'_>) -> bool {
    let Some(text) = markdown_line(line.text)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return false;
    };
    !line.fenced
        && !text.starts_with(['>', '<'])
        && !["- ", "+ ", "* "]
            .iter()
            .any(|prefix| text.starts_with(prefix))
        && !['*', '-', '_'].iter().any(|marker| {
            text.chars()
                .filter(|c| !c.is_whitespace())
                .all(|c| c == *marker)
                && text.chars().filter(|c| c == marker).count() >= 3
        })
}

fn chunks<'a>(lines: &'a [Line<'a>]) -> Vec<Chunk<'a>> {
    let mut chunks = Vec::new();
    let mut stack = Vec::<(usize, String, usize)>::new();
    let mut occurrences = BTreeMap::<(Vec<(String, usize)>, String), usize>::new();
    let path = |stack: &[(usize, String, usize)]| {
        stack
            .iter()
            .map(|(_, title, count)| (title.clone(), *count))
            .collect::<Vec<_>>()
    };
    let mut start = 0;
    let mut paragraph = None;
    for (i, &line) in lines.iter().enumerate() {
        let heading = if let Some((level, title)) = atx(line) {
            Some((i, level, title.to_owned()))
        } else if let (Some(level), Some(first)) = (setext(line), paragraph) {
            let title = lines[first..i]
                .iter()
                .map(|line| line.text.trim())
                .collect::<Vec<_>>()
                .join(" ");
            Some((first, level, title))
        } else {
            None
        };
        if let Some((heading_start, level, title)) = heading {
            if heading_start > start {
                chunks.push(Chunk {
                    path: path(&stack),
                    lines: &lines[start..heading_start],
                });
                if chunks.len() > MAX_SECTIONS {
                    return chunks;
                }
            }
            while stack.last().is_some_and(|(old, _, _)| *old >= level) {
                stack.pop();
            }
            let occurrence = occurrences
                .entry((path(&stack), title.clone()))
                .or_default();
            *occurrence += 1;
            stack.push((level, title, *occurrence));
            start = heading_start;
            paragraph = None;
        } else if paragraph_line(line) {
            paragraph.get_or_insert(i);
        } else {
            paragraph = None;
        }
    }
    if start < lines.len() {
        chunks.push(Chunk {
            path: path(&stack),
            lines: &lines[start..],
        });
    }
    chunks
}

/// An @path token in prose. Code spans and escaped markers are literal Markdown.
fn imports<'a>(line: &'a str, ticks: &mut usize) -> Vec<(std::ops::Range<usize>, &'a str)> {
    let mut found = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let n = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            if *ticks == 0 {
                *ticks = n;
            } else if *ticks == n {
                *ticks = 0;
            }
            i += n;
        } else if bytes[i] == b'\\' {
            i += 2;
        } else if *ticks == 0
            && bytes[i] == b'@'
            && (i == 0 || bytes[i - 1].is_ascii_whitespace() || b"([{".contains(&bytes[i - 1]))
        {
            let start = i;
            i += 1;
            let end = bytes[i..]
                .iter()
                .position(|byte| byte.is_ascii_whitespace() || b"`<>()[]{};,!\"".contains(byte))
                .map_or(bytes.len(), |n| i + n);
            if end > i {
                found.push((start..end, &line[i..end]));
                i = end;
            }
        } else {
            i += 1;
        }
    }
    found
}

impl Source {
    fn split(&mut self, root: &Path, text: &str, remaining: &mut usize, budget: &mut usize) {
        for chunk in chunks(&lines(text)) {
            if *remaining == 0 {
                self.omitted.push((self.file.clone(), Omission::Limit));
                break;
            }
            *remaining -= 1;
            let heading_path: Vec<String> =
                chunk.path.iter().map(|(title, _)| title.clone()).collect();
            let id =
                hash(&serde_json::to_vec(&(&self.file, &chunk.path)).expect("section identity"));
            let mut body = String::new();
            let mut imports_for_section = Vec::new();
            let mut ticks = 0;
            let mut import_limit = false;
            for line in chunk.lines {
                if self.file != "CLAUDE.md" || line.fenced {
                    ticks = 0;
                    body.push_str(line.text);
                    continue;
                }
                let mut last = 0;
                let mut literal = String::new();
                let mut had_imports = false;
                for (range, file) in imports(line.text, &mut ticks) {
                    literal.push_str(&line.text[last..range.start]);
                    last = range.end;
                    had_imports = true;
                    if *remaining == 0 {
                        import_limit = true;
                        continue;
                    }
                    *remaining -= 1;
                    let read = if file.starts_with('~') {
                        Err(Omission::UnsafeImport)
                    } else {
                        read_file(root, Path::new(file), budget)
                    };
                    match read {
                        Ok(text) => {
                            let import_id = hash(
                                &serde_json::to_vec(&(
                                    &id,
                                    file,
                                    imports_for_section
                                        .iter()
                                        .filter(|s: &&Section| s.file == file)
                                        .count(),
                                ))
                                .expect("import identity"),
                            );
                            imports_for_section.push(Section {
                                id: import_id,
                                file: file.to_owned(),
                                heading_path: heading_path.clone(),
                                hash: hash(text.as_bytes()),
                                text,
                            });
                        }
                        Err(reason) => self.omitted.push((file.to_owned(), reason)),
                    }
                }
                literal.push_str(&line.text[last..]);
                if !had_imports || !literal.trim().is_empty() {
                    body.push_str(&literal);
                }
            }
            if !body.is_empty() {
                self.sections.push(Section {
                    id,
                    file: self.file.clone(),
                    heading_path,
                    hash: hash(body.as_bytes()),
                    text: body,
                });
            }
            self.sections.extend(imports_for_section);
            if import_limit {
                self.omitted.push((self.file.clone(), Omission::Limit));
                break;
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSection {
    hash: String,
    selected: bool,
}

pub struct Remembered(BTreeMap<String, SavedSection>);

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    New,
    Changed,
    Unchanged,
}

impl Remembered {
    pub fn status(&self, section: &Section) -> (bool, Change) {
        self.0
            .get(&section.id)
            .map_or((false, Change::New), |saved| {
                if saved.hash == section.hash {
                    (saved.selected, Change::Unchanged)
                } else {
                    // Text the owner has not seen is never carried in by an earlier approval.
                    (false, Change::Changed)
                }
            })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    choices: BTreeMap<String, BTreeMap<String, SavedSection>>,
}

fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn read(directory: &Path) -> io::Result<State> {
    let path = directory.join(FILE);
    let bad = || io::Error::other("project instructions store unreadable");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(State {
                schema_version: 1,
                choices: BTreeMap::new(),
            });
        }
        Ok(meta) if meta.is_file() => {}
        _ => return Err(bad()),
    }
    let bytes = store::read_bounded(&path, MAX_STATE + 1)
        .filter(|b| b.len() as u64 <= MAX_STATE)
        .ok_or_else(bad)?;
    let state: State = serde_json::from_slice(&bytes).map_err(|_| bad())?;
    if state.schema_version != 1
        || state.choices.len() > MAX_CHOICES
        || state.choices.iter().any(|(key, sections)| {
            !key.split_once(':').is_some_and(|(preset, project)| {
                super::role_instructions::id_ok(preset) && hex(project)
            }) || sections.len() > MAX_SECTIONS
                || sections
                    .iter()
                    .any(|(id, saved)| !hex(id) || !hex(&saved.hash))
        })
    {
        return Err(bad());
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("instruction-review-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("project")).unwrap();
            Self(root)
        }
        fn project(&self) -> PathBuf {
            self.0.join("project")
        }
        fn write(&self, name: &str, text: &str) {
            fs::write(self.project().join(name), text).unwrap();
        }
        fn snapshot(&self) -> Snapshot {
            Snapshot::read(&self.project()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn headings_keep_preamble_fences_and_duplicate_paths() {
        let f = Fixture::new();
        f.write("AGENTS.md", "Preamble\n\n# A\n\n````md\n# Hidden\n```\n## Also hidden\n````\n\nRepeat\n------\nfirst\n## Repeat ###\n~~~txt\n# Hidden too\n~~~~\nsecond\n");
        let snapshot = f.snapshot();
        let sections: Vec<_> = snapshot.sections().collect();
        assert_eq!(sections.len(), 4);
        assert!(sections[0].heading_path.is_empty());
        assert_eq!(sections[0].text, "Preamble\n\n");
        assert_eq!(sections[1].heading_path, ["A"]);
        assert!(sections[1].text.contains("## Also hidden"));
        assert_eq!(sections[2].heading_path, ["A", "Repeat"]);
        assert_eq!(sections[3].heading_path, ["A", "Repeat"]);
        assert_ne!(sections[2].id, sections[3].id);
        assert!(sections[3].text.contains("# Hidden too"));
    }

    #[test]
    fn multiline_setext_heading_keeps_its_original_markdown() {
        let f = Fixture::new();
        f.write(
            "AGENTS.md",
            "Intro\n\nFirst line\nsecond line\n====\nInstructions\n",
        );
        let snapshot = f.snapshot();
        let sections: Vec<_> = snapshot.sections().collect();
        assert_eq!(sections[0].text, "Intro\n\n");
        assert_eq!(sections[1].heading_path, ["First line second line"]);
        assert_eq!(
            sections[1].text,
            "First line\nsecond line\n====\nInstructions\n"
        );
    }

    #[test]
    fn claude_imports_stay_inside_the_project_and_code_is_literal() {
        let f = Fixture::new();
        f.write("CLAUDE.md", "# Main\nRead @inside.md for details.\n@alias.md\n@outside.md\n@../secret.md\n@missing.md\n@folder\n`@code.md`\n~~~\n@fenced.md\n~~~\n");
        f.write("inside.md", "inside bytes\n");
        fs::write(f.0.join("secret.md"), "private bytes").unwrap();
        symlink("inside.md", f.project().join("alias.md")).unwrap();
        symlink(f.0.join("secret.md"), f.project().join("outside.md")).unwrap();
        fs::create_dir(f.project().join("folder")).unwrap();
        let snapshot = f.snapshot();
        let source = &snapshot.sources[0];
        assert_eq!(
            source
                .sections
                .iter()
                .map(|s| s.file.as_str())
                .collect::<Vec<_>>(),
            ["CLAUDE.md", "inside.md", "alias.md"]
        );
        assert!(source.sections[0].text.contains("`@code.md`"));
        assert!(source.sections[0].text.contains("@fenced.md"));
        assert!(!source.sections[0].text.contains("@inside.md"));
        assert_eq!(source.sections[1].text, "inside bytes\n");
        assert_eq!(
            source.omitted,
            [
                ("outside.md".into(), Omission::UnsafeImport),
                ("../secret.md".into(), Omission::UnsafeImport),
                ("missing.md".into(), Omission::Unreadable),
                ("folder".into(), Omission::Unreadable),
            ]
        );
    }

    #[test]
    fn oversight_of_large_or_unreadable_sources_is_visible() {
        let f = Fixture::new();
        assert!(f.snapshot().sources.is_empty());
        f.write("CLAUDE.md", &"x".repeat(MAX_FILE + 1));
        f.write("AGENTS.md", "bad\0data");
        let snapshot = f.snapshot();
        assert_eq!(snapshot.sections().count(), 0);
        assert_eq!(snapshot.sources[0].omitted[0].1, Omission::TooLarge);
        assert_eq!(snapshot.sources[1].omitted[0].1, Omission::Unreadable);
    }

    #[test]
    fn a_full_store_evicts_another_choice_instead_of_blocking_the_launch() {
        let f = Fixture::new();
        f.write("AGENTS.md", "# One\ntext\n");
        let snapshot = f.snapshot();
        let ids: BTreeSet<_> = snapshot.sections().map(|s| s.id.clone()).collect();
        let saved = || snapshot.sections().map(|s| (s.id.clone(), SavedSection { hash: s.hash.clone(), selected: true })).collect::<BTreeMap<_, _>>();
        let choices = (0..MAX_CHOICES).map(|n| (format!("old{n}:{}", "0".repeat(64)), saved())).collect();
        let state = State { schema_version: 1, choices };
        store::write_atomic(&f.0.join(FILE), &serde_json::to_vec(&state).unwrap()).unwrap();
        snapshot.remember(&f.0, "p1", &ids).unwrap();
        let section = snapshot.sections().next().unwrap();
        assert_eq!(snapshot.remembered(&f.0, "p1").unwrap().status(section), (true, Change::Unchanged));
        assert!(read(&f.0).unwrap().choices.len() <= MAX_CHOICES);
    }

    #[test]
    fn remembers_choice_per_preset_and_project_and_marks_changed_bytes() {
        let f = Fixture::new();
        f.write(
            "AGENTS.md",
            "# Same\nunchanged\n# Edited\nsecret project text\n",
        );
        let first = f.snapshot();
        let ids: BTreeSet<_> = first.sections().map(|s| s.id.clone()).collect();
        assert!(first.sections().all(
            |section| first.remembered(&f.0, "p1").unwrap().status(section) == (false, Change::New)
        ));
        first.remember(&f.0, "p1", &ids).unwrap();
        let stored = fs::read_to_string(f.0.join(FILE)).unwrap();
        assert!(!stored.contains("secret project text"));
        assert!(!stored.contains("AGENTS.md"));
        f.write(
            "AGENTS.md",
            "# Same\nunchanged\n# Edited\nchanged\n# New\nmore\n",
        );
        let next = f.snapshot();
        let remembered = next.remembered(&f.0, "p1").unwrap();
        assert_eq!(
            next.sections()
                .map(|s| remembered.status(s))
                .collect::<Vec<_>>(),
            [
                (true, Change::Unchanged),
                (false, Change::Changed),
                (false, Change::New)
            ]
        );
        assert_eq!(
            next.remembered(&f.0, "p2")
                .unwrap()
                .status(next.sections().next().unwrap()),
            (false, Change::New)
        );
        let second_project = f.0.join("other-project");
        fs::create_dir(&second_project).unwrap();
        fs::copy(
            f.project().join("AGENTS.md"),
            second_project.join("AGENTS.md"),
        )
        .unwrap();
        let other = Snapshot::read(&second_project).unwrap();
        assert_eq!(
            other
                .remembered(&f.0, "p1")
                .unwrap()
                .status(other.sections().next().unwrap()),
            (false, Change::New)
        );
        fs::write(
            f.0.join(FILE),
            r#"{"schema_version":1,"choices":{},"surprise":true}"#,
        )
        .unwrap();
        assert!(next.remembered(&f.0, "p1").is_err());
        assert!(next.remember(&f.0, "p1", &ids).is_err());
    }

    #[test]
    fn composition_is_exact_and_counts_utf8_bytes_and_separator() {
        let f = Fixture::new();
        f.write("CLAUDE.md", "# Á\nél\n");
        f.write("AGENTS.md", "# B\r\ntext\r\n");
        let snapshot = f.snapshot();
        let all = snapshot.sections().map(|s| s.id.clone()).collect();
        let role = Choice::Role {
            role: "Base".into(),
        };
        let composed = snapshot.compose(Some(&role), &all);
        assert_eq!(composed.text, "Base\n\n# Á\nél\n\n\n# B\r\ntext\r\n");
        assert_eq!(
            composed.into_choice(),
            Ok(Choice::Role {
                role: "Base\n\n# Á\nél\n\n\n# B\r\ntext\r\n".into()
            })
        );
        assert_eq!(
            snapshot
                .compose(
                    Some(&Choice::Exclude {
                        role: "draft".into()
                    }),
                    &BTreeSet::new()
                )
                .into_choice(),
            Ok(Choice::Exclude {
                role: String::new()
            })
        );
        let max = Choice::Role {
            role: "x".repeat(MAX_ROLE),
        };
        assert_eq!(
            snapshot.compose(Some(&max), &BTreeSet::new()).into_choice(),
            Ok(max.clone())
        );
        assert_eq!(snapshot.compose(Some(&max), &all).into_choice(), Err(24));
    }
}
