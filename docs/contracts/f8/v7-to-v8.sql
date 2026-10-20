-- CANDIDATE CONTRACT ONLY. Not loaded by production or executable against live state here.
-- Caller: verify exact V7 objects/canonical records, BEGIN IMMEDIATE, execute statements,
-- validate complete V8 schema/FKs/records and preservation, COMMIT. Any error => ROLLBACK.
-- Requires an owner-approved quiescent migration; never normalizes live rows silently.
CREATE TEMP TABLE f8_quiescent (ok INTEGER NOT NULL CHECK(ok=1)) STRICT;
INSERT INTO f8_quiescent VALUES(CASE WHEN
    (SELECT user_version FROM pragma_user_version)=7
    AND NOT EXISTS(SELECT 1 FROM managed_turns WHERE state IN ('starting','running'))
    AND NOT EXISTS(SELECT 1 FROM managed_approvals WHERE state='pending')
    AND NOT EXISTS(SELECT 1 FROM execution_leases WHERE state='active')
    AND NOT EXISTS(SELECT 1 FROM managed_session_operations WHERE state IN ('reserved','dispatching'))
    THEN 1 ELSE 0 END);
DROP TABLE f8_quiescent;

ALTER TABLE managed_runtimes RENAME TO managed_runtimes_legacy_v7;
CREATE TRIGGER managed_runtimes_legacy_v7_no_insert BEFORE INSERT ON managed_runtimes_legacy_v7 BEGIN SELECT RAISE(ABORT,'legacy_runtime_immutable'); END;
CREATE TRIGGER managed_runtimes_legacy_v7_no_update BEFORE UPDATE ON managed_runtimes_legacy_v7 BEGIN SELECT RAISE(ABORT,'legacy_runtime_immutable'); END;
CREATE TRIGGER managed_runtimes_legacy_v7_no_delete BEFORE DELETE ON managed_runtimes_legacy_v7 BEGIN SELECT RAISE(ABORT,'legacy_runtime_immutable'); END;

CREATE UNIQUE INDEX managed_sessions_runtime_target ON managed_sessions(session_id,adapter_id,runtime_kind);
CREATE TABLE managed_runtimes (managed_session_id TEXT NOT NULL, adapter_id TEXT NOT NULL CHECK(adapter_id='codex'), runtime_kind TEXT NOT NULL CHECK(runtime_kind='codex'), desired_state TEXT NOT NULL CHECK(desired_state IN ('running','stopped')), revision INTEGER NOT NULL CHECK(revision>0), generation INTEGER NOT NULL CHECK(generation>=0), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(managed_session_id,adapter_id,runtime_kind), FOREIGN KEY(managed_session_id,adapter_id,runtime_kind) REFERENCES managed_sessions(session_id,adapter_id,runtime_kind)) STRICT;
CREATE TRIGGER managed_runtimes_transition BEFORE UPDATE ON managed_runtimes WHEN NOT (NEW.managed_session_id=OLD.managed_session_id AND NEW.adapter_id=OLD.adapter_id AND NEW.runtime_kind=OLD.runtime_kind AND NEW.created_at=OLD.created_at AND NEW.revision=OLD.revision+1 AND (NEW.generation=OLD.generation OR (OLD.generation<9223372036854775807 AND NEW.generation=OLD.generation+1))) BEGIN SELECT RAISE(ABORT,'runtime_transition'); END;
CREATE TRIGGER managed_runtimes_no_delete BEFORE DELETE ON managed_runtimes BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_runtimes_capacity BEFORE INSERT ON managed_runtimes WHEN (SELECT count(*) FROM managed_runtimes)>=128 BEGIN SELECT RAISE(ABORT,'capacity_exceeded'); END;

