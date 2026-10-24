use super::*;
use crate::m3::usage::{Ledger, LimitStatus, LimitWindow, Tokens};
use std::collections::BTreeMap;

const NOW: i64 = 1_800_000_000;
const SESSION: &str = "0d0d0d0d-1111-4222-8333-444444444444";
const RUNTIME: &str = "1e1e1e1e-1111-4222-8333-555555555555";

fn parent() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "wyss-m3-history-{}",
        crate::control::store::uuid().as_str()
    ));
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    fs::canonicalize(p).unwrap()
}
fn origin(generation: u64) -> Origin<'static> {
    Origin {
        provider: Provider::Claude,
        provider_build: "2.1.309",
        session_id: SESSION,
        runtime_id: RUNTIME,
        generation,
    }
}
fn tokens(input: Option<u64>, output: Option<u64>) -> Tokens {
    Tokens {
        input,
        output,
        ..Tokens::default()
    }
}
fn usage(turn: &str, t: Tokens, how: Completeness) -> TurnUsage {
    TurnUsage {
        turn_id: turn.into(),
        provider: Provider::Claude,
        total: t,
        by_model: BTreeMap::new(),
        echoed_models: vec![],
        completeness: how,
        estimate: None,
    }
}
fn rec(turn: &str, generation: u64, at: i64) -> Record {
    Record::turn_usage(
        &origin(generation),
        &usage(turn, tokens(Some(1), Some(2)), Completeness::Complete),
        at,
    )
    .unwrap()
}
fn open(p: &Path, limits: Limits) -> History {
    History::enable_with_gate(p).unwrap();
    History::open_with(p, limits).unwrap()
}
fn small() -> Limits {
    Limits {
        max_records: 20,
        ..Limits::default()
    }
}
fn digest(p: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(p).unwrap()).to_vec()
}

#[test]
fn the_contract_is_not_accepted_so_nothing_opens_or_enables() {
    let p = parent();
    assert_eq!(History::enable(&p), Err(HistoryError::NotAccepted));
    assert!(History::open(&p).is_err());
    assert!(!History::is_enabled(&p));
    assert!(!p.join(DIR).exists(), "nothing was created");
}

#[test]
fn history_is_off_until_the_owner_opts_in() {
    let p = parent();
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::NotEnabled)
    );
    assert!(!p.join(DIR).exists(), "opening never creates a directory");
    History::enable_with_gate(&p).unwrap();
    assert!(History::is_enabled(&p));
    assert!(History::open_with_gate(&p).is_ok());
}

#[test]
fn a_record_has_exactly_the_allowlisted_fields_and_nothing_that_could_hold_content() {
    let r = rec("turn-1", 1, NOW);
    let v: serde_json::Value = serde_json::from_slice(&r.canonical().unwrap()).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    let mut allowed = [
        "schema_version",
        "provider",
        "provider_build",
        "session_id",
        "runtime_id",
        "generation",
        "turn_id",
        "origin_event_id",
        "kind",
        "input_tokens",
        "output_tokens",
        "cache_creation_tokens",
        "cache_read_tokens",
        "reasoning_tokens",
        "limit_window",
        "limit_status",
        "limit_utilization_permille",
        "limit_resets_at",
        "provenance",
        "completeness",
        "freshness",
        "event_at",
        "received_at",
    ]
    .map(String::from);
    allowed.sort();
    assert_eq!(keys, allowed);
    for forbidden in [
        "prompt",
        "text",
        "content",
        "reasoning_text",
        "thinking",
        "command",
        "diff",
        "path",
        "account",
        "email",
        "payload",
        "cost",
        "estimate",
        "model",
    ] {
        assert!(!keys.iter().any(|k| k.contains(forbidden)), "{forbidden}");
    }
}

