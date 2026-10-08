//! The action under approval. It lives only in authority memory: it is bounded, normalized,
//! fingerprinted and measured here, and never reaches the store, the audit log or a log line.
use crate::contracts::control::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};

const DOMAIN: &[u8] = b"wyss.approval.action.v1";
/// Names the runtime's own sandbox protects inside every writable root. An action that
/// names one would plant project policy or repository hooks: it is never acceptable.
const PROTECTED: [&str; 3] = [".codex", ".git", ".agents"];

/// What the last component of a confined path has to be.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::control) enum Leaf {
    Directory,
    ExistingFile,
    FileOrMissing,
}
/// Proves that the normalized absolute `path` names something inside the canonical
/// `workspace`. The proof never compares strings as prefixes and never follows a link: the
/// path must extend the workspace component by component, every component that exists must
/// be a real directory on the workspace's own filesystem, and the leaf must be what the
/// action needs. A symbolic link anywhere, a hard-linked file, a mount point, a protected
/// name or anything that cannot be inspected makes the proof fail, and without the proof
/// an action is never accepted. The check is repeated immediately before every Accept; the
/// runtime's own sandbox remains the enforcement against a change after it.
pub(in crate::control) fn confine(workspace: &str, path: &str, leaf: Leaf) -> bool {
    let root = Path::new(workspace);
    let Ok(root_meta) = fs::symlink_metadata(root) else {
        return false;
    };
    // The workspace is still the canonical directory the session was bound to.
    if !root_meta.is_dir() || !fs::canonicalize(root).is_ok_and(|p| p == root) {
        return false;
    }
    let Ok(relative) = Path::new(path).strip_prefix(root) else {
        return false;
    };
    let mut names = Vec::new();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return false;
        };
        if PROTECTED.iter().any(|p| name == *p) {
            return false;
        }
        names.push(name);
    }
    if names.is_empty() {
        return leaf == Leaf::Directory;
    }
    let mut current = root.to_path_buf();
    for (index, name) in names.iter().enumerate() {
        current.push(name);
        let last = index + 1 == names.len();
        match fs::symlink_metadata(&current) {
            Ok(m) if m.file_type().is_symlink() || m.dev() != root_meta.dev() => return false,
            Ok(m) if !last => {
                if !m.is_dir() {
                    return false;
                }
            }
            Ok(m) => {
                return match leaf {
                    Leaf::Directory => m.is_dir(),
                    // A second name for the same file may live outside the workspace.
                    Leaf::ExistingFile | Leaf::FileOrMissing => m.is_file() && m.nlink() == 1,
                };
            }
            // Nothing exists below a missing component: creating it stays inside.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return leaf == Leaf::FileOrMissing;
            }
            Err(_) => return false,
        }
    }
    false
}