-- Allocation history, not durable process liveness. Reserved before spawn; failed attempts remain.
CREATE TABLE managed_runtime_launches (managed_session_id TEXT NOT NULL, adapter_id TEXT NOT NULL CHECK(adapter_id='codex'), runtime_kind TEXT NOT NULL CHECK(runtime_kind='codex'), generation INTEGER NOT NULL CHECK(generation>0), runtime_instance_id TEXT NOT NULL UNIQUE, authority_instance_id TEXT NOT NULL, created_at TEXT NOT NULL, PRIMARY KEY(managed_session_id,runtime_instance_id,generation), UNIQUE(managed_session_id,adapter_id,runtime_kind,generation), FOREIGN KEY(managed_session_id,adapter_id,runtime_kind) REFERENCES managed_runtimes(managed_session_id,adapter_id,runtime_kind), CHECK(runtime_instance_id<>managed_session_id AND runtime_instance_id<>authority_instance_id AND managed_session_id<>authority_instance_id)) STRICT;
CREATE TRIGGER managed_runtime_launches_current BEFORE INSERT ON managed_runtime_launches WHEN NOT EXISTS(SELECT 1 FROM managed_runtimes r JOIN managed_sessions s ON s.session_id=r.managed_session_id WHERE r.managed_session_id=NEW.managed_session_id AND r.adapter_id=NEW.adapter_id AND r.runtime_kind=NEW.runtime_kind AND r.generation=NEW.generation AND r.desired_state='running' AND s.control_state='prepared') BEGIN SELECT RAISE(ABORT,'launch_not_reserved'); END;
CREATE TRIGGER managed_runtime_launches_no_update BEFORE UPDATE ON managed_runtime_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_runtime_launches_no_delete BEFORE DELETE ON managed_runtime_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_runtime_launches_capacity BEFORE INSERT ON managed_runtime_launches WHEN (SELECT count(*) FROM managed_runtime_launches)>=4096 BEGIN SELECT RAISE(ABORT,'capacity_exceeded'); END;

-- Sidecars preserve every historical V7 row and receipt byte. Missing sidecar = legacy history,
-- never permission to attach/admit/dispatch. New records require a sidecar in the same transaction.
CREATE TABLE managed_turn_launches (managed_turn_id TEXT PRIMARY KEY NOT NULL REFERENCES managed_turns(managed_turn_id) DEFERRABLE INITIALLY DEFERRED, managed_session_id TEXT NOT NULL, runtime_instance_id TEXT NOT NULL, generation INTEGER NOT NULL, FOREIGN KEY(managed_session_id,runtime_instance_id,generation) REFERENCES managed_runtime_launches(managed_session_id,runtime_instance_id,generation), UNIQUE(managed_turn_id,managed_session_id,runtime_instance_id,generation)) STRICT;
CREATE TRIGGER managed_turn_launches_current BEFORE INSERT ON managed_turn_launches WHEN NOT EXISTS(SELECT 1 FROM managed_runtime_launches l JOIN managed_runtimes r ON r.managed_session_id=l.managed_session_id AND r.adapter_id=l.adapter_id AND r.runtime_kind=l.runtime_kind JOIN managed_sessions s ON s.session_id=r.managed_session_id WHERE l.managed_session_id=NEW.managed_session_id AND l.runtime_instance_id=NEW.runtime_instance_id AND l.generation=NEW.generation AND r.generation=l.generation AND r.desired_state='running' AND s.control_state='prepared') BEGIN SELECT RAISE(ABORT,'stale_launch'); END;
CREATE TRIGGER managed_turn_launches_no_update BEFORE UPDATE ON managed_turn_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_turn_launches_no_delete BEFORE DELETE ON managed_turn_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_turn_launches_no_legacy_attach BEFORE INSERT ON managed_turn_launches WHEN EXISTS(SELECT 1 FROM managed_turns WHERE managed_turn_id=NEW.managed_turn_id) BEGIN SELECT RAISE(ABORT,'legacy_launch_unassigned'); END;
CREATE TRIGGER managed_turns_require_launch BEFORE INSERT ON managed_turns WHEN NOT EXISTS(SELECT 1 FROM managed_turn_launches l WHERE l.managed_turn_id=NEW.managed_turn_id AND l.managed_session_id=NEW.managed_session_id AND l.runtime_instance_id=NEW.runtime_instance_id) BEGIN SELECT RAISE(ABORT,'turn_launch_required'); END;

