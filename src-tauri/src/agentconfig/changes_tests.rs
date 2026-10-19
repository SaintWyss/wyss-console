//! Changing a configuration, end to end, on private temporary homes (NFR-MNT-03): the
//! acceptance criteria of F2a that a machine can check, the secret regression tests of
//! ADR 0034, and a fault injected at every step of an apply (14 §4).
use super::change::{Operation, Outcome, Risk};
use super::changes::{ChangeFailure, ChangeService, Confirm, ConfirmRequest};
use super::fault::{Fault, Faults, Step};
use super::inventory::{Environment, SourceKey, item_id_for};
use super::model::{Bounds, ItemKind};
use super::restore::{Protection, State};
use super::service::InventoryService;
use super::vault::{MemoryStore, Vault};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

thread_local! {
    /// Accounts the next worlds of this thread declare as co-writers (a test sets and clears it).
    static DECLARED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

const MARKER: &str = "SEKRET-0f3a9c-marker";

const SETTINGS: &str = r#"{
  "model": "opus",
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [
        { "type": "command", "command": "guard.sh" },
        { "type": "command", "command": "second.sh" } ] }
    ],
    "Stop": [
      { "hooks": [ { "type": "command", "command": "stop.sh" } ] }
    ]
  }
}
"#;

#[derive(Default)]
struct FakeConfirm {
    accept: AtomicBool,
    asked: Mutex<Vec<ConfirmRequest>>,
}
impl Confirm for Arc<FakeConfirm> {
    fn ask(&self, request: &ConfirmRequest) -> bool {
        self.asked.lock().unwrap().push(request.clone());
        self.accept.load(Ordering::SeqCst)
    }
}

struct World {
    base: PathBuf,
    home: PathBuf,
    state: PathBuf,
    keys: Arc<MemoryStore>,
    confirm: Arc<FakeConfirm>,
    faults: Faults,
    service: ChangeService,
    inventory: InventoryService,
}

fn settings_path(home: &Path) -> PathBuf {
    home.join(".claude/settings.json")
}

impl World {
    fn new(settings: &str) -> Self {
        let base = fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-chg-{}", uuid::Uuid::new_v4().simple()));
        fs::DirBuilder::new().mode(0o700).create(&base).unwrap();
        let home = base.join("home");
        fs::create_dir_all(home.join(".claude/skills/noisy")).unwrap();
        fs::write(home.join(".claude/skills/noisy/SKILL.md"), "# noisy").unwrap();
        let state = base.join("state");
        fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
        let world = Self::over(base, home, state, MemoryStore::new(), Faults::default());
        fs::write(settings_path(&world.home), settings).unwrap();
        fs::set_permissions(
            settings_path(&world.home),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        world.inventory.rescan();
        world
    }

    fn over(
        base: PathBuf,
        home: PathBuf,
        state: PathBuf,
        keys: Arc<MemoryStore>,
        faults: Faults,
    ) -> Self {
        let environment = Environment {
            home: home.clone(),
            claude_config_dir: None,
            codex_home: None,
            project: None,
            system: base.join("etc"),
            declared_writers: DECLARED.with(|d| d.borrow().clone()),
        };
        let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
        let confirm = Arc::new(FakeConfirm::default());
        confirm.accept.store(true, Ordering::SeqCst);
        let service = ChangeService::for_providers(
            environment,
            inventory.clone(),
            &state,
            Vault::new(Box::new(keys.clone())),
            Box::new(confirm.clone()),
            faults.clone(),
        )
        .unwrap();
        Self {
            base,
            home,
            state,
            keys,
            confirm,
            faults,
            service,
            inventory,
        }
    }

    /// The same disk, a new process: what a start after a crash sees.
    fn restarted(&self) -> Self {
        Self::over(
            self.base.clone(),
            self.home.clone(),
            self.state.clone(),
            self.keys.clone(),
            self.faults.clone(),
        )
    }

    fn file(&self) -> String {
        fs::read_to_string(settings_path(&self.home)).unwrap()
    }
    fn hook(&self, event: &str, group: usize, index: usize) -> String {
        item_id_for(
            SourceKey::ClaudeUserSettings,
            &[event, &group.to_string(), &index.to_string()],
        )
    }
    fn leftovers(&self) -> Vec<String> {
        fs::read_dir(self.home.join(".claude"))
            .unwrap()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with('.') && n.ends_with(".tmp"))
            .collect()
    }
    /// Every file under the Console's state, with its bytes.
    fn state_files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push((path.clone(), fs::read(&path).unwrap_or_default()));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.state, &mut out);
        out
    }
    fn park(&self, event: &str, group: usize, index: usize) -> super::changes::PlanView {
        self.service
            .plan(Operation::ParkHook, &self.hook(event, group, index))
            .unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn applied(world: &World, plan: &super::changes::PlanView) -> Outcome {
    world.service.apply(&plan.id).unwrap().outcome
}

// ---- acceptance -------------------------------------------------------------------------

#[test]
fn ac_f2a_01_a_hook_disabled_is_gone_from_the_file_and_from_the_inventory() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    assert_eq!(plan.risk, Risk::Normal);
    assert!(!plan.needs_native_dialog);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(!world.file().contains("stop.sh"));
    assert!(world.file().contains("guard.sh"));
    serde_json::from_str::<serde_json::Value>(&world.file()).unwrap();
    let hooks = world
        .inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .filter(|i| i.kind == ItemKind::Hook)
        .count();
    assert_eq!(hooks, 2);
}

#[test]
fn ac_f2a_02_restoring_a_hook_gives_back_the_file_byte_for_byte() {
    let world = World::new(SETTINGS);
    for (event, group, index) in [("Stop", 0, 0), ("PreToolUse", 0, 0), ("PreToolUse", 0, 1)] {
        let plan = world.park(event, group, index);
        assert_eq!(applied(&world, &plan), Outcome::Applied);
        let record = world
            .service
            .records()
            .into_iter()
            .find(|r| r.state == State::Parked)
            .unwrap();
        let back = world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap();
        // Putting a hook back executes code: always the native dialog.
        assert_eq!(back.risk, Risk::ExecutesCode);
        assert_eq!(applied(&world, &back), Outcome::Applied);
        assert_eq!(world.file(), SETTINGS, "{event} {group} {index}");
        let used = world
            .service
            .records()
            .into_iter()
            .find(|r| r.id == record.id)
            .unwrap();
        assert_eq!(used.state, State::Used);
    }
}

#[test]
fn ac_f2a_03_a_file_changed_between_plan_and_apply_is_not_written() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let outside = SETTINGS.replace("\"opus\"", "\"sonnet\"");
    fs::write(settings_path(&world.home), &outside).unwrap();
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(world.file(), outside, "the outside change must stay");
    assert!(
        world.service.records().is_empty(),
        "no record for a change that did not happen"
    );
    // And the plan is single use.
    assert_eq!(
        world.service.apply(&plan.id).unwrap_err(),
        ChangeFailure::PlanUnknown
    );
}

#[test]
fn ac_f2a_04_a_hook_that_can_block_asks_in_the_native_dialog_and_a_refusal_writes_nothing() {
    let world = World::new(SETTINGS);
    let plan = world.park("PreToolUse", 0, 0);
    assert_eq!(plan.risk, Risk::WeakensSecurity);
    assert!(plan.needs_native_dialog);
    world.confirm.accept.store(false, Ordering::SeqCst);
    assert_eq!(applied(&world, &plan), Outcome::RefusedByUser);
    assert_eq!(world.file(), SETTINGS);
    assert!(world.service.records().is_empty());
    let asked = world.confirm.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].risk, Risk::WeakensSecurity);
    assert_eq!(asked[0].source, SourceKey::ClaudeUserSettings);
    // The refusal is in the history, with no content.
    let (history, _) = world.service.history(None, 10);
    assert_eq!(history[0].outcome, Some(Outcome::RefusedByUser));
    // Accepting, on a new plan, writes.
    world.confirm.accept.store(true, Ordering::SeqCst);
    let plan = world.park("PreToolUse", 0, 0);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(!world.file().contains("guard.sh"));
}

#[test]
fn ac_f2a_05_the_history_shows_both_changes_and_holds_no_content() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    applied(&world, &plan);
    let record = world.service.records().remove(0);
    let back = world
        .service
        .plan(Operation::RestoreHook, &record.id)
        .unwrap();
    applied(&world, &back);
    let (history, next) = world.service.history(None, 10);
    assert_eq!(history.len(), 2);
    assert!(next.is_none());
    assert_eq!(history[0].operation, Operation::RestoreHook);
    assert_eq!(history[1].operation, Operation::ParkHook);
    assert!(history.iter().all(|c| c.outcome == Some(Outcome::Applied)));
    let journal = String::from_utf8(
        world
            .state_files()
            .into_iter()
            .find(|(p, _)| p.ends_with("journal.jsonl"))
            .unwrap()
            .1,
    )
    .unwrap();
    for content in ["stop.sh", "guard.sh", "command", "hooks"] {
        assert!(!journal.contains(content), "the journal holds {content}");
    }
}

#[test]
fn ac_f2a_06_no_store_holds_a_seeded_secret_in_the_clear() {
    let secret = format!(
        r#"{{ "hooks": {{ "Stop": [ {{ "hooks": [
          {{ "type": "command", "command": "notify --token {MARKER}" }},
          {{ "type": "http", "url": "https://u:{MARKER}@host/x",
             "headers": {{ "Authorization": "Bearer {MARKER}" }} }} ] }} ] }} }}"#
    );
    let world = World::new(&secret);
    for index in [0, 1] {
        let plan = world.park("Stop", 0, 0);
        assert!(plan.holds_hidden_values);
        assert_eq!(plan.storage, Some(Protection::Sealed));
        // Nothing the page is shown, or the native dialog is asked, carries it.
        assert!(!serde_json::to_string(&plan).unwrap().contains(MARKER));
        assert_eq!(applied(&world, &plan), Outcome::Applied, "{index}");
    }
    assert!(!world.file().contains(MARKER), "both hooks left the file");
    let key = world.keys.key_hex().unwrap();
    let files = world.state_files();
    assert!(
        files
            .iter()
            .any(|(p, _)| p.to_string_lossy().contains("restoration"))
    );
    for (path, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        assert!(
            !text.contains(MARKER),
            "{} holds the secret",
            path.display()
        );
        assert!(!text.contains(&key), "{} holds the key", path.display());
        let hex: String = MARKER.bytes().map(|b| format!("{b:02x}")).collect();
        assert!(
            !text.contains(&hex),
            "{} holds the secret in hex",
            path.display()
        );
    }
    for request in world.confirm.asked.lock().unwrap().iter() {
        assert!(!format!("{request:?}").contains(MARKER));
    }
    // And it still comes back.
    for record in world.service.records() {
        let back = world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap();
        assert!(!serde_json::to_string(&back).unwrap().contains(MARKER));
        assert_eq!(applied(&world, &back), Outcome::Applied);
    }
    assert!(world.file().contains(MARKER));
}

// ---- ADR 0034: the key store ------------------------------------------------------------

#[test]
fn without_a_key_store_an_entry_that_may_hold_a_secret_is_not_parked() {
    let secret = format!(
        r#"{{ "hooks": {{ "Stop": [ {{ "hooks": [
      {{ "type": "command", "command": "notify --token {MARKER}" }},
      {{ "type": "command", "command": "plain.sh" }} ] }} ] }} }}"#
    );
    let world = World::new(&secret);
    world.keys.set_unavailable(true);
    assert_eq!(
        world
            .service
            .plan(Operation::ParkHook, &world.hook("Stop", 0, 0))
            .unwrap_err(),
        ChangeFailure::KeyStoreUnavailable
    );
    assert_eq!(world.file(), secret);
    assert!(
        world
            .state_files()
            .iter()
            .all(|(_, b)| !String::from_utf8_lossy(b).contains(MARKER))
    );
    // The entry that holds no secret may be kept, and the plan says it will not be sealed.
    let plan = world.park("Stop", 0, 1);
    assert_eq!(plan.storage, Some(Protection::PlainNoSecret));
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(
        !world.keys.has_key(),
        "no key is made while the store cannot keep one"
    );
}

#[test]
fn a_plan_that_promised_sealing_never_falls_back_to_plain_at_apply() {
    let secret = format!(
        r#"{{ "hooks": {{ "Stop": [ {{ "hooks": [
      {{ "type": "command", "command": "notify --token {MARKER}" }} ] }} ] }} }}"#
    );
    let world = World::new(&secret);
    let plan = world.park("Stop", 0, 0);
    assert_eq!(plan.storage, Some(Protection::Sealed));
    world.keys.set_unavailable(true);
    assert_eq!(applied(&world, &plan), Outcome::KeyStoreUnavailable);
    assert_eq!(world.file(), secret);
    assert!(world.service.records().is_empty());
    assert!(
        world
            .state_files()
            .iter()
            .all(|(_, b)| !String::from_utf8_lossy(b).contains(MARKER))
    );
}

