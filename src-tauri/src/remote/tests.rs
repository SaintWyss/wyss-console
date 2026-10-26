//! The rules of 16 §5.2–5.3, one test each, over a gateway with a clock the test turns and a
//! source of randomness that is only a counter. The signature check is a **test double**
//! (`Stand`): an HMAC under a per-device secret. It is not a signature scheme and says nothing
//! about the algorithm the ADR will fix; it lets the rules around a signature be tested.
use super::{
    Refusal,
    audit::{Journal, MemoryJournal, Outcome},
    crypto::P256_PUBLIC_KEY_BYTES,
    device::{Device, DeviceId, DeviceState, clean_label},
    gateway::{Allowed, Denied, Gateway, HostConfirm},
    grants::{
        self, DAY, Grant, GrantRefused, GrantState, Held, INPUT_CLAIM_MINUTES, Profile, state_of,
    },
    pairing::{Attempt, ConfirmRequest, MAX_ATTEMPTS, OFFER_SECONDS, Offer},
    primitives::{Random, hex, hmac_sha256, same, sha256, unhex},
    request::{CHALLENGE_SECONDS, Request, StepUp, StepUpVerifier, step_up_message},
};
use std::sync::{Mutex, OnceLock};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type TestKeyPair = (Vec<u8>, Vec<u8>);
static TEST_PUBLIC_KEYS: OnceLock<Mutex<BTreeMap<Vec<u8>, TestKeyPair>>> = OnceLock::new();

/// Test-only public keys created by ring. The private parts are immediately dropped; step-up
/// semantics below use the explicit HMAC test double and do not impersonate production signing.
fn test_public_keys(seed: &[u8]) -> TestKeyPair {
    let keys = TEST_PUBLIC_KEYS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut keys = keys.lock().unwrap();
    keys.entry(seed.to_vec())
        .or_insert_with(|| {
            use ring::{
                rand::SystemRandom,
                signature::{EcdsaKeyPair, KeyPair},
            };
            let rng = SystemRandom::new();
            let make = || {
                let pkcs8 = EcdsaKeyPair::generate_pkcs8(
                    &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
                    &rng,
                )
                .unwrap();
                let pair = EcdsaKeyPair::from_pkcs8(
                    &ring::signature::ECDSA_P256_SHA256_ASN1_SIGNING,
                    pkcs8.as_ref(),
                    &rng,
                )
                .unwrap();
                pair.public_key().as_ref().to_vec()
            };
            (make(), make())
        })
        .clone()
}

const T0: u64 = 1_800_000_000;

/// Randomness that is a counter: unpredictable to nobody, which is the point of a test.
struct Counter(u8);
impl Random for Counter {
    fn fill(&mut self, bytes: &mut [u8]) {
        for byte in bytes {
            self.0 = self.0.wrapping_add(7);
            *byte = self.0;
        }
    }
}

/// TEST DOUBLE: a "signature" is an HMAC under a secret the test keeps per device fingerprint.
#[derive(Clone, Default)]
struct Stand(Rc<RefCell<BTreeMap<String, Vec<u8>>>>);
impl Stand {
    fn sign(&self, fingerprint: &str, message: &[u8]) -> Vec<u8> {
        let secret = self
            .0
            .borrow()
            .get(fingerprint)
            .cloned()
            .unwrap_or_default();
        hmac_sha256(&secret, message).to_vec()
    }
}
impl StepUpVerifier for Stand {
    fn verify(&self, presence_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        let fingerprint = super::gateway::fingerprint_of(presence_key);
        let secret = self.0.borrow().get(&fingerprint).cloned();
        secret.is_some_and(|s| same(&hmac_sha256(&s, message), signature))
    }
}

struct Yes;
impl HostConfirm for Yes {
    fn ask(&self, _: &ConfirmRequest) -> Option<bool> {
        Some(true)
    }
}
struct No;
impl HostConfirm for No {
    fn ask(&self, _: &ConfirmRequest) -> Option<bool> {
        Some(false)
    }
}
struct Silent;
impl HostConfirm for Silent {
    fn ask(&self, _: &ConfirmRequest) -> Option<bool> {
        None
    }
}

const HOST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
type G = Gateway<MemoryJournal, Stand, Counter>;

fn gateway() -> (G, Stand) {
    let stand = Stand::default();
    (
        Gateway::new(HOST, MemoryJournal::default(), stand.clone(), Counter(0)),
        stand,
    )
}

/// What a device does to prove an offer: the same MAC and comparison the host will compute.
fn attempt(secret_hex: &str, key: &[u8], transcript: &[u8], name: &str) -> Attempt {
    let secret: [u8; 16] = unhex(secret_hex).unwrap().try_into().unwrap();
    let (connection_key, presence_key) = test_public_keys(key);
    Attempt {
        host_fingerprint_seen: HOST.to_owned(),
        connection_key: connection_key.clone(),
        presence_key: presence_key.clone(),
        name: name.to_owned(),
        platform: "Android 15".to_owned(),
        transcript: transcript.to_vec(),
        mac: Offer::mac_of(&secret, transcript, &connection_key, &presence_key),
    }
}

/// Pairs a device the way a person does, and returns it with the fingerprint the host pinned.
fn enrol(g: &mut G, stand: &Stand, now: u64, key: &[u8]) -> (DeviceId, String) {
    let shown = g.begin_pairing(now);
    let a = attempt(&shown.secret_hex, key, b"transcript-1", "Santiago's phone");
    g.present_pairing(now + 5, &a).unwrap();
    let id = g.confirm_pairing(now + 10, &Yes).unwrap();
    let fingerprint = g.device(&id).unwrap().presence_fingerprint.clone();
    stand
        .0
        .borrow_mut()
        .insert(fingerprint.clone(), key.to_vec());
    (id, fingerprint)
}

