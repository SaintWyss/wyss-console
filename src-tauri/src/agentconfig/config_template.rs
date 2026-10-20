//! Portable hook and MCP-server templates (F5, ADR 0050).
//!
//! A template is what the user typed for one entry — a hook's event, matcher, command and
//! timeout, or a server's program and arguments or address — sealed under a passphrase in a
//! domain of its own. It carries no provider, no file, no name, no grant and no instruction to
//! apply itself. Opening one only fills the form of the native window that already writes such
//! an entry; what is there is untrusted, is reviewed in that window, and then goes through the
//! ordinary plan, the native confirmation and the one transaction.
use super::author::HookSpec;
use super::opencode_plan::McpSpec;
use super::vault::{RecordKind, Sealed, open_with_passphrase, seal_with_passphrase};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
}

const FORMAT: &str = "wyss-config-template";
const VERSION: u32 = 1;
const MOST_EXPORT_BYTES: usize = 64 * 1024;

/// What a template holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Template {
    Hook(HookSpec),
    Mcp(McpSpec),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Body {
    Hook {
        event: String,
        matcher: Option<String>,
        command: String,
        timeout: Option<u32>,
    },
    McpLocal {
        command: Vec<String>,
    },
    McpRemote {
        url: String,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Content {
    version: u32,
    body: Body,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    format: String,
    version: u32,
    id: String,
    sealed: Sealed,
}

impl Template {
    /// The same checks the native window applies to what the user types, so that nothing a
    /// template says is looser than what could be typed.
    fn check(&self) -> Result<(), Error> {
        match self {
            Self::Hook(spec) => spec.check().map_err(|_| Error::Invalid),
            Self::Mcp(spec) => spec.check().map_err(|_| Error::Invalid),
        }
    }

    fn body(&self) -> Body {
        match self {
            Self::Hook(spec) => Body::Hook {
                event: spec.event.clone(),
                matcher: spec.matcher.clone(),
                command: spec.command.clone(),
                timeout: spec.timeout,
            },
            Self::Mcp(McpSpec::Local { command }) => Body::McpLocal {
                command: command.clone(),
            },
            Self::Mcp(McpSpec::Remote { url }) => Body::McpRemote { url: url.clone() },
        }
    }
}

pub fn export(template: &Template, passphrase: &str, rounds: u32) -> Result<Vec<u8>, Error> {
    template.check()?;
    let content = Content {
        version: VERSION,
        body: template.body(),
    };
    let plaintext =
        zeroize::Zeroizing::new(serde_json::to_vec(&content).map_err(|_| Error::Invalid)?);
    let id = super::journal::new_id();
    let sealed = seal_with_passphrase(
        RecordKind::ConfigTemplate,
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

/// Opens a template. Whatever it holds is untrusted: it is checked as typed text is, and it
/// is only ever shown in the window that writes such an entry.
pub fn open(bytes: &[u8], passphrase: &str) -> Result<Template, Error> {
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
        RecordKind::ConfigTemplate,
        &envelope.id,
        &envelope.sealed,
        passphrase,
    )
    .map_err(|_| Error::Invalid)?;
    if plaintext.len() > MOST_EXPORT_BYTES {
        return Err(Error::Invalid);
    }
    let content: Content = serde_json::from_slice(&plaintext).map_err(|_| Error::Invalid)?;
    if content.version != VERSION {
        return Err(Error::Invalid);
    }
    let template = match content.body {
        Body::Hook {
            event,
            matcher,
            command,
            timeout,
        } => Template::Hook(HookSpec {
            event,
            matcher,
            command,
            timeout,
        }),
        Body::McpLocal { command } => Template::Mcp(McpSpec::Local { command }),
        Body::McpRemote { url } => Template::Mcp(McpSpec::Remote { url }),
    };
    template.check()?;
    Ok(template)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook() -> Template {
        Template::Hook(HookSpec {
            event: "PreToolUse".to_owned(),
            matcher: Some("Bash".to_owned()),
            command: "guard.sh --strict".to_owned(),
            timeout: Some(5),
        })
    }

    #[test]
    fn a_template_keeps_exactly_what_was_typed_and_shows_nothing_without_the_passphrase() {
        for template in [
            hook(),
            Template::Mcp(McpSpec::Local {
                command: vec!["npx".to_owned(), "-y".to_owned(), "docs-server".to_owned()],
            }),
            Template::Mcp(McpSpec::Remote {
                url: "https://example.org/mcp".to_owned(),
            }),
        ] {
            let bytes = export(&template, "test passphrase", 1000).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("guard") && !text.contains("npx") && !text.contains("example"));
            assert_eq!(open(&bytes, "test passphrase").unwrap(), template);
            assert!(open(&bytes, "wrong passphrase").is_err());
        }
    }

    #[test]
    fn a_template_is_sealed_in_its_own_domain_and_a_forged_one_is_refused() {
        let bytes = export(&hook(), "test passphrase", 1000).unwrap();
        let envelope: Envelope = serde_json::from_slice(&bytes).unwrap();
        for domain in [
            RecordKind::Restoration,
            RecordKind::Export,
            RecordKind::Credential,
            RecordKind::InstructionTemplate,
        ] {
            assert!(
                open_with_passphrase(domain, &envelope.id, &envelope.sealed, "test passphrase")
                    .is_err(),
                "{domain:?}"
            );
        }
        // An instruction template is not a hook template, and the other way round.
        let instruction = super::super::instruction_template::export(
            &super::super::document::Target::user(
                super::super::inventory::SourceKey::ClaudeUserRules,
                "CLAUDE.md",
            ),
            "text",
            "test passphrase",
            1000,
        )
        .unwrap();
        assert!(open(&instruction, "test passphrase").is_err());
        assert!(open(b"{}", "x").is_err());
        assert!(open(&vec![b' '; MOST_EXPORT_BYTES + 1], "x").is_err());
    }

    #[test]
    fn nothing_a_template_says_is_looser_than_what_could_be_typed() {
        for bad in [
            Template::Hook(HookSpec {
                event: "NotAnEvent".to_owned(),
                matcher: None,
                command: "x".to_owned(),
                timeout: None,
            }),
            Template::Hook(HookSpec {
                event: "PreToolUse".to_owned(),
                matcher: None,
                command: "a\nb".to_owned(),
                timeout: None,
            }),
            Template::Hook(HookSpec {
                event: "PreToolUse".to_owned(),
                matcher: None,
                command: "ok\u{202E}evil".to_owned(),
                timeout: None,
            }),
            Template::Hook(HookSpec {
                event: "PreToolUse".to_owned(),
                matcher: Some("\"quoted\"".to_owned()),
                command: "x".to_owned(),
                timeout: Some(0),
            }),
            Template::Mcp(McpSpec::Local { command: vec![] }),
            Template::Mcp(McpSpec::Local {
                command: vec!["x".to_owned(); 33],
            }),
            Template::Mcp(McpSpec::Remote {
                url: "https://user:pass@host.example/mcp".to_owned(),
            }),
            Template::Mcp(McpSpec::Remote {
                url: "file:///etc/passwd".to_owned(),
            }),
        ] {
            assert!(export(&bad, "p", 1000).is_err(), "{bad:?}");
        }
    }
}
