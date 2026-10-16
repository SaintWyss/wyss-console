//! Older turns against the scripted authority of `tests.rs`: that pages go back through a
//! stream without a repeat or a loss, that the work of one page is bounded, that a position
//! from another history is refused, and that nothing about an old turn is made up. How the
//! real authority answers is covered by `tests/turns_process.rs`.
use super::*;
use crate::sessions::SessionError;

const ADMITTED: &str = "authority.managed_turn.admitted";
const GET: &str = "managedSession/get";

impl World {
    /// A turn of `owner` the authority admitted and has since ended, as the durable stream
    /// and its records show it, put there without going through the service. `kept` says
    /// whether the authority still holds any output of it.
    fn past_turn(&mut self, owner: &ManagedSessionId, kept: bool) -> ManagedTurnId {
        let record: ManagedTurnRecord = serde_json::from_value(json!({
            "managed_turn_id": uuid(), "managed_session_id": owner,
            "codex_thread_id": THREAD, "codex_turn_id": null,
            "admitted_lease_id": uuid(), "admitted_lease_revision": 1,
            "runtime_instance_id": uuid(), "state": "starting", "revision": 1,
            "created_at": "2026-10-08T00:00:00Z", "updated_at": "2026-10-08T00:00:00Z",
            "terminal_at": null, "terminal_reason": null,
        }))
        .unwrap();
        let id = record.managed_turn_id.clone();
        self.audit.push((ADMITTED, owner.clone(), id.clone()));
        self.turns.push(Turn {
            intent: uuid(),
            record,
            events: if kept {
                vec![(StreamKind::AssistantMessageDelta, Some("kept"))]
            } else {
                Vec::new()
            },
            dropped: 0,
            retained: kept,
        });
        self.advance(&id, TurnState::Completed, Some(TurnReason::Completed));
        id
    }
    /// Durable events that name no turn of anyone.
    fn quiet(&mut self, count: usize) {
        let turn = ManagedTurnId::parse(uuid().as_str()).unwrap();
        for _ in 0..count {
            self.audit.push((
                "authority.runtime.observed",
                self.session.clone(),
                turn.clone(),
            ));
        }
    }
    /// The position of the event that admitted `turn`.
    fn position(&self, turn: &ManagedTurnId) -> u64 {
        self.audit
            .iter()
            .position(|(kind, _, id)| *kind == ADMITTED && id == turn)
            .map(|at| at as u64 + 1)
            .unwrap()
    }
}

struct Fixture {
    peer: Peer,
    sessions: SessionService,
    turns: TurnService,
    world: Arc<Mutex<World>>,
    session: ManagedSessionId,
}
impl Fixture {
    fn new() -> Self {
        let session = session_id();
        let (peer, sessions, turns, world) = authority(World::new(&session));
        Self {
            peer,
            sessions,
            turns,
            world,
            session,
        }
    }
    /// The durable head is wherever the stream ends.
    fn settle(&self) {
        *self.peer.head.lock().unwrap() = self.world.lock().unwrap().audit.len() as u64;
    }
    fn stream(&self) -> WireUuid {
        self.sessions
            .connect()
            .unwrap()
            .hello_status()
            .stream_id
            .clone()
    }
    fn older(&self, cursor: &HistoryCursor) -> Result<TurnHistory, SessionError> {
        self.turns.older(
            self.session.clone(),
            cursor.stream_id.clone(),
            cursor.before,
        )
    }
    /// The positions the audit reads since `from` asked for, and how many each asked.
    fn audit_reads(&self, from: usize) -> Vec<(u64, u16)> {
        self.peer
            .requests()
            .split_off(from)
            .into_iter()
            .filter_map(|op| match op {
                Operation::AuditRead(p) => {
                    Some((p.after_sequence.map_or(0, Sequence::get), p.limit))
                }
                _ => None,
            })
            .collect()
    }
}
fn ids(turns: &[TurnSummary]) -> Vec<ManagedTurnId> {
    turns.iter().map(|t| t.id.clone()).collect()
}

