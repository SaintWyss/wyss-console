# Reconciliation of the 2026-10-19 drafts with the implementation

> **Historical draft pack.** Current scope/status/dependencies are consolidated in
> [docs/product](../../product/README.md). Proposed ADR numbers and “not built” statements
> below are dated; use the [crosswalk/resolutions](../../product/traceability.md).
> Preserve the contracts as inputs, not a second current execution roadmap.

The drafts in this folder were written without sight of the code. They stay **proposals**.
The accepted record is ADR 0034–0046 and `docs/acceptance/`.

| Draft topic                       | Where the implementation is the authority                                                                                                  |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Native launcher and profiles      | ADR 0035, `docs/acceptance/f3-2026-10-19.md`                                                                                               |
| Host and session persistence (F9) | ADR 0036 (socket, ring, closed op list), not the draft schema                                                                              |
| Terminal T1/T2                    | ADR 0037 (own search, no add-on; `layout.json`)                                                                                            |
| Credentials (F14)                 | ADR 0038 (sealed in the encrypted store, key typed in a native dialog); the draft's vault contract is replaced                             |
| Handoff (F11a)                    | ADR 0039 (brief as one argv word, new session)                                                                                             |
| Install (F13)                     | ADR 0040, `scripts/install.sh`, `--diagnose`                                                                                               |
| Authoring                         | ADR 0042/0044 cover hooks/MCP; ADR 0045/0046 cover existing user instructions and sealed templates. Creation/deletion remain open          |
| Remote, F1b, F8, F12              | Current owner authorisation permits eligible engineering; specific security/network/install/credential boundaries still apply (CHECKPOINT) |

Where a draft and an ADR disagree, the ADR wins. No code was changed to match a draft.
