//! Choosing the project whose own files the configuration engine reads and may write (F5, ADR 0050).
//!
//! The path comes from the desktop's native folder window, never from the page. Before it
//! becomes the project it is judged the way every other place is: from the root, one directory
//! descriptor at a time, with no link anywhere on the way and nobody else able to write any
//! directory down to it. It must be the account's own, inside the account's home, and it must
//! not be a provider's own folder or a credential folder.
use super::inventory::Environment;
use super::model::PathTrust;
use super::place::{Reached, Want, names, reach};
use super::roots::ApprovedRoots;
use nix::{sys::stat::fstat, unistd::geteuid};
use std::{
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

/// Why a folder cannot be the project. Said to the user as one word; none carries the path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectRefusal {
    /// Not a plain absolute path.
    NotAbsolute,
    /// Not below the account's home, or the home itself.
    OutsideHome,
    /// A provider's own folder, or a credential folder.
    ProviderFolder,
    /// There is a link on the way, or the folder is one.
    Link,
    /// Another account owns it, or can write it or a folder above it.
    NotOurs,
    /// Not there, or not a folder.
    Missing,
}

/// The project as the engine will keep it, or the reason it cannot be one.
pub fn validate(env: &Environment, project: &Path) -> Result<PathBuf, ProjectRefusal> {
    let plain = names(project).map_err(|_| ProjectRefusal::NotAbsolute)?;
    if !project.is_absolute() || plain.is_empty() {
        return Err(ProjectRefusal::NotAbsolute);
    }
    let mut rebuilt = PathBuf::from("/");
    rebuilt.extend(plain);
    if !rebuilt.starts_with(&env.home) || rebuilt == env.home {
        return Err(ProjectRefusal::OutsideHome);
    }
    let home = ApprovedRoots::new([env.home.clone()]).map_err(|_| ProjectRefusal::OutsideHome)?;
    if home.in_credential_directory(&rebuilt, true)
        || [
            env.claude_dir(),
            env.codex_dir(),
            env.opencode_dir(),
            env.agents_skills_dir(),
        ]
        .iter()
        .any(|folder| rebuilt.starts_with(folder) || folder.starts_with(&rebuilt))
    {
        return Err(ProjectRefusal::ProviderFolder);
    }
    let declared = super::declared::uids(&env.declared_writers);
    match reach(&rebuilt, Want::Directory, &declared) {
        Ok(Reached::Node(fd, PathTrust::Trusted)) => {
            let stat = fstat(fd.as_raw_fd()).map_err(|_| ProjectRefusal::Missing)?;
            if stat.st_uid == geteuid().as_raw() {
                Ok(rebuilt)
            } else {
                Err(ProjectRefusal::NotOurs)
            }
        }
        Ok(Reached::Node(..)) => Err(ProjectRefusal::NotOurs),
        Ok(Reached::Link(..)) => Err(ProjectRefusal::Link),
        Err(super::model::SourceFailure::Missing) => Err(ProjectRefusal::Missing),
        Err(super::model::SourceFailure::UntrustedPath { reason }) => Err(match reason {
            super::model::UntrustedReason::AncestorLink => ProjectRefusal::Link,
            _ => ProjectRefusal::NotOurs,
        }),
        Err(_) => Err(ProjectRefusal::Missing),
    }
}