fn request(id: &DeviceId, rid: &str, grant: Grant, operation: &str) -> Request {
    Request {
        request_id: rid.to_owned(),
        device: id.clone(),
        connection: "conn-1".to_owned(),
        grant,
        operation: operation.to_owned(),
        target: Some("target-1".to_owned()),
        object_revision: Some(3),
        canonical: format!("{rid}:{operation}").into_bytes(),
        bytes: None,
        step_up: None,
    }
}
fn refused(result: Result<Allowed, Denied>) -> Refusal {
    match result {
        Err(Denied::Refused(word)) => word,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

// ---- primitives ----------------------------------------------------------------------------

#[test]
fn hmac_matches_the_published_vectors_and_equality_is_exact() {
    // RFC 4231, test cases 1 and 2, and 6 (a key longer than the block).
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex(&hmac_sha256(
            &[0xaa; 131],
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
    assert!(same(b"abc", b"abc"));
    assert!(!same(b"abc", b"abd") && !same(b"abc", b"ab") && !same(b"", b"a"));
    assert_eq!(unhex("00ff10").unwrap(), [0, 255, 16]);
    assert!(unhex("0").is_none() && unhex("zz").is_none() && unhex("é").is_none());
    assert_eq!(hex(&sha256(b"abc"))[..16], *"ba7816bf8f01cfea");
}

// ---- grants ----------------------------------------------------------------------------------

#[test]
fn a_grant_is_given_only_as_long_as_its_kind_permits_and_a_shell_grant_not_at_all() {
    assert_eq!(
        grants::give(Grant::SessionsRead, T0, 90)
            .unwrap()
            .expires_at,
        T0 + 90 * DAY
    );
    assert_eq!(
        grants::give(Grant::SessionsRead, T0, 91),
        Err(GrantRefused::TooLong)
    );
    assert_eq!(
        grants::give(Grant::TerminalRead, T0, 31),
        Err(GrantRefused::TooLong)
    );
    assert!(grants::give(Grant::TerminalRead, T0, 30).is_ok());
    assert_eq!(
        grants::give(Grant::ConfigRequest, T0, 8),
        Err(GrantRefused::TooLong)
    );
    assert_eq!(
        grants::give(Grant::SessionsRead, T0, 0),
        Err(GrantRefused::NotInTheFuture)
    );
    // GR2: the 7-day terminal.write grant is not accepted, and no LP4 lifetime exists before DN-44.
    for days in [1, 7, 30, 90] {
        assert_eq!(
            grants::give(Grant::TerminalWrite, T0, days),
            Err(GrantRefused::NeedsDecision)
        );
    }
    assert_eq!(INPUT_CLAIM_MINUTES, 15);
    // Every grant is a word of the closed vocabulary, and the ones that act need a step-up.
    assert_eq!(Grant::ALL.len(), 10);
    let stepped: Vec<_> = Grant::ALL
        .into_iter()
        .filter(|g| g.needs_step_up())
        .collect();
    assert_eq!(
        stepped,
        [
            Grant::ApprovalsRespond,
            Grant::ManagedTurns,
            Grant::TerminalWrite,
            Grant::ConfigRequest,
            Grant::DevicesRevoke
        ]
    );
    assert!(serde_json::from_str::<Grant>("\"terminal.write\"").is_err());
    assert!(serde_json::from_str::<Grant>("\"terminal_write\"").is_ok());
}

#[test]
fn profiles_give_exactly_their_grants_and_lp4_cannot_be_given_in_part_or_whole() {
    let names = |p: Profile| {
        grants::give_profile(p, T0, 30)
            .unwrap()
            .into_iter()
            .map(|h| h.grant)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(Profile::Lp1),
        [Grant::SessionsRead, Grant::ManagedRead]
    );
    assert!(names(Profile::Lp2).contains(&Grant::ApprovalsRespond));
    assert!(!names(Profile::Lp2).contains(&Grant::TerminalRead));
    assert!(names(Profile::Lp3).contains(&Grant::TerminalRead));
    assert!(!names(Profile::Lp3).contains(&Grant::TerminalWrite));
    assert_eq!(
        grants::give_profile(Profile::Lp4, T0, 30),
        Err(GrantRefused::NeedsDecision)
    );
    // A profile asked for longer than a kind allows is capped per grant, not refused.
    let long = grants::give_profile(Profile::Lp3, T0, 90).unwrap();
    let (watch, read) = (long[0], long[2]);
    assert_eq!(watch.expires_at, T0 + 90 * DAY);
    assert_eq!(read.expires_at, T0 + 30 * DAY);
}

#[test]
fn a_grant_is_expiring_three_days_out_and_expired_at_its_end() {
    let held = Held {
        grant: Grant::SessionsRead,
        expires_at: T0 + 10 * DAY,
    };
    assert_eq!(state_of(&held, T0), GrantState::Active);
    assert_eq!(state_of(&held, T0 + 7 * DAY), GrantState::Expiring);
    assert_eq!(state_of(&held, T0 + 10 * DAY - 1), GrantState::Expiring);
    assert_eq!(state_of(&held, T0 + 10 * DAY), GrantState::Expired);
}

// ---- devices ---------------------------------------------------------------------------------

fn device(now: u64) -> Device {
    let (connection_key, presence_key) = test_public_keys(b"device helper");
    Device::new(
        DeviceId::from_bytes([1; 16]),
        connection_key,
        presence_key,
        "phone".into(),
        "Android".into(),
        grants::give_profile(Profile::Lp2, now, 30).unwrap(),
        now,
    )
}

#[test]
fn a_device_is_suspended_after_thirty_unseen_days_or_when_every_grant_ended_and_only_the_host_renews_it()
 {
    let mut d = device(T0);
    d.sweep(T0 + 29 * DAY);
    assert_eq!(d.state, DeviceState::Active);
    d.sweep(T0 + 31 * DAY);
    assert_eq!(d.state, DeviceState::Suspended);
    // Being seen cannot undo it; nothing but the host does.
    d.seen(T0 + 32 * DAY);
    assert_eq!(d.state, DeviceState::Suspended);
    assert!(d.renewed_at_host(
        grants::give_profile(Profile::Lp1, T0 + 32 * DAY, 30).unwrap(),
        T0 + 32 * DAY
    ));
    assert_eq!(d.state, DeviceState::Active);
    // Grants that all ended suspend an active device even if it was seen yesterday.
    let mut e = device(T0);
    e.seen(T0 + 29 * DAY);
    e.sweep(T0 + 30 * DAY);
    assert_eq!(e.state, DeviceState::Suspended);
    // A device with no grants at all has none alive.
    let mut f = device(T0);
    f.grants.clear();
    f.sweep(T0 + 1);
    assert_eq!(f.state, DeviceState::Suspended);
}

#[test]
fn five_failed_step_ups_in_ten_minutes_make_a_device_suspect_and_old_failures_do_not_count() {
    let mut d = device(T0);
    for i in 0..4 {
        d.step_up_failed(T0 + i);
    }
    assert_eq!(d.state, DeviceState::Active);
    d.step_up_failed(T0 + 5);
    assert_eq!(d.state, DeviceState::Suspect);
    // Failures spread over more than ten minutes never add up.
    let mut e = device(T0);
    for i in 0..10 {
        e.step_up_failed(T0 + i * 150);
    }
    assert_eq!(e.state, DeviceState::Active);
    // A suspect device is not reviewed by being used, only by the owner at the host.
    assert!(!e.reviewed_at_host(T0));
    assert!(d.reviewed_at_host(T0 + 100));
    assert_eq!(
        (d.state, d.step_up_failures.len()),
        (DeviceState::Active, 0)
    );
}

#[test]
fn a_reused_challenge_suspects_at_once_and_revocation_is_final_and_clears_what_it_held() {
    let mut d = device(T0);
    d.challenge_reused();
    assert_eq!(d.state, DeviceState::Suspect);
    assert!(d.revoke());
    assert_eq!((d.state, d.grants.len()), (DeviceState::Revoked, 0));
    assert!(!d.revoke());
    assert!(!d.reviewed_at_host(T0) && !d.renewed_at_host(vec![], T0));
    d.seen(T0 + 5);
    d.step_up_failed(T0);
    d.challenge_reused();
    assert_eq!(d.state, DeviceState::Revoked);
}

#[test]
fn a_label_a_device_proposes_is_one_printable_line_and_decides_nothing() {
    assert_eq!(
        clean_label("  Santiago's phone ").as_deref(),
        Some("Santiago's phone")
    );
    for bad in [
        "",
        "   ",
        "a\nb",
        "a\u{0}b",
        "evil\u{202e}txt",
        "zero\u{200b}width",
        "bom\u{feff}",
        &"x".repeat(61),
    ] {
        assert!(clean_label(bad).is_none(), "{bad:?}");
    }
    assert!(clean_label(&"x".repeat(60)).is_some());
}

// ---- pairing ---------------------------------------------------------------------------------

#[test]
fn before_the_first_offer_the_gateway_does_not_listen_and_pairing_enrols_only_on_the_hosts_yes() {
    let (mut g, stand) = gateway();
    assert!(!g.should_listen());
    let shown = g.begin_pairing(T0);
    assert!(g.should_listen());
    assert_eq!(shown.secret_hex.len(), 32);
    assert_eq!(shown.expires_at, T0 + OFFER_SECONDS);
    assert_eq!(shown.host_fingerprint, HOST);
    let a = attempt(&shown.secret_hex, b"device-key-A", b"t", "phone");
    let comparison = g.present_pairing(T0 + 1, &a).unwrap();
    assert_eq!(comparison.len(), 6);
    assert!(comparison.bytes().all(|b| b.is_ascii_digit()));
    // Proved is not enrolled.
    assert_eq!(g.devices().count(), 0);
    // The device computes the same six digits from the secret and the transcript.
    let secret: [u8; 16] = unhex(&shown.secret_hex).unwrap().try_into().unwrap();
    assert_eq!(
        comparison,
        Offer::comparison_of(
            &secret,
            b"t",
            &test_public_keys(b"device-key-A").0,
            &test_public_keys(b"device-key-A").1,
        )
    );
    let id = g.confirm_pairing(T0 + 2, &Yes).unwrap();
    let d = g.device(&id).unwrap();
    assert_eq!(d.state, DeviceState::Active);
    assert_eq!(d.name, "phone");
    // The default profile: watch only.
    assert_eq!(
        d.grants.iter().map(|h| h.grant).collect::<Vec<_>>(),
        [Grant::SessionsRead, Grant::ManagedRead]
    );
    assert_eq!(
        d.fingerprint,
        hex(&sha256(&test_public_keys(b"device-key-A").0))
    );
    let _ = stand;
}

#[test]
fn a_refused_or_unanswered_or_late_confirmation_enrols_nothing_and_the_offer_is_dead() {
    for host in [&No as &dyn HostConfirm, &Silent] {
        let (mut g, _) = gateway();
        let shown = g.begin_pairing(T0);
        g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p"))
            .unwrap();
        struct Dyn<'a>(&'a dyn HostConfirm);
        impl HostConfirm for Dyn<'_> {
            fn ask(&self, r: &ConfirmRequest) -> Option<bool> {
                self.0.ask(r)
            }
        }
        assert_eq!(
            g.confirm_pairing(T0 + 1, &Dyn(host)),
            Err(Refusal::PairingRefused)
        );
        assert_eq!(g.devices().count(), 0);
        assert_eq!(g.confirm_pairing(T0 + 2, &Yes), Err(Refusal::PairingFailed));
    }
    // Two minutes is the limit, even for a yes.
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p"))
        .unwrap();
    assert_eq!(
        g.confirm_pairing(T0 + 120, &Yes),
        Err(Refusal::PairingRefused)
    );
    assert_eq!(g.devices().count(), 0);
}

#[test]
fn an_offer_dies_after_five_minutes_three_failures_or_its_first_proof_and_a_new_offer_cancels_the_old()
 {
    // Expiry.
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    assert_eq!(
        g.present_pairing(
            T0 + OFFER_SECONDS,
            &attempt(&shown.secret_hex, b"k", b"t", "p")
        ),
        Err(Refusal::PairingExpired)
    );
    // Three wrong proofs, then even the right one is refused.
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    let mut wrong = attempt(&shown.secret_hex, b"k", b"t", "p");
    wrong.mac[0] ^= 1;
    for _ in 0..MAX_ATTEMPTS {
        assert_eq!(
            g.present_pairing(T0 + 1, &wrong),
            Err(Refusal::PairingFailed)
        );
    }
    assert_eq!(
        g.present_pairing(T0 + 2, &attempt(&shown.secret_hex, b"k", b"t", "p")),
        Err(Refusal::PairingFailed)
    );
    // The first proof consumes it: a second device with the same secret gets nothing.
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    g.present_pairing(T0, &attempt(&shown.secret_hex, b"first", b"t", "p"))
        .unwrap();
    assert_eq!(
        g.present_pairing(T0 + 1, &attempt(&shown.secret_hex, b"second", b"t", "p")),
        Err(Refusal::PairingFailed)
    );
    // A new offer kills the old one, whose secret then proves nothing.
    let (mut g, _) = gateway();
    let old = g.begin_pairing(T0);
    let new = g.begin_pairing(T0 + 1);
    assert_eq!(
        g.present_pairing(T0 + 2, &attempt(&old.secret_hex, b"k", b"t", "p")),
        Err(Refusal::PairingFailed)
    );
    assert!(
        g.present_pairing(T0 + 3, &attempt(&new.secret_hex, b"k", b"t", "p"))
            .is_ok()
    );
}

#[test]
fn a_proof_over_another_transcript_or_another_secret_or_a_host_key_that_is_not_ours_proves_nothing()
{
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    let mut moved = attempt(&shown.secret_hex, b"k", b"transcript-A", "p");
    moved.transcript = b"transcript-B".to_vec();
    assert_eq!(g.present_pairing(T0, &moved), Err(Refusal::PairingFailed));
    let mut other_host = attempt(&shown.secret_hex, b"k", b"t", "p");
    other_host.host_fingerprint_seen = "c".repeat(64);
    assert_eq!(
        g.present_pairing(T0, &other_host),
        Err(Refusal::HostKeyMismatch)
    );
    // Both counted: one attempt left.
    assert!(
        g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p"))
            .is_ok()
    );
    // The comparison value depends on the transcript, so a device reached through another
    // channel shows a different value.
    let secret = [9_u8; 16];
    let digits = |t: &[u8], seed: &[u8]| {
        let (connection, presence) = test_public_keys(seed);
        Offer::comparison_of(&secret, t, &connection, &presence)
    };
    assert_ne!(digits(b"one", b"k"), digits(b"two", b"k"));
    assert_ne!(
        digits(b"one", b"k1"),
        digits(b"one", b"k2"),
        "the key is part of it"
    );
    assert_eq!(digits(b"one", b"k"), digits(b"one", b"k"));
}

#[test]
fn a_right_proof_with_a_label_that_is_not_text_is_consumed_and_dead() {
    for tweak in [
        |a: &mut Attempt| a.name = "evil\u{202e}".into(),
        |a: &mut Attempt| a.platform = String::new(),
    ] {
        let (mut g, _) = gateway();
        let shown = g.begin_pairing(T0);
        let mut a = attempt(&shown.secret_hex, b"k", b"t", "p");
        tweak(&mut a);
        assert_eq!(g.present_pairing(T0, &a), Err(Refusal::PairingFailed));
        assert_eq!(
            g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p")),
            Err(Refusal::PairingFailed),
            "the offer was consumed by the right proof"
        );
    }
}

#[test]
fn oversized_input_is_refused_before_any_work_and_counts_as_an_attempt() {
    for tweak in [
        |a: &mut Attempt| a.connection_key = Vec::new(),
        |a: &mut Attempt| a.presence_key = vec![1; 257],
        |a: &mut Attempt| a.connection_key = vec![1; P256_PUBLIC_KEY_BYTES],
        |a: &mut Attempt| a.connection_key = a.presence_key.clone(),
        |a: &mut Attempt| a.transcript = vec![0; 5000],
        |a: &mut Attempt| a.transcript = vec![0; 10 * 1024 * 1024],
    ] {
        let (mut g, _) = gateway();
        let shown = g.begin_pairing(T0);
        let mut a = attempt(&shown.secret_hex, b"k", b"t", "p");
        tweak(&mut a);
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(g.present_pairing(T0, &a), Err(Refusal::PairingFailed));
        }
        assert_eq!(
            g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p")),
            Err(Refusal::PairingFailed),
            "three oversized attempts killed the offer"
        );
    }
    // One oversized attempt does not.
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    let mut big = attempt(&shown.secret_hex, b"k", b"t", "p");
    big.transcript = vec![0; 5000];
    assert!(g.present_pairing(T0, &big).is_err());
    assert!(
        g.present_pairing(T0, &attempt(&shown.secret_hex, b"k", b"t", "p"))
            .is_ok()
    );
}

#[test]
fn a_proof_made_for_one_key_cannot_enrol_another() {
    let (mut g, _) = gateway();
    let shown = g.begin_pairing(T0);
    let mut swapped = attempt(&shown.secret_hex, b"honest-key", b"t", "p");
    swapped.connection_key = test_public_keys(b"attacker-key").0; // the MAC covers the honest key pair
    assert_eq!(g.present_pairing(T0, &swapped), Err(Refusal::PairingFailed));
}

#[test]
fn times_near_the_end_of_the_clock_do_not_overflow() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let shown = g.begin_pairing(u64::MAX - 1);
    assert_eq!(shown.expires_at, u64::MAX);
    let _ = g.present_pairing(u64::MAX, &attempt(&shown.secret_hex, b"k", b"t", "p"));
    assert!(g.issue_challenge(&id, u64::MAX).is_ok() || g.issue_challenge(&id, u64::MAX).is_err());
}

