use std::collections::{BTreeMap, BTreeSet};

use wyss_console::{
    agentconfig::catalogue::{self, Candidate},
    arsenal::{
        self,
        store::{self, Error, NewCapability, Store},
    },
};

pub struct Library {
    pub imported: usize,
    pub eligible: BTreeSet<String>,
}

impl Library {
    pub fn scan(store: Option<&Store>) -> Option<Self> {
        let scan = catalogue::scan_process(None)?;
        Self::from_candidates(store, scan.candidates).ok()
    }

    fn from_candidates(store: Option<&Store>, candidates: Vec<Candidate>) -> Result<Self, Error> {
        Ok(Self {
            imported: store
                .map(Store::list)
                .transpose()?
                .map_or(0, |items| items.len()),
            eligible: eligible(store, candidates)?.into_keys().collect(),
        })
    }
}

fn eligible(
    store: Option<&Store>,
    candidates: Vec<Candidate>,
) -> Result<BTreeMap<String, Candidate>, Error> {
    let mut occurrences = BTreeMap::new();
    let mut eligible = BTreeMap::new();
    for candidate in candidates {
        let base = store::identity(
            candidate.provider,
            candidate.kind,
            &candidate.source,
            &candidate.name,
        );
        let seen = occurrences.entry(base.clone()).or_insert(0);
        let id = if *seen == 0 {
            base
        } else {
            format!("{base}#{seen}")
        };
        *seen += 1;
        if candidate.blocked.is_some() || !matches!(candidate.kind, "skill" | "agent" | "rule") {
            continue;
        }
        let current = store.map(|store| store.current(&id)).transpose()?.flatten();
        if current.is_some_and(|item| {
            item.tree_hash == store::tree_digest(&candidate.content)
                && (!item.description.is_empty() || candidate.description.is_empty())
        }) {
            continue;
        }
        eligible.insert(id, candidate);
    }
    Ok(eligible)
}

pub fn import(ids: &BTreeSet<String>) -> Result<(), Error> {
    // Only identities offered by the last scan may be imported. Read their contents again.
    let scan = catalogue::scan_process(None).ok_or(Error::Storage)?;
    import_candidates(ids, scan.candidates)
}

fn import_candidates(ids: &BTreeSet<String>, candidates: Vec<Candidate>) -> Result<(), Error> {
    let (mut store, _) = arsenal::open()?;
    let mut result = Ok(());
    for (identity, candidate) in eligible(Some(&store), candidates)? {
        if !ids.contains(&identity) {
            continue;
        }
        if let Err(error) = store.import(&NewCapability {
            identity,
            provider: candidate.provider.into(),
            source: candidate.source,
            kind: candidate.kind.into(),
            name: candidate.name,
            description: candidate.description,
            origin: candidate.origin,
            files: candidate.content,
        }) {
            result = Err(error);
            if matches!(error, Error::Storage | Error::Corrupt | Error::Unsupported) {
                break;
            }
        }
    }
    publish(&mut store);
    result
}

pub fn delete(id: &str) -> Result<(), Error> {
    let (mut store, _) = arsenal::open()?;
    store.delete_selection(id)?;
    publish(&mut store);
    Ok(())
}

/// How a decision on a received change went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decided {
    /// Kept and applied.
    Done,
    /// Kept; applying it failed and the next run retries.
    Kept,
    /// Not kept: the change was replaced since it was shown, or the library could not be written.
    Failed,
}

/// Accepts or refuses the displayed offer and applies an accepted one now.
pub fn decide(offer: &store::Incoming, accept: bool) -> Decided {
    let Ok((mut store, _)) = arsenal::open() else {
        return Decided::Failed;
    };
    let Some(paths) = arsenal::sync::Paths::from_env() else {
        return Decided::Failed;
    };
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    match arsenal::sync::decide(&mut store, &paths, host.trim(), offer, accept) {
        Ok(Ok(_)) => Decided::Done,
        Ok(Err(_)) => Decided::Kept,
        Err(_) => Decided::Failed,
    }
}

