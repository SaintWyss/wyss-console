use super::*;
use serde_json::json;
use std::io::Write;

fn rollout_record() -> Value {
    json!({
        "timestamp": "2026-11-23T03:46:12.001Z",
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "model_context_window": 1000,
                "last_token_usage": {"input_tokens": 700, "total_tokens": 750},
                "total_token_usage": {"total_tokens": 10000}
            },
            "rate_limits": {
                "limit_id": "codex",
                "secondary": {"window_minutes": 300, "used_percent": 12.5, "resets_at": 1795400000u64},
                "primary": {"window_minutes": 10080, "used_percent": 20, "resets_at": 1795900000u64}
            }
        }
    })
}

fn sample(data: &Value) -> Option<Reading> {
    codex_reading(&serde_json::from_value(data.clone()).ok()?)
}

#[test]
fn claude_status_is_session_bound_and_only_accepts_numeric_fields() {
    let id = Uuid::new_v4();
    let raw = json!({"session_id":id.to_string(), "context_window":{"used_percentage": 49.5},
        "rate_limits":{"five_hour":{"used_percentage":100, "resets_at":150},
                       "seven_day":{"used_percentage":"9", "resets_at":900}}});
    let bytes = raw.to_string();
    let reading = claude_reading(bytes.as_bytes(), id, 30).unwrap();
    assert_eq!(reading.context, Some(49.5));
    assert_eq!(
        reading.plan.five_hours,
        Some(Window {
            used: 100.,
            resets_at: Some(150),
            at: 30
        })
    );
    assert_eq!(reading.plan.weekly, None);
    assert!(claude_reading(bytes.as_bytes(), Uuid::new_v4(), 30).is_none());
    assert!(claude_reading(&bytes.as_bytes()[..bytes.len() - 1], id, 30).is_none());
    assert!(claude_reading(b"unexpected", id, 30).is_none());
    let absent = json!({"session_id":id.to_string(), "context_window":{"used_percentage":-1}});
    assert_eq!(
        claude_reading(absent.to_string().as_bytes(), id, 30),
        Some(Reading::default())
    );
}

#[test]
fn codex_reads_last_measured_context_and_discriminates_rate_windows_by_duration() {
    let mut data = rollout_record();
    let reading = sample(&data).unwrap();
    assert_eq!(reading.context, Some(75.0));
    assert_eq!(reading.plan.five_hours.unwrap().used, 12.5);
    assert_eq!(reading.plan.weekly.unwrap().used, 20.0);
    assert_eq!(reading.plan.five_hours.unwrap().at, 1795405572);
    data["payload"]["info"]["last_token_usage"]["input_tokens"] = json!(0);
    assert_eq!(sample(&data).unwrap().context, None);
    data["payload"]["rate_limits"]["limit_id"] = json!("different");
    assert_eq!(sample(&data).unwrap().plan, Plan::default());
    data["payload"]["type"] = json!("unrecognized");
    assert_eq!(sample(&data), None);
}

#[test]
fn codex_missing_malformed_and_ambiguous_fields_stay_unknown() {
    let mut data = rollout_record();
    data["timestamp"] = json!("2026-02-30T03:46:00Z");
    assert_eq!(sample(&data).unwrap().plan, Plan::default());
    assert!(codex_time(&json!("2026-11-23T03:46:00.Z")).is_none());
    assert!(codex_time(&json!("2026-11-23T03:46:00.ßZ")).is_none());
    data = rollout_record();
    data["payload"]["rate_limits"]["primary"]["window_minutes"] = json!(300);
    assert_eq!(sample(&data).unwrap().plan, Plan::default());
    data["payload"]["rate_limits"] = Value::Null;
    data["payload"]["info"]["last_token_usage"]["total_tokens"] = json!("750");
    assert_eq!(sample(&data), Some(Reading::default()));
}