#[test]
fn the_sql_schema_is_closed_too() {
    let p = parent();
    let h = open(&p, small());
    let cols: Vec<String> = h
        .conn
        .prepare("SELECT name FROM pragma_table_info('records')")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        cols.len(),
        25,
        "id plus the 24 allowlisted fields: {cols:?}"
    );
    // The database itself refuses what the types would: unknown provenance, provider, negative counters.
    let bad = |sql: &str| h.conn.execute(sql, []).is_err();
    let row = |col: &str, val: &str| {
        format!(
            "INSERT INTO records (dedupe,schema_version,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,provenance,completeness,freshness,event_at,received_at,{col}) VALUES (lower(hex(randomblob(32))),1,'claude','1.0','s','r',1,'t','o','turn_usage','provider_usage_event','complete','fresh',1,1,{val})"
        )
    };
    assert!(bad(&row("input_tokens", "-1")));
    assert!(bad(&row("limit_window", "'hourly'")));
    assert!(bad(&row("limit_utilization_permille", "1001")));
    assert!(!bad(&row("input_tokens", "5")));
    assert!(bad(
        "INSERT INTO records (dedupe,schema_version,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,provenance,completeness,freshness,event_at,received_at) VALUES (lower(hex(randomblob(32))),1,'gemini','1.0','s','r',1,'t','o','turn_usage','provider_usage_event','complete','fresh',1,1)"
    ));
    assert!(bad(
        "INSERT INTO records (dedupe,schema_version,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,provenance,completeness,freshness,event_at,received_at) VALUES (lower(hex(randomblob(32))),1,'claude','1.0','s','r',1,'t','o','turn_usage','free text here','complete','fresh',1,1)"
    ));
    assert!(
        h.conn
            .execute("ALTER TABLE records ADD COLUMN notes TEXT", [])
            .is_ok(),
        "SQLite allows it, so the open-time check must catch it"
    );
}

#[test]
fn a_foreign_table_or_extra_column_makes_it_a_different_file() {
    let p = parent();
    {
        let h = open(&p, small());
        h.conn
            .execute("CREATE TABLE prompts (text TEXT)", [])
            .unwrap();
    }
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::Corrupt)
    );
    assert!(
        p.join(DIR).join("history.sqlite3.corrupt").exists(),
        "quarantined, not deleted"
    );
}

#[test]
fn identifiers_are_bounded_charset_checked_and_refused_never_truncated() {
    let o = origin(1);
    let t = usage("turn-1", tokens(Some(1), None), Completeness::Complete);
    let with = |session: &str, runtime: &str, build: &str| {
        let mut o = origin(1);
        o.session_id = session;
        o.runtime_id = runtime;
        o.provider_build = build;
        Record::turn_usage(&o, &t, NOW).err()
    };
    let invalid = Some(HistoryError::InvalidRecord);
    assert_eq!(with("", RUNTIME, "2.1.309"), invalid);
    assert_eq!(with(&"s".repeat(65), RUNTIME, "2.1.309"), invalid);
    assert_eq!(with(SESSION, "has space", "2.1.309"), invalid);
    assert_eq!(with(SESSION, "slash/inside", "2.1.309"), invalid);
    assert_eq!(with(SESSION, "é", "2.1.309"), invalid);
    assert_eq!(with(SESSION, RUNTIME, ""), invalid);
    assert_eq!(with(SESSION, RUNTIME, "2.1.309-rc1"), invalid);
    assert_eq!(with(SESSION, RUNTIME, &"1.".repeat(40)), invalid);
    assert_eq!(with(&"s".repeat(64), RUNTIME, "2.1.309"), None);
    let long = usage(&"t".repeat(129), tokens(None, None), Completeness::Complete);
    assert_eq!(Record::turn_usage(&o, &long, NOW).err(), invalid);
    assert!(
        Record::turn_usage(
            &o,
            &usage(&"t".repeat(128), tokens(None, None), Completeness::Complete),
            NOW
        )
        .is_ok()
    );
    let mut huge = origin(1);
    huge.generation = u64::MAX;
    assert_eq!(Record::turn_usage(&huge, &t, NOW).err(), invalid);
}

#[test]
fn secret_looking_identifiers_are_refused_even_though_the_charset_allows_them() {
    let t = |id: &str| usage(id, tokens(Some(1), None), Completeness::Complete);
    // Each corpus entry is assembled from two pieces so that no source line looks like a secret.
    let join = |a: &str, b: &str| format!("{a}{b}");
    let corpus = [
        join("sk-", "ant-api03-abcdefghijklmnopqrstuvwxyz0123456789"),
        join("sk_", "live_abcdefghijklmnop"),
        join("gh", "p_abcdefghijklmnopqrstuvwxyz0123456789"),
        join("github", "_pat_11ABCDEFG0abcdefghijkl"),
        join("xo", "xb-1234-5678-abcdefghijkl"),
        join("AK", "IAIOSFODNN7EXAMPLE"),
        join("ey", "JhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"),
        "Bearer-abcdefghijkl".to_owned(),
        join("turn:sk-", "ant-api03-aaaaaaaaaaaa"),
        "abc-QwErTy1234567890QwErTy1234567890abcd".to_owned(),
    ];
    for secret in &corpus {
        assert_eq!(
            Record::turn_usage(&origin(1), &t(secret), NOW).err(),
            Some(HistoryError::SecretLooking),
            "{secret}"
        );
    }
    for fine in [
        "turn-1",
        "0d0d0d0d-1111-4222-8333-444444444444",
        "msg_01XYZabc",
        "claude-act-1",
        "limit:five_hour:1900000000",
        "t.1:2_3",
    ] {
        assert!(
            Record::turn_usage(&origin(1), &t(fine), NOW).is_ok(),
            "{fine}"
        );
    }
    let mut o = origin(1);
    let key = ["sk", "-ant-api03-", "zzzzzzzzzzzzzzzz"].concat();
    o.session_id = &key;
    assert_eq!(
        Record::turn_usage(&o, &t("t"), NOW).err(),
        Some(HistoryError::SecretLooking)
    );
}

