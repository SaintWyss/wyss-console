//! Cross-record rules of `StoreV1` (contract §2). Deserializing proves each record's shape;
//! this proves the store is consistent: unique IDs, references that exist on this host,
//! the Scratch and tombstone rules, layouts inside their bounds, and the Native/Managed
//! profile separation. Ceilings are checked first, so an oversized store is reported as such.
use super::records::{
    Association, Binding, LaunchProvider, ManagedSafetyProfile, Mode, Node, Operation,
    OperationState, Preferences, StoreV1, Target, Workspace,
};
use super::{
    M1Result, MAX_ASSOCIATIONS, MAX_BINDINGS_PER_PROJECT, MAX_CLOSED_OPERATIONS, MAX_LAYOUT_DEPTH,
    MAX_LEAVES, MAX_MIGRATION_ROWS, MAX_OPERATIONS, MAX_PROJECTS, MAX_SPLIT_RATIO,
    MAX_WORKSPACES_PER_PROJECT, MIN_SPLIT_RATIO, ensure, scalar::Counter, within,
};
use std::collections::{HashMap, HashSet};

/// The label the one projectless workspace must carry.
const SCRATCH_LABEL: &str = "Scratch";

/// Keys the records of one kind by ID, refusing a repeat.
fn unique<'a, T>(
    items: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    field: &'static str,
) -> M1Result<HashMap<&'a str, &'a T>> {
    let mut by_id = HashMap::with_capacity(items.len());
    for item in items {
        ensure(by_id.insert(id(item), item).is_none(), field)?;
    }
    Ok(by_id)
}

/// A revision at the end of the range could never advance.
fn can_advance(revision: Counter, field: &'static str) -> M1Result {
    ensure(revision.next().is_some(), field)
}

/// A binding reference is an ID and the revision it was observed at, or neither.
fn paired<T, U>(id: &Option<T>, revision: &Option<U>, field: &'static str) -> M1Result {
    ensure(id.is_some() == revision.is_some(), field)
}

impl StoreV1 {
    /// Checks every ceiling and every cross-record rule. A store that fails is never repaired
    /// or replaced by an empty one.
    pub fn validate(&self) -> M1Result {
        self.check_ceilings()?;
        can_advance(self.revision, "revision")?;

        let projects = unique(&self.projects, |p| p.id.as_str(), "projects")?;
        let bindings = unique(&self.bindings, |b| b.id.as_str(), "bindings")?;
        let workspaces = unique(&self.workspaces, |w| w.id.as_str(), "workspaces")?;
        let sessions = unique(&self.sessions, |a| a.id.as_str(), "sessions")?;
        let operations = unique(&self.operations, |o| o.id.as_str(), "operations")?;
        let closed: HashSet<&str> = self.closed_operations.iter().map(|o| o.as_str()).collect();
        ensure(
            closed.len() == self.closed_operations.len()
                && !operations.keys().any(|id| closed.contains(id)),
            "closedOperations",
        )?;

        self.check_projects(&bindings, &workspaces)?;
        self.check_bindings(&projects, &bindings)?;
        self.check_sessions(&projects, &bindings, &operations, &closed)?;
        self.check_workspaces(&projects, &sessions)?;
        self.check_operations(&projects, &bindings, &workspaces, &sessions)?;
        self.check_migration()
    }

    fn check_ceilings(&self) -> M1Result {
        within(self.projects.len(), MAX_PROJECTS, "projects")?;
        within(self.sessions.len(), MAX_ASSOCIATIONS, "sessions")?;
        within(self.operations.len(), MAX_OPERATIONS, "operations")?;
        within(
            self.closed_operations.len(),
            MAX_CLOSED_OPERATIONS,
            "closedOperations",
        )?;
        if let Some(migration) = &self.migration {
            within(migration.map.len(), MAX_MIGRATION_ROWS, "migration")?;
        }
        let mut per_project: HashMap<&str, (usize, usize)> = HashMap::new();
        for workspace in &self.workspaces {
            within(workspace.leaves.len(), MAX_LEAVES, "leaves")?;
            if let Some(project) = &workspace.project_id {
                per_project.entry(project.as_str()).or_default().0 += 1;
            }
        }
        for binding in &self.bindings {
            per_project
                .entry(binding.project_id.as_str())
                .or_default()
                .1 += 1;
        }
        for (workspaces, bindings) in per_project.values() {
            within(*workspaces, MAX_WORKSPACES_PER_PROJECT, "workspaces")?;
            within(*bindings, MAX_BINDINGS_PER_PROJECT, "bindings")?;
        }
        Ok(())
    }