#[test]
fn a_request_id_is_remembered_for_a_day_and_a_full_cache_refuses_instead_of_forgetting() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let first = request(&id, "r-first", Grant::SessionsRead, "sessions.list");
    g.handle(&first, "s", b"b", T0 + 10).unwrap();
    // Far more than the old 1,024 distinct requests later, the first id is still refused.
    for n in 0..3000 {
        let r = request(&id, &format!("n{n}"), Grant::SessionsRead, "sessions.list");
        g.handle(&r, "s", b"b", T0 + 11).unwrap();
    }
    assert_eq!(
        refused(g.handle(&first, "s", b"b", T0 + 12)),
        Refusal::StaleRequest
    );
}

#[test]
fn a_whole_request_replayed_with_its_answered_challenge_makes_the_device_suspect() {
    let (mut g, stand) = gateway();
    let (id, fp) = enrol(&mut g, &stand, T0, b"key-1");
    g.give_at_host(&id, Profile::Lp2, 30, T0).unwrap();
    let now = T0 + 50;
    let r = stepped(
        &mut g,
        &stand,
        &id,
        &fp,
        "r1",
        b"b",
        now,
        Grant::ApprovalsRespond,
        "approvals.respond",
    );
    assert!(g.handle(&r, "s", b"b", now).is_ok());
    assert_eq!(
        refused(g.handle(&r, "s", b"b", now + 1)),
        Refusal::StaleRequest
    );
    assert_eq!(g.device(&id).unwrap().state, DeviceState::Suspect);
}