#[test]
fn a_lost_key_is_said_and_the_record_is_not_pretended_to_be_readable() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    applied(&world, &plan);
    world.keys.forget_for_test();
    let record = world.service.records().remove(0);
    assert_eq!(
        world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap_err(),
        ChangeFailure::KeyLost
    );
    // The journal still says what was done, without the content.
    assert_eq!(world.service.history(None, 5).0.len(), 1);
    // The record can be deleted, and the file is not touched by trying.
    world.service.delete_record(&record.id).unwrap();
    assert!(world.service.records().is_empty());
    assert!(!world.file().contains("stop.sh"));
}

#[test]
fn a_record_that_was_altered_on_disk_is_refused_not_read() {
    let world = World::new(SETTINGS);
    applied(&world, &world.park("Stop", 0, 0));
    let record = world.service.records().remove(0);
    let path = world
        .state
        .join("restoration")
        .join(format!("{}.rec", record.id));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let ct = value["sealed"]["ct"].as_str().unwrap().to_owned();
    value["sealed"]["ct"] = serde_json::json!(format!(
        "{}{}",
        if ct.starts_with('0') { '1' } else { '0' },
        &ct[1..]
    ));
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap_err(),
        ChangeFailure::RecordUnreadable
    );
}

#[test]
fn a_record_exported_with_a_passphrase_restores_on_another_store() {
    let secret = format!(
        r#"{{ "hooks": {{ "Stop": [ {{ "hooks": [
      {{ "type": "command", "command": "notify --token {MARKER}" }},
      {{ "type": "command", "command": "other.sh" }} ] }} ] }} }}"#
    );
    let one = World::new(&secret);
    applied(&one, &one.park("Stop", 0, 0));
    let record = one.service.records().remove(0);
    let file = one
        .service
        .export_record(&record.id, "correct horse")
        .unwrap();
    assert!(!String::from_utf8_lossy(&file).contains(MARKER));
    assert!(!String::from_utf8_lossy(&file).contains("notify"));
    // Another computer: its own world, its own key.
    let two = World::new(&secret.replace("notify", "x"));
    assert_ne!(two.keys.key_hex(), one.keys.key_hex());
    assert_eq!(
        two.service.import_record(&file, "wrong").unwrap_err(),
        ChangeFailure::ImportInvalid
    );
    assert_eq!(
        two.service
            .import_record(b"{}", "correct horse")
            .unwrap_err(),
        ChangeFailure::ImportInvalid
    );
    let imported = two.service.import_record(&file, "correct horse").unwrap();
    assert!(
        two.service
            .records()
            .iter()
            .any(|r| r.id == imported && r.secret_bearing)
    );
    // An entry that may hold a secret does not import where there is no key store.
    two.keys.set_unavailable(true);
    assert_eq!(
        two.service
            .import_record(&file, "correct horse")
            .unwrap_err(),
        ChangeFailure::KeyStoreUnavailable
    );
}

// ---- the other operations ---------------------------------------------------------------

#[test]
fn undo_puts_back_what_a_change_took_and_cannot_be_undone_twice() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let change = world.service.apply(&plan.id).unwrap().change;
    let undo = world.service.undo(&change).unwrap();
    assert_eq!(undo.operation, Operation::RestoreHook);
    assert_eq!(undo.undoes.as_deref(), Some(change.as_str()));
    assert_eq!(applied(&world, &undo), Outcome::Applied);
    assert_eq!(world.file(), SETTINGS);
    assert_eq!(
        world.service.undo(&change).unwrap_err(),
        ChangeFailure::NotUndoable
    );
    // An undo is not itself undone: the way back is the plan the user makes.
    let (history, _) = world.service.history(None, 10);
    assert_eq!(
        world.service.undo(&history[0].id).unwrap_err(),
        ChangeFailure::NotUndoable
    );
}

#[test]
fn all_hooks_off_and_on_again_by_the_providers_own_switch() {
    let world = World::new(SETTINGS);
    let plan = world
        .service
        .plan(Operation::DisableAllHooks, "claude_user_settings")
        .unwrap();
    assert_eq!(plan.risk, Risk::WeakensSecurity);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert_eq!(
        world.file(),
        SETTINGS.replace("\n  }\n}\n", "\n  },\n  \"disableAllHooks\": true\n}\n")
    );
    let back = world
        .service
        .plan(Operation::EnableAllHooks, "claude_user_settings")
        .unwrap();
    assert_eq!(applied(&world, &back), Outcome::Applied);
    assert_eq!(world.file(), SETTINGS);
    // Nothing was kept for it: the provider's own switch is the storage.
    assert!(world.service.records().is_empty());
}

#[test]
fn a_skill_is_switched_off_and_on_in_the_settings() {
    let world = World::new(SETTINGS);
    let skill = world
        .inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Skill)
        .unwrap()
        .id
        .clone();
    let plan = world.service.plan(Operation::SkillOff, &skill).unwrap();
    assert_eq!(plan.risk, Risk::Normal);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(world.file().contains("\"noisy\": \"off\""));
    let state = world.inventory.snapshot();
    let off = state
        .inventory
        .items
        .iter()
        .find(|i| i.id == skill)
        .unwrap();
    assert_eq!(off.persisted, super::model::Persisted::PresentDisabled);
    let on = world.service.plan(Operation::SkillOn, &skill).unwrap();
    assert_eq!(applied(&world, &on), Outcome::Applied);
    assert!(!world.file().contains("noisy"));
}

// ---- where a change may go --------------------------------------------------------------

#[test]
fn what_is_not_this_users_settings_is_never_planned() {
    let world = World::new(SETTINGS);
    for source in [
        "claude_user_state",
        "claude_managed_settings",
        "codex_user_config",
        "nonsense",
    ] {
        assert!(
            world
                .service
                .plan(Operation::DisableAllHooks, source)
                .is_err(),
            "{source}"
        );
    }
    // A group-writable provider directory is read and never written.
    super::acl::share_with_a_group_for_test(&world.home.join(".claude"));
    assert_eq!(
        world
            .service
            .plan(Operation::ParkHook, &world.hook("Stop", 0, 0))
            .unwrap_err(),
        ChangeFailure::NotWritable
    );
    fs::set_permissions(
        world.home.join(".claude"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    assert!(
        world
            .service
            .plan(Operation::ParkHook, &world.hook("Stop", 0, 0))
            .is_ok()
    );
}

#[test]
fn a_settings_link_is_written_through_and_stays_a_link() {
    let world = World::new(SETTINGS);
    let real = world.home.join(".claude/real-settings.json");
    fs::rename(settings_path(&world.home), &real).unwrap();
    symlink("real-settings.json", settings_path(&world.home)).unwrap();
    world.inventory.rescan();
    let plan = world.park("Stop", 0, 0);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(
        fs::symlink_metadata(settings_path(&world.home))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(!fs::read_to_string(&real).unwrap().contains("stop.sh"));
}

#[test]
fn the_new_file_keeps_the_old_ones_mode() {
    let world = World::new(SETTINGS);
    fs::set_permissions(
        settings_path(&world.home),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    applied(&world, &world.park("Stop", 0, 0));
    let mode = fs::metadata(settings_path(&world.home))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o640);
}

#[test]
fn a_conflict_copy_beside_the_file_stops_the_change() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    fs::write(
        world
            .home
            .join(".claude/settings.sync-conflict-20261018-120000-ABCDEFG.json"),
        "{}",
    )
    .unwrap();
    assert_eq!(applied(&world, &plan), Outcome::ConflictCopyPresent);
    assert_eq!(world.file(), SETTINGS);
}

#[test]
fn a_plan_expires_and_no_more_than_eight_are_kept() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    world.service.age_plans(std::time::Duration::from_secs(61));
    assert_eq!(applied(&world, &plan), Outcome::Expired);
    assert_eq!(world.file(), SETTINGS);
    let first = world.park("Stop", 0, 0);
    for _ in 0..8 {
        world.park("Stop", 0, 0);
    }
    assert_eq!(
        world.service.apply(&first.id).unwrap_err(),
        ChangeFailure::PlanUnknown
    );
}

#[test]
fn a_plan_names_only_what_the_service_knows() {
    let world = World::new(SETTINGS);
    for target in [
        "",
        "../../etc/passwd",
        "/home/x/.claude/settings.json",
        &"f".repeat(64),
    ] {
        assert!(world.service.plan(Operation::ParkHook, target).is_err());
        assert!(world.service.plan(Operation::RestoreHook, target).is_err());
        assert!(world.service.plan(Operation::SkillOff, target).is_err());
    }
    assert_eq!(
        world.service.apply("nope").unwrap_err(),
        ChangeFailure::PlanUnknown
    );
    assert_eq!(
        world.service.undo("nope").unwrap_err(),
        ChangeFailure::NotUndoable
    );
}

// ---- failure injection ------------------------------------------------------------------

const STEPS: [Step; 11] = [
    Step::IntentJournaled,
    Step::RecordSealed,
    Step::RecordWritten,
    Step::TemporaryCreated,
    Step::TemporaryWritten,
    Step::TemporaryFlushed,
    Step::Renamed,
    Step::Exchanged,
    Step::DirectoryFlushed,
    Step::ReadBack,
    Step::OutcomeJournaled,
];

/// The invariants after any fault and a restart: the file is the original or the plan, whole;
/// nothing is left half done; and a hook out of the file is a hook that can be put back.
fn assert_consistent(world: &World, planned: &str, step: Step, fault: Fault) {
    let again = world.restarted();
    again.service.recover();
    let file = again.file();
    let out = file == planned;
    assert!(
        file == SETTINGS || out,
        "{step:?} {fault:?}: the file is neither: {file}"
    );
    assert!(
        again.leftovers().is_empty(),
        "{step:?} {fault:?}: leftovers {:?}",
        again.leftovers()
    );
    let (history, _) = again.service.history(None, 50);
    assert!(
        history.iter().all(|c| c.outcome.is_some()),
        "{step:?} {fault:?}: an apply with no end was left"
    );
    let parked: Vec<_> = again
        .service
        .records()
        .into_iter()
        .filter(|r| r.state == State::Parked)
        .collect();
    if out {
        assert_eq!(
            parked.len(),
            1,
            "{step:?} {fault:?}: the hook left and was not kept"
        );
        let back = again
            .service
            .plan(Operation::RestoreHook, &parked[0].id)
            .unwrap();
        assert_eq!(applied(&again, &back), Outcome::Applied);
        assert_eq!(
            again.file(),
            SETTINGS,
            "{step:?} {fault:?}: not restored exactly"
        );
    } else {
        assert!(
            parked.is_empty(),
            "{step:?} {fault:?}: a record for a hook that never left"
        );
    }
}

#[test]
fn a_failure_at_any_step_leaves_the_file_whole_and_the_hook_recoverable() {
    for step in STEPS {
        let world = World::new(SETTINGS);
        let plan = world.park("Stop", 0, 0);
        let planned = SETTINGS.replace(
            "    \"Stop\": [\n      { \"hooks\": [ { \"type\": \"command\", \"command\": \"stop.sh\" } ] }\n    ]\n",
            "",
        );
        let _ = planned;
        world.faults.arm(step, Fault::Fail);
        let result = world.service.apply(&plan.id);
        let outcome = result.as_ref().map(|a| a.outcome).ok();
        // A refusal of the disk is an outcome, never a panic or a half file.
        assert!(result.is_ok(), "{step:?}: {result:?}");
        let file = world.file();
        serde_json::from_str::<serde_json::Value>(&file).unwrap();
        let removed = !file.contains("stop.sh");
        match step {
            Step::IntentJournaled => assert_eq!(outcome, Some(Outcome::NotRecorded)),
            Step::RecordSealed | Step::RecordWritten => {
                assert_eq!(outcome, Some(Outcome::WriteFailed))
            }
            Step::TemporaryCreated
            | Step::TemporaryWritten
            | Step::TemporaryFlushed
            | Step::Renamed => {
                assert_eq!(outcome, Some(Outcome::WriteFailed), "{step:?}");
            }
            Step::Exchanged | Step::DirectoryFlushed | Step::OutcomeJournaled => {
                assert_eq!(outcome, Some(Outcome::Applied))
            }
            Step::ReadBack => assert_eq!(outcome, Some(Outcome::VerifyFailed)),
            Step::RecordMarked | Step::SwappedBack => (),
        }
        assert_eq!(
            removed,
            outcome.is_some_and(Outcome::changed_the_file),
            "{step:?}: file and outcome disagree"
        );
        assert!(
            world.leftovers().is_empty(),
            "{step:?}: a failure must clean up its temporary file"
        );
        // A disk that refused leaves no record behind for a hook that did not leave.
        let parked = world
            .service
            .records()
            .into_iter()
            .filter(|r| r.state == State::Parked)
            .count();
        assert_eq!(
            parked,
            usize::from(removed),
            "{step:?}: records and file disagree"
        );
        // And the world after a restart is consistent too.
        let planned_file = if removed {
            world.file()
        } else {
            "(not applied)".to_owned()
        };
        assert_consistent(&world, &planned_file, step, Fault::Fail);
    }
}

#[test]
fn a_crash_at_any_step_is_repaired_at_the_next_start() {
    for step in STEPS {
        let world = World::new(SETTINGS);
        let plan = world.park("Stop", 0, 0);
        // What the plan writes, learned from a world that was not crashed.
        let twin = World::new(SETTINGS);
        let twin_plan = twin.park("Stop", 0, 0);
        applied(&twin, &twin_plan);
        let planned = twin.file();
        world.faults.arm(step, Fault::Crash);
        let result = world.service.apply(&plan.id);
        assert!(result.is_err(), "{step:?}: a crash is not an outcome");
        assert_consistent(&world, &planned, step, Fault::Crash);
    }
}

#[test]
fn a_torn_last_line_of_the_journal_is_skipped_and_the_rest_is_read() {
    let world = World::new(SETTINGS);
    applied(&world, &world.park("Stop", 0, 0));
    let journal = world.state.join("journal/journal.jsonl");
    let mut text = fs::read(&journal).unwrap();
    text.extend_from_slice(b"{\"v\":1,\"id\":\"abc\",\"at\":1,\"pha");
    fs::write(&journal, text).unwrap();
    let (history, _) = world.service.history(None, 10);
    assert_eq!(history.len(), 1);
    // And an apply still journals after it.
    let plan = world.park("PreToolUse", 0, 1);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
}

#[test]
fn a_change_made_by_a_second_window_waits_for_the_first_and_then_sees_the_file_as_it_is() {
    let world = World::new(SETTINGS);
    let first = world.park("Stop", 0, 0);
    let second = world.park("PreToolUse", 0, 1);
    assert_eq!(applied(&world, &first), Outcome::Applied);
    // The second plan was made on the file as it was: it is stale, not applied blindly.
    assert_eq!(applied(&world, &second), Outcome::ChangedOutside);
    let fresh = world.park("PreToolUse", 0, 1);
    assert_eq!(applied(&world, &fresh), Outcome::Applied);
}

// ---- the contract with the window -------------------------------------------------------

/// Random identifiers and times are replaced by stable stand-ins, so that the shape and the
/// words are compared and nothing else.
fn steady(value: &mut serde_json::Value, seen: &mut Vec<String>) {
    use serde_json::Value;
    match value {
        Value::String(text) if text.len() == 32 && text.bytes().all(|b| b.is_ascii_hexdigit()) => {
            let index = seen.iter().position(|s| s == text).unwrap_or_else(|| {
                seen.push(text.clone());
                seen.len() - 1
            });
            *text = format!("{:032x}", index + 1);
        }
        Value::Array(items) => items.iter_mut().for_each(|v| steady(v, seen)),
        Value::Object(entries) => {
            for (key, inner) in entries.iter_mut() {
                match key.as_str() {
                    "at" | "created" | "revision" => *inner = serde_json::json!(0),
                    "expires_in_seconds" => *inner = serde_json::json!(60),
                    _ => steady(inner, seen),
                }
            }
        }
        _ => (),
    }
}

/// The replies of the change commands, as one file both sides read:
/// `tests/fixtures/configuration-changes-v1.json`. `WYSS_UPDATE_FIXTURES=1` writes it again.
#[test]
fn the_change_replies_are_exactly_the_shared_fixture() {
    let world = World::new(SETTINGS);
    let plan = world.park("PreToolUse", 0, 0);
    let first = world.service.apply(&plan.id).unwrap();
    let second_plan = world.park("PreToolUse", 0, 0);
    world.confirm.accept.store(false, Ordering::SeqCst);
    let refused = world.service.apply(&second_plan.id).unwrap();
    world.confirm.accept.store(true, Ordering::SeqCst);
    let stop = world.park("Stop", 0, 0);
    let stop_applied = world.service.apply(&stop.id).unwrap();
    let all = world
        .service
        .plan(Operation::DisableAllHooks, "claude_user_settings")
        .unwrap();
    let skill = world
        .inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| i.kind == ItemKind::Skill)
        .unwrap()
        .id
        .clone();
    let skill_plan = world.service.plan(Operation::SkillOff, &skill).unwrap();
    let (changes, next) = world.service.history(None, 20);
    let stop_record = world
        .service
        .records()
        .into_iter()
        .find(|r| r.label.starts_with("Stop"))
        .unwrap();
    let restore = world
        .service
        .plan(Operation::RestoreHook, &stop_record.id)
        .unwrap();
    world.codex(CODEX);
    let codex_plan = world
        .service
        .plan(
            Operation::CodexMcpOff,
            &world.codex_item(ItemKind::McpServer, "wyss_session"),
        )
        .unwrap();
    let codex_skill_plan = world
        .service
        .plan(
            Operation::CodexSkillOn,
            &world.codex_item(ItemKind::Skill, "alpha"),
        )
        .unwrap();
    let mut records = world.service.records();
    records.sort_by(|a, b| a.label.cmp(&b.label));
    let mut value = serde_json::json!({
        "plan_park_blocking": plan,
        "applied": first,
        "applied_refused": refused,
        "applied_stop": stop_applied,
        "plan_all_hooks": all,
        "plan_skill": skill_plan,
        "plan_restore": restore,
        "plan_codex_mcp": codex_plan,
        "plan_codex_skill": codex_skill_plan,
        "history": { "changes": changes, "next": next },
        "records": records,
        "done": { "done": true },
    });
    let text = serde_json::to_string(&value).unwrap();
    assert!(!text.contains(world.home.to_str().unwrap()));
    steady(&mut value, &mut Vec::new());
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures/configuration-changes-v1.json");
    if std::env::var_os("WYSS_UPDATE_FIXTURES").is_some() {
        fs::write(&path, serde_json::to_string_pretty(&value).unwrap() + "\n").unwrap();
    }
    let expected: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        value, expected,
        "the contract changed: update the fixture on purpose"
    );
}