#[test]
fn the_canonical_size_bound_binds_before_the_identifier_bounds_do() {
    // The contract allows 64+64+128+128 bytes of identifiers and also caps the canonical row at
    // 1,024 bytes. All maxima at once exceed it: the row is refused, never truncated.
    let (s, r, b) = ("s".repeat(64), "r".repeat(64), "9.".repeat(31) + "9");
    let mut o = origin(i64::MAX as u64);
    o.session_id = &s;
    o.runtime_id = &r;
    o.provider_build = &b;
    let max = Tokens {
        input: Some(i64::MAX as u64),
        output: Some(i64::MAX as u64),
        cache_creation: Some(i64::MAX as u64),
        cache_read: Some(i64::MAX as u64),
        reasoning: Some(i64::MAX as u64),
    };
    let t = usage(&"t".repeat(128), max, Completeness::Complete);
    assert_eq!(
        Record::turn_usage(&o, &t, i64::MAX).err(),
        Some(HistoryError::RecordTooLarge)
    );
    // A record built from what the authority actually produces fits with room to spare.
    let real = Record::turn_usage(
        &origin(i64::MAX as u64),
        &usage(&"t".repeat(36), max, Completeness::Complete),
        i64::MAX,
    )
    .unwrap();
    let n = real.canonical().unwrap().len();
    assert!(n < MAX_RECORD_BYTES && n > 400, "{n}");
}

