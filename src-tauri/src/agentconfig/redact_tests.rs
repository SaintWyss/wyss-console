//! Redaction against a corpus shaped like each provider's files (TS-SEC-01). Every value
//! that must not reach the page carries a marker; the test fails if a marker survives in
//! the serialised projection. Two controls keep the test honest: the raw document must
//! contain every marker (the naive projection fails), and text that is configuration
//! must still be shown (redacting everything does not pass either).
use super::model::{Bounds, SourceFailure, TooLarge};
use super::redact::{self, MASK, Projection, SecretClass, project};
use serde_json::{Value, json};

const MARKER: &str = "wyss-marker";

/// A Claude Code `settings.json` and `~/.claude.json` projection, by shape. URLs with
/// credentials are put together at run time, so the repository holds none.
fn claude_code() -> Value {
    json!({
        "env": {
            "ANTHROPIC_API_KEY": "wyss-marker-01",
            "VISIBLE_CONTROL_NAME": "wyss-marker-02"
        },
        "apiKeyHelper": "/home/u/bin/visible-control-helper.sh --token wyss-marker-03",
        "model": "visible-control-model",
        "hooks": {
            "PreToolUse": [{
                "matcher": "Bash",
                "hooks": [
                    { "type": "command", "command": "/home/u/.claude/hooks/visible-control-guard.py --phase build", "timeout": 5 },
                    { "type": "command", "command": "GH_TOKEN=wyss-marker-04 DEBUG=\"wyss-marker-05 x\" /home/u/bin/notify --api-key wyss-marker-06 --verbose" },
                    { "type": "command", "command": format!("curl -sS -H 'Authorization: Bearer wyss-marker-07' -u admin:wyss-marker-08 'https://bot:{}@hooks.example.com/x?access_token=wyss-marker-10&visible=1'", "wyss-marker-09") },
                    { "type": "command", "command": "tool --password=wyss-marker-11 && other --secret \"wyss-marker-12\"" },
                    {
                        "type": "http",
                        "url": "https://api.example.com/hook?key=wyss-marker-13",
                        "headers": { "Authorization": "Bearer wyss-marker-14", "X-Visible-Control-Header": "wyss-marker-15" }
                    }
                ]
            }]
        },
        "mcpServers": {
            "visible-control-server": {
                "command": "npx",
                "args": ["-y", "visible-control-package", "--token", "wyss-marker-16", "--client-secret=wyss-marker-17"],
                "env": { "SERVER_TOKEN": "wyss-marker-18", "PLAIN": "wyss-marker-19" }
            },
            "remote": {
                "type": "http",
                "url": format!("https://user:{}@mcp.example.com/sse", "wyss-marker-20"),
                "headers": { "X-Api-Key": "wyss-marker-21" }
            }
        },
        "oauthAccount": { "emailAddress": "wyss-marker-22", "accessToken": "wyss-marker-23" }
    })
}

/// A Codex `config.toml` and `hooks.json`, as the JSON their parse produces.
fn codex() -> Value {
    json!({
        "model": "visible-control-codex-model",
        "model_auto_compact_token_limit": 200000,
        "notify": ["python3", "/home/u/.codex/visible-control-notify.py", "--password=wyss-marker-30"],
        "mcp_servers": {
            "docs": {
                "command": "visible-control-docs",
                "args": ["--api-key", "wyss-marker-31"],
                "env": ["DOCS_KEY=wyss-marker-32", "wyss-marker-33"],
                "bearer_token": "wyss-marker-34",
                "http_headers": { "X-Team": "wyss-marker-35" },
                "env_http_headers": { "X-Auth": "wyss-marker-36" }
            }
        },
        "model_providers": {
            "local": { "base_url": "http://u:wyss-marker-37@localhost:4000/v1", "api_key": "wyss-marker-38" }
        },
        "hooks": [
            { "event": "PreToolUse", "command": "export OPENAI_API_KEY=wyss-marker-39; visible-control-hook" }
        ]
    })
}

fn markers(value: &Value) -> usize {
    serde_json::to_string(value)
        .unwrap()
        .matches(MARKER)
        .count()
}