#[test]
fn a_list_points_to_older_turns_only_where_the_index_may_not_hold_them_all() {
    let f = Fixture::new();
    let other = session_id();
    // Nothing, and then three turns the index read from the very beginning: whole.
    f.settle();
    assert!(f.turns.list(f.session.clone()).unwrap().older.is_none());
    for _ in 0..3 {
        f.world.lock().unwrap().past_turn(&f.session, false);
    }
    f.settle();
    let whole = f.turns.list(f.session.clone()).unwrap();
    assert_eq!(whole.turns.len(), 3);
    assert!(whole.older.is_none() && !whole.partial);
    assert_eq!(whole.schema_version, 3);

    // More than a page: the list says where the rest begins, which is the place of the
    // oldest turn it shows, in the stream it was read from.
    let mut mine = Vec::new();
    for _ in 0..22 {
        mine.push(f.world.lock().unwrap().past_turn(&f.session, false));
        f.world.lock().unwrap().past_turn(&other, false);
    }
    f.settle();
    let long = f.turns.list(f.session.clone()).unwrap();
    assert_eq!(long.turns.len(), TURN_PAGE);
    let oldest_shown = long.turns.last().unwrap().id.clone();
    let cursor = long.older.clone().unwrap();
    assert_eq!(
        cursor.before.get(),
        f.world.lock().unwrap().position(&oldest_shown)
    );
    assert_eq!(cursor.stream_id, f.stream());
    // Another session sees its own end of the same stream.
    let theirs = f.turns.list(other.clone()).unwrap();
    assert_eq!(theirs.turns.len(), TURN_PAGE);
    assert!(theirs.older.is_some());

    // A stream longer than one read covers: what lies before it is out of reach of the
    // list, and it says where to look.
    let f = Fixture::new();
    f.world.lock().unwrap().past_turn(&f.session, false);
    f.world.lock().unwrap().quiet(2600);
    let recent = f.world.lock().unwrap().past_turn(&f.session, false);
    f.settle();
    let partial = f.turns.list(f.session.clone()).unwrap();
    assert!(partial.partial);
    assert_eq!(ids(&partial.turns), std::slice::from_ref(&recent));
    assert_eq!(
        partial.older.unwrap().before.get(),
        f.world.lock().unwrap().position(&recent)
    );
    // And when it saw nothing at all, it says to look through everything it did not see.
    let f = Fixture::new();
    f.world.lock().unwrap().past_turn(&f.session, false);
    f.world.lock().unwrap().quiet(2600);
    f.settle();
    let none = f.turns.list(f.session.clone()).unwrap();
    assert!(none.turns.is_empty() && none.partial);
    assert_eq!(none.older.unwrap().before.get(), 2602);
}

#[test]
fn pages_go_back_through_the_whole_history_with_no_repeat_and_no_loss() {
    let f = Fixture::new();
    let other = session_id();
    let mut mine = Vec::new();
    {
        let mut world = f.world.lock().unwrap();
        for i in 0..70 {
            mine.push(world.past_turn(&f.session, i % 5 == 0));
            if i % 3 == 0 {
                world.past_turn(&other, false);
            }
            if i % 7 == 0 {
                world.quiet(10);
            }
        }
    }
    f.settle();
    // Newest first, as every list and page is.
    mine.reverse();
    let listed = f.turns.list(f.session.clone()).unwrap();
    assert_eq!(ids(&listed.turns), mine[..TURN_PAGE]);
    let mut seen = ids(&listed.turns);
    let mut cursor = listed.older.clone();
    let mut previous = cursor.clone().unwrap().before.get();
    let mut pages = 0;
    while let Some(at) = cursor {
        let from = f.peer.requests().len();
        let page = f.older(&at).unwrap();
        pages += 1;
        assert_eq!(page.schema_version, 1);
        assert_eq!(page.session, f.session);
        assert!(page.turns.len() <= TURN_PAGE);
        // Every turn is this session's, newer than the ones after it, and older than the
        // position the page was asked for.
        let places: Vec<u64> = page
            .turns
            .iter()
            .map(|t| f.world.lock().unwrap().position(&t.id))
            .collect();
        assert!(places.windows(2).all(|w| w[0] > w[1]), "{places:?}");
        assert!(places.iter().all(|place| *place < at.before.get()));
        assert!(page.turns.iter().all(|t| t.session == f.session));
        seen.extend(ids(&page.turns));
        // The work of a page: one connection's reads, bounded, and no turn's output.
        let methods = since(&f.peer, from);
        assert!(methods.iter().filter(|m| *m == AUDIT).count() <= 40);
        assert_eq!(
            methods.iter().filter(|m| *m == STATUS).count(),
            page.turns.len()
        );
        assert!(!methods.iter().any(|m| m == EVENTS));
        assert!(methods.iter().any(|m| m == GET));
        // It moves back, strictly, and stays in one stream.
        cursor = page.older;
        if let Some(next) = &cursor {
            assert!(next.before.get() < previous);
            assert_eq!(next.stream_id, f.stream());
            previous = next.before.get();
        }
        assert!(pages < 20);
    }
    // Everything there is, once, in order.
    assert_eq!(seen, mine);
    let mut distinct: Vec<&str> = seen.iter().map(|t| t.as_str()).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), seen.len());
}

