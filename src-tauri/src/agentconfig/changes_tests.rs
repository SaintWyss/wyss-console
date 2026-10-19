//! Changing a configuration, end to end, on private temporary homes (NFR-MNT-03): the
//! acceptance criteria of F2a that a machine can check, the secret regression tests of
//! ADR 0029, and a fault injected at every step of an apply (14 §4).
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
        };
        let inventory = InventoryService::start(environment.clone(), Bounds::PROPOSED).unwrap();
        let confirm = Arc::new(FakeConfirm::default());
        confirm.accept.store(true, Ordering::SeqCst);
        let service = ChangeService::for_claude(
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

// ---- ADR 0029: the key store ------------------------------------------------------------

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
        ChangeFailure::RecordUnreadable
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
    fs::set_permissions(
        world.home.join(".claude"),
        fs::Permissions::from_mode(0o775),
    )
    .unwrap();
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

const STEPS: [Step; 10] = [
    Step::IntentJournaled,
    Step::RecordSealed,
    Step::RecordWritten,
    Step::TemporaryCreated,
    Step::TemporaryWritten,
    Step::TemporaryFlushed,
    Step::Renamed,
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
            Step::DirectoryFlushed | Step::OutcomeJournaled => {
                assert_eq!(outcome, Some(Outcome::Applied))
            }
            Step::ReadBack => assert_eq!(outcome, Some(Outcome::VerifyFailed)),
            Step::RecordMarked => (),
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
