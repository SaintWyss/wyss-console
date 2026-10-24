use super::*;

fn host(claude: ClaudeInstall, codex: CodexInstall) -> Host {
    Host {
        linux_x86_64: true,
        claude,
        codex,
    }
}
fn verified() -> ClaudeInstall {
    ClaudeInstall::Verified {
        sdk_version: "0.2.173".into(),
        cli_version: "2.1.309".into(),
    }
}
fn connected() -> CodexInstall {
    CodexInstall::Connected {
        runtime_version: Some("0.159.0".into()),
    }
}

#[test]
fn all_four_profiles_are_always_listed_in_a_fixed_order() {
    let c = cards(&host(
        ClaudeInstall::NotConfigured,
        CodexInstall::NotConnected,
    ));
    let ids: Vec<_> = c.iter().map(|c| c.id).collect();
    assert_eq!(ids, ProfileId::ALL);
    assert_eq!(
        c.iter().map(|c| c.name).collect::<Vec<_>>(),
        [
            "codex_inference_v1",
            "codex_local_tools_v1",
            "claude_inference_v1",
            "claude_local_tools_v1"
        ]
    );
}

#[test]
fn claude_is_blocked_in_this_source_even_with_a_perfect_installation() {
    for id in [ProfileId::ClaudeInference, ProfileId::ClaudeLocalTools] {
        let c = card(id, &host(verified(), connected()));
        assert_eq!(c.availability, Availability::Blocked, "{id:?}");
        assert!(!c.can_start);
        for r in [
            "adr0053_not_accepted",
            "store_cannot_hold_claude",
            "real_provider_evidence_missing",
        ] {
            assert!(
                c.reasons.contains(&r.to_owned()),
                "{id:?} {r}: {:?}",
                c.reasons
            );
        }
        assert_eq!(c.build, Some(("0.2.173".into(), "2.1.309".into())));
        // Fixtures pass; the real provider was not run: the card says both, never one for the other.
        assert_eq!(
            (c.evidence.fixture, c.evidence.real_provider),
            (EvidenceState::Passed, EvidenceState::NotRun)
        );
    }
}

#[test]
fn an_unverified_claude_installation_adds_its_own_reason_after_the_decisions_that_block() {
    let c = card(
        ProfileId::ClaudeInference,
        &host(ClaudeInstall::NotConfigured, connected()),
    );
    assert_eq!(c.availability, Availability::Blocked);
    assert!(c.reasons.contains(&"sdk_not_configured".to_owned()));
    assert_eq!(c.reasons[0], "adr0053_not_accepted");
    let c = card(
        ProfileId::ClaudeLocalTools,
        &host(
            ClaudeInstall::Refused {
                reason: "managed_policy_present".into(),
            },
            connected(),
        ),
    );
    assert!(c.reasons.contains(&"managed_policy_present".to_owned()));
    assert_eq!(c.build, None);
}

#[test]
fn codex_follows_the_authority_connection_and_is_never_blocked_by_m3() {
    let on = cards(&host(verified(), connected()));
    for c in &on[..2] {
        assert_eq!(c.availability, Availability::Available, "{:?}", c.id);
        assert!(c.can_start && c.reasons.is_empty());
        assert_eq!(c.build, Some((String::new(), "0.159.0".into())));
    }
    let off = cards(&host(verified(), CodexInstall::NotConnected));
    for c in &off[..2] {
        assert_eq!(c.availability, Availability::Unverified);
        assert_eq!(c.reasons, ["authority_not_connected"]);
        assert!(!c.can_start);
    }
}

#[test]
fn an_unsupported_platform_overrides_everything_and_says_so_first() {
    let mut h = host(verified(), connected());
    h.linux_x86_64 = false;
    for c in cards(&h) {
        assert_eq!(c.availability, Availability::Unsupported, "{:?}", c.id);
        assert_eq!(c.reasons[0], "platform_unsupported");
        assert!(!c.can_start);
    }
}

#[test]
fn only_available_has_a_start_action_and_in_this_source_that_is_codex_alone() {
    let c = cards(&host(verified(), connected()));
    let startable: Vec<_> = c.iter().filter(|c| c.can_start).map(|c| c.id).collect();
    assert_eq!(
        startable,
        [ProfileId::CodexInference, ProfileId::CodexLocalTools]
    );
    assert!(
        c.iter()
            .all(|c| c.can_start == (c.availability == Availability::Available))
    );
}

#[test]
fn profile_facts_are_consistent() {
    assert_eq!(ProfileId::ClaudeLocalTools.provider(), Provider::Claude);
    assert_eq!(ProfileId::ClaudeLocalTools.mode(), Mode::LocalTools);
    assert_eq!(ProfileId::CodexInference.mode(), Mode::Inference);
    // The names are the attachment profile strings the workers present.
    for id in [ProfileId::ClaudeInference, ProfileId::ClaudeLocalTools] {
        assert!(crate::adapters::claude::Profile::from_attachment(id.name()).is_some());
    }
}

#[test]
fn the_serialized_card_is_the_closed_projection_the_window_receives() {
    let json = serde_json::to_value(card(
        ProfileId::ClaudeInference,
        &host(verified(), connected()),
    ))
    .unwrap();
    let mut keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "availability",
            "build",
            "can_start",
            "evidence",
            "id",
            "mode",
            "name",
            "provider",
            "reasons"
        ]
    );
    assert_eq!(json["availability"], "blocked");
    assert_eq!(json["can_start"], false);
}
