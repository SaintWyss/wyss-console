//! Credential vault tests with an in-memory key store.
use super::vault::{MemoryStore, Vault, VaultError};

const MARKER: &[u8] = b"SEKRET-token-in-a-hook-command-0123456789";

fn vault() -> (Vault, std::sync::Arc<MemoryStore>) {
    let store = MemoryStore::new();
    (Vault::new(Box::new(store.clone())), store)
}

#[test]
fn a_sealed_record_opens_to_what_was_sealed_and_shows_none_of_it() {
    let (vault, _) = vault();
    let sealed = vault.seal("rec-1", MARKER).unwrap();
    let written = serde_json::to_string(&sealed).unwrap();
    assert!(!written.contains("SEKRET") && !written.contains("hook-command"));
    let hex_of_marker: String = MARKER.iter().map(|b| format!("{b:02x}")).collect();
    assert!(!written.contains(&hex_of_marker));
    let opened = vault.open("rec-1", &sealed).unwrap();
    assert_eq!(&opened[..], MARKER);
}

#[test]
fn the_same_text_sealed_twice_is_two_different_records() {
    let (vault, _) = vault();
    let a = vault.seal("r", MARKER).unwrap();
    let b = vault.seal("r", MARKER).unwrap();
    assert_ne!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

#[test]
fn a_changed_record_is_refused_not_read() {
    let (vault, _) = vault();
    let sealed = vault.seal("r", MARKER).unwrap();
    let mut value = serde_json::to_value(&sealed).unwrap();
    // Flip one hex digit of the ciphertext.
    let ct = value["ct"].as_str().unwrap().to_owned();
    let flipped = if ct.starts_with('0') { "1" } else { "0" };
    value["ct"] = serde_json::json!(format!("{flipped}{}", &ct[1..]));
    let damaged = serde_json::from_value(value).unwrap();
    assert_eq!(vault.open("r", &damaged).unwrap_err(), VaultError::Tampered);
}

#[test]
fn a_record_cannot_be_opened_as_another_record() {
    let (vault, _) = vault();
    let sealed = vault.seal("r1", MARKER).unwrap();
    assert_eq!(vault.open("r2", &sealed).unwrap_err(), VaultError::Tampered);
}

#[test]
fn a_record_with_extra_fields_or_another_version_is_not_understood() {
    let (vault, _) = vault();
    let sealed = vault.seal("r", b"x").unwrap();
    let mut value = serde_json::to_value(&sealed).unwrap();
    value["plaintext"] = serde_json::json!("x");
    assert!(serde_json::from_value::<super::vault::Sealed>(value).is_err());
    let mut value = serde_json::to_value(&sealed).unwrap();
    value["v"] = serde_json::json!(2);
    let newer = serde_json::from_value(value).unwrap();
    assert_eq!(vault.open("r", &newer).unwrap_err(), VaultError::Malformed);
}

#[test]
fn without_a_key_store_nothing_is_sealed_and_nothing_is_made_up() {
    let (vault, store) = vault();
    store.set_unavailable(true);
    assert_eq!(
        vault.seal("r", MARKER).unwrap_err(),
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
        vault.seal("r", MARKER).unwrap_err(),
        VaultError::KeyStoreUnavailable
    );
    assert!(!store.has_key());
}

#[test]
fn a_locked_store_does_not_open_records_and_does_not_call_them_lost() {
    let (vault, store) = vault();
    let sealed = vault.seal("r", MARKER).unwrap();
    store.set_unavailable(true);
    assert_eq!(
        vault.open("r", &sealed).unwrap_err(),
        VaultError::KeyStoreUnavailable
    );
    store.set_unavailable(false);
    assert!(vault.open("r", &sealed).is_ok());
}

#[test]
fn a_reset_store_makes_records_lost_and_says_so() {
    let (vault, store) = vault();
    let sealed = vault.seal("r", MARKER).unwrap();
    store.forget_for_test();
    assert_eq!(vault.open("r", &sealed).unwrap_err(), VaultError::KeyLost);
    // A new key is made for the next record; the old one is not recoverable.
    let again = vault.seal("r", MARKER).unwrap();
    assert_ne!(again.key_id_for_test(), sealed.key_id_for_test());
    assert_eq!(vault.open("r", &sealed).unwrap_err(), VaultError::KeyLost);
}

#[test]
fn the_key_is_in_the_store_and_in_no_record() {
    let (vault, store) = vault();
    let sealed = vault.seal("r", MARKER).unwrap();
    let key = store.key_hex().unwrap();
    assert_eq!(key.len(), 64);
    assert!(!serde_json::to_string(&sealed).unwrap().contains(&key));
    // What names the key is a digest, not the key.
    assert!(!key.contains(sealed.key_id_for_test()));
}

#[test]
fn the_key_store_answer_is_read_without_reallocating_its_buffer() {
    let full = vec![b'a'; super::vault::OUTPUT_CAPACITY - 1];
    let out = super::vault::read_output(&mut full.as_slice()).unwrap();
    assert_eq!(out.len(), super::vault::OUTPUT_CAPACITY - 1);
    assert_eq!(out.capacity(), super::vault::OUTPUT_CAPACITY);
    let long = vec![b'a'; 10_000];
    assert!(super::vault::read_output(&mut long.as_slice()).is_err());
    let key = "ab".repeat(32);
    let out = super::vault::read_output(&mut key.as_bytes()).unwrap();
    assert_eq!(
        (out.as_str(), out.capacity()),
        (key.as_str(), super::vault::OUTPUT_CAPACITY)
    );
}

#[test]
fn a_key_store_answer_with_anything_more_is_refused() {
    use super::vault::{KeyStoreError, OUTPUT_CAPACITY};
    let key = "ab".repeat(32);
    // A key, padding to the capacity and then more: never cut down to the key.
    let mut long = key.clone().into_bytes();
    long.resize(OUTPUT_CAPACITY + 10, b' ');
    assert_eq!(
        super::vault::read_output(&mut long.as_slice()).err(),
        Some(KeyStoreError::Malformed)
    );
    // Not text.
    let mut bad = key.clone().into_bytes();
    bad.push(0xff);
    assert!(super::vault::read_output(&mut bad.as_slice()).is_err());
    // A reader that fails part way.
    struct Failing(bool);
    impl std::io::Read for Failing {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if std::mem::replace(&mut self.0, true) {
                return Err(std::io::Error::other("gone"));
            }
            buf[..4].copy_from_slice(b"abab");
            Ok(4)
        }
    }
    assert!(super::vault::read_output(&mut Failing(false)).is_err());
}