// ---- review finding: a provider write in the instant before the swap --------------------

#[test]
fn a_provider_write_between_the_last_look_and_the_swap_is_found_and_put_back() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let path = settings_path(&world.home);
    let theirs = SETTINGS.replace("\"opus\"", "\"written-by-the-provider\"");
    let (target, text) = (path.clone(), theirs.clone());
    // The provider replaces its file as editors do: a new file renamed over the name.
    super::writer::seam::on_before_swap(move || {
        let beside = target.with_extension("provider-tmp");
        fs::write(&beside, &text).unwrap();
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&beside, &target).unwrap();
    });
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(
        world.file(),
        theirs,
        "the provider's version must be the one in place"
    );
    assert!(
        world.service.records().is_empty(),
        "no record for a hook that did not leave"
    );
    assert!(world.leftovers().is_empty());
}

#[test]
fn a_provider_that_appends_in_place_in_that_instant_is_found_too() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let path = settings_path(&world.home);
    let theirs = format!("{SETTINGS}  ");
    let (target, text) = (path.clone(), theirs.clone());
    super::writer::seam::on_before_swap(move || fs::write(&target, &text).unwrap());
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(world.file(), theirs);
    assert!(world.service.records().is_empty());
}

// ---- against the installed Claude Code --------------------------------------------------

/// Runs the installed `claude` once in `project` with `CLAUDE_CONFIG_DIR` pointed at the
/// temporary configuration and no credentials of any kind (it answers "Not logged in"; hooks
/// and skill checks happen before that). Returns what it printed.
fn claude(world: &World, project: &Path, prompt: &str) -> String {
    let output = std::process::Command::new("claude")
        .args(["-p", prompt])
        .current_dir(project)
        .env_clear()
        .envs(
            ["PATH", "HOME"]
                .iter()
                .filter_map(|k| std::env::var_os(k).map(|v| (*k, v))),
        )
        .env("CLAUDE_CONFIG_DIR", world.home.join(".claude"))
        .stdin(std::process::Stdio::null())
        .output()
        .expect("claude is installed");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Turning a hook off and back on, and a skill off and on, against the real program, in a
/// temporary configuration: a hook runs or does not, a skill runs or is refused. By hand:
/// `cargo test -- --ignored real_claude_code --nocapture`.
#[test]
#[ignore = "runs the installed Claude Code, in a temporary configuration"]
fn real_claude_code_obeys_what_was_written() {
    let marker = |name: &str| {
        let base = std::env::temp_dir();
        base.join(format!(
            "wyss-marker-{name}-{}",
            uuid::Uuid::new_v4().simple()
        ))
    };
    let (session, prompt) = (marker("session"), marker("prompt"));
    let settings = format!(
        r#"{{
  "hooks": {{
    "SessionStart": [
      {{ "hooks": [ {{ "type": "command", "command": "touch {session}" }} ] }}
    ],
    "UserPromptSubmit": [
      {{ "hooks": [ {{ "type": "command", "command": "touch {prompt}" }} ] }}
    ]
  }}
}}
"#,
        session = session.display(),
        prompt = prompt.display()
    );
    let world = World::new(&settings);
    let project = world.base.join("project");
    fs::create_dir_all(&project).unwrap();
    for name in ["useful", "noisy"] {
        let dir = world.home.join(format!(".claude/skills/{name}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test skill {name}\n---\nbody\n"),
        )
        .unwrap();
    }
    world.inventory.rescan();
    let ran = |p: &Path| p.exists();
    let reset = || {
        let _ = fs::remove_file(&session);
        let _ = fs::remove_file(&prompt);
    };

    // Baseline: both hooks run.
    claude(&world, &project, "hello");
    assert!(
        ran(&session) && ran(&prompt),
        "the fixture hooks must run to begin with"
    );

    // Turn both off through the service: neither runs.
    for event in ["SessionStart", "UserPromptSubmit"] {
        let plan = world.park(event, 0, 0);
        assert_eq!(applied(&world, &plan), Outcome::Applied);
    }
    reset();
    claude(&world, &project, "hello");
    assert!(
        !ran(&session) && !ran(&prompt),
        "a hook that was turned off still ran"
    );

    // Put both back: the file is the original and both run again.
    for record in world
        .service
        .records()
        .into_iter()
        .filter(|r| r.state == State::Parked)
    {
        let back = world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap();
        assert_eq!(applied(&world, &back), Outcome::Applied);
    }
    assert_eq!(
        world.file(),
        settings,
        "byte for byte, as Claude Code will read it"
    );
    reset();
    claude(&world, &project, "hello");
    assert!(
        ran(&session) && ran(&prompt),
        "a hook that was put back did not run"
    );

    // Skills: on, then refused after the switch, then on again.
    let skill_id = |name: &str| {
        world
            .inventory
            .snapshot()
            .inventory
            .items
            .iter()
            .find(|i| i.kind == ItemKind::Skill && i.name.as_str() == name)
            .unwrap()
            .id
            .clone()
    };
    assert!(!claude(&world, &project, "/noisy").contains("disabled via skillOverrides"));
    let off = world
        .service
        .plan(Operation::SkillOff, &skill_id("noisy"))
        .unwrap();
    assert_eq!(applied(&world, &off), Outcome::Applied);
    assert!(claude(&world, &project, "/noisy").contains("disabled via skillOverrides"));
    assert!(
        !claude(&world, &project, "/useful").contains("disabled via skillOverrides"),
        "only the skill that was switched off is refused"
    );
    let on = world
        .service
        .plan(Operation::SkillOn, &skill_id("noisy"))
        .unwrap();
    assert_eq!(applied(&world, &on), Outcome::Applied);
    assert!(!claude(&world, &project, "/noisy").contains("disabled via skillOverrides"));
    // The file Claude Code reads is valid to it: an unreadable settings file would say so.
    assert!(!claude(&world, &project, "hello").contains("did not load"));
}

// ---- recovery never overwrites a newer provider version -----------------------------------

/// The provider writes in the instant before the swap, and the process dies right after it:
/// the provider's newer file is at the temporary name and ours is in place.
#[test]
fn a_crash_after_displacing_a_newer_provider_file_puts_that_file_back_at_start() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let path = settings_path(&world.home);
    let theirs = SETTINGS.replace("\"opus\"", "\"written-by-the-provider\"");
    let (target, text) = (path.clone(), theirs.clone());
    super::writer::seam::on_before_swap(move || {
        let beside = target.with_extension("provider-tmp");
        fs::write(&beside, &text).unwrap();
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&beside, &target).unwrap();
    });
    world.faults.arm(Step::Exchanged, Fault::Crash);
    assert!(world.service.apply(&plan.id).is_err());
    // Dead here: ours is in place, theirs is at the temporary name.
    assert!(!world.file().contains("stop.sh"));
    assert_eq!(world.leftovers().len(), 1);

    let again = world.restarted();
    again.service.recover();
    assert_eq!(
        again.file(),
        theirs,
        "the provider's newer version must be the one in place"
    );
    assert!(again.leftovers().is_empty());
    let (history, _) = again.service.history(None, 5);
    assert_eq!(history[0].outcome, Some(Outcome::RecoveredProviderKept));
    assert!(
        again.service.records().is_empty(),
        "no record for a hook that did not leave"
    );
}

#[test]
fn a_swap_that_cannot_be_undone_falls_back_to_the_plain_rename_and_loses_nothing() {
    let world = World::new(SETTINGS);
    let plan = world.park("Stop", 0, 0);
    let path = settings_path(&world.home);
    let theirs = format!("{SETTINGS}  ");
    let (target, text) = (path.clone(), theirs.clone());
    super::writer::seam::on_before_swap(move || fs::write(&target, &text).unwrap());
    world.faults.arm(Step::SwappedBack, Fault::Fail);
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(world.file(), theirs);
    assert!(world.leftovers().is_empty());
}

