# ADR 0039: Handing work from one agent to another is a reviewed brief (F11a)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19)
Date: 2026-10-19

## Decision

1. A handoff is **a new session** of the chosen agent (Claude Code or Codex) in the same folder,
   whose first message is a brief the user has read and may edit. Nothing is moved or ended;
   the screen says it is not a transfer and that the new agent has only the text and the files
   (FR-SES-05: never called lossless).
2. The host builds the brief: a heading, the folder, the profile name, a placeholder for what the
   user wants done next, the repository's `git status --short --branch` and `git diff --stat
HEAD` (bounded to 60 lines, a few seconds, no repository lock), and the last 120 non-empty
   lines of the source terminal's scrollback with escape sequences removed. Every part passes the
   configuration view's redaction rules; the text is bounded to 16 KiB.
3. The brief reaches the agent as **one word** of its command line (ADR 0035's recipes), so no
   shell reads it. It must start with `# ` so it can never be taken for an option, contain no
   NUL, and be at most 16 KiB; otherwise the start is refused (`brief_invalid`). Restarting a
   session never carries a brief.
4. The user can also save the reviewed brief to a file in the native chooser.
5. Not built: a structured package format, transcripts of the agent's own conversation (their
   storage formats are the providers'), or an automatic handoff.
