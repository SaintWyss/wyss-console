# Security assurance: trust boundaries, threats and verification map

**Status:** Proposed supplement to canonical architecture [11](../../../architecture/11-SECURITY-THREAT-MODEL.md), 10 and accepted SG contracts. This is **not** a new acceptance of residual risk. A coding agent may not treat these controls as permission to bypass existing local authority restrictions.

## Nonnegotiable principles

1. **Two distinct planes.** Native CLI execution inherits provider security; Managed work requires its separate authority attestations, policy, approvals and audit. Neither plane impersonates the other.
2. **Provider files remain provider-owned.** Never edit without verified expected revision and trust, explicit plan/confirmation, atomic update and safe undo.
3. **Evidence is typed.** A configured switch is not a provider-effective state; a requested model is not observed; a socket/PID is not live process identity.
4. **The application must fail closed.** Unknown version, unsafe ACL, untrusted symlink, missing wallet key, failed attestation, corrupt remote grant and ambiguous import never auto-enable a capability.
5. **No ambient remote authority.** A connected device only receives explicitly granted, revocable operations.
6. **No generic credential circulation.** Wallet secrets are never serialized to general webview state, logs, overlays, argv or handoff.

## Threat-to-control matrix

| Threat                                 | Trust boundary                                | Mandatory defense                                                                                                   | Negative verification                               |
| -------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| Symlink/TOCTOU file replacement        | Config engine → provider-owned FS             | Descriptor-based trust, nofollow, revision check, atomic/exchange semantics                                         | Swap target/source during apply; no untrusted write |
| Trusted `agent` co-writer abused       | User ACL exception                            | Named, local, owner-controlled trusted list with restrictive ownership; warn that co-writer has genuine write power | Unknown user/shared group/world-writable denied     |
| Secret cached in undo                  | Provider settings → Console journal           | AEAD at rest with OS wallet, AAD and private modes; refuse unsafe store                                             | Search logs/records and lock wallet; zero leaks     |
| Unsafe profile overlay                 | Profiles → provider CLI                       | Verified provider argument grammar, native confirmation for security-control reduction                              | Imported profile cannot grant code execution        |
| Shell injection via launcher           | Webview → native process                      | Structured argv, closed executable catalogue and escaped shell replacement contract                                 | Metacharacters remain inert UI data                 |
| Forged Native/Managed status           | Host UI → authority                           | Explicit plane/source plus attested managed identity                                                                | Tampered source can't show Managed badge            |
| Managed hook bypass                    | Provider config → Managed runtime             | Maintain SG requirements, test isolated Inference under canonical E1–E8                                             | Failed isolation cannot fall back to user home      |
| Cross-runtime attestation reuse        | F8/authority                                  | Per-generation process/policy/lease binding                                                                         | A response from runtime A rejected in B             |
| Cross-session/handoff prompt injection | Untrusted transcripts/artifacts → destination | Bounded reviewed package, quoted content inert and no grant transfer                                                | Malicious instruction not executed by Console       |
| Token exposure from parent environment | Launcher → provider child                     | Strip unrelated inherited secrets and only inject reviewed provider-specific values                                 | Marker token absent from child/proc/log             |
| Credential stolen from GUI             | KWallet → webview                             | Opaque handles, no raw secret in generic IPC, native sensitive actions                                              | API metadata never includes secret                  |
| Credentials sent via remote gateway    | Wallet → device                               | No remote secret export by default, explicit permissions                                                            | Read-only grant cannot access secret                |
| Remote replay/impersonation            | Device → gateway                              | Authenticated encrypted transport, device-bound keys, nonce/request IDs, host ID and expiry                         | Expired/other-host/malformed request denied         |
| Remote revocation gap                  | Gateway → host sessions                       | Revocation on each mutating request and active stream teardown                                                      | Revoked device immediately loses input/output       |
| Terminal output DoS                    | Provider PTY → host/network/page              | Bounded queues/scrollback, backpressure, fair client consumption                                                    | Runaway producer doesn't exhaust memory             |
| Privilege confusion during update      | Installer → authority binaries                | User-only trusted executable paths, artifact provenance and safe service migration                                  | Group-writable binary path rejected                 |
| False terminal liveness                | Host registry → GUI                           | Host generation, stable session ID and process evidence                                                             | Killed host/session never shown Active              |
| Sensitive diagnostics                  | Error/log exporter → disk/user                | Closed codes, user-previewed redacted export                                                                        | Seed credentials not in logs/report                 |
| Dependency supply chain                | Build → installed package                     | Lockfile, license/SBOM audit, signed/provenance release                                                             | Tampered bundle rejected                            |

## Independent review criteria

F5/F14: review executable authoring, credential storage/argv leakage, journal/undo and imported templates. F10: review pairing, grant isolation, revocation, transport and escalation. G-INF/F8/F11b: independent review of authority attestation, runtime isolation and schema compatibility. F13: provenance, installer, update and key retention.

Ordinary UI copy or new documentation does not need an unrelated full-scope security audit. Any security review must identify tested revision, affected threats, severity, reproduction and explicitly fixed versus accepted residual risks.

## Non-negotiable release blocks

- Provider file mutation without verified writable trust + human permission policy.
- An uncontrolled free shell command constructed from UI/remote input.
- A blanket remote same-user authority bridge.
- A failed managed attestation followed by native fallback.
- Logging/export of API keys, token values or transcript.
- Silent overwrite of provider changes on undo or schema downgrade.
- Public gateway listener without owner authorization and applied network restrictions.
- Crypto without wallet protection where secure storage is required.
- Reporting an active process/model/credential state not actually verified.

## Residual risks (explicitly disclosed, not approved away)

A locally trusted ACL co-writer can intentionally modify provider files; the Console cannot prevent it by app policy. Provider sessions and their own subprocesses may emit secrets to terminal output. OS screenshots/clipboard can leak user-displayed content. Host or power failure can lose volatile terminal scrollback. Readable source status may become stale before provider hot-reload.

Owner approval is required for accepting these risks in a release scope; the software must report them accurately.
