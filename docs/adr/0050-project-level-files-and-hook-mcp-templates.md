# ADR 0050: Project-level instruction files and skills, and hook/MCP templates (F5)

Status: Accepted within Santiago's 2026-10-19 product-completion authorisation
Date: 2026-10-20

Closes the F5 scope left open by ADR 0047. Both parts extend the one configuration transaction
(plan → native confirmation → lock → intent → sealed record → write → read back → outcome →
journal → recovery). There is no second mutation system.

## Selecting a project

The page cannot name a path. `configuration_select_project` opens the desktop's own folder window
(or, with `clear`, selects none); `configuration_project` answers with the folder's last name only.
Before a folder is kept, `agentconfig/project.rs` judges it as every other place is judged: from `/`,
one directory descriptor at a time, no link anywhere on the way or at the end, nobody else able to write
it or a folder above it (the owner's private group counts as nobody, as elsewhere), owned by this user.
It must also be **inside the account's home and not the home itself**, and not a provider's folder
(`~/.claude`, `~/.codex`, `~/.config/opencode`, `~/.agents/skills`) or a credential folder (`.ssh`,
`.gnupg`, `.aws`, `secrets`). Refusal is the one word `project_refused`.

`ChangeService::select_project` then rebuilds the approved roots with the project's own files, swaps
the reader, forgets the plans drafted for the previous project, and has the inventory read again. A
project outside the home (a repository on another disk) is therefore not selectable; that is a limit of
this increment, not a claim.

## Project files

Supported: Claude Code `CLAUDE.md`, `.claude/rules/*.md`, `.claude/skills/<name>/SKILL.md`; Codex and
OpenCode `AGENTS.md` (the one file the inventory reads for a project). Codex project skills and
`AGENTS.override.md` are not offered at the project level: the inventory does not read them there, and
creating a file the Console cannot then list would be a blind write.

The document `Target` now carries its project. A project source needs one and an account source must have
none; a record names it (`RecordBody.project`, absent in older records, which stay account files). A
project file's path is only computed while that project is **the selected one**, so a plan drafted for
one project, or the undo of a change made in it, can never reach into another: the plan is dropped on
selection, and an undo under a different project is `not_supported` until the first is selected again.

Creation, deletion, edit and exact undo are the ADR 0047 transaction unchanged (`RENAME_NOREPLACE`
creation, tombstone deletion, sealed records, native confirmation with `weakens_security`). A new skill or
rule may also make the project's `.claude` (and `skills`/`rules`/`<name>`), each `0700`, judged, removed
again empty and deepest first on undo; the account's provider folder is still never made. A `.claude` that
is a link, or a name that already holds anything, refuses without writing.

An independent cold review (Codex) of this change found one issue: a project change could land between
an apply's checks and its write, so a write confirmed for project A could finish after B was selected.
Applies and project changes are now serialised by one lock held for the whole apply (a change of project
waits for an apply in flight, including its native confirmation) and the write that was confirmed for a
project finishes in that project.

## Hook and MCP templates

A template is what the user typed for one entry: a hook's event, matcher, command and timeout, or a
server's program and arguments or address. It is sealed under a passphrase in a domain of its own
(`RecordKind::ConfigTemplate`, not interchangeable with restoration, export, credential or instruction
templates) and contains no provider, file, name, grant or instruction to apply itself.

- **Export**: `configuration_template_export` takes an inventory ID; the passphrase and the save file are
  asked in native windows; the page never receives content.
- **Application**: `from_template` on the existing hook/MCP authoring commands. The file and passphrase
  are asked natively, the content is checked with the very validators typed text goes through
  (`HookSpec::check`, `McpSpec::check`), and it only **fills the form of the native window** that writes
  such an entry. The user reviews and may change every field; then it is the ordinary plan, the native
  confirmation (a hook or a server runs code) and the transaction. Nothing a template says is looser than
  what could be typed, and opening one plans and writes nothing.

## Capability and tests

New exact commands: `configuration_project`, `configuration_select_project`,
`configuration_template_export` (export gets the same narrow word exemption as the instruction-template
export). Tests: creation/edit/deletion/undo for every project kind with true absence restored, folder
removal, refusal of outside/home/provider/credential/relative/dotted/missing/file/link/world-writable
folders, no write for another project, nothing replaced or written through a link, record/target
consistency, template round trips and domain separation, forged and loosened templates, the hook export
and its application through the ordinary plan, and the window controller's calls.

## Not claimed

That a provider loads, reloads or obeys a project file or a template-derived entry is **not observed**. Native project selection, path rejection, authoring, deletion, exact Undo and encrypted hook/MCP
export/import/prefill/cancellation were observed on the owner's desktop with GUI revision `2f50390`;
see [native acceptance](../acceptance/f5-native-wayland-2026-10-20.md). Confirmed application of
imported account templates remains NOT_RUN. Provider consumption remains NOT_PROVEN. Codex trust for a project (`projects.<path>.trust_level`) is read, not set.