fn publish(store: &mut Store) {
    if let Some(paths) = arsenal::sync::Paths::from_env() {
        let host = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
        let _ = arsenal::sync::reconcile(store, &paths, host.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use wyss_console::{arsenal::store::SelectionInput, launch};

    #[test]
    fn imports_creates_launch_and_deletes_preset_in_fixture_copy() {
        crate::personalization_round_trip(
            "arsenal-library",
            "arsenal::library::tests::round_trip_child",
        );
    }

    #[test]
    fn round_trip_child() {
        let Some(root) = crate::personalization_child_root("arsenal-library") else {
            return;
        };
        let skill = root.join("home/.claude/skills/fixture-research");
        fs::create_dir_all(&skill).unwrap();
        let write_skill = |description| {
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: fixture-research\ndescription: {description}\n---\nRead the fixture.\n"),
            ).unwrap();
        };
        write_skill("Research fixture.");
        // The test runner's /home is owned by nobody, so the production reader rejects
        // that mount before it can read our fixture. Feed the storage flow the same
        // candidate shape using content from the scratch skill.
        let candidates = || {
            vec![Candidate {
                kind: "skill",
                provider: "claude_code",
                source: "claude_user_skills".into(),
                scope: "user",
                name: "fixture-research".into(),
                description: "Research fixture.".into(),
                origin: "~/.claude/skills/fixture-research".into(),
                files: Vec::new(),
                excluded: Vec::new(),
                review: Vec::new(),
                executes_code: false,
                blocked: None,
                managed_by: None,
                content: vec![("SKILL.md".into(), fs::read(skill.join("SKILL.md")).unwrap())],
            }]
        };
        let scan = |store: &Store| Library::from_candidates(Some(store), candidates()).unwrap();
        let (mut seed, directory) = arsenal::open().unwrap();
        assert!(directory.starts_with(&root));
        let saved = seed
            .save_selection(&SelectionInput {
                id: None,
                name: "Fixture preset".into(),
                provider: "claude_code".into(),
                items: Vec::new(),
                composites: Vec::new(),
            })
            .unwrap();
        drop(seed);
        let sqlite = directory.join("arsenal.sqlite");
        let original = root.join("seed.sqlite");
        fs::rename(&sqlite, &original).unwrap();
        fs::copy(&original, &sqlite).unwrap();

        let store = arsenal::open_read_only().unwrap();
        let before = scan(&store);
        assert_eq!(before.imported, 0);
        assert_eq!(before.eligible.len(), 1);
        drop(store);
        import_candidates(&before.eligible, candidates()).unwrap();
        let store = arsenal::open_read_only().unwrap();
        let after = scan(&store);
        assert_eq!(after.imported, 1);
        assert!(after.eligible.is_empty());
        write_skill("Research fixture revised.");
        let revised = scan(&store);
        assert_eq!(revised.eligible, before.eligible);
        drop(store);
        import_candidates(&revised.eligible, candidates()).unwrap();
        let store = arsenal::open_read_only().unwrap();
        assert_eq!(scan(&store).imported, 1);
        assert!(scan(&store).eligible.is_empty());

        let places = launch::Places::from_env().unwrap();
        assert!(places.folder.starts_with(&root));
        let draft = crate::workspace::model::Draft::from_preset(&places, &saved, &store).unwrap();
        let created = crate::workspace::model::save(&places, &draft).unwrap();
        assert_eq!(
            super::super::model::launches_for(&saved, std::slice::from_ref(&created)),
            [&created]
        );
        drop(store);
        delete(&saved.id).unwrap();
        assert!(
            arsenal::open_read_only()
                .unwrap()
                .selections()
                .unwrap()
                .is_empty()
        );
        assert_eq!(launch::get(&places, &created.id).unwrap(), created);
        assert_ne!(fs::read(original).unwrap(), fs::read(sqlite).unwrap());
    }
}