    fn check_projects(
        &self,
        bindings: &HashMap<&str, &Binding>,
        workspaces: &HashMap<&str, &Workspace>,
    ) -> M1Result {
        for project in &self.projects {
            can_advance(project.revision, "projects.revision")?;
            let id = project.id.as_str();
            let owned_workspaces = self
                .workspaces
                .iter()
                .filter(|w| w.project_id.as_ref().is_some_and(|p| p.as_str() == id))
                .count();
            let owned_bindings = self
                .bindings
                .iter()
                .filter(|b| b.project_id.as_str() == id)
                .count();
            if project.tombstone {
                // A tombstone keeps its ID, label and history, and nothing that could act.
                ensure(
                    project.default_binding_id.is_none()
                        && project.default_workspace_id.is_none()
                        && owned_workspaces == 0
                        && owned_bindings == 0,
                    "projects.tombstone",
                )?;
                continue;
            }
            let default = project
                .default_workspace_id
                .as_ref()
                .and_then(|w| workspaces.get(w.as_str()));
            ensure(
                default.is_some_and(|w| w.project_id.as_ref() == Some(&project.id)),
                "projects.defaultWorkspaceId",
            )?;
            if let Some(binding) = &project.default_binding_id {
                ensure(
                    bindings
                        .get(binding.as_str())
                        .is_some_and(|b| b.project_id == project.id),
                    "projects.defaultBindingId",
                )?;
            }
        }
        Ok(())
    }

    fn check_bindings(
        &self,
        projects: &HashMap<&str, &super::records::Project>,
        bindings: &HashMap<&str, &Binding>,
    ) -> M1Result {
        for binding in &self.bindings {
            can_advance(binding.revision, "bindings.revision")?;
            ensure(binding.host_id == self.host_id, "bindings.hostId")?;
            ensure(
                projects
                    .get(binding.project_id.as_str())
                    .is_some_and(|p| !p.tombstone),
                "bindings.projectId",
            )?;
            // Relinking chains each binding to the one it replaced: same project, no cycle.
            let mut next = binding.previous_binding_id.as_ref();
            let mut steps = 0;
            while let Some(previous) = next {
                steps += 1;
                let found = bindings.get(previous.as_str());
                ensure(
                    steps <= MAX_BINDINGS_PER_PROJECT
                        && previous != &binding.id
                        && found.is_some_and(|b| b.project_id == binding.project_id),
                    "bindings.previousBindingId",
                )?;
                next = found.and_then(|b| b.previous_binding_id.as_ref());
            }
        }
        Ok(())
    }

    fn check_workspaces(
        &self,
        projects: &HashMap<&str, &super::records::Project>,
        sessions: &HashMap<&str, &Association>,
    ) -> M1Result {
        let mut scratch = 0;
        let mut leaf_ids = HashSet::new();
        for workspace in &self.workspaces {
            // A leaf ID names one placement in the whole store, so a duplicated workspace
            // must have new ones.
            for leaf in &workspace.leaves {
                ensure(leaf_ids.insert(leaf.id.as_str()), "workspaces.leaves")?;
            }
            can_advance(workspace.revision, "workspaces.revision")?;
            ensure(workspace.host_id == self.host_id, "workspaces.hostId")?;
            match &workspace.project_id {
                Some(project) => ensure(
                    projects.get(project.as_str()).is_some_and(|p| !p.tombstone),
                    "workspaces.projectId",
                )?,
                None => {
                    scratch += 1;
                    let native = workspace.preferences.mode == Mode::Native;
                    ensure(
                        workspace.label.as_str() == SCRATCH_LABEL && native,
                        "workspaces.scratch",
                    )?;
                }
            }
            check_preferences(&workspace.preferences)?;
            check_layout(workspace, sessions)?;
        }
        // The projectless workspace exists exactly once, even in an otherwise empty store.
        ensure(scratch == 1, "workspaces.scratch")
    }

    fn check_sessions(
        &self,
        projects: &HashMap<&str, &super::records::Project>,
        bindings: &HashMap<&str, &Binding>,
        operations: &HashMap<&str, &Operation>,
        closed: &HashSet<&str>,
    ) -> M1Result {
        let mut targets = HashSet::new();
        let mut native_sessions = HashSet::new();
        for session in &self.sessions {
            ensure(session.host_id == self.host_id, "sessions.hostId")?;
            ensure(
                session
                    .origin_project_id
                    .as_ref()
                    .is_none_or(|p| projects.contains_key(p.as_str())),
                "sessions.originProjectId",
            )?;
            paired(
                &session.binding_id,
                &session.binding_revision,
                "sessions.bindingRevision",
            )?;
            if let Some(binding) = &session.binding_id {
                // A bound session belongs to the project its binding belongs to.
                ensure(
                    bindings
                        .get(binding.as_str())
                        .is_some_and(|b| session.origin_project_id.as_ref() == Some(&b.project_id)),
                    "sessions.bindingId",
                )?;
            }
            // One owner target has one association: the same terminal or Managed session
            // never appears twice under different IDs.
            ensure(
                targets.insert(target_key(&session.target)),
                "sessions.target",
            )?;
            if let Target::Native { session_id, .. } = &session.target {
                ensure(
                    native_sessions.insert(session_id.as_str()),
                    "sessions.target",
                )?;
            }
            if let Some(id) = &session.operation_id {
                match operations.get(id.as_str()) {
                    Some(operation) => ensure(
                        operation.association_id.as_ref() == Some(&session.id),
                        "sessions.operationId",
                    )?,
                    None => ensure(closed.contains(id.as_str()), "sessions.operationId")?,
                }
            }
        }
        Ok(())
    }

