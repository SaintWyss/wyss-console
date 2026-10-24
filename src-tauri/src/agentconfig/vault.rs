//! Authenticated encryption of what the Console keeps to put an entry back (ADR 0034).
//!
//! A record is sealed with ChaCha20-Poly1305 under a 256-bit key that lives in the
//! operating system's credential store and nowhere else: not in the state directory, not
//! in a record, not in a log. The associated data names the kind and identifier of the
//! record, so a record cannot be opened as another one. There is no plaintext path in this
//! module: a caller that cannot have a key does not get a sealed record, and what it does
//! about that is its own decision, made in the open (`restore.rs`).
//!
//! Failures are closed words. None carries a byte of the plaintext, the key or an
//! operating-system message.
use chacha20poly1305::{
    ChaCha20Poly1305, Key as AeadKey, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
/// PBKDF2 rounds for a passphrase-sealed export (OWASP, 2023, for HMAC-SHA256).
pub const EXPORT_ROUNDS: u32 = 600_000;
const FORMAT: u32 = 1;
const ALG_KEY: &str = "chacha20poly1305";
const ALG_PASSPHRASE: &str = "pbkdf2-sha256+chacha20poly1305";

/// What a record is, bound into its associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// An entry taken out of a provider's file (F2a).
    Restoration,
    /// A restoration record sealed again under a passphrase, to move to another computer.
    Export,
    /// A provider's API key kept by the Console (F14).
    Credential,
    /// An untrusted instruction template, sealed for transfer (F5).
    InstructionTemplate,
    /// An untrusted hook or MCP-server template, sealed for transfer (F5).
    ConfigTemplate,
}

impl RecordKind {
    const fn word(self) -> &'static str {
        match self {
            Self::Restoration => "restoration",
            Self::Export => "export",
            Self::Credential => "credential",
            Self::InstructionTemplate => "instruction-template",
            Self::ConfigTemplate => "config-template",
        }
    }
}

/// The key. It has no `Debug`, no `Clone` and no `Display`, and is overwritten when it goes.
pub struct Key([u8; KEY_LEN]);

impl Key {
    fn generate() -> Result<Self, VaultError> {
        let mut bytes = [0_u8; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|_| VaultError::NoRandomness)?;
        Ok(Self(bytes))
    }
    fn from_hex(text: &str) -> Option<Self> {
        let mut bytes = Zeroizing::new([0_u8; KEY_LEN]);
        decode_hex(text.trim(), bytes.as_mut_slice())?;
        Some(Self(*bytes))
    }
    fn to_hex(&self) -> Zeroizing<String> {
        Zeroizing::new(encode_hex(&self.0))
    }
    /// A name for the key that reveals nothing of it: which key sealed a record.
    pub fn id(&self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"wyss-console/key-id/v1");
        digest.update(self.0);
        encode_hex(&digest.finalize()[..8])
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Why the key store did not give or take a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStoreError {
    /// There is no credential store to ask, or it does not answer.
    Unavailable,
    /// It is locked and was not unlocked in time.
    Locked,
    /// It answered something that is not a key.
    Malformed,
}

/// The operating system's credential store, as far as the Console needs it.
pub trait KeyStore: Send + Sync {
    /// The stored key, or `None` when the store works and holds none.
    fn load(&self) -> Result<Option<Key>, KeyStoreError>;
    fn store(&self, key: &Key) -> Result<(), KeyStoreError>;
    /// Forgets the key. Every record sealed under it becomes unreadable.
    fn forget(&self) -> Result<(), KeyStoreError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultError {
    /// No key store, or it is locked: nothing is sealed or opened.
    KeyStoreUnavailable,
    /// The record was sealed under a key this computer no longer has.
    KeyLost,
    /// The record does not authenticate: it was changed, or is another record's.
    Tampered,
    /// It is not a record of a version this build understands.
    Malformed,
    /// The operating system gave no randomness.
    NoRandomness,
    /// The passphrase is empty.
    NoPassphrase,
}

impl VaultError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyStoreUnavailable => "key_store_unavailable",
            Self::KeyLost => "key_lost",
            Self::Tampered => "record_tampered",
            Self::Malformed => "record_malformed",
            Self::NoRandomness => "no_randomness",
            Self::NoPassphrase => "no_passphrase",
        }
    }
}
impl fmt::Display for VaultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
impl std::error::Error for VaultError {}

