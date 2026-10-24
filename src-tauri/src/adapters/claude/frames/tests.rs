use super::*;
use crate::adapters::claude::Profile;

const FIXTURES: [(&str, &str); 6] = [
    (
        "inference-turn",
        include_str!("../../../../claude-bridge/fixtures/inference-turn.jsonl"),
    ),
    (
        "local-tools-approved",
        include_str!("../../../../claude-bridge/fixtures/local-tools-approved.jsonl"),
    ),
    (
        "local-tools-denied",
        include_str!("../../../../claude-bridge/fixtures/local-tools-denied.jsonl"),
    ),
    (
        "local-tools-write-file",
        include_str!("../../../../claude-bridge/fixtures/local-tools-write-file.jsonl"),
    ),
    (
        "builtin-tool-demanded",
        include_str!("../../../../claude-bridge/fixtures/builtin-tool-demanded.jsonl"),
    ),
    (
        "reroute",
        include_str!("../../../../claude-bridge/fixtures/reroute.jsonl"),
    ),
];

fn parse(s: &str) -> ControlResult<Event> {
    Event::parse(s.as_bytes())
}

#[test]
fn every_frame_the_real_sdk_produced_parses() {
    // Recorded from the real claude-agent-sdk 0.2.173 + Claude Code 2.1.309 (`probe/capture_fixtures.py`).
    let mut n = 0;
    for (name, body) in FIXTURES {
        for line in body.lines() {
            Event::parse(line.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e:?}: {line}"));
            n += 1;
        }
    }
    assert!(n > 80, "{n}");
}