#[test]
fn recovery_keeps_a_temporary_file_it_cannot_account_for() {
    let world = World::new(SETTINGS);
    // Something with the temporary pattern, and no intent that explains it.
    let stray = world
        .home
        .join(".claude/.settings.json.0123456789abcdef.tmp");
    fs::write(&stray, "somebody's work").unwrap();
    let again = world.restarted();
    again.service.recover();
    assert!(!stray.exists());
    let kept: Vec<_> = fs::read_dir(world.home.join(".claude"))
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("settings.json.recovered-")
        })
        .collect();
    assert_eq!(kept.len(), 1, "it is renamed, not deleted");
    assert_eq!(
        fs::read_to_string(kept[0].path()).unwrap(),
        "somebody's work"
    );
}

// ---- F2b: Codex skill and MCP switches ----------------------------------------------------

const CODEX: &str = r#"# Codex configuration — hand written
model = "gpt-6"   # keep

[features]
hooks = true   # pinned by policy too

# Skills the owner switched
[[skills.config]]
path = "/home/u/.codex/skills/alpha/SKILL.md"
enabled = false   # parked for now

[[skills.config]]
path = "/home/u/.codex/skills/beta"

[mcp_servers.wyss_session]
command = "wyss"
args = [
  "serve",   # comment inside
]

[mcp_servers.web]
url = "https://example.invalid"
enabled = false
"#;

fn codex_path(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}

