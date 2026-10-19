//! The vault: what it promises about secrets at rest (ADR 0029).
use super::vault::{
    KeyStore, MemoryStore, RecordKind, SecretToolStore, Vault, VaultError, open_with_passphrase,
    seal_with_passphrase,
};

const MARKER: &[u8] = b"SEKRET-token-in-a-hook-command-0123456789";

fn vault() -> (Vault, std::sync::Arc<MemoryStore>) {
    let store = MemoryStore::new();
    (Vault::new(Box::new(store.clone())), store)
}

#[test]
fn a_sealed_record_opens_to_what_was_sealed_and_shows_none_of_it() {
    let (vault, _) = vault();
    let sealed = vault
        .seal(RecordKind::Restoration, "rec-1", MARKER)
        .unwrap();
    let written = serde_json::to_string(&sealed).unwrap();
    assert!(!written.contains("SEKRET") && !written.contains("hook-command"));
    let hex_of_marker: String = MARKER.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!written.contains(&hex_of_marker));
    let opened = vault
        .open(RecordKind::Restoration, "rec-1", &sealed)
        .unwrap();
    assert_eq!(&opened[..], MARKER);
}

#[test]
fn the_same_text_sealed_twice_is_two_different_records() {
    let (vault, _) = vault();
    let a = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    let b = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    assert_ne!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn a_changed_record_is_refused_not_read() {
    let (vault, _) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    let mut value = serde_json::to_value(&sealed).unwrap();
    // Flip one hex digit of the ciphertext.
    let ct = value["ct"].as_str().unwrap().to_owned();
    let flipped = if ct.starts_with('0') { "1" } else { "0" };
    value["ct"] = serde_json::json!(format!("{flipped}{}", &ct[1..]));
    let damaged = serde_json::from_value(value).unwrap();
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r", &damaged)
            .unwrap_err(),
        VaultError::Tampered
    );
}

#[test]
fn a_record_cannot_be_opened_as_another_record_or_another_kind() {
    let (vault, _) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r1", MARKER).unwrap();
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r2", &sealed)
            .unwrap_err(),
        VaultError::Tampered
    );
    assert_eq!(
        vault.open(RecordKind::Export, "r1", &sealed).unwrap_err(),
        VaultError::Tampered
    );
}

#[test]
fn a_record_with_extra_fields_or_another_version_is_not_understood() {
    let (vault, _) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r", b"x").unwrap();
    let mut value = serde_json::to_value(&sealed).unwrap();
    value["plaintext"] = serde_json::json!("x");
    assert!(serde_json::from_value::<super::vault::Sealed>(value).is_err());
    let mut value = serde_json::to_value(&sealed).unwrap();
    value["v"] = serde_json::json!(2);
    let newer = serde_json::from_value(value).unwrap();
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r", &newer)
            .unwrap_err(),
        VaultError::Malformed
    );
}

#[test]
fn without_a_key_store_nothing_is_sealed_and_nothing_is_made_up() {
    let (vault, store) = vault();
    store.set_unavailable(true);
    assert!(!vault.available());
    assert_eq!(
        vault
            .seal(RecordKind::Restoration, "r", MARKER)
            .unwrap_err(),
        VaultError::KeyStoreUnavailable
    );
    store.set_unavailable(false);
    assert!(
        !store.has_key(),
        "a key must not be created while it cannot be kept"
    );
}

#[test]
fn a_key_that_cannot_be_kept_is_not_used() {
    let (vault, store) = vault();
    store.set_refuses_writes(true);
    assert_eq!(
        vault
            .seal(RecordKind::Restoration, "r", MARKER)
            .unwrap_err(),
        VaultError::KeyStoreUnavailable
    );
    assert!(!store.has_key());
}