#[test]
fn no_marker_reaches_a_serialised_projection() {
    for (name, document, expected) in [("claude_code", claude_code(), 23), ("codex", codex(), 10)] {
        // Negative control: the naive projection, the document itself, holds them all.
        assert_eq!(markers(&document), expected, "{name}: corpus lost a marker");
        let projection = project(&document, &Bounds::PROPOSED).unwrap();
        let shown = serde_json::to_string(&projection).unwrap();
        assert!(!shown.contains(MARKER), "{name}: {shown}");
        assert!(projection.may_hold_secret());
        // Positive control: configuration is still shown.
        assert!(shown.contains("visible-control"), "{name}: {shown}");
    }
}

#[test]
fn configuration_around_the_secrets_stays_visible() {
    let shown =
        serde_json::to_string(&project(&claude_code(), &Bounds::PROPOSED).unwrap()).unwrap();
    for visible in [
        "VISIBLE_CONTROL_NAME",
        "visible-control-model",
        "/home/u/.claude/hooks/visible-control-guard.py --phase build",
        "visible-control-server",
        "visible-control-package",
        "X-Visible-Control-Header",
        "hooks.example.com/x?access_token=[redacted]&visible=1",
        "\"timeout\",{\"kind\":\"number\",\"value\":5}",
    ] {
        assert!(shown.contains(visible), "{visible} missing from {shown}");
    }
    let codex = serde_json::to_string(&project(&codex(), &Bounds::PROPOSED).unwrap()).unwrap();
    // A count named like a token is a number, not a secret.
    assert!(codex.contains("200000"), "{codex}");
}

#[test]
fn a_clean_entry_is_not_marked_and_a_secret_one_is() {
    let clean = json!({ "type": "command", "command": "/home/u/.claude/hooks/check.sh --phase build", "timeout": 5 });
    assert!(
        !project(&clean, &Bounds::PROPOSED)
            .unwrap()
            .may_hold_secret()
    );
    let secret = json!({ "type": "command", "command": "API_TOKEN=x /home/u/run.sh" });
    assert!(
        project(&secret, &Bounds::PROPOSED)
            .unwrap()
            .may_hold_secret()
    );
}

#[test]
fn command_lines_keep_their_shape_and_lose_their_values() {
    let cases = [
        (
            "FOO=bar BAZ=\"a b\" run --flag x",
            "FOO=[redacted] BAZ=[redacted] run --flag x",
        ),
        (
            "tool --password hunter2 --verbose",
            "tool --password [redacted] --verbose",
        ),
        ("tool --api-key=abc", "tool --api-key=[redacted]"),
        (
            "curl -u admin:pw https://h",
            "curl -u admin:[redacted] https://h",
        ),
        (
            "curl -H 'Authorization: Bearer abc' https://h",
            "curl -H 'Authorization: [redacted]' https://h",
        ),
        (
            "curl -H \"X-Custom: v\" https://h",
            "curl -H \"X-Custom: [redacted]\" https://h",
        ),
        ("echo $HOME && ls -la", "echo $HOME && ls -la"),
        (
            "/home/u/.claude/bin/check-route-execution.py --phase build",
            "/home/u/.claude/bin/check-route-execution.py --phase build",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(redact::command(input).text.as_str(), expected, "{input}");
    }
    let found = redact::command("A=1 curl -u u:p -H 'Authorization: Bearer t' --token x").found;
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        [
            SecretClass::EnvironmentValue,
            SecretClass::Header,
            SecretClass::UrlCredentials,
            SecretClass::SecretField
        ]
    );
}

#[test]
fn arguments_are_redacted_across_words() {
    let shown: Vec<_> =
        redact::arguments(&["--token", "v1", "plain", "--password=v2", "-H", "X-Key: v3"])
            .into_iter()
            .map(|r| r.text.as_str().to_owned())
            .collect();
    assert_eq!(
        shown,
        [
            "--token",
            MASK,
            "plain",
            "--password=[redacted]",
            "-H",
            "X-Key: [redacted]"
        ]
    );
}

#[test]
fn text_loses_url_credentials_schemes_and_secret_assignments() {
    let cases = [
        ("https://user:pw@host/path", "https://[redacted]@host/path"),
        (
            "https://host/path?a=1&api_key=k&b=2",
            "https://host/path?a=1&api_key=[redacted]&b=2",
        ),
        ("Bearer abc.def", "Bearer [redacted]"),
        ("Authorization: token ab12cd", "Authorization: [redacted]"),
        ("the token limit is 5", "the token limit is 5"),
        ("password: hunter2", "password: [redacted]"),
        ("visible text", "visible text"),
    ];
    for (input, expected) in cases {
        assert_eq!(redact::text(input).text.as_str(), expected, "{input}");
    }
}

