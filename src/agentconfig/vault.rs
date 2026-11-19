//! Authenticated encryption for stored provider credentials. The key lives in the
//! operating system credential store. The record identifier is authenticated with the
//! ciphertext, and errors carry no plaintext, key material, or system messages.

use chacha20poly1305::{
    ChaCha20Poly1305, Key as AeadKey, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const FORMAT: u32 = 1;
const ALG_KEY: &str = "chacha20poly1305";
const RESTORATION_ACCOUNT: &str = "restoration-key-v1";

pub(crate) fn restoration_key_lock() -> std::io::Result<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .filter(|path| path.is_absolute())
        .ok_or_else(|| std::io::Error::other("no private data directory"))?;
    let dir = base.join("wyss-console");
    crate::native::store::private_dir(&dir)?;
    Ok(dir.join(format!(".{RESTORATION_ACCOUNT}.lock")))
}

/// The key. It has no `Debug`, no `Clone` and no `Display`, and is overwritten when it goes.
pub struct Key([u8; KEY_LEN]);

impl Key {
    fn generate() -> Result<Self, VaultError> {
        // Filled in place: no loose copy of the key is left on the stack, and a failed fill
        // is overwritten by `Drop`.
        let mut key = Self([0_u8; KEY_LEN]);
        getrandom::fill(&mut key.0).map_err(|_| VaultError::NoRandomness)?;
        Ok(key)
    }
    /// Exactly 64 hex digits, with at most one line end after them: nothing else is a key.
    fn from_hex(text: &str) -> Option<Self> {
        let text = text.strip_suffix('\n').unwrap_or(text);
        let mut key = Self([0_u8; KEY_LEN]);
        decode_hex(text, &mut key.0)?;
        Some(key)
    }
    /// Written into a buffer sized once and overwritten when it goes: no temporary copy.
    fn to_hex(&self) -> Zeroizing<String> {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = Zeroizing::new(String::with_capacity(KEY_LEN * 2));
        for byte in self.0 {
            out.push(DIGITS[usize::from(byte >> 4)] as char);
            out.push(DIGITS[usize::from(byte & 0x0f)] as char);
        }
        out
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
}

impl VaultError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyStoreUnavailable => "key_store_unavailable",
            Self::KeyLost => "key_lost",
            Self::Tampered => "record_tampered",
            Self::Malformed => "record_malformed",
            Self::NoRandomness => "no_randomness",
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
    /// Which key sealed it (`Key::id`).
    key_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rounds: Option<u32>,
    nonce: String,
    ct: String,
}

#[cfg(test)]
impl Sealed {
    pub(super) fn key_id_for_test(&self) -> &str {
        &self.key_id
    }
}

fn aad(id: &str, alg: &str, rounds: Option<u32>) -> Vec<u8> {
    format!(
        "wyss-console/v{FORMAT}/credential/{id}/{alg}/{}",
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
    id: &str,
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
                aad: &aad(id, ALG_KEY, None),
            },
        )
        .map_err(|_| VaultError::Malformed)?;
    Ok((encode_hex(&nonce), encode_hex(&ct)))
}

fn decrypt(
    key: &[u8; KEY_LEN],
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
                aad: &aad(id, &sealed.alg, sealed.rounds),
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| VaultError::Tampered)
}

/// The vault: seals and opens records with the key the credential store holds.
pub struct Vault {
    store: Box<dyn KeyStore>,
    /// The key is fetched once per use and not kept; this serialises first creation, so two
    /// seals at once cannot make two keys.
    creating: Mutex<()>,
    /// A file whose lock serialises first creation between vaults and processes too.
    lock: Option<PathBuf>,
}

impl Vault {
    pub fn new(store: Box<dyn KeyStore>) -> Self {
        Self {
            store,
            creating: Mutex::new(()),
            lock: None,
        }
    }

    /// The same vault, creating its first key only while it holds the lock of `path`, a file
    /// in a private folder: two instances, in this process or another, make one key.
    pub fn locked_by(mut self, path: PathBuf) -> Self {
        self.lock = Some(path);
        self
    }

    fn key_for_sealing(&self) -> Result<Key, VaultError> {
        let _one_at_a_time = self
            .creating
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _between_instances = match &self.lock {
            Some(path) => Some(exclusive(path).map_err(|_| VaultError::KeyStoreUnavailable)?),
            None => None,
        };
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

    pub fn seal(&self, id: &str, plaintext: &[u8]) -> Result<Sealed, VaultError> {
        let key = self.key_for_sealing()?;
        let (nonce, ct) = encrypt(&key.0, id, plaintext)?;
        Ok(Sealed {
            v: FORMAT,
            alg: ALG_KEY.to_owned(),
            key_id: key.id(),
            rounds: None,
            nonce,
            ct,
        })
    }

    pub fn open(&self, id: &str, sealed: &Sealed) -> Result<Zeroizing<Vec<u8>>, VaultError> {
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
        decrypt(&key.0, id, sealed)
    }
}

/// Holds an exclusive lock on `path` (created 0600, never through a link) until dropped.
pub fn exclusive(path: &Path) -> std::io::Result<nix::fcntl::Flock<std::fs::File>> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_CLOEXEC)
        .open(path)?;
    nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusive)
        .map_err(|(_, errno)| std::io::Error::from(errno))
}