/// Lexical normal form of an absolute path: no empty or `.` component and no trailing
/// slash. A `..` component is refused rather than resolved, because resolving it lexically
/// is wrong as soon as a symlink is involved.
pub(in crate::control) fn normal_path(raw: &str, limit: usize) -> ControlResult<String> {
    if raw.len() > limit || !raw.starts_with('/') || raw.chars().any(char::is_control) {
        return Err(ControlError::InvalidRequest);
    }
    let mut out = String::with_capacity(raw.len());
    for part in raw.split('/').filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            return Err(ControlError::InvalidRequest);
        }
        out.push('/');
        out.push_str(part);
    }
    if out.is_empty() {
        out.push('/');
    }
    Ok(out)
}
/// Length-prefixed field: no two different field sequences share one encoding.
fn field(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}
fn kind_text(kind: FileChangeKind) -> &'static [u8] {
    match kind {
        FileChangeKind::Add => b"add",
        FileChangeKind::Delete => b"delete",
        FileChangeKind::Update => b"update",
        FileChangeKind::Move => b"move",
    }
}
// Deliberately no Debug on any action type.
pub(in crate::control) struct CommandAction {
    provider_item_id: String,
    command: String,
    cwd: String,
    parsed_actions: Vec<String>,
}
impl CommandAction {
    pub fn new(
        provider_item_id: &str,
        command: &str,
        cwd: &str,
        parsed_actions: Vec<String>,
    ) -> ControlResult<Self> {
        if !valid_provider_item(provider_item_id)
            || command.is_empty()
            || command.len() > MAX_COMMAND_BYTES
            || command.contains('\0')
            || parsed_actions.len() > MAX_PARSED_ACTIONS
            || parsed_actions
                .iter()
                .any(|a| a.len() > MAX_PARSED_ACTION_BYTES)
        {
            return Err(ControlError::InvalidRequest);
        }
        let action = Self {
            provider_item_id: provider_item_id.to_owned(),
            command: command.to_owned(),
            cwd: normal_path(cwd, MAX_CWD_BYTES)?,
            parsed_actions,
        };
        // The client is shown the whole command or nothing: never a truncated one.
        let encoded = serde_json::to_vec(&action.presentation())
            .map_err(|_| ControlError::InvalidRequest)?
            .len();
        if encoded > MAX_COMMAND_PRESENTATION_BYTES {
            return Err(ControlError::InvalidRequest);
        }
        Ok(action)
    }
    pub fn provider_item_id(&self) -> &str {
        &self.provider_item_id
    }
    #[cfg(test)]
    pub fn cwd(&self) -> &str {
        &self.cwd
    }
    /// SHA-256 over the kind, the provider item, the normalized cwd and the exact command.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        field(&mut h, DOMAIN);
        field(&mut h, b"command_execution");
        field(&mut h, self.provider_item_id.as_bytes());
        field(&mut h, self.cwd.as_bytes());
        field(&mut h, self.command.as_bytes());
        format!("{:x}", h.finalize())
    }
    pub fn structure(&self) -> ApprovalStructure {
        ApprovalStructure {
            command_bytes: self.command.len() as u32,
            cwd_bytes: self.cwd.len() as u32,
            parsed_actions: self.parsed_actions.len() as u16,
            files: 0,
            diff_bytes: 0,
        }
    }
    pub fn presentation(&self) -> CommandPresentation {
        CommandPresentation {
            command: self.command.clone(),
            cwd: self.cwd.clone(),
            parsed_actions: self.parsed_actions.clone(),
        }
    }
    /// The command runs in an existing directory of the workspace.
    pub fn confined(&self, workspace: &str) -> bool {
        confine(workspace, &self.cwd, Leaf::Directory)
    }
}
pub(in crate::control) struct FileChange {
    pub path: String,
    pub kind: FileChangeKind,
    pub move_path: Option<String>,
    pub diff: String,
}
pub(in crate::control) struct FileAction {
    provider_item_id: String,
    changes: Vec<FileChange>,
}
impl FileAction {
    /// The complete proposal: every path normalized, every destination distinct, the set
    /// ordered by path so that the provider's enumeration order carries no meaning.
    pub fn new(provider_item_id: &str, mut changes: Vec<FileChange>) -> ControlResult<Self> {
        if !valid_provider_item(provider_item_id) || changes.is_empty() || changes.len() > MAX_FILES
        {
            return Err(ControlError::InvalidRequest);
        }
        let mut total = 0usize;
        let mut touched = std::collections::BTreeSet::new();
        for c in &mut changes {
            c.path = normal_path(&c.path, MAX_PATH_BYTES)?;
            c.move_path = c
                .move_path
                .as_deref()
                .map(|p| normal_path(p, MAX_PATH_BYTES))
                .transpose()?;
            total += c.diff.len();
            if c.diff.len() > MAX_FILE_DIFF_BYTES
                || total > MAX_TOTAL_DIFF_BYTES
                || c.diff.contains('\0')
                || (c.kind == FileChangeKind::Move) != c.move_path.is_some()
                || c.move_path.as_ref() == Some(&c.path)
                || !touched.insert(c.path.clone())
                || c.move_path
                    .as_ref()
                    .is_some_and(|p| !touched.insert(p.clone()))
            {
                return Err(ControlError::InvalidRequest);
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            provider_item_id: provider_item_id.to_owned(),
            changes,
        })
    }
    pub fn provider_item_id(&self) -> &str {
        &self.provider_item_id
    }
    #[cfg(test)]
    pub fn changes(&self) -> &[FileChange] {
        &self.changes
    }
    pub fn files(&self) -> u16 {
        self.changes.len() as u16
    }
    /// One page of one proposed file: its paths, kind and a bounded part of its complete
    /// diff. Pages are contiguous and split only on character boundaries, so the client
    /// reads exactly the change set that was fingerprinted.
    pub fn page(&self, index: u16, offset: u32) -> ControlResult<FilePresentation> {
        let change = self
            .changes
            .get(usize::from(index))
            .ok_or(ControlError::InvalidRequest)?;
        let start = offset as usize;
        if start > change.diff.len() || !change.diff.is_char_boundary(start) {
            return Err(ControlError::InvalidRequest);
        }
        let mut end = (start + DIFF_PAGE_BYTES).min(change.diff.len());
        while !change.diff.is_char_boundary(end) {
            end -= 1;
        }
        Ok(FilePresentation {
            index,
            path: change.path.clone(),
            kind: change.kind,
            move_path: change.move_path.clone(),
            diff_bytes: change.diff.len() as u32,
            offset,
            chunk: change.diff[start..end].to_owned(),
            next_offset: (end < change.diff.len()).then_some(end as u32),
        })
    }
    /// SHA-256 over the kind, the provider item and the complete normalized change set:
    /// every path, change kind, move destination and diff.
    pub fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        field(&mut h, DOMAIN);
        field(&mut h, b"file_change");
        field(&mut h, self.provider_item_id.as_bytes());
        h.update((self.changes.len() as u64).to_be_bytes());
        for c in &self.changes {
            field(&mut h, c.path.as_bytes());
            field(&mut h, kind_text(c.kind));
            match &c.move_path {
                Some(p) => {
                    h.update([1]);
                    field(&mut h, p.as_bytes());
                }
                None => h.update([0]),
            }
            field(&mut h, c.diff.as_bytes());
        }
        format!("{:x}", h.finalize())
    }
    pub fn structure(&self) -> ApprovalStructure {
        ApprovalStructure {
            command_bytes: 0,
            cwd_bytes: 0,
            parsed_actions: 0,
            files: self.changes.len() as u16,
            diff_bytes: self.changes.iter().map(|c| c.diff.len()).sum::<usize>() as u32,
        }
    }
    /// Every source and destination of the change set stays inside the workspace.
    pub fn confined(&self, workspace: &str) -> bool {
        self.changes.iter().all(|c| {
            let source = match c.kind {
                FileChangeKind::Add => Leaf::FileOrMissing,
                FileChangeKind::Delete | FileChangeKind::Update | FileChangeKind::Move => {
                    Leaf::ExistingFile
                }
            };
            confine(workspace, &c.path, source)
                && c.move_path
                    .as_deref()
                    .is_none_or(|to| confine(workspace, to, Leaf::FileOrMissing))
        })
    }
}
#[cfg(test)]
#[path = "approval_action_tests.rs"]
mod tests;