/// A sealed record, as it is written: all text, nothing readable in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    v: u32,
    alg: String,
    /// Which key sealed it (`Key::id`), or for a passphrase export, the KDF salt.
    key_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rounds: Option<u32>,
    nonce: String,
    ct: String,
}

impl Sealed {
    pub fn key_id(&self) -> &str {
        &self.key_id
    }
}

fn aad(kind: RecordKind, id: &str, alg: &str, rounds: Option<u32>) -> Vec<u8> {
    format!(
        "wyss-console/v{FORMAT}/{}/{id}/{alg}/{}",
        kind.word(),
        rounds.unwrap_or(0)
    )
    .into_bytes()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn decode_hex(text: &str, into: &mut [u8]) -> Option<()> {
    if text.len() != into.len() * 2 || !text.is_ascii() {
        return None;
    }
    for (slot, pair) in into.iter_mut().zip(text.as_bytes().chunks(2)) {
        *slot = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(())
}
fn decode_hex_vec(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    let mut bytes = vec![0_u8; text.len() / 2];
    decode_hex(text, &mut bytes)?;
    Some(bytes)
}

fn encrypt(
    key: &[u8; KEY_LEN],
    kind: RecordKind,
    id: &str,
    alg: &str,
    rounds: Option<u32>,
    plaintext: &[u8],
) -> Result<(String, String), VaultError> {
    let mut nonce = [0_u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| VaultError::NoRandomness)?;
    let cipher = ChaCha20Poly1305::new(AeadKey::from_slice(key));
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(kind, id, alg, rounds),
            },
        )
        .map_err(|_| VaultError::Malformed)?;
    Ok((encode_hex(&nonce), encode_hex(&ct)))
}

fn decrypt(
    key: &[u8; KEY_LEN],
    kind: RecordKind,
    id: &str,
    sealed: &Sealed,
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    let mut nonce = [0_u8; NONCE_LEN];
    decode_hex(&sealed.nonce, &mut nonce).ok_or(VaultError::Malformed)?;
    let ct = decode_hex_vec(&sealed.ct).ok_or(VaultError::Malformed)?;
    let cipher = ChaCha20Poly1305::new(AeadKey::from_slice(key));
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &ct,
                aad: &aad(kind, id, &sealed.alg, sealed.rounds),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Tampered)
}

fn derive(passphrase: &str, salt: &[u8], rounds: u32) -> Zeroizing<[u8; KEY_LEN]> {
    let mut key = Zeroizing::new([0_u8; KEY_LEN]);
    pbkdf2::pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, rounds, key.as_mut_slice());
    key
}

/// The vault: seals and opens records with the key the credential store holds.
pub struct Vault {
    store: Box<dyn KeyStore>,
    /// The key is fetched once per use and not kept; this serialises first creation, so two
    /// seals at once cannot make two keys.
    creating: Mutex<()>,
}

impl Vault {
    pub fn new(store: Box<dyn KeyStore>) -> Self {
        Self {
            store,
            creating: Mutex::new(()),
        }
    }

    /// Whether a record could be sealed now: the store answers and is unlocked. It creates
    /// nothing and prompts for nothing the user did not already face.
    pub fn available(&self) -> bool {
        self.store.load().is_ok()
    }

    fn key_for_sealing(&self) -> Result<Key, VaultError> {
        let _one_at_a_time = self
            .creating
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match self.store.load() {
            Ok(Some(key)) => Ok(key),
            Ok(None) => {
                let key = Key::generate()?;
                self.store
                    .store(&key)
                    .map_err(|_| VaultError::KeyStoreUnavailable)?;
                // What was stored is what is read back, or nothing is sealed under it.
                match self.store.load() {
                    Ok(Some(back)) if back.id() == key.id() => Ok(key),
                    _ => Err(VaultError::KeyStoreUnavailable),
                }
            }
            Err(_) => Err(VaultError::KeyStoreUnavailable),
        }
    }

    pub fn seal(&self, kind: RecordKind, id: &str, plaintext: &[u8]) -> Result<Sealed, VaultError> {
        let key = self.key_for_sealing()?;
        let (nonce, ct) = encrypt(&key.0, kind, id, ALG_KEY, None, plaintext)?;
        Ok(Sealed {
            v: FORMAT,
            alg: ALG_KEY.to_owned(),
            key_id: key.id(),
            rounds: None,
            nonce,
            ct,
        })
    }

