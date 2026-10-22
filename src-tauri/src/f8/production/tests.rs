use super::*;
use crate::contracts::{
    WireUuid,
    identities::{AgentRuntimeInstanceId, ManagedSessionId},
};
use std::os::unix::fs::PermissionsExt;

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("wyss-prod-{}", uuid::Uuid::new_v4()));
        for d in ["", "run", "state", "home"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(root.join(d))
                .unwrap();
        }
        Self(root)
    }
    fn config(&self, credential: bool) -> CodexConfig {
        CodexConfig {
            executable: self.0.join("codex"),
            home: self.0.join("home"),
            allow_credential: credential,
        }
    }
    fn auth(&self) {
        let path = self.0.join("home/auth.json");
        fs::write(&path, "FIXTURE-CREDENTIAL").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn spawner(&self, credential: bool) -> ControlResult<(CodexSpawner, StagingRoot)> {
        CodexSpawner::new(
            self.config(credential),
            &self.0.join("run"),
            &self.0.join("state"),
        )
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn reference() -> RuntimeRef {
    RuntimeRef {
        schema: 2,
        authority_instance_id: WireUuid::try_from(
            "00000000-0000-4000-8000-0000000000a1".to_owned(),
        )
        .unwrap(),
        managed_session_id: ManagedSessionId::parse("00000000-0000-4000-8000-000000000001")
            .unwrap(),
        runtime_instance_id: AgentRuntimeInstanceId::parse("00000000-0000-4000-8000-000000000002")
            .unwrap(),
        generation: 1,
    }
}

#[test]
fn the_choices_are_checked_before_anything_is_created() {
    let t = Tree::new();
    let mut c = t.config(false);
    assert!(c.check().is_ok());
    c.executable = "relative/codex".into();
    assert_eq!(c.check(), Err(E::UnsafePath));
    let mut c = t.config(false);
    c.home = t.0.join("missing");
    assert_eq!(c.check(), Err(E::UnsafePath));
    c.home = "relative".into();
    assert_eq!(c.check(), Err(E::UnsafePath));
    // Nothing was created by any of that.
    assert!(!t.0.join("state/staging").exists());
    assert!(!t.0.join("run/runtime-cwd").exists());
}

#[test]
fn a_credential_in_the_home_is_refused_unless_it_was_allowed_by_name() {
    let t = Tree::new();
    t.auth();
    assert_eq!(t.config(false).check(), Err(E::GateDenied));
    assert!(t.spawner(false).is_err());
    assert!(
        !t.0.join("state/staging").exists(),
        "a refusal creates nothing"
    );
    assert!(t.config(true).check().is_ok());
    assert!(t.spawner(true).is_ok());
}

#[test]
fn a_credential_that_appears_after_the_start_is_refused_at_the_start_that_would_bind_it() {
    let t = Tree::new();
    let (mut spawner, staging_root) = t.spawner(false).unwrap();
    t.auth(); // `codex login` after the authority was started
    let launch = reference();
    let mut staging = staging_root.create(&launch.runtime_instance_id).unwrap();
    let refused = spawner.spawn_for(&launch, Some(&mut staging), Some("/tmp"));
    assert_eq!(refused.err(), Some(E::GateDenied));
}

#[test]
fn it_creates_what_it_owns_privately_and_starts_in_an_empty_directory_or_not_at_all() {
    let t = Tree::new();
    let (_spawner, _root) = t.spawner(false).unwrap();
    for dir in ["state/staging", "run/runtime-cwd"] {
        let mode = fs::metadata(t.0.join(dir)).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "{dir}");
    }
    fs::write(t.0.join("run/runtime-cwd/AGENTS.md"), "x").unwrap();
    assert_eq!(t.spawner(false).err(), Some(E::UnsafePath));
}

#[test]
fn only_the_local_tools_profile_with_a_workspace_is_started() {
    let t = Tree::new();
    let (mut spawner, staging_root) = t.spawner(false).unwrap();
    assert!(spawner.supports_profile(RuntimeSafetyProfile::CodexLocalToolsV1));
    assert!(!spawner.supports_profile(RuntimeSafetyProfile::CodexInferenceV1));
    assert!(spawner.descendant_containment());
    let launch = reference();
    // No workspace: nothing to isolate this launch for.
    assert_eq!(spawner.spawn(&launch, None).err(), Some(E::Unsupported));
    let mut staging = staging_root.create(&launch.runtime_instance_id).unwrap();
    assert_eq!(
        spawner.spawn_for(&launch, Some(&mut staging), None).err(),
        Some(E::Unsupported)
    );
    // No staging: nowhere private to put this launch's catalogue.
    assert_eq!(
        spawner.spawn_for(&launch, None, Some("/tmp")).err(),
        Some(E::Unsupported)
    );
}

#[test]
fn a_missing_or_foreign_model_cache_keeps_the_runtime_from_starting() {
    let t = Tree::new();
    let (mut spawner, staging_root) = t.spawner(false).unwrap();
    let launch = reference();
    let mut staging = staging_root.create(&launch.runtime_instance_id).unwrap();
    // No cache at all.
    assert_eq!(
        spawner
            .spawn_for(&launch, Some(&mut staging), Some("/tmp"))
            .err(),
        Some(E::Unsupported)
    );
    // A link in place of the cache is not followed.
    fs::write(t.0.join("elsewhere.json"), "{}").unwrap();
    std::os::unix::fs::symlink(
        t.0.join("elsewhere.json"),
        t.0.join("home/models_cache.json"),
    )
    .unwrap();
    assert_eq!(
        spawner
            .spawn_for(&launch, Some(&mut staging), Some("/tmp"))
            .err(),
        Some(E::Unsupported)
    );
    // Something that is not a catalogue.
    fs::remove_file(t.0.join("home/models_cache.json")).unwrap();
    fs::write(t.0.join("home/models_cache.json"), "not json").unwrap();
    assert_eq!(
        spawner
            .spawn_for(&launch, Some(&mut staging), Some("/tmp"))
            .err(),
        Some(E::Unsupported)
    );
}