#[test]
fn more_turns_than_a_page_in_one_stretch_are_found_by_the_next_page_not_lost() {
    let f = Fixture::new();
    let other = session_id();
    {
        let mut world = f.world.lock().unwrap();
        for _ in 0..5 {
            world.past_turn(&other, false);
        }
        // Forty-five turns in as many consecutive positions: more than one page, inside
        // the stretch a single step looks at.
        for _ in 0..45 {
            world.past_turn(&f.session, false);
        }
    }
    f.settle();
    let listed = f.turns.list(f.session.clone()).unwrap();
    let mut all = ids(&listed.turns);
    let mut cursor = listed.older.clone();
    while let Some(at) = cursor {
        let page = f.older(&at).unwrap();
        all.extend(ids(&page.turns));
        cursor = page.older;
    }
    assert_eq!(all.len(), 45);
    let places: Vec<u64> = all
        .iter()
        .map(|t| f.world.lock().unwrap().position(t))
        .collect();
    assert_eq!(places, (6..=50).rev().collect::<Vec<u64>>());
}

#[test]
fn a_long_quiet_stretch_costs_bounded_reads_and_goes_on_exactly_where_it_stopped() {
    let f = Fixture::new();
    let (old, recent) = {
        let mut world = f.world.lock().unwrap();
        let old = world.past_turn(&f.session, false);
        world.quiet(4500);
        (old, world.past_turn(&f.session, false))
    };
    f.settle();
    let listed = f.turns.list(f.session.clone()).unwrap();
    assert_eq!(ids(&listed.turns), std::slice::from_ref(&recent));
    let start = listed.older.unwrap();
    assert_eq!(start.before.get(), 4502);

    // The first page looks at two thousand positions and finds nothing. It says so by
    // handing out the position it got to, and it asked for no more than forty pages.
    let from = f.peer.requests().len();
    let first = f.older(&start).unwrap();
    assert!(first.turns.is_empty());
    let reads = f.audit_reads(from);
    assert_eq!(reads.len(), 40);
    assert!(reads.iter().all(|(_, limit)| *limit <= MAX_PAGE));
    let next = first.older.unwrap();
    assert_eq!(next.before.get(), 4502 - 2000);
    // The reads are exactly the positions in between, going back, each step forwards.
    assert_eq!(reads[0].0, 4502 - 1 - 50);
    assert_eq!(reads[39].0, 4502 - 2000 - 1);

    // And so on, to the turn at the very beginning, and the end of history.
    let second = f.older(&next).unwrap();
    assert!(second.turns.is_empty());
    let last = f.older(&second.older.unwrap()).unwrap();
    assert_eq!(ids(&last.turns), [old]);
    assert!(last.older.is_none());
}