#[test]
fn unknown_stays_null_and_is_never_stored_as_zero() {
    let p = parent();
    let mut h = open(&p, small());
    let r = Record::turn_usage(
        &origin(1),
        &usage("t1", tokens(Some(5), None), Completeness::Interrupted),
        NOW,
    )
    .unwrap();
    assert_eq!(
        (r.output_tokens, r.cache_read_tokens, r.reasoning_tokens),
        (None, None, None)
    );
    h.insert(&r, NOW).unwrap();
    let (_, back) = h.page(0, 10).unwrap().remove(0);
    assert_eq!(
        (
            back.input_tokens,
            back.output_tokens,
            back.cache_creation_tokens,
            back.cache_read_tokens,
            back.reasoning_tokens
        ),
        (Some(5), None, None, None, None)
    );
    assert_eq!(back.completeness, Complete::Partial);
    let n: i64 = h
        .conn
        .query_row(
            "SELECT count(*) FROM records WHERE output_tokens=0 OR cache_read_tokens=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn the_providers_cost_estimate_and_per_model_bookkeeping_are_not_carried_over() {
    let mut l = Ledger::new(Provider::Claude);
    l.begin_turn("t1");
    let mut by_model = BTreeMap::new();
    by_model.insert("claude-sonnet-5-5".to_owned(), tokens(Some(1), Some(1)));
    l.result("t1", tokens(Some(1), Some(1)), by_model, Some(12.34), false);
    let turn = l.snapshot().turns.remove(0);
    assert!(turn.estimate.is_some());
    let r = Record::turn_usage(&origin(1), &turn, NOW).unwrap();
    let json = String::from_utf8(r.canonical().unwrap()).unwrap();
    assert!(
        !json.contains("12.34") && !json.contains("sonnet") && !json.contains("estimate"),
        "{json}"
    );
}

#[test]
fn a_duplicate_origin_event_is_ignored_and_the_key_is_derived() {
    let p = parent();
    let mut h = open(&p, small());
    let a = rec("t1", 1, NOW);
    assert_eq!(h.insert(&a, NOW), Ok(Inserted::New));
    assert_eq!(h.insert(&a, NOW + 5), Ok(Inserted::Duplicate));
    assert_eq!(h.status().unwrap().records, 1);
    // The same turn id in another generation, or another kind, is another observation.
    assert_eq!(h.insert(&rec("t1", 2, NOW), NOW), Ok(Inserted::New));
    let limit = Record::limit(
        &origin(1),
        "t1",
        &LimitObservation {
            window: LimitWindow::FiveHour,
            status: LimitStatus::Allowed,
            resets_at: Some(9),
            utilization: Some(0.5),
            observed_at: 7,
        },
        NOW,
    )
    .unwrap();
    assert_eq!(h.insert(&limit, NOW), Ok(Inserted::New));
    assert_eq!(h.insert(&limit, NOW), Ok(Inserted::Duplicate));
    assert_eq!(h.status().unwrap().records, 3);
    // The key depends on exactly (provider, generation, origin event, kind).
    assert_eq!(a.dedupe_key().len(), 64);
    let mut other_build = a.clone();
    other_build.provider_build = "9.9.9".into();
    assert_eq!(a.dedupe_key(), other_build.dedupe_key());
    let mut other_provider = a.clone();
    other_provider.provider = "codex";
    assert_ne!(a.dedupe_key(), other_provider.dedupe_key());
}

#[test]
fn old_records_are_pruned_by_age_and_the_pruning_is_counted() {
    let p = parent();
    let mut h = open(&p, small());
    let day = 86_400;
    // Each row is stored when it happens (`now` = its own time).
    for (name, age) in [("a", 40), ("b", 35), ("c", 29)] {
        let at = NOW - age * day;
        h.insert(&rec(name, 1, at), at).unwrap();
    }
    assert_eq!(
        h.status().unwrap().records,
        3,
        "nothing was old enough yet at those moments"
    );
    h.insert(&rec("d", 1, NOW), NOW).unwrap();
    let s = h.status().unwrap();
    assert_eq!(
        s.records, 2,
        "40- and 35-day-old rows went when the next one arrived"
    );
    assert_eq!(s.pruned_total, 2, "nothing disappears silently");
    assert_eq!(s.oldest_event_at, Some(NOW - 29 * day));
    // The boundary is exact: a row exactly at the retention age stays, one second older goes.
    let mut h = open(&parent(), small());
    h.insert(&rec("edge", 1, NOW - 30 * day), NOW - 30 * day)
        .unwrap();
    h.insert(&rec("n1", 1, NOW), NOW).unwrap();
    assert_eq!(h.status().unwrap().records, 2);
    let mut h = open(&parent(), small());
    h.insert(&rec("edge", 1, NOW - 30 * day - 1), NOW - 30 * day - 1)
        .unwrap();
    h.insert(&rec("n1", 1, NOW), NOW).unwrap();
    assert_eq!(h.status().unwrap().records, 1);
}

#[test]
fn the_record_count_cap_drops_the_oldest_first_and_counts_them() {
    let p = parent();
    let mut h = open(
        &p,
        Limits {
            max_records: 5,
            ..Limits::default()
        },
    );
    for i in 0..12 {
        h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i)
            .unwrap();
    }
    let s = h.status().unwrap();
    assert_eq!((s.records, s.pruned_total), (5, 7));
    let turns: Vec<_> = h
        .page(0, 50)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r.turn_id)
        .collect();
    assert_eq!(turns, ["t7", "t8", "t9", "t10", "t11"]);
}