#[test]
fn a_locked_store_does_not_open_records_and_does_not_call_them_lost() {
    let (vault, store) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    store.set_unavailable(true);
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r", &sealed)
            .unwrap_err(),
        VaultError::KeyStoreUnavailable
    );
    store.set_unavailable(false);
    assert!(vault.open(RecordKind::Restoration, "r", &sealed).is_ok());
}

#[test]
fn a_reset_store_makes_records_lost_and_says_so() {
    let (vault, store) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    store.forget().unwrap();
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r", &sealed)
            .unwrap_err(),
        VaultError::KeyLost
    );
    // A new key is made for the next record; the old one is not recoverable.
    let again = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    assert_ne!(again.key_id(), sealed.key_id());
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "r", &sealed)
            .unwrap_err(),
        VaultError::KeyLost
    );
}

#[test]
fn the_key_is_in_the_store_and_in_no_record() {
    let (vault, store) = vault();
    let sealed = vault.seal(RecordKind::Restoration, "r", MARKER).unwrap();
    let key = store.key_hex().unwrap();
    assert_eq!(key.len(), 64);
    assert!(!serde_json::to_string(&sealed).unwrap().contains(&key));
    // What names the key is a digest, not the key.
    assert!(!key.contains(sealed.key_id()));
}

#[test]
fn a_passphrase_export_opens_only_with_the_passphrase() {
    let sealed =
        seal_with_passphrase(RecordKind::Export, "e1", MARKER, "correct horse", 1_000).unwrap();
    let written = serde_json::to_string(&sealed).unwrap();
    assert!(!written.contains("SEKRET") && !written.contains("correct horse"));
    let opened = open_with_passphrase(RecordKind::Export, "e1", &sealed, "correct horse").unwrap();
    assert_eq!(&opened[..], MARKER);
    assert_eq!(
        open_with_passphrase(RecordKind::Export, "e1", &sealed, "wrong").unwrap_err(),
        VaultError::Tampered
    );
    assert_eq!(
        open_with_passphrase(RecordKind::Export, "e2", &sealed, "correct horse").unwrap_err(),
        VaultError::Tampered
    );
    assert_eq!(
        seal_with_passphrase(RecordKind::Export, "e1", MARKER, "", 1_000).unwrap_err(),
        VaultError::NoPassphrase
    );
}

#[test]
fn a_file_cannot_pick_the_stretch_of_its_own_passphrase() {
    let sealed = seal_with_passphrase(RecordKind::Export, "e", b"x", "p", 1_000).unwrap();
    for rounds in [0_u32, 1, 999, 5_000_001, u32::MAX] {
        let mut value = serde_json::to_value(&sealed).unwrap();
        value["rounds"] = serde_json::json!(rounds);
        let forged = serde_json::from_value(value).unwrap();
        assert_eq!(
            open_with_passphrase(RecordKind::Export, "e", &forged, "p").unwrap_err(),
            VaultError::Malformed,
            "{rounds}"
        );
    }
}

#[test]
fn the_production_stretch_is_the_recommended_one() {
    const { assert!(super::vault::EXPORT_ROUNDS >= 600_000) };
}

/// Against the real Secret Service of the machine, with an account of its own that is
/// removed at the end. By hand: it may ask for the wallet to be unlocked.
#[test]
#[ignore = "talks to the real credential store of the user running it"]
fn the_real_secret_service_keeps_and_forgets_a_key() {
    let store = SecretToolStore::with_account("test-only-delete-me");
    let vault = Vault::new(Box::new(SecretToolStore::with_account(
        "test-only-delete-me",
    )));
    let _ = store.forget();
    let sealed = vault.seal(RecordKind::Restoration, "real", MARKER).unwrap();
    let opened = vault
        .open(RecordKind::Restoration, "real", &sealed)
        .unwrap();
    assert_eq!(&opened[..], MARKER);
    store.forget().unwrap();
    assert_eq!(
        vault
            .open(RecordKind::Restoration, "real", &sealed)
            .unwrap_err(),
        VaultError::KeyLost
    );
}