#[test]
fn a_position_of_another_history_is_refused_as_changed_and_nothing_is_read() {
    let f = Fixture::new();
    for _ in 0..4 {
        f.world.lock().unwrap().past_turn(&f.session, false);
    }
    f.settle();
    let head = 4;
    let from = f.peer.requests().len();
    let at =
        |stream: WireUuid, before: u64| (stream, before.to_string().parse::<Sequence>().unwrap());
    // Another stream.
    let (stream, before) = at(uuid(), 3);
    let error = f
        .turns
        .older(f.session.clone(), stream, before)
        .unwrap_err();
    assert_eq!((error.code, error.retry), (SessionFailure::Changed, false));
    // Beyond the head, plus the one position that means "everything".
    let (stream, before) = at(f.stream(), head + 2);
    assert_eq!(
        f.turns
            .older(f.session.clone(), stream, before)
            .unwrap_err()
            .code,
        SessionFailure::Changed
    );
    assert!(
        since(&f.peer, from)
            .iter()
            .all(|m| m != AUDIT && m != STATUS)
    );
    let (stream, before) = at(f.stream(), head + 1);
    assert_eq!(
        f.turns
            .older(f.session.clone(), stream, before)
            .unwrap()
            .turns
            .len(),
        4
    );
    // Nothing is before the first position, and that is not an error.
    let (stream, before) = at(f.stream(), 1);
    let none = f.turns.older(f.session.clone(), stream, before).unwrap();
    assert!(none.turns.is_empty() && none.older.is_none());
}

#[test]
fn a_turn_the_stream_gives_to_the_wrong_session_is_refused() {
    let f = Fixture::new();
    let other = session_id();
    let strange = {
        let mut world = f.world.lock().unwrap();
        world.past_turn(&f.session, false);
        let theirs = world.past_turn(&other, false);
        // The stream says this turn is the first session's, and its record says it is not.
        world
            .audit
            .push((ADMITTED, f.session.clone(), theirs.clone()));
        theirs
    };
    f.settle();
    let cursor = HistoryCursor {
        stream_id: f.stream(),
        before: "4".parse().unwrap(),
    };
    let error = f.older(&cursor).unwrap_err();
    assert_eq!(
        error.code,
        SessionError::of(ControlError::CorrelationError).code
    );
    // And the other session, which was never named for it, gets its own turn only.
    let theirs = f
        .turns
        .older(other.clone(), cursor.stream_id, cursor.before)
        .unwrap();
    assert_eq!(ids(&theirs.turns), [strange]);
}

#[test]
fn a_hole_in_the_stream_fails_the_page_instead_of_being_taken_for_an_empty_stretch() {
    let f = Fixture::new();
    {
        let mut world = f.world.lock().unwrap();
        for _ in 0..6 {
            world.past_turn(&f.session, false);
        }
        world.holes = vec![2];
    }
    f.settle();
    let cursor = HistoryCursor {
        stream_id: f.stream(),
        before: "7".parse().unwrap(),
    };
    assert_eq!(
        f.older(&cursor).unwrap_err().code,
        SessionError::of(ControlError::CorrelationError).code
    );
    // Whole again, it is read.
    f.world.lock().unwrap().holes.clear();
    assert_eq!(f.older(&cursor).unwrap().turns.len(), 6);
}

#[test]
fn a_restarted_desktop_is_given_the_same_pages_from_the_same_positions() {
    let f = Fixture::new();
    {
        let mut world = f.world.lock().unwrap();
        for i in 0..30 {
            world.past_turn(&f.session, false);
            if i % 4 == 0 {
                world.quiet(3);
            }
        }
    }
    f.settle();
    let before = f.turns.list(f.session.clone()).unwrap();
    let cursor = before.older.clone().unwrap();
    let page = f.older(&cursor).unwrap();
    // A new desktop: nothing is remembered, the cursor from the old one is still good.
    let restarted = TurnService::new(f.sessions.clone());
    let again = restarted.list(f.session.clone()).unwrap();
    assert_eq!(ids(&again.turns), ids(&before.turns));
    assert_eq!(again.older, before.older);
    let same = restarted
        .older(f.session.clone(), cursor.stream_id, cursor.before)
        .unwrap();
    assert_eq!(same, page);
}