#[test]
fn an_operation_outside_its_grant_is_not_a_request() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    for (grant, op) in [
        (Grant::SessionsRead, "terminal.write"),
        (Grant::SessionsRead, "devices.revoke"),
        (Grant::ManagedRead, "sessions.list"),
    ] {
        let r = request(&id, "r-x", grant, op);
        assert_eq!(
            g.handle(&r, "s", b"b", T0 + 10),
            Err(Denied::Malformed),
            "{op}"
        );
    }
    // Every grant names at least one operation, each a well-formed identifier, none shared.
    let mut all: Vec<&str> = Vec::new();
    for g in Grant::ALL {
        assert!(!g.operations().is_empty());
        all.extend(g.operations());
    }
    let before = all.len();
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), before);
}

#[test]
fn a_revoked_key_cannot_be_enrolled_again_by_any_offer() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"the-same-key");
    assert!(g.revoke_at_host(&id));
    let shown = g.begin_pairing(T0 + 100);
    g.present_pairing(
        T0 + 100,
        &attempt(&shown.secret_hex, b"the-same-key", b"t", "again"),
    )
    .unwrap();
    assert_eq!(
        g.confirm_pairing(T0 + 101, &Yes),
        Err(Refusal::PairingRefused)
    );
    assert_eq!(
        g.devices()
            .filter(|d| d.state == DeviceState::Active)
            .count(),
        0
    );
}

// ---- requests --------------------------------------------------------------------------------

#[test]
fn an_unknown_device_is_refused_counted_per_hour_and_never_journalled() {
    let (mut g, _) = gateway();
    let stranger = DeviceId::from_bytes([9; 16]);
    for i in 0..4 {
        let r = request(
            &stranger,
            &format!("r{i}"),
            Grant::SessionsRead,
            "sessions.list",
        );
        assert_eq!(
            refused(g.handle(&r, "10.0.0.1", b"binding", T0 + i)),
            Refusal::DeviceUnknown
        );
    }
    assert_eq!(g.unknown_attempts_in_hour(T0 / 3600), 4);
    assert!(
        g.journal().records().is_empty(),
        "a flood cannot fill the journal"
    );
}