/// Tokens of common services and random-looking runs are hidden wherever they are. Built
/// at run time, so the repository holds nothing shaped like a credential.
#[test]
fn token_shapes_are_hidden_anywhere() {
    let body = "A1b2C3d4E5f6G7h8J9k0L1m2N3p4Q5r6";
    for token in [
        format!("{}{body}", concat!("gh", "p_")),
        format!("{}{body}", concat!("s", "k-")),
        format!("{}{body}", concat!("AK", "IA")),
        format!("q9Z{body}x7W"),
    ] {
        let line = format!("run --flag /srv/hooks/{token}/x {token}");
        let shown = redact::text(&line);
        assert!(!shown.text.as_str().contains(body), "{line}");
        assert!(shown.found.contains(&SecretClass::TokenShape));
        assert!(shown.text.as_str().starts_with("run --flag /srv/hooks/"));
    }
    // Long words of one kind are prose or paths, not tokens.
    for plain in [
        "model_auto_compact_token_limit",
        "/home/u/Github_repos/wyss-console/target/release",
        "visible-control-package-with-a-long-name",
    ] {
        assert_eq!(redact::text(plain).text.as_str(), plain);
    }
}

#[test]
fn a_projection_deeper_than_the_bound_is_refused() {
    let bounds = Bounds {
        depth: 3,
        ..Bounds::PROPOSED
    };
    assert!(project(&json!([[[1]]]), &bounds).is_ok());
    assert_eq!(
        project(&json!([[[[1]]]]), &bounds),
        Err(SourceFailure::TooLarge {
            bound: TooLarge::Depth
        })
    );
}

#[test]
fn a_hidden_value_says_only_its_class() {
    let projection = project(
        &json!({ "env": { "K": "wyss-marker-99" } }),
        &Bounds::PROPOSED,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&projection).unwrap(),
        r#"{"kind":"map","value":[["env",{"kind":"map","value":[["K",{"kind":"hidden","value":"environment_value"}]]}]]}"#
    );
    assert!(matches!(projection, Projection::Map(_)));
}

/// Redaction never fails on what a file may hold: open quotes, a trailing backslash,
/// multi-byte characters next to every separator. A fixed generator, so every run sees
/// the same inputs.
#[test]
fn redaction_never_panics_on_odd_text() {
    let alphabet: Vec<char> = "aZ09=:@/?&#;'\"\\ -_.é€😀\t\n|>".chars().collect();
    let mut state: u64 = 0x5eed;
    for _ in 0..5000 {
        let mut line = String::new();
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        for _ in 0..(state >> 58) {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            line.push(alphabet[(state >> 33) as usize % alphabet.len()]);
        }
        for input in [
            line.clone(),
            format!("--token {line}"),
            format!("-H {line}"),
            format!("A={line}"),
        ] {
            let _ = redact::command(&input);
            let _ = redact::text(&input);
            let _ = redact::arguments(&[input.as_str(), "-u", input.as_str()]);
        }
    }
}

/// Texts shaped the way a secret hides in a command, a log line or a document pasted into
/// a field. Each carries markers; none may survive.
#[test]
fn quoted_escaped_and_multi_line_shapes_lose_their_values() {
    let cases = [
        r#"{"apiKey":"wyss-marker-40","note":"visible-control-a"}"#,
        r#"{\"password\":\"wyss-marker-41\",\"note\":\"visible-control-a\"}"#,
        r#"password: "wyss-marker-42 with spaces" note: visible-control-a"#,
        "password='wyss-marker-43' note=visible-control-a",
        r#"password="wyss-marker-50\"wyss-marker-51" note=visible-control-a"#,
        "Cookie: a=wyss-marker-44; b=wyss-marker-45\nnote: visible-control-a",
        r#"curl -H "Cookie: s=wyss-marker-46; t=wyss-marker-47" https://visible-control-a.example"#,
        "SERVICE_KEY=wyss-marker-48 run --visible-control-a",
        "db_pass=wyss-marker-49 run --visible-control-a",
        "api_key: wyss-marker-52\nname: visible-control-a",
        "https://host/x?X-Amz-Signature=wyss-marker-53&note=visible-control-a",
        "token=wyss-marker-54é note=visible-control-a",
    ];
    for case in cases {
        let text = redact::text(case);
        assert!(
            !text.text.as_str().contains("wyss-marker"),
            "{case}\n=> {text:?}"
        );
        assert!(
            text.text.as_str().contains("visible-control-a"),
            "{case}\n=> {text:?}"
        );
        // As a command line every assignment is an environment value and goes, so only
        // the markers are checked.
        let command = redact::command(case);
        assert!(
            !command.text.as_str().contains("wyss-marker"),
            "{case}\n=> {command:?}"
        );
        // Control: the raw text holds a marker, so the test can fail.
        assert!(case.contains("wyss-marker"), "{case}");
    }
}