/// `secret-tool` (libsecret) at a fixed absolute path: the Secret Service of KWallet or
/// GNOME Keyring. The key travels on the process's standard input and output, never on its
/// command line, and the environment it runs with is the minimum it needs to find the bus.
pub struct SecretToolStore {
    program: &'static str,
    timeout: Duration,
}

const SECRET_TOOL: &str = "/usr/bin/secret-tool";
const SERVICE: &str = "wyss-console";
/// Long enough for the user to type a wallet password; short enough to end.
const UNLOCK_TIMEOUT: Duration = Duration::from_secs(60);

impl SecretToolStore {
    pub fn new() -> Self {
        Self {
            program: SECRET_TOOL,
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
        let reader = std::thread::spawn(move || read_output(&mut stdout));
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
        let out = reader.join().map_err(|_| KeyStoreError::Unavailable)??;
        let complained = complaint.join().map_err(|_| KeyStoreError::Unavailable)?;
        Ok((status.code(), out, complained))
    }

    fn attributes() -> [&'static str; 4] {
        // Keep the account identifier used by existing installations.
        ["service", SERVICE, "account", RESTORATION_ACCOUNT]
    }
}

/// Capacity of the buffer the key store's answer is read into.
pub(super) const OUTPUT_CAPACITY: usize = 4096;

/// What the key store printed, read into a buffer that is never reallocated: a grown buffer
/// would leave the old copy of the key in freed memory, where `Zeroizing` cannot reach it.
/// A key is 64 characters. An answer that does not fit, is not text or could not be read whole
/// is malformed: a prefix of it is never taken for the answer.
pub(super) fn read_output(reader: &mut impl Read) -> Result<Zeroizing<String>, KeyStoreError> {
    let mut out = Zeroizing::new(String::with_capacity(OUTPUT_CAPACITY));
    reader
        .take(OUTPUT_CAPACITY as u64 - 1)
        .read_to_string(&mut out)
        .map_err(|_| KeyStoreError::Malformed)?;
    let mut more = [0_u8; 1];
    loop {
        match reader.read(&mut more) {
            Ok(0) => return Ok(out),
            Ok(_) => return Err(KeyStoreError::Malformed),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(KeyStoreError::Malformed),
        }
    }
}

impl Default for SecretToolStore {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyStore for SecretToolStore {
    fn load(&self) -> Result<Option<Key>, KeyStoreError> {
        let [a, b, c, d] = Self::attributes();
        let (code, out, complained) = self.run(&["lookup", a, b, c, d], None)?;
        match (code, out.is_empty()) {
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
        let [a, b, c, d] = Self::attributes();
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
}

#[cfg(test)]
pub(super) fn key_from_hex_for_test(text: &str) -> bool {
    Key::from_hex(text).is_some()
}

/// A key store for tests: in memory, and able to fail the way the real one can.
#[cfg(any(test, feature = "test-fixtures"))]
pub struct MemoryStore {
    state: Mutex<MemoryState>,
}

#[cfg(any(test, feature = "test-fixtures"))]
struct MemoryState {
    key: Option<[u8; KEY_LEN]>,
    unavailable: bool,
    refuses_writes: bool,
    /// Each answer takes this long: two callers that do not wait for each other overlap.
    slow: Option<Duration>,
}

#[cfg(any(test, feature = "test-fixtures"))]
impl MemoryStore {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            state: Mutex::new(MemoryState {
                key: None,
                unavailable: false,
                refuses_writes: false,
                slow: None,
            }),
        })
    }
    pub fn set_unavailable(&self, unavailable: bool) {
        self.state.lock().unwrap().unavailable = unavailable;
    }
    pub fn set_refuses_writes(&self, refuses: bool) {
        self.state.lock().unwrap().refuses_writes = refuses;
    }
    pub fn set_slow(&self, slow: Option<Duration>) {
        self.state.lock().unwrap().slow = slow;
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

#[cfg(any(test, feature = "test-fixtures"))]
impl KeyStore for std::sync::Arc<MemoryStore> {
    fn load(&self) -> Result<Option<Key>, KeyStoreError> {
        let slow = self.state.lock().unwrap().slow;
        if let Some(slow) = slow {
            std::thread::sleep(slow);
        }
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
}

#[cfg(test)]
mod compatibility_tests {
    use super::{Key, KeyStore, MemoryStore, Sealed, Vault};

    #[test]
    fn opens_a_persisted_v1_credential() {
        let store = MemoryStore::new();
        store.store(&Key([42; 32])).unwrap();
        let sealed: Sealed = serde_json::from_str(
            r#"{"v":1,"alg":"chacha20poly1305","key_id":"22258ce500cf7389","nonce":"000102030405060708090a0b","ct":"edc45ce4d9024c90f6d93dd673a799ae8313e9b7badf61639463ef203a8077a37f8fcf62"}"#,
        )
        .unwrap();
        let vault = Vault::new(Box::new(store));
        assert_eq!(
            &vault.open("fixture", &sealed).unwrap()[..],
            b"synthetic credential"
        );
    }
}
