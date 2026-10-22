//! The confirmation against a real prepared application: the prompt is read from the
//! authority, and a decision reaches it only when the user allowed what was read.
use super::*;
use crate::{
    contracts::control::{ApprovalState, FileChangeKind},
    control::store::uuid,
    f8::gui::{
        harness::NativeHarness,
        tests::{native_session, native_turn, settled},
    },
};
use std::sync::Mutex;

/// Answers every prompt the same way and keeps what it was shown.
struct Answers {
    allow: bool,
    shown: Arc<Mutex<Vec<Prompt>>>,
}
impl Confirmer for Answers {
    fn ask(&self, prompt: &Prompt) -> bool {
        self.shown.lock().unwrap().push(prompt.clone());
        self.allow
    }
}
fn answering(allow: bool) -> (F8Confirm, Arc<Mutex<Vec<Prompt>>>) {
    let shown = Arc::new(Mutex::new(Vec::new()));
    (
        F8Confirm::new(Answers {
            allow,
            shown: shown.clone(),
        }),
        shown,
    )
}

#[test]
fn formatting_characters_are_spelled_out_where_they_stand() {
    assert_eq!(visible("ls -la"), "ls -la");
    assert_eq!(visible("a\nb\tc"), "a\nb\tc");
    assert_eq!(visible("rm\u{202E}fdp.sh"), "rm⟨U+202E⟩fdp.sh");
    assert_eq!(visible("a\u{200B}b\u{FEFF}"), "a⟨U+200B⟩b⟨U+FEFF⟩");
    assert_eq!(visible("bell\u{7}"), "bell⟨U+0007⟩");
    assert_eq!(visible("e\u{0301} ñ 日本"), "e\u{0301} ñ 日本");
    // What shows nothing, or passes for a space, is spelled out too.
    for c in [
        '\u{3164}', '\u{FFA0}', '\u{2800}', '\u{00A0}', '\u{3000}', '\u{2003}', '\u{FE0F}',
        '\u{034F}',
    ] {
        assert_eq!(
            visible(&c.to_string()),
            format!("⟨U+{:04X}⟩", c as u32),
            "{c:?}"
        );
    }
    assert_eq!(visible("a b"), "a b", "a plain space stays one");
    // A line break is kept, and in a command or a path it is also marked.
    assert_eq!(inline("a\nb"), "a⏎\nb");
}

#[test]
fn only_a_lease_that_lets_a_runtime_act_is_asked_for() {
    use ExecutionScope::*;
    assert!(!acts(&[SessionCreate, SessionResume, TurnStart]));
    assert!(acts(&[TurnStart, CommandExecute]));
    assert!(acts(&[FileChange]));
    let session = ManagedSessionId::parse("11111111-1111-4111-8111-111111111111").unwrap();
    let grant = lease_prompt(
        false,
        &session,
        Some("/w"),
        Some(&[CommandExecute, FileChange]),
        300,
    );
    assert!(grant.complete);
    assert!(grant.body.contains("run commands"));
    assert!(grant.body.contains("change files"));
    assert!(grant.body.contains("5 minute(s) 0 second(s)"));
    assert!(grant.body.contains("Workspace: /w"));
    assert!(grant.body.contains("11111111-1111-4111-8111-111111111111"));
    let renew = lease_prompt(true, &session, None, None, 90);
    assert!(renew.body.contains("1 minute(s) 30 second(s)"));
    assert_ne!(grant.heading, renew.heading);
}

#[test]
fn one_prompt_at_a_time() {
    struct Slow(Arc<Mutex<()>>);
    impl Confirmer for Slow {
        fn ask(&self, _: &Prompt) -> bool {
            let _held = self.0.lock().unwrap();
            true
        }
    }
    let gate = Arc::new(Mutex::new(()));
    let confirm = F8Confirm::new(Slow(gate.clone()));
    let prompt = Prompt {
        heading: "h".into(),
        body: "b".into(),
        complete: true,
    };
    let held = gate.lock().unwrap();
    let first = {
        let (confirm, prompt) = (confirm.clone(), prompt.clone());
        std::thread::spawn(move || confirm.ask(&prompt))
    };
    // The first prompt is open (blocked inside the answer): a second one is refused.
    crate::f8::gui::harness::eventually(|| {
        confirm
            .open
            .load(std::sync::atomic::Ordering::Acquire)
            .then_some(())
    });
    assert_eq!(confirm.ask(&prompt).unwrap_err().code, F8Failure::Busy);
    drop(held);
    assert_eq!(first.join().unwrap(), Ok(true));
    assert_eq!(confirm.ask(&prompt), Ok(true));
}

