# What was built, against the planning set merged from the documentation branches

> **Historical record.** For current status use the [gap register](../product/implementation-status.md)
> and [remaining roadmap](../product/roadmap.md). F8 was subsequently activated in production
> with real Codex, P2/V8, ADR0051 Accepted and cap1; P1 remains. The report below retains
> its original observations and limitations; its procedures are not new authorization.

The architecture set (`docs/architecture/`, from `docs/console-planning-review-2026-10-13` and
`docs/fx-b1-native-verification-2026-10-14`) was written before F1 to F14 were built and still
says "proposed" in places. Where the owner decided or the build differs, the accepted ADRs and
acceptance records below win. This page lists the differences so nobody has to find them.

| Topic in the planning set                                                                                             | What the owner decided / what was built                                                                                                                                                                                                                                        | Record                                                       |
| --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ |
| DN-01, the restoration store, ADR 0029 "second draft" (`0029-secrets-restoration-data-and-configuration-persistence`) | The owner chose strategy B (2026-10-19): secret-bearing entries may be kept **encrypted at rest** with a key in the Secret Service, failing closed. Built as the accepted ADR **0034**; the 0029 draft's custody options (passphrase-derived key, consent file) were not built | ADR 0034, `docs/acceptance/f1-f2a-2026-10-19.md`             |
| DN-02, starting an agent                                                                                              | Approved as recommended: the user's shell with `exec`; recipes for sh, bash, zsh, fish; direct start as a labelled fallback                                                                                                                                                    | ADR 0035, `docs/acceptance/f3-2026-10-19.md`                 |
| DN-05, host service shape HS1 (a separate user service `wyss-host`)                                                   | Built as **the same executable** started with `--wyss-host` (one file to install, same private socket rules); a systemd user unit can supervise it in F13. Not a separate binary                                                                                               | ADR 0036                                                     |
| DN-04, search add-on                                                                                                  | Own search over xterm's buffer; no dependency added                                                                                                                                                                                                                            | ADR 0037                                                     |
| Layout file (05 §15)                                                                                                  | `layout.json` as built: tabs, split trees, focus, titles, folders, an `native` flag                                                                                                                                                                                            | ADR 0037                                                     |
| F14 credentials                                                                                                       | Built as sealed records with a `credential` kind; no use of a key by a provider                                                                                                                                                                                                | ADR 0038                                                     |
| Codex skill switches (F2b)                                                                                            | Verified through Codex's own `skills/list`                                                                                                                                                                                                                                     | `docs/acceptance/f2b-owner-decisions.md`                     |
| A folder with an ACL for the sandboxed `agent` account                                                                | Judged by its ACL; accounts the owner names in `trusted-writers` are accepted                                                                                                                                                                                                  | `docs/acceptance/f1-f2a-2026-10-19.md`, `agentconfig/acl.rs` |

ADR numbers: the documentation branches already used 0029 to 0031 (a draft restoration ADR,
"corrections to ADR 0028", "observed model of managed sessions"). The ADRs written during the
build were renumbered to **0034 to 0038** so that no number means two things.

Not reconciled: the security audits and corrective plans (docs 36 to 49) assume a stricter
authority isolation than the local product uses; nothing in F1 to F14 relies on them or weakens
the managed plane (the authority boundary test still passes).