#[test]
fn only_a_key_and_one_line_end_are_a_key() {
    let key = "ab".repeat(32);
    assert!(super::vault::key_from_hex_for_test(&key));
    assert!(super::vault::key_from_hex_for_test(&format!("{key}\n")));
    assert!(!super::vault::key_from_hex_for_test(&format!("{key} \n")));
    assert!(!super::vault::key_from_hex_for_test(&format!("  {key}")));
    assert!(!super::vault::key_from_hex_for_test(&format!(
        "{key}\njunk"
    )));
}

#[test]
fn two_vaults_on_one_lock_make_one_key() {
    let dir = std::env::temp_dir().join(format!("wyss-vault-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = MemoryStore::new();
    store.set_slow(Some(std::time::Duration::from_millis(30)));
    let lock = dir.join(".key.lock");
    let threads: Vec<_> = (0..2)
        .map(|_| {
            let vault = Vault::new(Box::new(store.clone())).locked_by(lock.clone());
            std::thread::spawn(move || {
                vault
                    .seal("r", MARKER)
                    .map(|s| s.key_id_for_test().to_owned())
            })
        })
        .collect();
    let ids: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(ids.iter().all(Result::is_ok), "{ids:?}");
    assert_eq!(
        ids[0], ids[1],
        "both sealed under the one key the store holds"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
