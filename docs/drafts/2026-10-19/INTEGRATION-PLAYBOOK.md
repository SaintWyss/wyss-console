# Integration, review and reconciliation instructions for Sonnet

> **Historical draft pack.** Current scope/status/dependencies are consolidated in
> [docs/product](../../product/README.md). Proposed ADR numbers and “not built” statements
> below are dated; use the [crosswalk/resolutions](../../product/traceability.md).
> Preserve the contracts as inputs, not a second current execution roadmap.

**Source:** GitHub branch `docs/wyss-console-planning-2026-10-19`.
**Baseline:** main `916521c05cb38c292c0d8b15faee68f5bce8fb03`.
**Local development reported by owner:** `f1-f2a-configuration`, HEAD `5708c09`, with F9 implemented and T1/T2 GUI acceptance outstanding.
**No assumption:** this docs branch has not seen local commits, ADR 0029–0031, private credentials or current filesystem.

## Aim

Integrate ready-to-use design and documentation without restarting planning, breaking the development branch or pretending a proposed ADR is accepted.

## Safe integration procedure

From the actual local repository:

```bash
git status --short
git branch --show-current
git log -1 --oneline
git fetch origin docs/wyss-console-planning-2026-10-19
git diff --stat HEAD origin/docs/wyss-console-planning-2026-10-19 -- docs/drafts/2026-10-19/
```

Inspect the files first. If the working tree is clean in the target paths and the owner wants copies of these drafts into the current branch:

```bash
git restore --source origin/docs/wyss-console-planning-2026-10-19 -- docs/drafts/2026-10-19/
git diff --cached --stat
git status --short
```

This copies only documentation paths. It does **not** merge older `main` or overwrite local ADR/code by default; still verify the status before committing. If existing local drafts differ, stop and reconcile that specific file rather than blindly replace it.

## One-time reconciliation matrix

| Topic                                | Must read locally                                    | Accepted vs draft handling                                                   |
| ------------------------------------ | ---------------------------------------------------- | ---------------------------------------------------------------------------- |
| Restorable encrypted mutations / ACL | ADR 0029, F2a/F2b writer, `trusted-writers` behavior | Preserve security model and accurately describe named writer risk            |
| Native launcher/profiles             | ADR 0030, F3 test evidence                           | Update user guide/versions, leave provider-auth in providers                 |
| Host/session persistence             | ADR 0031 and F9 implementation                       | Schema + state/process owner/upgrade behavior from code, not proposal        |
| Terminal T1/T2                       | `docs/CHECKPOINT.md`, tests, GUI                     | Complete renderer search test, GUI walkthrough, ADR/README/CHANGELOG         |
| Managed hooks/Inference              | Architecture 10; current authority hooks             | E1–E8 research and security ADR before unlocking                             |
| Model projection                     | Authority API/schema and architecture 12             | No guess of actual model, DN-09 before projection                            |
| Remote service                       | Architecture 16, accepted DN-05 parts                | No graphical desktop, gateway only if new remote ADR and owner network scope |
| Secure credentials                   | ADR 0029 and key-store code                          | F14 vault is independent new security feature                                |
| Original Wyss/Hermes                 | Architecture 17 plus live provider/source evidence   | Don't mark F12 strategy selected without DN-06                               |
| Distribution                         | ADR 0027, actual package scripts and services        | No release or updater until DN-07 and user acceptance                        |

## Resolve vs leave open

**Resolve directly:** outdated docs paths, typos, aliases, duplicate test IDs, known real provider versions, implemented behavior explicitly proven by current tests, crosslinks and exact UI labels.

**Present as proposed:** schema fields not implemented, new gateway/credentials vault contract, model observation projection, Hermes/OpenCode writer, F8 concurrency strategy, architecture alternatives.

**Leave genuinely open:** irreversible data migrations, unapproved security policy changes, network listener exposure, third-party installation, public license/release and access to real API credentials.

## Documentation completion review

- [ ] Every new design includes requirement IDs and trusted-plane ownership.
- [ ] No proposal contradicts accepted local ADR or writes authority controls without approval.
- [ ] Every feature lists success, failure, rollback, security, version and UI negative states.
- [ ] No fabricated verification, install command, screenshot, signed package or API surface.
- [ ] Markdown formatting and links pass repo tooling.
- [ ] No private paths, passwords, token-like strings, provider transcript or user secret in repo.
- [ ] Canonical README/CHANGELOG/roadmap updated only from accepted verified implementation.
- [ ] Editor marks which draft supersedes or supplements which architecture section.
- [ ] Local branch continues implementation after documentation import; no new approval loop.

## Don't do

Do not push Sonnet's local code, merge docs into main, publish a release, install reference terminals, enable gateway, collect credentials, change owner ACLs or restart the authority merely to satisfy a documentation checklist.
