use super::{
    CoordinatorSnapshot, DependencyV1, FailureCode, HandoffConfirmationV1, HandoffEnvelopeV1,
    MAX_ATTEMPTS, OrchestrationPlanV1, PlanReviewV1, RunState, StepPlanV1, StepSnapshot, StepState,
    valid_digest,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    sync::{Mutex, PoisonError},
};
use uuid::Uuid;

// Schema 2 is the fail-closed identity checkpoint. Schema 3 adds the durable coordinator ID
// and immutable M6-to-F8 import snapshot. Schema 4 records F8 settlement receipts and the
// coordinator revision acknowledged by F8. Schema 5 retains released claim history while
// allowing a new active generation to claim the same workspace.
const SCHEMA_VERSION: i64 = 5;
const APPLICATION_ID: i64 = 0x5759_4f52; // "WYOR"
const MAX_RUNS: usize = 128;
const MAX_LIST: usize = 128;
const EVENT_KINDS: &[&str] = &[
    "run_created",
    "run_state_changed",
    "step_state_changed",
    "attempt_started",
    "attempt_acknowledged",
    "attempt_unknown",
    "attempt_finished",
    "cancellation_started",
    "cancellation_acknowledged",
    "cancellation_refused",
    "cancellation_unknown",
    "recovered_after_restart",
    "handoff_reviewed",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalError {
    Invalid,
    NotFound,
    Stale,
    InvalidTransition,
    WorkspaceConflict,
    Capacity,
    Storage,
    Incompatible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancellationOutcome {
    Requested,
    Acknowledged,
    Refused,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceImportClaim {
    pub claim_id: String,
    pub run_id: String,
    pub claim_generation: u64,
    pub source_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceClaimBinding {
    pub authority_store_id: String,
    pub authority_epoch: String,
    pub root_binding_id: String,
    pub claim_id: String,
    pub claim_generation: u64,
    pub claim_revision: u64,
    pub gate_revision: u64,
    pub root_observation_revision: u64,
    pub phase: String,
    pub gate_state: String,
    pub settlement_id: Option<String>,
    pub coordinator_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSettlementIntent {
    pub observation_id: String,
    pub attempt_id: String,
    pub decision: String,
    pub reviewed_digest: String,
}

type WorkspaceClaimBindingRow = (
    String,
    String,
    String,
    String,
    i64,
    i64,
    i64,
    i64,
    String,
    String,
    Option<String>,
    i64,
);
type WorkspaceClaimStorageRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
);
type WorkspaceSettlementRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);
impl From<rusqlite::Error> for JournalError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Storage
    }
}

/// The database contains no objective, prompt, provider output, private path, credential,
/// or arbitrary diagnostic. A prompt digest is the most content-related field persisted.
pub struct RunJournal {
    connection: Mutex<Connection>,
}

impl RunJournal {
    pub fn open(path: &Path) -> Result<Self, JournalError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| JournalError::Storage)?;
        let metadata = file.metadata().map_err(|_| JournalError::Storage)?;
        if !metadata.is_file()
            || metadata.mode() & 0o777 != 0o600
            || metadata.nlink() != 1
            || metadata.uid() != nix::unistd::geteuid().as_raw()
        {
            return Err(JournalError::Incompatible);
        }
        drop(file);
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
        )
        .map_err(|_| JournalError::Storage)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, JournalError> {
        Self::from_connection(Connection::open_in_memory().map_err(|_| JournalError::Storage)?)
    }

    fn from_connection(connection: Connection) -> Result<Self, JournalError> {
        let failure_sql = failure_sql();
        let events_sql = events_sql();
        connection
            .busy_timeout(std::time::Duration::from_secs(1))
            .map_err(|_| JournalError::Storage)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON;
                 PRAGMA journal_mode=WAL;
                 PRAGMA synchronous=FULL;
                 PRAGMA trusted_schema=OFF;
                 PRAGMA secure_delete=ON;
                 PRAGMA temp_store=MEMORY;",
            )
            .map_err(|_| JournalError::Storage)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| JournalError::Storage)?;
        let application_id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(|_| JournalError::Storage)?;
        if version == 0 && application_id == 0 {
            let coordinator_store_id = Uuid::new_v4().to_string();
            connection
                .execute_batch(&format!(
                    "BEGIN IMMEDIATE;
                     PRAGMA application_id={APPLICATION_ID};
                     CREATE TABLE orchestration_runs (
                       run_id TEXT PRIMARY KEY CHECK(length(run_id)=36),
                       plan_id TEXT NOT NULL UNIQUE CHECK(length(plan_id)=36),
                       revision INTEGER NOT NULL CHECK(revision>0),
                       plan_revision INTEGER NOT NULL CHECK(plan_revision>0),
                       plan_digest TEXT NOT NULL CHECK(length(plan_digest)=64),
                       workspace_id TEXT NOT NULL CHECK(length(workspace_id)=64),
                       workspace_revision TEXT NOT NULL CHECK(length(workspace_revision)=64),
                       current_workspace_revision TEXT NOT NULL CHECK(length(current_workspace_revision)=64),
                       state TEXT NOT NULL CHECK(state IN ('ready','running','pausing','paused','cancelling','completed','failed','cancelled','expired','needs_review','blocked')),
                       failure TEXT CHECK(failure IS NULL OR failure IN ({failure_sql})),
                       created_at INTEGER NOT NULL CHECK(created_at>=0),
                       updated_at INTEGER NOT NULL CHECK(updated_at>=created_at),
                       expires_at INTEGER NOT NULL CHECK(expires_at>created_at),
                       max_steps INTEGER NOT NULL CHECK(max_steps BETWEEN 1 AND 16),
                       max_parallelism INTEGER NOT NULL CHECK(max_parallelism=1),
                       max_total_seconds INTEGER NOT NULL CHECK(max_total_seconds BETWEEN 1 AND 3600),
                       stop_policy TEXT NOT NULL CHECK(stop_policy IN ('stop_scheduling','cancel_current_managed_turn')),
                       owner_digest TEXT NOT NULL CHECK(length(owner_digest)=64)
                     ) STRICT;
                     CREATE TABLE orchestration_steps (
                       run_id TEXT NOT NULL REFERENCES orchestration_runs(run_id),
                       step_id TEXT NOT NULL CHECK(length(step_id)=36),
                       ordinal INTEGER NOT NULL CHECK(ordinal BETWEEN 0 AND 15),
                       dependencies_json TEXT NOT NULL,
                       required INTEGER NOT NULL CHECK(required IN (0,1)),
                       provider TEXT NOT NULL CHECK(provider IN ('managed_codex','managed_claude','native_codex','native_claude','hermes_planner')),
                       effect TEXT NOT NULL CHECK(effect IN ('proposal_only','managed_inference','managed_local_tools','native_full_account')),
                       managed_session_id TEXT CHECK(managed_session_id IS NULL OR length(managed_session_id)=36),
                       model_id TEXT,
                       input_digest TEXT NOT NULL CHECK(length(input_digest)=64),
                       max_attempts INTEGER NOT NULL CHECK(max_attempts BETWEEN 1 AND 3),
                       max_duration_seconds INTEGER NOT NULL CHECK(max_duration_seconds BETWEEN 1 AND 1800),
                       state TEXT NOT NULL CHECK(state IN ('proposed','waiting_dependencies','eligible','waiting_resource','awaiting_permission','queued','starting','running','verifying','succeeded','skipped','blocked','failed','cancelled','needs_review')),
                       attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count BETWEEN 0 AND 3),
                       current_attempt_id TEXT,
                       managed_turn_id TEXT,
                       authority_instance_id TEXT,
                       runtime_instance_id TEXT,
                       runtime_generation INTEGER,
                       turn_revision INTEGER,
                       workspace_revision_after TEXT CHECK(workspace_revision_after IS NULL OR length(workspace_revision_after)=64),
                       failure TEXT CHECK(failure IS NULL OR failure IN ({failure_sql})),
                       updated_at INTEGER NOT NULL CHECK(updated_at>=0),
                       PRIMARY KEY(run_id,step_id),
                       UNIQUE(run_id,ordinal)
                     ) STRICT;
                     CREATE TABLE orchestration_attempts (
                       run_id TEXT NOT NULL,
                       step_id TEXT NOT NULL,
                       attempt_id TEXT NOT NULL CHECK(length(attempt_id)=36),
                       authority_intent_id TEXT NOT NULL CHECK(length(authority_intent_id)=36),
                       state TEXT NOT NULL CHECK(state IN ('dispatching','acknowledged','running','terminal','unknown','refused')),
                       authority_instance_id TEXT NOT NULL CHECK(length(authority_instance_id)=36),
                       runtime_instance_id TEXT,
                       runtime_generation INTEGER,
                       managed_turn_id TEXT,
                       turn_revision INTEGER,
                       failure TEXT CHECK(failure IS NULL OR failure IN ({failure_sql})),
                       started_at INTEGER NOT NULL CHECK(started_at>=0),
                       updated_at INTEGER NOT NULL CHECK(updated_at>=started_at),
                       PRIMARY KEY(run_id,step_id,attempt_id),
                       UNIQUE(authority_instance_id,authority_intent_id),
                       FOREIGN KEY(run_id,step_id) REFERENCES orchestration_steps(run_id,step_id)
                     ) STRICT;
                     CREATE TABLE orchestration_cancellations (
                       run_id TEXT NOT NULL,
                       step_id TEXT NOT NULL,
                       attempt_id TEXT NOT NULL CHECK(length(attempt_id)=36),
                       intent_id TEXT NOT NULL UNIQUE CHECK(length(intent_id)=36),
                       state TEXT NOT NULL CHECK(state IN ('requested','acknowledged','refused','unknown')),
                       started_at INTEGER NOT NULL CHECK(started_at>=0),
                       updated_at INTEGER NOT NULL CHECK(updated_at>=started_at),
                       PRIMARY KEY(run_id,step_id,attempt_id),
                       FOREIGN KEY(run_id,step_id,attempt_id) REFERENCES orchestration_attempts(run_id,step_id,attempt_id)
                     ) STRICT;
                     CREATE TABLE orchestration_events (
                       sequence INTEGER PRIMARY KEY AUTOINCREMENT,
                       run_id TEXT NOT NULL REFERENCES orchestration_runs(run_id),
                       step_id TEXT,
                       attempt_id TEXT,
                       event_kind TEXT NOT NULL CHECK(event_kind IN ({events_sql})),
                       outcome TEXT CHECK(outcome IS NULL OR outcome IN ({failure_sql})),
                       timestamp INTEGER NOT NULL CHECK(timestamp>=0)
                     ) STRICT;
                     CREATE TABLE orchestration_workspace_claims (
                       workspace_id TEXT NOT NULL CHECK(length(workspace_id)=64),
                       run_id TEXT PRIMARY KEY NOT NULL REFERENCES orchestration_runs(run_id),
                       claimed_at INTEGER NOT NULL CHECK(claimed_at>=0),
                       authority_store_id TEXT,
                       authority_epoch TEXT,
                       root_binding_id TEXT,
                       claim_id TEXT,
                       claim_generation INTEGER CHECK(claim_generation IS NULL OR claim_generation>0),
                       claim_revision INTEGER CHECK(claim_revision IS NULL OR claim_revision>0),
                       gate_revision INTEGER CHECK(gate_revision IS NULL OR gate_revision>0),
                       root_observation_revision INTEGER CHECK(root_observation_revision IS NULL OR root_observation_revision>0),
                       claim_phase TEXT CHECK(claim_phase IS NULL OR claim_phase IN ('held','unsettled','verified','released','quarantined')),
                       gate_state TEXT CHECK(gate_state IS NULL OR gate_state IN ('open','paused','stopped','quarantined')),
                       settlement_id TEXT CHECK(settlement_id IS NULL OR length(settlement_id)=36),
                       coordinator_revision INTEGER NOT NULL DEFAULT 0 CHECK(coordinator_revision>=0),
                       settlement_intent_id TEXT CHECK(settlement_intent_id IS NULL OR length(settlement_intent_id)=36),
                       settlement_attempt_id TEXT CHECK(settlement_attempt_id IS NULL OR length(settlement_attempt_id)=36),
                       settlement_decision TEXT CHECK(settlement_decision IS NULL OR settlement_decision IN ('confirmed_completed','confirmed_no_effect','confirmed_cancelled')),
                       settlement_digest TEXT CHECK(settlement_digest IS NULL OR length(settlement_digest)=64),
                       proof_state TEXT NOT NULL DEFAULT 'identity_unproven'
                         CHECK(proof_state IN ('pinned','identity_unproven','quarantined'))
                     ) STRICT;
                     CREATE TABLE orchestration_metadata (
                       id INTEGER PRIMARY KEY CHECK(id=1),
                       coordinator_store_id TEXT NOT NULL UNIQUE CHECK(length(coordinator_store_id)=36)
                     ) STRICT;
                     INSERT INTO orchestration_metadata VALUES(1,'{coordinator_store_id}');
                     CREATE TRIGGER orchestration_metadata_identity_no_update BEFORE UPDATE OF coordinator_store_id ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TRIGGER orchestration_metadata_no_delete BEFORE DELETE ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TABLE orchestration_import_snapshot (
                       claim_id TEXT PRIMARY KEY CHECK(length(claim_id)=36),
                       run_id TEXT NOT NULL CHECK(length(run_id)=36),
                       claim_generation INTEGER NOT NULL CHECK(claim_generation>0),
                       source_revision INTEGER NOT NULL CHECK(source_revision>0),
                       identity_state TEXT NOT NULL CHECK(identity_state='identity_unproven')
                     ) STRICT;
                     CREATE TRIGGER orchestration_import_snapshot_no_update BEFORE UPDATE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     CREATE TRIGGER orchestration_import_snapshot_no_delete BEFORE DELETE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     CREATE TABLE orchestration_handoffs (
                       handoff_id TEXT PRIMARY KEY CHECK(length(handoff_id)=36),
                       revision INTEGER NOT NULL CHECK(revision>0),
                       source_host_id TEXT NOT NULL CHECK(length(source_host_id) BETWEEN 1 AND 64),
                       source_session_id TEXT NOT NULL CHECK(length(source_session_id) BETWEEN 32 AND 36),
                       source_generation INTEGER NOT NULL CHECK(source_generation>0),
                       source_plane TEXT NOT NULL CHECK(source_plane IN ('native','managed')),
                       source_project_id TEXT NOT NULL CHECK(length(source_project_id) BETWEEN 1 AND 128),
                       source_workspace_id TEXT NOT NULL CHECK(length(source_workspace_id)=64),
                       source_workspace_revision TEXT NOT NULL CHECK(length(source_workspace_revision)=64),
                       target_host_id TEXT NOT NULL CHECK(length(target_host_id) BETWEEN 1 AND 64),
                       target_project_id TEXT NOT NULL CHECK(length(target_project_id) BETWEEN 1 AND 128),
                       target_workspace_id TEXT NOT NULL CHECK(length(target_workspace_id)=64),
                       target_workspace_revision TEXT NOT NULL CHECK(length(target_workspace_revision)=64),
                       target_provider TEXT NOT NULL CHECK(target_provider IN ('managed_codex','managed_claude','native_codex','native_claude','hermes_planner')),
                       target_session_id TEXT CHECK(target_session_id IS NULL OR length(target_session_id)=36),
                       content_digest TEXT NOT NULL CHECK(length(content_digest)=64),
                       confirmed_at INTEGER NOT NULL CHECK(confirmed_at>=0),
                       run_id TEXT NOT NULL UNIQUE REFERENCES orchestration_runs(run_id)
                     ) STRICT;
                     CREATE INDEX orchestration_steps_state ON orchestration_steps(state,run_id,ordinal);
                     CREATE INDEX orchestration_attempts_state ON orchestration_attempts(state,run_id);
                     CREATE INDEX orchestration_cancellations_state ON orchestration_cancellations(state,run_id);
                     CREATE INDEX orchestration_events_run ON orchestration_events(run_id,sequence);
                     CREATE UNIQUE INDEX orchestration_active_workspace_claim ON orchestration_workspace_claims(workspace_id) WHERE claim_phase IS NULL OR claim_phase<>'released';
                     CREATE TRIGGER orchestration_handoffs_no_update BEFORE UPDATE ON orchestration_handoffs BEGIN SELECT RAISE(ABORT,'immutable handoff'); END;
                     CREATE TRIGGER orchestration_handoffs_no_delete BEFORE DELETE ON orchestration_handoffs BEGIN SELECT RAISE(ABORT,'immutable handoff'); END;
                     CREATE TRIGGER orchestration_events_no_update BEFORE UPDATE ON orchestration_events BEGIN SELECT RAISE(ABORT,'append only'); END;
                     CREATE TRIGGER orchestration_events_no_delete BEFORE DELETE ON orchestration_events BEGIN SELECT RAISE(ABORT,'append only'); END;
                     CREATE TRIGGER orchestration_runs_immutable_plan BEFORE UPDATE ON orchestration_runs
                       WHEN NEW.plan_id!=OLD.plan_id OR NEW.plan_revision!=OLD.plan_revision OR NEW.plan_digest!=OLD.plan_digest
                         OR NEW.workspace_id!=OLD.workspace_id OR NEW.workspace_revision!=OLD.workspace_revision
                         OR NEW.created_at!=OLD.created_at OR NEW.expires_at!=OLD.expires_at
                         OR NEW.max_steps!=OLD.max_steps OR NEW.max_parallelism!=OLD.max_parallelism
                         OR NEW.max_total_seconds!=OLD.max_total_seconds OR NEW.stop_policy!=OLD.stop_policy
                         OR NEW.owner_digest!=OLD.owner_digest
                       BEGIN SELECT RAISE(ABORT,'immutable plan'); END;
                     CREATE TRIGGER orchestration_steps_immutable_plan BEFORE UPDATE ON orchestration_steps
                       WHEN NEW.ordinal!=OLD.ordinal OR NEW.dependencies_json!=OLD.dependencies_json
                         OR NEW.required!=OLD.required OR NEW.provider!=OLD.provider OR NEW.effect!=OLD.effect
                         OR NEW.managed_session_id IS NOT OLD.managed_session_id
                         OR NEW.model_id IS NOT OLD.model_id OR NEW.input_digest!=OLD.input_digest
                         OR NEW.max_attempts!=OLD.max_attempts OR NEW.max_duration_seconds!=OLD.max_duration_seconds
                       BEGIN SELECT RAISE(ABORT,'immutable plan'); END;
                     PRAGMA user_version={SCHEMA_VERSION};
                     COMMIT;"
                ))
                .map_err(|_| JournalError::Storage)?;
        } else if version == 1 && application_id == APPLICATION_ID {
            // Schema 1 recorded only a path-derived logical workspace ID. Never infer a
            // physical root from today's filesystem: all existing claims migrate unproven.
            let coordinator_store_id = Uuid::new_v4().to_string();
            connection
                .execute_batch(&format!(
                    "BEGIN IMMEDIATE;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN authority_store_id TEXT;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN authority_epoch TEXT;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN root_binding_id TEXT;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN claim_id TEXT;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN claim_generation INTEGER CHECK(claim_generation IS NULL OR claim_generation>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN claim_revision INTEGER CHECK(claim_revision IS NULL OR claim_revision>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN gate_revision INTEGER CHECK(gate_revision IS NULL OR gate_revision>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN root_observation_revision INTEGER CHECK(root_observation_revision IS NULL OR root_observation_revision>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN claim_phase TEXT CHECK(claim_phase IS NULL OR claim_phase IN ('held','unsettled','verified','released','quarantined'));
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN gate_state TEXT CHECK(gate_state IS NULL OR gate_state IN ('open','paused','stopped','quarantined'));
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN proof_state TEXT NOT NULL DEFAULT 'identity_unproven' CHECK(proof_state IN ('pinned','identity_unproven','quarantined'));
                     CREATE TABLE orchestration_metadata (id INTEGER PRIMARY KEY CHECK(id=1), coordinator_store_id TEXT NOT NULL UNIQUE CHECK(length(coordinator_store_id)=36)) STRICT;
                     INSERT INTO orchestration_metadata VALUES(1,'{coordinator_store_id}');
                     CREATE TRIGGER orchestration_metadata_identity_no_update BEFORE UPDATE OF coordinator_store_id ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TRIGGER orchestration_metadata_no_delete BEFORE DELETE ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TABLE orchestration_import_snapshot (claim_id TEXT PRIMARY KEY CHECK(length(claim_id)=36), run_id TEXT NOT NULL CHECK(length(run_id)=36), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), source_revision INTEGER NOT NULL CHECK(source_revision>0), identity_state TEXT NOT NULL CHECK(identity_state='identity_unproven')) STRICT;
                     CREATE TRIGGER orchestration_import_snapshot_no_update BEFORE UPDATE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     CREATE TRIGGER orchestration_import_snapshot_no_delete BEFORE DELETE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     INSERT INTO orchestration_import_snapshot(claim_id,run_id,claim_generation,source_revision,identity_state) SELECT run_id,run_id,1,1,'identity_unproven' FROM orchestration_workspace_claims;
                     PRAGMA user_version=3;
                     COMMIT;"
                ))
                .map_err(|_| JournalError::Storage)?;
        } else if version == 2 && application_id == APPLICATION_ID {
            // Schema 2 was a published fail-closed checkpoint. Upgrade its nullable references
            // without deriving identity from the current workspace path.
            let coordinator_store_id = Uuid::new_v4().to_string();
            connection
                .execute_batch(&format!(
                    "BEGIN IMMEDIATE;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN gate_revision INTEGER CHECK(gate_revision IS NULL OR gate_revision>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN root_observation_revision INTEGER CHECK(root_observation_revision IS NULL OR root_observation_revision>0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN claim_phase TEXT CHECK(claim_phase IS NULL OR claim_phase IN ('held','unsettled','verified','released','quarantined'));
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN gate_state TEXT CHECK(gate_state IS NULL OR gate_state IN ('open','paused','stopped','quarantined'));
                     CREATE TABLE orchestration_metadata (id INTEGER PRIMARY KEY CHECK(id=1), coordinator_store_id TEXT NOT NULL UNIQUE CHECK(length(coordinator_store_id)=36)) STRICT;
                     INSERT INTO orchestration_metadata VALUES(1,'{coordinator_store_id}');
                     CREATE TRIGGER orchestration_metadata_identity_no_update BEFORE UPDATE OF coordinator_store_id ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TRIGGER orchestration_metadata_no_delete BEFORE DELETE ON orchestration_metadata BEGIN SELECT RAISE(ABORT,'immutable_coordinator_identity'); END;
                     CREATE TABLE orchestration_import_snapshot (claim_id TEXT PRIMARY KEY CHECK(length(claim_id)=36), run_id TEXT NOT NULL CHECK(length(run_id)=36), claim_generation INTEGER NOT NULL CHECK(claim_generation>0), source_revision INTEGER NOT NULL CHECK(source_revision>0), identity_state TEXT NOT NULL CHECK(identity_state='identity_unproven')) STRICT;
                     CREATE TRIGGER orchestration_import_snapshot_no_update BEFORE UPDATE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     CREATE TRIGGER orchestration_import_snapshot_no_delete BEFORE DELETE ON orchestration_import_snapshot BEGIN SELECT RAISE(ABORT,'immutable_import_snapshot'); END;
                     INSERT INTO orchestration_import_snapshot(claim_id,run_id,claim_generation,source_revision,identity_state) SELECT run_id,run_id,1,1,'identity_unproven' FROM orchestration_workspace_claims WHERE proof_state<>'pinned';
                     PRAGMA user_version=3;
                     COMMIT;"
                ))
                .map_err(|_| JournalError::Storage)?;
        } else if version == 3 && application_id == APPLICATION_ID {
            // Version 3 already carries the identity-unproven fence. Add only settlement
            // acknowledgements; no physical identity or settlement is inferred for old rows.
        } else if version == 4 && application_id == APPLICATION_ID {
            // Version 4 retained one row per workspace forever. Version 5 rebuilds the claim
            // table so released history does not prevent admission of a later generation.
        } else if version != SCHEMA_VERSION || application_id != APPLICATION_ID {
            return Err(JournalError::Incompatible);
        }
        let mut migrated_version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| JournalError::Storage)?;
        if migrated_version == 3 {
            connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN settlement_id TEXT CHECK(settlement_id IS NULL OR length(settlement_id)=36);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN coordinator_revision INTEGER NOT NULL DEFAULT 0 CHECK(coordinator_revision>=0);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN settlement_intent_id TEXT CHECK(settlement_intent_id IS NULL OR length(settlement_intent_id)=36);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN settlement_attempt_id TEXT CHECK(settlement_attempt_id IS NULL OR length(settlement_attempt_id)=36);
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN settlement_decision TEXT CHECK(settlement_decision IS NULL OR settlement_decision IN ('confirmed_completed','confirmed_no_effect','confirmed_cancelled'));
                     ALTER TABLE orchestration_workspace_claims ADD COLUMN settlement_digest TEXT CHECK(settlement_digest IS NULL OR length(settlement_digest)=64);
                     PRAGMA user_version=4;
                     COMMIT;",
                )
                .map_err(|_| JournalError::Storage)?;
            migrated_version = 4;
        }
        if migrated_version == 4 {
            connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     ALTER TABLE orchestration_workspace_claims RENAME TO orchestration_workspace_claims_v4;
                     CREATE TABLE orchestration_workspace_claims (
                       workspace_id TEXT NOT NULL CHECK(length(workspace_id)=64),
                       run_id TEXT PRIMARY KEY NOT NULL REFERENCES orchestration_runs(run_id),
                       claimed_at INTEGER NOT NULL CHECK(claimed_at>=0),
                       authority_store_id TEXT,
                       authority_epoch TEXT,
                       root_binding_id TEXT,
                       claim_id TEXT,
                       claim_generation INTEGER CHECK(claim_generation IS NULL OR claim_generation>0),
                       claim_revision INTEGER CHECK(claim_revision IS NULL OR claim_revision>0),
                       gate_revision INTEGER CHECK(gate_revision IS NULL OR gate_revision>0),
                       root_observation_revision INTEGER CHECK(root_observation_revision IS NULL OR root_observation_revision>0),
                       claim_phase TEXT CHECK(claim_phase IS NULL OR claim_phase IN ('held','unsettled','verified','released','quarantined')),
                       gate_state TEXT CHECK(gate_state IS NULL OR gate_state IN ('open','paused','stopped','quarantined')),
                       settlement_id TEXT CHECK(settlement_id IS NULL OR length(settlement_id)=36),
                       coordinator_revision INTEGER NOT NULL DEFAULT 0 CHECK(coordinator_revision>=0),
                       settlement_intent_id TEXT CHECK(settlement_intent_id IS NULL OR length(settlement_intent_id)=36),
                       settlement_attempt_id TEXT CHECK(settlement_attempt_id IS NULL OR length(settlement_attempt_id)=36),
                       settlement_decision TEXT CHECK(settlement_decision IS NULL OR settlement_decision IN ('confirmed_completed','confirmed_no_effect','confirmed_cancelled')),
                       settlement_digest TEXT CHECK(settlement_digest IS NULL OR length(settlement_digest)=64),
                       proof_state TEXT NOT NULL DEFAULT 'identity_unproven'
                         CHECK(proof_state IN ('pinned','identity_unproven','quarantined'))
                     ) STRICT;
                     INSERT INTO orchestration_workspace_claims(
                       workspace_id,run_id,claimed_at,authority_store_id,authority_epoch,root_binding_id,
                       claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,
                       claim_phase,gate_state,settlement_id,coordinator_revision,settlement_intent_id,
                       settlement_attempt_id,settlement_decision,settlement_digest,proof_state
                     ) SELECT
                       workspace_id,run_id,claimed_at,authority_store_id,authority_epoch,root_binding_id,
                       claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,
                       claim_phase,gate_state,settlement_id,coordinator_revision,settlement_intent_id,
                       settlement_attempt_id,settlement_decision,settlement_digest,proof_state
                     FROM orchestration_workspace_claims_v4;
                     DROP TABLE orchestration_workspace_claims_v4;
                     CREATE UNIQUE INDEX orchestration_active_workspace_claim ON orchestration_workspace_claims(workspace_id) WHERE claim_phase IS NULL OR claim_phase<>'released';
                     PRAGMA user_version=5;
                     COMMIT;",
                )
                .map_err(|_| JournalError::Storage)?;
        }
        let integrity: String = connection
            .pragma_query_value(None, "integrity_check", |row| row.get(0))
            .map_err(|_| JournalError::Storage)?;
        if integrity != "ok" {
            return Err(JournalError::Incompatible);
        }
        let foreign_key_violation: Option<i64> = connection
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()
            .map_err(|_| JournalError::Storage)?;
        if foreign_key_violation.is_some() {
            return Err(JournalError::Incompatible);
        }
        let schema_objects: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type IN ('table','index','trigger') AND name NOT LIKE 'sqlite_%'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| JournalError::Storage)?;
        if schema_objects != 24 {
            return Err(JournalError::Incompatible);
        }
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn coordinator_store_id(&self) -> Result<String, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        connection
            .query_row(
                "SELECT coordinator_store_id FROM orchestration_metadata WHERE id=1",
                [],
                |row| row.get(0),
            )
            .map_err(|_| JournalError::Incompatible)
    }

    /// This immutable snapshot contains only unresolved claims that existed before M6/F8
    /// identity proof was available. New claims are acquired directly from F8 and never added.
    pub fn import_snapshot(&self) -> Result<Vec<WorkspaceImportClaim>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT claim_id,run_id,claim_generation,source_revision FROM orchestration_import_snapshot ORDER BY claim_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(WorkspaceImportClaim {
                claim_id: row.get(0)?,
                run_id: row.get(1)?,
                claim_generation: row.get::<_, i64>(2)? as u64,
                source_revision: row.get::<_, i64>(3)? as u64,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn workspace_claim_binding(
        &self,
        run_id: &str,
    ) -> Result<Option<WorkspaceClaimBinding>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let row: Option<WorkspaceClaimBindingRow> = connection
            .query_row(
                "SELECT authority_store_id,authority_epoch,root_binding_id,claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,claim_phase,gate_state,settlement_id,coordinator_revision FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state='pinned'",
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?)),
            )
            .optional()?;
        row.map(
            |(
                authority_store_id,
                authority_epoch,
                root_binding_id,
                claim_id,
                claim_generation,
                claim_revision,
                gate_revision,
                root_observation_revision,
                phase,
                gate_state,
                settlement_id,
                coordinator_revision,
            )| {
                Ok(WorkspaceClaimBinding {
                    authority_store_id,
                    authority_epoch,
                    root_binding_id,
                    claim_id,
                    claim_generation: u64::try_from(claim_generation)
                        .map_err(|_| JournalError::Incompatible)?,
                    claim_revision: u64::try_from(claim_revision)
                        .map_err(|_| JournalError::Incompatible)?,
                    gate_revision: u64::try_from(gate_revision)
                        .map_err(|_| JournalError::Incompatible)?,
                    root_observation_revision: u64::try_from(root_observation_revision)
                        .map_err(|_| JournalError::Incompatible)?,
                    phase,
                    gate_state,
                    settlement_id,
                    coordinator_revision: u64::try_from(coordinator_revision)
                        .map_err(|_| JournalError::Incompatible)?,
                })
            },
        )
        .transpose()
    }

    /// Bind the exact immutable F8 acquisition receipt before making the run schedulable.
    pub fn bind_workspace_claim(
        &self,
        run_id: &str,
        binding: &WorkspaceClaimBinding,
    ) -> Result<(), JournalError> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: Option<WorkspaceClaimStorageRow> = tx.query_row(
            "SELECT authority_store_id,authority_epoch,root_binding_id,claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,claim_phase,gate_state,settlement_id,coordinator_revision,settlement_intent_id,settlement_attempt_id,settlement_decision,settlement_digest,proof_state FROM orchestration_workspace_claims WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?,row.get(13)?,row.get(14)?,row.get(15)?,row.get(16)?)),
        ).optional()?;
        let current = current.ok_or(JournalError::NotFound)?;
        let expected = (
            Some(binding.authority_store_id.clone()),
            Some(binding.authority_epoch.clone()),
            Some(binding.root_binding_id.clone()),
            Some(binding.claim_id.clone()),
            Some(binding.claim_generation as i64),
            Some(binding.claim_revision as i64),
            Some(binding.gate_revision as i64),
            Some(binding.root_observation_revision as i64),
            Some(binding.phase.clone()),
            Some(binding.gate_state.clone()),
            binding.settlement_id.clone(),
            Some(binding.coordinator_revision as i64),
            None,
            None,
            None,
            None,
            "pinned".to_owned(),
        );
        if (
            &current.0,
            &current.1,
            &current.2,
            &current.3,
            &current.4,
            &current.5,
            &current.6,
            &current.7,
            &current.8,
            &current.9,
            &current.10,
            &current.11,
        ) == (
            &expected.0,
            &expected.1,
            &expected.2,
            &expected.3,
            &expected.4,
            &expected.5,
            &expected.6,
            &expected.7,
            &expected.8,
            &expected.9,
            &expected.10,
            &expected.11,
        ) && (&current.12, &current.13, &current.14, &current.15)
            == (&expected.12, &expected.13, &expected.14, &expected.15)
            && current.16 == expected.16
        {
            tx.commit()?;
            return Ok(());
        }
        let unbound = current.0.is_none()
            && current.1.is_none()
            && current.2.is_none()
            && current.3.is_none()
            && current.4.is_none()
            && current.5.is_none()
            && current.6.is_none()
            && current.7.is_none()
            && current.8.is_none()
            && current.9.is_none()
            && current.10.is_none()
            && current.11 == Some(0)
            && current.12.is_none()
            && current.13.is_none()
            && current.14.is_none()
            && current.15.is_none()
            && current.16 == "identity_unproven";
        if !unbound
            || binding.phase != "held"
            || binding.gate_state != "open"
            || [
                &binding.authority_store_id,
                &binding.authority_epoch,
                &binding.root_binding_id,
                &binding.claim_id,
            ]
            .iter()
            .any(|id| Uuid::parse_str(id).is_err())
            || [
                binding.claim_generation,
                binding.claim_revision,
                binding.gate_revision,
                binding.root_observation_revision,
            ]
            .contains(&0)
        {
            return Err(JournalError::Stale);
        }
        let changed = tx.execute(
            "UPDATE orchestration_workspace_claims SET authority_store_id=?1,authority_epoch=?2,root_binding_id=?3,claim_id=?4,claim_generation=?5,claim_revision=?6,gate_revision=?7,root_observation_revision=?8,claim_phase=?9,gate_state=?10,settlement_id=?11,coordinator_revision=?12,proof_state='pinned' WHERE run_id=?13 AND proof_state='identity_unproven' AND authority_store_id IS NULL AND claim_id IS NULL",
            params![binding.authority_store_id,binding.authority_epoch,binding.root_binding_id,binding.claim_id,binding.claim_generation as i64,binding.claim_revision as i64,binding.gate_revision as i64,binding.root_observation_revision as i64,binding.phase,binding.gate_state,binding.settlement_id,binding.coordinator_revision as i64,run_id],
        )?;
        if changed != 1 {
            return Err(JournalError::Stale);
        }
        tx.commit()?;
        Ok(())
    }

    /// Mirror an F8 gate CAS only after its authority response is correlated. If the reply was
    /// lost, the stale local revision remains a fence and the scheduler cannot reopen it.
    pub fn update_workspace_claim_gate(
        &self,
        run_id: &str,
        expected: &WorkspaceClaimBinding,
        updated: &WorkspaceClaimBinding,
    ) -> Result<(), JournalError> {
        if expected.authority_store_id != updated.authority_store_id
            || expected.authority_epoch != updated.authority_epoch
            || expected.root_binding_id != updated.root_binding_id
            || expected.claim_id != updated.claim_id
            || expected.claim_generation != updated.claim_generation
            || expected.root_observation_revision != updated.root_observation_revision
            || !((expected.phase == updated.phase)
                || (expected.phase == "verified"
                    && updated.phase == "held"
                    && expected.settlement_id.is_some()
                    && expected.settlement_id == updated.settlement_id
                    && updated.coordinator_revision > expected.coordinator_revision))
            || (expected.phase == updated.phase
                && (expected.settlement_id != updated.settlement_id
                    || expected.coordinator_revision != updated.coordinator_revision))
            || !matches!(updated.gate_state.as_str(), "open" | "paused" | "stopped")
            || !((updated.claim_revision == expected.claim_revision
                && updated.gate_revision == expected.gate_revision)
                || (updated.claim_revision == expected.claim_revision + 1
                    && updated.gate_revision == expected.gate_revision + 1))
        {
            return Err(JournalError::Invalid);
        }
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let changed = connection.execute(
            "UPDATE orchestration_workspace_claims SET claim_revision=?1,gate_revision=?2,gate_state=?3,claim_phase=?4,settlement_id=?5,coordinator_revision=?6 WHERE run_id=?7 AND proof_state='pinned' AND authority_store_id=?8 AND authority_epoch=?9 AND root_binding_id=?10 AND claim_id=?11 AND claim_generation=?12 AND claim_revision=?13 AND gate_revision=?14 AND root_observation_revision=?15 AND claim_phase=?16 AND gate_state=?17 AND settlement_id IS ?18 AND coordinator_revision=?19",
            params![updated.claim_revision as i64,updated.gate_revision as i64,updated.gate_state,updated.phase,updated.settlement_id,updated.coordinator_revision as i64,run_id,expected.authority_store_id,expected.authority_epoch,expected.root_binding_id,expected.claim_id,expected.claim_generation as i64,expected.claim_revision as i64,expected.gate_revision as i64,expected.root_observation_revision as i64,expected.phase,expected.gate_state,expected.settlement_id,expected.coordinator_revision as i64],
        )?;
        if changed != 1 {
            drop(connection);
            let current = self.workspace_claim_binding(run_id)?;
            return if current.as_ref() == Some(updated) {
                Ok(())
            } else {
                Err(JournalError::Stale)
            };
        }
        Ok(())
    }

    /// Mirror an F8 release receipt only after M6 has already committed a terminal run.
    pub fn record_workspace_claim_release(
        &self,
        run_id: &str,
        expected: &WorkspaceClaimBinding,
        released: &WorkspaceClaimBinding,
        coordinator_revision: u64,
    ) -> Result<(), JournalError> {
        let settled = expected.phase == "verified" && expected.settlement_id.is_some();
        let unused = expected.phase == "held" && expected.settlement_id.is_none();
        if (!settled && !unused)
            || released.authority_store_id != expected.authority_store_id
            || released.authority_epoch != expected.authority_epoch
            || released.root_binding_id != expected.root_binding_id
            || released.claim_id != expected.claim_id
            || released.claim_generation != expected.claim_generation
            || released.claim_revision != expected.claim_revision.saturating_add(1)
            || released.gate_revision != expected.gate_revision.saturating_add(1)
            || released.phase != "released"
            || released.gate_state != "stopped"
            || released.settlement_id != expected.settlement_id
            || released.coordinator_revision != coordinator_revision
            || coordinator_revision <= expected.coordinator_revision
        {
            return Err(JournalError::Invalid);
        }
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let changed = if settled {
            connection.execute(
                "UPDATE orchestration_workspace_claims SET claim_revision=?1,gate_revision=?2,claim_phase='released',gate_state='stopped',coordinator_revision=?3 WHERE run_id=?4 AND proof_state='pinned' AND authority_store_id=?5 AND authority_epoch=?6 AND root_binding_id=?7 AND claim_id=?8 AND claim_generation=?9 AND claim_revision=?10 AND gate_revision=?11 AND root_observation_revision=?12 AND claim_phase='verified' AND gate_state=?13 AND settlement_id=?14 AND coordinator_revision=?15",
                params![released.claim_revision as i64,released.gate_revision as i64,released.coordinator_revision as i64,run_id,expected.authority_store_id,expected.authority_epoch,expected.root_binding_id,expected.claim_id,expected.claim_generation as i64,expected.claim_revision as i64,expected.gate_revision as i64,expected.root_observation_revision as i64,expected.gate_state,expected.settlement_id,expected.coordinator_revision as i64],
            )?
        } else {
            connection.execute(
                "UPDATE orchestration_workspace_claims SET claim_revision=?1,gate_revision=?2,claim_phase='released',gate_state='stopped',coordinator_revision=?3 WHERE run_id=?4 AND proof_state='pinned' AND authority_store_id=?5 AND authority_epoch=?6 AND root_binding_id=?7 AND claim_id=?8 AND claim_generation=?9 AND claim_revision=?10 AND gate_revision=?11 AND root_observation_revision=?12 AND claim_phase='held' AND gate_state=?13 AND settlement_id IS NULL AND coordinator_revision=?14",
                params![released.claim_revision as i64,released.gate_revision as i64,released.coordinator_revision as i64,run_id,expected.authority_store_id,expected.authority_epoch,expected.root_binding_id,expected.claim_id,expected.claim_generation as i64,expected.claim_revision as i64,expected.gate_revision as i64,expected.root_observation_revision as i64,expected.gate_state,expected.coordinator_revision as i64],
            )?
        };
        if changed == 1 {
            return Ok(());
        }
        drop(connection);
        if self.workspace_claim_binding(run_id)?.as_ref() == Some(released) {
            Ok(())
        } else {
            Err(JournalError::Stale)
        }
    }

    /// Persist the exact settlement request before calling F8. A restart can therefore query
    /// or replay only this immutable metadata operation; it cannot invent a new review intent.
    pub fn begin_workspace_settlement(
        &self,
        run_id: &str,
        step_id: &str,
        expected_step_state: StepState,
        binding: &WorkspaceClaimBinding,
        intent: &WorkspaceSettlementIntent,
    ) -> Result<(), JournalError> {
        if !valid_uuid(&intent.observation_id)
            || !valid_uuid(&intent.attempt_id)
            || !valid_digest(&intent.reviewed_digest)
            || !matches!(
                intent.decision.as_str(),
                "confirmed_completed" | "confirmed_no_effect" | "confirmed_cancelled"
            )
            || binding.phase != "unsettled"
            || binding.settlement_id.is_some()
        {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let step: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT state,current_attempt_id FROM orchestration_steps WHERE run_id=?1 AND step_id=?2",
                params![run_id, step_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, attempt_id)) = step else {
            return Err(JournalError::NotFound);
        };
        if state != enum_text(&expected_step_state)?
            || attempt_id.as_deref() != Some(&intent.attempt_id)
        {
            return Err(JournalError::Stale);
        }
        let current: Option<WorkspaceSettlementRow> = tx
            .query_row(
                "SELECT authority_store_id,authority_epoch,root_binding_id,claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,claim_phase,gate_state,settlement_id,coordinator_revision,settlement_intent_id,settlement_attempt_id,settlement_decision,settlement_digest FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state='pinned'",
                [run_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?,row.get(13)?,row.get(14)?,row.get(15)?)),
            )
            .optional()?;
        let Some(current) = current else {
            return Err(JournalError::NotFound);
        };
        let expected = (
            Some(binding.authority_store_id.clone()),
            Some(binding.authority_epoch.clone()),
            Some(binding.root_binding_id.clone()),
            Some(binding.claim_id.clone()),
            Some(binding.claim_generation as i64),
            Some(binding.claim_revision as i64),
            Some(binding.gate_revision as i64),
            Some(binding.root_observation_revision as i64),
            Some(binding.phase.clone()),
            Some(binding.gate_state.clone()),
            binding.settlement_id.clone(),
            Some(binding.coordinator_revision as i64),
        );
        if (
            current.0.clone(),
            current.1.clone(),
            current.2.clone(),
            current.3.clone(),
            current.4,
            current.5,
            current.6,
            current.7,
            current.8.clone(),
            current.9.clone(),
            current.10.clone(),
            current.11,
        ) != expected
        {
            return Err(JournalError::Stale);
        }
        let requested = (
            Some(intent.observation_id.clone()),
            Some(intent.attempt_id.clone()),
            Some(intent.decision.clone()),
            Some(intent.reviewed_digest.clone()),
        );
        if (
            current.12.clone(),
            current.13.clone(),
            current.14.clone(),
            current.15.clone(),
        ) == requested
        {
            tx.commit()?;
            return Ok(());
        }
        if current.12.is_some()
            || current.13.is_some()
            || current.14.is_some()
            || current.15.is_some()
        {
            return Err(JournalError::Stale);
        }
        let changed = tx.execute(
            "UPDATE orchestration_workspace_claims SET settlement_intent_id=?1,settlement_attempt_id=?2,settlement_decision=?3,settlement_digest=?4 WHERE run_id=?5 AND proof_state='pinned' AND claim_id=?6 AND claim_generation=?7 AND claim_revision=?8 AND gate_revision=?9 AND claim_phase='unsettled' AND settlement_intent_id IS NULL",
            params![intent.observation_id,intent.attempt_id,intent.decision,intent.reviewed_digest,run_id,binding.claim_id,binding.claim_generation as i64,binding.claim_revision as i64,binding.gate_revision as i64],
        )?;
        if changed != 1 {
            return Err(JournalError::Stale);
        }
        tx.commit()?;
        Ok(())
    }

    pub fn workspace_settlement_intent(
        &self,
        run_id: &str,
    ) -> Result<Option<WorkspaceSettlementIntent>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        connection
            .query_row(
                "SELECT settlement_intent_id,settlement_attempt_id,settlement_decision,settlement_digest FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state='pinned' AND settlement_intent_id IS NOT NULL",
                [run_id],
                |row| Ok(WorkspaceSettlementIntent { observation_id: row.get(0)?, attempt_id: row.get(1)?, decision: row.get(2)?, reviewed_digest: row.get(3)? }),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Commit the F8 settlement receipt and the M6 step transition in one journal transaction.
    /// F8 remains fenced until a later explicit coordinator-revision acknowledgement.
    #[allow(clippy::too_many_arguments)] // Inputs bind the step, F8 receipt, and exact reviewed intent.
    pub fn settle_workspace_step(
        &self,
        run_id: &str,
        step_id: &str,
        expected_step_state: StepState,
        next_step_state: StepState,
        failure: Option<FailureCode>,
        expected_binding: &WorkspaceClaimBinding,
        verified_binding: &WorkspaceClaimBinding,
        intent: &WorkspaceSettlementIntent,
        now: u64,
    ) -> Result<CoordinatorSnapshot, JournalError> {
        let allowed = matches!(
            (
                expected_step_state,
                next_step_state,
                intent.decision.as_str()
            ),
            (
                StepState::Verifying,
                StepState::Succeeded,
                "confirmed_completed"
            ) | (
                StepState::NeedsReview,
                StepState::Succeeded,
                "confirmed_completed"
            ) | (
                StepState::NeedsReview,
                StepState::Failed,
                "confirmed_no_effect"
            ) | (
                StepState::NeedsReview,
                StepState::Cancelled,
                "confirmed_cancelled"
            )
        );
        if !allowed
            || !valid_digest(&intent.reviewed_digest)
            || verified_binding.authority_store_id != expected_binding.authority_store_id
            || verified_binding.authority_epoch != expected_binding.authority_epoch
            || verified_binding.root_binding_id != expected_binding.root_binding_id
            || verified_binding.claim_id != expected_binding.claim_id
            || verified_binding.claim_generation != expected_binding.claim_generation
            || verified_binding.root_observation_revision
                != expected_binding.root_observation_revision
            || verified_binding.claim_revision != expected_binding.claim_revision.saturating_add(1)
            || verified_binding.gate_revision != expected_binding.gate_revision.saturating_add(1)
            || verified_binding.phase != "verified"
            || verified_binding.gate_state != "stopped"
            || verified_binding.settlement_id.as_deref() != Some(&intent.observation_id)
            || verified_binding.coordinator_revision != expected_binding.coordinator_revision
        {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let row: Option<(String, Option<String>)> = tx
            .query_row(
                "SELECT state,current_attempt_id FROM orchestration_steps WHERE run_id=?1 AND step_id=?2",
                params![run_id, step_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((state, attempt_id)) = row else {
            return Err(JournalError::NotFound);
        };
        if state != enum_text(&expected_step_state)?
            || attempt_id.as_deref() != Some(&intent.attempt_id)
        {
            return Err(JournalError::Stale);
        }
        let claim: Option<WorkspaceSettlementRow> = tx
            .query_row(
                "SELECT authority_store_id,authority_epoch,root_binding_id,claim_id,claim_generation,claim_revision,gate_revision,root_observation_revision,claim_phase,gate_state,settlement_id,coordinator_revision,settlement_intent_id,settlement_attempt_id,settlement_decision,settlement_digest FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state='pinned'",
                [run_id],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?,row.get(7)?,row.get(8)?,row.get(9)?,row.get(10)?,row.get(11)?,row.get(12)?,row.get(13)?,row.get(14)?,row.get(15)?)),
            )
            .optional()?;
        let Some(claim) = claim else {
            return Err(JournalError::NotFound);
        };
        let expected_local = (
            Some(expected_binding.authority_store_id.clone()),
            Some(expected_binding.authority_epoch.clone()),
            Some(expected_binding.root_binding_id.clone()),
            Some(expected_binding.claim_id.clone()),
            Some(expected_binding.claim_generation as i64),
            Some(expected_binding.claim_revision as i64),
            Some(expected_binding.gate_revision as i64),
            Some(expected_binding.root_observation_revision as i64),
            Some(expected_binding.phase.clone()),
            Some(expected_binding.gate_state.clone()),
            expected_binding.settlement_id.clone(),
            Some(expected_binding.coordinator_revision as i64),
            Some(intent.observation_id.clone()),
            Some(intent.attempt_id.clone()),
            Some(intent.decision.clone()),
            Some(intent.reviewed_digest.clone()),
        );
        if (
            &claim.0, &claim.1, &claim.2, &claim.3, &claim.4, &claim.5, &claim.6, &claim.7,
            &claim.8, &claim.9, &claim.10, &claim.11,
        ) != (
            &expected_local.0,
            &expected_local.1,
            &expected_local.2,
            &expected_local.3,
            &expected_local.4,
            &expected_local.5,
            &expected_local.6,
            &expected_local.7,
            &expected_local.8,
            &expected_local.9,
            &expected_local.10,
            &expected_local.11,
        ) || (&claim.12, &claim.13, &claim.14, &claim.15)
            != (
                &expected_local.12,
                &expected_local.13,
                &expected_local.14,
                &expected_local.15,
            )
        {
            return Err(JournalError::Stale);
        }
        let next_text = enum_text(&next_step_state)?;
        let failure_text = failure.map(|value| enum_text(&value)).transpose()?;
        let changed = tx.execute(
            "UPDATE orchestration_workspace_claims SET claim_revision=?1,gate_revision=?2,claim_phase='verified',gate_state='stopped',settlement_id=?3 WHERE run_id=?4 AND proof_state='pinned' AND claim_revision=?5 AND gate_revision=?6 AND claim_phase='unsettled' AND settlement_intent_id=?3",
            params![verified_binding.claim_revision as i64,verified_binding.gate_revision as i64,intent.observation_id,run_id,expected_binding.claim_revision as i64,expected_binding.gate_revision as i64],
        )?;
        if changed != 1 {
            return Err(JournalError::Stale);
        }
        let changed = tx.execute(
            "UPDATE orchestration_steps SET state=?3,failure=?4,workspace_revision_after=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2 AND state=?7 AND current_attempt_id=?8",
            params![run_id,step_id,next_text,failure_text,if next_step_state == StepState::Succeeded { Some(intent.reviewed_digest.as_str()) } else { None },now as i64,enum_text(&expected_step_state)?,intent.attempt_id],
        )?;
        if changed != 1 {
            return Err(JournalError::Stale);
        }
        tx.execute(
            "UPDATE orchestration_attempts SET state='terminal',failure=?4,updated_at=?5 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3",
            params![run_id,step_id,intent.attempt_id,failure_text,now as i64],
        )?;
        if next_step_state == StepState::Succeeded {
            tx.execute(
                "UPDATE orchestration_runs SET revision=revision+1,current_workspace_revision=?2,updated_at=?3 WHERE run_id=?1",
                params![run_id,intent.reviewed_digest,now as i64],
            )?;
        } else {
            tx.execute(
                "UPDATE orchestration_runs SET revision=revision+1,updated_at=?2 WHERE run_id=?1",
                params![run_id, now as i64],
            )?;
        }
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(&intent.attempt_id),
            "attempt_finished",
            failure_text.as_deref(),
            now,
        )?;
        let result = get_locked(&tx, run_id)?.ok_or(JournalError::Storage)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn create_run(
        &self,
        plan: &OrchestrationPlanV1,
        review: &PlanReviewV1,
        now: u64,
    ) -> Result<CoordinatorSnapshot, JournalError> {
        plan.validate(now).map_err(|_| JournalError::Invalid)?;
        if !review.matches(plan) {
            return Err(JournalError::Stale);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let existing: Option<(String, String, String, String)> = connection.query_row(
            "SELECT run_id,plan_digest,workspace_id,workspace_revision FROM orchestration_runs WHERE plan_id=?1",
            [&plan.plan_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        if let Some((run_id, digest, workspace_id, workspace_revision)) = existing {
            if digest != review.digest
                || workspace_id != plan.workspace_id
                || workspace_revision != plan.workspace_revision
            {
                return Err(JournalError::Stale);
            }
            return get_locked(&connection, &run_id)?.ok_or(JournalError::Storage);
        }
        let existing: i64 =
            connection.query_row("SELECT count(*) FROM orchestration_runs", [], |row| {
                row.get(0)
            })?;
        if existing >= MAX_RUNS as i64 {
            return Err(JournalError::Capacity);
        }
        let run_id = Uuid::new_v4().to_string();
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
                     "INSERT INTO orchestration_runs(run_id,revision,plan_revision,plan_digest,workspace_id,workspace_revision,current_workspace_revision,state,failure,created_at,updated_at,expires_at,max_steps,max_parallelism,max_total_seconds,stop_policy,owner_digest,plan_id) VALUES(?1,1,?2,?3,?4,?5,?5,'ready',NULL,?6,?6,?7,?8,1,?9,?10,?3,?11)",
            params![
                run_id,
                plan.revision as i64,
                review.digest,
                plan.workspace_id,
                plan.workspace_revision,
                now as i64,
                plan.expires_at_unix as i64,
                plan.limits.max_steps as i64,
                plan.limits.max_total_seconds as i64,
                enum_text(&plan.limits.stop_policy)?,
                plan.plan_id,
            ],
        )?;
        for (ordinal, step) in plan.steps.iter().enumerate() {
            insert_step(&tx, &run_id, ordinal, step, now)?;
        }
        match tx.execute(
            "INSERT INTO orchestration_workspace_claims(workspace_id,run_id,claimed_at) VALUES(?1,?2,?3)",
            params![plan.workspace_id, run_id, now as i64],
        ) {
            Ok(_) => (),
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Err(JournalError::WorkspaceConflict);
            }
            Err(error) => return Err(error.into()),
        }
        add_event(&tx, &run_id, None, None, "run_created", None, now)?;
        tx.commit()?;
        drop(connection);
        self.get(&run_id)?.ok_or(JournalError::Storage)
    }

    /// Current exclusive claims, including needs-review runs recovered without volatile paths.
    /// Only opaque workspace identities cross the durable boundary.
    pub fn claimed_workspaces(&self) -> Result<Vec<(String, String)>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT run_id,workspace_id FROM orchestration_workspace_claims WHERE claim_phase IS NULL OR claim_phase<>'released' ORDER BY run_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let claims = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(claims)
    }

    pub fn list(&self, limit: usize) -> Result<Vec<CoordinatorSnapshot>, JournalError> {
        if limit == 0 || limit > MAX_LIST {
            return Err(JournalError::Invalid);
        }
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let mut statement = connection.prepare(
            "SELECT run_id FROM orchestration_runs ORDER BY created_at DESC,run_id DESC LIMIT ?1",
        )?;
        let ids = statement
            .query_map([limit as i64], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| get_locked(&connection, &id)?.ok_or(JournalError::Storage))
            .collect()
    }

    pub fn get(&self, run_id: &str) -> Result<Option<CoordinatorSnapshot>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        get_locked(&connection, run_id)
    }

    /// Stores only reviewed handoff provenance and digest. The envelope and generated first
    /// message remain volatile; this row is linked to the same run intent before dispatch.
    pub fn record_handoff(
        &self,
        run_id: &str,
        handoff: &HandoffEnvelopeV1,
        confirmation: &HandoffConfirmationV1,
        now: u64,
    ) -> Result<(), JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.unchecked_transaction()?;
        let inserted = tx.execute(
            "INSERT INTO orchestration_handoffs(handoff_id,revision,source_host_id,source_session_id,source_generation,source_plane,source_project_id,source_workspace_id,source_workspace_revision,target_host_id,target_project_id,target_workspace_id,target_workspace_revision,target_provider,target_session_id,content_digest,confirmed_at,run_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                handoff.handoff_id,
                handoff.revision as i64,
                handoff.source.host_id,
                handoff.source.session_id,
                handoff.source.session_generation as i64,
                enum_text(&handoff.source.plane)?,
                handoff.source.project_id,
                handoff.source.workspace_id,
                handoff.source.workspace_revision,
                handoff.target.host_id,
                handoff.target.project_id,
                handoff.target.workspace_id,
                handoff.target.workspace_revision,
                enum_text(&handoff.target.provider)?,
                handoff.target.managed_session_id,
                handoff.content_digest,
                confirmation.confirmed_at_unix as i64,
                run_id,
            ],
        )?;
        if inserted != 1 {
            return Err(JournalError::Storage);
        }
        add_event(&tx, run_id, None, None, "handoff_reviewed", None, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Returns the existing accepted run for a retried IPC submit. The handoff id is a
    /// user-reviewed idempotency key; a different revision/digest is rejected.
    pub fn handoff_run(
        &self,
        handoff_id: &str,
        revision: u64,
        content_digest: &str,
        confirmed_at: u64,
    ) -> Result<Option<CoordinatorSnapshot>, JournalError> {
        let connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let existing: Option<(i64, String, i64, String)> = connection.query_row(
            "SELECT revision,content_digest,confirmed_at,run_id FROM orchestration_handoffs WHERE handoff_id=?1",
            [handoff_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).optional()?;
        let Some((stored_revision, stored_digest, stored_confirmed_at, run_id)) = existing else {
            return Ok(None);
        };
        if stored_revision != revision as i64
            || stored_digest != content_digest
            || stored_confirmed_at != confirmed_at as i64
        {
            return Err(JournalError::Stale);
        }
        get_locked(&connection, &run_id)?
            .map(Some)
            .ok_or(JournalError::Storage)
    }

    pub fn set_run_state(
        &self,
        run_id: &str,
        expected_revision: u64,
        next: RunState,
        failure: Option<FailureCode>,
        now: u64,
    ) -> Result<CoordinatorSnapshot, JournalError> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = get_locked(&tx, run_id)?.ok_or(JournalError::NotFound)?;
        if current.revision != expected_revision {
            return Err(JournalError::Stale);
        }
        if !valid_run_transition(current.state, next) {
            return Err(JournalError::InvalidTransition);
        }
        tx.execute(
            "UPDATE orchestration_runs SET revision=revision+1,state=?2,failure=?3,updated_at=?4 WHERE run_id=?1 AND revision=?5",
            params![run_id, enum_text(&next)?, failure.map(|v| enum_text(&v)).transpose()?, now as i64, expected_revision as i64],
        )?;
        // A terminal M6 row is not authority to release an F8-owned claim. Retain the
        // immutable binding until an F8 settlement/release receipt is mirrored explicitly.
        if next.terminal() {
            tx.execute(
                "DELETE FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state<>'pinned'",
                [run_id],
            )?;
        }
        add_event(
            &tx,
            run_id,
            None,
            None,
            "run_state_changed",
            failure.map(|v| enum_text(&v)).transpose()?.as_deref(),
            now,
        )?;
        tx.commit()?;
        drop(connection);
        self.get(run_id)?.ok_or(JournalError::Storage)
    }

    pub fn set_step_state(
        &self,
        run_id: &str,
        step_id: &str,
        expected: StepState,
        next: StepState,
        failure: Option<FailureCode>,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !valid_step_transition(expected, next) {
            return Err(JournalError::InvalidTransition);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_steps SET state=?3,failure=?4,updated_at=?5 WHERE run_id=?1 AND step_id=?2 AND state=?6",
            params![run_id, step_id, enum_text(&next)?, failure.map(|v| enum_text(&v)).transpose()?, now as i64, enum_text(&expected)?],
        )?;
        if updated != 1 {
            return Err(if get_locked(&tx, run_id)?.is_none() {
                JournalError::NotFound
            } else {
                JournalError::Stale
            });
        }
        bump_run(&tx, run_id, now)?;
        if next == StepState::NeedsReview {
            tx.execute(
                "UPDATE orchestration_runs SET state='needs_review',failure=?2,updated_at=?3 WHERE run_id=?1 AND state IN ('running','pausing','cancelling','ready')",
                params![run_id, failure.map(|v| enum_text(&v)).transpose()?, now as i64],
            )?;
        }
        add_event(
            &tx,
            run_id,
            Some(step_id),
            None,
            "step_state_changed",
            failure.map(|v| enum_text(&v)).transpose()?.as_deref(),
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    /// The attempt row and dispatch marker are committed before the Managed authority is called.
    // These values are the complete, atomic identity fence for one durable dispatch.
    #[allow(clippy::too_many_arguments)]
    pub fn begin_attempt(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        intent_id: &str,
        authority_id: &str,
        runtime_id: &str,
        generation: u64,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !valid_uuid(attempt_id)
            || !valid_uuid(intent_id)
            || !valid_uuid(authority_id)
            || !valid_uuid(runtime_id)
            || generation == 0
        {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let step = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        if !matches!(step.state, StepState::Eligible | StepState::WaitingResource)
            || step.attempt_count >= MAX_ATTEMPTS
        {
            return Err(JournalError::InvalidTransition);
        }
        tx.execute(
            "INSERT INTO orchestration_attempts(run_id,step_id,attempt_id,authority_intent_id,state,authority_instance_id,runtime_instance_id,runtime_generation,managed_turn_id,turn_revision,failure,started_at,updated_at) VALUES(?1,?2,?3,?4,'dispatching',?5,?6,?7,NULL,NULL,NULL,?8,?8)",
            params![run_id,step_id,attempt_id,intent_id,authority_id,runtime_id,generation as i64,now as i64],
        )?;
        tx.execute(
            "UPDATE orchestration_steps SET state='starting',attempt_count=attempt_count+1,current_attempt_id=?3,managed_turn_id=NULL,authority_instance_id=?4,runtime_instance_id=?5,runtime_generation=?6,turn_revision=NULL,failure=NULL,updated_at=?7 WHERE run_id=?1 AND step_id=?2",
            params![run_id,step_id,attempt_id,authority_id,runtime_id,generation as i64,now as i64],
        )?;
        bump_run(&tx, run_id, now)?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_started",
            None,
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::Storage)?;
        tx.commit()?;
        Ok(result)
    }

    pub fn acknowledge_attempt(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        turn_id: &str,
        turn_revision: u64,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !valid_uuid(turn_id) || turn_revision == 0 {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_attempts SET state='acknowledged',managed_turn_id=?4,turn_revision=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state='dispatching'",
            params![run_id,step_id,attempt_id,turn_id,turn_revision as i64,now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        tx.execute(
            "UPDATE orchestration_steps SET state='running',managed_turn_id=?4,turn_revision=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2 AND current_attempt_id=?3 AND state='starting'",
            params![run_id,step_id,attempt_id,turn_id,turn_revision as i64,now as i64],
        )?;
        bump_run(&tx, run_id, now)?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_acknowledged",
            None,
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    /// Commit the F8 L1 acknowledgement and its one-step claim revision advance together.
    /// A missing/stale local claim rolls back the turn acknowledgement; it never retries F8.
    #[allow(clippy::too_many_arguments)] // Mirrors the immutable attempt tuple and F8 acknowledgement.
    pub fn acknowledge_guarded_attempt(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        turn_id: &str,
        turn_revision: u64,
        claim: &WorkspaceClaimBinding,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !valid_uuid(turn_id) || turn_revision == 0 {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_attempts SET state='acknowledged',managed_turn_id=?4,turn_revision=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state='dispatching'",
            params![run_id,step_id,attempt_id,turn_id,turn_revision as i64,now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        let advanced = tx.execute(
            "UPDATE orchestration_workspace_claims SET claim_revision=claim_revision+1,claim_phase='unsettled' WHERE run_id=?1 AND proof_state='pinned' AND authority_store_id=?2 AND authority_epoch=?3 AND root_binding_id=?4 AND claim_id=?5 AND claim_generation=?6 AND claim_revision=?7 AND gate_revision=?8 AND claim_phase='held' AND gate_state='open'",
            params![run_id,claim.authority_store_id,claim.authority_epoch,claim.root_binding_id,claim.claim_id,claim.claim_generation as i64,claim.claim_revision as i64,claim.gate_revision as i64],
        )?;
        if advanced != 1 {
            return Err(JournalError::Stale);
        }
        tx.execute(
            "UPDATE orchestration_steps SET state='running',managed_turn_id=?4,turn_revision=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2 AND current_attempt_id=?3 AND state='starting'",
            params![run_id,step_id,attempt_id,turn_id,turn_revision as i64,now as i64],
        )?;
        bump_run(&tx, run_id, now)?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_acknowledged",
            None,
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    /// Persists the cancellation intent before invoking the authority. A restart converts an
    /// unanswered request to unknown; it is never retransmitted automatically.
    pub fn begin_cancellation(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        intent_id: &str,
        now: u64,
    ) -> Result<CancellationOutcome, JournalError> {
        if !valid_uuid(attempt_id) || !valid_uuid(intent_id) {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx.query_row(
            "SELECT state FROM orchestration_cancellations WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3",
            params![run_id, step_id, attempt_id],
            |row| row.get(0),
        ).optional()?;
        if let Some(state) = existing {
            let outcome = match state.as_str() {
                // A previous request may have reached the authority before its reply was
                // lost. Even if this process is still alive, the same intent is not resent.
                "requested" => CancellationOutcome::Unknown,
                "acknowledged" => CancellationOutcome::Acknowledged,
                "refused" => CancellationOutcome::Refused,
                "unknown" => CancellationOutcome::Unknown,
                _ => return Err(JournalError::Incompatible),
            };
            tx.commit()?;
            return Ok(outcome);
        }
        let step = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        if step.attempt_id.as_deref() != Some(attempt_id)
            || !matches!(step.state, StepState::Starting | StepState::Running)
        {
            return Err(JournalError::Stale);
        }
        tx.execute(
            "INSERT INTO orchestration_cancellations(run_id,step_id,attempt_id,intent_id,state,started_at,updated_at) VALUES(?1,?2,?3,?4,'requested',?5,?5)",
            params![run_id, step_id, attempt_id, intent_id, now as i64],
        )?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "cancellation_started",
            None,
            now,
        )?;
        tx.commit()?;
        Ok(CancellationOutcome::Requested)
    }

    pub fn finish_cancellation(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        outcome: CancellationOutcome,
        now: u64,
    ) -> Result<(), JournalError> {
        let (state, event) = match outcome {
            CancellationOutcome::Requested => return Err(JournalError::Invalid),
            CancellationOutcome::Acknowledged => ("acknowledged", "cancellation_acknowledged"),
            CancellationOutcome::Refused => ("refused", "cancellation_refused"),
            CancellationOutcome::Unknown => ("unknown", "cancellation_unknown"),
        };
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_cancellations SET state=?4,updated_at=?5 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state='requested'",
            params![run_id, step_id, attempt_id, state, now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            event,
            None,
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn mark_attempt_unknown(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_attempts SET state='unknown',failure='dispatch_unknown',updated_at=?4 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state IN ('dispatching','acknowledged','running')",
            params![run_id,step_id,attempt_id,now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        tx.execute(
            "UPDATE orchestration_steps SET state='needs_review',failure='dispatch_unknown',updated_at=?4 WHERE run_id=?1 AND step_id=?2 AND current_attempt_id=?3",
            params![run_id,step_id,attempt_id,now as i64],
        )?;
        tx.execute(
            "UPDATE orchestration_runs SET state='needs_review',failure='dispatch_unknown',updated_at=?2 WHERE run_id=?1 AND state IN ('running','pausing','cancelling','ready')",
            params![run_id, now as i64],
        )?;
        bump_run(&tx, run_id, now)?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_unknown",
            Some("dispatch_unknown"),
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    /// A typed authority refusal is known not to have dispatched. Keep the attempt evidence,
    /// but do not consume the plan's bounded *effectful* attempt count.
    pub fn refuse_attempt(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        failure: FailureCode,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let updated = tx.execute(
            "UPDATE orchestration_attempts SET state='refused',failure=?4,updated_at=?5 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state='dispatching'",
            params![run_id, step_id, attempt_id, enum_text(&failure)?, now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        let updated = tx.execute(
            "UPDATE orchestration_steps SET state='waiting_resource',attempt_count=CASE WHEN attempt_count>0 THEN attempt_count-1 ELSE 0 END,current_attempt_id=NULL,managed_turn_id=NULL,authority_instance_id=NULL,runtime_instance_id=NULL,runtime_generation=NULL,turn_revision=NULL,failure=?4,updated_at=?5 WHERE run_id=?1 AND step_id=?2 AND current_attempt_id=?3 AND state='starting'",
            params![run_id, step_id, attempt_id, enum_text(&failure)?, now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::Stale);
        }
        bump_run(&tx, run_id, now)?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_finished",
            Some(&enum_text(&failure)?),
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    // A turn observation is committed with its run, step, attempt and expected revision.
    #[allow(clippy::too_many_arguments)]
    pub fn set_turn_observation(
        &self,
        run_id: &str,
        step_id: &str,
        attempt_id: &str,
        turn_revision: u64,
        next: StepState,
        failure: Option<FailureCode>,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !matches!(
            next,
            StepState::Running
                | StepState::Verifying
                | StepState::Failed
                | StepState::NeedsReview
                | StepState::Cancelled
        ) {
            return Err(JournalError::InvalidTransition);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        if current.attempt_id.as_deref() != Some(attempt_id)
            || !matches!(
                current.state,
                StepState::Starting | StepState::Running | StepState::NeedsReview
            )
        {
            return Err(JournalError::Stale);
        }
        let turn_id = current.managed_turn_id.as_deref();
        let attempt_state = match next {
            StepState::Failed | StepState::Cancelled => "terminal",
            StepState::NeedsReview => "unknown",
            _ => "running",
        };
        tx.execute(
            "UPDATE orchestration_attempts SET state=?4,turn_revision=?5,failure=?6,updated_at=?7 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3",
            params![run_id,step_id,attempt_id,attempt_state,turn_revision as i64,failure.map(|v| enum_text(&v)).transpose()?,now as i64],
        )?;
        tx.execute(
            "UPDATE orchestration_steps SET state=?3,turn_revision=?4,failure=?5,updated_at=?6 WHERE run_id=?1 AND step_id=?2",
            params![run_id,step_id,enum_text(&next)?,turn_revision as i64,failure.map(|v| enum_text(&v)).transpose()?,now as i64],
        )?;
        bump_run(&tx, run_id, now)?;
        if next == StepState::NeedsReview {
            tx.execute(
                "UPDATE orchestration_runs SET state='needs_review',failure=?2,updated_at=?3 WHERE run_id=?1 AND state IN ('running','pausing','cancelling','ready')",
                params![run_id, failure.map(|v| enum_text(&v)).transpose()?, now as i64],
            )?;
        }
        add_event(
            &tx,
            run_id,
            Some(step_id),
            Some(attempt_id),
            "attempt_finished",
            failure.map(|v| enum_text(&v)).transpose()?.as_deref(),
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        let _ = turn_id;
        tx.commit()?;
        Ok(result)
    }

    pub fn verify_step(
        &self,
        run_id: &str,
        step_id: &str,
        expected_workspace_revision: &str,
        now: u64,
    ) -> Result<StepSnapshot, JournalError> {
        if !valid_digest(expected_workspace_revision) {
            return Err(JournalError::Invalid);
        }
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current: Option<String> = tx
            .query_row(
                "SELECT current_workspace_revision FROM orchestration_runs WHERE run_id=?1",
                [run_id],
                |row| row.get(0),
            )
            .optional()?;
        let _current = current.ok_or(JournalError::NotFound)?;
        let step: Option<String> = tx
            .query_row(
                "SELECT state FROM orchestration_steps WHERE run_id=?1 AND step_id=?2",
                params![run_id, step_id],
                |row| row.get(0),
            )
            .optional()?;
        if step.as_deref() != Some("verifying") {
            return Err(JournalError::InvalidTransition);
        }
        let updated = tx.execute(
            "UPDATE orchestration_steps SET state='succeeded',failure=NULL,workspace_revision_after=?3,updated_at=?4 WHERE run_id=?1 AND step_id=?2 AND state='verifying'",
            params![run_id,step_id,expected_workspace_revision,now as i64],
        )?;
        if updated != 1 {
            return Err(JournalError::InvalidTransition);
        }
        // The owner has explicitly reviewed the observed revision. Promotion is the only
        // operation that may advance the workspace head after a step.
        tx.execute(
            "UPDATE orchestration_runs SET revision=revision+1,current_workspace_revision=?2,updated_at=?3 WHERE run_id=?1",
            params![run_id, expected_workspace_revision, now as i64],
        )?;
        add_event(
            &tx,
            run_id,
            Some(step_id),
            None,
            "step_state_changed",
            None,
            now,
        )?;
        let result = get_step_locked(&tx, run_id, step_id)?.ok_or(JournalError::NotFound)?;
        tx.commit()?;
        Ok(result)
    }

    /// A restarted coordinator never resumes work. Dispatching attempts become unknown and
    /// every nonterminal step is held for owner review, including work that had not started.
    pub fn recover_after_restart(
        &self,
        now: u64,
    ) -> Result<Vec<CoordinatorSnapshot>, JournalError> {
        let mut connection = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let tx = connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let run_ids = {
            let mut q = tx.prepare(
                "SELECT run_id FROM orchestration_runs WHERE state IN ('ready','running','pausing','paused','cancelling') ORDER BY created_at,run_id",
            )?;
            q.query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut recovered = Vec::new();
        for run_id in run_ids {
            let pending_cancellations = {
                let mut query = tx.prepare(
                    "SELECT step_id,attempt_id FROM orchestration_cancellations WHERE run_id=?1 AND state='requested'",
                )?;
                query
                    .query_map([&run_id], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?
            };
            for (step_id, attempt_id) in pending_cancellations {
                tx.execute(
                    "UPDATE orchestration_cancellations SET state='unknown',updated_at=?4 WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3 AND state='requested'",
                    params![run_id, step_id, attempt_id, now as i64],
                )?;
                add_event(
                    &tx,
                    &run_id,
                    Some(&step_id),
                    Some(&attempt_id),
                    "cancellation_unknown",
                    None,
                    now,
                )?;
            }
            tx.execute(
                "UPDATE orchestration_attempts SET state='unknown',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state IN ('dispatching','acknowledged','running')",
                params![run_id,now as i64],
            )?;
            // A step with no attempt record and no current attempt ID cannot have reached F8:
            // M6 persists the attempt intent before sending the guarded start. Cancel only
            // those never-started steps; any inconsistent or possibly attempted row stays
            // in NeedsReview and keeps the authority claim fenced.
            tx.execute(
                "UPDATE orchestration_steps AS s SET state='cancelled',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state IN ('proposed','waiting_dependencies','eligible','waiting_resource','awaiting_permission','queued') AND current_attempt_id IS NULL AND NOT EXISTS(SELECT 1 FROM orchestration_attempts a WHERE a.run_id=s.run_id AND a.step_id=s.step_id)",
                params![run_id,now as i64],
            )?;
            tx.execute(
                "UPDATE orchestration_steps SET state='needs_review',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state NOT IN ('succeeded','skipped','blocked','failed','cancelled')",
                params![run_id,now as i64],
            )?;
            tx.execute(
                "UPDATE orchestration_runs SET revision=revision+1,state='needs_review',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state NOT IN ('completed','failed','cancelled','expired')",
                params![run_id,now as i64],
            )?;
            add_event(
                &tx,
                &run_id,
                None,
                None,
                "recovered_after_restart",
                Some("coordinator_restarted"),
                now,
            )?;
            if let Some(view) = get_locked(&tx, &run_id)? {
                record_recovered(&mut recovered, view);
            }
        }
        // A prior owner-confirmed reconciliation can be interrupted between SQLite
        // transactions. A needs-review run may then contain a Verifying step or a
        // fully resolved result with no remaining reviewable step. Normalize both
        // cases under this single transaction; never re-dispatch a lost volatile plan.
        let review_ids = {
            let mut statement = tx.prepare(
                "SELECT run_id FROM orchestration_runs WHERE state='needs_review' ORDER BY created_at,run_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for run_id in review_ids {
            let before = get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?;
            let mut changed = false;
            for step in &before.steps {
                let next = match step.state {
                    StepState::Verifying | StepState::Starting | StepState::Running => {
                        Some(StepState::NeedsReview)
                    }
                    StepState::Proposed
                    | StepState::WaitingDependencies
                    | StepState::Eligible
                    | StepState::WaitingResource
                    | StepState::AwaitingPermission
                    | StepState::Queued
                        if step.attempt_id.is_none() =>
                    {
                        Some(StepState::Cancelled)
                    }
                    StepState::Proposed
                    | StepState::WaitingDependencies
                    | StepState::Eligible
                    | StepState::WaitingResource
                    | StepState::AwaitingPermission
                    | StepState::Queued => Some(StepState::NeedsReview),
                    _ => None,
                };
                if let Some(next) = next {
                    tx.execute(
                        "UPDATE orchestration_steps SET state=?3,failure='coordinator_restarted',updated_at=?4 WHERE run_id=?1 AND step_id=?2",
                        params![run_id, step.step_id, enum_text(&next)?, now as i64],
                    )?;
                    changed = true;
                }
            }
            if changed {
                tx.execute(
                    "UPDATE orchestration_attempts SET state='unknown',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state IN ('dispatching','acknowledged','running')",
                    params![run_id, now as i64],
                )?;
                bump_run(&tx, &run_id, now)?;
                add_event(
                    &tx,
                    &run_id,
                    None,
                    None,
                    "recovered_after_restart",
                    Some("coordinator_restarted"),
                    now,
                )?;
            }
            let snapshot = get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?;
            if snapshot.steps.iter().all(|step| step.state.terminal()) {
                let failed = snapshot.steps.iter().any(|step| {
                    step.required && matches!(step.state, StepState::Failed | StepState::Blocked)
                });
                let cancelled = snapshot
                    .steps
                    .iter()
                    .any(|step| step.required && step.state == StepState::Cancelled);
                let next = if failed {
                    RunState::Failed
                } else if cancelled {
                    RunState::Cancelled
                } else {
                    RunState::Completed
                };
                tx.execute(
                    "UPDATE orchestration_runs SET revision=revision+1,state=?2,failure=?3,updated_at=?4 WHERE run_id=?1 AND state='needs_review'",
                    params![
                        run_id,
                        enum_text(&next)?,
                        if failed || cancelled {
                            Some(enum_text(&super::FailureCode::OwnerReviewRequired)?)
                        } else {
                            None
                        },
                        now as i64
                    ],
                )?;
                // Recovery may close historical metadata, but cannot release the durable F8
                // ownership record. A later explicit F8 release is required.
                tx.execute(
                    "DELETE FROM orchestration_workspace_claims WHERE run_id=?1 AND proof_state<>'pinned'",
                    [&run_id],
                )?;
                add_event(
                    &tx,
                    &run_id,
                    None,
                    None,
                    "run_state_changed",
                    (failed || cancelled).then_some("owner_review_required"),
                    now,
                )?;
                changed = true;
            }
            if changed {
                record_recovered(
                    &mut recovered,
                    get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?,
                );
            }
        }
        // A prior owner-confirmed reconciliation can be interrupted between SQLite
        // transactions. A needs-review run may then contain a Verifying step or a
        // fully resolved result with no remaining reviewable step. Normalize both
        // cases under this single transaction; never re-dispatch a lost volatile plan.
        let review_ids = {
            let mut statement = tx.prepare(
                "SELECT run_id FROM orchestration_runs WHERE state='needs_review' ORDER BY created_at,run_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for run_id in review_ids {
            let before = get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?;
            let mut changed = false;
            for step in &before.steps {
                let next = match step.state {
                    StepState::Verifying | StepState::Starting | StepState::Running => {
                        Some(StepState::NeedsReview)
                    }
                    StepState::Proposed
                    | StepState::WaitingDependencies
                    | StepState::Eligible
                    | StepState::WaitingResource
                    | StepState::AwaitingPermission
                    | StepState::Queued => Some(StepState::Cancelled),
                    _ => None,
                };
                if let Some(next) = next {
                    tx.execute(
                        "UPDATE orchestration_steps SET state=?3,failure='coordinator_restarted',updated_at=?4 WHERE run_id=?1 AND step_id=?2",
                        params![run_id, step.step_id, enum_text(&next)?, now as i64],
                    )?;
                    changed = true;
                }
            }
            if changed {
                tx.execute(
                    "UPDATE orchestration_attempts SET state='unknown',failure='coordinator_restarted',updated_at=?2 WHERE run_id=?1 AND state IN ('dispatching','acknowledged','running')",
                    params![run_id, now as i64],
                )?;
                bump_run(&tx, &run_id, now)?;
                add_event(
                    &tx,
                    &run_id,
                    None,
                    None,
                    "recovered_after_restart",
                    Some("coordinator_restarted"),
                    now,
                )?;
            }
            let snapshot = get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?;
            if snapshot.steps.iter().all(|step| step.state.terminal()) {
                let failed = snapshot.steps.iter().any(|step| {
                    step.required && matches!(step.state, StepState::Failed | StepState::Blocked)
                });
                let cancelled = snapshot
                    .steps
                    .iter()
                    .any(|step| step.required && step.state == StepState::Cancelled);
                let next = if failed {
                    RunState::Failed
                } else if cancelled {
                    RunState::Cancelled
                } else {
                    RunState::Completed
                };
                tx.execute(
                    "UPDATE orchestration_runs SET revision=revision+1,state=?2,failure=?3,updated_at=?4 WHERE run_id=?1 AND state='needs_review'",
                    params![
                        run_id,
                        enum_text(&next)?,
                        if failed || cancelled {
                            Some(enum_text(&super::FailureCode::OwnerReviewRequired)?)
                        } else {
                            None
                        },
                        now as i64
                    ],
                )?;
                tx.execute(
                    "DELETE FROM orchestration_workspace_claims WHERE run_id=?1",
                    [&run_id],
                )?;
                add_event(
                    &tx,
                    &run_id,
                    None,
                    None,
                    "run_state_changed",
                    (failed || cancelled).then_some("owner_review_required"),
                    now,
                )?;
                changed = true;
            }
            if changed {
                recovered.push(get_locked(&tx, &run_id)?.ok_or(JournalError::Storage)?);
            }
        }
        tx.commit()?;
        Ok(recovered)
    }
}

fn record_recovered(recovered: &mut Vec<CoordinatorSnapshot>, view: CoordinatorSnapshot) {
    if let Some(existing) = recovered.iter_mut().find(|item| item.run_id == view.run_id) {
        *existing = view;
    } else {
        recovered.push(view);
    }
}

fn insert_step(
    tx: &Transaction<'_>,
    run_id: &str,
    ordinal: usize,
    step: &StepPlanV1,
    now: u64,
) -> Result<(), JournalError> {
    let dependency_json =
        serde_json::to_string(&step.dependencies).map_err(|_| JournalError::Invalid)?;
    let input_digest = digest(&step.prompt);
    tx.execute(
        "INSERT INTO orchestration_steps(run_id,step_id,ordinal,dependencies_json,required,provider,effect,managed_session_id,model_id,input_digest,max_attempts,max_duration_seconds,state,attempt_count,current_attempt_id,managed_turn_id,authority_instance_id,runtime_instance_id,runtime_generation,turn_revision,workspace_revision_after,failure,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,?14)",
        params![
            run_id,
            step.step_id,
            ordinal as i64,
            dependency_json,
            i64::from(step.required),
            enum_text(&step.provider)?,
            enum_text(&step.effect)?,
            step.managed_session_id,
            step.model_id,
            input_digest,
            step.max_attempts as i64,
            step.max_duration_seconds as i64,
            enum_text(&if step.dependencies.is_empty() { StepState::Eligible } else { StepState::WaitingDependencies })?,
            now as i64,
        ],
    )?;
    Ok(())
}

type RunBaseRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    RunState,
    Option<FailureCode>,
    i64,
    i64,
);

fn get_locked(
    connection: &Connection,
    run_id: &str,
) -> Result<Option<CoordinatorSnapshot>, JournalError> {
    let base: Option<RunBaseRow> = connection
        .query_row(
            "SELECT revision,plan_revision,plan_digest,workspace_id,workspace_revision,current_workspace_revision,state,failure,created_at,updated_at FROM orchestration_runs WHERE run_id=?1",
            [run_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,decode_text(row.get::<_,String>(6)?)?,row.get::<_,Option<String>>(7)?.map(decode_text).transpose()?,row.get(8)?,row.get(9)?)),
        )
        .optional()?;
    let Some((
        revision,
        plan_revision,
        plan_digest,
        workspace_id,
        _reviewed_workspace_revision,
        current_workspace_revision,
        state,
        failure,
        created_at_unix,
        updated_at_unix,
    )) = base
    else {
        return Ok(None);
    };
    let mut statement = connection.prepare(
        "SELECT step_id,dependencies_json,required,provider,effect,managed_session_id,model_id,state,attempt_count,current_attempt_id,managed_turn_id,authority_instance_id,runtime_instance_id,runtime_generation,turn_revision,workspace_revision_after,failure,updated_at FROM orchestration_steps WHERE run_id=?1 ORDER BY ordinal",
    )?;
    let steps = statement
        .query_map([run_id], |row| {
            let dependencies = serde_json::from_str::<Vec<DependencyV1>>(&row.get::<_, String>(1)?)
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
            Ok(StepSnapshot {
                step_id: row.get(0)?,
                dependencies,
                required: row.get::<_, i64>(2)? == 1,
                provider: decode_text(row.get::<_, String>(3)?)?,
                effect: decode_text(row.get::<_, String>(4)?)?,
                managed_session_id: row.get(5)?,
                model_id: row.get(6)?,
                state: decode_text(row.get::<_, String>(7)?)?,
                attempt_count: row.get::<_, i64>(8)? as u8,
                attempt_id: row.get(9)?,
                managed_turn_id: row.get(10)?,
                authority_instance_id: row.get(11)?,
                runtime_instance_id: row.get(12)?,
                runtime_generation: row.get::<_, Option<i64>>(13)?.map(|v| v as u64),
                turn_revision: row.get::<_, Option<i64>>(14)?.map(|v| v as u64),
                workspace_revision_after: row.get(15)?,
                failure: row
                    .get::<_, Option<String>>(16)?
                    .map(decode_text)
                    .transpose()?,
                updated_at_unix: row.get::<_, i64>(17)? as u64,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(CoordinatorSnapshot {
        run_id: run_id.to_owned(),
        revision: revision as u64,
        plan_revision: plan_revision as u64,
        plan_digest,
        workspace_id,
        current_workspace_revision,
        state,
        failure,
        created_at_unix: created_at_unix as u64,
        updated_at_unix: updated_at_unix as u64,
        steps,
    }))
}

fn bump_run(tx: &Transaction<'_>, run_id: &str, now: u64) -> Result<(), JournalError> {
    let updated = tx.execute(
        "UPDATE orchestration_runs SET revision=revision+1,updated_at=?2 WHERE run_id=?1",
        params![run_id, now as i64],
    )?;
    if updated != 1 {
        return Err(JournalError::NotFound);
    }
    Ok(())
}

fn get_step_locked(
    connection: &Connection,
    run_id: &str,
    step_id: &str,
) -> Result<Option<StepSnapshot>, JournalError> {
    let run = get_locked(connection, run_id)?;
    Ok(run.and_then(|run| run.steps.into_iter().find(|step| step.step_id == step_id)))
}

fn add_event(
    tx: &Transaction<'_>,
    run_id: &str,
    step_id: Option<&str>,
    attempt_id: Option<&str>,
    kind: &str,
    outcome: Option<&str>,
    now: u64,
) -> Result<(), JournalError> {
    if !EVENT_KINDS.contains(&kind) {
        return Err(JournalError::Invalid);
    }
    tx.execute(
        "INSERT INTO orchestration_events(run_id,step_id,attempt_id,event_kind,outcome,timestamp) VALUES(?1,?2,?3,?4,?5,?6)",
        params![run_id,step_id,attempt_id,kind,outcome,now as i64],
    )?;
    Ok(())
}

fn valid_run_transition(from: RunState, to: RunState) -> bool {
    use RunState as R;
    matches!(
        (from, to),
        (
            R::Ready,
            R::Running | R::Cancelled | R::Expired | R::NeedsReview | R::Blocked
        ) | (
            R::Running,
            R::Pausing
                | R::Cancelling
                | R::Completed
                | R::Failed
                | R::Expired
                | R::NeedsReview
                | R::Blocked
        ) | (
            R::Pausing,
            R::Paused | R::Expired | R::NeedsReview | R::Cancelling
        ) | (
            R::Paused,
            R::Running | R::Cancelling | R::Expired | R::NeedsReview
        ) | (
            R::Cancelling,
            R::Cancelled | R::Expired | R::NeedsReview | R::Failed
        ) | (
            R::NeedsReview,
            R::Running | R::Cancelling | R::Cancelled | R::Failed | R::Completed
        )
    )
}

fn valid_step_transition(from: StepState, to: StepState) -> bool {
    use StepState as S;
    matches!(
        (from, to),
        (
            S::Proposed,
            S::WaitingDependencies | S::Eligible | S::Blocked | S::Cancelled | S::NeedsReview
        ) | (
            S::WaitingDependencies,
            S::Eligible | S::Blocked | S::Skipped | S::Cancelled | S::Failed | S::NeedsReview
        ) | (
            S::Eligible,
            S::WaitingResource
                | S::AwaitingPermission
                | S::Queued
                | S::Starting
                | S::Blocked
                | S::Cancelled
                | S::NeedsReview
        ) | (
            S::WaitingResource,
            S::Eligible | S::Queued | S::Blocked | S::Cancelled | S::Failed | S::NeedsReview
        ) | (
            S::AwaitingPermission,
            S::Queued | S::Blocked | S::Cancelled | S::Failed
        ) | (S::Queued, S::Starting | S::Cancelled | S::Blocked)
            | (
                S::Starting,
                S::Running | S::NeedsReview | S::Failed | S::Cancelled
            )
            | (
                S::Running,
                S::Running | S::Verifying | S::Failed | S::NeedsReview | S::Cancelled
            )
            | (
                S::Verifying,
                S::Succeeded | S::Failed | S::NeedsReview | S::Cancelled
            )
            | (
                S::NeedsReview,
                S::Running | S::Verifying | S::Failed | S::Cancelled
            )
    )
}

fn digest(text: &str) -> String {
    let bytes = Sha256::digest(text.as_bytes());
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value.to_ascii_lowercase())
}

fn enum_text<T: Serialize>(value: &T) -> Result<String, JournalError> {
    serde_json::to_value(value)
        .map_err(|_| JournalError::Invalid)?
        .as_str()
        .map(str::to_owned)
        .ok_or(JournalError::Invalid)
}

fn decode_text<T: DeserializeOwned>(value: String) -> Result<T, rusqlite::Error> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn failure_sql() -> String {
    super::FailureCode::all_sql()
}
fn events_sql() -> String {
    EVENT_KINDS
        .iter()
        .map(|s| format!("'{s}'"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::{
        DependencyV1, EffectClass, HandoffConfirmationV1, HandoffEnvelopeV1, HandoffItemKind,
        HandoffItemV1, HandoffPlane, HandoffProvenance, HandoffSourceV1, HandoffTargetV1,
        OrchestrationPlanV1, PlanLimitsV1, PlanReviewV1, ProviderLane, StepPlanV1, StopPolicy,
    };

    fn plan() -> OrchestrationPlanV1 {
        OrchestrationPlanV1 {
            schema_version: 1,
            plan_id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa".into(),
            revision: 1,
            host_id: "local".into(),
            project_id: "project-1".into(),
            workspace_id: "a".repeat(64),
            workspace_root: "/tmp/wyss-fixture-workspace".into(),
            workspace_revision: "b".repeat(64),
            objective: "synthetic objective never persisted".into(),
            created_at_unix: 100,
            expires_at_unix: 400,
            limits: PlanLimitsV1 {
                max_steps: 2,
                max_parallelism: 1,
                max_total_seconds: 300,
                stop_policy: StopPolicy::CancelCurrentManagedTurn,
            },
            steps: vec![
                StepPlanV1 {
                    step_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
                    dependencies: vec![],
                    required: true,
                    provider: ProviderLane::ManagedCodex,
                    effect: EffectClass::ManagedInference,
                    managed_session_id: Some("11111111-1111-4111-8111-111111111111".into()),
                    model_id: Some("gpt-test".into()),
                    prompt: "SYNTHETIC_PRIVATE_PROMPT_NEVER_STORE".into(),
                    max_attempts: 1,
                    max_duration_seconds: 60,
                },
                StepPlanV1 {
                    step_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".into(),
                    dependencies: vec![DependencyV1 {
                        step_id: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb".into(),
                        allow_missing_output: false,
                    }],
                    required: true,
                    provider: ProviderLane::ManagedCodex,
                    effect: EffectClass::ManagedInference,
                    managed_session_id: Some("11111111-1111-4111-8111-111111111111".into()),
                    model_id: Some("gpt-test".into()),
                    prompt: "synthetic second".into(),
                    max_attempts: 1,
                    max_duration_seconds: 60,
                },
            ],
        }
    }

    fn review(plan: &OrchestrationPlanV1) -> PlanReviewV1 {
        PlanReviewV1 {
            plan_id: plan.plan_id.clone(),
            revision: plan.revision,
            digest: plan.digest().unwrap(),
        }
    }

    #[test]
    fn terminal_run_state_does_not_delete_an_f8_owned_claim() {
        let journal = RunJournal::open_in_memory().unwrap();
        let draft = plan();
        let run = journal.create_run(&draft, &review(&draft), 120).unwrap();
        let binding = WorkspaceClaimBinding {
            authority_store_id: Uuid::new_v4().to_string(),
            authority_epoch: Uuid::new_v4().to_string(),
            root_binding_id: Uuid::new_v4().to_string(),
            claim_id: Uuid::new_v4().to_string(),
            claim_generation: 1,
            claim_revision: 1,
            gate_revision: 1,
            root_observation_revision: 1,
            phase: "held".into(),
            gate_state: "open".into(),
            settlement_id: None,
            coordinator_revision: 0,
        };
        journal.bind_workspace_claim(&run.run_id, &binding).unwrap();
        let running = journal
            .set_run_state(&run.run_id, run.revision, RunState::Running, None, 121)
            .unwrap();
        let needs_review = journal
            .set_run_state(
                &run.run_id,
                running.revision,
                RunState::NeedsReview,
                None,
                122,
            )
            .unwrap();
        journal
            .set_run_state(
                &run.run_id,
                needs_review.revision,
                RunState::Failed,
                Some(super::super::FailureCode::OwnerReviewRequired),
                123,
            )
            .unwrap();
        assert_eq!(
            journal.workspace_claim_binding(&run.run_id).unwrap(),
            Some(binding)
        );
        assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
    }

    fn handoff() -> HandoffEnvelopeV1 {
        let mut envelope = HandoffEnvelopeV1 {
            schema_version: 1,
            handoff_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd".into(),
            revision: 1,
            source: HandoffSourceV1 {
                host_id: "local".into(),
                session_id: "0123456789abcdef0123456789abcdef".into(),
                session_generation: 1,
                plane: HandoffPlane::Native,
                project_id: "source-project".into(),
                workspace_id: "a".repeat(64),
                workspace_revision: "b".repeat(64),
                workspace_root: "/tmp/wyss-fixture-workspace".into(),
            },
            target: HandoffTargetV1 {
                host_id: "local".into(),
                project_id: "target-project".into(),
                workspace_id: "a".repeat(64),
                workspace_revision: "b".repeat(64),
                workspace_root: "/tmp/wyss-fixture-workspace".into(),
                provider: ProviderLane::ManagedCodex,
                effect: EffectClass::ManagedInference,
                managed_session_id: Some("11111111-1111-4111-8111-111111111111".into()),
                native_profile_id: None,
                profile_revision: None,
            },
            objective: "Owner-selected objective".into(),
            selected_context_items: vec![HandoffItemV1 {
                item_id: "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee".into(),
                source_revision: "b".repeat(64),
                kind: HandoffItemKind::Excerpt,
                provenance: HandoffProvenance::ProviderOutputUntrusted,
                text: "SENSITIVE_HANDOFF_BODY_NEVER_STORE".into(),
            }],
            selected_file_references: vec![],
            open_questions: vec![],
            created_at_unix: 100,
            expires_at_unix: 200,
            content_digest: String::new(),
        };
        envelope.seal().unwrap();
        envelope
    }

    #[test]
    fn interrupted_owner_confirmation_restores_reviewable_step_and_keeps_claim() {
        let journal = RunJournal::open_in_memory().unwrap();
        let draft = plan();
        let accepted = journal.create_run(&draft, &review(&draft), 120).unwrap();
        journal
            .set_run_state(
                &accepted.run_id,
                accepted.revision,
                RunState::NeedsReview,
                None,
                121,
            )
            .unwrap();
        journal
            .set_step_state(
                &accepted.run_id,
                &draft.steps[0].step_id,
                StepState::Eligible,
                StepState::NeedsReview,
                Some(FailureCode::CoordinatorRestarted),
                122,
            )
            .unwrap();
        // Crash after first commit in ConfirmedCompleted but before verify_step.
        journal
            .set_step_state(
                &accepted.run_id,
                &draft.steps[0].step_id,
                StepState::NeedsReview,
                StepState::Verifying,
                Some(FailureCode::OwnerReviewRequired),
                123,
            )
            .unwrap();
        let recovered = journal.recover_after_restart(124).unwrap();
        assert_eq!(recovered.len(), 1);
        let view = journal.get(&accepted.run_id).unwrap().unwrap();
        assert_eq!(view.state, RunState::NeedsReview);
        assert_eq!(view.steps[0].state, StepState::NeedsReview);
        assert_eq!(view.steps[1].state, StepState::Cancelled);
        assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
        // Repeated startup is idempotent: it cannot accidentally free the claim.
        assert!(journal.recover_after_restart(125).unwrap().is_empty());
        assert_eq!(journal.claimed_workspaces().unwrap().len(), 1);
    }

    #[test]
    fn confirmed_result_committed_before_crash_closes_only_after_safe_recovery() {
        let journal = RunJournal::open_in_memory().unwrap();
        let mut draft = plan();
        draft.steps.truncate(1);
        let accepted = journal.create_run(&draft, &review(&draft), 120).unwrap();
        journal
            .set_run_state(
                &accepted.run_id,
                accepted.revision,
                RunState::NeedsReview,
                None,
                121,
            )
            .unwrap();
        journal
            .set_step_state(
                &accepted.run_id,
                &draft.steps[0].step_id,
                StepState::Eligible,
                StepState::NeedsReview,
                Some(FailureCode::CoordinatorRestarted),
                122,
            )
            .unwrap();
        journal
            .set_step_state(
                &accepted.run_id,
                &draft.steps[0].step_id,
                StepState::NeedsReview,
                StepState::Verifying,
                Some(FailureCode::OwnerReviewRequired),
                123,
            )
            .unwrap();
        journal
            .verify_step(
                &accepted.run_id,
                &draft.steps[0].step_id,
                &draft.workspace_revision,
                124,
            )
            .unwrap();
        // Crash after the durable verified workspace revision but before run closure.
        let recovered = journal.recover_after_restart(125).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].state, RunState::Completed);
        assert_eq!(recovered[0].steps[0].state, StepState::Succeeded);
        assert!(journal.claimed_workspaces().unwrap().is_empty());
        assert!(journal.recover_after_restart(126).unwrap().is_empty());
    }

    #[test]
    fn journal_persists_only_closed_metadata_and_never_prompt_or_objective() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let run = journal.create_run(&plan, &review(&plan), 120).unwrap();
        let encoded = {
            let connection = journal.connection.lock().unwrap();
            let mut statement = connection
                .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL")
                .unwrap();
            statement.query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap().join("\n")
                + &connection.query_row("SELECT group_concat(plan_digest || workspace_id || workspace_revision) FROM orchestration_runs",[],|r|r.get::<_,String>(0)).unwrap()
                + &connection.query_row("SELECT group_concat(input_digest || COALESCE(model_id,'')) FROM orchestration_steps",[],|r|r.get::<_,String>(0)).unwrap()
        };
        assert!(!encoded.contains("SYNTHETIC_PRIVATE_PROMPT_NEVER_STORE"));
        assert!(!encoded.contains("synthetic objective never persisted"));
        assert_eq!(run.steps.len(), 2);
        assert_eq!(run.steps[0].state, StepState::Eligible);
        assert_eq!(run.steps[1].state, StepState::WaitingDependencies);
    }

    #[test]
    fn run_plan_and_dag_are_immutable_and_reviews_are_digest_bound() {
        let journal = RunJournal::open_in_memory().unwrap();
        let mut plan = plan();
        let review = review(&plan);
        plan.steps[0].prompt.push_str(" mutation");
        assert_eq!(
            journal.create_run(&plan, &review, 120).unwrap_err(),
            JournalError::Stale
        );
    }

    #[test]
    fn duplicate_plan_submit_returns_same_run_without_resetting_or_releasing_claim() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let review = review(&plan);
        let first = journal.create_run(&plan, &review, 120).unwrap();
        let running = journal
            .set_run_state(&first.run_id, first.revision, RunState::Running, None, 121)
            .unwrap();
        let repeated = journal.create_run(&plan, &review, 122).unwrap();
        assert_eq!(repeated.run_id, running.run_id);
        assert_eq!(repeated.state, RunState::Running);
        assert_eq!(repeated.revision, running.revision);
        let count: i64 = journal
            .connection
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM orchestration_runs", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn dispatch_marker_precedes_ack_and_ambiguous_attempt_is_never_retried() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let run = journal.create_run(&plan, &review(&plan), 120).unwrap();
        let step = &run.steps[0];
        journal
            .begin_attempt(
                &run.run_id,
                &step.step_id,
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "ffffffff-ffff-4fff-8fff-ffffffffffff",
                "99999999-9999-4999-8999-999999999999",
                1,
                130,
            )
            .unwrap();
        let unknown = journal
            .mark_attempt_unknown(
                &run.run_id,
                &step.step_id,
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                140,
            )
            .unwrap();
        assert_eq!(unknown.state, StepState::NeedsReview);
        assert_eq!(unknown.attempt_count, 1);
        assert_eq!(
            journal
                .begin_attempt(
                    &run.run_id,
                    &step.step_id,
                    "12121212-1212-4212-8212-121212121212",
                    "13131313-1313-4313-8313-131313131313",
                    "ffffffff-ffff-4fff-8fff-ffffffffffff",
                    "99999999-9999-4999-8999-999999999999",
                    1,
                    150,
                )
                .unwrap_err(),
            JournalError::InvalidTransition
        );
    }

    #[test]
    fn crash_reconciliation_cancels_only_never_started_work() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let created = journal.create_run(&plan, &review(&plan), 120).unwrap();
        let running = journal
            .set_run_state(&created.run_id, 1, RunState::Running, None, 121)
            .unwrap();
        journal
            .begin_attempt(
                &running.run_id,
                &running.steps[0].step_id,
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "ffffffff-ffff-4fff-8fff-ffffffffffff",
                "99999999-9999-4999-8999-999999999999",
                1,
                122,
            )
            .unwrap();
        let recovered = journal.recover_after_restart(130).unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].run_id, running.run_id);
        assert_eq!(recovered[0].state, RunState::NeedsReview);
        assert_eq!(recovered[0].steps[0].state, StepState::NeedsReview);
        assert_eq!(recovered[0].steps[1].state, StepState::Cancelled);
    }

    #[test]
    fn cancellation_intent_is_durable_idempotent_and_unknown_after_crash() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let created = journal.create_run(&plan, &review(&plan), 120).unwrap();
        journal
            .set_run_state(
                &created.run_id,
                created.revision,
                RunState::Running,
                None,
                121,
            )
            .unwrap();
        let step = &created.steps[0];
        let attempt_id = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
        journal
            .begin_attempt(
                &created.run_id,
                &step.step_id,
                attempt_id,
                "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "ffffffff-ffff-4fff-8fff-ffffffffffff",
                "99999999-9999-4999-8999-999999999999",
                1,
                122,
            )
            .unwrap();
        let cancel_id = "12121212-1212-4212-8212-121212121212";
        assert_eq!(
            journal
                .begin_cancellation(&created.run_id, &step.step_id, attempt_id, cancel_id, 123)
                .unwrap(),
            CancellationOutcome::Requested
        );
        assert_eq!(
            journal
                .begin_cancellation(
                    &created.run_id,
                    &step.step_id,
                    attempt_id,
                    "13131313-1313-4313-8313-131313131313",
                    124
                )
                .unwrap(),
            CancellationOutcome::Unknown
        );
        let recovered = journal.recover_after_restart(130).unwrap();
        assert_eq!(recovered[0].state, RunState::NeedsReview);
        assert_eq!(
            journal
                .begin_cancellation(&created.run_id, &step.step_id, attempt_id, cancel_id, 131)
                .unwrap(),
            CancellationOutcome::Unknown
        );
        let (state, intent): (String, String) = journal.connection.lock().unwrap().query_row(
            "SELECT state,intent_id FROM orchestration_cancellations WHERE run_id=?1 AND step_id=?2 AND attempt_id=?3",
            params![created.run_id, step.step_id, attempt_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(state, "unknown");
        assert_eq!(intent, cancel_id);
    }

    #[test]
    fn handoff_storage_is_digest_only_and_duplicate_confirmation_is_idempotent() {
        let journal = RunJournal::open_in_memory().unwrap();
        let plan = plan();
        let run = journal.create_run(&plan, &review(&plan), 120).unwrap();
        let envelope = handoff();
        let confirmation = HandoffConfirmationV1 {
            handoff_id: envelope.handoff_id.clone(),
            revision: envelope.revision,
            reviewed_digest: envelope.content_digest.clone(),
            confirmed_at_unix: 120,
        };
        journal
            .record_handoff(&run.run_id, &envelope, &confirmation, 120)
            .unwrap();
        let repeated = journal
            .handoff_run(
                &envelope.handoff_id,
                envelope.revision,
                &envelope.content_digest,
                confirmation.confirmed_at_unix,
            )
            .unwrap()
            .unwrap();
        assert_eq!(repeated.run_id, run.run_id);
        assert_eq!(
            journal
                .handoff_run(
                    &envelope.handoff_id,
                    envelope.revision + 1,
                    &envelope.content_digest,
                    confirmation.confirmed_at_unix,
                )
                .unwrap_err(),
            JournalError::Stale
        );
        let contents: String = journal.connection.lock().unwrap().query_row(
            "SELECT group_concat(handoff_id || content_digest || source_session_id || target_project_id) FROM orchestration_handoffs",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(!contents.contains("SENSITIVE_HANDOFF_BODY_NEVER_STORE"));
        assert!(!contents.contains("Owner-selected objective"));
    }

    #[test]
    fn schema_one_claims_migrate_as_identity_unproven_without_backfill() {
        for populated in [false, true] {
            let journal = RunJournal::open_in_memory().unwrap();
            if populated {
                let plan = plan();
                journal.create_run(&plan, &review(&plan), 120).unwrap();
            }
            let connection = journal.connection.into_inner().unwrap();
            connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     DROP TRIGGER orchestration_metadata_identity_no_update;
                     DROP TRIGGER orchestration_metadata_no_delete;
                     DROP TRIGGER orchestration_import_snapshot_no_update;
                     DROP TRIGGER orchestration_import_snapshot_no_delete;
                     DROP TABLE orchestration_import_snapshot;
                     DROP TABLE orchestration_metadata;
                     CREATE TABLE orchestration_workspace_claims_v1 (
                       workspace_id TEXT PRIMARY KEY CHECK(length(workspace_id)=64),
                       run_id TEXT NOT NULL UNIQUE REFERENCES orchestration_runs(run_id),
                       claimed_at INTEGER NOT NULL CHECK(claimed_at>=0)
                     ) STRICT;
                     INSERT INTO orchestration_workspace_claims_v1(workspace_id,run_id,claimed_at)
                       SELECT workspace_id,run_id,claimed_at FROM orchestration_workspace_claims;
                     DROP TABLE orchestration_workspace_claims;
                     ALTER TABLE orchestration_workspace_claims_v1 RENAME TO orchestration_workspace_claims;
                     CREATE INDEX orchestration_claims_run ON orchestration_workspace_claims(run_id);
                     PRAGMA user_version=1;
                     COMMIT;",
                )
                .unwrap();

            let migrated = RunJournal::from_connection(connection).unwrap();
            let connection = migrated.connection.lock().unwrap();
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            let claims = connection
                .prepare(
                    "SELECT proof_state,authority_store_id,authority_epoch,root_binding_id FROM orchestration_workspace_claims",
                )
                .unwrap()
                .query_map([], |row| {
                    let proof: String = row.get(0)?;
                    let store: Option<String> = row.get(1)?;
                    let epoch: Option<String> = row.get(2)?;
                    let root: Option<String> = row.get(3)?;
                    Ok((proof, store, epoch, root))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(version, 5);
            assert_eq!(claims.len(), usize::from(populated));
            assert!(claims.iter().all(|(proof, store, epoch, root)| {
                proof == "identity_unproven" && store.is_none() && epoch.is_none() && root.is_none()
            }));
            let imported: i64 = connection
                .query_row(
                    "SELECT count(*) FROM orchestration_import_snapshot",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(imported, i64::from(populated));
        }
    }

    #[test]
    fn schema_four_claim_history_migrates_and_released_workspace_can_be_reclaimed() {
        let journal = RunJournal::open_in_memory().unwrap();
        let first_plan = plan();
        let first = journal
            .create_run(&first_plan, &review(&first_plan), 120)
            .unwrap();
        let connection = journal.connection.into_inner().unwrap();
        connection
            .execute_batch(
                "BEGIN IMMEDIATE;
                 DROP INDEX orchestration_active_workspace_claim;
                 ALTER TABLE orchestration_workspace_claims RENAME TO orchestration_workspace_claims_v5;
                 CREATE TABLE orchestration_workspace_claims (
                   workspace_id TEXT PRIMARY KEY CHECK(length(workspace_id)=64),
                   run_id TEXT NOT NULL UNIQUE REFERENCES orchestration_runs(run_id),
                   claimed_at INTEGER NOT NULL CHECK(claimed_at>=0),
                   authority_store_id TEXT,
                   authority_epoch TEXT,
                   root_binding_id TEXT,
                   claim_id TEXT,
                   claim_generation INTEGER CHECK(claim_generation IS NULL OR claim_generation>0),
                   claim_revision INTEGER CHECK(claim_revision IS NULL OR claim_revision>0),
                   gate_revision INTEGER CHECK(gate_revision IS NULL OR gate_revision>0),
                   root_observation_revision INTEGER CHECK(root_observation_revision IS NULL OR root_observation_revision>0),
                   claim_phase TEXT CHECK(claim_phase IS NULL OR claim_phase IN ('held','unsettled','verified','released','quarantined')),
                   gate_state TEXT CHECK(gate_state IS NULL OR gate_state IN ('open','paused','stopped','quarantined')),
                   settlement_id TEXT CHECK(settlement_id IS NULL OR length(settlement_id)=36),
                   coordinator_revision INTEGER NOT NULL DEFAULT 0 CHECK(coordinator_revision>=0),
                   settlement_intent_id TEXT CHECK(settlement_intent_id IS NULL OR length(settlement_intent_id)=36),
                   settlement_attempt_id TEXT CHECK(settlement_attempt_id IS NULL OR length(settlement_attempt_id)=36),
                   settlement_decision TEXT CHECK(settlement_decision IS NULL OR settlement_decision IN ('confirmed_completed','confirmed_no_effect','confirmed_cancelled')),
                   settlement_digest TEXT CHECK(settlement_digest IS NULL OR length(settlement_digest)=64),
                   proof_state TEXT NOT NULL DEFAULT 'identity_unproven'
                     CHECK(proof_state IN ('pinned','identity_unproven','quarantined'))
                 ) STRICT;
                 INSERT INTO orchestration_workspace_claims SELECT * FROM orchestration_workspace_claims_v5;
                 DROP TABLE orchestration_workspace_claims_v5;
                 CREATE INDEX orchestration_claims_run ON orchestration_workspace_claims(run_id);
                 PRAGMA user_version=4;
                 COMMIT;",
            )
            .unwrap();

        let migrated = RunJournal::from_connection(connection).unwrap();
        let version: i64 = migrated
            .connection
            .lock()
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 5);
        assert_eq!(
            migrated.workspace_claim_binding(&first.run_id).unwrap(),
            None
        );
        assert_eq!(migrated.claimed_workspaces().unwrap().len(), 1);

        let mut conflicting_plan = plan();
        conflicting_plan.plan_id = Uuid::new_v4().to_string();
        assert_eq!(
            migrated
                .create_run(&conflicting_plan, &review(&conflicting_plan), 121)
                .unwrap_err(),
            JournalError::WorkspaceConflict
        );
        migrated
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE orchestration_workspace_claims SET claim_phase='released' WHERE run_id=?1",
                [&first.run_id],
            )
            .unwrap();
        let mut next_plan = plan();
        next_plan.plan_id = Uuid::new_v4().to_string();
        assert!(
            migrated
                .create_run(&next_plan, &review(&next_plan), 122)
                .is_ok()
        );
        assert_eq!(migrated.claimed_workspaces().unwrap().len(), 1);
    }
}