#[test]
fn rollout_tails_only_appended_bytes_and_ignores_bad_lines_and_other_threads() {
    let root = std::env::temp_dir().join(format!("usage-{}", Uuid::new_v4().simple()));
    store::private_dir(&root).unwrap();
    let path = root.join("fixture.jsonl");
    let mut writer = File::create(&path).unwrap();
    let expected = Uuid::new_v4();
    let meta = json!({"type":"session_meta","payload":{"id":expected.to_string()}});
    let record = rollout_record().to_string();
    writeln!(writer, "{meta}\ninvalid").unwrap();
    writer.write_all(record.as_bytes()).unwrap();
    let mut tail = Rollout::default();
    let read = |tail: &mut Rollout| tail.read(File::open(&path).unwrap(), expected).unwrap();
    assert_eq!(read(&mut tail), Reading::default());
    let at = tail.tail.offset;
    assert_eq!(read(&mut tail), Reading::default());
    assert_eq!(tail.tail.offset, at);
    writer.write_all(b"\n").unwrap();
    assert_eq!(read(&mut tail).context, Some(75.0));
    assert_eq!(tail.tail.offset, at + 1);
    writeln!(
        writer,
        "{}",
        json!({"type":"session_meta","payload":{"id":Uuid::new_v4().to_string()}})
    )
    .unwrap();
    let mut other = rollout_record();
    other["payload"]["info"]["last_token_usage"]["total_tokens"] = json!(900);
    writeln!(writer, "{other}").unwrap();
    assert_eq!(read(&mut tail).context, Some(75.0));
    writer.set_len(0).unwrap();
    assert_eq!(read(&mut tail), Reading::default());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollout_record_budget_recovers_after_an_oversized_line() {
    let root = std::env::temp_dir().join(format!("usage-limit-{}", Uuid::new_v4().simple()));
    store::private_dir(&root).unwrap();
    let path = root.join("fixture.jsonl");
    let bytes = format!("{}\nok\n", "x".repeat(MAX_LINE + 1));
    fs::write(&path, bytes).unwrap();
    let mut file = File::open(&path).unwrap();
    let mut tail = Tail::default();
    let mut lines = Vec::new();
    for _ in 0..4 {
        tail.read(&mut file, |line| lines.push(line.to_vec()))
            .unwrap();
    }
    assert_eq!(lines, [b"ok\n"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollout_name_requires_exact_path_and_supports_reverted_threads() {
    let base = Path::new("/fixture/sessions");
    let id = Uuid::new_v4();
    let name = format!("2026/11/23/rollout-2026-11-23T03-46-12-{id}.jsonl");
    assert_eq!(rollout_name(&base.join(&name), base), Some(id));
    assert_eq!(
        rollout_name(
            &base.join(format!(
                "{}_{}.jsonl",
                name.trim_end_matches(".jsonl"),
                Uuid::new_v4()
            )),
            base
        ),
        Some(id)
    );
    assert_eq!(rollout_name(&base.join("../other").join(&name), base), None);
    assert_eq!(rollout_name(&base.join("auth.json"), base), None);
}

#[test]
fn cached_quota_stays_stale_after_reopen_and_is_not_refreshed_by_a_repeated_snapshot() {
    let root = std::env::temp_dir().join(format!("usage-cache-{}", Uuid::new_v4().simple()));
    store::private_dir(&root).unwrap();
    let mut usage = Usage::new(&root);
    let reading = Window {
        used: 12.,
        resets_at: Some(10000),
        at: 1000,
    };
    usage.plans.claude.merge(Plan {
        five_hours: Some(reading),
        weekly: None,
    });
    assert_eq!(usage.plans(), Usage::new(&root).plans);
    let mut restored = Usage::new(&root);
    restored.plans.claude.merge(Plan {
        five_hours: Some(Window {
            at: 2000,
            ..reading
        }),
        weekly: None,
    });
    let cached = restored.plans().claude.five_hours.unwrap();
    assert_eq!(cached.at, 1000);
    assert!(!cached.stale(1899));
    assert!(cached.stale(1900));
    assert!(
        Window {
            resets_at: Some(1001),
            ..cached
        }
        .stale(1001)
    );
    assert_eq!(
        fs::metadata(root.join("usage.json")).unwrap().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn context_thresholds_include_the_boundary() {
    assert_eq!(context_level(49.99), Level::Normal);
    assert_eq!(context_level(50.0), Level::Warning);
    assert_eq!(context_level(74.99), Level::Warning);
    assert_eq!(context_level(75.0), Level::Danger);
}

#[test]
fn codex_account_limits_come_from_the_newest_rollout_without_a_console_session() {
    let root = std::env::temp_dir().join(format!("wyss-usage-{}", std::process::id()));
    let day = root.join("sessions/2026/11/24");
    std::fs::create_dir_all(&day).unwrap();
    let name = "rollout-2026-11-24T10-00-00-01a2105c-62d3-7383-a151-ffe1535ac125.jsonl";
    let mut text = serde_json::to_string(&rollout_record()).unwrap();
    text.push('\n');
    std::fs::write(day.join(name), text).unwrap();
    let mut usage = Usage::new(&root);
    usage.observe_codex_account(&root);
    assert_eq!(usage.plans().codex.five_hours.unwrap().used, 12.5);
    std::fs::remove_dir_all(&root).unwrap();
}