#[test]
fn a_write_that_cannot_fit_the_database_quota_is_rolled_back_and_reported() {
    let p = parent();
    let mut h = open(
        &p,
        Limits {
            db_bytes: 16 * 4096,
            max_records: 1_000_000,
            ..Limits::default()
        },
    );
    let mut stored = 0;
    let mut quota = false;
    for i in 0..5_000 {
        match h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i) {
            Ok(Inserted::New) => stored += 1,
            Err(HistoryError::Quota) => {
                quota = true;
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    assert!(quota, "the page limit never triggered");
    assert_eq!(
        h.status().unwrap().records,
        stored,
        "the failed write left no partial row"
    );
    assert!(h.status().unwrap().db_bytes <= 16 * 4096);
    // Pruning does not rescue a full store silently: the owner sees `Quota`.
    assert_eq!(
        h.insert(&rec("another", 1, NOW + 9_999), NOW + 9_999),
        Err(HistoryError::Quota)
    );
}

#[test]
fn the_combined_on_disk_bound_includes_the_journal() {
    let p = parent();
    let mut h = open(
        &p,
        Limits {
            total_bytes: 3 * 4096,
            ..Limits::default()
        },
    );
    let mut last = Ok(Inserted::New);
    for i in 0..400 {
        last = h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i);
        if last.is_err() {
            break;
        }
    }
    assert_eq!(last, Err(HistoryError::Quota));
    h.status().unwrap();
}

/// Child half of the crash test: begins an insert and dies before commit. A no-op in a normal run.
#[test]
fn crash_child_begins_an_insert_and_dies() {
    let Ok(dir) = std::env::var("WYSS_HISTORY_CRASH_DIR") else {
        return;
    };
    let mut h = History::open_with_gate(Path::new(&dir)).unwrap();
    let r = rec("in-flight", 1, NOW + 1);
    let tx = h.conn.transaction().unwrap();
    tx.execute("INSERT INTO records (dedupe,schema_version,provider,provider_build,session_id,runtime_id,generation,turn_id,origin_event_id,kind,provenance,completeness,freshness,event_at,received_at) VALUES (?1,1,'claude','2.1.309','s','r',1,'in-flight','in-flight','turn_usage','provider_usage_event','complete','fresh',1,1)", [r.dedupe_key()]).unwrap();
    std::process::abort();
}

#[test]
fn a_process_that_dies_inside_a_transaction_leaves_no_row_and_no_dedupe_marker() {
    let p = parent();
    {
        let mut h = open(&p, small());
        h.insert(&rec("committed", 1, NOW), NOW).unwrap();
    }
    // A real crash: a separate process of this very test binary, killed before commit.
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "m3::history::tests::crash_child_begins_an_insert_and_dies",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("WYSS_HISTORY_CRASH_DIR", &p)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success(), "the child died by abort");
    let mut h = History::open_with_gate(&p).unwrap();
    assert_eq!(h.status().unwrap().records, 1, "only the committed row");
    let r = rec("in-flight", 1, NOW + 1);
    assert_eq!(
        h.insert(&r, NOW + 1),
        Ok(Inserted::New),
        "its dedupe marker did not survive the crash"
    );
    let ok: String = h
        .conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
}

#[test]
fn a_locked_database_is_a_storage_error_that_the_recorder_turns_into_a_state() {
    let p = parent();
    let h = open(&p, small());
    let mut rec_ = Recorder::with(h);
    // Another connection holds the write lock.
    let other = Connection::open(p.join(DIR).join(DB)).unwrap();
    other
        .execute_batch("PRAGMA busy_timeout=0; BEGIN EXCLUSIVE;")
        .unwrap();
    rec_.store
        .as_mut()
        .unwrap()
        .conn
        .busy_timeout(std::time::Duration::from_millis(0))
        .unwrap();
    rec_.record(&rec("t1", 1, NOW), NOW);
    assert_eq!(rec_.state(), SinkState::Partial("storage"));
    // Further observations are dropped, not retried against a store that is down.
    other.execute_batch("ROLLBACK").unwrap();
    rec_.record(&rec("t2", 1, NOW), NOW);
    assert_eq!(rec_.counts(), (0, 2));
}

#[test]
fn the_recorder_never_returns_an_error_and_off_means_dropped() {
    let mut off = Recorder::off();
    off.record(&rec("t1", 1, NOW), NOW);
    assert_eq!((off.state(), off.counts()), (SinkState::Off, (0, 1)));
    let p = parent();
    let mut on = Recorder::with(open(&p, small()));
    on.record(&rec("t1", 1, NOW), NOW);
    assert_eq!((on.state(), on.counts()), (SinkState::Recording, (1, 0)));
    // A record the store refuses is dropped alone; the store stays up.
    let mut bad = rec("t2", 1, NOW);
    bad.generation = -1;
    on.record(&bad, NOW);
    assert_eq!(on.state(), SinkState::Recording);
    on.record(&rec("t3", 1, NOW), NOW);
    assert_eq!(on.counts(), (2, 1));
}