    fn check_operations(
        &self,
        projects: &HashMap<&str, &super::records::Project>,
        bindings: &HashMap<&str, &Binding>,
        workspaces: &HashMap<&str, &Workspace>,
        sessions: &HashMap<&str, &Association>,
    ) -> M1Result {
        let mut busy = HashSet::new();
        for operation in &self.operations {
            check_launch_profiles(operation)?;
            paired(
                &operation.binding_id,
                &operation.binding_revision,
                "operations.bindingRevision",
            )?;
            let pending = operation.state == OperationState::Pending;
            ensure(pending == operation.claim.is_some(), "operations.claim")?;
            // Only `committed` must have an association; `ambiguous` may keep the one a lost
            // acknowledgement left behind; `pending` and `refused` have none.
            ensure(
                match operation.state {
                    OperationState::Committed => operation.association_id.is_some(),
                    OperationState::Ambiguous => true,
                    OperationState::Pending | OperationState::Refused => {
                        operation.association_id.is_none()
                    }
                },
                "operations.associationId",
            )?;
            if matches!(
                operation.state,
                OperationState::Pending | OperationState::Ambiguous
            ) {
                // A leaf has at most one unfinished receipt (`leaf_busy`), whether or not
                // the leaf still exists.
                ensure(
                    busy.insert((operation.workspace_id.as_str(), operation.leaf_id.as_str())),
                    "operations.leafId",
                )?;
            }
            if pending {
                // A request in flight names a context that exists and agrees with itself.
                let workspace = workspaces.get(operation.workspace_id.as_str());
                ensure(
                    workspace.is_some_and(|w| {
                        w.leaves.iter().any(|l| l.id == operation.leaf_id)
                            && w.project_id == operation.project_id
                    }),
                    "operations.leafId",
                )?;
                ensure(
                    operation
                        .project_id
                        .as_ref()
                        .is_none_or(|p| projects.get(p.as_str()).is_some_and(|p| !p.tombstone)),
                    "operations.projectId",
                )?;
                ensure(
                    operation.binding_id.as_ref().is_none_or(|b| {
                        bindings
                            .get(b.as_str())
                            .is_some_and(|b| Some(&b.project_id) == operation.project_id.as_ref())
                    }),
                    "operations.bindingId",
                )?;
            }
            // Where both ends of a receipt still exist they must agree. Finished receipts are
            // history and may outlive the records they name.
            if let Some(session) = operation
                .association_id
                .as_ref()
                .and_then(|id| sessions.get(id.as_str()))
            {
                ensure(
                    session.operation_id.as_ref() == Some(&operation.id),
                    "operations.associationId",
                )?;
                ensure(
                    receipt_matches_target(operation, &session.target),
                    "operations.target",
                )?;
            }
        }
        Ok(())
    }

    fn check_migration(&self) -> M1Result {
        let Some(migration) = &self.migration else {
            return Ok(());
        };
        let kinds: HashSet<_> = migration.sources.iter().map(|s| s.kind).collect();
        ensure(
            !migration.sources.is_empty() && kinds.len() == migration.sources.len(),
            "migration.sources",
        )?;
        let mut rows = HashSet::new();
        for row in &migration.map {
            ensure(
                rows.insert((row.source_kind, row.source_key.as_str())),
                "migration.map",
            )?;
        }
        Ok(())
    }
}

/// The identity of an owner's session, as the owner names it.
fn target_key(target: &Target) -> (u8, &str, &str, &str) {
    match target {
        // A terminal is one thing, whichever kind of session runs in it.
        Target::Native {
            terminal_id,
            host_epoch,
            ..
        }
        | Target::Shell {
            terminal_id,
            host_epoch,
        } => (1, terminal_id.as_str(), host_epoch.as_str(), ""),
        Target::Managed {
            protocol,
            store_stream_id,
            session_id,
            ..
        } => (
            match protocol {
                super::records::ManagedProtocol::P1 => 3,
                super::records::ManagedProtocol::P2 => 4,
            },
            store_stream_id.as_str(),
            session_id.as_str(),
            "",
        ),
    }
}