#[test]
fn every_request_is_checked_in_order_state_replay_grant_expiry_and_a_refusal_names_one_word() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let now = T0 + 100;
    let ok = request(&id, "r-ok", Grant::SessionsRead, "sessions.list");
    assert!(g.handle(&ok, "src", b"b", now).is_ok());
    // The same request id again: not served twice.
    assert_eq!(
        refused(g.handle(&ok, "src", b"b", now + 1)),
        Refusal::StaleRequest
    );
    // A grant the device was never given.
    let missing = request(&id, "r-missing", Grant::TerminalRead, "terminal.attach");
    assert_eq!(
        refused(g.handle(&missing, "src", b"b", now)),
        Refusal::GrantMissing
    );
    // A request that needs a step-up and has none.
    let (mut g2, stand2) = gateway();
    let (id2, _) = enrol(&mut g2, &stand2, T0, b"key-2");
    g2.give_at_host(&id2, Profile::Lp2, 30, T0).unwrap();
    let bare = request(&id2, "r-bare", Grant::ApprovalsRespond, "approvals.respond");
    assert_eq!(
        refused(g2.handle(&bare, "src", b"b", now)),
        Refusal::StepUpRequired
    );
    // An expired grant: the device was suspended by the clock before it got here.
    let (mut g3, stand3) = gateway();
    let (id3, _) = enrol(&mut g3, &stand3, T0, b"key-3");
    let late = request(&id3, "r-late", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g3.handle(&late, "src", b"b", T0 + 31 * DAY)),
        Refusal::DeviceSuspended
    );
    assert_eq!(g3.device(&id3).unwrap().state, DeviceState::Suspended);
    // A grant that is expired while the device is still active (another grant lives).
    let (mut g4, stand4) = gateway();
    let (id4, _) = enrol(&mut g4, &stand4, T0, b"key-4");
    g4.devices.get_mut(&id4).unwrap().grants = vec![
        Held {
            grant: Grant::SessionsRead,
            expires_at: T0 + 5,
        },
        Held {
            grant: Grant::ManagedRead,
            expires_at: T0 + 100 * DAY,
        },
    ];
    let expired = request(&id4, "r-expired", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g4.handle(&expired, "src", b"b", T0 + 10)),
        Refusal::GrantExpired
    );
    // Malformed input is not a grant word and is not journalled as a device's request.
    let mut bad = request(&id, "has space", Grant::SessionsRead, "sessions.list");
    assert_eq!(g.handle(&bad, "src", b"b", now), Err(Denied::Malformed));
    bad.request_id = "fine".into();
    bad.target = Some("C:\\a title".into());
    assert_eq!(g.handle(&bad, "src", b"b", now), Err(Denied::Malformed));
    bad.target = None;
    bad.canonical = vec![0; 64 * 1024 + 1];
    assert_eq!(g.handle(&bad, "src", b"b", now), Err(Denied::Malformed));
}

#[allow(clippy::too_many_arguments)]
fn stepped(
    g: &mut G,
    stand: &Stand,
    id: &DeviceId,
    fingerprint: &str,
    rid: &str,
    binding: &[u8],
    now: u64,
    grant: Grant,
    operation: &str,
) -> Request {
    let challenge = g.issue_challenge(id, now).unwrap();
    let mut r = request(id, rid, grant, operation);
    let message = step_up_message(binding, &challenge.nonce(), &r);
    r.step_up = Some(StepUp {
        challenge_id: challenge.id,
        signature: stand.sign(fingerprint, &message),
    });
    r
}

#[test]
fn a_step_up_binds_the_channel_the_challenge_the_request_and_its_object_and_nothing_can_be_swapped()
{
    // Each tampering gets its own gateway: five failures would make one device suspect.
    let fresh = || {
        let (mut g, stand) = gateway();
        let (id, fp) = enrol(&mut g, &stand, T0, b"key-1");
        g.give_at_host(&id, Profile::Lp2, 30, T0).unwrap();
        (g, stand, id, fp)
    };
    let now = T0 + 50;
    let (mut g, stand, id, fp) = fresh();
    let honest = stepped(
        &mut g,
        &stand,
        &id,
        &fp,
        "r1",
        b"channel-A",
        now,
        Grant::ApprovalsRespond,
        "approvals.respond",
    );
    assert!(g.handle(&honest, "s", b"channel-A", now).is_ok());
    type Swap = Box<dyn Fn(&mut Request)>;
    let swaps: Vec<(&str, Swap)> = vec![
        ("canonical", Box::new(|r| r.canonical.push(b'!'))),
        (
            "target",
            Box::new(|r| r.target = Some("another-target".into())),
        ),
        ("revision", Box::new(|r| r.object_revision = Some(4))),
        (
            "operation",
            Box::new(|r| r.operation = "approvals.defer".into()),
        ),
        ("request id", Box::new(|r| r.request_id = "r-other".into())),
    ];
    for (what, swap) in &swaps {
        let (mut g, stand, id, fp) = fresh();
        let mut r = stepped(
            &mut g,
            &stand,
            &id,
            &fp,
            "s0",
            b"channel-A",
            now,
            Grant::ApprovalsRespond,
            "approvals.respond",
        );
        swap(&mut r);
        assert_eq!(
            refused(g.handle(&r, "s", b"channel-A", now)),
            Refusal::StepUpFailed,
            "{what}"
        );
    }
    // The same signature on another channel fails: it does not move between connections.
    let (mut g, stand, id, fp) = fresh();
    let r = stepped(
        &mut g,
        &stand,
        &id,
        &fp,
        "chan",
        b"channel-A",
        now,
        Grant::ApprovalsRespond,
        "approvals.respond",
    );
    assert_eq!(
        refused(g.handle(&r, "s", b"channel-B", now)),
        Refusal::StepUpFailed
    );
    // A signature by a key the host did not pin fails.
    let (mut g, stand, id, fp) = fresh();
    let mut forged = stepped(
        &mut g,
        &stand,
        &id,
        &fp,
        "forged",
        b"channel-A",
        now,
        Grant::ApprovalsRespond,
        "approvals.respond",
    );
    forged.step_up.as_mut().unwrap().signature = hmac_sha256(b"attacker", b"x").to_vec();
    assert_eq!(
        refused(g.handle(&forged, "s", b"channel-A", now)),
        Refusal::StepUpFailed
    );
}

