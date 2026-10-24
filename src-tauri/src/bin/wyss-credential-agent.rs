//! Vault delivery helper. The browser/GUI never receives the provider key. Native consent
//! is a distinct system dialog process containing metadata only; the reply goes to the
//! verified authority parent over its inherited pipe. Real admission remains closed.
#![allow(dead_code)]
#[path = "../credentials/core.rs"]
mod credentials;
#[path = "../native/store.rs"]
mod store_io;
#[path = "../agentconfig/vault.rs"]
mod vault;
use std::{
    io,
    os::unix::process::CommandExt,
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use wyss_console::{
    contracts::control::{ControlError as E, ControlResult},
    f8::claude::handoff::{self, Delivery, Request},
};
fn native_consent(request: &Request, meta: &credentials::Meta) -> ControlResult<()> {
    let dialog = wyss_console::screen::locate::verify(
        std::path::Path::new("/usr/bin/kdialog"),
        nix::unistd::geteuid().as_raw(),
    )
    .map_err(|_| E::GateDenied)?;
    let safe = meta
        .name
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let text = format!(
        "Authorize Anthropic credential {} ({}) for runtime {} generation {} profile {}? Deleting the record blocks future starts; stop this runtime to revoke current access.",
        safe,
        meta.fingerprint,
        request.binding.runtime,
        request.binding.generation,
        request.binding.profile
    );
    let mut cmd = Command::new(dialog.exec_path());
    cmd.args(["--yesno", &text, "--title", "Wyss credential delivery"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        cmd.pre_exec(|| {
            let parent = nix::libc::getppid();
            if nix::libc::prctl(nix::libc::PR_SET_PDEATHSIG, nix::libc::SIGKILL) != 0
                || nix::libc::getppid() != parent
            {
                return Err(io::Error::other("parent ended"));
            }
            Ok(())
        });
    }
    let mut child = Owned(cmd.spawn().map_err(|_| E::GateDenied)?);
    let end = Instant::now() + Duration::from_secs(240);
    loop {
        if let Some(status) = child.0.try_wait().map_err(|_| E::GateDenied)? {
            return if status.success() {
                Ok(())
            } else {
                Err(E::GateDenied)
            };
        }
        if Instant::now() > end {
            let _ = child.0.kill();
            let _ = child.0.wait();
            return Err(E::Timeout);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn authority_parent() -> ControlResult<()> {
    use sha2::{Digest, Sha256};
    let own = std::env::current_exe().map_err(|_| E::GateDenied)?;
    let expected = own.parent().ok_or(E::GateDenied)?.join("wyss-control");
    let authority =
        wyss_console::screen::locate::verify(&expected, nix::unistd::geteuid().as_raw())
            .map_err(|_| E::GateDenied)?;
    fn hash(mut file: std::fs::File) -> ControlResult<Vec<u8>> {
        use std::io::Read;
        let size = file.metadata().map_err(|_| E::GateDenied)?.len();
        if size == 0 || size > 512 * 1024 * 1024 {
            return Err(E::GateDenied);
        }
        let mut digest = Sha256::new();
        let mut buf = [0; 65536];
        let mut total = 0;
        loop {
            let n = file.read(&mut buf).map_err(|_| E::GateDenied)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > size {
                return Err(E::GateDenied);
            }
            digest.update(&buf[..n]);
        }
        if total != size {
            return Err(E::GateDenied);
        }
        Ok(digest.finalize().to_vec())
    }
    let actual = std::fs::File::open(format!("/proc/{}/exe", nix::unistd::getppid()))
        .map_err(|_| E::GateDenied)?;
    if hash(actual)? != hash(authority.reopen().map_err(|_| E::GateDenied)?)? {
        return Err(E::GateDenied);
    }
    Ok(())
}
fn deliver() -> ControlResult<()> {
    let raw = zeroize::Zeroizing::new(handoff::read_frame(&mut io::stdin().lock())?);
    let request: Request = serde_json::from_slice(&raw).map_err(|_| E::GateDenied)?;
    request.binding.validate()?;
    if request.capability.len() != 32 {
        return Err(E::GateDenied);
    }
    let fixture = {
        #[cfg(feature = "f8-fixture")]
        {
            request.synthetic.is_some()
        }
        #[cfg(not(feature = "f8-fixture"))]
        {
            false
        }
    };
    let profile = if request.binding.profile == "claude_local_tools_v1" {
        wyss_console::adapters::claude::Profile::LocalTools
    } else {
        wyss_console::adapters::claude::Profile::Inference
    };
    if !fixture {
        wyss_console::f8::claude::admission::Admission::production(profile)
            .map_err(|_| E::GateDenied)?;
        authority_parent()?;
    }
    // Disable core files and ordinary same-UID dump access before loading vault material.
    unsafe {
        if nix::libc::prctl(nix::libc::PR_SET_DUMPABLE, 0, 0, 0, 0) != 0 {
            return Err(E::GateDenied);
        }
        let limit = nix::libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if nix::libc::setrlimit(nix::libc::RLIMIT_CORE, &limit) != 0 {
            return Err(E::GateDenied);
        }
    }
    #[cfg(feature = "f8-fixture")]
    if let Some(secret) = request.synthetic.as_ref() {
        // Synthetic process evidence cannot name an existing credential store or load a keyring.
        use std::os::unix::fs::DirBuilderExt;
        if request.store.parent() != Some(std::env::temp_dir().as_path())
            || !request
                .store
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("wyss-credential-fixture-"))
        {
            return Err(E::GateDenied);
        }
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&request.store)
            .map_err(|_| E::GateDenied)?;
        let vault = vault::Vault::new(Box::new(vault::MemoryStore::new()));
        let store = credentials::CredentialStore::open(request.store.join("sealed"), vault)
            .map_err(|_| E::GateDenied)?;
        let meta = store
            .synthetic_record(&request.binding.record, secret)
            .map_err(|_| E::GateDenied)?;
        if meta.fingerprint != request.binding.fingerprint {
            return Err(E::GateDenied);
        }
        let key = store.unseal_for_broker(&meta).map_err(|_| E::GateDenied)?;
        let frame = Delivery {
            binding: request.binding.clone(),
            mac: handoff::tag(&request.capability, &request.binding, &key)?,
            secret: key.to_string(),
            fixture_marker: Some("WYSS_CREDENTIAL_AGENT_FIXTURE".into()),
        };
        return handoff::write_frame(
            &mut io::stdout().lock(),
            &zeroize::Zeroizing::new(serde_json::to_vec(&frame).map_err(|_| E::GateDenied)?),
        );
    }
    let store = credentials::CredentialStore::open(
        request.store.clone(),
        vault::Vault::new(Box::new(vault::SecretToolStore::new())),
    )
    .map_err(|_| E::GateDenied)?;
    let meta = store
        .list()
        .into_iter()
        .find(|m| {
            m.id == request.binding.record
                && m.provider == credentials::Provider::Anthropic
                && m.fingerprint == request.binding.fingerprint
        })
        .ok_or(E::GateDenied)?;
    native_consent(&request, &meta)?;
    let key = store.unseal_for_broker(&meta).map_err(|_| E::GateDenied)?;
    let frame = Delivery {
        binding: request.binding.clone(),
        mac: handoff::tag(&request.capability, &request.binding, &key)?,
        secret: key.to_string(),
        #[cfg(feature = "f8-fixture")]
        fixture_marker: None,
    };
    handoff::write_frame(
        &mut io::stdout().lock(),
        &zeroize::Zeroizing::new(serde_json::to_vec(&frame).map_err(|_| E::GateDenied)?),
    )
}
fn main() {
    if deliver().is_err() {
        std::process::exit(2);
    }
}

struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
