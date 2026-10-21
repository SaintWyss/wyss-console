//! Transient, connection-scoped native mediation. Durable authorization stays in Authority.
use super::*;
use crate::adapters::codex::safety::{ActionKind, ItemEnd, NoticeKind, TurnNotice};
use crate::f8::operations::ProviderAction;

impl<W: Worker> NativeCodexWorker<W> {
    pub(super) fn host_proven(&mut self) -> ControlResult<()> {
        if let Some(local) = &self.local
            && (!local.holds() || self.worker.containment() != Containment::PidNamespace)
        {
            return Err(E::GateDenied);
        }
        Ok(())
    }
    fn action(&self, value: Value, item: &str) -> ControlResult<ProviderAction> {
        let action: ProviderAction =
            serde_json::from_value(value).map_err(|_| E::InvalidRequest)?;
        let held = action.clone().build(item)?;
        if !held.confined(&self.attachment.workspace) {
            return Err(E::GateDenied);
        }
        Ok(action)
    }
    pub(super) fn mediate(
        &mut self,
        message: &WireMessage,
        notice: Option<TurnNotice>,
    ) -> ControlResult<Vec<Value>> {
        let binding = self.binding.as_ref().ok_or(E::BindingNotFound)?;
        let (_, provider) = self.turn.as_ref().ok_or(E::CorrelationError)?;
        let provider = provider.as_ref().ok_or(E::CorrelationError)?;
        if let Some(notice) = &notice
            && (notice.thread != binding.thread_id || &notice.turn != provider)
        {
            return Err(E::CorrelationError);
        }
        if message.method() == Some("serverRequest/resolved") {
            let p = message.params().ok_or(E::InvalidRequest)?;
            if p["threadId"].as_str() != Some(binding.thread_id.as_str()) {
                return Err(E::CorrelationError);
            }
            let request = codec(CodexRequestId::from_value(&p["requestId"]))?;
            let live = self
                .approvals
                .values_mut()
                .find(|a| a.message.id() == Some(&request))
                .ok_or(E::CorrelationError)?;
            // Resolution can also be a withdrawal. It is never proof that a
            // particular decision was consumed, nor that an action completed.
            if live.decision.is_none() || live.consumed {
                return Err(E::GateDenied);
            }
            live.consumed = true;
            return Ok(Vec::new());
        }
        match notice.ok_or(E::Unsupported)?.kind {
            NoticeKind::CommandProposed(p) => {
                let item = p.item.as_str();
                if self.seen_items.contains(item) || self.proposals.len() >= 4 {
                    return Err(E::CapacityExceeded);
                }
                let value = json!({"kind":"command","command":p.command,"cwd":p.cwd,"parsed_actions":p.actions});
                self.action(value.clone(), item)?;
                self.seen_items.insert(item.into());
                self.proposals.insert(item.into(), value);
                Ok(Vec::new())
            }
            NoticeKind::FileProposed(p) => {
                let item = p.item.as_str();
                if self.approvals.contains_key(item)
                    || (self.seen_items.contains(item) && !self.proposals.contains_key(item))
                    || self.proposals.len() >= 4
                {
                    return Err(E::GateDenied);
                }
                let changes: Vec<_> = p.changes.into_iter().map(|c| json!({"path":c.path,"kind":c.kind,"move_path":c.move_path,"diff":c.diff})).collect();
                let value = json!({"kind":"files","changes":changes});
                self.action(value.clone(), item)?;
                self.seen_items.insert(item.into());
                self.proposals.insert(item.into(), value);
                Ok(Vec::new())
            }
            NoticeKind::ApprovalRequested(r) => {
                self.host_proven()?;
                let key = serde_json::to_string(&r.request).map_err(|_| E::InvalidRequest)?;
                if self.seen_requests.len() >= 4096
                    || !self.seen_requests.insert(key)
                    || self.approvals.len() >= 128
                    || self.approvals.values().any(|a| a.decision.is_none())
                {
                    return Err(E::IntentConflict);
                }
                let item = r.item.as_str();
                let value = self.proposals.remove(item).ok_or(E::GateDenied)?;
                if (r.kind == ActionKind::Command
                    && (value["kind"] != "command"
                        || r.command.as_ref().is_some_and(|c| value["command"] != *c)
                        || r.cwd.as_ref().is_some_and(|c| value["cwd"] != *c)
                        || (!r.actions.is_empty() && value["parsed_actions"] != json!(r.actions))))
                    || (r.kind == ActionKind::File && value["kind"] != "files")
                {
                    return Err(E::CorrelationError);
                }
                let action = self.action(value, item)?;
                let (kind, fingerprint, structure) = action.metadata(item)?;
                let event = json!({"kind":"approval_required","approval_kind":kind,
                    "provider_item_id":item,"action_fingerprint":fingerprint,"structure":structure,"action":action});
                self.approvals.insert(
                    item.into(),
                    NativeApproval {
                        message: message.clone(),
                        action,
                        fingerprint,
                        offered: r.offered,
                        decision: None,
                        consumed: false,
                        ended: false,
                    },
                );
                Ok(vec![event])
            }
            NoticeKind::Output { item, kind, .. } => {
                let a = self.approvals.get(item.as_str()).ok_or(E::GateDenied)?;
                let expected = match a.action {
                    ProviderAction::Command { .. } => ActionKind::Command,
                    _ => ActionKind::File,
                };
                if a.decision.as_deref() != Some("accept") || a.ended || kind != expected {
                    return Err(E::GateDenied);
                }
                // Tool output is deliberately not promoted to assistant text.
                Ok(Vec::new())
            }
            NoticeKind::DiffUpdated => {
                if !self.approvals.values().any(|a| {
                    a.decision.as_deref() == Some("accept")
                        && matches!(a.action, ProviderAction::Files { .. })
                }) {
                    return Err(E::GateDenied);
                }
                Ok(Vec::new())
            }
            NoticeKind::ItemEnded {
                item, kind, end, ..
            } => {
                let reported = self.reported_fingerprint(
                    message.params().ok_or(E::InvalidRequest)?.clone(),
                    item.as_str(),
                )?;
                let a = self.approvals.get_mut(item.as_str()).ok_or(E::GateDenied)?;
                let expected = match a.action {
                    ProviderAction::Command { .. } => ActionKind::Command,
                    _ => ActionKind::File,
                };
                if a.fingerprint != reported
                    || a.ended
                    || kind != expected
                    || a.decision.is_none()
                    || (a.decision.as_deref() != Some("accept") && end != ItemEnd::Declined)
                {
                    return Err(E::GateDenied);
                }
                a.ended = true;
                if a.decision.as_deref() == Some("accept") && end == ItemEnd::Completed {
                    Ok(vec![
                        json!({"kind":"approval_item_confirmed","provider_item_id":item.as_str()}),
                    ])
                } else {
                    Ok(Vec::new())
                }
            }
            _ => Err(E::Unsupported),
        }
    }
    fn reported_fingerprint(&self, p: Value, item: &str) -> ControlResult<String> {
        let mut p = p;
        p["item"]["status"] = json!("inProgress");
        p.as_object_mut()
            .ok_or(E::InvalidRequest)?
            .remove("completedAtMs");
        p["startedAtMs"] = json!(0);
        let proposed = codec(WireMessage::from_value(
            json!({"method":"item/started","params":p}),
            Direction::ServerToClient,
        ))?;
        let proposal = codec(safety::local_notification(&proposed))?.ok_or(E::GateDenied)?;
        let value = match proposal.kind {
            NoticeKind::CommandProposed(p) => {
                json!({"kind":"command","command":p.command,"cwd":p.cwd,"parsed_actions":p.actions})
            }
            NoticeKind::FileProposed(p) => {
                json!({"kind":"files","changes":p.changes.into_iter().map(|c|json!({"path":c.path,"kind":c.kind,"move_path":c.move_path,"diff":c.diff})).collect::<Vec<_>>()})
            }
            _ => return Err(E::GateDenied),
        };
        let reported = self.action(value, item)?.metadata(item)?.1;
        Ok(reported)
    }
    pub(super) fn terminal_items(&self, message: &WireMessage) -> ControlResult<()> {
        let p = message.params().ok_or(E::InvalidRequest)?;
        for item in p["turn"]["items"].as_array().ok_or(E::InvalidRequest)? {
            if !matches!(
                item["type"].as_str(),
                Some("commandExecution" | "fileChange")
            ) {
                continue;
            }
            let id = item["id"].as_str().ok_or(E::InvalidRequest)?;
            let a = self.approvals.get(id).ok_or(E::GateDenied)?;
            if !a.ended || a.decision.is_none() {
                return Err(E::GateDenied);
            }
            let params = json!({"threadId":p["threadId"],"turnId":p["turn"]["id"],"item":item});
            if self.reported_fingerprint(params, id)? != a.fingerprint {
                return Err(E::CorrelationError);
            }
        }
        Ok(())
    }
    pub(super) fn critical(&mut self, frame: &[u8]) -> ControlResult<()> {
        if self.local.is_none() {
            return Err(E::Unsupported);
        }
        self.host_proven()?;
        if !self.ready || !self.proof || self.turn.is_none() {
            return Err(E::GateDenied);
        }
        let frame = std::str::from_utf8(frame).map_err(|_| E::InvalidRequest)?;
        let words: Vec<_> = frame.split(' ').collect();
        if words.len() != 3 || words[0] != "approval" {
            return Err(E::InvalidRequest);
        }
        let a = self
            .approvals
            .get_mut(words[1])
            .ok_or(E::ApprovalNotFound)?;
        if a.decision.is_some()
            || a.consumed
            || a.ended
            || a.action.metadata(words[1])?.1 != a.fingerprint
        {
            return Err(E::CorrelationError);
        }
        let (offered, decision) = match words[2] {
            "accept" => (a.offered.accept, codex::ApprovalDecision::Accept),
            "decline" => (a.offered.decline, codex::ApprovalDecision::Decline),
            "cancel" => (a.offered.cancel, codex::ApprovalDecision::Cancel),
            _ => return Err(E::InvalidRequest),
        };
        if !offered {
            return Err(E::Unsupported);
        }
        let encoded =
            codec(codec(codex::encode_approval_response(&a.message, decision))?.encode())?;
        // Consume before writing, including EAGAIN or uncertain partial writes.
        // Authority fences failures; this connection never retransmits decisions.
        a.decision = Some(words[2].into());
        self.proof = false;
        #[cfg(test)]
        if let Some(fault) = self.critical_fault.take() {
            return match fault {
                NativeCriticalFault::Backpressure => Err(E::CapacityExceeded),
                NativeCriticalFault::ReapStuck => Err(E::TransportError),
                NativeCriticalFault::PrefixThenFail => {
                    self.worker.send_critical(&encoded[..encoded.len() / 2])?;
                    Err(E::TransportError)
                }
                NativeCriticalFault::AfterWriteFail => {
                    self.worker.send_critical(&encoded)?;
                    Err(E::TransportError)
                }
            };
        }
        self.worker.send_critical(&encoded)
    }
}
