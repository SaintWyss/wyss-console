//! TS-SEC-07: the classifier of ADR 0029 on a labelled corpus. The rate at which a
//! secret-bearing entry is passed as clean must be zero on this corpus, and the rate at
//! which a clean entry is held back is printed so that it is known, not assumed.
use super::classify::may_hold_secret;
use super::model::Bounds;
use serde_json::{Value, json};

fn secret_bearing() -> Vec<(&'static str, Value)> {
    vec![
        (
            "token flag",
            json!({"type":"command","command":"notify --token abc123def"}),
        ),
        (
            "token flag, equals",
            json!({"type":"command","command":"notify --api-key=abc123def"}),
        ),
        (
            "env assignment",
            json!({"type":"command","command":"SERVICE_KEY=abcdef run.sh"}),
        ),
        (
            "db password",
            json!({"type":"command","command":"DB_PASS=hunter2 run.sh"}),
        ),
        (
            "bearer header in curl",
            json!({"type":"command","command":"curl -H 'Authorization: Bearer abcdef123' https://x"}),
        ),
        (
            "basic header",
            json!({"type":"command","command":"curl -H \"Authorization: Basic dXNlcjpwYXNz\" u"}),
        ),
        (
            "url credentials",
            json!({"type":"command","command":"git clone https://user:pw@host/r"}),
        ),
        (
            "cookie",
            json!({"type":"command","command":"curl -H 'Cookie: sid=abc; other=1' u"}),
        ),
        (
            "prefixed token",
            json!({"type":"command","command":concat!("x ghp", "_abcdefghijklmnopqrstuvwxyz0123456789")}),
        ),
        (
            "sk token",
            json!({"type":"command","command":concat!("x sk", "-abcdefghijklmnopqrstuvwxyz0123")}),
        ),
        (
            "http headers",
            json!({"type":"http","url":"https://h/x","headers":{"X-Api-Key":"abc"}}),
        ),
        (
            "http authorization",
            json!({"type":"http","url":"https://h/x","headers":{"Authorization":"Bearer abc"}}),
        ),
        (
            "env map",
            json!({"type":"mcp_tool","server":"s","tool":"t","env":{"TOKEN":"abc"}}),
        ),
        (
            "env map, benign names",
            json!({"type":"command","command":"run.sh","env":{"MODE":"fast"}}),
        ),
        (
            "secret field",
            json!({"type":"command","command":"run.sh","password":"abc"}),
        ),
        (
            "nested secret",
            json!({"type":"command","command":"run.sh","options":{"client_secret":"abc"}}),
        ),
        (
            "name/value pair",
            json!({"type":"command","command":"run.sh","extra":[{"name":"X-Auth-Token","value":"abc"}]}),
        ),
        (
            "numeric pin",
            json!({"type":"command","command":"run.sh","pin":123456}),
        ),
        (
            "quoted secret with space",
            json!({"type":"command","command":"run.sh --password 'two words'"}),
        ),
        (
            "bearer env var slot",
            json!({"type":"http","url":"https://h","bearer_token_env_var":"MY_TOKEN"}),
        ),
        (
            "allowed env vars slot",
            json!({"type":"http","url":"https://h","allowedEnvVars":["TOKEN"]}),
        ),
        (
            "token in a url query",
            json!({"type":"command","command":"curl 'https://h/x?access_token=abcdef123456'"}),
        ),
    ]
}

fn clean() -> Vec<(&'static str, Value)> {
    vec![
        (
            "plain script",
            json!({"type":"command","command":"/home/u/.claude/hooks/guard.sh"}),
        ),
        (
            "with timeout",
            json!({"type":"command","command":"guard.sh","timeout":5}),
        ),
        (
            "with flags",
            json!({"type":"command","command":"lint --fix --max-tokens 4096"}),
        ),
        (
            "with a message",
            json!({"type":"command","command":"echo 'session started'","statusMessage":"Starting"}),
        ),
        (
            "matcher style",
            json!({"type":"command","command":"python3 /opt/hooks/check.py --strict"}),
        ),
        (
            "empty env slot",
            json!({"type":"command","command":"run.sh","env":{}}),
        ),
        (
            "a hotkey",
            json!({"type":"command","command":"notify --hotkey ctrl+k"}),
        ),
        (
            "a url without credentials",
            json!({"type":"http","url":"https://example.com/hook"}),
        ),
        (
            "a prompt handler",
            json!({"type":"prompt","prompt":"Is this change safe?"}),
        ),
        (
            "a monkey",
            json!({"type":"command","command":"monkey-test.sh"}),
        ),
    ]
}

#[test]
fn no_secret_bearing_entry_in_the_corpus_is_passed_as_clean() {
    let bounds = Bounds::PROPOSED;
    let missed: Vec<&str> = secret_bearing()
        .into_iter()
        .filter(|(_, entry)| !may_hold_secret(entry, &bounds))
        .map(|(name, _)| name)
        .collect();
    assert!(missed.is_empty(), "passed as clean: {missed:?}");
}

#[test]
fn what_is_clean_is_mostly_let_through_and_the_rate_is_known() {
    let bounds = Bounds::PROPOSED;
    let corpus = clean();
    let held: Vec<&str> = corpus
        .iter()
        .filter(|(_, entry)| may_hold_secret(entry, &bounds))
        .map(|(name, _)| *name)
        .collect();
    println!("held back of {} clean entries: {held:?}", corpus.len());
    // Doubt costs a refused plan, not a leak, so this is a budget and not a promise of zero.
    assert!(
        held.len() * 10 <= corpus.len(),
        "too many clean entries held back: {held:?}"
    );
}

#[test]
fn an_entry_too_deep_to_judge_is_not_trusted() {
    let mut deep = json!("x");
    for _ in 0..80 {
        deep = json!([deep]);
    }
    assert!(may_hold_secret(&deep, &Bounds::PROPOSED));
}
