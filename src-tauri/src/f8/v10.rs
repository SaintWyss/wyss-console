//! Offline V9→V10 schema preparation for F8 workspace ownership.
//!
//! The migration creates a persistent store identity and an import-required fence. It never
//! migrates a live installation. Directory paths and prompts are absent from the schema.
use super::{migration, v9};
use crate::contracts::control::{ControlError as E, ControlResult};
use rusqlite::{Connection, TransactionBehavior, types::Value};
use std::collections::{BTreeMap, BTreeSet};

const BOUNDARY_V10: [&str; 3] = [
    "CREATE TABLE migration_boundary_v10 (id INTEGER PRIMARY KEY CHECK(id=1), v9_last_sequence TEXT NOT NULL CHECK(length(v9_last_sequence) BETWEEN 1 AND 20), v9_events_sha256 TEXT NOT NULL CHECK(length(v9_events_sha256)=64), v9_stream_id TEXT NOT NULL) STRICT",
    "CREATE TRIGGER migration_boundary_v10_no_update BEFORE UPDATE ON migration_boundary_v10 BEGIN SELECT RAISE(ABORT,'append_only'); END",
    "CREATE TRIGGER migration_boundary_v10_no_delete BEFORE DELETE ON migration_boundary_v10 BEGIN SELECT RAISE(ABORT,'append_only'); END",
];

const GUARD_OBJECTS: [&str; 23] = [
    "CREATE TABLE workspace_guard_state (id INTEGER PRIMARY KEY CHECK(id=1), authority_store_id TEXT NOT NULL UNIQUE CHECK(length(authority_store_id)=36), import_state TEXT NOT NULL CHECK(import_state IN ('required','complete')), coordinator_store_id TEXT, snapshot_digest TEXT CHECK(snapshot_digest IS NULL OR length(snapshot_digest)=64), snapshot_revision INTEGER CHECK(snapshot_revision IS NULL OR snapshot_revision>0), CHECK((import_state='required' AND coordinator_store_id IS NULL AND snapshot_digest IS NULL AND snapshot_revision IS NULL) OR (import_state='complete' AND coordinator_store_id IS NOT NULL AND length(coordinator_store_id)=36 AND snapshot_digest IS NOT NULL AND snapshot_revision IS NOT NULL))) STRICT",
    "CREATE TRIGGER workspace_guard_store_id_no_update BEFORE UPDATE OF authority_store_id ON workspace_guard_state BEGIN SELECT RAISE(ABORT,'immutable_authority_store_id'); END",
    "CREATE TRIGGER workspace_guard_store_id_no_delete BEFORE DELETE ON workspace_guard_state BEGIN SELECT RAISE(ABORT,'immutable_authority_store_id'); END",
    "CREATE TABLE workspace_root_bindings (root_binding_id TEXT PRIMARY KEY CHECK(length(root_binding_id)=36), authority_store_id TEXT NOT NULL REFERENCES workspace_guard_state(authority_store_id), authority_epoch TEXT NOT NULL CHECK(length(authority_epoch)=36), proof_state TEXT NOT NULL CHECK(proof_state IN ('pinned','identity_unproven','released')), proof_revision INTEGER NOT NULL CHECK(proof_revision>0), filesystem_fingerprint TEXT NOT NULL CHECK(length(filesystem_fingerprint)=64), object_fingerprint TEXT NOT NULL CHECK(length(object_fingerprint)=64), ancestry_fingerprint TEXT NOT NULL CHECK(length(ancestry_fingerprint)=64), mount_id INTEGER NOT NULL CHECK(mount_id>0), created_at TEXT NOT NULL) STRICT",
    "CREATE TRIGGER workspace_root_identity_no_update BEFORE UPDATE OF root_binding_id,authority_store_id,authority_epoch,filesystem_fingerprint,object_fingerprint,ancestry_fingerprint,mount_id ON workspace_root_bindings BEGIN SELECT RAISE(ABORT,'immutable_root_binding'); END",
    "CREATE TABLE workspace_claims (claim_id TEXT PRIMARY KEY CHECK(length(claim_id)=36), coordinator_store_id TEXT NOT NULL CHECK(length(coordinator_store_id)=36), run_id TEXT NOT NULL CHECK(length(run_id)=36), plan_revision INTEGER NOT NULL CHECK(plan_revision>0), plan_digest TEXT NOT NULL CHECK(length(plan_digest)=64), root_binding_id TEXT NOT NULL REFERENCES workspace_root_bindings(root_binding_id), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), claim_revision INTEGER NOT NULL CHECK(claim_revision>0), gate_state TEXT NOT NULL CHECK(gate_state IN ('open','paused','stopped','quarantined')), gate_revision INTEGER NOT NULL CHECK(gate_revision>0), phase TEXT NOT NULL CHECK(phase IN ('held','unsettled','verified','released','quarantined')), quarantine_reason TEXT, settlement_id TEXT, coordinator_revision INTEGER NOT NULL DEFAULT 0 CHECK(coordinator_revision>=0), created_at TEXT NOT NULL, UNIQUE(coordinator_store_id,run_id,claim_generation), CHECK((phase='quarantined' AND gate_state='quarantined' AND quarantine_reason IS NOT NULL) OR (phase<>'quarantined' AND quarantine_reason IS NULL))) STRICT",
    "CREATE TABLE guarded_start_receipts (receipt_id TEXT PRIMARY KEY CHECK(length(receipt_id)=36), authority_store_id TEXT NOT NULL REFERENCES workspace_guard_state(authority_store_id), authority_epoch TEXT NOT NULL CHECK(length(authority_epoch)=36), claim_id TEXT NOT NULL REFERENCES workspace_claims(claim_id), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), claim_revision INTEGER NOT NULL CHECK(claim_revision>0), root_binding_id TEXT NOT NULL REFERENCES workspace_root_bindings(root_binding_id), tuple_digest TEXT NOT NULL CHECK(length(tuple_digest)=64), purpose TEXT NOT NULL CHECK(purpose IN ('pipeline_step','handoff_fresh_receiver')), expires_at TEXT NOT NULL, receipt_revision INTEGER NOT NULL CHECK(receipt_revision>0), state TEXT NOT NULL CHECK(state IN ('prepared','consumed','invalidated')), attempt_id TEXT, turn_id TEXT, UNIQUE(attempt_id), CHECK((state='consumed' AND attempt_id IS NOT NULL AND turn_id IS NOT NULL) OR (state<>'consumed' AND attempt_id IS NULL AND turn_id IS NULL))) STRICT",
    "CREATE TABLE guarded_attempts (attempt_id TEXT PRIMARY KEY CHECK(length(attempt_id)=36), intent_id TEXT NOT NULL UNIQUE CHECK(length(intent_id)=36), claim_id TEXT NOT NULL REFERENCES workspace_claims(claim_id), receipt_id TEXT NOT NULL UNIQUE REFERENCES guarded_start_receipts(receipt_id), turn_id TEXT NOT NULL UNIQUE CHECK(length(turn_id)=36), tuple_digest TEXT NOT NULL CHECK(length(tuple_digest)=64), send_state TEXT NOT NULL CHECK(send_state IN ('reserved','possibly_sent','verifying','known_no_send','terminal','needs_review')), created_at TEXT NOT NULL) STRICT",
    "CREATE TABLE verification_observations (observation_id TEXT PRIMARY KEY CHECK(length(observation_id)=36), claim_id TEXT NOT NULL REFERENCES workspace_claims(claim_id), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), claim_revision INTEGER NOT NULL CHECK(claim_revision>0), root_binding_id TEXT NOT NULL REFERENCES workspace_root_bindings(root_binding_id), authority_epoch TEXT NOT NULL CHECK(length(authority_epoch)=36), attempt_id TEXT NOT NULL CHECK(length(attempt_id)=36), content_digest TEXT NOT NULL CHECK(length(content_digest)=64), state TEXT NOT NULL CHECK(state IN ('prepared','settled','invalidated')), decision TEXT CHECK(decision IS NULL OR decision IN ('confirmed_completed','confirmed_no_effect','confirmed_cancelled')), created_at TEXT NOT NULL) STRICT",
    "CREATE TABLE managed_session_freshness (session_id TEXT PRIMARY KEY REFERENCES managed_sessions(session_id), freshness_state TEXT NOT NULL CHECK(freshness_state IN ('proven','unproven','closed')), authority_epoch TEXT CHECK(authority_epoch IS NULL OR length(authority_epoch)=36), preparation_generation INTEGER NOT NULL CHECK(preparation_generation>=0), ever_started INTEGER NOT NULL CHECK(ever_started>=0), CHECK((freshness_state='proven' AND authority_epoch IS NOT NULL AND preparation_generation>0) OR (freshness_state IN ('unproven','closed') AND authority_epoch IS NULL))) STRICT",
    "CREATE TRIGGER managed_session_freshness_insert AFTER INSERT ON managed_sessions BEGIN INSERT INTO managed_session_freshness(session_id,freshness_state,authority_epoch,preparation_generation,ever_started) VALUES(NEW.session_id,'unproven',NULL,0,0); END",
    "CREATE TABLE managed_runtime_workspace_bindings (runtime_instance_id TEXT PRIMARY KEY CHECK(length(runtime_instance_id)=36), managed_session_id TEXT NOT NULL REFERENCES managed_sessions(session_id), generation INTEGER NOT NULL CHECK(generation>0), authority_store_id TEXT NOT NULL REFERENCES workspace_guard_state(authority_store_id), authority_epoch TEXT NOT NULL CHECK(length(authority_epoch)=36), root_binding_id TEXT NOT NULL REFERENCES workspace_root_bindings(root_binding_id), bound_at TEXT NOT NULL, UNIQUE(managed_session_id,generation)) STRICT",
    "CREATE TRIGGER managed_runtime_workspace_binding_immutable BEFORE UPDATE ON managed_runtime_workspace_bindings BEGIN SELECT RAISE(ABORT,'immutable_runtime_workspace_binding'); END",
    "CREATE TABLE workspace_imported_claims (coordinator_store_id TEXT NOT NULL CHECK(length(coordinator_store_id)=36), claim_id TEXT NOT NULL CHECK(length(claim_id)=36), run_id TEXT NOT NULL CHECK(length(run_id)=36), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), identity_state TEXT NOT NULL CHECK(identity_state='identity_unproven'), source_revision INTEGER NOT NULL CHECK(source_revision>0), PRIMARY KEY(coordinator_store_id,claim_id,claim_generation)) STRICT",
    "CREATE TRIGGER workspace_imported_claims_no_update BEFORE UPDATE ON workspace_imported_claims BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END",
    "CREATE TRIGGER workspace_imported_claims_no_delete BEFORE DELETE ON workspace_imported_claims BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END",
    "CREATE TABLE authority_epoch_history (authority_epoch TEXT PRIMARY KEY CHECK(length(authority_epoch)=36), started_at TEXT NOT NULL) STRICT",
    "CREATE TRIGGER authority_epoch_history_no_update BEFORE UPDATE ON authority_epoch_history BEGIN SELECT RAISE(ABORT,'immutable_authority_epoch'); END",
    "CREATE TRIGGER authority_epoch_history_no_delete BEFORE DELETE ON authority_epoch_history BEGIN SELECT RAISE(ABORT,'immutable_authority_epoch'); END",
    "CREATE INDEX workspace_claims_unreleased ON workspace_claims(phase,gate_state)",
    "CREATE INDEX guarded_attempts_claim_state ON guarded_attempts(claim_id,send_state)",
    BOUNDARY_V10[0],
    BOUNDARY_V10[1],
];

