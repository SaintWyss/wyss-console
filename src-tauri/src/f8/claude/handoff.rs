//! One-runtime, one-use authenticated delivery over inherited pipes. No public credential
//! socket, window command, plaintext file, credential argv/environment or diagnostic value.
use super::{
    credential::{ApiKey, CredentialSource},
    worker::Identity,
};
use crate::{
    adapters::claude::Profile,
    contracts::control::{ControlError as E, ControlResult},
    screen::locate::Verified,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    io::{Read, Write},
    os::fd::{AsRawFd, BorrowedFd},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};
pub const MAX_FRAME: usize = 4096;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub authority: String,
    pub session: String,
    pub runtime: String,
    pub generation: u64,
    pub profile: String,
    pub record: String,
    pub fingerprint: String,
}
impl Binding {
    pub fn validate(&self) -> ControlResult<()> {
        for id in [&self.authority, &self.session, &self.runtime] {
            crate::f8::protocol::canonical(id)?;
        }
        if self.authority == self.session
            || self.authority == self.runtime
            || self.session == self.runtime
            || self.generation == 0
            || self.generation > i64::MAX as u64
            || !matches!(
                self.profile.as_str(),
                "claude_inference_v1" | "claude_local_tools_v1"
            )
            || !hex(&self.record, 32)
            || !hex(&self.fingerprint, 12)
        {
            return Err(E::GateDenied);
        }
        Ok(())
    }
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub binding: Binding,
    pub capability: Vec<u8>,
    pub store: PathBuf,
    #[cfg(feature = "f8-fixture")]
    pub synthetic: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delivery {
    pub binding: Binding,
    pub secret: String,
    pub mac: Vec<u8>,
    #[cfg(feature = "f8-fixture")]
    pub fixture_marker: Option<String>,
}
pub fn tag(capability: &[u8], binding: &Binding, secret: &str) -> ControlResult<Vec<u8>> {
    if capability.len() != 32 {
        return Err(E::GateDenied);
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(capability).map_err(|_| E::GateDenied)?;
    mac.update(b"wyss-credential-delivery-v1\0");
    mac.update(&serde_json::to_vec(binding).map_err(|_| E::GateDenied)?);
    mac.update(&[0]);
    mac.update(secret.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}
pub fn write_frame(out: &mut impl Write, body: &[u8]) -> ControlResult<()> {
    if body.is_empty() || body.len() > MAX_FRAME {
        return Err(E::GateDenied);
    }
    out.write_all(&(body.len() as u32).to_be_bytes())
        .and_then(|_| out.write_all(body))
        .and_then(|_| out.flush())
        .map_err(|_| E::GateDenied)
}
pub fn read_frame(input: &mut impl Read) -> ControlResult<Vec<u8>> {
    let mut head = [0; 4];
    input.read_exact(&mut head).map_err(|_| E::GateDenied)?;
    let n = u32::from_be_bytes(head) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(E::GateDenied);
    }
    let mut b = vec![0; n];
    input.read_exact(&mut b).map_err(|_| E::GateDenied)?;
    Ok(b)
}
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
/// Only an authority-owned launcher constructs a source. `key()` without a runtime is refused.
pub struct ProcessCredentialSource {
    helper: Verified,
    store: PathBuf,
    authority: String,
    record: String,
    fingerprint: String,
    used: Mutex<bool>,
    #[cfg(feature = "f8-fixture")]
    synthetic: Option<String>,
}
impl ProcessCredentialSource {
    pub fn new(
        helper: Verified,
        store: PathBuf,
        authority: String,
        record: String,
        fingerprint: String,
    ) -> Self {
        Self {
            helper,
            store,
            authority,
            record,
            fingerprint,
            used: Mutex::new(false),
            #[cfg(feature = "f8-fixture")]
            synthetic: None,
        }
    }
    #[cfg(feature = "f8-fixture")]
    pub fn synthetic(mut self, secret: String) -> Self {
        self.synthetic = Some(secret);
        self
    }
}
impl CredentialSource for ProcessCredentialSource {
    fn key(&self) -> ControlResult<Option<ApiKey>> {
        Err(E::GateDenied)
    }
    fn key_for(&self, identity: &Identity, profile: Profile) -> ControlResult<Option<ApiKey>> {
        let fixture = {
            #[cfg(feature = "f8-fixture")]
            {
                self.synthetic.is_some()
            }
            #[cfg(not(feature = "f8-fixture"))]
            {
                false
            }
        };
        if !fixture && super::admission::Admission::production(profile).is_err() {
            return Err(E::GateDenied);
        }
        let mut used = self.used.lock().map_err(|_| E::GateDenied)?;
        if *used {
            return Err(E::GateDenied);
        }
        *used = true;
        let binding = Binding {
            authority: self.authority.clone(),
            session: identity.managed_session.clone(),
            runtime: identity.runtime_instance.clone(),
            generation: identity.generation,
            profile: profile.attachment_name().into(),
            record: self.record.clone(),
            fingerprint: self.fingerprint.clone(),
        };
        binding.validate()?;
        let mut capability = vec![0; 32];
        getrandom::fill(&mut capability).map_err(|_| E::GateDenied)?;
        let req = Request {
            binding: binding.clone(),
            capability: capability.clone(),
            store: self.store.clone(),
            #[cfg(feature = "f8-fixture")]
            synthetic: self.synthetic.clone(),
        };
        let frame = zeroize::Zeroizing::new(serde_json::to_vec(&req).map_err(|_| E::GateDenied)?);
        if frame.len() > MAX_FRAME {
            return Err(E::GateDenied);
        }
        let mut command = Command::new(self.helper.exec_path());
        command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // The trusted vault/consent process needs the owner's native bus, never provider auth env.
        for name in [
            "HOME",
            "XDG_RUNTIME_DIR",
            "XDG_STATE_HOME",
            "XDG_DATA_HOME",
            "DBUS_SESSION_BUS_ADDRESS",
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
        ] {
            if let Some(v) = std::env::var_os(name) {
                command.env(name, v);
            }
        }
        command.env("PATH", "/usr/bin:/bin");
        let mut child = Owned(command.spawn().map_err(|_| E::GateDenied)?);
        let mut to = child.0.stdin.take().ok_or(E::GateDenied)?;
        write_frame(&mut to, &frame)?;
        drop(to);
        let mut from = child.0.stdout.take().ok_or(E::GateDenied)?;
        // A bounded reader so a broken consent/helper process cannot hold a start forever.
        let deadline = Instant::now() + Duration::from_secs(300);
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        let mut need = 4;
        while bytes.len() < need {
            if Instant::now() > deadline {
                return Err(E::Timeout);
            }
            let mut p = [nix::poll::PollFd::new(
                unsafe { BorrowedFd::borrow_raw(from.as_raw_fd()) },
                nix::poll::PollFlags::POLLIN,
            )];
            if nix::poll::poll(&mut p, 100u16).map_err(|_| E::GateDenied)? == 0 {
                continue;
            }
            let mut buf = [0; 1024];
            let size = (need - bytes.len()).min(buf.len());
            let n = from.read(&mut buf[..size]).map_err(|_| E::GateDenied)?;
            if n == 0 {
                return Err(E::GateDenied);
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() == 4 && need == 4 {
                let n =
                    u32::from_be_bytes(bytes[..4].try_into().map_err(|_| E::GateDenied)?) as usize;
                if n == 0 || n > MAX_FRAME {
                    return Err(E::GateDenied);
                }
                need = 4 + n;
            }
        }
        let mut delivery: Delivery =
            serde_json::from_slice(&bytes[4..]).map_err(|_| E::GateDenied)?;
        verify_delivery(&capability, &binding, &delivery)?;
        ApiKey::new(std::mem::take(&mut delivery.secret)).map(Some)
    }
}

/// Complete the existing broker's stdin configuration without returning a key-bearing DTO.
/// The destination is fixed; the only consumer is an owned child pipe.
pub fn configure_broker(
    key: &ApiKey,
    pipe: &mut std::process::ChildStdin,
    directory: &std::path::Path,
    token: &str,
    profile: Profile,
) -> ControlResult<()> {
    if !hex(token, 64) || !directory.is_absolute() {
        return Err(E::GateDenied);
    }
    let profile = if profile == Profile::Inference {
        "inference"
    } else {
        "local_tools"
    };
    let mut frame=zeroize::Zeroizing::new(serde_json::to_vec(&serde_json::json!({"v":1,"dir":directory,"token":token,"origin":"https://api.anthropic.com","profile":profile,"api_key":key.expose()})).map_err(|_|E::GateDenied)?);
    frame.push(b'\n');
    pipe.write_all(&frame)
        .and_then(|_| pipe.flush())
        .map_err(|_| E::GateDenied)
}

impl Drop for Delivery {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.secret);
    }
}
impl Drop for Request {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.capability);
        #[cfg(feature = "f8-fixture")]
        if let Some(s) = self.synthetic.as_mut() {
            zeroize::Zeroize::zeroize(s);
        }
    }
}
fn verify_delivery(capability: &[u8], binding: &Binding, delivery: &Delivery) -> ControlResult<()> {
    binding.validate()?;
    if capability.len() != 32 || delivery.binding != *binding {
        return Err(E::GateDenied);
    }
    let mut verifier = Hmac::<Sha256>::new_from_slice(capability).map_err(|_| E::GateDenied)?;
    verifier.update(b"wyss-credential-delivery-v1\0");
    verifier.update(&serde_json::to_vec(binding).map_err(|_| E::GateDenied)?);
    verifier.update(&[0]);
    verifier.update(delivery.secret.as_bytes());
    verifier
        .verify_slice(&delivery.mac)
        .map_err(|_| E::GateDenied)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn delivery_rejects_wrong_nonce_runtime_generation_profile_record_and_secret() {
        let id = || uuid::Uuid::new_v4().to_string();
        let b = Binding {
            authority: id(),
            session: id(),
            runtime: id(),
            generation: 1,
            profile: "claude_inference_v1".into(),
            record: "ab".repeat(16),
            fingerprint: "ab".repeat(6),
        };
        let cap = [13; 32];
        let mut d = Delivery {
            binding: b.clone(),
            secret: "SYNTHETIC-only".into(),
            mac: tag(&cap, &b, "SYNTHETIC-only").unwrap(),
            #[cfg(feature = "f8-fixture")]
            fixture_marker: None,
        };
        verify_delivery(&cap, &b, &d).unwrap();
        assert!(verify_delivery(&[14; 32], &b, &d).is_err());
        for field in [
            "authority",
            "session",
            "runtime",
            "generation",
            "profile",
            "record",
            "fingerprint",
        ] {
            let mut other = b.clone();
            match field {
                "authority" => other.authority = id(),
                "session" => other.session = id(),
                "runtime" => other.runtime = id(),
                "generation" => other.generation += 1,
                "profile" => other.profile = "claude_local_tools_v1".into(),
                "record" => other.record = "cd".repeat(16),
                _ => other.fingerprint = "cd".repeat(6),
            }
            assert!(verify_delivery(&cap, &other, &d).is_err(), "{field}");
        }
        d.secret.push('x');
        assert!(verify_delivery(&cap, &b, &d).is_err());
        assert!(read_frame(&mut &4097u32.to_be_bytes()[..]).is_err());
        assert!(read_frame(&mut &[0, 0, 0, 2, 1][..]).is_err());
    }
}