CREATE TABLE managed_approval_launches (managed_approval_id TEXT PRIMARY KEY NOT NULL REFERENCES managed_approvals(managed_approval_id) DEFERRABLE INITIALLY DEFERRED, managed_turn_id TEXT NOT NULL, managed_session_id TEXT NOT NULL, runtime_instance_id TEXT NOT NULL, generation INTEGER NOT NULL, FOREIGN KEY(managed_turn_id,managed_session_id,runtime_instance_id,generation) REFERENCES managed_turn_launches(managed_turn_id,managed_session_id,runtime_instance_id,generation)) STRICT;
CREATE TRIGGER managed_approval_launches_current BEFORE INSERT ON managed_approval_launches WHEN NOT EXISTS(SELECT 1 FROM managed_runtime_launches l JOIN managed_runtimes r ON r.managed_session_id=l.managed_session_id AND r.adapter_id=l.adapter_id AND r.runtime_kind=l.runtime_kind JOIN managed_sessions s ON s.session_id=r.managed_session_id WHERE l.managed_session_id=NEW.managed_session_id AND l.runtime_instance_id=NEW.runtime_instance_id AND l.generation=NEW.generation AND r.generation=l.generation AND r.desired_state='running' AND s.control_state='prepared') BEGIN SELECT RAISE(ABORT,'stale_launch'); END;
CREATE TRIGGER managed_approval_launches_no_update BEFORE UPDATE ON managed_approval_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_approval_launches_no_delete BEFORE DELETE ON managed_approval_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_approval_launches_no_legacy_attach BEFORE INSERT ON managed_approval_launches WHEN EXISTS(SELECT 1 FROM managed_approvals WHERE managed_approval_id=NEW.managed_approval_id) BEGIN SELECT RAISE(ABORT,'legacy_launch_unassigned'); END;
CREATE TRIGGER managed_approvals_require_launch BEFORE INSERT ON managed_approvals WHEN NOT EXISTS(SELECT 1 FROM managed_approval_launches l WHERE l.managed_approval_id=NEW.managed_approval_id AND l.managed_turn_id=NEW.managed_turn_id AND l.managed_session_id=NEW.managed_session_id AND l.runtime_instance_id=NEW.runtime_instance_id) BEGIN SELECT RAISE(ABORT,'approval_launch_required'); END;

CREATE TABLE managed_operation_launches (operation_id TEXT PRIMARY KEY NOT NULL REFERENCES managed_session_operations(operation_id) DEFERRABLE INITIALLY DEFERRED, managed_session_id TEXT NOT NULL, runtime_instance_id TEXT NOT NULL, generation INTEGER NOT NULL, FOREIGN KEY(managed_session_id,runtime_instance_id,generation) REFERENCES managed_runtime_launches(managed_session_id,runtime_instance_id,generation)) STRICT;
CREATE TRIGGER managed_operation_launches_current BEFORE INSERT ON managed_operation_launches WHEN NOT EXISTS(SELECT 1 FROM managed_runtime_launches l JOIN managed_runtimes r ON r.managed_session_id=l.managed_session_id AND r.adapter_id=l.adapter_id AND r.runtime_kind=l.runtime_kind JOIN managed_sessions s ON s.session_id=r.managed_session_id WHERE l.managed_session_id=NEW.managed_session_id AND l.runtime_instance_id=NEW.runtime_instance_id AND l.generation=NEW.generation AND r.generation=l.generation AND r.desired_state='running' AND s.control_state='prepared') BEGIN SELECT RAISE(ABORT,'stale_launch'); END;
CREATE TRIGGER managed_operation_launches_no_update BEFORE UPDATE ON managed_operation_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_operation_launches_no_delete BEFORE DELETE ON managed_operation_launches BEGIN SELECT RAISE(ABORT,'append_only'); END;
CREATE TRIGGER managed_operation_launches_no_legacy_attach BEFORE INSERT ON managed_operation_launches WHEN EXISTS(SELECT 1 FROM managed_session_operations WHERE operation_id=NEW.operation_id) BEGIN SELECT RAISE(ABORT,'legacy_launch_unassigned'); END;
CREATE TRIGGER managed_session_operations_require_launch BEFORE INSERT ON managed_session_operations WHEN NOT EXISTS(SELECT 1 FROM managed_operation_launches l WHERE l.operation_id=NEW.operation_id AND l.managed_session_id=NEW.managed_session_id) BEGIN SELECT RAISE(ABORT,'operation_launch_required'); END;

DROP INDEX managed_turns_one_active;
CREATE UNIQUE INDEX managed_turns_one_active ON managed_turns(managed_session_id) WHERE state IN ('starting','running');
DROP INDEX managed_approvals_one_pending;
CREATE UNIQUE INDEX managed_approvals_one_pending ON managed_approvals(managed_session_id) WHERE state='pending';

ALTER TABLE authority_metadata RENAME TO authority_metadata_v7;
CREATE TABLE authority_metadata (id INTEGER PRIMARY KEY CHECK(id=1), schema_version INTEGER NOT NULL CHECK(schema_version=8), stream_id TEXT NOT NULL, last_sequence TEXT NOT NULL, event_vocabulary INTEGER NOT NULL CHECK(event_vocabulary>0)) STRICT;
INSERT INTO authority_metadata SELECT id,8,stream_id,last_sequence,max(event_vocabulary,2) FROM authority_metadata_v7;
DROP TABLE authority_metadata_v7;
PRAGMA user_version=8;