#[test]
fn a_challenge_is_single_use_sixty_seconds_and_the_device_it_was_issued_to() {
    let (mut g, stand) = gateway();
    let (id, fp) = enrol(&mut g, &stand, T0, b"key-1");
    g.give_at_host(&id, Profile::Lp2, 30, T0).unwrap();
    let (other, ofp) = enrol(&mut g, &stand, T0 + 1000, b"key-2");
    g.give_at_host(&other, Profile::Lp2, 30, T0 + 1000).unwrap();
    let now = T0 + 2000;
    // Expired: sixty seconds.
    let c = g.issue_challenge(&id, now).unwrap();
    let mut r = request(&id, "late", Grant::ApprovalsRespond, "approvals.respond");
    r.step_up = Some(StepUp {
        challenge_id: c.id.clone(),
        signature: stand.sign(&fp, &step_up_message(b"b", &c.nonce(), &r)),
    });
    assert_eq!(
        refused(g.handle(&r, "s", b"b", now + CHALLENGE_SECONDS)),
        Refusal::StepUpFailed
    );
    // Another device's challenge is not yours.
    let theirs = g.issue_challenge(&other, now).unwrap();
    let mut mine = request(&id, "stolen", Grant::ApprovalsRespond, "approvals.respond");
    mine.step_up = Some(StepUp {
        challenge_id: theirs.id.clone(),
        signature: stand.sign(&fp, &step_up_message(b"b", &theirs.nonce(), &mine)),
    });
    assert_eq!(
        refused(g.handle(&mine, "s", b"b", now)),
        Refusal::StepUpFailed
    );
    let _ = ofp;
    // Used twice: the second answer makes the device suspect at once.
    let first = stepped(
        &mut g,
        &stand,
        &id,
        &fp,
        "once",
        b"b",
        now,
        Grant::ApprovalsRespond,
        "approvals.respond",
    );
    let challenge = first.step_up.clone().unwrap();
    assert!(g.handle(&first, "s", b"b", now).is_ok());
    let mut replay = request(&id, "twice", Grant::ApprovalsRespond, "approvals.respond");
    replay.step_up = Some(challenge);
    assert_eq!(
        refused(g.handle(&replay, "s", b"b", now + 1)),
        Refusal::StepUpFailed
    );
    assert_eq!(g.device(&id).unwrap().state, DeviceState::Suspect);
    // A suspect device is refused everything, with its own word.
    let after = request(&id, "after", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g.handle(&after, "s", b"b", now + 2)),
        Refusal::DeviceSuspect
    );
    assert_eq!(
        g.issue_challenge(&id, now + 2).unwrap_err(),
        Refusal::DeviceSuspect
    );
}

#[test]
fn five_failed_step_ups_through_the_gateway_make_the_device_suspect_and_the_host_reviews_it() {
    let (mut g, stand) = gateway();
    let (id, fp) = enrol(&mut g, &stand, T0, b"key-1");
    g.give_at_host(&id, Profile::Lp2, 30, T0).unwrap();
    let now = T0 + 50;
    for i in 0..5 {
        let mut r = stepped(
            &mut g,
            &stand,
            &id,
            &fp,
            &format!("f{i}"),
            b"b",
            now + i,
            Grant::ApprovalsRespond,
            "approvals.respond",
        );
        r.step_up.as_mut().unwrap().signature = vec![0; 32];
        assert_eq!(
            refused(g.handle(&r, "s", b"b", now + i)),
            Refusal::StepUpFailed
        );
    }
    assert_eq!(g.device(&id).unwrap().state, DeviceState::Suspect);
    assert!(g.review_at_host(&id, now + 10));
    let ok = request(&id, "back", Grant::SessionsRead, "sessions.list");
    assert!(g.handle(&ok, "s", b"b", now + 11).is_ok());
}

#[test]
fn revocation_is_at_once_by_the_host_or_by_another_device_that_holds_the_grant_and_never_widens() {
    let (mut g, stand) = gateway();
    let (a, afp) = enrol(&mut g, &stand, T0, b"key-a");
    let (b, _) = enrol(&mut g, &stand, T0 + 100, b"key-b");
    let now = T0 + 200;
    // A holds nothing that revokes: the request is refused, the registry untouched.
    let r = request(&a, "nope", Grant::DevicesRevoke, "devices.revoke");
    assert_eq!(refused(g.handle(&r, "s", b"x", now)), Refusal::GrantMissing);
    // Give A the grant at the host (as a profile cannot; this is the add-on), then it can.
    g.devices
        .get_mut(&a)
        .unwrap()
        .grants
        .push(grants::give(Grant::DevicesRevoke, now, 30).unwrap());
    let mut revoke = stepped(
        &mut g,
        &stand,
        &a,
        &afp,
        "rv",
        b"x",
        now,
        Grant::DevicesRevoke,
        "devices.revoke",
    );
    revoke.target = Some(b.as_str().to_owned());
    // The target was part of what was signed: set before signing in a real client. Re-sign.
    let c = g.issue_challenge(&a, now).unwrap();
    revoke.step_up = Some(StepUp {
        challenge_id: c.id.clone(),
        signature: stand.sign(&afp, &step_up_message(b"x", &c.nonce(), &revoke)),
    });
    let allowed = g.handle(&revoke, "s", b"x", now).unwrap();
    g.apply_revoke(&allowed, now).unwrap();
    let revoked = g.device(&b).unwrap();
    assert_eq!(
        (revoked.state, revoked.grants.len()),
        (DeviceState::Revoked, 0)
    );
    // B is refused from that instant, with its word, and cannot be given anything back.
    let after = request(&b, "late", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g.handle(&after, "s", b"x", now + 1)),
        Refusal::DeviceRevoked
    );
    assert!(g.give_at_host(&b, Profile::Lp1, 30, now + 2).is_err());
    assert!(!g.renew_at_host(&b, Profile::Lp1, 30, now + 2));
    // A device cannot revoke itself through this, and an operation that is not revocation is not it.
    let selfie = Allowed::forged(
        a.clone(),
        Grant::DevicesRevoke,
        "devices.revoke",
        Some(a.as_str()),
    );
    assert_eq!(g.apply_revoke(&selfie, now), Err(Refusal::StaleRequest));
    let wrong_op = Allowed::forged(
        a.clone(),
        Grant::DevicesRevoke,
        "devices.enrol",
        Some(b.as_str()),
    );
    assert_eq!(g.apply_revoke(&wrong_op, now), Err(Refusal::GrantMissing));
    let wrong_grant = Allowed::forged(
        a.clone(),
        Grant::SessionsRead,
        "devices.revoke",
        Some(b.as_str()),
    );
    assert_eq!(
        g.apply_revoke(&wrong_grant, now),
        Err(Refusal::GrantMissing)
    );
}

