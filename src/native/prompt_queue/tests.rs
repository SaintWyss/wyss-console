use super::*;
use std::os::unix::fs::PermissionsExt;

fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("prompt-{}", Uuid::new_v4().simple()));
    store::private_dir(&path).unwrap();
    path
}

#[test]
fn edits_preserve_order_and_session_scope_across_reopen() {
    let dir = root();
    let session = Uuid::new_v4().simple().to_string();
    let other = Uuid::new_v4().simple().to_string();
    let mut book = Book::open(&dir);
    let mut queue = book.get(&session).unwrap();
    for text in ["first", "second", "third"] {
        queue = book
            .edit(&session, queue.revision, Edit::Add { text: text.into() })
            .unwrap();
    }
    let queue = book
        .edit(&session, queue.revision, Edit::Move { index: 2, up: true })
        .unwrap();
    let queue = book
        .edit(
            &session,
            queue.revision,
            Edit::Replace {
                index: 1,
                text: "edited".into(),
            },
        )
        .unwrap();
    assert!(
        book.edit(&session, queue.revision - 1, Edit::Remove { index: 0 })
            .is_none()
    );
    let queue = book
        .edit(&session, queue.revision, Edit::Remove { index: 0 })
        .unwrap();
    assert_eq!(queue.entries, ["edited", "second"]);
    let path = dir.join("prompt-queues.json");
    let reopened = Book::open(&dir);
    assert_eq!(reopened.get(&session), Some(queue));
    assert!(reopened.get(&other).unwrap().entries.is_empty());
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!Arming::default().active(), "a reloaded app begins paused");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn failed_attempt_keeps_the_first_item_and_cannot_retry_silently() {
    let dir = root();
    let id = Uuid::new_v4().simple().to_string();
    let mut book = Book::open(&dir);
    let queue = book.edit(&id, 0, Edit::Add { text: "one".into() }).unwrap();
    assert!(book.send(&id, queue.revision, |_| false).is_none());
    let queue = Book::open(&dir).get(&id).unwrap();
    assert_eq!(queue.entries, ["one"]);
    assert!(queue.uncertain);
    assert!(
        book.send(&id, queue.revision, |_| panic!("ambiguous repeat"))
            .is_none()
    );
    let queue = book
        .edit(
            &id,
            queue.revision,
            Edit::Replace {
                index: 0,
                text: "checked".into(),
            },
        )
        .unwrap();
    let queue = book
        .edit(
            &id,
            queue.revision,
            Edit::Add {
                text: "next".into(),
            },
        )
        .unwrap();
    assert_eq!(
        book.send(&id, queue.revision, |b| b == b"\x1b[200~checked\x1b[201~\r")
            .unwrap()
            .entries,
        ["next"]
    );
    assert!(
        book.send(&id, queue.revision, |_| panic!("stale revision"))
            .is_none()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn dispatch_needs_a_fresh_paired_idle_even_when_explicitly_armed() {
    let busy = Turn::event(None, "UserPromptSubmit", 1).unwrap();
    assert!(Arming::arm(3, 6, busy).active());
    assert_eq!(dispatch(true, true, None, None, 1), Dispatch::Pause);
    assert_eq!(
        dispatch(
            true,
            true,
            Turn::event(Some(busy), "idle_prompt", 3),
            None,
            3
        ),
        Dispatch::Wait
    );
    let stopping = Turn::event(Some(busy), "Stop", 2).unwrap();
    assert_eq!(
        dispatch(true, true, Some(stopping), None, 4),
        Dispatch::Wait
    );
    assert_eq!(
        dispatch(true, true, Some(stopping), None, 200),
        Dispatch::Pause
    );
    let idle = Turn::event(Some(stopping), "idle_prompt", 60).unwrap();
    assert_eq!(dispatch(true, true, Some(idle), None, 61), Dispatch::Send);
    let duplicate = Turn::event(Some(idle), "idle_prompt", 61);
    assert_eq!(duplicate, Some(idle));
    assert_eq!(
        dispatch(true, true, duplicate, Some(idle.id), 61),
        Dispatch::Wait
    );
    assert_eq!(dispatch(true, true, Some(idle), None, 70), Dispatch::Pause);
    assert_eq!(dispatch(true, false, Some(idle), None, 61), Dispatch::Pause);
    assert_eq!(dispatch(false, true, Some(idle), None, 61), Dispatch::Pause);
    assert!(
        !Arming::arm(3, 6, idle).active(),
        "arming cannot replay a finished turn"
    );
    for event in [
        "PermissionRequest",
        "PreToolUse",
        "elicitation_dialog",
        "StopFailure",
        "unknown",
    ] {
        let doubtful = Turn::event(Some(stopping), event, 3);
        let late_idle = Turn::event(doubtful, "idle_prompt", 60);
        assert_eq!(dispatch(true, true, late_idle, None, 60), Dispatch::Pause);
    }
}

#[test]
fn input_strips_control_sequences_before_exactly_one_bracketed_submit() {
    let hostile = "uno\x1b[201~\r\ndos\x1b]52;private\x07\u{009b}2J\x1bPdiscard\x1b\\\x03\t🙂";
    let input = input(hostile).unwrap();
    assert_eq!(input, "\x1b[200~uno\ndos\t🙂\x1b[201~\r".as_bytes());
    assert_eq!(input.iter().filter(|&&byte| byte == b'\r').count(), 1);
    assert!(super::input("\x1b[31m\x00").is_none());
}