#[test]
fn a_corrupt_database_quarantines_only_itself() {
    let p = parent();
    {
        let mut h = open(&p, small());
        h.insert(&rec("t1", 1, NOW), NOW).unwrap();
    }
    let sibling = p.join("authority.sqlite3");
    fs::write(&sibling, b"control authority bytes").unwrap();
    let before = digest(&sibling);
    fs::write(
        p.join(DIR).join(DB),
        b"this is not a database at all, just garbage bytes",
    )
    .unwrap();
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::Corrupt)
    );
    assert_eq!(
        digest(&sibling),
        before,
        "the control authority's file was not touched"
    );
    // After quarantine the optional store can start fresh.
    let h = History::open_with_gate(&p).unwrap();
    assert_eq!(h.status().unwrap().records, 0);
}

#[test]
fn directory_and_file_attacks_are_refused() {
    // The parent is a symlink to a directory.
    let real = parent();
    let link = std::env::temp_dir().join(format!(
        "wyss-m3-link-{}",
        crate::control::store::uuid().as_str()
    ));
    std::os::unix::fs::symlink(&real, &link).unwrap();
    assert_eq!(
        History::enable_with_gate(&link),
        Err(HistoryError::UnsafeDirectory)
    );
    // The store directory is group-readable.
    let p = parent();
    History::enable_with_gate(&p).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p.join(DIR), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::UnsafeDirectory)
    );
    // The database is a symlink or a hard link to something else.
    let p = parent();
    let mut h = open(&p, small());
    h.insert(&rec("t1", 1, NOW), NOW).unwrap();
    drop(h);
    let victim = p.join("victim.sqlite3");
    fs::write(&victim, b"x").unwrap();
    fs::remove_file(p.join(DIR).join(DB)).unwrap();
    std::os::unix::fs::symlink(&victim, p.join(DIR).join(DB)).unwrap();
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::UnsafeDirectory)
    );
    fs::remove_file(p.join(DIR).join(DB)).unwrap();
    fs::hard_link(&victim, p.join(DIR).join(DB)).unwrap();
    assert_eq!(
        History::open_with_gate(&p).err(),
        Some(HistoryError::UnsafeDirectory)
    );
    assert_eq!(fs::read(&victim).unwrap(), b"x");
}

#[test]
fn audit_configuration_and_vault_files_are_byte_identical_across_every_operation() {
    let p = parent();
    let protected = [
        "authority.sqlite3",
        "control.sqlite3",
        "configuration-journal.jsonl",
        "vault.bin",
    ];
    for f in protected {
        fs::write(p.join(f), format!("protected {f}")).unwrap();
    }
    let before: Vec<_> = protected.iter().map(|f| digest(&p.join(f))).collect();
    let mut h = open(&p, small());
    for i in 0..30 {
        h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i)
            .unwrap();
    }
    let mut out = Vec::new();
    h.export(&mut out).unwrap();
    drop(h);
    History::reset(&p).unwrap();
    let after: Vec<_> = protected.iter().map(|f| digest(&p.join(f))).collect();
    assert_eq!(before, after);
    assert!(
        !p.join(DIR).exists(),
        "reset removed this store's own directory"
    );
}

#[test]
fn export_holds_only_the_allowlisted_schema_and_says_what_retention_removed() {
    let p = parent();
    let mut h = open(
        &p,
        Limits {
            max_records: 3,
            ..Limits::default()
        },
    );
    for i in 0..5 {
        h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i)
            .unwrap();
    }
    let mut out = Vec::new();
    let n = h.export(&mut out).unwrap();
    assert_eq!(n as usize, out.len());
    let lines: Vec<serde_json::Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0]["pruned_total"], 2);
    assert_eq!(lines[0]["records"], 3);
    assert_eq!(
        lines[0]["contains"],
        "provider-reported usage and limit metadata only"
    );
    for row in &lines[1..] {
        assert_eq!(row["schema_version"], 1);
        assert!(row.get("prompt").is_none() && row.get("text").is_none());
    }
}

#[test]
fn export_aborts_cleanly_at_its_size_limit() {
    let p = parent();
    let mut h = open(
        &p,
        Limits {
            export_bytes: 2_000,
            ..Limits::default()
        },
    );
    for i in 0..30 {
        h.insert(&rec(&format!("t{i}"), 1, NOW + i), NOW + i)
            .unwrap();
    }
    let mut out = Vec::new();
    assert_eq!(h.export(&mut out), Err(HistoryError::ExportLimit));
    assert!(out.len() <= 2_000, "{}", out.len());
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.trim_end()
            .ends_with("{\"export_aborted\":\"size_limit\"}"),
        "{text}"
    );
    // The store is untouched by a failed export.
    assert_eq!(h.status().unwrap().records, 30);
}