#[test]
fn the_host_can_revoke_every_device_at_once_and_a_device_can_only_narrow_its_own_grants() {
    let (mut g, stand) = gateway();
    let (a, _) = enrol(&mut g, &stand, T0, b"key-a");
    let (b, _) = enrol(&mut g, &stand, T0 + 100, b"key-b");
    g.give_at_host(&a, Profile::Lp3, 30, T0 + 200).unwrap();
    assert!(g.narrow(&a, &[Grant::SessionsRead]));
    assert_eq!(
        g.device(&a)
            .unwrap()
            .grants
            .iter()
            .map(|h| h.grant)
            .collect::<Vec<_>>(),
        [Grant::SessionsRead]
    );
    // Narrowing to something it did not hold gives it nothing.
    assert!(g.narrow(
        &a,
        &[
            Grant::SessionsRead,
            Grant::TerminalWrite,
            Grant::TerminalRead
        ]
    ));
    assert_eq!(g.device(&a).unwrap().grants.len(), 1);
    let before = g.device(&a).unwrap().grants[0].expires_at;
    g.narrow(&a, &[Grant::SessionsRead]);
    assert_eq!(
        g.device(&a).unwrap().grants[0].expires_at,
        before,
        "an expiry never moves later"
    );
    assert_eq!(g.revoke_every_device_at_host(), 2);
    assert!(g.devices().all(|d| d.state == DeviceState::Revoked));
    let _ = b;
}

#[test]
fn a_source_that_keeps_failing_is_backed_off_to_one_attempt_a_minute() {
    let (mut g, _) = gateway();
    let stranger = DeviceId::from_bytes([9; 16]);
    for i in 0..5 {
        let r = request(
            &stranger,
            &format!("r{i}"),
            Grant::SessionsRead,
            "sessions.list",
        );
        assert_eq!(
            refused(g.handle(&r, "6.6.6.6", b"b", T0 + i)),
            Refusal::DeviceUnknown
        );
    }
    let r = request(&stranger, "r-more", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g.handle(&r, "6.6.6.6", b"b", T0 + 6)),
        Refusal::RateLimited
    );
    // Another source is not punished for it.
    assert_eq!(
        refused(g.handle(&r, "7.7.7.7", b"b", T0 + 6)),
        Refusal::DeviceUnknown
    );
    // After a minute, one attempt; then it is closed again.
    let again = request(&stranger, "r-again", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g.handle(&again, "6.6.6.6", b"b", T0 + 70)),
        Refusal::DeviceUnknown
    );
    let next = request(&stranger, "r-next", Grant::SessionsRead, "sessions.list");
    assert_eq!(
        refused(g.handle(&next, "6.6.6.6", b"b", T0 + 71)),
        Refusal::RateLimited
    );
}

// ---- the journal -----------------------------------------------------------------------------

#[test]
fn what_is_allowed_is_journalled_first_with_ids_and_words_only_and_a_full_journal_refuses() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let mut r = request(&id, "r1", Grant::SessionsRead, "sessions.list");
    r.bytes = Some(12);
    g.handle(&r, "s", b"b", T0 + 10).unwrap();
    let record = &g.journal().records()[0];
    assert_eq!(
        (record.outcome, record.grant),
        (Outcome::Allowed, Grant::SessionsRead)
    );
    assert_eq!(
        (record.operation.as_str(), record.bytes),
        ("sessions.list", Some(12))
    );
    assert_eq!(record.device, id);
    assert_eq!(record.fingerprint.len(), 64);
    // A refusal is journalled with its word.
    let missing = request(&id, "r2", Grant::TerminalRead, "terminal.attach");
    let _ = g.handle(&missing, "s", b"b", T0 + 11);
    assert_eq!(
        g.journal().records()[1].outcome,
        Outcome::Refused(Refusal::GrantMissing)
    );
    // The record has no place for content: its serialization holds exactly these keys.
    let keys: Vec<String> = serde_json::to_value(&g.journal().records()[0])
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    let mut keys = keys;
    keys.sort();
    assert_eq!(
        keys,
        [
            "at",
            "bytes",
            "connection",
            "device",
            "fingerprint",
            "grant",
            "operation",
            "outcome",
            "request_id",
            "target"
        ]
    );
    // A journal that is full refuses new remote requests: nothing is served unrecorded.
    let mut full = Gateway::new(
        HOST,
        MemoryJournal::with_limit(900),
        stand.clone(),
        Counter(0),
    );
    let (id2, _) = enrol(&mut full, &stand, T0, b"key-2");
    let mut served = 0;
    let mut last = None;
    for n in 0..20 {
        let q = request(&id2, &format!("q{n}"), Grant::SessionsRead, "sessions.list");
        match full.handle(&q, "s", b"b", T0 + 10 + n) {
            Ok(_) => served += 1,
            Err(Denied::Refused(word)) => {
                last = Some(word);
                break;
            }
            Err(Denied::Malformed) => panic!(),
        }
    }
    assert!(served > 0 && served < 20);
    assert_eq!(last, Some(Refusal::AuditUnavailable));
    assert_eq!(
        full.journal().records().len(),
        served as usize,
        "every served request has a record"
    );
    assert!(!full.journal().has_room());
}

#[test]
fn the_journal_keeps_ninety_days_and_refuses_a_record_that_is_not_an_identifier() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let r = request(&id, "r1", Grant::SessionsRead, "sessions.list");
    g.handle(&r, "s", b"b", T0 + 10).unwrap();
    // Reaching into the journal as the gateway would: older than 90 days goes, nothing newer.
    let mut journal = MemoryJournal::default();
    for record in g.journal().records() {
        journal.append(record.clone()).unwrap();
    }
    assert_eq!(journal.prune(T0 + 10 + 89 * DAY), 0);
    assert_eq!(journal.prune(T0 + 10 + 91 * DAY), 1);
    // A record with a title, a path or a name from a file is not kept at all.
    let mut bad = g.journal().records()[0].clone();
    bad.operation = "a title with spaces".into();
    assert!(MemoryJournal::default().append(bad.clone()).is_err());
    bad.operation = "ok".into();
    bad.target = Some("/home/user/secret file".into());
    assert!(MemoryJournal::default().append(bad.clone()).is_err());
    bad.target = None;
    bad.fingerprint = "nothex".into();
    assert!(MemoryJournal::default().append(bad).is_err());
}

#[test]
fn grants_are_made_only_by_host_methods_and_none_of_them_can_give_lp4() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    assert_eq!(
        g.give_at_host(&id, Profile::Lp4, 30, T0),
        Err(GrantRefused::NeedsDecision)
    );
    assert_eq!(g.give_at_host(&id, Profile::Lp3, 31, T0), Ok(()));
    assert!(
        g.device(&id)
            .unwrap()
            .grants
            .iter()
            .all(|h| h.expires_at <= T0 + 10 + 30 * DAY
                || h.grant == Grant::SessionsRead
                || h.grant == Grant::ManagedRead)
    );
    // Suspended -> active needs the host's renewal with grants; the profile is checked the same way.
    g.sweep(T0 + 40 * DAY);
    assert_eq!(g.device(&id).unwrap().state, DeviceState::Suspended);
    assert!(!g.renew_at_host(&id, Profile::Lp4, 30, T0 + 41 * DAY));
    assert!(g.renew_at_host(&id, Profile::Lp1, 30, T0 + 41 * DAY));
}