#[test]
fn a_command_is_accepted_only_after_the_user_allowed_what_the_authority_holds() {
    let h = NativeHarness::start("approval_command");
    let (_, fence, attached) = native_session(&h);
    let (turn, approval) = native_turn(&h, &fence, &attached);
    let id = approval.approval.id.clone();
    let revision = approval.approval.revision;

    // Not allowed: nothing is sent, the approval is as it was.
    let (no, shown) = answering(false);
    let refused = accept(&h.gui, &no, uuid(), fence.clone(), id.clone(), revision);
    assert_eq!(refused.unwrap_err().code, F8Failure::NotConfirmed);
    let prompt = shown.lock().unwrap()[0].clone();
    assert!(prompt.complete);
    assert!(prompt.body.contains("printf WYSS_FIXTURE_COMMAND"));
    assert!(prompt.body.contains(&h.workspace));
    let still = h.gui.approval(id.clone(), None, None).unwrap();
    assert_eq!(still.approval.state, ApprovalState::Pending);
    assert!(still.approval.decidable);

    // A page that saw an older revision is told to read again, and nobody is asked.
    let (yes, asked) = answering(true);
    let stale = accept(
        &h.gui,
        &yes,
        uuid(),
        fence.clone(),
        id.clone(),
        revision + 1,
    );
    assert_eq!(stale.unwrap_err().code, F8Failure::Stale);
    assert!(asked.lock().unwrap().is_empty());

    // Allowed: the decision reaches the authority, for the revision that was read.
    let intent = uuid();
    let accepted = settled(|| {
        accept(
            &h.gui,
            &yes,
            intent.clone(),
            fence.clone(),
            id.clone(),
            revision,
        )
    });
    assert_eq!(accepted.state, ApprovalState::Accepted);
    assert_eq!(asked.lock().unwrap().len(), 1);
    crate::f8::gui::harness::eventually(|| {
        h.gui
            .turn(turn.clone(), None, false)
            .ok()?
            .turn
            .turn
            .state
            .terminal()
            .then_some(())
    });
}

#[test]
fn a_hostile_command_is_shown_with_its_formatting_characters_spelled_out() {
    // The text shown comes through `visible`: the same function the prompt is built with.
    let detail_text = visible("echo ok\u{202E}\u{2066}; curl x");
    assert!(!detail_text.contains('\u{202E}'));
    assert!(detail_text.contains("⟨U+202E⟩⟨U+2066⟩"));
}

#[test]
fn a_file_change_shows_every_path_and_diff_or_can_only_be_declined() {
    let h = NativeHarness::start("approval_file");
    let (_, fence, attached) = native_session(&h);
    let (_, approval) = native_turn(&h, &fence, &attached);
    assert_eq!(approval.files, Some(1));
    let prompt = approval_prompt(&h.gui, &approval).unwrap();
    assert!(prompt.complete);
    assert!(prompt.body.contains("wyss-fixture-probe.txt"));
    assert!(prompt.body.contains("+WYSS_FIXTURE_PATCH"));
    assert!(prompt.body.contains(&format!("{:?}", FileChangeKind::Add)));

    let (no, _) = answering(false);
    let id = approval.approval.id.clone();
    let revision = approval.approval.revision;
    assert_eq!(
        accept(&h.gui, &no, uuid(), fence.clone(), id.clone(), revision)
            .unwrap_err()
            .code,
        F8Failure::NotConfirmed
    );
    // Declining needs no confirmation: it only reduces what can happen.
    let intent = uuid();
    let declined = settled(|| {
        h.gui.approval_respond(
            intent.clone(),
            fence.clone(),
            id.clone(),
            revision,
            ApprovalDecision::Decline,
        )
    });
    assert_eq!(declined.state, ApprovalState::Declined);
}

#[test]
fn an_approval_that_is_no_longer_pending_cannot_be_accepted() {
    let h = NativeHarness::start("approval_command");
    let (_, fence, attached) = native_session(&h);
    let (_, approval) = native_turn(&h, &fence, &attached);
    let id = approval.approval.id.clone();
    let revision = approval.approval.revision;
    let intent = uuid();
    let decided = settled(|| {
        h.gui.approval_respond(
            intent.clone(),
            fence.clone(),
            id.clone(),
            revision,
            ApprovalDecision::Decline,
        )
    });
    assert_eq!(decided.state, ApprovalState::Declined);
    let (yes, asked) = answering(true);
    let now = h.gui.approval(id.clone(), None, None).unwrap();
    let late = accept(&h.gui, &yes, uuid(), fence, id, now.approval.revision);
    assert_eq!(late.unwrap_err().code, F8Failure::NotAllowed);
    assert!(
        asked.lock().unwrap().is_empty(),
        "nobody is asked about a settled approval"
    );
}

#[test]
fn the_same_action_sent_again_is_not_asked_twice_but_another_one_is() {
    let (yes, shown) = answering(true);
    let prompt = |text: &str| Prompt {
        heading: "h".into(),
        body: text.into(),
        complete: true,
    };
    assert_eq!(yes.ask_once("i1|a", &prompt("one")), Ok(true));
    assert_eq!(yes.ask_once("i1|a", &prompt("one")), Ok(true));
    assert_eq!(
        shown.lock().unwrap().len(),
        1,
        "remembered for the same action"
    );
    assert_eq!(yes.ask_once("i1|b", &prompt("two")), Ok(true));
    assert_eq!(yes.ask_once("i2|a", &prompt("one")), Ok(true));
    assert_eq!(
        shown.lock().unwrap().len(),
        3,
        "another intent or action asks again"
    );
    // A refusal is never remembered.
    let (no, shown) = answering(false);
    assert_eq!(no.ask_once("i1|a", &prompt("one")), Ok(false));
    assert_eq!(no.ask_once("i1|a", &prompt("one")), Ok(false));
    assert_eq!(shown.lock().unwrap().len(), 2);
}