#[test]
fn reset_deletes_this_store_only_and_reports_what_it_did() {
    let p = parent();
    let mut h = open(&p, small());
    h.insert(&rec("t1", 1, NOW), NOW).unwrap();
    drop(h);
    fs::write(p.join("keep.txt"), "mine").unwrap();
    let removed = History::reset(&p).unwrap();
    assert!(removed >= 2, "{removed}");
    assert!(!History::is_enabled(&p));
    assert_eq!(fs::read_to_string(p.join("keep.txt")).unwrap(), "mine");
    // Nothing to reset is not an error.
    assert_eq!(History::reset(&p), Err(HistoryError::UnsafeDirectory));
}

#[test]
fn reset_leaves_a_directory_with_foreign_files_alone() {
    let p = parent();
    History::enable_with_gate(&p).unwrap();
    fs::write(p.join(DIR).join("someone-elses.txt"), "x").unwrap();
    History::reset(&p).unwrap();
    assert!(p.join(DIR).join("someone-elses.txt").exists());
}

#[test]
fn turn_usage_maps_completeness_honestly() {
    let t = |how| {
        Record::turn_usage(&origin(1), &usage("t", tokens(Some(1), Some(1)), how), NOW)
            .unwrap()
            .completeness
    };
    assert_eq!(t(Completeness::Complete), Complete::Complete);
    assert_eq!(t(Completeness::ProviderError), Complete::Partial);
    assert_eq!(t(Completeness::Interrupted), Complete::Partial);
    assert_eq!(t(Completeness::Unreported), Complete::Unknown);
    assert_eq!(t(Completeness::Running), Complete::Unknown);
}

#[test]
fn limit_records_keep_utilization_as_integer_permille_and_drop_nonsense() {
    let l = |u| {
        Record::limit(
            &origin(1),
            "t",
            &LimitObservation {
                window: LimitWindow::SevenDay,
                status: LimitStatus::AllowedWarning,
                resets_at: Some(1_900_000_000),
                utilization: u,
                observed_at: 5,
            },
            NOW,
        )
        .unwrap()
    };
    assert_eq!(l(Some(0.723)).limit_utilization_permille, Some(723));
    assert_eq!(l(Some(1.0)).limit_utilization_permille, Some(1000));
    assert_eq!(l(Some(1.5)).limit_utilization_permille, None);
    assert_eq!(l(Some(-0.1)).limit_utilization_permille, None);
    assert_eq!(l(Some(f64::NAN)).limit_utilization_permille, None);
    assert_eq!(l(None).limit_utilization_permille, None);
    let r = l(Some(0.5));
    assert_eq!(
        (r.kind, r.limit_window, r.limit_status, r.provenance),
        (
            Kind::Limit,
            Some(LimitWindowName::SevenDay),
            Some(LimitStatusName::AllowedWarning),
            Provenance::ProviderLimitEvent
        )
    );
}

#[test]
fn a_turn_record_cannot_carry_limit_fields_and_a_limit_record_must() {
    let mut r = rec("t", 1, NOW);
    r.limit_window = Some(LimitWindowName::FiveHour);
    assert_eq!(r.validate(), Err(HistoryError::InvalidRecord));
    let mut r = rec("t", 1, NOW);
    r.kind = Kind::Limit;
    assert_eq!(r.validate(), Err(HistoryError::InvalidRecord));
    let mut r = rec("t", 1, NOW);
    r.input_tokens = Some(-1);
    assert_eq!(r.validate(), Err(HistoryError::InvalidRecord));
}

#[test]
fn error_codes_are_unique_snake_case() {
    use HistoryError::*;
    let all = [
        NotAccepted,
        NotEnabled,
        UnsafeDirectory,
        InvalidRecord,
        RecordTooLarge,
        SecretLooking,
        Quota,
        Corrupt,
        Storage,
        ExportLimit,
    ];
    let codes: std::collections::BTreeSet<_> = all.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), all.len());
}

