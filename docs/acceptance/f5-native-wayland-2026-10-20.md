# F5 native Wayland acceptance, 2026-10-20

Observed on Santiago's real Wayland desktop, normal HOME, session bus and wallet. Source
`2f503902946a4a6db0632a784fb96613ed550c36`. Native GTK dialogs and the installed WebKit GUI
were operated through the desktop accessibility tree; screenshots were captured with Spectacle
and inspected. These are agent observations, not a claim that Santiago performed the steps.
[Reproducible Spanish guide](f5-manual-wayland.md).

## GUI-only installation and preservation — PASS

The earlier GUI had already exited naturally. Before installation, inspected the host, authority,
provider processes, terminal and managed sessions. No process was stopped to make installation
possible. Built the GUI from the revision above, pinning its authority digest to the existing
installed payload. Copied the installed package and replaced only its GUI and manifest GUI digest.
The installer ran with `--keep-host`; no authority install/restart or database migration occurred.

- GUI digest: `0d9bcda50911d0e321df538d738088ecd683f0ba0c252a36df5fe2307ffd4829`.
- Unchanged packaged authority: `29beda6732022cc8b98cc2b177e43573fb914ef2e6b741539355b0640ca4bdf3`.
- New GUI PID 1852991 remains open. Host 1607394 and its fish 1607420 retained start ticks,
  parent identities and executable inode/hash. Terminal `7a3cccd7-ed49-47bc-84df-0992335ef291`
  remained running and non-native; no replacement terminal was created.
- Authority 1167285 and owned Codex 1167441 retained process identities. Authority instance
  `9185486f-6a0c-4815-a8e1-20ded7324a84`, schema 7, stream/head 76 and both closed managed
  session records were unchanged. Runtime instance `18d26de7-429c-44b5-8998-877a6eceaaa7`
  stayed ready, revision 9, Codex 0.159.0, seven cached models. All outbound counters unchanged;
  thread start/resume and turn start remained zero. Other initially inventoried terminals and
  provider processes retained start ticks/parents.
- Claude settings, Codex config and OpenCode config retained exact bytes, uid/gid and mode;
  the absent OpenCode JSON file stayed absent. No paid inference or provider MCP execution.
- All three retained package manifests/hashes verify. Previous installed GUI is now
  `~/.local/opt/wyss-console.previous`; the older package whose inode the host uses remains
  `~/.local/opt/wyss-console.retained-20261020-2f50390`.

These PIDs are historical evidence, never cleanup targets. The GUI correctly displays compatible
**update pending**. Standard installer rollback stops the host: **do not run it unattended**.
Retained packages preserve rollback material; any rollback must first account for live sessions.

## Observed native interactions

| Slice                            | Result and evidence                                                                                                                                                                                                                                                                                                                           |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Project chooser                  | PASS: native folder chooser, cancellation, A/B selection and No project.                                                                                                                                                                                                                                                                      |
| Path/trust boundary              | PASS: HOME, `.codex`, `.ssh`, `/tmp`, symlink A-link and an owned mode-0777 fixture refused; prior A retained. Exact visible error: “That folder cannot be the project: it must be your own, inside your home, with no link on the way and nobody else able to write it or the folders above it.” The fixture was restored to 0700 afterward. |
| Create instruction               | PASS: native editor → plan without write; plan cancellation and native confirmation cancellation preserve absence; confirmed `CLAUDE.md` equals `F5-A-v1` plus LF, mode 0600.                                                                                                                                                                 |
| Edit and exact Undo              | PASS: editor cancellation preserves v1; confirmed edit writes v2; Undo restores v1 bytes/hash/uid/gid/mode.                                                                                                                                                                                                                                   |
| Delete and exact Undo            | PASS: cancellation preserves file; confirmation deletes; Undo restores exact v1 metadata; Undo original creation restores absence.                                                                                                                                                                                                            |
| Other project documents          | PASS: Claude rule and valid skill, Codex AGENTS.md and OpenCode AGENTS.md each created and undone natively. Files 0600, new directories 0700; empty created directory chain removed by Undo. Skill without required front matter correctly refused `invalid_entry` without writing.                                                           |
| Switching project                | PASS: switching A→B discards pending A plan without either write. Undo of A from B refuses with “That cannot be changed from here.” A unchanged, B empty. During native confirmation project button disabled. This last observation is UI protection, not proof of a backend concurrent race.                                                 |
| Hook template                    | PASS: native double-passphrase export and file save; encrypted `wyss-config-template` envelope, mode 0600. Wrong passphrase refused; correct passphrase fills event/matcher/command/timeout exactly from the original entry; cancelled before applying.                                                                                       |
| MCP template                     | PASS: existing Codex remote MCP exported encrypted, mode 0600. Wrong passphrase refused: “That file is not a kept entry, or the passphrase is wrong.” Correct import fills the original address exactly; cancelled before applying or connecting.                                                                                             |
| Domain separation / cancellation | PASS: hook-as-MCP and MCP-as-hook rejected. Native import file picker, passphrase and export save cancellations preserve files/configuration.                                                                                                                                                                                                 |

Real private project fixtures are `~/wyss-console-f5-acceptance-20261020/A` and `B`; both ended
empty through native Undo. GUI ended with no selected project. Encrypted exports remain in the
sibling `exports` directory. Original entry fields were compared locally without recording secrets.
An owned temporary input daemon was used only for a focused MCP name field and was reaped via
the existing owned-process harness; no discovered/external PID was signalled.

## Explicit acceptance limits

- **NOT_RUN:** confirmed application of an imported hook/MCP template to account configuration.
  Import/decryption/prefill/cancel are observed; no global write was needed for these checks.
- **NOT_PROVEN:** provider loads/reloads/obeys new project instructions or template-derived entries.
  Inventory discovery and correct prefilling do not prove consumption. No hook/server executed.
- **NOT_RUN natively:** backend race with project switch during an in-flight write; the native UI
  disables switching then. Existing unit coverage and observed UI behavior are separate evidence.
- No F5 production source was changed; the existing revision was compiled for the GUI-only update. This closes the observed GUI slices, not all
  provider acceptance or the remaining confirmed-template application slice.

Local ignored evidence: `.artifacts/acceptance/f5-native-followup/` contains before/after JSON,
GUI build/install/provenance logs, six rejection records and screenshots including
`native-create-editor.png`, `native-create-confirm.png`, `project-A.png`, `rejection-current.png`
and `final-native.png`. Screenshots/passphrases/config contents are not committed.

## Regression checks

`npm run check` passed: formatting, lint, zero type diagnostics, 835 frontend tests, 15 owned-process
harness tests and production build. Vite reports the existing >500 kB bundle warning.
Rust fmt/check and selected suites passed: 637 library tests, 361 desktop-bin tests, one authority
boundary test and ten new F8 contract tests; inherited 7/12 ignored tests remain explicit. Clippy
initially refused a collapsible conditional in the new test oracle; corrected and rechecked with
all-targets warnings denied. No prohibited blanket lifecycle suite or isolated installation run.
