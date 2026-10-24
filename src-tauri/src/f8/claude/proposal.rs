//! A model's tool call, turned into the complete action the user is asked to approve.
//!
//! The model proposes; this module decides whether there is anything to ask. It refuses (the
//! model is told, the user is never asked) a proposal that is malformed, would not be shown
//! completely, is outside the workspace, names a protected path or changes nothing. What
//! survives is a `ProviderAction` for the existing approval machinery and the `Approved` payload
//! the executor will perform, both built from the same exact text.
use super::{
    diff,
    exec::{Approved, ApprovedCommand, ApprovedWrite, touches_protected},
};
use crate::{
    contracts::approval::{MAX_COMMAND_BYTES, MAX_FILE_DIFF_BYTES, MAX_PATH_BYTES},
    control::approval_action::{Leaf, confine, normal_path},
    f8::operations::ProviderAction,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, os::unix::fs::MetadataExt, path::Path};

/// Largest existing file the preview will read, and largest content a proposal may carry.
pub const MAX_CONTENT_BYTES: usize = 32 * 1024;
const MAX_EXISTING_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refuse {
    UnknownTool,
    BadArguments,
    TooLarge,
    OutOfScope,
    NotAPlainFile,
    NotText,
    NoChange,
}
impl Refuse {
    /// What the model is told. It never reveals the host's paths or file contents.
    pub const fn model_text(self) -> &'static str {
        match self {
            Self::UnknownTool => "refused: not a tool of this workspace; nothing was run",
            Self::BadArguments => "refused: the arguments are not valid; nothing was run",
            Self::TooLarge => "refused: too large to show completely for approval; nothing was run",
            Self::OutOfScope => {
                "refused: the target is outside the workspace or protected; nothing was run"
            }
            Self::NotAPlainFile => "refused: the target is not a plain file; nothing was written",
            Self::NotText => "refused: the target is not valid text; nothing was written",
            Self::NoChange => "refused: the file already has this content; nothing was written",
        }
    }
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::BadArguments => "bad_arguments",
            Self::TooLarge => "too_large",
            Self::OutOfScope => "out_of_scope",
            Self::NotAPlainFile => "not_a_plain_file",
            Self::NotText => "not_text",
            Self::NoChange => "no_change",
        }
    }
}

pub struct Prepared {
    pub action: ProviderAction,
    pub approved: Approved,
}

fn exact_keys(args: &BTreeMap<String, String>, keys: &[&str]) -> bool {
    args.len() == keys.len() && keys.iter().all(|k| args.contains_key(*k))
}

/// `workspace` is the canonical directory the runtime was attached with.
pub fn prepare(
    workspace: &str,
    tool: &str,
    args: &BTreeMap<String, String>,
    item: &str,
) -> Result<Prepared, Refuse> {
    match tool {
        "run_command" => {
            if !exact_keys(args, &["command"]) {
                return Err(Refuse::BadArguments);
            }
            let command = &args["command"];
            if command.trim().is_empty() || command.contains('\0') {
                return Err(Refuse::BadArguments);
            }
            if command.len() > MAX_COMMAND_BYTES {
                return Err(Refuse::TooLarge);
            }
            let action: ProviderAction = serde_json::from_value(
                json!({"kind":"command","command":command,"cwd":workspace,"parsed_actions":[]}),
            )
            .map_err(|_| Refuse::BadArguments)?;
            let held = action.clone().build(item).map_err(|_| Refuse::TooLarge)?;
            if !held.confined(workspace) {
                return Err(Refuse::OutOfScope);
            }
            Ok(Prepared {
                action,
                approved: Approved::Command(ApprovedCommand {
                    command: command.clone(),
                    cwd: workspace.to_owned(),
                    workspace: workspace.to_owned(),
                }),
            })
        }
        "write_file" => {
            if !exact_keys(args, &["path", "content"]) {
                return Err(Refuse::BadArguments);
            }
            let (raw, content) = (&args["path"], &args["content"]);
            if raw.is_empty() || raw.contains('\0') || content.contains('\0') {
                return Err(Refuse::BadArguments);
            }
            if content.len() > MAX_CONTENT_BYTES {
                return Err(Refuse::TooLarge);
            }
            let absolute = if raw.starts_with('/') {
                raw.clone()
            } else {
                format!("{workspace}/{raw}")
            };
            let path = normal_path(&absolute, MAX_PATH_BYTES).map_err(|_| Refuse::OutOfScope)?;
            if touches_protected(workspace, &path)
                || !confine(workspace, &path, Leaf::FileOrMissing)
            {
                return Err(Refuse::OutOfScope);
            }
            let (old, kind) = match fs::symlink_metadata(&path) {
                Ok(m) if m.is_file() && m.nlink() == 1 => {
                    if m.len() > MAX_EXISTING_BYTES {
                        return Err(Refuse::TooLarge);
                    }
                    let bytes = fs::read(&path).map_err(|_| Refuse::NotAPlainFile)?;
                    let text = String::from_utf8(bytes.clone()).map_err(|_| Refuse::NotText)?;
                    (
                        Some((text, <[u8; 32]>::from(Sha256::digest(&bytes)))),
                        "update",
                    )
                }
                Ok(_) => return Err(Refuse::NotAPlainFile),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, "add"),
                Err(_) => return Err(Refuse::NotAPlainFile),
            };
            if old.as_ref().is_some_and(|(t, _)| t == content) {
                return Err(Refuse::NoChange);
            }
            let preview = diff::unified(&path, old.as_ref().map(|(t, _)| t.as_str()), content);
            if preview.len() > MAX_FILE_DIFF_BYTES {
                return Err(Refuse::TooLarge);
            }
            let action: ProviderAction = serde_json::from_value(json!({"kind":"files","changes":[
                {"path":path,"kind":kind,"move_path":null,"diff":preview}]}))
            .map_err(|_| Refuse::BadArguments)?;
            let held = action.clone().build(item).map_err(|_| Refuse::TooLarge)?;
            if !held.confined(workspace) {
                return Err(Refuse::OutOfScope);
            }
            debug_assert!(Path::new(&path).starts_with(workspace));
            Ok(Prepared {
                action,
                approved: Approved::Write(ApprovedWrite {
                    path,
                    workspace: workspace.to_owned(),
                    content: content.clone(),
                    expected_old: old.map(|(_, h)| h),
                }),
            })
        }
        _ => Err(Refuse::UnknownTool),
    }
}

#[cfg(test)]
mod tests;