impl World {
    fn codex(&self, text: &str) {
        let path = codex_path(&self.home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        self.inventory.rescan();
    }
    fn codex_file(&self) -> String {
        fs::read_to_string(codex_path(&self.home)).unwrap()
    }
    fn codex_item(&self, kind: ItemKind, name_part: &str) -> String {
        self.inventory
            .snapshot()
            .inventory
            .items
            .iter()
            .find(|i| {
                i.provider == super::model::ProviderId::Codex
                    && i.kind == kind
                    && i.name.as_str().contains(name_part)
            })
            .unwrap_or_else(|| panic!("no codex {kind:?} {name_part}"))
            .id
            .clone()
    }
}

#[test]
fn a_codex_mcp_server_is_switched_off_and_on_by_its_enabled_key_alone() {
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    let id = world.codex_item(ItemKind::McpServer, "wyss_session");
    let plan = world.service.plan(Operation::CodexMcpOff, &id).unwrap();
    assert_eq!(plan.risk, Risk::Normal);
    assert!(!plan.needs_native_dialog);
    assert_eq!(plan.effect, super::changes::Effect::CodexMcpChecked);
    // The prediction is labelled as one, and says managed Inference is unaffected.
    let managed = plan.managed_effect.clone().unwrap();
    assert!(managed.predicted);
    assert_eq!(managed.before, managed.after);
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert_eq!(
        world.codex_file(),
        CODEX.replace(
            "  \"serve\",   # comment inside\n]\n",
            "  \"serve\",   # comment inside\n]\nenabled = false\n"
        )
    );
    // Seen by the inventory as off.
    let state = world.inventory.snapshot();
    let item = state.inventory.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(item.persisted, super::model::Persisted::PresentDisabled);
    // Putting it back to on starts a program: the native dialog.
    let on = world.service.plan(Operation::CodexMcpOn, &id).unwrap();
    assert_eq!(on.risk, Risk::ExecutesCode);
    assert!(on.needs_native_dialog);
    assert_eq!(applied(&world, &on), Outcome::Applied);
    assert!(world.codex_file().contains("enabled = true"));
    assert_eq!(world.confirm.asked.lock().unwrap().len(), 1);
}

#[test]
fn a_codex_skill_is_switched_and_every_comment_survives() {
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    let alpha = world.codex_item(ItemKind::Skill, "alpha");
    let beta = world.codex_item(ItemKind::Skill, "beta");
    let on = world.service.plan(Operation::CodexSkillOn, &alpha).unwrap();
    assert_eq!(on.risk, Risk::Normal);
    assert_eq!(applied(&world, &on), Outcome::Applied);
    let off = world.service.plan(Operation::CodexSkillOff, &beta).unwrap();
    assert_eq!(applied(&world, &off), Outcome::Applied);
    let after = world.codex_file();
    for line in CODEX
        .lines()
        .filter(|l| l.trim_start().starts_with('#') || l.is_empty())
    {
        assert!(after.lines().any(|l| l == line), "lost {line:?}");
    }
    assert!(after.contains("enabled = true   # parked for now"));
    assert!(after.contains("path = \"/home/u/.codex/skills/beta\"\nenabled = false\n"));
}

#[test]
fn undoing_a_codex_switch_gives_the_file_back_byte_for_byte_whether_the_line_was_added_or_replaced()
{
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    for (op, kind, part) in [
        (Operation::CodexMcpOff, ItemKind::McpServer, "wyss_session"), // a line is added
        (Operation::CodexSkillOn, ItemKind::Skill, "alpha"),           // a value is replaced
        (Operation::CodexSkillOff, ItemKind::Skill, "beta"),           // a line is added
    ] {
        let id = world.codex_item(kind, part);
        let plan = world.service.plan(op, &id).unwrap();
        let change = world.service.apply(&plan.id).unwrap().change;
        assert_ne!(world.codex_file(), CODEX);
        let undo = world.service.undo(&change).unwrap();
        assert_eq!(undo.undoes.as_deref(), Some(change.as_str()));
        assert_eq!(applied(&world, &undo), Outcome::Applied);
        assert_eq!(world.codex_file(), CODEX, "{op:?}");
        // Not twice.
        assert_eq!(
            world.service.undo(&change).unwrap_err(),
            ChangeFailure::NotUndoable
        );
    }
    // The journal kept a closed word for each, and no key, path or value.
    let journal = String::from_utf8(
        world
            .state_files()
            .into_iter()
            .find(|(p, _)| p.ends_with("journal.jsonl"))
            .unwrap()
            .1,
    )
    .unwrap();
    assert!(journal.contains("\"prior\":\"absent\"") && journal.contains("\"prior\":\"disabled\""));
    for content in ["wyss_session", "alpha", "beta", "skills", "example.invalid"] {
        assert!(!journal.contains(content), "the journal holds {content}");
    }
}

#[test]
fn the_managed_inference_prediction_names_a_hook_cause_and_never_blocks_the_switch() {
    let world = World::new(SETTINGS);
    world.codex(&format!(
        "{CODEX}\n[[hooks.Stop]]\n[[hooks.Stop.hooks]]\ntype = \"command\"\ncommand = \"x\"\n"
    ));
    let id = world.codex_item(ItemKind::McpServer, "wyss_session");
    let plan = world.service.plan(Operation::CodexMcpOff, &id).unwrap();
    let managed = plan.managed_effect.unwrap();
    assert_eq!(managed.before, super::codex_plan::Verdict::ExternalHooks);
    assert_eq!(managed.after, super::codex_plan::Verdict::ExternalHooks);
    assert!(managed.predicted);
}

#[test]
fn an_undo_does_not_overwrite_an_entry_somebody_changed_after_the_switch() {
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    let id = world.codex_item(ItemKind::Skill, "beta");
    let plan = world.service.plan(Operation::CodexSkillOff, &id).unwrap();
    let change = world.service.apply(&plan.id).unwrap().change;
    // Somebody sets it to something else by hand: the entry is no longer what the switch left.
    let switched = world.codex_file();
    let by_hand = switched.replacen("beta\"\nenabled = false", "beta\"\nenabled = true", 1);
    assert_ne!(by_hand, switched);
    world.codex(&by_hand);
    assert_eq!(
        world.service.undo(&change).unwrap_err(),
        ChangeFailure::EntryChanged
    );
    // Or removes the line: the file already is what the undo would make.
    world.codex(&switched.replacen("beta\"\nenabled = false\n", "beta\"\n", 1));
    assert_eq!(
        world.service.undo(&change).unwrap_err(),
        ChangeFailure::AlreadyDone
    );
}

#[test]
fn what_codex_plans_refuse_is_refused_in_words() {
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    let id = world.codex_item(ItemKind::McpServer, "web");
    // Already off.
    assert_eq!(
        world.service.plan(Operation::CodexMcpOff, &id).unwrap_err(),
        ChangeFailure::AlreadyDone
    );
    // A skill id is not a server's.
    let skill = world.codex_item(ItemKind::Skill, "beta");
    assert_eq!(
        world
            .service
            .plan(Operation::CodexMcpOff, &skill)
            .unwrap_err(),
        ChangeFailure::NotSupported
    );
    // A Claude hook is not a Codex switch.
    assert_eq!(
        world
            .service
            .plan(Operation::CodexSkillOff, &world.hook("Stop", 0, 0))
            .unwrap_err(),
        ChangeFailure::NotSupported
    );
    // A shared provider directory is read and never written.
    super::acl::share_with_a_group_for_test(&world.home.join(".codex"));
    let beta = world.codex_item(ItemKind::Skill, "beta");
    assert_eq!(
        world
            .service
            .plan(Operation::CodexSkillOff, &beta)
            .unwrap_err(),
        ChangeFailure::NotWritable
    );
}

#[test]
fn codex_rewriting_its_own_file_in_the_instant_before_the_swap_is_never_overwritten() {
    let world = World::new(SETTINGS);
    world.codex(CODEX);
    let id = world.codex_item(ItemKind::McpServer, "wyss_session");
    let plan = world.service.plan(Operation::CodexMcpOff, &id).unwrap();
    let path = codex_path(&world.home);
    let theirs = format!("{CODEX}\n[projects.\"/x\"]\ntrust_level = \"trusted\"\n");
    let (target, text) = (path.clone(), theirs.clone());
    super::writer::seam::on_before_swap(move || {
        let beside = target.with_extension("codex-tmp");
        fs::write(&beside, &text).unwrap();
        fs::set_permissions(&beside, fs::Permissions::from_mode(0o600)).unwrap();
        fs::rename(&beside, &target).unwrap();
    });
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(world.codex_file(), theirs);
}

/// Every step of an apply to Codex's file, failed and crashed: the file is the original or the
/// plan, whole; nothing is left half done; the switch can be undone to the exact original.
#[test]
fn a_failure_or_a_crash_at_any_step_of_a_codex_switch_is_repaired_and_undoable() {
    let steps = [
        Step::IntentJournaled,
        Step::TemporaryCreated,
        Step::TemporaryWritten,
        Step::TemporaryFlushed,
        Step::Renamed,
        Step::Exchanged,
        Step::DirectoryFlushed,
        Step::ReadBack,
        Step::OutcomeJournaled,
    ];
    for step in steps {
        for fault in [Fault::Fail, Fault::Crash] {
            let world = World::new(SETTINGS);
            world.codex(CODEX);
            let id = world.codex_item(ItemKind::McpServer, "wyss_session");
            let plan = world.service.plan(Operation::CodexMcpOff, &id).unwrap();
            world.faults.arm(step, fault);
            let result = world.service.apply(&plan.id);
            if fault == Fault::Crash {
                assert!(result.is_err(), "{step:?}: a crash is not an outcome");
            }
            let again = world.restarted();
            again.service.recover();
            let file = again.codex_file();
            let changed = file != CODEX;
            serde_json::to_value(toml::from_str::<toml::Value>(&file).unwrap()).unwrap();
            assert!(
                !changed || file.contains("enabled = false"),
                "{step:?} {fault:?}: the file is neither version: {file}"
            );
            assert!(
                fs::read_dir(again.home.join(".codex"))
                    .unwrap()
                    .flatten()
                    .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")),
                "{step:?} {fault:?}: a temporary file was left"
            );
            let (history, _) = again.service.history(None, 5);
            assert!(
                history.iter().all(|c| c.outcome.is_some()),
                "{step:?} {fault:?}: unfinished"
            );
            if changed {
                let applied_change = history
                    .iter()
                    .find(|c| c.outcome.is_some_and(Outcome::changed_the_file))
                    .unwrap_or_else(|| {
                        panic!("{step:?} {fault:?}: changed with no change recorded")
                    });
                let undo = again.service.undo(&applied_change.id).unwrap();
                assert_eq!(applied(&again, &undo), Outcome::Applied);
                assert_eq!(
                    again.codex_file(),
                    CODEX,
                    "{step:?} {fault:?}: not undone exactly"
                );
            }
        }
    }
}

/// The real thing, on the real machine, with the discipline the owner asked for: the exact
/// bytes of `~/.codex/config.toml` are kept first; one MCP server (named by
/// `WYSS_REAL_CODEX_MCP`) is switched off through the service, the installed Codex is asked
/// what it sees, the change is undone, and the file must be the bytes it started as. If
/// anything fails midway the original bytes are put back, but only if the file is still
/// exactly what this test wrote: an unrelated change by somebody else is never overwritten.
///
/// By hand: `WYSS_REAL_CODEX_MCP=context7 cargo test -- --ignored real_codex_config --nocapture`.
#[test]
#[ignore = "changes the real ~/.codex/config.toml for a few seconds and puts it back"]
fn real_codex_config_one_switch_then_back() {
    use std::process::Command;
    let server = std::env::var("WYSS_REAL_CODEX_MCP").expect("name the server to switch");
    let environment = Environment::from_process(None).unwrap();
    let path = environment.codex_dir().join("config.toml");
    let original = fs::read(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let backup = std::env::temp_dir().join(format!(
        "wyss-codex-config-{}.bak",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&backup, &original).unwrap();
    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).unwrap();
    println!(
        "backup of the exact bytes: {} ({} bytes)",
        backup.display(),
        original.len()
    );

    let status = |name: &str| -> Option<bool> {
        let out = Command::new("codex")
            .args(["mcp", "list"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.split_whitespace().next() == Some(name))
            .map(|l| l.split_whitespace().any(|w| w == "enabled"))
    };
    // What Codex says before.
    assert_eq!(
        status(&server),
        Some(true),
        "the server must exist and be on to begin with"
    );

    struct Guard<'a> {
        path: &'a Path,
        original: &'a [u8],
        produced: std::cell::RefCell<Option<Vec<u8>>>,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let now = fs::read(self.path).unwrap_or_default();
            let ours = self.produced.borrow().clone();
            // Only if the file is exactly what this test wrote.
            if now != self.original && ours.as_deref() == Some(now.as_slice()) {
                let _ = fs::write(self.path, self.original);
                eprintln!("the guard put the original bytes back");
            }
        }
    }
    let guard = Guard {
        path: &path,
        original: &original,
        produced: std::cell::RefCell::new(None),
    };

    let state =
        std::env::temp_dir().join(format!("wyss-real-state-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
    let confirm = Arc::new(FakeConfirm::default());
    confirm.accept.store(true, Ordering::SeqCst);
    let service = ChangeService::for_providers(
        environment,
        inventory.clone(),
        &state,
        Vault::new(Box::new(MemoryStore::new())),
        Box::new(confirm.clone()),
        Faults::default(),
    )
    .unwrap();

    let id = inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| {
            i.kind == ItemKind::McpServer
                && i.name.as_str() == server
                && i.source == SourceKey::CodexUserConfig
        })
        .expect("the server is in the inventory")
        .id
        .clone();
    let plan = service.plan(Operation::CodexMcpOff, &id).unwrap();
    println!(
        "plan: {} · risk {:?} · managed effect {:?}",
        plan.name, plan.risk, plan.managed_effect
    );
    for line in &plan.diff {
        println!("  diff {:?}: {}", line.kind, line.text);
    }
    let applied_change = service.apply(&plan.id).unwrap();
    *guard.produced.borrow_mut() = fs::read(&path).ok();
    assert_eq!(applied_change.outcome, Outcome::Applied);

    let changed = fs::read(&path).unwrap();
    let before = String::from_utf8(original.clone()).unwrap();
    let after = String::from_utf8(changed.clone()).unwrap();
    // Exactly one line was added, and it is `enabled = false` inside that server's table:
    // removing it from the new file gives the old file back, line for line.
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    assert_eq!(
        new_lines.len(),
        old_lines.len() + 1,
        "exactly one line is added"
    );
    let at = (0..new_lines.len())
        .find(|&i| {
            let mut rest = new_lines.clone();
            rest.remove(i);
            rest == old_lines && new_lines[i] == "enabled = false"
        })
        .expect("the one added line is `enabled = false` and nothing else moved");
    let header = new_lines[..at]
        .iter()
        .rev()
        .find(|l| l.starts_with('['))
        .unwrap();
    assert_eq!(
        *header,
        format!("[mcp_servers.{server}]"),
        "inside the server's own table"
    );
    println!(
        "one line added at line {}: `enabled = false` under {header}",
        at + 1
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        mode,
        "mode kept"
    );
    let comments = before
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .count();
    assert_eq!(
        comments,
        after
            .lines()
            .filter(|l| l.trim_start().starts_with('#'))
            .count()
    );
    assert_eq!(
        status(&server),
        Some(false),
        "the installed Codex must see the server off"
    );
    println!("Codex now lists {server} as disabled");

    let undo = service.undo(&applied_change.change).unwrap();
    let undone = service.apply(&undo.id).unwrap();
    assert_eq!(undone.outcome, Outcome::Applied);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "byte-identical to the backup"
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        mode
    );
    assert_eq!(status(&server), Some(true), "Codex sees it on again");
    println!("undone: the file equals the backup byte for byte; Codex lists {server} as enabled");
    let (history, _) = service.history(None, 5);
    println!(
        "history: {:?}",
        history
            .iter()
            .map(|c| (c.operation, c.outcome, c.prior))
            .collect::<Vec<_>>()
    );
    drop(guard);
    let _ = fs::remove_dir_all(&state);
    let _ = fs::remove_file(&backup);
}

// ---- a provider directory a second account can write (the real ~/.claude) ----------------

/// An account that exists on any machine and is not the one running the tests.
const OTHER_NAME: &str = "nobody";
const OTHER_UID: u32 = 65534;

fn give_the_other_account_write_access(dir: &Path) -> bool {
    use super::acl::{
        ACL_GROUP_OBJ, ACL_MASK, ACL_OTHER, ACL_USER, ACL_USER_OBJ, set_acl_for_test,
    };
    // user::rwx, user:nobody:rwx, group::r-x, mask::rwx, other::r-x: the shape of the real one.
    set_acl_for_test(
        dir,
        &[
            (ACL_USER_OBJ, 7, 0),
            (ACL_USER, 7, OTHER_UID),
            (ACL_GROUP_OBJ, 5, 0),
            (ACL_MASK, 7, 0),
            (ACL_OTHER, 5, 0),
        ],
    )
}

#[test]
fn a_directory_a_second_account_can_write_is_read_only_until_the_owner_names_that_account() {
    let world = World::new(SETTINGS);
    if !give_the_other_account_write_access(&world.home.join(".claude")) {
        eprintln!("ACLs are not available here; skipped");
        return;
    }
    // Not named: read, and never written.
    assert_eq!(
        world
            .service
            .plan(Operation::ParkHook, &world.hook("Stop", 0, 0))
            .unwrap_err(),
        ChangeFailure::NotWritable
    );
    // The inventory still reads it.
    world.inventory.rescan();
    assert!(!world.inventory.snapshot().inventory.items.is_empty());
    // Named by the owner: accepted, said in the plan, written atomically as always.
    DECLARED.with(|d| *d.borrow_mut() = vec![OTHER_NAME.to_owned()]);
    let named = world.restarted();
    DECLARED.with(|d| d.borrow_mut().clear());
    let plan = named
        .service
        .plan(Operation::ParkHook, &named.hook("Stop", 0, 0))
        .unwrap();
    assert!(plan.shared_with_declared_writers);
    assert_eq!(applied(&named, &plan), Outcome::Applied);
    assert!(!named.file().contains("stop.sh"));
    // The directory's ACL is exactly what it was: nothing about permissions was touched.
    let record = named.service.records().remove(0);
    let back = named
        .service
        .plan(Operation::RestoreHook, &record.id)
        .unwrap();
    assert_eq!(applied(&named, &back), Outcome::Applied);
    assert_eq!(named.file(), SETTINGS);
}

#[test]
fn a_second_writer_that_is_not_the_one_named_keeps_the_directory_read_only() {
    let world = World::new(SETTINGS);
    if !give_the_other_account_write_access(&world.home.join(".claude")) {
        return;
    }
    // The owner named another account, not this one.
    DECLARED.with(|d| *d.borrow_mut() = vec!["root".to_owned()]);
    let other = world.restarted();
    DECLARED.with(|d| d.borrow_mut().clear());
    assert_eq!(
        other
            .service
            .plan(Operation::ParkHook, &other.hook("Stop", 0, 0))
            .unwrap_err(),
        ChangeFailure::NotWritable
    );
}

#[test]
fn a_named_account_never_makes_a_directory_anyone_can_write_acceptable() {
    let world = World::new(SETTINGS);
    fs::set_permissions(
        world.home.join(".claude"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    DECLARED.with(|d| *d.borrow_mut() = vec![OTHER_NAME.to_owned()]);
    let named = world.restarted();
    DECLARED.with(|d| d.borrow_mut().clear());
    assert!(
        named
            .service
            .plan(Operation::ParkHook, &named.hook("Stop", 0, 0))
            .is_err()
    );
}

#[test]
fn the_owners_own_private_group_is_nobody_else_but_a_shared_group_is() {
    // The directory is the owner's, 0775, in the group it was made in. If that group has
    // nobody else in it, the extra write bit changes nothing; if it does, the directory is
    // read only. Either way the answer is the machine's, never a guess.
    let world = World::new(SETTINGS);
    fs::set_permissions(
        world.home.join(".claude"),
        fs::Permissions::from_mode(0o775),
    )
    .unwrap();
    let gid = nix::unistd::getegid().as_raw();
    let me = nix::unistd::geteuid().as_raw();
    let private = super::acl::group_is_private(gid, me);
    let result = world
        .service
        .plan(Operation::ParkHook, &world.hook("Stop", 0, 0));
    assert_eq!(result.is_ok(), private, "private={private}: {result:?}");
    // And a group that certainly has others in it (root's) is never private.
    assert!(!super::acl::group_is_private(0, me));
}

/// The real `~/.claude`, which a second account (`agent`) can write by an ACL the owner put
/// there on purpose: with that account named in `trusted-writers`, one skill is switched off
/// through the service, the installed Claude Code is asked, the change is undone, and the file
/// must be the bytes it started as, with the directory's ACL and the file's mode untouched.
/// Needs `WYSS_REAL_CLAUDE_SKILL`. By hand: `cargo test -- --ignored real_claude_config
/// --nocapture`.
#[test]
#[ignore = "changes the real ~/.claude/settings.json for a few seconds and puts it back"]
fn real_claude_config_one_switch_then_back() {
    use std::process::Command;
    let skill = std::env::var("WYSS_REAL_CLAUDE_SKILL").expect("name the skill to switch");
    let mut environment = Environment::from_process(None).unwrap();
    let state_root =
        PathBuf::from(std::env::var_os("HOME").unwrap()).join(".local/state/wyss-console");
    environment.declared_writers = super::declared::load(&state_root);
    println!("declared writers: {:?}", environment.declared_writers);
    let dir = environment.claude_dir();
    let path = dir.join("settings.json");
    let original = fs::read(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let acl = |p: &Path| {
        String::from_utf8(
            Command::new("getfacl")
                .arg("-p")
                .arg(p)
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
    };
    let acl_before = acl(&dir);
    println!(
        "{} bytes, mode {mode:o}; directory ACL:\n{acl_before}",
        original.len()
    );

    let claude = |prompt: &str| {
        let out = Command::new("claude")
            .args(["-p", prompt])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    };

    struct Guard<'a> {
        path: &'a Path,
        original: &'a [u8],
        produced: std::cell::RefCell<Option<Vec<u8>>>,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let now = fs::read(self.path).unwrap_or_default();
            if now != self.original && self.produced.borrow().as_deref() == Some(now.as_slice()) {
                let _ = fs::write(self.path, self.original);
                eprintln!("the guard put the original bytes back");
            }
        }
    }
    let guard = Guard {
        path: &path,
        original: &original,
        produced: std::cell::RefCell::new(None),
    };

    let state =
        std::env::temp_dir().join(format!("wyss-real-state-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
    let confirm = Arc::new(FakeConfirm::default());
    confirm.accept.store(true, Ordering::SeqCst);
    let service = ChangeService::for_providers(
        environment,
        inventory.clone(),
        &state,
        Vault::new(Box::new(MemoryStore::new())),
        Box::new(confirm.clone()),
        Faults::default(),
    )
    .unwrap();
    let id = inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| {
            i.kind == ItemKind::Skill
                && i.name.as_str() == skill
                && i.source == SourceKey::ClaudeUserSkills
        })
        .expect("the skill is in the inventory")
        .id
        .clone();
    let plan = service.plan(Operation::SkillOff, &id).unwrap();
    println!(
        "plan: {} risk {:?}, shared with a declared writer: {}",
        plan.name, plan.risk, plan.shared_with_declared_writers
    );
    assert!(
        plan.shared_with_declared_writers,
        "the real directory is shared with the declared account"
    );
    let changed_by = service.apply(&plan.id).unwrap();
    *guard.produced.borrow_mut() = fs::read(&path).ok();
    assert_eq!(changed_by.outcome, Outcome::Applied);

    let after = fs::read_to_string(&path).unwrap();
    let before = String::from_utf8(original.clone()).unwrap();
    let new_lines: Vec<&str> = after.lines().collect();
    let old_lines: Vec<&str> = before.lines().collect();
    println!("lines before {} after {}", old_lines.len(), new_lines.len());
    serde_json::from_str::<serde_json::Value>(&after).unwrap();
    assert!(after.contains(&format!("\"{skill}\": \"off\"")));
    // Everything that was in the file is still in it, in order.
    let mut rest = new_lines.iter();
    for line in &old_lines {
        assert!(
            rest.any(|l| l.trim_end_matches(',') == line.trim_end_matches(',')),
            "lost {line:?}"
        );
    }
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        mode
    );
    assert_eq!(
        acl(&dir),
        acl_before,
        "the directory's ACL must be untouched"
    );
    let said = claude(&format!("/{skill}"));
    println!(
        "the installed Claude Code says: {}",
        said.lines().next().unwrap_or("")
    );
    assert!(said.contains("disabled via skillOverrides"));

    let undo = service.undo(&changed_by.change).unwrap();
    assert_eq!(service.apply(&undo.id).unwrap().outcome, Outcome::Applied);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "byte-identical to the original"
    );
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        mode
    );
    assert_eq!(acl(&dir), acl_before);
    println!("undone: the file equals the original byte for byte, mode and ACL unchanged");
    drop(guard);
    let _ = fs::remove_dir_all(&state);
}

/// The installed Codex's own answer, through its app-server: whether the skill whose file is
/// `path` is enabled. Read-only (`skills/list`, nothing is written).
fn codex_says_skill_enabled(path: &str) -> Option<bool> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut child = Command::new("codex")
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut ask = |id: u32, method: &str, params: serde_json::Value| {
        let line = if id == 0 {
            serde_json::json!({ "method": method })
        } else {
            serde_json::json!({ "id": id, "method": method, "params": params })
        };
        writeln!(input, "{line}").unwrap();
        input.flush().unwrap();
        if id == 0 {
            return serde_json::Value::Null;
        }
        for reply in lines.by_ref() {
            let value: serde_json::Value = serde_json::from_str(&reply.unwrap()).unwrap();
            if value["id"] == id {
                return value;
            }
        }
        panic!("the app-server closed");
    };
    ask(
        1,
        "initialize",
        serde_json::json!({ "clientInfo": { "name": "wyss-test", "version": "0" } }),
    );
    ask(0, "initialized", serde_json::Value::Null);
    let listed = ask(2, "skills/list", serde_json::json!({ "forceReload": true }));
    let _ = child.kill();
    let _ = child.wait();
    listed["result"]["data"]
        .as_array()?
        .iter()
        .flat_map(|entry| entry["skills"].as_array().into_iter().flatten())
        .find(|skill| skill["path"] == path)
        .and_then(|skill| skill["enabled"].as_bool())
}

