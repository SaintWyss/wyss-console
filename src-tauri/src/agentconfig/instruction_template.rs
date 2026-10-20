//! Portable instruction text, never a destination or a capability. Imports are reviewed
//! by the native editor and become ordinary, separately confirmed document-edit plans.
//! Always passphrase-sealed in a domain distinct from restoration records/API credentials.
use super::document::{MOST_BYTES, Target, valid_text};
use super::vault::{RecordKind, Sealed, open_with_passphrase, seal_with_passphrase};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
}

const FORMAT: &str = "wyss-instruction-template";
const VERSION: u32 = 1;
const MOST_EXPORT_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Skill,
    Rule,
}

fn kind(target: &Target) -> Result<Kind, Error> {
    target.check().map_err(|_| Error::Invalid)?;
    Ok(if target.owns_folder() {
        Kind::Skill
    } else {
        Kind::Rule
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    version: u32,
    kind: Kind,
    text: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    id: String,
    sealed: Sealed,
}

pub fn export(
    target: &Target,
    text: &str,
    passphrase: &str,
    rounds: u32,
) -> Result<Vec<u8>, Error> {
    if !valid_text(text) {
        return Err(Error::Invalid);
    }
    let content = Content {
        version: VERSION,
        kind: kind(target)?,
        text: text.to_owned(),
    };
    let plaintext =
        zeroize::Zeroizing::new(serde_json::to_vec(&content).map_err(|_| Error::Invalid)?);
    let id = super::journal::new_id();
    let sealed = seal_with_passphrase(
        RecordKind::InstructionTemplate,
        &id,
        &plaintext,
        passphrase,
        rounds,
    )
    .map_err(|_| Error::Invalid)?;
    serde_json::to_vec(&Envelope {
        format: FORMAT.to_owned(),
        version: VERSION,
        id,
        sealed,
    })
    .map_err(|_| Error::Invalid)
}

/// The caller chooses an existing target independently of the imported file. A template
/// contains no provider, path, executable, grant or automatic apply instruction.
pub fn open(target: &Target, bytes: &[u8], passphrase: &str) -> Result<String, Error> {
    if bytes.len() > MOST_EXPORT_BYTES {
        return Err(Error::Invalid);
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
    if envelope.format != FORMAT
        || envelope.version != VERSION
        || envelope.id.len() != 32
        || !envelope
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Invalid);
    }
    let plaintext = open_with_passphrase(
        RecordKind::InstructionTemplate,
        &envelope.id,
        &envelope.sealed,
        passphrase,
    )
    .map_err(|_| Error::Invalid)?;
    // Serialized escapes can make text larger than its UTF-8 length. The input limit
    // bounds decryption; the text limit is checked again after decoding.
    if plaintext.len() > MOST_BYTES * 6 + 1024 {
        return Err(Error::Invalid);
    }
    let content: Content = serde_json::from_slice(&plaintext).map_err(|_| Error::Invalid)?;
    if content.version != VERSION || content.kind != kind(target)? || !valid_text(&content.text) {
        return Err(Error::Invalid);
    }
    Ok(content.text)
}

#[cfg(test)]
mod tests {
    use super::super::inventory::SourceKey;
    use super::*;
    fn rule() -> Target {
        Target {
            source: SourceKey::ClaudeUserRules,
            name: "rules/review.md".into(),
        }
    }

    #[test]
    fn templates_preserve_bytes_and_are_bound_to_their_kind_and_encryption_domain() {
        let text = "# café\r\nSEKRET-template-content\r\n";
        let bytes = export(&rule(), text, "test passphrase", 1000).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("SEKRET"));
        assert_eq!(open(&rule(), &bytes, "test passphrase").unwrap(), text);
        let codex = Target {
            source: SourceKey::CodexUserRules,
            name: "AGENTS.md".into(),
        };
        assert_eq!(open(&codex, &bytes, "test passphrase").unwrap(), text);
        let skill = Target {
            source: SourceKey::ClaudeUserSkills,
            name: "review".into(),
        };
        assert!(open(&skill, &bytes, "test passphrase").is_err());
        assert!(open(&rule(), &bytes, "wrong passphrase").is_err());
        let envelope: Envelope = serde_json::from_slice(&bytes).unwrap();
        for domain in [
            RecordKind::Restoration,
            RecordKind::Export,
            RecordKind::Credential,
        ] {
            assert!(
                open_with_passphrase(domain, &envelope.id, &envelope.sealed, "test passphrase")
                    .is_err()
            );
        }
    }

    #[test]
    fn imports_reject_unknown_schemas_destinations_tampering_and_oversized_content() {
        let bytes = export(&rule(), "instructions", "passphrase", 1000).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (key, new) in [
            ("version", serde_json::json!(2)),
            ("destination", serde_json::json!("/etc/policy")),
            ("id", serde_json::json!("../../wrong")),
        ] {
            let mut damaged = value.clone();
            damaged[key] = new;
            assert!(
                open(
                    &rule(),
                    &serde_json::to_vec(&damaged).unwrap(),
                    "passphrase"
                )
                .is_err()
            );
        }
        let mut damaged = value.clone();
        let mut ciphertext = damaged["sealed"]["ct"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec();
        ciphertext[0] = if ciphertext[0] == b'0' { b'1' } else { b'0' };
        damaged["sealed"]["ct"] = serde_json::json!(String::from_utf8(ciphertext).unwrap());
        assert!(
            open(
                &rule(),
                &serde_json::to_vec(&damaged).unwrap(),
                "passphrase"
            )
            .is_err()
        );
        assert!(open(&rule(), &vec![b' '; MOST_EXPORT_BYTES + 1], "passphrase").is_err());
        assert!(export(&rule(), &"x".repeat(MOST_BYTES + 1), "passphrase", 1000).is_err());
        assert!(export(&rule(), "hidden\u{202e}", "passphrase", 1000).is_err());
        assert!(export(&rule(), "text", "", 1000).is_err());
    }
}