const BOUNDARY_V10_LAST: &str = BOUNDARY_V10[2];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Metadata,
    GuardObjects,
    Freshness,
    Boundary,
    Validated,
}

/// The exact V10 schema, mechanically derived from the reviewed V9 schema plus guard objects.
pub fn expected_v10() -> ControlResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (name, sql) in v9::expected_v9()? {
        let sql = if name == "authority_metadata" {
            replace_once(&sql, "CHECK(schema_version=9)", "CHECK(schema_version=10)")?
        } else {
            sql
        };
        out.insert(name, sql);
    }
    for sql in GUARD_OBJECTS.into_iter().chain([BOUNDARY_V10_LAST]) {
        let name = object_name(sql)?;
        out.insert(name, sql.to_owned());
    }
    Ok(out)
}

/// Prepare a V10 image from a validated, quiescent V9 source. The source is never changed.
pub fn migrate(
    v9_image: &Connection,
    mut after: impl FnMut(Stage) -> ControlResult<()>,
) -> ControlResult<Connection> {
    v9::validate_v9(v9_image)?;
    if !v9::quiescent(v9_image)? {
        return Err(E::StorageIncompatible);
    }
    let mut image = migration::clone_database(v9_image)?;
    image
        .execute_batch("PRAGMA foreign_keys=OFF; PRAGMA legacy_alter_table=ON;")
        .map_err(invalid)?;
    let tx = image
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(db)?;
    rebuild_metadata_v10(&tx)?;
    after(Stage::Metadata)?;

    let expected = expected_v10()?;
    let previous = v9::expected_v9()?.into_keys().collect::<BTreeSet<_>>();
    let added = expected
        .into_iter()
        .filter(|(name, _)| !previous.contains(name))
        .collect::<Vec<_>>();
    for (_, sql) in added
        .iter()
        .filter(|(_, sql)| sql.starts_with("CREATE TABLE"))
    {
        tx.execute_batch(sql).map_err(invalid)?;
    }
    for (_, sql) in added
        .iter()
        .filter(|(_, sql)| !sql.starts_with("CREATE TABLE"))
    {
        tx.execute_batch(sql).map_err(invalid)?;
    }
    let store_id = crate::control::store::uuid();
    tx.execute(
        "INSERT INTO workspace_guard_state(id,authority_store_id,import_state,coordinator_store_id,snapshot_digest,snapshot_revision) VALUES(1,?1,'required',NULL,NULL,NULL)",
        [store_id.as_str()],
    )
    .map_err(invalid)?;
    after(Stage::GuardObjects)?;
    tx.execute(
        "INSERT INTO managed_session_freshness(session_id,freshness_state,authority_epoch,preparation_generation,ever_started) SELECT session_id,'unproven',NULL,0,0 FROM managed_sessions",
        [],
    )
    .map_err(invalid)?;
    after(Stage::Freshness)?;

    let (last, stream): (String, String) = tx
        .query_row(
            "SELECT last_sequence,stream_id FROM authority_metadata WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(invalid)?;
    let digest = migration::boundary_digest(&tx, last.parse().map_err(invalid)?)?;
    tx.execute(
        "INSERT INTO migration_boundary_v10 VALUES(1,?1,?2,?3)",
        rusqlite::params![last, digest, stream],
    )
    .map_err(invalid)?;
    after(Stage::Boundary)?;
    tx.pragma_update(None, "user_version", 10)
        .map_err(invalid)?;
    tx.commit().map_err(db)?;
    image
        .execute_batch("PRAGMA legacy_alter_table=OFF; PRAGMA foreign_keys=ON;")
        .map_err(db)?;
    validate_v10(&image)?;
    after(Stage::Validated)?;
    Ok(image)
}

/// Validate exact V10 schema, preserve the V9 replay boundary and verify all V9 data through a
/// private V9 projection. Guard rows are validated separately and never trust saved path data.
pub fn validate_v10(c: &Connection) -> ControlResult<()> {
    let version = c
        .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
        .map_err(invalid)?;
    let actual_objects = migration::objects(c)?;
    let expected_objects = expected_v10()?;
    if version != 10 {
        #[cfg(test)]
        eprintln!("V10 validator: user_version mismatch: {version}");
        return Err(E::StorageIncompatible);
    }
    if actual_objects != expected_objects {
        #[cfg(test)]
        for name in actual_objects.keys().chain(expected_objects.keys()) {
            if actual_objects.get(name) != expected_objects.get(name) {
                eprintln!(
                    "V10 schema mismatch for {name}: actual={:?} expected={:?}",
                    actual_objects.get(name),
                    expected_objects.get(name)
                );
            }
        }
        return Err(E::StorageIncompatible);
    }
    migration::integrity(c)?;
    let version: i64 = c
        .query_row(
            "SELECT schema_version FROM authority_metadata WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    if version != 10 {
        #[cfg(test)]
        eprintln!("V10 validator: metadata version mismatch: {version}");
        return Err(E::StorageIncompatible);
    }
    let legacy = v9_projection(c)?;
    v9::validate_v9(&legacy)?;

    let (store_id, state, coordinator, digest, revision): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
    ) = c
        .query_row(
            "SELECT authority_store_id,import_state,coordinator_store_id,snapshot_digest,snapshot_revision FROM workspace_guard_state WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .map_err(invalid)?;
    super::protocol::canonical(&store_id)?;
    match state.as_str() {
        "required" if coordinator.is_none() && digest.is_none() && revision.is_none() => {
            let imported: i64 = c
                .query_row(
                    "SELECT count(*) FROM workspace_imported_claims",
                    [],
                    |row| row.get(0),
                )
                .map_err(invalid)?;
            if imported != 0 {
                return Err(E::StorageIncompatible);
            }
        }
        "complete" => {
            let coordinator = coordinator.ok_or(E::StorageIncompatible)?;
            super::protocol::canonical(&coordinator)?;
            if digest.as_ref().is_none_or(|value| value.len() != 64)
                || revision.is_none_or(|value| value <= 0)
            {
                return Err(E::StorageIncompatible);
            }
            let foreign_imports: i64 = c
                .query_row(
                    "SELECT count(*) FROM workspace_imported_claims WHERE coordinator_store_id<>?1",
                    [coordinator],
                    |row| row.get(0),
                )
                .map_err(invalid)?;
            if foreign_imports != 0 {
                return Err(E::StorageIncompatible);
            }
        }
        _ => return Err(E::StorageIncompatible),
    }
    let invalid_freshness: i64 = c
        .query_row(
            "SELECT count(*) FROM managed_sessions s LEFT JOIN managed_session_freshness f USING(session_id) WHERE f.session_id IS NULL OR (f.freshness_state='proven' AND NOT EXISTS(SELECT 1 FROM authority_epoch_history e WHERE e.authority_epoch=f.authority_epoch)) OR (f.freshness_state IN ('unproven','closed') AND f.authority_epoch IS NOT NULL)",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    let extra_freshness: i64 = c
        .query_row(
            "SELECT count(*) FROM managed_session_freshness f LEFT JOIN managed_sessions s USING(session_id) WHERE s.session_id IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    if invalid_freshness != 0 || extra_freshness != 0 {
        #[cfg(test)]
        eprintln!("V10 validator: freshness rows mismatch {invalid_freshness}/{extra_freshness}");
        return Err(E::StorageIncompatible);
    }

    let (last, digest, stream): (String, String, String) = c
        .query_row(
            "SELECT v9_last_sequence,v9_events_sha256,v9_stream_id FROM migration_boundary_v10 WHERE id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(invalid)?;
    let current_stream: String = c
        .query_row(
            "SELECT stream_id FROM authority_metadata WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    let head: String = c
        .query_row(
            "SELECT last_sequence FROM authority_metadata WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    if stream != current_stream
        || migration::boundary_digest(c, last.parse().map_err(invalid)?)? != digest
        || last.parse::<u64>().map_err(invalid)? > head.parse::<u64>().map_err(invalid)?
    {
        #[cfg(test)]
        eprintln!("V10 validator: V10 boundary mismatch");
        return Err(E::StorageIncompatible);
    }
    Ok(())
}

/// A downgrade projection is permitted only while V10 has no M6 import or guard state to erase.
pub fn v9_view(c: &Connection) -> ControlResult<Connection> {
    if c.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
        .map_err(invalid)?
        != 10
    {
        return Err(E::StorageIncompatible);
    }
    let guarded: i64 = c
        .query_row(
            "SELECT (SELECT count(*) FROM workspace_claims) + (SELECT count(*) FROM guarded_start_receipts) + (SELECT count(*) FROM guarded_attempts) + (SELECT count(*) FROM verification_observations) + (SELECT count(*) FROM workspace_imported_claims)",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    let imported: String = c
        .query_row(
            "SELECT import_state FROM workspace_guard_state WHERE id=1",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    if guarded != 0 || imported != "required" {
        return Err(E::StorageIncompatible);
    }
    v9_projection(c)
}

fn v9_projection(c: &Connection) -> ControlResult<Connection> {
    let schema = v9::expected_v9()?;
    let view = Connection::open_in_memory().map_err(db)?;
    view.pragma_update(None, "foreign_keys", false)
        .map_err(db)?;
    for sql in schema
        .values()
        .filter(|sql| sql.starts_with("CREATE TABLE"))
    {
        view.execute_batch(sql).map_err(invalid)?;
    }
    let source_rows = migration::rows(c)?;
    for (name, mut rows) in source_rows {
        if !schema.contains_key(&name) {
            continue;
        }
        if name == "authority_metadata" {
            for row in &mut rows {
                row[1] = Value::Integer(9);
            }
        }
        migration::insert_rows(&view, &name, &rows)?;
    }
    for sql in schema
        .values()
        .filter(|sql| !sql.starts_with("CREATE TABLE"))
    {
        view.execute_batch(sql).map_err(invalid)?;
    }
    view.pragma_update(None, "user_version", 9).map_err(db)?;
    view.pragma_update(None, "foreign_keys", true).map_err(db)?;
    migration::integrity(&view)?;
    Ok(view)
}

fn rebuild_metadata_v10(tx: &rusqlite::Transaction<'_>) -> ControlResult<()> {
    let old: String = tx
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='authority_metadata'",
            [],
            |row| row.get(0),
        )
        .map_err(invalid)?;
    let new = replace_once(&old, "CHECK(schema_version=9)", "CHECK(schema_version=10)")?;
    let mut q = tx
        .prepare("SELECT sql FROM sqlite_master WHERE tbl_name='authority_metadata' AND type IN ('index','trigger') AND sql IS NOT NULL ORDER BY name")
        .map_err(invalid)?;
    let dependents = q
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(invalid)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    drop(q);
    tx.execute_batch("ALTER TABLE authority_metadata RENAME TO authority_metadata_v9_old")
        .map_err(invalid)?;
    tx.execute_batch(&new).map_err(invalid)?;
    tx.execute_batch("INSERT INTO authority_metadata SELECT id,10,stream_id,last_sequence,event_vocabulary FROM authority_metadata_v9_old")
        .map_err(invalid)?;
    tx.execute_batch("DROP TABLE authority_metadata_v9_old")
        .map_err(invalid)?;
    for dependent in dependents {
        tx.execute_batch(&dependent).map_err(invalid)?;
    }
    Ok(())
}

fn replace_once(source: &str, old: &str, new: &str) -> ControlResult<String> {
    if source.matches(old).count() != 1 {
        return Err(E::StorageIncompatible);
    }
    Ok(source.replacen(old, new, 1))
}

fn object_name(sql: &str) -> ControlResult<String> {
    sql.split_whitespace()
        .nth(2)
        .map(|name| name.trim_end_matches('(').to_owned())
        .ok_or(E::StorageIncompatible)
}

fn db(_: impl std::fmt::Debug) -> E {
    E::StorageUnavailable
}
fn invalid(_: impl std::fmt::Debug) -> E {
    E::StorageIncompatible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        control::f8_fixture,
        f8::{
            engine::{Attachment, Containment, Engine, Spawned, Spawner, Worker},
            migration,
            protocol::RuntimeRef,
            registry::Registry,
            staging::LaunchStaging,
        },
    };
    use serde_json::Value as JsonValue;
    use std::{
        fs,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    struct WorkspaceFixture(PathBuf);
    impl WorkspaceFixture {
        fn new() -> Self {
            let base = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .or_else(|| {
                    std::env::current_exe()
                        .ok()?
                        .parent()?
                        .parent()
                        .map(PathBuf::from)
                })
                .expect("cargo target directory is available");
            let base = if base.is_absolute() {
                base
            } else {
                std::env::current_dir().unwrap().join(base)
            };
            let path = base.join(format!("wyss-v10-workspace-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for WorkspaceFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct NoSpawn;
    impl Spawner for NoSpawn {
        fn spawn(
            &mut self,
            _: &RuntimeRef,
            _: Option<&mut LaunchStaging>,
        ) -> ControlResult<Spawned> {
            Err(E::Unsupported)
        }
    }

    #[derive(Clone)]
    struct GuardedFrames(Arc<Mutex<Vec<Vec<u8>>>>);
    struct GuardedWorker(GuardedFrames);
    impl Worker for GuardedWorker {
        fn preflight(&mut self, _: &Attachment) -> ControlResult<()> {
            Ok(())
        }
        fn send(&mut self, frame: &[u8]) -> ControlResult<()> {
            self.0.0.lock().unwrap().push(frame.to_vec());
            Ok(())
        }
        fn send_guarded_start(&mut self, frame: &[u8]) -> ControlResult<()> {
            self.0.0.lock().unwrap().push(frame.to_vec());
            Ok(())
        }
        fn containment(&mut self) -> Containment {
            Containment::PidNamespace
        }
    }
    struct GuardedSpawner(GuardedFrames);
    impl Spawner for GuardedSpawner {
        fn supports_profile(
            &self,
            profile: crate::contracts::control::RuntimeSafetyProfile,
        ) -> bool {
            profile == crate::contracts::control::RuntimeSafetyProfile::CodexLocalToolsV1
        }
        fn descendant_containment(&self) -> bool {
            true
        }
        fn spawn(
            &mut self,
            _: &RuntimeRef,
            _: Option<&mut LaunchStaging>,
        ) -> ControlResult<Spawned> {
            Err(E::Unsupported)
        }
        fn spawn_for_pinned_workspace(
            &mut self,
            _: &RuntimeRef,
            _: Option<&mut LaunchStaging>,
            _: &str,
            _: &str,
            workspace_fd: std::fs::File,
        ) -> ControlResult<Spawned> {
            if !workspace_fd
                .metadata()
                .map_err(|_| E::WorkspaceInvalid)?
                .is_dir()
            {
                return Err(E::WorkspaceInvalid);
            }
            Ok(Spawned {
                worker: Box::new(GuardedWorker(self.0.clone())),
            })
        }
    }

    fn guard_frame(
        authority: &crate::contracts::WireUuid,
        method: &str,
        params: JsonValue,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": uuid::Uuid::new_v4().to_string(),
            "authority_instance_id": authority,
            "method": method,
            "params": params
        }))
        .unwrap()
    }

    fn authority_frame(
        authority: &crate::contracts::WireUuid,
        method: &str,
        params: JsonValue,
    ) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": uuid::Uuid::new_v4().to_string(),
            "authority_instance_id": authority,
            "method": method,
            "params": params
        }))
        .unwrap()
    }

    fn f8_call(
        authority: &mut crate::f8::authority::Authority,
        client: &mut crate::f8::authority::Client,
        spawner: &mut dyn Spawner,
        authority_id: &crate::contracts::WireUuid,
        method: &str,
        params: JsonValue,
    ) -> JsonValue {
        let request = authority_frame(authority_id, method, params);
        let response = authority.handle(client, &request, spawner, std::time::Instant::now());
        serde_json::from_slice(&response).unwrap()
    }

    fn v9_image(sessions: usize) -> Connection {
        let mut source = f8_fixture::v7_with_sessions(sessions);
        let v8 = migration::prepare(&mut source).unwrap().into_connection();
        v9::migrate(&v8, |_| Ok(())).unwrap()
    }

    #[test]
    fn v10_migration_preserves_v9_history_and_fences_legacy_freshness() {
        let source = v9_image(1);
        let before: String = source
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let image = migrate(&source, |_| Ok(())).unwrap();
        assert_eq!(
            source
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            9
        );
        validate_v10(&image).unwrap();
        assert_eq!(
            image
                .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            10
        );
        let after: String = image
            .query_row(
                "SELECT last_sequence FROM authority_metadata WHERE id=1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        let (state, freshness): (String, String) = image
            .query_row(
                "SELECT (SELECT import_state FROM workspace_guard_state WHERE id=1),(SELECT freshness_state FROM managed_session_freshness LIMIT 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(state, "required");
        assert_eq!(freshness, "unproven");
        assert!(
            v9_view(&image).is_ok(),
            "empty guarded state has a safe V9 projection"
        );
    }

    #[test]
    fn interrupted_migration_leaves_source_at_v9_and_never_publishes_a_mixed_image() {
        let source = v9_image(0);
        for failing_at in [
            Stage::Metadata,
            Stage::GuardObjects,
            Stage::Freshness,
            Stage::Boundary,
        ] {
            let result = migrate(&source, |stage| {
                if stage == failing_at {
                    Err(E::StorageUnavailable)
                } else {
                    Ok(())
                }
            });
            assert!(result.is_err());
            assert_eq!(
                source
                    .pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
                    .unwrap(),
                9
            );
            v9::validate_v9(&source).unwrap();
        }
        let image = migrate(&source, |_| Ok(())).unwrap();
        validate_v10(&image).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn authority_ledger_blocks_conflicts_preserves_independent_roots_and_quarantines_on_restart() {
        use crate::f8::workspace_guard::WorkspaceGuard;
        const COORDINATOR: &str = "00000000-0000-4000-8000-000000000010";
        const EPOCH1: &str = "00000000-0000-4000-8000-000000000011";
        const EPOCH2: &str = "00000000-0000-4000-8000-000000000012";
        const RUN1: &str = "00000000-0000-4000-8000-000000000013";
        const RUN2: &str = "00000000-0000-4000-8000-000000000014";
        let fixture = WorkspaceFixture::new();
        let first_path = fixture.path("first");
        let second_path = fixture.path("second");
        fs::create_dir(&first_path).unwrap();
        fs::create_dir(&second_path).unwrap();
        let mut conn = migrate(&v9_image(0), |_| Ok(())).unwrap();
        let mut guard = WorkspaceGuard::open(&mut conn, EPOCH1).unwrap();
        assert!(!guard.capability_ready(&conn).unwrap());
        let digest = guard.import_claims(&mut conn, COORDINATOR, 1, &[]).unwrap();
        assert_eq!(
            guard.import_claims(&mut conn, COORDINATOR, 1, &[]).unwrap(),
            digest
        );
        assert!(guard.capability_ready(&conn).unwrap());
        let root1 = guard.observe_root(&mut conn, &first_path).unwrap();
        let root2 = guard.observe_root(&mut conn, &second_path).unwrap();
        let (selected_root, pinned_fd) = guard
            .duplicate_root_fd_for_locator(&conn, &first_path)
            .unwrap();
        assert_eq!(selected_root, root1);
        assert!(pinned_fd.metadata().unwrap().is_dir());
        drop(pinned_fd);
        let plan_digest = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let claim1 = guard
            .acquire_claim(&mut conn, COORDINATOR, RUN1, 1, plan_digest, &root1)
            .unwrap();
        assert_eq!(claim1.phase, "held");
        let moved_path = fixture.path("first-moved");
        fs::rename(&first_path, &moved_path).unwrap();
        fs::create_dir(&first_path).unwrap();
        assert!(
            guard
                .duplicate_root_fd_for_locator(&conn, &first_path)
                .is_err()
        );
        assert!(
            guard
                .acquire_claim(&mut conn, COORDINATOR, RUN2, 1, plan_digest, &root1)
                .is_err()
        );
        let independent = guard
            .acquire_claim(&mut conn, COORDINATOR, RUN2, 1, plan_digest, &root2)
            .unwrap();
        assert_ne!(claim1.claim_id, independent.claim_id);
        let paused = guard
            .set_gate(&mut conn, &claim1.claim_id, 1, 1, "paused", None, None)
            .unwrap();
        assert_eq!(
            (paused.phase.as_str(), paused.gate_state.as_str()),
            ("held", "paused")
        );
        let count_before: i64 = conn
            .query_row(
                "SELECT count(*) FROM workspace_claims WHERE phase='held'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count_before, 2,
            "pause must not release workspace ownership"
        );

        drop(guard);
        let restarted = WorkspaceGuard::open(&mut conn, EPOCH2).unwrap();
        assert!(!restarted.capability_ready(&conn).unwrap());
        let (phase_count, root_state): (i64, String) = conn.query_row(
            "SELECT (SELECT count(*) FROM workspace_claims WHERE phase='quarantined'),(SELECT proof_state FROM workspace_root_bindings WHERE root_binding_id=?1)",
            [&root1],
            |r| Ok((r.get(0)?, r.get(1)?)),
        ).unwrap();
        assert_eq!(phase_count, 2);
        assert_eq!(root_state, "identity_unproven");
        assert!(
            restarted.duplicate_root_fd(&root1).is_err(),
            "a restarted authority must not reopen the old root by path"
        );
        validate_v10(&conn).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn nonempty_legacy_import_remains_unproven_and_disables_new_root_admission() {
        use crate::f8::workspace_guard::{ImportedClaim, WorkspaceGuard};
        const COORDINATOR: &str = "00000000-0000-4000-8000-000000000020";
        const EPOCH: &str = "00000000-0000-4000-8000-000000000021";
        let fixture = WorkspaceFixture::new();
        let path = fixture.path("workspace");
        fs::create_dir(&path).unwrap();
        let mut conn = migrate(&v9_image(0), |_| Ok(())).unwrap();
        let mut guard = WorkspaceGuard::open(&mut conn, EPOCH).unwrap();
        let item = ImportedClaim {
            claim_id: "00000000-0000-4000-8000-000000000022".to_owned(),
            run_id: "00000000-0000-4000-8000-000000000023".to_owned(),
            claim_generation: 1,
            source_revision: 7,
        };
        guard
            .import_claims(&mut conn, COORDINATOR, 8, std::slice::from_ref(&item))
            .unwrap();
        assert!(!guard.capability_ready(&conn).unwrap());
        assert_eq!(
            conn.query_row(
                "SELECT identity_state FROM workspace_imported_claims",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "identity_unproven"
        );
        assert!(guard.observe_root(&mut conn, &path).is_err());
        validate_v10(&conn).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn authority_protocol_import_observe_and_claim_use_the_real_v10_guard() {
        use crate::{
            contracts::WireUuid,
            f8::{
                authority::{Authority, Client},
                guard_protocol::response_value,
            },
        };
        let fixture = WorkspaceFixture::new();
        let root = fixture.path("workspace");
        fs::create_dir(&root).unwrap();
        let connection = migrate(&v9_image(0), |_| Ok(())).unwrap();
        let authority_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let registry = Registry::with_history(
            authority_id.clone(),
            Vec::<String>::new(),
            Vec::<(String, u64)>::new(),
        )
        .unwrap();
        let engine = Engine::new(connection, authority_id.clone(), registry, None).unwrap();
        let mut authority = Authority::new_guarded(engine).unwrap();
        let mut client = Client::default();
        let mut spawner = NoSpawn;
        let hello_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let hello = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": hello_id,
            "method": "authority/hello",
            "params": {"protocol_version": 2, "client_instance_id": uuid::Uuid::new_v4().to_string()}
        })).unwrap();
        let response =
            authority.handle(&mut client, &hello, &mut spawner, std::time::Instant::now());
        crate::f8::protocol::Response::parse(&response).unwrap();

        let capabilities = authority.handle(
            &mut client,
            &guard_frame(
                &authority_id,
                "authority/capabilities",
                serde_json::json!({}),
            ),
            &mut spawner,
            std::time::Instant::now(),
        );
        let capability = response_value(
            &capabilities,
            &WireUuid::try_from(
                serde_json::from_slice::<JsonValue>(&capabilities).unwrap()["request_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            )
            .unwrap(),
            &authority_id,
        )
        .unwrap();
        assert_eq!(capability["ready"], false);
        let coordinator = "00000000-0000-4000-8000-000000000030";
        let imported = authority.handle(&mut client, &guard_frame(&authority_id, "orchestration/claims/import", serde_json::json!({"coordinator_store_id":coordinator,"snapshot_revision":1,"claims":[]})), &mut spawner, std::time::Instant::now());
        let imported_id = WireUuid::try_from(
            serde_json::from_slice::<JsonValue>(&imported).unwrap()["request_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap();
        let imported_value = response_value(&imported, &imported_id, &authority_id).unwrap();
        assert_eq!(imported_value["guard_ready"], true);
        assert_eq!(imported_value["managed_execution_enabled"], false);
        assert_eq!(imported_value["ready"], false);
        let observed = authority.handle(
            &mut client,
            &guard_frame(
                &authority_id,
                "orchestration/root/observe",
                serde_json::json!({"locator":root}),
            ),
            &mut spawner,
            std::time::Instant::now(),
        );
        let observed_value = serde_json::from_slice::<JsonValue>(&observed).unwrap();
        let observed_id =
            WireUuid::try_from(observed_value["request_id"].as_str().unwrap().to_owned()).unwrap();
        let root_binding_id =
            response_value(&observed, &observed_id, &authority_id).unwrap()["root_binding_id"]
                .as_str()
                .unwrap()
                .to_owned();
        let observed_again = authority.handle(
            &mut client,
            &guard_frame(
                &authority_id,
                "orchestration/root/observe",
                serde_json::json!({"locator":root}),
            ),
            &mut spawner,
            std::time::Instant::now(),
        );
        let observed_again_value = serde_json::from_slice::<JsonValue>(&observed_again).unwrap();
        let observed_again_id = WireUuid::try_from(
            observed_again_value["request_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap();
        assert_eq!(
            response_value(&observed_again, &observed_again_id, &authority_id).unwrap()["root_binding_id"],
            root_binding_id,
            "same live root observation must reuse the immutable pin"
        );
        let run_id = "00000000-0000-4000-8000-000000000031";
        let acquire = || {
            guard_frame(
                &authority_id,
                "orchestration/claim/acquire",
                serde_json::json!({"coordinator_store_id":coordinator,"run_id":run_id,"plan_revision":1,"plan_digest":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","root_binding_id":root_binding_id}),
            )
        };
        let claimed = authority.handle(
            &mut client,
            &acquire(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let claimed_value = serde_json::from_slice::<JsonValue>(&claimed).unwrap();
        let claimed_id =
            WireUuid::try_from(claimed_value["request_id"].as_str().unwrap().to_owned()).unwrap();
        assert_eq!(
            response_value(&claimed, &claimed_id, &authority_id).unwrap()["phase"],
            "held"
        );
        let conflict = authority.handle(&mut client, &guard_frame(&authority_id, "orchestration/claim/acquire", serde_json::json!({"coordinator_store_id":coordinator,"run_id":"00000000-0000-4000-8000-000000000032","plan_revision":1,"plan_digest":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","root_binding_id":root_binding_id})), &mut spawner, std::time::Instant::now());
        let conflict_value = serde_json::from_slice::<JsonValue>(&conflict).unwrap();
        assert!(
            conflict_value.get("error").is_some(),
            "conflicting claim must be refused by F8"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn authority_guarded_commit_consumes_once_and_uses_nonbuffering_start() {
        use crate::{
            contracts::{
                WireUuid,
                execution::ExecutionScope,
                identities::ManagedTurnId,
                turn::{TurnReason, TurnState},
            },
            f8::{
                authority::{Authority, Client},
                guard_protocol::{
                    StartEffect, StartProfile, StartPurpose, StartTuple, response_value,
                },
            },
        };
        use sha2::Digest;

        let workspace_fixture = WorkspaceFixture::new();
        let workspace = workspace_fixture.path("bound-root");
        let mut source = f8_fixture::v7_with_sessions_at(1, workspace.clone());
        let v8 = migration::prepare(&mut source).unwrap().into_connection();
        let legacy = v9::migrate(&v8, |_| Ok(())).unwrap();
        let (session, workspace): (String, String) = legacy
            .query_row(
                "SELECT s.session_id,b.workspace_root FROM managed_sessions s JOIN managed_session_bindings b ON b.managed_session_id=s.session_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(
            PathBuf::from(&workspace),
            workspace_fixture.path("bound-root")
        );
        fs::create_dir_all(&workspace).unwrap();
        let connection = migrate(&legacy, |_| Ok(())).unwrap();
        let authority_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let registry = Registry::with_history(
            authority_id.clone(),
            Vec::<String>::new(),
            Vec::<(String, u64)>::new(),
        )
        .unwrap();
        let engine = Engine::new(connection, authority_id.clone(), registry, None).unwrap();
        let mut authority = Authority::new_guarded(engine).unwrap();
        // This fixture is a freshly prepared test session. The legacy migration test separately
        // asserts that untouched sessions remain `unproven` and cannot use this path.
        authority
            .engine_mut()
            .connection_mut()
            .execute(
                "UPDATE managed_session_freshness SET freshness_state='proven',authority_epoch=?1,preparation_generation=1,ever_started=0 WHERE session_id=?2",
                rusqlite::params![authority_id.as_str(), session],
            )
            .unwrap();
        let frames = GuardedFrames(Arc::default());
        let mut spawner = GuardedSpawner(frames.clone());
        let mut client = Client::default();

        let hello = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": uuid::Uuid::new_v4().to_string(),
            "method": "authority/hello",
            "params": {"protocol_version": 2, "client_instance_id": uuid::Uuid::new_v4().to_string()}
        }))
        .unwrap();
        let hello_response =
            authority.handle(&mut client, &hello, &mut spawner, std::time::Instant::now());
        crate::f8::protocol::Response::parse(&hello_response).unwrap();

        let coordinator = "00000000-0000-4000-8000-000000000030";
        let imported = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claims/import",
            serde_json::json!({"coordinator_store_id":coordinator,"snapshot_revision":1,"claims":[]}),
        );
        assert!(imported.get("error").is_none(), "{imported}");

        let capability = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "authority/capabilities",
            serde_json::json!({}),
        );
        let capability_id =
            WireUuid::try_from(capability["request_id"].as_str().unwrap().to_owned()).unwrap();
        let capability = response_value(
            &serde_json::to_vec(&capability).unwrap(),
            &capability_id,
            &authority_id,
        )
        .unwrap();
        assert_eq!(capability["guard_ready"], true);
        assert_eq!(capability["ready"], false);
        assert_eq!(capability["managed_execution_enabled"], false);

        let observed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/root/observe",
            serde_json::json!({"locator":workspace}),
        );
        assert!(observed.get("error").is_none(), "{observed}");
        let root_binding_id = observed["result"]["value"]["root_binding_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let run_id = uuid::Uuid::new_v4().to_string();
        let plan_digest = "a".repeat(64);
        let claim = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/acquire",
            serde_json::json!({"coordinator_store_id":coordinator,"run_id":run_id,"plan_revision":1,"plan_digest":plan_digest,"root_binding_id":root_binding_id}),
        );
        assert!(claim.get("error").is_none(), "{claim}");
        let claim = claim["result"]["value"].clone();

        let (generation, revision): (u64, u64) = authority
            .engine()
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT generation FROM managed_runtimes WHERE managed_session_id=?1),0),COALESCE((SELECT revision FROM managed_runtimes WHERE managed_session_id=?1),0)",
                [&session],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .unwrap();
        let started = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "runtime/start",
            serde_json::json!({
                "intent_id":uuid::Uuid::new_v4().to_string(),"adapter_id":"codex","runtime_kind":"codex",
                "managed_session_id":session,"expected_generation":generation,"expected_revision":revision,
                "safety_profile":"codex_local_tools_v1","workspace_root":workspace
            }),
        );
        assert!(started.get("error").is_none(), "{started}");
        let reference = authority.engine().reference(&session).unwrap();
        authority
            .engine_mut()
            .complete_handshake(
                &reference,
                "0.159.0",
                Attachment {
                    workspace: workspace.clone(),
                    profile: "codex_local_tools_v1".into(),
                    revision: 1,
                },
            )
            .unwrap();
        let lease = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "executionLease/grant",
            serde_json::json!({
                "intent_id":uuid::Uuid::new_v4().to_string(),"session_id":session,"expected_session_revision":1,
                "scopes":[ExecutionScope::SessionResume,ExecutionScope::TurnStart],"ttl_seconds":600,"runtime_fence":reference.fence()
            }),
        );
        assert!(lease.get("error").is_none(), "{lease}");
        let lease_id = lease["result"]["value"]["lease_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let lease_revision = lease["result"]["value"]["revision"].as_u64().unwrap();
        let resume_id = uuid::Uuid::new_v4().to_string();
        let resumed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "managedSession/runtime/resume",
            serde_json::json!({"intent_id":resume_id,"managed_session_id":session,"expected_session_revision":1,"runtime_fence":reference.fence()}),
        );
        assert_eq!(
            resumed["error"]["code"], "runtime_operation_in_progress",
            "{resumed}"
        );
        let thread: String = authority
            .engine()
            .connection()
            .query_row(
                "SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        crate::f8::operations::confirm_binding(
            authority.engine_mut(),
            &reference,
            &WireUuid::try_from(resume_id).unwrap(),
            &thread,
        )
        .unwrap();
        let binding_revision: u64 = authority
            .engine()
            .connection()
            .query_row(
                "SELECT binding_revision FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |row| row.get::<_, i64>(0).map(|v| v as u64),
            )
            .unwrap();

        let prompt = "synthetic guarded test prompt";
        let intent_id = uuid::Uuid::new_v4().to_string();
        let tuple = StartTuple {
            authority_store_id: capability["authority_store_id"]
                .as_str()
                .unwrap()
                .to_owned(),
            authority_epoch: authority_id.as_str().to_owned(),
            coordinator_store_id: coordinator.into(),
            run_id,
            plan_revision: 1,
            plan_digest,
            step_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: uuid::Uuid::new_v4().to_string(),
            intent_id,
            effect_class: StartEffect::ManagedLocalTools,
            prompt_digest: format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())),
            model_id: None,
            model_profile: StartProfile::CodexLocalToolsV1,
            root_binding_id,
            claim_id: claim["claim_id"].as_str().unwrap().to_owned(),
            claim_generation: claim["claim_generation"].as_u64().unwrap(),
            claim_revision: claim["claim_revision"].as_u64().unwrap(),
            root_observation_revision: 1,
            dispatch_gate_revision: claim["gate_revision"].as_u64().unwrap(),
            session_id: session.clone(),
            session_revision: 1,
            attachment_revision: binding_revision,
            runtime_instance_id: reference.runtime_instance_id.as_str().into(),
            runtime_generation: reference.generation,
            lease_id,
            lease_revision,
            approval_reference: None,
            purpose: StartPurpose::PipelineStep,
        };
        let prepare = || {
            guard_frame(
                &authority_id,
                "orchestration/start/prepare",
                serde_json::json!({"tuple":tuple}),
            )
        };
        let disabled = authority.handle(
            &mut client,
            &prepare(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let disabled_id = WireUuid::try_from(
            serde_json::from_slice::<JsonValue>(&disabled).unwrap()["request_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        )
        .unwrap();
        assert!(
            response_value(&disabled, &disabled_id, &authority_id).is_err(),
            "disabled Managed execution must fail closed"
        );
        authority.fixture_enable_guarded_execution();
        let prepared = authority.handle(
            &mut client,
            &prepare(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let prepared_json: JsonValue = serde_json::from_slice(&prepared).unwrap();
        let prepared_id =
            WireUuid::try_from(prepared_json["request_id"].as_str().unwrap().to_owned()).unwrap();
        let prepared = response_value(&prepared, &prepared_id, &authority_id).unwrap();
        let receipt_id = prepared["receipt_id"].as_str().unwrap().to_owned();
        let receipt_revision = prepared["receipt_revision"].as_u64().unwrap();
        let commit_params = serde_json::json!({"tuple":tuple,"receipt_id":receipt_id,"receipt_revision":receipt_revision,"prompt":prompt});
        let commit_frame = || {
            guard_frame(
                &authority_id,
                "orchestration/start/commit",
                commit_params.clone(),
            )
        };
        let first = authority.handle(
            &mut client,
            &commit_frame(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let first_json: JsonValue = serde_json::from_slice(&first).unwrap();
        let diagnostic: (i64, i64, i64) = authority.engine().connection().query_row(
            "SELECT (SELECT count(*) FROM managed_turns WHERE state='starting'),(SELECT count(*) FROM guarded_attempts),(SELECT count(*) FROM guarded_start_receipts WHERE state='consumed')",
            [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(
            diagnostic,
            (1, 1, 1),
            "L1 must reserve exactly one turn, attempt and consumed receipt"
        );
        assert!(
            first_json.get("error").is_none(),
            "guarded commit failed: {first_json}"
        );
        let first_id =
            WireUuid::try_from(first_json["request_id"].as_str().unwrap().to_owned()).unwrap();
        let first_value = response_value(&first, &first_id, &authority_id).unwrap();
        assert_eq!(first_value["replayed"], false, "{first_value}");
        assert_eq!(first_value["kind"], "turn");
        let sends: Vec<Vec<u8>> = frames.0.lock().unwrap().clone();
        let starts = sends
            .iter()
            .filter(|bytes| {
                serde_json::from_slice::<JsonValue>(bytes)
                    .is_ok_and(|v| v["method"] == "turn/start")
            })
            .count();
        assert_eq!(
            starts, 1,
            "guarded start must use exactly one provider write"
        );
        let (receipt_state, attempt_state, claim_phase): (String, String, String) = authority
            .engine()
            .connection()
            .query_row(
                "SELECT r.state,a.send_state,c.phase FROM guarded_start_receipts r JOIN guarded_attempts a ON a.receipt_id=r.receipt_id JOIN workspace_claims c ON c.claim_id=a.claim_id WHERE r.receipt_id=?1",
                [&receipt_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (
                receipt_state.as_str(),
                attempt_state.as_str(),
                claim_phase.as_str()
            ),
            ("consumed", "possibly_sent", "unsettled")
        );

        let replay = authority.handle(
            &mut client,
            &commit_frame(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let replay_json: JsonValue = serde_json::from_slice(&replay).unwrap();
        let replay_id =
            WireUuid::try_from(replay_json["request_id"].as_str().unwrap().to_owned()).unwrap();
        let replay_value = response_value(&replay, &replay_id, &authority_id).unwrap();
        assert_eq!(replay_value["replayed"], true, "{replay_value}");
        let sends_after_replay = frames
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|bytes| {
                serde_json::from_slice::<JsonValue>(bytes)
                    .is_ok_and(|v| v["method"] == "turn/start")
            })
            .count();
        assert_eq!(
            sends_after_replay, 1,
            "receipt replay must never write provider bytes"
        );

        let ordinary = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "managedSession/turn/start",
            serde_json::json!({"intent_id":uuid::Uuid::new_v4().to_string(),"managed_session_id":session,"expected_session_revision":1,"prompt":"must not bypass guard","runtime_fence":reference.fence()}),
        );
        assert_eq!(
            ordinary["error"]["code"], "gate_denied",
            "ordinary V10 TurnStart must not bypass the guard"
        );
        let sends_after_ordinary = frames
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|bytes| {
                serde_json::from_slice::<JsonValue>(bytes)
                    .is_ok_and(|v| v["method"] == "turn/start")
            })
            .count();
        assert_eq!(sends_after_ordinary, 1);

        let (claim_revision, gate_revision, claim_generation): (u64, u64, u64) = authority
            .engine()
            .connection()
            .query_row(
                "SELECT claim_revision,gate_revision,claim_generation FROM workspace_claims WHERE claim_id=?1",
                [&tuple.claim_id],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64)),
            )
            .unwrap();
        let early_release = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/release",
            serde_json::json!({
                "claim_id":tuple.claim_id,
                "claim_generation":claim_generation,
                "expected_claim_revision":claim_revision,
                "settlement_id":uuid::Uuid::new_v4().to_string(),
                "coordinator_revision":1
            }),
        );
        assert!(
            early_release.get("error").is_some(),
            "an unsettled effect cannot release its workspace claim"
        );

        let turn_id = ManagedTurnId::parse(
            &authority
                .engine()
                .connection()
                .query_row(
                    "SELECT turn_id FROM guarded_attempts WHERE attempt_id=?1",
                    [&tuple.attempt_id],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
        )
        .unwrap();
        authority
            .provider_turn_event(
                &reference,
                &turn_id,
                Some("synthetic-provider-turn"),
                TurnState::Running,
                None,
            )
            .unwrap();
        authority
            .provider_turn_event(
                &reference,
                &turn_id,
                Some("synthetic-provider-turn"),
                TurnState::Completed,
                Some(TurnReason::Completed),
            )
            .unwrap();
        let attempt_state: String = authority
            .engine()
            .connection()
            .query_row(
                "SELECT send_state FROM guarded_attempts WHERE attempt_id=?1",
                [&tuple.attempt_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attempt_state, "verifying");

        let reviewed_digest =
            crate::orchestration::workspace_revision(std::path::Path::new(&workspace)).unwrap();
        let observation_id = uuid::Uuid::new_v4().to_string();
        let prepare_verification = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/verification/prepare",
            serde_json::json!({
                "observation_id":observation_id,
                "claim_id":tuple.claim_id,
                "claim_generation":claim_generation,
                "expected_claim_revision":claim_revision,
                "root_binding_id":tuple.root_binding_id,
                "attempt_id":tuple.attempt_id,
                "locator":workspace,
                "reviewed_digest":reviewed_digest
            }),
        );
        assert!(
            prepare_verification.get("error").is_none(),
            "{prepare_verification}"
        );
        assert_eq!(
            prepare_verification["result"]["value"]["content_digest"],
            reviewed_digest
        );
        let commit_verification = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/verification/commit",
            serde_json::json!({
                "claim_id":tuple.claim_id,
                "claim_generation":claim_generation,
                "expected_claim_revision":claim_revision,
                "observation_id":observation_id,
                "attempt_id":tuple.attempt_id,
                "locator":workspace,
                "decision":"confirmed_completed",
                "reviewed_digest":reviewed_digest
            }),
        );
        assert!(
            commit_verification.get("error").is_none(),
            "{commit_verification}"
        );
        assert_eq!(commit_verification["result"]["value"]["phase"], "verified");
        let settlement_id = commit_verification["result"]["value"]["settlement_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let verified_revision = commit_verification["result"]["value"]["claim_revision"]
            .as_u64()
            .unwrap();
        let release = serde_json::json!({
            "claim_id":tuple.claim_id,
            "claim_generation":claim_generation,
            "expected_claim_revision":verified_revision,
            "settlement_id":settlement_id,
            "coordinator_revision":1
        });
        let released = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/release",
            release.clone(),
        );
        assert!(released.get("error").is_none(), "{released}");
        assert_eq!(released["result"]["value"]["phase"], "released");
        let release_replay = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/release",
            release,
        );
        assert!(release_replay.get("error").is_none(), "{release_replay}");
        assert_eq!(release_replay["result"]["value"]["phase"], "released");
        assert_eq!(
            release_replay["result"]["value"]["claim_revision"],
            released["result"]["value"]["claim_revision"],
            "lost release acknowledgements are recovered by status, without a second transition"
        );
        assert_eq!(gate_revision, 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn coordinator_uses_live_f8_guard_and_waits_for_reconciliation_before_next_effect() {
        use crate::{
            contracts::{
                WireUuid,
                control::{Operation, TurnStart},
                execution::ExecutionScope,
                identities::{ManagedSessionId, ManagedTurnId},
                turn::{TurnReason, TurnState},
            },
            f8::{
                authority::{Authority, Client},
                protocol::{Request, RuntimeRef, Target},
            },
            orchestration::{
                Coordinator, EffectClass, ManagedAuthority, OrchestrationPlanV1, PlanLimitsV1,
                PlanReviewV1, ProviderLane, RunJournal, StepPlanV1, StopPolicy, workspace_identity,
                workspace_revision,
            },
        };
        use std::{
            os::unix::fs::PermissionsExt,
            os::unix::net::UnixListener,
            sync::{
                atomic::{AtomicBool, Ordering},
                mpsc,
            },
            thread,
            time::Duration,
        };

        let fixture = WorkspaceFixture::new();
        let workspace = fixture.path("coordinator-bound-root");
        let journal_path = fixture.path("coordinator-journal.sqlite3");
        let journal = Arc::new(RunJournal::open(&journal_path).unwrap());
        let coordinator_store_id = journal.coordinator_store_id().unwrap();

        let mut source = f8_fixture::v7_with_sessions_at(1, workspace.clone());
        let v8 = migration::prepare(&mut source).unwrap().into_connection();
        let legacy = v9::migrate(&v8, |_| Ok(())).unwrap();
        let (session, bound_workspace): (String, String) = legacy
            .query_row(
                "SELECT s.session_id,b.workspace_root FROM managed_sessions s JOIN managed_session_bindings b ON b.managed_session_id=s.session_id LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(PathBuf::from(&bound_workspace), workspace);
        let connection = migrate(&legacy, |_| Ok(())).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("identity"), b"same workspace bytes").unwrap();
        let authority_id = WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap();
        let registry = Registry::with_history(
            authority_id.clone(),
            Vec::<String>::new(),
            Vec::<(String, u64)>::new(),
        )
        .unwrap();
        let engine = Engine::new(connection, authority_id.clone(), registry, None).unwrap();
        let mut authority = Authority::new_guarded(engine).unwrap();
        authority
            .engine_mut()
            .connection_mut()
            .execute(
                "UPDATE managed_session_freshness SET freshness_state='proven',authority_epoch=?1,preparation_generation=1,ever_started=0 WHERE session_id=?2",
                rusqlite::params![authority_id.as_str(), session],
            )
            .unwrap();
        // Production remains default-off. This test-only gate enables the real protocol path
        // against a local Authority and a worker that records, but never executes, provider data.
        authority.fixture_enable_guarded_execution();
        let frames = GuardedFrames(Arc::default());
        let mut spawner = GuardedSpawner(frames.clone());
        let mut client = Client::default();
        let hello = serde_json::to_vec(&serde_json::json!({
            "schema_version": 2,
            "request_id": uuid::Uuid::new_v4().to_string(),
            "method": "authority/hello",
            "params": {"protocol_version": 2, "client_instance_id": uuid::Uuid::new_v4().to_string()}
        }))
        .unwrap();
        let hello_response =
            authority.handle(&mut client, &hello, &mut spawner, std::time::Instant::now());
        crate::f8::protocol::Response::parse(&hello_response).unwrap();

        let imported = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claims/import",
            serde_json::json!({
                "coordinator_store_id": coordinator_store_id,
                "snapshot_revision": 1,
                "claims": []
            }),
        );
        assert!(imported.get("error").is_none(), "{imported}");
        let observed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/root/observe",
            serde_json::json!({"locator": workspace}),
        );
        assert!(observed.get("error").is_none(), "{observed}");
        let root_binding_id = observed["result"]["value"]["root_binding_id"]
            .as_str()
            .unwrap()
            .to_owned();

        let (generation, revision): (u64, u64) = authority
            .engine()
            .connection()
            .query_row(
                "SELECT COALESCE((SELECT generation FROM managed_runtimes WHERE managed_session_id=?1),0),COALESCE((SELECT revision FROM managed_runtimes WHERE managed_session_id=?1),0)",
                [&session],
                |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64)),
            )
            .unwrap();
        let started = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "runtime/start",
            serde_json::json!({
                "intent_id": uuid::Uuid::new_v4().to_string(),
                "adapter_id": "codex",
                "runtime_kind": "codex",
                "managed_session_id": session,
                "expected_generation": generation,
                "expected_revision": revision,
                "safety_profile": "codex_local_tools_v1",
                "workspace_root": workspace
            }),
        );
        assert!(started.get("error").is_none(), "{started}");
        let reference = authority.engine().reference(&session).unwrap();
        authority
            .engine_mut()
            .complete_handshake(
                &reference,
                "0.159.0",
                Attachment {
                    workspace: workspace.to_string_lossy().into_owned(),
                    profile: "codex_local_tools_v1".into(),
                    revision: 1,
                },
            )
            .unwrap();
        let lease = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "executionLease/grant",
            serde_json::json!({
                "intent_id": uuid::Uuid::new_v4().to_string(),
                "session_id": session,
                "expected_session_revision": 1,
                "scopes": [ExecutionScope::SessionResume, ExecutionScope::TurnStart],
                "ttl_seconds": 600,
                "runtime_fence": reference.fence()
            }),
        );
        assert!(lease.get("error").is_none(), "{lease}");
        let resume_id = uuid::Uuid::new_v4().to_string();
        let resumed = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "managedSession/runtime/resume",
            serde_json::json!({
                "intent_id": resume_id,
                "managed_session_id": session,
                "expected_session_revision": 1,
                "runtime_fence": reference.fence()
            }),
        );
        assert_eq!(
            resumed["error"]["code"], "runtime_operation_in_progress",
            "{resumed}"
        );
        let runtime_thread: String = authority
            .engine()
            .connection()
            .query_row(
                "SELECT runtime_session_id FROM managed_session_bindings WHERE managed_session_id=?1",
                [&session],
                |row| row.get(0),
            )
            .unwrap();
        crate::f8::operations::confirm_binding(
            authority.engine_mut(),
            &reference,
            &WireUuid::try_from(resume_id).unwrap(),
            &runtime_thread,
        )
        .unwrap();

        let ordinary_request = Request::new(
            WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
            Some(authority_id.clone()),
            Operation::TurnStart(TurnStart {
                intent_id: WireUuid::try_from(uuid::Uuid::new_v4().to_string()).unwrap(),
                managed_session_id: ManagedSessionId::parse(&session).unwrap(),
                expected_session_revision: 1,
                prompt: "synthetic independent manual turn".into(),
            }),
            Target::Fenced(reference.fence()),
        )
        .unwrap();
        let ordinary_response = authority.handle(
            &mut client,
            &ordinary_request.encode().unwrap(),
            &mut spawner,
            std::time::Instant::now(),
        );
        let ordinary_response = crate::f8::protocol::Response::parse(&ordinary_response).unwrap();
        let manual_turn_id = match ordinary_response.outcome().as_ref().unwrap() {
            crate::contracts::control::ResultValue::Turn(turn) => turn.managed_turn_id.clone(),
            other => panic!("ordinary turn returned unexpected result: {other:?}"),
        };
        let conflicting_claim = f8_call(
            &mut authority,
            &mut client,
            &mut spawner,
            &authority_id,
            "orchestration/claim/acquire",
            serde_json::json!({
                "coordinator_store_id": coordinator_store_id,
                "run_id": uuid::Uuid::new_v4().to_string(),
                "plan_revision": 1,
                "plan_digest": "c".repeat(64),
                "root_binding_id": root_binding_id
            }),
        );
        assert!(
            conflicting_claim.get("error").is_some(),
            "F8 cannot admit an M6 workspace claim while an overlapping ordinary turn is active: {conflicting_claim}"
        );
        assert_eq!(
            authority
                .engine()
                .connection()
                .query_row("SELECT count(*) FROM workspace_claims", [], |row| row
                    .get::<_, i64>(0),)
                .unwrap(),
            0,
            "a rejected overlapping admission must not leave a claim behind"
        );
        authority
            .provider_turn_event(
                &reference,
                &manual_turn_id,
                Some("synthetic-manual-provider-turn"),
                TurnState::Running,
                None,
            )
            .unwrap();
        authority
            .provider_turn_event(
                &reference,
                &manual_turn_id,
                Some("synthetic-manual-provider-turn"),
                TurnState::Completed,
                Some(TurnReason::Completed),
            )
            .unwrap();
        assert_eq!(
            authority
                .engine()
                .connection()
                .query_row(
                    "SELECT ever_started FROM managed_session_freshness WHERE session_id=?1",
                    [&session],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1,
            "ordinary V10 admission permanently consumes receiver freshness"
        );

        let runtime_root = std::env::temp_dir().join(format!("wyss-f8-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&runtime_root).unwrap();
        fs::set_permissions(&runtime_root, fs::Permissions::from_mode(0o700)).unwrap();
        let product = runtime_root.join("wyss-console");
        fs::create_dir(&product).unwrap();
        fs::set_permissions(&product, fs::Permissions::from_mode(0o700)).unwrap();
        let socket_path = product.join("control.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = stop.clone();
        let (complete_tx, complete_rx) = mpsc::channel::<(String, mpsc::Sender<()>)>();
        let server_reference: RuntimeRef = reference.clone();
        let server = thread::spawn(move || {
            use crate::control::framing::{read_frame, write_frame};

            listener.set_nonblocking(true).unwrap();
            while !server_stop.load(Ordering::Acquire) {
                if let Ok((turn_id, ack)) = complete_rx.try_recv() {
                    let turn_id = ManagedTurnId::parse(&turn_id).unwrap();
                    authority
                        .provider_turn_event(
                            &server_reference,
                            &turn_id,
                            Some("synthetic-provider-turn"),
                            TurnState::Running,
                            None,
                        )
                        .unwrap();
                    authority
                        .provider_turn_event(
                            &server_reference,
                            &turn_id,
                            Some("synthetic-provider-turn"),
                            TurnState::Completed,
                            Some(TurnReason::Completed),
                        )
                        .unwrap();
                    let _ = ack.send(());
                }
                let (mut stream, _) = match listener.accept() {
                    Ok(pair) => pair,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("fixture F8 listener failed: {error}"),
                };
                stream.set_nonblocking(true).unwrap();
                let mut client = Client::default();
                while let Ok(Some(bytes)) = read_frame(
                    &mut stream,
                    std::time::Instant::now() + Duration::from_secs(10),
                    &server_stop,
                ) {
                    let response = authority.handle(
                        &mut client,
                        &bytes,
                        &mut spawner,
                        std::time::Instant::now(),
                    );
                    if write_frame(
                        &mut stream,
                        &response,
                        std::time::Instant::now() + Duration::from_secs(10),
                        &server_stop,
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            }
        });

        let bridge = Arc::new(crate::orchestration::F8Authority::new(&runtime_root));
        let authority_trait: Arc<dyn crate::orchestration::ManagedAuthority> = bridge.clone();
        let coordinator = Coordinator::new(journal.clone(), authority_trait.clone(), 100).unwrap();
        let plan = OrchestrationPlanV1 {
            schema_version: 1,
            plan_id: uuid::Uuid::new_v4().to_string(),
            revision: 1,
            host_id: "local".into(),
            project_id: "aud01-f8-integration".into(),
            workspace_id: workspace_identity(&workspace).unwrap(),
            workspace_root: workspace.to_string_lossy().into_owned(),
            workspace_revision: workspace_revision(&workspace).unwrap(),
            objective: "two synthetic managed turns separated by F8 review".into(),
            created_at_unix: 100,
            expires_at_unix: 400,
            limits: PlanLimitsV1 {
                max_steps: 2,
                max_parallelism: 1,
                max_total_seconds: 300,
                stop_policy: StopPolicy::StopScheduling,
            },
            steps: [
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa2",
            ]
            .into_iter()
            .map(|step_id| StepPlanV1 {
                step_id: step_id.into(),
                dependencies: Vec::new(),
                required: true,
                provider: ProviderLane::ManagedCodex,
                effect: EffectClass::ManagedLocalTools,
                managed_session_id: Some(session.clone()),
                model_id: None,
                prompt: format!("synthetic step {step_id}"),
                max_attempts: 1,
                max_duration_seconds: 60,
            })
            .collect(),
        };
        let review = PlanReviewV1 {
            plan_id: plan.plan_id.clone(),
            revision: plan.revision,
            digest: plan.digest().unwrap(),
        };
        let submitted = coordinator.submit(plan, review, 100).unwrap();
        coordinator.tick(101).unwrap();
        let after_first_start = coordinator.get(&submitted.run_id).unwrap();
        let first_step = after_first_start
            .steps
            .iter()
            .find(|step| step.step_id == "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1")
            .unwrap();
        let first_turn = first_step.managed_turn_id.clone().unwrap();
        let count_starts = || {
            frames
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|bytes| {
                    serde_json::from_slice::<JsonValue>(bytes)
                        .is_ok_and(|value| value["method"] == "turn/start")
                })
                .count()
        };
        assert_eq!(
            count_starts(),
            2,
            "first turn must reach the fake F8 worker once"
        );

        let (ack_tx, ack_rx) = mpsc::channel();
        complete_tx.send((first_turn, ack_tx)).unwrap();
        ack_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        coordinator.tick(102).unwrap();
        let verifying = coordinator.get(&submitted.run_id).unwrap();
        assert_eq!(
            verifying.steps[0].state,
            crate::orchestration::StepState::Verifying
        );
        coordinator.tick(103).unwrap();
        assert_eq!(
            count_starts(),
            2,
            "repeated scheduler ticks cannot dispatch the sibling while F8 retains the unsettled claim"
        );
        let claim = journal
            .workspace_claim_binding(&submitted.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            bridge.workspace_claim_status(&claim).unwrap().phase,
            "unsettled"
        );

        let moved_workspace = fixture.path("moved-original-workspace");
        fs::rename(&workspace, &moved_workspace).unwrap();
        fs::create_dir(&workspace).unwrap();
        fs::write(workspace.join("identity"), b"same workspace bytes").unwrap();
        let replacement_digest = workspace_revision(&workspace).unwrap();

        // The coordinator journal survives, while the same F8 instance retains its original
        // descriptor. Recovery must not treat the same-path, same-content replacement as proof.
        drop(coordinator);
        drop(journal);
        let journal = Arc::new(RunJournal::open(&journal_path).unwrap());
        let coordinator = Coordinator::new(journal.clone(), authority_trait, 104).unwrap();
        let recovered = coordinator.get(&submitted.run_id).unwrap();
        assert_eq!(recovered.state, crate::orchestration::RunState::NeedsReview);
        assert_eq!(
            recovered.steps[0].state,
            crate::orchestration::StepState::NeedsReview
        );
        assert_eq!(
            recovered.steps[1].state,
            crate::orchestration::StepState::Cancelled
        );
        let claim = journal
            .workspace_claim_binding(&submitted.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            bridge.workspace_claim_status(&claim).unwrap().phase,
            "unsettled"
        );
        coordinator.tick(105).unwrap();
        assert_eq!(
            count_starts(),
            2,
            "coordinator recovery must not resume or redispatch the unresolved effect"
        );

        let replacement = coordinator.reconcile(
            &submitted.run_id,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
            crate::orchestration::ReconcileDecision::ConfirmedCompleted,
            Some(workspace.to_str().unwrap()),
            Some(&replacement_digest),
            106,
        );
        assert!(
            matches!(
                replacement,
                Err(crate::orchestration::CoordinatorError::WorkspaceConflict)
            ),
            "F8's live identity pin must reject an identical replacement after coordinator restart"
        );
        let claim = journal
            .workspace_claim_binding(&submitted.run_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            bridge.workspace_claim_status(&claim).unwrap().phase,
            "unsettled"
        );
        assert_eq!(count_starts(), 2);

        let original_digest = workspace_revision(&moved_workspace).unwrap();
        let reconciled = coordinator
            .reconcile(
                &submitted.run_id,
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaa1",
                crate::orchestration::ReconcileDecision::ConfirmedCompleted,
                Some(moved_workspace.to_str().unwrap()),
                Some(&original_digest),
                107,
            )
            .unwrap();
        assert_eq!(
            reconciled.steps[0].state,
            crate::orchestration::StepState::Succeeded
        );
        assert_eq!(
            count_starts(),
            2,
            "recovery settles the selected original but never auto-resumes the plan"
        );
        let claim = journal
            .workspace_claim_binding(&submitted.run_id)
            .unwrap()
            .unwrap();
        let final_claim = bridge.workspace_claim_status(&claim).unwrap();
        assert_eq!(
            final_claim.phase,
            "released",
            "run={:?} steps={:?} local_claim_phase={}",
            reconciled.state,
            reconciled
                .steps
                .iter()
                .map(|step| (step.step_id.as_str(), step.state))
                .collect::<Vec<_>>(),
            claim.phase
        );

        stop.store(true, Ordering::Release);
        server.join().unwrap();
        fs::remove_dir_all(&runtime_root).unwrap();
    }
}