/// The review's S6: `Record`'s fields are public, and `insert` used to trust a validation that only
/// looked at the numbers. Every field is judged at the boundary, so a record changed after it was
/// built cannot carry text, a path or a secret into the store.
#[test]
fn every_field_is_judged_at_the_insertion_boundary_whatever_was_changed_after_construction() {
    let p = parent();
    let mut h = open(&p, Limits::default());
    let key_like = ["sk", "-ant-api03-", "abcdefghijklmnopqrstuvwxyz0123"].concat();
    let usage_case = || rec("t-ok", 1, NOW);
    let limit_case = || {
        let l = crate::m3::usage::LimitObservation {
            window: LimitWindow::FiveHour,
            status: LimitStatus::Allowed,
            resets_at: Some(1_893_456_000),
            utilization: Some(0.5),
            observed_at: 1_800_000_000,
        };
        Record::limit(&origin(1), "t-ok", &l, NOW).unwrap()
    };
    let mutations: Vec<(&str, Record, HistoryError)> = vec![
        (
            "session with a space",
            {
                let mut r = usage_case();
                r.session_id = "has space".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "session with a newline",
            {
                let mut r = usage_case();
                r.session_id = "a\nb".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "session with a path",
            {
                let mut r = usage_case();
                r.session_id = "../../etc/passwd".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "session too long",
            {
                let mut r = usage_case();
                r.session_id = "s".repeat(65);
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "session empty",
            {
                let mut r = usage_case();
                r.session_id = String::new();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "session that is a key",
            {
                let mut r = usage_case();
                r.session_id = key_like.clone();
                r
            },
            HistoryError::SecretLooking,
        ),
        (
            "runtime that is text",
            {
                let mut r = usage_case();
                r.runtime_id = "please summarize my repository".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "runtime that is a key",
            {
                let mut r = usage_case();
                r.runtime_id = key_like.clone();
                r
            },
            HistoryError::SecretLooking,
        ),
        (
            "turn that is unicode",
            {
                let mut r = usage_case();
                r.turn_id = "t\u{e9}".into();
                r.origin_event_id = "t\u{e9}".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "turn that is a key",
            {
                let mut r = usage_case();
                r.turn_id = key_like.clone();
                r.origin_event_id = key_like.clone();
                r
            },
            HistoryError::SecretLooking,
        ),
        (
            "origin event unlike its turn",
            {
                let mut r = usage_case();
                r.origin_event_id = "other".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "provider that is not one",
            {
                let mut r = usage_case();
                r.provider = "openai";
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "build that is text",
            {
                let mut r = usage_case();
                r.provider_build = "1.2.x".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "build empty",
            {
                let mut r = usage_case();
                r.provider_build = String::new();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "build too long",
            {
                let mut r = usage_case();
                r.provider_build = "9".repeat(65);
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "usage with limit provenance",
            {
                let mut r = usage_case();
                r.provenance = Provenance::ProviderLimitEvent;
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "usage with a catalogue provenance",
            {
                let mut r = usage_case();
                r.provenance = Provenance::ProviderCatalogue;
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "limit with usage provenance",
            {
                let mut r = limit_case();
                r.provenance = Provenance::ProviderUsageEvent;
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "limit whose origin is not one",
            {
                let mut r = limit_case();
                r.origin_event_id = "evil".into();
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "negative counter",
            {
                let mut r = usage_case();
                r.input_tokens = Some(-1);
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "utilization over a hundred percent",
            {
                let mut r = limit_case();
                r.limit_utilization_permille = Some(1001);
                r
            },
            HistoryError::InvalidRecord,
        ),
        (
            "a usage record with a limit window",
            {
                let mut r = usage_case();
                r.limit_window = Some(LimitWindowName::FiveHour);
                r
            },
            HistoryError::InvalidRecord,
        ),
    ];
    let before = h.status().unwrap().records;
    for (name, record, expected) in &mutations {
        assert_eq!(h.insert(record, NOW), Err(*expected), "{name}");
    }
    assert_eq!(h.status().unwrap().records, before, "nothing was stored");
    // The unmodified records are accepted: the boundary refuses mutations, not the producer.
    assert_eq!(h.insert(&usage_case(), NOW), Ok(Inserted::New));
    assert_eq!(h.insert(&limit_case(), NOW), Ok(Inserted::New));
}

#[test]
fn the_doors_that_skip_the_policy_gate_exist_only_in_test_and_fixture_builds() {
    let source = include_str!("../history.rs");
    let code = source.split("#[cfg(test)]\nmod tests;").next().unwrap();
    for door in [
        "fn enable_with_gate(",
        "fn open_with_gate(",
        "fn open_with(",
    ] {
        let at = code.find(door).unwrap_or_else(|| panic!("{door}"));
        let above = &code[..at];
        let attr = above
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with("#["))
            .unwrap();
        assert!(
            attr.contains("cfg(test)") || attr.contains("cfg(any(test, feature = \"m3-fixture\"))"),
            "{door} is not test/fixture only: {attr}"
        );
    }
}