    pub fn open(
        &self,
        kind: RecordKind,
        id: &str,
        sealed: &Sealed,
    ) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        if sealed.v != FORMAT || sealed.alg != ALG_KEY {
            return Err(VaultError::Malformed);
        }
        let key = match self.store.load() {
            Ok(Some(key)) => key,
            // The store works and holds no key: whatever was sealed is gone with it.
            Ok(None) => return Err(VaultError::KeyLost),
            Err(_) => return Err(VaultError::KeyStoreUnavailable),
        };
        if key.id() != sealed.key_id {
            return Err(VaultError::KeyLost);
        }
        decrypt(&key.0, kind, id, sealed)
    }
}

/// Seals under a passphrase, for a record that leaves this computer. The passphrase is
/// stretched with a random salt; neither it nor the derived key is kept.
pub fn seal_with_passphrase(
    kind: RecordKind,
    id: &str,
    plaintext: &[u8],
    passphrase: &str,
    rounds: u32,
) -> Result<Sealed, VaultError> {
    if passphrase.is_empty() {
        return Err(VaultError::NoPassphrase);
    }
    let mut salt = [0_u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| VaultError::NoRandomness)?;
    let key = derive(passphrase, &salt, rounds);
    let (nonce, ct) = encrypt(&key, kind, id, ALG_PASSPHRASE, Some(rounds), plaintext)?;
    Ok(Sealed {
        v: FORMAT,
        alg: ALG_PASSPHRASE.to_owned(),
        key_id: encode_hex(&salt),
        rounds: Some(rounds),
        nonce,
        ct,
    })
}

pub fn open_with_passphrase(
    kind: RecordKind,
    id: &str,
    sealed: &Sealed,
    passphrase: &str,
) -> Result<Zeroizing<Vec<u8>>, VaultError> {
    if passphrase.is_empty() {
        return Err(VaultError::NoPassphrase);
    }
    if sealed.v != FORMAT || sealed.alg != ALG_PASSPHRASE {
        return Err(VaultError::Malformed);
    }
    let rounds = sealed.rounds.ok_or(VaultError::Malformed)?;
    // A file may not ask for a stretch long enough to hang the application, or for none.
    if !(1_000..=5_000_000).contains(&rounds) {
        return Err(VaultError::Malformed);
    }
    let mut salt = [0_u8; SALT_LEN];
    decode_hex(&sealed.key_id, &mut salt).ok_or(VaultError::Malformed)?;
    let key = derive(passphrase, &salt, rounds);
    decrypt(&key, kind, id, sealed)
}

/// `secret-tool` (libsecret) at a fixed absolute path: the Secret Service of KWallet or
/// GNOME Keyring. The key travels on the process's standard input and output, never on its
/// command line, and the environment it runs with is the minimum it needs to find the bus.
pub struct SecretToolStore {
    program: &'static str,
    account: String,
    timeout: Duration,
}

const SECRET_TOOL: &str = "/usr/bin/secret-tool";
const SERVICE: &str = "wyss-console";
/// Long enough for the user to type a wallet password; short enough to end.
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(60);

impl SecretToolStore {
    pub fn new() -> Self {
        Self::with_account("restoration-key-v1")
    }
    pub fn with_account(account: &str) -> Self {
        Self {
            program: SECRET_TOOL,
            account: account.to_owned(),
            timeout: UNLOCK_TIMEOUT,
        }
    }

    fn run(
        &self,
        arguments: &[&str],
        input: Option<&str>,
    ) -> Result<(Option<i32>, Zeroizing<String>, bool), KeyStoreError> {
        if !Path::new(self.program).is_file() {
            return Err(KeyStoreError::Unavailable);
        }
        let mut command = Command::new(self.program);
        command
            .args(arguments)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in [
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
            "HOME",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XDG_SESSION_TYPE",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().map_err(|_| KeyStoreError::Unavailable)?;
        // Closing the input is what tells `secret-tool store` the secret is complete.
        if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
            stdin
                .write_all(input.as_bytes())
                .map_err(|_| KeyStoreError::Unavailable)?;
        }
        let mut stdout = child.stdout.take().ok_or(KeyStoreError::Unavailable)?;
        // `secret-tool` says nothing when an item is simply not there, and says why when
        // the service cannot be reached: the two share an exit status, and only this tells
        // them apart. What it says is read to know that it said something, and dropped.
        let mut stderr = child.stderr.take().ok_or(KeyStoreError::Unavailable)?;
        let complaint = std::thread::spawn(move || {
            let mut said = Vec::new();
            let _ = stderr.by_ref().take(4096).read_to_end(&mut said);
            !said.iter().all(u8::is_ascii_whitespace)
        });
        let reader = std::thread::spawn(move || {
            let mut out = Zeroizing::new(String::new());
            // A key is 64 characters; anything longer is not read.
            let _ = stdout.by_ref().take(4096).read_to_string(&mut out);
            out
        });
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() < self.timeout => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(KeyStoreError::Locked);
                }
                Err(_) => return Err(KeyStoreError::Unavailable),
            }
        };
        let out = reader.join().map_err(|_| KeyStoreError::Unavailable)?;
        let complained = complaint.join().map_err(|_| KeyStoreError::Unavailable)?;
        Ok((status.code(), out, complained))
    }

    fn attributes(&self) -> [&str; 4] {
        ["service", SERVICE, "account", &self.account]
    }
}

