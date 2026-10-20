//! Test-only canonical V7 image for the F8 post-migration validator: one prepared session with a
//! binding and a completed, audited turn (legacy history), its lease revoked so the image is
//! quiescent and migratable. Built only through the authority's own store paths.
use super::{
    runtime::MaterializationStage,
    store::{Store, turns::StartFingerprint, uuid},
};
use crate::{
    adapters::codex::CodexThreadId,
    contracts::{control::*, identities::AgentRuntimeInstanceId},
};
use rusqlite::Connection;
use std::os::unix::fs::DirBuilderExt;

pub(crate) fn v7_with_completed_turn() -> Connection {
    v7_with_sessions(0)
}

/// The same image plus `extra` independent prepared sessions (no binding, no work).
pub(crate) fn v7_with_sessions(extra: usize) -> Connection {
    let root = std::env::temp_dir().join(format!("wyss-f8-fixture-{}", uuid().as_str()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap();
    let mut store = Store::in_memory_fixture();
    let ResultValue::Session(session) = store
        .apply(&Operation::Prepare(Prepare {
            intent_id: uuid(),
            adapter_id: RuntimeTarget::codex().adapter_id,
            runtime_kind: RuntimeTarget::codex().runtime_kind,
        }))
        .unwrap()
    else {
        panic!("session fixture")
    };
    let ResultValue::Lease(lease) = store
        .apply(&Operation::LeaseGrant(LeaseGrant {
            intent_id: uuid(),
            session_id: session.managed_session_id.clone(),
            expected_session_revision: 1,
            scopes: vec![
                ExecutionScope::SessionCreate,
                ExecutionScope::SessionResume,
                ExecutionScope::TurnStart,
            ],
            ttl_seconds: 3600,
        }))
        .unwrap()
    else {
        panic!("lease fixture")
    };
    let runtime = AgentRuntimeInstanceId::parse(uuid().as_str()).unwrap();
    let op = Operation::BindingCreate(BindingCreate {
        intent_id: uuid(),
        managed_session_id: session.managed_session_id.clone(),
        expected_session_revision: 1,
        workspace_root: root.to_str().unwrap().into(),
    });
    let r = store.reserve_binding(&op).unwrap();
    store.dispatch_binding(&r).unwrap();
    let thread = CodexThreadId::parse("opaque-durable-thread").unwrap();
    for stage in [
        MaterializationStage::Started,
        MaterializationStage::Archived,
        MaterializationStage::Unarchived,
    ] {
        store.checkpoint_binding(&r, &thread, stage).unwrap();
    }
    let binding = store.commit_binding(&r, &thread, &runtime).unwrap();
    let prompt = TurnStart {
        intent_id: uuid(),
        managed_session_id: session.managed_session_id.clone(),
        expected_session_revision: 1,
        prompt: "legacy history".into(),
    };
    let turn = store
        .reserve_turn(
            &StartFingerprint::compute(&prompt).unwrap(),
            &binding,
            &lease,
            &runtime,
        )
        .unwrap();
    let provider = crate::adapters::codex::CodexTurnId::parse("provider-turn").unwrap();
    for state in [
        crate::contracts::turn::TurnState::Running,
        crate::contracts::turn::TurnState::Completed,
    ] {
        store
            .transition_turn(
                &turn.managed_turn_id,
                (state == crate::contracts::turn::TurnState::Running).then_some(&provider),
                state,
                state
                    .terminal()
                    .then_some(crate::contracts::turn::TurnReason::Completed),
            )
            .unwrap();
    }
    store
        .apply(&Operation::LeaseRevoke(LeaseRevoke {
            intent_id: uuid(),
            lease_id: lease.lease_id,
            expected_revision: lease.revision,
        }))
        .unwrap();
    for _ in 0..extra {
        store
            .apply(&Operation::Prepare(Prepare {
                intent_id: uuid(),
                adapter_id: RuntimeTarget::codex().adapter_id,
                runtime_kind: RuntimeTarget::codex().runtime_kind,
            }))
            .unwrap();
    }
    let _ = std::fs::remove_dir_all(&root);
    store.conn
}
