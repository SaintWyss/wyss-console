# ADR 0056 — Delivering a provider credential to a Claude Managed runtime

Status: **Proposed; not accepted** · Date: 2026-10-23 · Owner: Santiago

## Context

[ADR0038](0038-api-credentials.md) keeps API keys sealed with the vault and returns none: "No operation
returns a key", and §5 says _how a key reaches a provider needs its own decision_. [ADR0053](0053-claude-code-managed-integration-boundary.md)
originally recommended "a key held in the existing credentials store" as if that route existed; it does not,
and the first implementation read a plaintext `0600` file instead. That was wrong on both counts and has been
removed. This ADR is the decision ADR0038 asked for.

## State of the code (what is built, what is not)

- `f8::claude::credential`: `CredentialSource` is the only way a launcher can get a key. **Production has no
  implementation** (`NoCredential` returns none). The module reads no file, keyring or environment variable
  (a test reads the source to check).
- A launcher made for production refuses to start a runtime before it asks any source for anything
  (`Admission`, ADR0053 gates), and refuses the real provider without a key.
- When a key exists it is handed once to the **credential broker** (`claude-bridge/broker.py`), a separate
  owned process on its stdin. It is never in an environment, a command line, a log, a frame to the bridge, a
  diagnostic or a window. The bridge and the CLI hold a per-run password for the broker's socket and nothing
  that spends money. The broker connects to `https://api.anthropic.com` only, over verified TLS; plain HTTP
  can never carry a key, and the origin is an enum in Rust and a one-element list in the broker.
- Tests cover the broker's refusals (origin, scheme, port, userinfo, path, malformed key/token/profile), a key
  reaching a TLS provider only when the certificate verifies, no key in `/proc/<pid>/cmdline` or `environ`,
  no key in any error or notice, and an environment with no credential-named variable in the whole namespace.
- **Not built**: any source of a real key.

## Options

| Option                                                                                             | What it means                                                                                                                                                                                                                                                                                          | Risks                                                                                                                |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------- |
| **A. A `SealedCredentialSource`: native, user-present unseal at each runtime start** (recommended) | The authority asks the vault (ADR0034) for one named `credential` record of service word `Anthropic`, after a native confirmation window the page cannot reach; the key goes from the vault to the broker's stdin and nowhere else. No operation returns it to a window; deletion still asks natively. | The key is in the memory of the authority and the broker for the life of the runtime; no zeroization is claimed.     |
| B. A file or an environment variable named by the operator                                         | What the first implementation did.                                                                                                                                                                                                                                                                     | Plaintext at rest or in a process listing; contradicts ADR0038. **Rejected.**                                        |
| C. The Native login (subscription sign-in)                                                         | Reuse the Claude Code login.                                                                                                                                                                                                                                                                           | Requires copying or re-homing Native credentials, and the provider says third parties cannot offer it. **Rejected.** |
| D. No delivery                                                                                     | Claude Managed stays a fixture; Native remains the way to use Claude Code.                                                                                                                                                                                                                             | Gives up the Managed goal for Claude.                                                                                |

## Recommendation: A, with these conditions

1. Scope: one record chosen by the owner for the runtime start (not "any key"); service word `Anthropic`.
2. The confirmation window names the record by its stored name and checksum prefix and the profile being started.
3. Lifetime: the key is requested at each runtime start and handed to the broker; a new key means a new
   runtime. Revocation is deleting the record and stopping the runtime; neither is automatic for the other.
4. Never returned to the window, never in a DTO, a diagnostic, an audit record or a history record.
5. Honest limits, to be written into the release record: no zeroization is claimed; a process of the same user
   that may read another's memory (ptrace) can read the key from the authority or the broker; the broker is a
   separate process so that the _model's_ process never has it, not to defeat the owner's own account.
6. Real-credential admission stays closed until a reviewer other than the author has inspected the final code
   and the network confinement (`m3::gate::INDEPENDENT_REVIEW_PASSED`), and until ADR0053 and ADR0055 are accepted.

## Tests this unlocks

A real Inference turn and a real Local Tools turn with an owner-supplied key, each paid, each with a rollback
(stop the runtime, delete the record), and the real-provider part of the adversarial matrix. **Effects of
accepting:** none by itself; the gates stay closed until a release decision.
