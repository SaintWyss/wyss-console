//! The approved roots: the closed list of places an adapter may read from (CTL-02, C-10).
//!
//! An adapter derives its roots from the user's home, the provider's documented relocation
//! variable, the provider's system directory and the project the user chose (07 §4.1), and
//! hands them to the reader. The reader refuses any path, and any place a link leads to,
//! that is not inside one of them, before it touches the file system: a hostile link
//! cannot make the reader walk, stat or open anything outside the list.
//!
//! Containment is by whole names (`/home/u/.claude-x` is not inside `/home/u/.claude`).
//! It is lexical, and that is sound because the reader refuses every link on the way:
//! the name it judges is the place it opens.
use super::place::names;
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Directories that hold credentials whatever the provider: nothing under one of them is
/// read, though the root that holds it is approved.
const CREDENTIAL_DIRECTORIES: [&str; 5] = [".ssh", ".gnupg", ".aws", "secrets", ".secrets"];

const MOST_ROOTS: usize = 64;

/// A root that is not a plain absolute path, is the whole file system, or is one of too
/// many.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRoot;

impl fmt::Display for InvalidRoot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("root_invalid")
    }
}

impl std::error::Error for InvalidRoot {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedRoots(Vec<PathBuf>);

impl ApprovedRoots {
    /// The roots of one adapter. Each is an absolute path with no `..`, no control
    /// character, and at least one name: `/` is never a root.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Result<Self, InvalidRoot> {
        let mut approved: Vec<PathBuf> = Vec::new();
        for root in roots {
            let plain = names(&root).map_err(|_| InvalidRoot)?;
            if plain.is_empty() {
                return Err(InvalidRoot);
            }
            // Rebuilt from its names, so `/a//b/` and `/a/b` are one root.
            let mut root = PathBuf::from("/");
            root.extend(plain);
            if !approved.contains(&root) {
                approved.push(root);
            }
            if approved.len() > MOST_ROOTS {
                return Err(InvalidRoot);
            }
        }
        Ok(Self(approved))
    }

    /// The roots of two adapters, as one list.
    pub fn union(&self, other: &Self) -> Result<Self, InvalidRoot> {
        Self::new(self.0.iter().chain(&other.0).cloned())
    }

    /// The approved root that holds `path`, if any.
    pub(crate) fn holding(&self, path: &Path) -> Option<&Path> {
        self.0
            .iter()
            .map(PathBuf::as_path)
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
    }

    /// Whether `path` is inside a directory of credentials below its root; with
    /// `or_is_one`, also when `path` is itself such a directory.
    pub(crate) fn in_credential_directory(&self, path: &Path, or_is_one: bool) -> bool {
        let Some(root) = self.holding(path) else {
            return false;
        };
        let Ok(below) = path.strip_prefix(root) else {
            return false;
        };
        let directories = if or_is_one {
            Some(below)
        } else {
            below.parent()
        };
        directories.is_some_and(|directories| {
            directories.components().any(|component| {
                component.as_os_str().to_str().is_some_and(|name| {
                    CREDENTIAL_DIRECTORIES
                        .iter()
                        .any(|credential| name.eq_ignore_ascii_case(credential))
                })
            })
        })
    }
}