/// Names that merely end like a secret, and counts named like one, are configuration.
#[test]
fn names_that_only_look_like_secrets_stay_visible() {
    for name in [
        "hotkey",
        "monkey",
        "keyboard",
        "tokenizer",
        "author",
        "pinned",
    ] {
        assert!(
            !redact::is_secret_name(name) || name == "tokenizer",
            "{name} hidden"
        );
    }
    for name in [
        "SERVICE_KEY",
        "serviceKey",
        "api-key",
        "DB_PASS",
        "x_pwd",
        "bearerToken",
        "key",
    ] {
        assert!(redact::is_secret_name(name), "{name} shown");
    }
    let shown = serde_json::to_string(
        &project(
            &json!({
                "max_tokens": 4096,
                "model_auto_compact_token_limit": 200000,
                "pin": 123456,
                "otp": 7,
                "apiKeyHelper": "/home/u/bin/key-helper.sh --visible-control-a",
                "hotkey": "ctrl+x",
                "env": { "PORT": 8080 }
            }),
            &Bounds::PROPOSED,
        )
        .unwrap(),
    )
    .unwrap();
    for visible in [
        "4096",
        "200000",
        "/home/u/bin/key-helper.sh --visible-control-a",
        "ctrl+x",
    ] {
        assert!(shown.contains(visible), "{visible} missing: {shown}");
    }
    for hidden in ["123456", "8080"] {
        assert!(!shown.contains(hidden), "{hidden} leaked: {shown}");
    }
}

/// A file of the largest size is one string: redaction must cost time in proportion to
/// it, whatever the string repeats. Pathological inputs, each a few hundred kilobytes.
#[test]
fn redaction_takes_linear_time_on_pathological_text() {
    let inputs = [
        "authorization:".repeat(40_000),
        "a=".repeat(250_000),
        "password=\"".repeat(60_000),
        "password:'".repeat(60_000),
        "://".repeat(150_000),
        "\\".repeat(300_000),
        "Bearer ".repeat(80_000),
        "token a1 ".repeat(60_000),
        "-H ".repeat(150_000),
        "A".repeat(500_000),
        "é=".repeat(100_000),
        "/".repeat(500_000),
    ];
    let started = std::time::Instant::now();
    for input in &inputs {
        let _ = redact::text(input);
        let _ = redact::command(input);
    }
    assert!(
        started.elapsed() < std::time::Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
}

/// The projection of a document at the element bound is built, and one past it never
/// reaches redaction: the parser refuses it first.
#[test]
fn a_document_at_the_bounds_projects_and_one_past_never_does() {
    let bounds = Bounds {
        elements: 1_000,
        ..Bounds::PROPOSED
    };
    let fits = format!("[{}0]", "0,".repeat(496));
    let parsed = super::parse_json(fits.as_bytes(), &bounds)
        .unwrap()
        .unwrap();
    assert!(project(&parsed, &bounds).is_ok());
    let past = format!("[{}0]", "0,".repeat(1_000));
    assert_eq!(
        super::parse_json(past.as_bytes(), &bounds),
        Err(SourceFailure::TooLarge {
            bound: TooLarge::Elements
        })
    );
}

/// A secret whose name and value sit in sibling fields is found by the document's shape,
/// whatever the key that holds the list.
#[test]
fn a_name_and_value_in_sibling_fields_hide_the_value() {
    let document = json!({
        "extra": [
            { "name": "Authorization", "value": "wyss-marker-55" },
            { "key": "X-Api-Key", "value": "wyss-marker-56" },
            { "name": "visible-control-name", "value": "visible-control-value" }
        ]
    });
    assert_eq!(markers(&document), 2);
    let shown = serde_json::to_string(&project(&document, &Bounds::PROPOSED).unwrap()).unwrap();
    assert!(!shown.contains(MARKER), "{shown}");
    assert!(shown.contains("visible-control-value"), "{shown}");
}
