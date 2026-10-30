//! Choosing the project whose own files the configuration engine reads and may write (F5, ADR 0050).
//!
//! The path comes from the desktop's native folder window, never from the page. Before it
//! becomes the project it is judged the way every other place is: from the root, one directory
//! descriptor at a time, with no link anywhere on the way. It must be the account's own, inside
//! the account's home, and it must not be a provider's own folder or a credential folder.
//!
//! A folder that another account can write (its group, or an account named in its access list)
//! is accepted read only. Its files carry that trust themselves, so the engine lists them and
//! never writes them (`model::writability`), and the catalogue never copies them. A folder that
//! anybody can write, or that another account owns, is refused.
use super::inventory::Environment;
use super::place::{Reached, Want, names, reach};
use super::roots::ApprovedRoots;
use nix::{sys::stat::fstat, unistd::geteuid};
use std::{
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

/// The folder names a provider keeps its own configuration under. A project is never one of them,
/// wherever it sits: a `.claude` folder inside a repository belongs to that provider, not to the repository.
const PROVIDER_FOLDER_NAMES: [&str; 4] = [".claude", ".codex", ".opencode", ".agents"];

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
    /// Another account owns it, or anybody can write it or a folder above it.
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
    if rebuilt
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| PROVIDER_FOLDER_NAMES.contains(&name))
    {
        return Err(ProjectRefusal::ProviderFolder);
    }
    let declared = super::declared::uids(&env.declared_writers);
    match reach(&rebuilt, Want::Directory, &declared) {
        // Trusted, or read only because another account can write it: either way it is the account's own
        // folder, and what it holds is read only (see the module note).
        Ok(Reached::Node(fd, _trust)) => {
            let stat = fstat(fd.as_raw_fd()).map_err(|_| ProjectRefusal::Missing)?;
            if stat.st_uid == geteuid().as_raw() {
                Ok(rebuilt)
            } else {
                Err(ProjectRefusal::NotOurs)
            }
        }
        Ok(Reached::Link(..)) => Err(ProjectRefusal::Link),
        Err(super::model::SourceFailure::Missing) => Err(ProjectRefusal::Missing),
        Err(super::model::SourceFailure::UntrustedPath { reason }) => Err(match reason {
            super::model::UntrustedReason::AncestorLink => ProjectRefusal::Link,
            _ => ProjectRefusal::NotOurs,
        }),
        Err(_) => Err(ProjectRefusal::Missing),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        os::unix::{fs::PermissionsExt, fs::symlink},
    };

    /// A home of this test's own, with a `work` folder that only this account can write.
    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!("project-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(home.join("work")).unwrap();
        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(home.join("work"), fs::Permissions::from_mode(0o755)).unwrap();
        home
    }

    fn folder(home: &Path, name: &str, mode: u32) -> PathBuf {
        let folder = home.join("work").join(name);
        fs::create_dir_all(&folder).unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(mode)).unwrap();
        folder
    }

    fn env_at(home: &Path) -> Environment {
        Environment {
            home: home.to_path_buf(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            system: PathBuf::from("/etc"),
            declared_writers: Vec::new(),
        }
    }

    #[test]
    fn a_folder_another_account_can_write_is_accepted_for_read_only_use() {
        let home = home();
        let shared = folder(&home, "repo", 0o775);
        assert_eq!(validate(&env_at(&home), &shared), Ok(shared.clone()));
        // Negative control: the same folder, closed to others, is accepted too, so the rule is about who can write.
        let own = folder(&home, "own", 0o755);
        assert_eq!(validate(&env_at(&home), &own), Ok(own.clone()));
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_folder_anybody_can_write_is_refused_whether_it_is_the_project_or_above_it() {
        let home = home();
        let open = folder(&home, "open", 0o777);
        assert_eq!(
            validate(&env_at(&home), &open),
            Err(ProjectRefusal::NotOurs)
        );
        // The same refusal for a project whose parent is open to everyone.
        fs::set_permissions(home.join("work"), fs::Permissions::from_mode(0o777)).unwrap();
        let inside = folder(&home, "inside", 0o755);
        assert_eq!(
            validate(&env_at(&home), &inside),
            Err(ProjectRefusal::NotOurs)
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_provider_folder_is_refused_wherever_it_sits_and_other_names_are_not() {
        let home = home();
        for name in PROVIDER_FOLDER_NAMES {
            let provider = folder(&home, name, 0o755);
            assert_eq!(
                validate(&env_at(&home), &provider),
                Err(ProjectRefusal::ProviderFolder),
                "{name} inside a repository must not be a project"
            );
        }
        // Negative control: a name that only resembles a provider folder is an ordinary project.
        let resembling = folder(&home, "claude-notes", 0o755);
        assert_eq!(
            validate(&env_at(&home), &resembling),
            Ok(resembling.clone())
        );
        fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_link_on_the_way_to_the_project_is_refused() {
        let home = home();
        folder(&home, "repo", 0o755);
        symlink(home.join("work"), home.join("linked")).unwrap();
        assert_eq!(
            validate(&env_at(&home), &home.join("linked/repo")),
            Err(ProjectRefusal::Link)
        );
        // Negative control: the same project through its real path is accepted.
        assert_eq!(
            validate(&env_at(&home), &home.join("work/repo")),
            Ok(home.join("work/repo"))
        );
        fs::remove_dir_all(home).unwrap();
    }
}