/// The kind of session a receipt asked for is the kind of session its association holds, and a
/// Managed association keeps the requested profile of the receipt, no other.
fn receipt_matches_target(operation: &Operation, target: &Target) -> bool {
    match target {
        Target::Native { .. } => {
            operation.mode == Mode::Native && operation.provider != LaunchProvider::Shell
        }
        Target::Shell { .. } => {
            operation.mode == Mode::Native && operation.provider == LaunchProvider::Shell
        }
        Target::Managed {
            protocol,
            store_stream_id,
            requested_safety_profile,
            ..
        } => {
            operation.mode == Mode::Managed
                && *requested_safety_profile == operation.managed_safety_profile
                // The session belongs to the authority that was asked, no other.
                && operation.managed_authority.as_ref().is_some_and(|authority| {
                    authority.protocol == *protocol && authority.store_stream_id == *store_stream_id
                })
        }
    }
}

/// A Native workspace names no Managed profile and a Managed one names no Native profile.
/// `local_tools` is the privileged choice, so it is never stored as a preference.
fn check_preferences(preferences: &Preferences) -> M1Result {
    match preferences.mode {
        Mode::Native => ensure(
            preferences.managed_safety_profile.is_none(),
            "preferences.managedSafetyProfile",
        ),
        Mode::Managed => {
            ensure(
                preferences.native_profile.is_none(),
                "preferences.nativeProfile",
            )?;
            ensure(
                preferences.managed_safety_profile != Some(ManagedSafetyProfile::LocalTools),
                "preferences.managedSafetyProfile",
            )
        }
    }
}

/// The same separation for a launch receipt: Native launches carry no Managed profile,
/// Managed launches an explicit one and no Native profile, and a shell is Native with neither.
fn check_launch_profiles(operation: &Operation) -> M1Result {
    let valid = match (operation.mode, operation.provider) {
        (Mode::Native, LaunchProvider::Shell) => {
            operation.native_profile.is_none() && operation.managed_safety_profile.is_none()
        }
        (Mode::Native, _) => operation.managed_safety_profile.is_none(),
        (Mode::Managed, LaunchProvider::Shell) => false,
        (Mode::Managed, _) => {
            operation.native_profile.is_none() && operation.managed_safety_profile.is_some()
        }
    };
    ensure(valid, "operations.profile")?;
    // The authority a Managed launch was first asked of is recorded with it, and only with it.
    ensure(
        (operation.mode == Mode::Managed) == operation.managed_authority.is_some(),
        "operations.managedAuthority",
    )
}

/// The layout tree, its leaves and the associations they hold.
fn check_layout(workspace: &Workspace, sessions: &HashMap<&str, &Association>) -> M1Result {
    let mut placed = Vec::new();
    if let Some(layout) = &workspace.layout {
        walk(layout, 0, &mut placed)?;
    }
    let declared: HashSet<&str> = workspace.leaves.iter().map(|l| l.id.as_str()).collect();
    let placed_once: HashSet<&str> = placed.iter().copied().collect();
    // Each leaf is placed exactly once, and every placement has its leaf record.
    ensure(
        declared.len() == workspace.leaves.len()
            && placed.len() == workspace.leaves.len()
            && declared == placed_once,
        "workspaces.leaves",
    )?;
    ensure(
        workspace
            .focus
            .as_ref()
            .is_none_or(|focus| declared.contains(focus.as_str())),
        "workspaces.focus",
    )?;
    let mut held = HashSet::new();
    for leaf in &workspace.leaves {
        if let Some(association) = &leaf.association_id {
            ensure(
                sessions.get(association.as_str()).is_some_and(|a| {
                    // A workspace shows its own project's sessions, and Scratch only the
                    // unassigned ones: a session moves between them by explicit adoption,
                    // never because a layout names it.
                    a.origin_project_id == workspace.project_id
                }) && held.insert(association.as_str()),
                "workspaces.leaves.associationId",
            )?;
        }
    }
    Ok(())
}

fn walk<'a>(node: &'a Node, level: usize, placed: &mut Vec<&'a str>) -> M1Result {
    ensure(level <= MAX_LAYOUT_DEPTH, "layout.depth")?;
    match node {
        Node::Leaf { id } => placed.push(id.as_str()),
        Node::Split { ratio, a, b, .. } => {
            ensure(
                ratio.is_finite() && (MIN_SPLIT_RATIO..=MAX_SPLIT_RATIO).contains(ratio),
                "layout.ratio",
            )?;
            walk(a, level + 1, placed)?;
            walk(b, level + 1, placed)?;
        }
    }
    Ok(())
}