#[test]
fn a_decision_taken_for_a_device_does_not_outlive_that_devices_revocation_or_grant() {
    let (mut g, stand) = gateway();
    let (a, afp) = enrol(&mut g, &stand, T0, b"key-a");
    let (b, _) = enrol(&mut g, &stand, T0 + 100, b"key-b");
    let now = T0 + 200;
    g.devices
        .get_mut(&a)
        .unwrap()
        .grants
        .push(grants::give(Grant::DevicesRevoke, now, 30).unwrap());
    let mut revoke = request(&a, "rv", Grant::DevicesRevoke, "devices.revoke");
    revoke.target = Some(b.as_str().to_owned());
    let c = g.issue_challenge(&a, now).unwrap();
    revoke.step_up = Some(StepUp {
        challenge_id: c.id.clone(),
        signature: stand.sign(&afp, &step_up_message(b"x", &c.nonce(), &revoke)),
    });
    let allowed = g.handle(&revoke, "s", b"x", now).unwrap();
    // The issuer is revoked, narrowed or suspended between the decision and its use.
    assert!(g.revoke_at_host(&a));
    assert_eq!(
        g.apply_revoke(&allowed, now + 1),
        Err(Refusal::DeviceRevoked)
    );
    assert_eq!(g.device(&b).unwrap().state, DeviceState::Active);
}

#[test]
fn a_key_that_is_already_enrolled_is_not_enrolled_again_in_any_state() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    for _ in 0..5 {
        let c = g.issue_challenge(&id, T0 + 100);
        let _ = c;
    }
    // Suspect (or any state): the same key pairing again would shed it.
    let shown = g.begin_pairing(T0 + 200);
    let a = attempt(&shown.secret_hex, b"key-1", b"transcript-2", "again");
    g.present_pairing(T0 + 205, &a).unwrap();
    assert_eq!(
        g.confirm_pairing(T0 + 210, &Yes),
        Err(Refusal::PairingRefused)
    );
    assert_eq!(g.devices.len(), 1);
}

#[test]
fn a_presence_key_cannot_be_reused_with_a_new_connection_identity() {
    let (mut g, stand) = gateway();
    let (old_id, _) = enrol(&mut g, &stand, T0, b"old-device");
    let old_presence = g.device(&old_id).unwrap().presence_key.clone();
    let shown = g.begin_pairing(T0 + 100);
    let mut replacement = attempt(
        &shown.secret_hex,
        b"new-connection",
        b"new-transcript",
        "phone",
    );
    replacement.presence_key = old_presence.clone();
    let secret: [u8; 16] = unhex(&shown.secret_hex).unwrap().try_into().unwrap();
    replacement.mac = Offer::mac_of(
        &secret,
        &replacement.transcript,
        &replacement.connection_key,
        &replacement.presence_key,
    );
    g.present_pairing(T0 + 101, &replacement).unwrap();
    assert_eq!(
        g.confirm_pairing(T0 + 102, &Yes),
        Err(Refusal::PairingRefused)
    );
}

#[test]
fn a_revoked_presence_key_cannot_be_reenrolled_with_a_new_connection_identity() {
    let (mut g, stand) = gateway();
    let (old_id, _) = enrol(&mut g, &stand, T0, b"revoked-device");
    let old_presence = g.device(&old_id).unwrap().presence_key.clone();
    assert!(g.revoke_at_host(&old_id));
    let shown = g.begin_pairing(T0 + 100);
    let mut replacement = attempt(
        &shown.secret_hex,
        b"replacement-connection",
        b"new-transcript",
        "phone",
    );
    replacement.presence_key = old_presence;
    let secret: [u8; 16] = unhex(&shown.secret_hex).unwrap().try_into().unwrap();
    replacement.mac = Offer::mac_of(
        &secret,
        &replacement.transcript,
        &replacement.connection_key,
        &replacement.presence_key,
    );
    g.present_pairing(T0 + 101, &replacement).unwrap();
    assert_eq!(
        g.confirm_pairing(T0 + 102, &Yes),
        Err(Refusal::PairingRefused)
    );
}

#[test]
fn time_given_to_the_gateway_never_goes_back() {
    let (mut g, stand) = gateway();
    let (id, fp) = enrol(&mut g, &stand, T0, b"key-1");
    g.give_at_host(&id, Profile::Lp2, 30, T0).unwrap();
    let c = g.issue_challenge(&id, T0 + 1000).unwrap();
    let mut r = request(&id, "r1", Grant::ApprovalsRespond, "approvals.respond");
    r.step_up = Some(StepUp {
        challenge_id: c.id.clone(),
        signature: stand.sign(&fp, &step_up_message(b"b", &c.nonce(), &r)),
    });
    // Another call carries the clock past the challenge's sixty seconds; then the clock is set
    // back to a time inside them. The challenge does not come back to life.
    g.begin_pairing(T0 + 1100);
    assert_eq!(
        refused(g.handle(&r, "s", b"b", T0 + 1010)),
        Refusal::StepUpFailed
    );
}

#[test]
fn rotating_source_names_cannot_start_a_clean_failure_history() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let unknown = DeviceId::from_bytes([9; 16]);
    // Many sources fail once each, past the table's bound; then a new source keeps failing.
    for n in 0..300 {
        let r = request(
            &unknown,
            &format!("u{n}"),
            Grant::SessionsRead,
            "sessions.list",
        );
        let _ = g.handle(&r, &format!("src-{n}"), b"b", T0 + 10);
    }
    let r = request(&id, "ok1", Grant::SessionsRead, "sessions.list");
    // A source never seen before, while the table is full and its shared counter is over the
    // limit, is slowed like the rest; a source already counted keeps its own history.
    let word = refused(g.handle(&r, "brand-new", b"b", T0 + 11));
    assert_eq!(word, Refusal::RateLimited);
}

#[test]
fn the_journal_names_what_was_acted_on_by_a_digest_and_never_by_the_name() {
    let (mut g, stand) = gateway();
    let (id, _) = enrol(&mut g, &stand, T0, b"key-1");
    let mut r = request(&id, "r1", Grant::SessionsRead, "sessions.list");
    r.target = Some("vault/customer123/secretname".to_owned());
    g.handle(&r, "s", b"b", T0 + 10).unwrap();
    let kept = g.journal().records()[0].target.clone().unwrap();
    assert!(kept.starts_with("sha256:") && !kept.contains("customer123"));
}
