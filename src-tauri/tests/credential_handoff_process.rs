//! Synthetic sealed-store → separate agent → authority → real broker process → verified local TLS.
//! No GUI process receives the plaintext; all credentials/stores/certificates are disposable.
#![cfg(all(target_os = "linux", feature = "f8-fixture"))]
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
};
use wyss_console::{
    adapters::claude::Profile,
    f8::claude::{
        credential::{ApiKey, CredentialSource},
        handoff::{ProcessCredentialSource, configure_broker},
        worker::Identity,
    },
    screen::locate,
};
fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
#[test]
fn a_sealed_synthetic_credential_reaches_only_the_owned_broker_and_verified_tls_origin_once() {
    let helper = std::path::Path::new(env!("CARGO_BIN_EXE_wyss-credential-agent"));
    // Release helper files are immutable to other principals, as the packager enforces.
    fs::set_permissions(helper, fs::Permissions::from_mode(0o755)).unwrap();
    let verified = locate::verify(helper, nix::unistd::geteuid().as_raw()).unwrap();
    let store = std::env::temp_dir().join(format!("wyss-credential-fixture-{}", id()));
    let secret = "wyss-SYNTHETIC-handoff-only";
    let fingerprint = Sha256::digest(secret.as_bytes())[..6]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let source =
        ProcessCredentialSource::new(verified, store.clone(), id(), "ab".repeat(16), fingerprint)
            .synthetic(secret.into());
    let source = if std::env::var_os("WYSS_TEST_NATIVE_CONSENT").is_some() {
        source.with_synthetic_consent()
    } else {
        source
    };
    let identity = Identity {
        managed_session: id(),
        runtime_instance: id(),
        generation: 1,
    };
    assert!(source.key().is_err());
    let key = source
        .key_for(&identity, Profile::Inference)
        .unwrap()
        .unwrap();
    assert_eq!(key, ApiKey::new(secret.into()).unwrap());
    assert!(source.key_for(&identity, Profile::Inference).is_err());
    for item in fs::read_dir(store.join("sealed")).unwrap() {
        assert!(
            !fs::read_to_string(item.unwrap().path())
                .unwrap()
                .contains(secret)
        );
    }
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/credential_broker_tls.py"
    );
    let mut child = Command::new("/usr/bin/python3")
        .args(["-I", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let broker_dir = store.join("broker");
    fs::create_dir(&broker_dir).unwrap();
    fs::set_permissions(&broker_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let mut stdin = child.stdin.take().unwrap();
    configure_broker(
        &key,
        &mut stdin,
        &broker_dir,
        &"12".repeat(32),
        Profile::Inference,
    )
    .unwrap();
    drop(stdin);
    drop(key);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"broker_tls_delivery_passed\n");
    fs::remove_dir_all(store).unwrap();
}