#[test]
fn real_inference_turn_has_the_expected_shape() {
    let events: Vec<_> = FIXTURES[0]
        .1
        .lines()
        .map(|l| Event::parse(l.as_bytes()).unwrap())
        .collect();
    assert!(matches!(events[0], Event::Hello { proto: 1, .. }));
    let ready = events
        .iter()
        .find_map(|e| match e {
            Event::Ready {
                profile,
                attestation,
            } => Some((profile, attestation)),
            _ => None,
        })
        .unwrap();
    assert_eq!(ready.0, "inference");
    assert_eq!(ready.1.sdk_version.as_deref(), Some("0.2.173"));
    assert_eq!(ready.1.claude_code_version.as_deref(), Some("2.1.309"));
    assert!(ready.1.mcp_servers.is_empty());
    let text: String = events
        .iter()
        .filter_map(|e| match e {
            Event::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "hello from the model. ".repeat(3));
    let result = events
        .iter()
        .find_map(|e| match e {
            Event::TurnResult {
                usage,
                cost_usd_estimate,
                ..
            } => Some((usage, cost_usd_estimate)),
            _ => None,
        })
        .unwrap();
    assert_eq!(result.0.input, Some(11));
    assert_eq!(result.0.cache_read, Some(5));
    assert!(result.1.is_some());
}

#[test]
fn real_local_tools_proposal_carries_the_exact_arguments() {
    let proposal = FIXTURES[1]
        .1
        .lines()
        .map(|l| Event::parse(l.as_bytes()).unwrap())
        .find_map(|e| match e {
            Event::ActionProposal {
                tool,
                args,
                action,
                turn,
            } => Some((tool, args, action, turn)),
            _ => None,
        })
        .unwrap();
    assert_eq!(proposal.0, "run_command");
    assert_eq!(proposal.1["command"], "ls -la");
    assert_eq!(proposal.1.len(), 1);
    assert_eq!(proposal.3, "turn-1");
}

#[test]
fn reroute_is_visible_as_the_models_in_the_usage_events() {
    let models: Vec<_> = FIXTURES[5]
        .1
        .lines()
        .filter_map(|l| match Event::parse(l.as_bytes()).unwrap() {
            Event::Usage { model, .. } => model,
            _ => None,
        })
        .collect();
    assert_eq!(models, ["claude-haiku-5-5"]);
}

#[test]
fn version_is_required_and_exact() {
    assert!(matches!(
        parse(r#"{"e":"turn_idle","turn":"t"}"#),
        Err(E::ProtocolVersionUnsupported)
    ));
    assert!(matches!(
        parse(r#"{"v":2,"e":"turn_idle","turn":"t"}"#),
        Err(E::ProtocolVersionUnsupported)
    ));
    assert!(matches!(
        parse(r#"{"v":"1","e":"turn_idle","turn":"t"}"#),
        Err(E::ProtocolVersionUnsupported)
    ));
    assert!(parse(r#"{"v":1,"e":"turn_idle","turn":"t"}"#).is_ok());
}

#[test]
fn unknown_events_fields_and_shapes_are_refused() {
    for frame in [
        r#"{"v":1,"e":"exec","cmd":"id"}"#,
        r#"{"v":1,"e":"turn_idle","turn":"t","extra":1}"#,
        r#"{"v":1,"e":"turn_idle"}"#,
        r#"{"v":1,"turn":"t"}"#,
        r#"[1]"#,
        r#"{"v":1,"e":"text","turn":"t","text":""}"#,
        r#"{"v":1,"e":"text","turn":"","text":"x"}"#,
        r#"{"v":1,"e":"text","turn":"t","text":5}"#,
        r#"{"v":1,"e":"usage","turn":"t","input":-1}"#,
        r#"{"v":1,"e":"usage","turn":"t","input":1.5}"#,
        r#"not json"#,
        "",
    ] {
        assert!(parse(frame).is_err(), "{frame}");
    }
}

#[test]
fn duplicate_keys_are_refused() {
    assert!(parse(r#"{"v":1,"e":"turn_idle","turn":"a","turn":"b"}"#).is_err());
    assert!(parse(r#"{"v":1,"v":1,"e":"turn_idle","turn":"a"}"#).is_err());
}

#[test]
fn oversized_frames_and_fields_are_refused() {
    let big = format!(
        r#"{{"v":1,"e":"text","turn":"t","text":"{}"}}"#,
        "a".repeat(MAX_FRAME)
    );
    assert!(matches!(parse(&big), Err(E::FrameTooLarge)));
    let text = format!(
        r#"{{"v":1,"e":"text","turn":"t","text":"{}"}}"#,
        "a".repeat(MAX_TEXT_BYTES + 1)
    );
    assert!(parse(&text).is_err());
    let ok = format!(
        r#"{{"v":1,"e":"text","turn":"t","text":"{}"}}"#,
        "a".repeat(MAX_TEXT_BYTES)
    );
    assert!(parse(&ok).is_ok());
    let long_turn = format!(r#"{{"v":1,"e":"turn_idle","turn":"{}"}}"#, "t".repeat(65));
    assert!(parse(&long_turn).is_err());
    let tools = (0..65)
        .map(|i| format!("\"t{i}\""))
        .collect::<Vec<_>>()
        .join(",");
    let tap = format!(
        r#"{{"v":1,"e":"tap_request","path":"/v1/messages","tools":[{tools}],"model":null,"effort":null,"thinking":null}}"#
    );
    assert!(parse(&tap).is_err());
}

#[test]
fn provider_errors_are_their_own_closed_event() {
    assert!(parse(r#"{"v":1,"e":"provider_error","turn":"t","kind":"server_error"}"#).is_ok());
    assert!(parse(r#"{"v":1,"e":"provider_error","turn":"t"}"#).is_err());
    assert!(parse(r#"{"v":1,"e":"provider_error","kind":"x"}"#).is_err());
    assert!(parse(r#"{"v":1,"e":"provider_error","turn":"t","kind":""}"#).is_err());
}

#[test]
fn proposals_are_bounded_and_complete() {
    let ok = r#"{"v":1,"e":"action_proposal","turn":"t","action":"a1","tool":"run_command","args":{"command":"ls"}}"#;
    assert!(parse(ok).is_ok());
    for bad in [
        r#"{"v":1,"e":"action_proposal","turn":"t","action":"a1","tool":"run_command","args":{}}"#,
        r#"{"v":1,"e":"action_proposal","turn":"t","action":"a1","tool":"run_command","args":{"command":5}}"#,
        r#"{"v":1,"e":"action_proposal","turn":"t","action":"a1","tool":"run_command"}"#,
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    let huge = format!(
        r#"{{"v":1,"e":"action_proposal","turn":"t","action":"a1","tool":"write_file","args":{{"path":"a","content":"{}"}}}}"#,
        "x".repeat(MAX_ACTION_BYTES + 1)
    );
    assert!(parse(&huge).is_err());
}

#[test]
fn non_finite_and_negative_numbers_are_refused() {
    assert!(parse(r#"{"v":1,"e":"rate_limit","status":"allowed","type":"five_hour","resets_at":1,"utilization":-0.5}"#).is_err());
    assert!(parse(r#"{"v":1,"e":"rate_limit","status":"allowed","type":"five_hour","resets_at":-1,"utilization":0.5}"#).is_err());
    assert!(parse(r#"{"v":1,"e":"rate_limit","status":"allowed","type":"five_hour","resets_at":1,"utilization":0.5}"#).is_ok());
}

#[test]
fn unknown_counters_stay_unknown() {
    let Event::Usage { input, output, cache_read, cache_creation, .. } =
        parse(r#"{"v":1,"e":"usage","turn":"t","message_id":null,"model":null,"input":null,"output":0,"cache_creation":null,"cache_read":null}"#)
            .unwrap()
    else {
        unreachable!()
    };
    assert_eq!(
        (input, output, cache_read, cache_creation),
        (None, Some(0), None, None)
    );
}

const SESSION: &str = "5f0c1f3e-8a42-4b0e-9a6f-0b6c1d2e3f40";

fn configure() -> Configure {
    Configure {
        profile: Profile::Inference,
        cwd: "/run/x".into(),
        session: SESSION.into(),
        resume: None,
        model: None,
        effort: None,
        upstream: None,
        api_key: None,
        turn_timeout_s: 600,
    }
}

#[test]
fn commands_encode_to_single_bounded_lines() {
    for c in [
        Command::Configure(Box::new(configure())),
        Command::Bind {
            resume: SESSION.into(),
        },
        Command::Prompt {
            turn: "t1".into(),
            text: "hello\nworld".into(),
            model: None,
        },
        Command::Interrupt { turn: "t1".into() },
        Command::ActionResult {
            action: "act-1".into(),
            outcome: ActionOutcome::Completed,
            output: "a\nb".into(),
        },
        Command::Shutdown,
    ] {
        let bytes = c.encode().unwrap();
        assert!(!bytes.contains(&b'\n'));
        assert!(bytes.len() <= MAX_FRAME);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["v"], 1);
    }
}

#[test]
fn commands_refuse_what_the_bridge_would_refuse() {
    let mut c = configure();
    c.cwd = "relative".into();
    assert!(Command::Configure(Box::new(c.clone())).encode().is_err());
    c.cwd = "/ok".into();
    c.turn_timeout_s = 0;
    assert!(Command::Configure(Box::new(c)).encode().is_err());
    let mut c = configure();
    c.session = "not-a-uuid".into();
    assert!(Command::Configure(Box::new(c)).encode().is_err());
    let mut c = configure();
    c.resume = Some("UPPER-CASE".into());
    assert!(Command::Configure(Box::new(c)).encode().is_err());
    assert!(
        Command::Prompt {
            turn: "t".into(),
            text: "a".into(),
            model: Some("has space".into())
        }
        .encode()
        .is_err()
    );
    assert!(
        Command::Prompt {
            turn: "t".into(),
            text: "a".into(),
            model: Some(String::new())
        }
        .encode()
        .is_err()
    );
    assert!(Command::Bind { resume: "x".into() }.encode().is_err());
    assert!(
        Command::Bind {
            resume: SESSION.to_uppercase()
        }
        .encode()
        .is_err()
    );
    assert!(
        Command::Prompt {
            turn: "t".into(),
            text: String::new(),
            model: None
        }
        .encode()
        .is_err()
    );
    assert!(
        Command::Prompt {
            turn: "t".into(),
            text: "a".repeat(MAX_PROMPT_BYTES + 1),
            model: None
        }
        .encode()
        .is_err()
    );
    assert!(
        Command::Prompt {
            turn: String::new(),
            text: "a".into(),
            model: None
        }
        .encode()
        .is_err()
    );
    assert!(
        Command::ActionResult {
            action: "a".into(),
            outcome: ActionOutcome::Failed,
            output: "o".repeat(MAX_OUTPUT_BYTES + 1)
        }
        .encode()
        .is_err()
    );
}

#[test]
fn bound_events_carry_a_canonical_uuid_only() {
    assert!(parse(&format!(r#"{{"v":1,"e":"bound","session":"{SESSION}"}}"#)).is_ok());
    for bad in ["", "x", &SESSION.to_uppercase(), &format!("{SESSION}0")] {
        assert!(
            parse(&format!(r#"{{"v":1,"e":"bound","session":"{bad}"}}"#)).is_err(),
            "{bad}"
        );
    }
}

#[test]
fn the_api_key_never_shows_in_debug_and_is_validated() {
    let key = ApiKey::new("sk-ant-api03-SECRETVALUE".into()).unwrap();
    let mut c = configure();
    c.api_key = Some(key);
    let shown = format!("{:?}", Command::Configure(Box::new(c.clone())));
    assert!(!shown.contains("SECRET"), "{shown}");
    let encoded = Command::Configure(Box::new(c)).encode().unwrap();
    assert!(String::from_utf8(encoded).unwrap().contains("SECRETVALUE"));
    assert!(ApiKey::new(String::new()).is_err());
    assert!(ApiKey::new("has space".into()).is_err());
    assert!(ApiKey::new("x".repeat(513)).is_err());
}

#[test]
fn outcomes_have_distinct_wire_names() {
    let names: std::collections::BTreeSet<_> = [
        ActionOutcome::Completed,
        ActionOutcome::Denied,
        ActionOutcome::Cancelled,
        ActionOutcome::Failed,
        ActionOutcome::Indeterminate,
    ]
    .into_iter()
    .map(ActionOutcome::wire)
    .collect();
    assert_eq!(names.len(), 5);
}
