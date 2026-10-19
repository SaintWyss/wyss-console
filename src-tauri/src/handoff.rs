//! A handoff brief (F11a, FR-SES-04, 05): what one agent's session leaves for another to start
//! from, as text the user reviews and may edit before anything starts. It is a new session's
//! first message, never a transfer: the receiving agent sees only this text (and the project's
//! files), not the other's context, and the screen says so. Secrets are redacted with the same
//! rules as the configuration view; the brief is bounded; nothing here starts anything.
use crate::agentconfig::redact;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

/// Most lines of the terminal's text that go into a brief.
pub const MAX_LINES: usize = 120;
/// Most lines of any one git answer.
const MAX_GIT_LINES: usize = 60;

/// The visible text of terminal output: escape sequences removed, carriage returns collapsed,
/// control characters other than newline and tab dropped.
pub fn plain_text(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek().copied() {
                // CSI: parameters, then a final byte @ to ~.
                Some('[') => {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                // OSC and the like: up to BEL or ST.
                Some(']' | 'P' | '_' | '^') => {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\r' => {
                // A bare carriage return rewrites the line: what follows replaces it.
                if chars.peek() == Some(&'\n') {
                    continue;
                }
                while out.chars().last().is_some_and(|l| l != '\n') {
                    out.pop();
                }
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// The last `MAX_LINES` non-empty lines of `text`, redacted.
pub fn excerpt(text: &str) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let from = lines.len().saturating_sub(MAX_LINES);
    redact::text(&lines[from..].join("\n"))
        .text
        .as_str()
        .to_owned()
}

/// What git says about a folder, bounded and redacted; empty when it is not a repository or
/// git does not answer in a few seconds.
fn git(dir: &Path, args: &[&str]) -> String {
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return String::new();
    };
    let Some(mut out) = child.stdout.take() else {
        return String::new();
    };
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut buffer = Vec::new();
        let _ = out.by_ref().take(64 * 1024).read_to_end(&mut buffer);
        let _ = send.send(buffer);
    });
    let bytes = receive
        .recv_timeout(Duration::from_secs(4))
        .unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().take(MAX_GIT_LINES).collect();
    redact::text(&lines.join("\n")).text.as_str().to_owned()
}

pub struct Source<'a> {
    pub agent: &'a str,
    pub project: &'a Path,
    pub started_at: u64,
    pub profile: Option<&'a str>,
}

/// The brief, as Markdown that starts with a heading (it is passed to an agent as one word).
pub fn brief(source: &Source<'_>, screen_text: &str) -> String {
    let status = git(source.project, &["status", "--short", "--branch"]);
    let stat = git(source.project, &["diff", "--stat", "HEAD"]);
    let mut out = format!(
        "# Handoff from {}\n\nYou are taking over work another agent was doing in this folder. \
         You have only this brief and the files: you do not have that agent's context. Check the \
         repository before relying on anything below.\n\n## Folder\n\n`{}`\n",
        source.agent,
        source.project.display()
    );
    if let Some(profile) = source.profile {
        out.push_str(&format!("\nIt ran with the profile “{profile}”.\n"));
    }
    out.push_str("\n## What I want done next\n\n(Write it here before starting.)\n");
    if !status.is_empty() {
        out.push_str(&format!("\n## Repository state\n\n```\n{status}\n```\n"));
    }
    if !stat.is_empty() {
        out.push_str(&format!("\n## Uncommitted changes\n\n```\n{stat}\n```\n"));
    }
    let tail = excerpt(screen_text);
    if !tail.is_empty() {
        out.push_str(&format!(
            "\n## The end of the other agent's screen (redacted, may be incomplete)\n\n```\n{tail}\n```\n"
        ));
    }
    let _ = source.started_at;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_sequences_and_rewritten_lines_do_not_reach_the_brief() {
        let raw =
            b"\x1b[31mred\x1b[0m text\r\nprogress 10%\rprogress 100%\r\n\x1b]0;title\x07done\x07\n";
        let text = plain_text(raw);
        assert_eq!(text, "red text\nprogress 100%\ndone\n");
        assert!(!text.contains('\u{1b}'));
    }

    #[test]
    fn only_the_end_of_a_long_screen_is_kept_and_a_secret_in_it_is_redacted() {
        let key = concat!("sk-ant-", "api03-ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789");
        let mut text = String::new();
        for n in 0..500 {
            text.push_str(&format!("line {n}\n"));
        }
        text.push_str(&format!("export ANTHROPIC_API_KEY={key}\n"));
        let tail = excerpt(&text);
        assert!(tail.lines().count() <= MAX_LINES);
        assert!(!tail.contains("line 0\n") && tail.contains("line 499"));
        assert!(!tail.contains("ABCDEFGH"), "{tail}");
    }

    #[test]
    fn a_brief_starts_with_a_heading_names_the_folder_and_says_it_is_not_a_transfer() {
        let dir = std::env::temp_dir();
        let text = brief(
            &Source {
                agent: "Claude Code",
                project: &dir,
                started_at: 0,
                profile: Some("Review"),
            },
            "hello\nworld\n",
        );
        assert!(text.starts_with("# Handoff from Claude Code"));
        assert!(text.contains("you do not have that agent's context"));
        assert!(text.contains("profile “Review”"));
        assert!(text.contains("hello\nworld"));
        assert!(text.len() < crate::native::service::MAX_BRIEF);
    }

    #[test]
    fn a_folder_that_is_a_repository_adds_its_state() {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("wyss-handoff-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let text = brief(
            &Source {
                agent: "Codex",
                project: &root,
                started_at: 0,
                profile: None,
            },
            "",
        );
        assert!(
            text.contains("## Repository state") && text.contains("a.txt"),
            "{text}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