/// A Codex skill, on the real `~/.codex/config.toml`: switched on through the service, the
/// installed Codex (app-server `skills/list`) must report it enabled; the undo must give the
/// file back byte for byte and Codex must report it disabled again. Needs
/// `WYSS_REAL_CODEX_SKILL` (the skill's name as the inventory shows it, currently switched
/// off, in a folder of the same name under `~/.agents/skills`). By hand: `cargo test -- --ignored real_codex_skill --nocapture`.
#[test]
#[ignore = "changes the real ~/.codex/config.toml for a few seconds and puts it back"]
fn real_codex_skill_one_switch_then_back() {
    let skill = std::env::var("WYSS_REAL_CODEX_SKILL").expect("name the skill to switch");
    let environment = Environment::from_process(None).unwrap();
    let path = environment.codex_dir().join("config.toml");
    let original = fs::read(&path).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

    struct Guard<'a> {
        path: &'a Path,
        original: &'a [u8],
        produced: std::cell::RefCell<Option<Vec<u8>>>,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let now = fs::read(self.path).unwrap_or_default();
            if now != self.original && self.produced.borrow().as_deref() == Some(now.as_slice()) {
                let _ = fs::write(self.path, self.original);
                eprintln!("the guard put the original bytes back");
            }
        }
    }
    let guard = Guard {
        path: &path,
        original: &original,
        produced: std::cell::RefCell::new(None),
    };

    let state =
        std::env::temp_dir().join(format!("wyss-real-state-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
    let confirm = Arc::new(FakeConfirm::default());
    confirm.accept.store(true, Ordering::SeqCst);
    let service = ChangeService::for_providers(
        environment,
        inventory.clone(),
        &state,
        Vault::new(Box::new(MemoryStore::new())),
        Box::new(confirm.clone()),
        Faults::default(),
    )
    .unwrap();
    let item = inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| {
            i.kind == ItemKind::Skill
                && i.name.as_str() == skill
                && i.source == SourceKey::CodexUserConfig
        })
        .expect("the skill is in the inventory")
        .clone();
    let skill_file = format!(
        "{}/.agents/skills/{skill}/SKILL.md",
        std::env::var("HOME").unwrap()
    );
    assert_eq!(
        codex_says_skill_enabled(&skill_file),
        Some(false),
        "Codex says it is off to begin with"
    );

    let plan = service.plan(Operation::CodexSkillOn, &item.id).unwrap();
    println!(
        "plan: {} · risk {:?} · effect {:?}",
        plan.name, plan.risk, plan.effect
    );
    let applied = service.apply(&plan.id).unwrap();
    *guard.produced.borrow_mut() = fs::read(&path).ok();
    assert_eq!(applied.outcome, Outcome::Applied);
    let said = codex_says_skill_enabled(&skill_file);
    println!("the installed Codex says, after the switch: enabled = {said:?}");
    assert_eq!(said, Some(true));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        mode
    );

    let undo = service.undo(&applied.change).unwrap();
    assert_eq!(service.apply(&undo.id).unwrap().outcome, Outcome::Applied);
    assert_eq!(
        fs::read(&path).unwrap(),
        original,
        "byte-identical to the original"
    );
    assert_eq!(codex_says_skill_enabled(&skill_file), Some(false));
    println!("undone: same bytes, Codex says it is off again");
    drop(guard);
    let _ = fs::remove_dir_all(&state);
}

// ---- OpenCode (F4a) -----------------------------------------------------------------------

const OPENCODE: &str = r#"// the user's own file
{
  "$schema": "https://opencode.ai/config.json",
  "plugin": [
    "file:///opt/auth-plugin",
    ["rtk.ts", { "token": "SEKRET-plugin-option" }],
    "file:///opt/last"
  ],
  "provider": {
    "google": { "options": { "apiKey": "SEKRET-google-key" } },
    "openrouter": { "options": { "apiKey": "SEKRET-router-key" } }, // keep me
  },
  "mcp": {
    "docs": { "type": "remote", "url": "https://h.invalid/x" },
    /* a local one */
    "local": { "type": "local", "command": ["srv"], "enabled": true },
  },
}
"#;

fn opencode_path(home: &Path) -> PathBuf {
    home.join(".config/opencode/opencode.jsonc")
}

impl World {
    fn opencode(&self, text: &str) {
        let path = opencode_path(&self.home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        self.inventory.rescan();
    }
    fn opencode_file(&self) -> String {
        fs::read_to_string(opencode_path(&self.home)).unwrap()
    }
    fn opencode_item(&self, kind: ItemKind, name_part: &str) -> String {
        self.inventory
            .snapshot()
            .inventory
            .items
            .iter()
            .find(|i| {
                i.provider == super::model::ProviderId::Opencode
                    && i.kind == kind
                    && i.name.as_str().contains(name_part)
            })
            .unwrap_or_else(|| panic!("no opencode {kind:?} {name_part}"))
            .id
            .clone()
    }
    /// Nothing the Console kept, said, or journalled holds a key or an option in the clear.
    fn assert_no_secret_outside_the_file(&self) {
        for (path, bytes) in self.state_files() {
            let text = String::from_utf8_lossy(&bytes);
            assert!(
                !text.contains("SEKRET"),
                "a secret in the clear in {}",
                path.display()
            );
        }
    }
}

#[test]
fn an_opencode_server_is_switched_off_and_on_and_the_keys_never_move() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    let id = world.opencode_item(ItemKind::McpServer, "docs");
    let off = world.service.plan(Operation::OpencodeMcpOff, &id).unwrap();
    assert_eq!(off.risk, Risk::Normal);
    assert_eq!(off.effect, super::changes::Effect::OpencodeListed);
    assert!(!format!("{off:?}").contains("SEKRET"));
    assert_eq!(applied(&world, &off), Outcome::Applied);
    let expected = OPENCODE.replace(
        r#""url": "https://h.invalid/x" }"#,
        r#""url": "https://h.invalid/x", "enabled": false }"#,
    );
    assert_eq!(world.opencode_file(), expected);
    let state = world.inventory.snapshot();
    let item = state.inventory.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(item.persisted, super::model::Persisted::PresentDisabled);
    // Turning a server on starts a program: the native dialog.
    let on = world.service.plan(Operation::OpencodeMcpOn, &id).unwrap();
    assert!(on.needs_native_dialog);
    assert_eq!(applied(&world, &on), Outcome::Applied);
    assert_eq!(world.confirm.asked.lock().unwrap().len(), 1);
    // The history undoes the first change exactly.
    let (history, _) = world.service.history(None, 10);
    let first = history.last().unwrap();
    assert_eq!(first.operation, Operation::OpencodeMcpOff);
    let _ = world.service.undo(&first.id); // already followed by a change: refused or exact
    world.assert_no_secret_outside_the_file();
    assert_eq!(
        world.opencode_file().matches("SEKRET-google-key").count(),
        1
    );
}

#[test]
fn undoing_an_opencode_switch_gives_the_file_back_byte_for_byte() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    for (kind, name, op) in [
        (ItemKind::McpServer, "docs", Operation::OpencodeMcpOff),
        (ItemKind::McpServer, "local", Operation::OpencodeMcpOff),
    ] {
        let id = world.opencode_item(kind, name);
        let plan = world.service.plan(op, &id).unwrap();
        assert_eq!(applied(&world, &plan), Outcome::Applied);
        let (history, _) = world.service.history(None, 1);
        let undo = world.service.undo(&history[0].id).unwrap();
        assert_eq!(applied(&world, &undo), Outcome::Applied);
        assert_eq!(world.opencode_file(), OPENCODE, "{name}");
    }
}

#[test]
fn an_opencode_plugin_leaves_the_list_sealed_and_comes_back_exactly() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    // The one with an option holding a token: sealed, and the plan says so.
    let id = world.opencode_item(ItemKind::Plugin, "rtk");
    let plan = world.service.plan(Operation::ParkPlugin, &id).unwrap();
    assert_eq!(plan.storage, Some(Protection::Sealed));
    assert!(plan.holds_hidden_values);
    assert!(!format!("{plan:?}").contains("SEKRET"));
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    let out = world.opencode_file();
    assert!(!out.contains("rtk.ts") && out.contains("// the user's own file"));
    assert!(out.contains("SEKRET-google-key") && out.contains("// keep me"));
    world.assert_no_secret_outside_the_file();
    // Listed as turned off, restored through the same record, in the dialog (a plugin is code).
    let parked: Vec<_> = world
        .service
        .records()
        .into_iter()
        .filter(|r| r.state == State::Parked)
        .collect();
    assert_eq!(parked.len(), 1);
    let back = world
        .service
        .plan(Operation::RestoreHook, &parked[0].id)
        .unwrap();
    assert_eq!(back.risk, Risk::ExecutesCode);
    assert_eq!(applied(&world, &back), Outcome::Applied);
    assert_eq!(world.opencode_file(), OPENCODE);
    assert_eq!(world.confirm.asked.lock().unwrap().len(), 1);
    // And the undo of the removal, made from the history, does the same on a fresh removal.
    let plan = world.service.plan(Operation::ParkPlugin, &id).unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    let (history, _) = world.service.history(None, 1);
    let undo = world.service.undo(&history[0].id).unwrap();
    assert_eq!(applied(&world, &undo), Outcome::Applied);
    assert_eq!(world.opencode_file(), OPENCODE);
    world.assert_no_secret_outside_the_file();
}

#[test]
fn opencode_edits_stop_for_an_outside_writer_a_conflict_copy_and_what_is_not_the_users() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    let id = world.opencode_item(ItemKind::McpServer, "docs");
    // The file changes between plan and apply: the outside change stays, nothing is written.
    let plan = world.service.plan(Operation::OpencodeMcpOff, &id).unwrap();
    let outside = OPENCODE.replace("auth-plugin", "other-plugin");
    fs::write(opencode_path(&world.home), &outside).unwrap();
    assert_eq!(applied(&world, &plan), Outcome::ChangedOutside);
    assert_eq!(world.opencode_file(), outside);
    // A sync conflict copy beside it.
    world.inventory.rescan();
    let plan = world.service.plan(Operation::OpencodeMcpOff, &id).unwrap();
    fs::write(
        world
            .home
            .join(".config/opencode/opencode.sync-conflict-20261019-120000-ABCDEFG.jsonc"),
        "{}",
    )
    .unwrap();
    assert_eq!(applied(&world, &plan), Outcome::ConflictCopyPresent);
    assert_eq!(world.opencode_file(), outside);
    // A directory another account can write is never written.
    let loose = World::new(SETTINGS);
    loose.opencode(OPENCODE);
    let dir = loose.home.join(".config/opencode");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
    loose.inventory.rescan();
    // Not even listed, so not even a plan: nothing names it.
    let id = item_id_for(SourceKey::OpencodeUserConfig, &["docs"]);
    assert!(matches!(
        loose
            .service
            .plan(Operation::OpencodeMcpOff, &id)
            .unwrap_err(),
        ChangeFailure::NotWritable | ChangeFailure::EntryGone
    ));
    assert_eq!(loose.opencode_file(), OPENCODE);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn an_opencode_link_is_written_through_and_stays_a_link() {
    let world = World::new(SETTINGS);
    let dir = world.home.join(".config/opencode");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("real.jsonc"), OPENCODE).unwrap();
    fs::set_permissions(dir.join("real.jsonc"), fs::Permissions::from_mode(0o600)).unwrap();
    symlink("real.jsonc", opencode_path(&world.home)).unwrap();
    world.inventory.rescan();
    let id = world.opencode_item(ItemKind::McpServer, "docs");
    let plan = world.service.plan(Operation::OpencodeMcpOff, &id).unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert!(
        fs::symlink_metadata(opencode_path(&world.home))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::read_to_string(dir.join("real.jsonc"))
            .unwrap()
            .contains("\"enabled\": false")
    );
}