impl Default for SecretToolStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyStore for SecretToolStore {
    fn load(&self) -> Result<Option<Key>, KeyStoreError> {
        let [a, b, c, d] = self.attributes();
        let (code, out, complained) = self.run(&["lookup", a, b, c, d], None)?;
        match (code, out.trim().is_empty()) {
            // Exit 1, nothing printed and nothing said: there is no such item. The same
            // exit with a complaint is a service that could not be reached.
            (Some(1), true) if !complained => Ok(None),
            (Some(0), false) => Key::from_hex(&out)
                .map(Some)
                .ok_or(KeyStoreError::Malformed),
            _ => Err(KeyStoreError::Unavailable),
        }
    }

    fn store(&self, key: &Key) -> Result<(), KeyStoreError> {
        let [a, b, c, d] = self.attributes();
        let hex = key.to_hex();
        let (code, _, _) = self.run(
            &[
                "store",
                "--label",
                "Wyss Console restoration key",
                a,
                b,
                c,
                d,
            ],
            Some(&hex),
        )?;
        if code == Some(0) {
            Ok(())
        } else {
            Err(KeyStoreError::Unavailable)
        }
    }

    fn forget(&self) -> Result<(), KeyStoreError> {
        let [a, b, c, d] = self.attributes();
        let (code, _, _) = self.run(&["clear", a, b, c, d], None)?;
        if code == Some(0) {
            Ok(())
        } else {
            Err(KeyStoreError::Unavailable)
        }
    }
}

/// A key store for tests: in memory, and able to fail the way the real one can.
#[cfg(any(test, feature = "f8-fixture"))]
pub struct MemoryStore {
    state: Mutex<MemoryState>,
}

#[cfg(any(test, feature = "f8-fixture"))]
struct MemoryState {
    key: Option<[u8; KEY_LEN]>,
    unavailable: bool,
    refuses_writes: bool,
}

#[cfg(any(test, feature = "f8-fixture"))]
impl MemoryStore {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            state: Mutex::new(MemoryState {
                key: None,
                unavailable: false,
                refuses_writes: false,
            }),
        })
    }
    pub fn set_unavailable(&self, unavailable: bool) {
        self.state.lock().unwrap().unavailable = unavailable;
    }
    pub fn set_refuses_writes(&self, refuses: bool) {
        self.state.lock().unwrap().refuses_writes = refuses;
    }
    pub fn forget_for_test(&self) {
        self.state.lock().unwrap().key = None;
    }
    pub fn has_key(&self) -> bool {
        self.state.lock().unwrap().key.is_some()
    }
    /// What the store holds, for the test that looks for it on disk. Tests only.
    pub fn key_hex(&self) -> Option<String> {
        self.state.lock().unwrap().key.map(|k| encode_hex(&k))
    }
}

#[cfg(any(test, feature = "f8-fixture"))]
impl KeyStore for std::sync::Arc<MemoryStore> {
    fn load(&self) -> Result<Option<Key>, KeyStoreError> {
        let state = self.state.lock().unwrap();
        if state.unavailable {
            return Err(KeyStoreError::Unavailable);
        }
        Ok(state.key.map(Key))
    }
    fn store(&self, key: &Key) -> Result<(), KeyStoreError> {
        let mut state = self.state.lock().unwrap();
        if state.unavailable || state.refuses_writes {
            return Err(KeyStoreError::Unavailable);
        }
        state.key = Some(key.0);
        Ok(())
    }
    fn forget(&self) -> Result<(), KeyStoreError> {
        let mut state = self.state.lock().unwrap();
        if state.unavailable {
            return Err(KeyStoreError::Unavailable);
        }
        state.key = None;
        Ok(())
    }
}