#[test]
fn an_old_turn_is_what_the_authority_holds_and_its_output_is_never_filled_in() {
    let f = Fixture::new();
    let (gone, kept) = {
        let mut world = f.world.lock().unwrap();
        let gone = world.past_turn(&f.session, false);
        let kept = world.past_turn(&f.session, true);
        for _ in 0..20 {
            world.past_turn(&f.session, false);
        }
        (gone, kept)
    };
    f.settle();
    let listed = f.turns.list(f.session.clone()).unwrap();
    let page = f.older(&listed.older.unwrap()).unwrap();
    assert_eq!(ids(&page.turns), [kept, gone]);
    // What the authority says of the output, and nothing the desktop adds to it.
    assert_eq!(
        page.turns.iter().map(|t| t.output).collect::<Vec<_>>(),
        [true, false]
    );
    assert!(
        page.turns
            .iter()
            .all(|t| t.state == TurnState::Completed && !t.cancellable)
    );
    let wire = serde_json::to_string(&page).unwrap();
    for private in [
        THREAD,
        PROVIDER_TURN,
        "lease",
        "codex",
        "runtime_instance",
        "kept",
    ] {
        assert!(!wire.contains(private), "{private}");
    }
    assert!(page.older.is_none());
}

#[test]
fn no_cursor_names_the_first_position_or_one_before_it() {
    let stream = uuid();
    assert!(super::super::service::cursor(stream.as_str(), 0).is_none());
    assert!(super::super::service::cursor(stream.as_str(), 1).is_none());
    let two = super::super::service::cursor(stream.as_str(), 2).unwrap();
    assert_eq!((two.before.get(), &two.stream_id), (2, &stream));
    // Not a stream: no cursor either.
    assert!(super::super::service::cursor("not a uuid", 9).is_none());
}

#[test]
fn a_page_of_exactly_one_page_of_turns_stops_there_and_goes_on_from_the_end_of_what_it_read() {
    let f = Fixture::new();
    let ids = {
        let mut world = f.world.lock().unwrap();
        // Positions 1 to 50 hold thirty turns of this session and some quiet; 51 to 80 are
        // quiet; 81 to 100 are twenty turns of this session: exactly one page, inside one
        // step, with more history below it.
        for _ in 0..30 {
            world.past_turn(&f.session, false);
        }
        world.quiet(20);
        world.quiet(30);
        (0..20)
            .map(|_| world.past_turn(&f.session, false))
            .collect::<Vec<_>>()
    };
    f.settle();
    let from = f.peer.requests().len();
    let page = f
        .turns
        .older(f.session.clone(), f.stream(), "101".parse().unwrap())
        .unwrap();
    // Exactly a page, found in the one step that was read: the next step is not read for
    // it, and the page goes on from the end of what it did read, not from its oldest turn.
    let mut expected = ids.clone();
    expected.reverse();
    assert_eq!(
        page.turns.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(f.audit_reads(from), [(50, 50)]);
    assert_eq!(page.older.unwrap().before.get(), 51);
}

#[test]
fn a_stream_that_answers_fewer_events_than_asked_is_read_in_as_many_requests_as_it_takes() {
    let f = Fixture::new();
    let mine = {
        let mut world = f.world.lock().unwrap();
        let mine: Vec<_> = (0..12)
            .map(|_| world.past_turn(&f.session, false))
            .collect();
        world.audit_cap = Some(5);
        mine
    };
    f.settle();
    let from = f.peer.requests().len();
    let page = f
        .turns
        .older(f.session.clone(), f.stream(), "13".parse().unwrap())
        .unwrap();
    let mut expected = mine.clone();
    expected.reverse();
    assert_eq!(
        page.turns.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        expected
    );
    // Twelve positions, five at a time: each request asks for what is left, no more.
    assert_eq!(f.audit_reads(from), [(0, 12), (5, 7), (10, 2)]);
    assert!(page.older.is_none());
}

#[test]
fn an_answer_that_is_empty_or_longer_than_asked_is_not_history_and_fails_the_page() {
    for fault in [AuditFault::Empty, AuditFault::TooMany] {
        let f = Fixture::new();
        {
            let mut world = f.world.lock().unwrap();
            // More history than the step asks about, so that "too many" has events to
            // overshoot with.
            for _ in 0..12 {
                world.past_turn(&f.session, false);
            }
            world.audit_fault = fault;
        }
        f.settle();
        let error = f
            .turns
            .older(f.session.clone(), f.stream(), "9".parse().unwrap())
            .unwrap_err();
        assert_eq!(
            error.code,
            SessionError::of(ControlError::CorrelationError).code
        );
    }
}