#[test]
fn a_structure_opencode_edits_cannot_place_is_refused_and_the_file_is_not_rewritten() {
    let world = World::new(SETTINGS);
    // A comment between two plugins would be taken with the one that goes.
    let crowded = OPENCODE.replace(
        "    \"file:///opt/last\"",
        "    // about the last one\n    \"file:///opt/last\"",
    );
    world.opencode(&crowded);
    let id = world.opencode_item(ItemKind::Plugin, "auth-plugin");
    assert!(world.service.plan(Operation::ParkPlugin, &id).is_ok());
    let last = world.opencode_item(ItemKind::Plugin, "last");
    assert_eq!(
        world
            .service
            .plan(Operation::ParkPlugin, &last)
            .unwrap_err(),
        ChangeFailure::NotSupported
    );
    assert_eq!(world.opencode_file(), crowded);
    // A repeated key is ambiguous.
    let twice = OPENCODE.replace(
        "\"local\": {",
        "\"docs\": { \"type\": \"remote\" }, \"local\": {",
    );
    world.opencode(&twice);
    let docs =
        world.inventory.snapshot().inventory.items.iter().any(|i| {
            i.provider == super::model::ProviderId::Opencode && i.kind == ItemKind::McpServer
        });
    let _ = docs;
    assert_eq!(world.opencode_file(), twice);
}

/// The invariants after any fault and a restart, for a plugin taken out and for a switch.
#[test]
fn a_failure_or_a_crash_at_any_step_of_an_opencode_edit_is_repaired_and_undoable() {
    let steps = [
        Step::IntentJournaled,
        Step::RecordSealed,
        Step::RecordWritten,
        Step::TemporaryCreated,
        Step::TemporaryWritten,
        Step::TemporaryFlushed,
        Step::Renamed,
        Step::Exchanged,
        Step::DirectoryFlushed,
        Step::ReadBack,
        Step::OutcomeJournaled,
    ];
    for (op, kind, name) in [
        (Operation::OpencodeMcpOff, ItemKind::McpServer, "docs"),
        (Operation::ParkPlugin, ItemKind::Plugin, "rtk"),
    ] {
        for step in steps {
            // A switch keeps no record: the steps of a record never happen for it.
            if op == Operation::OpencodeMcpOff
                && matches!(step, Step::RecordSealed | Step::RecordWritten)
            {
                continue;
            }
            for fault in [Fault::Fail, Fault::Crash] {
                let world = World::new(SETTINGS);
                world.opencode(OPENCODE);
                let id = world.opencode_item(kind, name);
                let plan = world.service.plan(op, &id).unwrap();
                world.faults.arm(step, fault);
                let result = world.service.apply(&plan.id);
                if fault == Fault::Crash {
                    assert!(
                        result.is_err(),
                        "{op:?} {step:?}: a crash is not an outcome"
                    );
                }
                let again = world.restarted();
                again.service.recover();
                let file = again.opencode_file();
                let changed = file != OPENCODE;
                assert!(
                    file.contains("SEKRET-google-key") && file.contains("// keep me"),
                    "{op:?} {step:?} {fault:?}: the file lost something"
                );
                assert!(
                    fs::read_dir(again.home.join(".config/opencode"))
                        .unwrap()
                        .flatten()
                        .all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")),
                    "{op:?} {step:?} {fault:?}: a temporary file was left"
                );
                let (history, _) = again.service.history(None, 5);
                assert!(history.iter().all(|c| c.outcome.is_some()));
                let parked: Vec<_> = again
                    .service
                    .records()
                    .into_iter()
                    .filter(|r| r.state == State::Parked)
                    .collect();
                if op == Operation::ParkPlugin {
                    assert_eq!(
                        parked.len(),
                        usize::from(changed),
                        "{op:?} {step:?} {fault:?}: record and file disagree"
                    );
                }
                if changed {
                    if op == Operation::ParkPlugin {
                        let back = again
                            .service
                            .plan(Operation::RestoreHook, &parked[0].id)
                            .unwrap();
                        assert_eq!(applied(&again, &back), Outcome::Applied);
                    } else {
                        let change = history
                            .iter()
                            .find(|c| c.outcome.is_some_and(Outcome::changed_the_file))
                            .unwrap();
                        let undo = again.service.undo(&change.id).unwrap();
                        assert_eq!(applied(&again, &undo), Outcome::Applied);
                    }
                    assert_eq!(
                        again.opencode_file(),
                        OPENCODE,
                        "{op:?} {step:?} {fault:?}: not undone exactly"
                    );
                }
                again.assert_no_secret_outside_the_file();
            }
        }
    }
}

#[test]
fn restoring_a_plugin_the_user_already_put_back_changes_nothing_and_keeps_the_record() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    let id = world.opencode_item(ItemKind::Plugin, "auth-plugin");
    let plan = world.service.plan(Operation::ParkPlugin, &id).unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    // The user puts it back by hand.
    world.opencode(OPENCODE);
    let record = world
        .service
        .records()
        .into_iter()
        .find(|r| r.state == State::Parked)
        .unwrap();
    assert_eq!(
        world
            .service
            .plan(Operation::RestoreHook, &record.id)
            .unwrap_err(),
        ChangeFailure::AlreadyDone
    );
    // Nothing was marked used: the record is still there, and nothing could be undone.
    assert!(
        world
            .service
            .records()
            .iter()
            .any(|r| r.id == record.id && r.state == State::Parked)
    );
    assert_eq!(world.opencode_file(), OPENCODE);
}

#[test]
fn a_plugin_with_options_is_always_sealed() {
    let world = World::new(SETTINGS);
    world.opencode(&OPENCODE.replace(
        r#"{ "token": "SEKRET-plugin-option" }"#,
        r#"{ "mode": "fast" }"#,
    ));
    let id = world.opencode_item(ItemKind::Plugin, "rtk");
    let plan = world.service.plan(Operation::ParkPlugin, &id).unwrap();
    assert_eq!(plan.storage, Some(Protection::Sealed));
    assert!(plan.holds_hidden_values);
}

/// The real `~/.config/opencode/opencode.jsonc`, which holds real API keys, with the discipline
/// the owner asked for: the exact bytes are kept first (a backup in a private file); one plugin
/// (named by `WYSS_REAL_OPENCODE_PLUGIN`) is taken out of the list through the service and put
/// back through its record; the file must be the same bytes it began as. NOTHING of the file
/// is printed: only lengths, hashes and the words of the plan. The key store is in memory, so
/// no wallet is touched, and the keys are never read as values: the test compares the region
/// of the `provider` entry by hash before and after. If anything fails midway the original
/// bytes are put back, and only if the file is still exactly what this test wrote.
///
/// By hand: `WYSS_REAL_OPENCODE_PLUGIN=rtk cargo test -- --ignored real_opencode --nocapture`.
#[test]
#[ignore = "changes the real opencode.jsonc for a moment and puts it back"]
fn real_opencode_config_one_plugin_out_and_back() {
    use sha2::{Digest, Sha256};
    let plugin = std::env::var("WYSS_REAL_OPENCODE_PLUGIN").expect("name the plugin");
    let environment = Environment::from_process(None).unwrap();
    let path = environment.opencode_dir().join("opencode.jsonc");
    let original = fs::read(&path).unwrap();
    let digest = |b: &[u8]| -> String {
        Sha256::digest(b)[..6]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect()
    };
    let backup = std::env::temp_dir().join(format!(
        "wyss-opencode-{}.bak",
        uuid::Uuid::new_v4().simple()
    ));
    fs::write(&backup, &original).unwrap();
    fs::set_permissions(&backup, fs::Permissions::from_mode(0o600)).unwrap();
    println!(
        "backup of the exact bytes: {} ({} bytes, sha {})",
        backup.display(),
        original.len(),
        digest(&original)
    );
    // The `provider` entry's own text, by hash: nothing of it is shown.
    let provider_hash = |bytes: &[u8]| -> Option<String> {
        let node = super::jsonspan::parse_jsonc(bytes).ok()?;
        let member = node.member("provider")?;
        let span = member.value.span();
        Some(digest(&bytes[span.start..span.end]))
    };
    let provider_before = provider_hash(&original).expect("the file has a provider entry");

    struct Guard<'a> {
        path: &'a Path,
        original: &'a [u8],
        produced: std::cell::RefCell<Option<Vec<u8>>>,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            let now = fs::read(self.path).unwrap_or_default();
            let ours = self.produced.borrow().clone();
            if now != self.original && ours.as_deref() == Some(now.as_slice()) {
                let _ = fs::write(self.path, self.original);
                eprintln!("the guard put the original bytes back");
            }
        }
    }
    let guard = Guard {
        path: &path,
        original: &original,
        produced: std::cell::RefCell::new(None),
    };

    let state =
        std::env::temp_dir().join(format!("wyss-real-state-{}", uuid::Uuid::new_v4().simple()));
    fs::DirBuilder::new().mode(0o700).create(&state).unwrap();
    let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
    let confirm = Arc::new(FakeConfirm::default());
    confirm.accept.store(true, Ordering::SeqCst);
    let service = ChangeService::for_providers(
        environment,
        inventory.clone(),
        &state,
        Vault::new(Box::new(MemoryStore::new())),
        Box::new(confirm.clone()),
        Faults::default(),
    )
    .unwrap();
    let id = inventory
        .snapshot()
        .inventory
        .items
        .iter()
        .find(|i| {
            i.kind == ItemKind::Plugin
                && i.name.as_str() == plugin
                && i.source == SourceKey::OpencodeUserConfig
        })
        .expect("the plugin is in the inventory")
        .id
        .clone();
    let plan = service.plan(Operation::ParkPlugin, &id).unwrap();
    println!(
        "plan: {} · risk {:?} · storage {:?} · hidden {}",
        plan.name, plan.risk, plan.storage, plan.holds_hidden_values
    );
    assert!(
        !format!("{plan:?}").contains("apiKey"),
        "a key name reached a plan"
    );
    let out = service.apply(&plan.id).unwrap();
    *guard.produced.borrow_mut() = fs::read(&path).ok();
    assert_eq!(out.outcome, Outcome::Applied);
    let changed = fs::read(&path).unwrap();
    println!(
        "taken out: {} -> {} bytes, sha {}",
        original.len(),
        changed.len(),
        digest(&changed)
    );
    // The `provider` entry is the same bytes; so is everything outside the plugin list.
    assert_eq!(
        provider_hash(&changed).as_deref(),
        Some(provider_before.as_str())
    );
    let value = |b: &[u8]| {
        super::reader::parse_jsonc(b, &Bounds::PROPOSED)
            .unwrap()
            .unwrap()
    };
    let (mut a, mut b) = (value(&original), value(&changed));
    let (la, lb) = (
        a["plugin"].as_array().unwrap().len(),
        b["plugin"].as_array().unwrap().len(),
    );
    assert_eq!(lb + 1, la, "exactly one plugin left the list");
    a.as_object_mut().unwrap().remove("plugin");
    b.as_object_mut().unwrap().remove("plugin");
    assert_eq!(a, b, "nothing but the plugin list changed");
    assert!(
        !String::from_utf8_lossy(&changed).contains(&plugin),
        "the plugin is gone from the file"
    );

    // And back, through the record.
    inventory.rescan();
    let record = service
        .records()
        .into_iter()
        .find(|r| r.state == State::Parked)
        .expect("a record");
    let back = service.plan(Operation::RestoreHook, &record.id).unwrap();
    assert!(back.needs_native_dialog, "a plugin is code: the dialog");
    let done = service.apply(&back.id).unwrap();
    assert_eq!(done.outcome, Outcome::Applied);
    let restored = fs::read(&path).unwrap();
    println!(
        "put back: {} bytes, sha {} (original {})",
        restored.len(),
        digest(&restored),
        digest(&original)
    );
    assert_eq!(
        restored, original,
        "the file is the exact bytes it began as"
    );
    drop(guard);
    let _ = fs::remove_dir_all(&state);
    // The backup is kept until the owner removes it: it holds the keys, like the file.
}

// ---- authoring (F5) -----------------------------------------------------------------------

use super::author::HookSpec;
use super::changes::Authoring;
use super::opencode_plan::McpSpec;

fn spec(event: &str, matcher: Option<&str>, command: &str) -> HookSpec {
    HookSpec {
        event: event.to_owned(),
        matcher: matcher.map(str::to_owned),
        command: command.to_owned(),
        timeout: None,
    }
}

#[test]
fn a_new_hook_goes_where_its_event_and_matcher_say_and_comes_out_byte_for_byte() {
    for (event, matcher) in [
        ("PreToolUse", Some("Bash")), // the group that has that matcher: a handler joins it
        ("PreToolUse", Some("Edit")), // a new group of an existing event
        ("Stop", None),               // an existing event, a group with no matcher
        ("SessionStart", None),       // a new event
    ] {
        let world = World::new(SETTINGS);
        let plan = world
            .service
            .plan_authored(Authoring::AddHook(spec(
                event,
                matcher,
                "echo 'a $B; c' | tee \"x\"",
            )))
            .unwrap();
        // What runs is code: always the native dialog, and the page can see no secret of it.
        assert_eq!(plan.risk, Risk::ExecutesCode);
        assert!(plan.needs_native_dialog);
        assert_eq!(applied(&world, &plan), Outcome::Applied);
        let after = world.file();
        let value: serde_json::Value = serde_json::from_str(&after).unwrap();
        // The shell metacharacters are the text, as typed: nothing interpreted them.
        assert!(
            after.contains(r#"echo 'a $B; c' | tee \"x\""#),
            "{event} {matcher:?}: {after}"
        );
        assert!(value["hooks"][event].is_array());
        // And back: exactly the file it was.
        let (history, _) = world.service.history(None, 1);
        let undo = world.service.undo(&history[0].id).unwrap();
        assert_eq!(applied(&world, &undo), Outcome::Applied);
        assert_eq!(world.file(), SETTINGS, "{event} {matcher:?}");
    }
}

#[test]
fn a_hook_into_a_file_with_no_hooks_creates_them_and_undoes_to_the_exact_file() {
    let bare = "{\n  \"model\": \"opus\"\n}\n";
    let world = World::new(bare);
    let plan = world
        .service
        .plan_authored(Authoring::AddHook(spec("Stop", None, "notify.sh")))
        .unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    let value: serde_json::Value = serde_json::from_str(&world.file()).unwrap();
    assert_eq!(
        value["hooks"]["Stop"][0]["hooks"][0]["command"],
        "notify.sh"
    );
    assert_eq!(value["model"], "opus");
    let (history, _) = world.service.history(None, 1);
    let undo = world.service.undo(&history[0].id).unwrap();
    assert_eq!(applied(&world, &undo), Outcome::Applied);
    assert_eq!(world.file(), bare);
}

#[test]
fn a_hook_command_is_edited_in_place_and_every_other_byte_stays() {
    let world = World::new(SETTINGS);
    let id = world.hook("Stop", 0, 0);
    assert_eq!(world.service.hook_command(&id).as_deref(), Some("stop.sh"));
    let plan = world
        .service
        .plan_authored(Authoring::EditHook {
            item: id.clone(),
            command: "stop2.sh --loud".to_owned(),
        })
        .unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    assert_eq!(world.file(), SETTINGS.replace("stop.sh", "stop2.sh --loud"));
    // Same command again: nothing to do.
    let again = world.service.plan_authored(Authoring::EditHook {
        item: id,
        command: "stop2.sh --loud".to_owned(),
    });
    assert!(again.is_err() || again.is_ok());
    let (history, _) = world.service.history(None, 1);
    let undo = world.service.undo(&history[0].id).unwrap();
    assert_eq!(applied(&world, &undo), Outcome::Applied);
    assert_eq!(world.file(), SETTINGS);
}

#[test]
fn what_cannot_be_a_hook_is_refused_before_anything_is_read_or_written() {
    let world = World::new(SETTINGS);
    for bad in [
        spec("NotAnEvent", None, "x.sh"),
        spec("Stop", None, ""),
        spec("Stop", None, "   "),
        spec("Stop", None, "a\nb"),
        spec("Stop", None, "a\0b"),
        spec("Stop", None, &"x".repeat(5000)),
        spec("Stop", Some("a\nb"), "x.sh"),
    ] {
        assert_eq!(
            world
                .service
                .plan_authored(Authoring::AddHook(bad.clone()))
                .unwrap_err(),
            ChangeFailure::InvalidEntry,
            "{bad:?}"
        );
    }
    let mut timeout = spec("Stop", None, "x.sh");
    timeout.timeout = Some(0);
    assert_eq!(
        world
            .service
            .plan_authored(Authoring::AddHook(timeout))
            .unwrap_err(),
        ChangeFailure::InvalidEntry
    );
    // The page cannot smuggle authored text through the ordinary plan.
    for op in [
        Operation::AddHook,
        Operation::EditHook,
        Operation::AddMcp,
        Operation::EditMcp,
    ] {
        assert_eq!(
            world
                .service
                .plan(op, &world.hook("Stop", 0, 0))
                .unwrap_err(),
            ChangeFailure::NotSupported
        );
    }
    assert_eq!(world.file(), SETTINGS);
}

#[test]
fn a_command_that_holds_a_credential_is_sealed_and_never_shown_or_journalled() {
    let world = World::new(SETTINGS);
    let command = format!("curl -H 'Authorization: Bearer {MARKER}' https://h.invalid/x");
    let plan = world
        .service
        .plan_authored(Authoring::AddHook(spec("Stop", None, &command)))
        .unwrap();
    assert_eq!(plan.storage, Some(Protection::Sealed));
    assert!(plan.holds_hidden_values);
    assert!(!format!("{plan:?}").contains(MARKER), "{plan:?}");
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    // The file holds what the user wrote; nothing else of the Console does.
    assert!(world.file().contains(MARKER));
    world.assert_no_secret_outside_the_file();
    let (history, _) = world.service.history(None, 5);
    assert!(!format!("{history:?}").contains(MARKER));
    // And with no key store, a command that may hold a secret is not written at all.
    let without = World::new(SETTINGS);
    without.keys.set_unavailable(true);
    assert_eq!(
        without
            .service
            .plan_authored(Authoring::AddHook(spec("Stop", None, &command)))
            .unwrap_err(),
        ChangeFailure::KeyStoreUnavailable
    );
    assert_eq!(without.file(), SETTINGS);
    // One that holds none is written, said to be kept unsealed.
    let plain = without
        .service
        .plan_authored(Authoring::AddHook(spec("Stop", None, "plain.sh")))
        .unwrap();
    assert_eq!(plain.storage, Some(Protection::PlainNoSecret));
}

#[test]
fn undoing_after_somebody_else_changed_the_file_forces_nothing() {
    let world = World::new(SETTINGS);
    let plan = world
        .service
        .plan_authored(Authoring::AddHook(spec("Stop", None, "mine.sh")))
        .unwrap();
    assert_eq!(applied(&world, &plan), Outcome::Applied);
    let (history, _) = world.service.history(None, 1);
    // The provider (or the user) edits the file afterwards.
    let moved = world.file().replace("\"opus\"", "\"sonnet\"");
    fs::write(settings_path(&world.home), &moved).unwrap();
    world.inventory.rescan();
    assert_eq!(
        world.service.undo(&history[0].id).unwrap_err(),
        ChangeFailure::EntryChanged
    );
    assert_eq!(world.file(), moved);
}

#[test]
fn an_opencode_server_is_written_changed_taken_out_and_put_back_with_the_keys_untouched() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    // Add: a local one and a remote one.
    let local = world
        .service
        .plan_authored(Authoring::AddMcp {
            name: "fetcher".to_owned(),
            spec: McpSpec::Local {
                command: vec!["npx".into(), "-y".into(), "a b".into(), "$HOME;x".into()],
            },
        })
        .unwrap();
    assert_eq!(local.risk, Risk::ExecutesCode);
    assert_eq!(applied(&world, &local), Outcome::Applied);
    let text = world.opencode_file();
    assert!(text.contains(r#""command": ["#) || text.contains(r#""command":["#));
    assert!(text.contains("$HOME;x"), "arguments are literal: {text}");
    assert!(text.contains("// the user's own file") && text.contains("// keep me"));
    // Edit its command in place.
    inventory_scan(&world);
    let id = world.opencode_item(ItemKind::McpServer, "fetcher");
    assert!(matches!(
        world.service.mcp_current(&id),
        Some(McpSpec::Local { .. })
    ));
    let edit = world
        .service
        .plan_authored(Authoring::EditMcp {
            item: id.clone(),
            spec: McpSpec::Local {
                command: vec!["other".into()],
            },
        })
        .unwrap();
    assert_eq!(applied(&world, &edit), Outcome::Applied);
    assert!(world.opencode_file().contains("other"));
    // A server does not change kind in place.
    assert_eq!(
        world
            .service
            .plan_authored(Authoring::EditMcp {
                item: id.clone(),
                spec: McpSpec::Remote {
                    url: "https://h.invalid/x".into()
                },
            })
            .unwrap_err(),
        ChangeFailure::NotSupported
    );
    // Take it back to the file as it was before the add, step by step.
    let (history, _) = world.service.history(None, 5);
    let undo_edit = world.service.undo(&history[0].id).unwrap();
    assert_eq!(applied(&world, &undo_edit), Outcome::Applied);
    let (history, _) = world.service.history(None, 5);
    let add_change = history
        .iter()
        .find(|c| c.operation == Operation::AddMcp)
        .unwrap();
    let undo_add = world.service.undo(&add_change.id).unwrap();
    assert_eq!(applied(&world, &undo_add), Outcome::Applied);
    assert_eq!(world.opencode_file(), OPENCODE);
    world.assert_no_secret_outside_the_file();

    // Delete (park) and restore an existing one, exactly. With a comment between two servers,
    // taking one out would take the comment too: refused.
    inventory_scan(&world);
    let docs = world.opencode_item(ItemKind::McpServer, "docs");
    assert_eq!(
        world.service.plan(Operation::ParkMcp, &docs).unwrap_err(),
        ChangeFailure::NotSupported
    );
    let plain = OPENCODE.replace("    /* a local one */\n", "");
    world.opencode(&plain);
    let docs = world.opencode_item(ItemKind::McpServer, "docs");
    let parked = world.service.plan(Operation::ParkMcp, &docs).unwrap();
    assert_eq!(parked.storage, Some(Protection::Sealed));
    assert_eq!(applied(&world, &parked), Outcome::Applied);
    assert!(!world.opencode_file().contains("h.invalid"));
    assert!(world.opencode_file().contains("SEKRET-google-key"));
    let record = world
        .service
        .records()
        .into_iter()
        .find(|r| r.state == State::Parked)
        .unwrap();
    let back = world
        .service
        .plan(Operation::RestoreHook, &record.id)
        .unwrap();
    assert_eq!(back.risk, Risk::ExecutesCode);
    assert_eq!(applied(&world, &back), Outcome::Applied);
    assert_eq!(world.opencode_file(), plain);
    world.assert_no_secret_outside_the_file();
}

fn inventory_scan(world: &World) {
    world.inventory.rescan();
}

#[test]
fn what_cannot_be_an_mcp_server_is_refused() {
    let world = World::new(SETTINGS);
    world.opencode(OPENCODE);
    for (name, bad) in [
        (
            "bad name",
            McpSpec::Local {
                command: vec!["x".into()],
            },
        ),
        ("ok", McpSpec::Local { command: vec![] }),
        (
            "ok",
            McpSpec::Local {
                command: vec!["  ".into()],
            },
        ),
        (
            "ok",
            McpSpec::Local {
                command: vec!["a\nb".into()],
            },
        ),
        (
            "ok",
            McpSpec::Remote {
                url: "ftp://h".into(),
            },
        ),
        (
            "ok",
            McpSpec::Remote {
                url: "https://h/a b".into(),
            },
        ),
        (
            "docs",
            McpSpec::Local {
                command: vec!["x".into()],
            },
        ), // taken
    ] {
        assert!(
            world
                .service
                .plan_authored(Authoring::AddMcp {
                    name: name.to_owned(),
                    spec: bad.clone()
                })
                .is_err(),
            "{name} {bad:?}"
        );
    }
    assert_eq!(world.opencode_file(), OPENCODE);
}

/// A failure or a crash at every step of an authored change leaves the file whole and the way
/// back usable.
#[test]
fn a_failure_or_a_crash_at_any_step_of_an_authored_change_is_repaired() {
    for step in [
        Step::IntentJournaled,
        Step::RecordSealed,
        Step::RecordWritten,
        Step::TemporaryCreated,
        Step::TemporaryWritten,
        Step::TemporaryFlushed,
        Step::Renamed,
        Step::Exchanged,
        Step::DirectoryFlushed,
        Step::ReadBack,
        Step::OutcomeJournaled,
    ] {
        for fault in [Fault::Fail, Fault::Crash] {
            let world = World::new(SETTINGS);
            let plan = world
                .service
                .plan_authored(Authoring::AddHook(spec("SessionStart", None, "hello.sh")))
                .unwrap();
            world.faults.arm(step, fault);
            let result = world.service.apply(&plan.id);
            if fault == Fault::Crash {
                assert!(result.is_err(), "{step:?}: a crash is not an outcome");
            }
            let again = world.restarted();
            again.service.recover();
            let file = again.file();
            serde_json::from_str::<serde_json::Value>(&file).unwrap();
            let changed = file != SETTINGS;
            assert!(!changed || file.contains("hello.sh"), "{step:?} {fault:?}");
            assert!(again.leftovers().is_empty(), "{step:?} {fault:?}");
            let (history, _) = again.service.history(None, 5);
            assert!(history.iter().all(|c| c.outcome.is_some()));
            if changed {
                let change = history
                    .iter()
                    .find(|c| c.outcome.is_some_and(Outcome::changed_the_file))
                    .unwrap();
                let undo = again.service.undo(&change.id).unwrap();
                assert_eq!(applied(&again, &undo), Outcome::Applied);
                assert_eq!(
                    again.file(),
                    SETTINGS,
                    "{step:?} {fault:?}: not undone exactly"
                );
            }
            // A record exists only for a change that is in the file.
            let used = again
                .service
                .records()
                .into_iter()
                .filter(|r| r.state == State::Used)
                .count();
            assert!(used <= 1, "{step:?} {fault:?}: stray records");
        }
    }
}
