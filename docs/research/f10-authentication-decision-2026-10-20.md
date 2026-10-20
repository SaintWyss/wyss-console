# F10 production authentication candidate

2026-10-20. **Decision preparation only; not accepted or implemented.** ADR 0049's reviewed
gateway foundations remain unchanged and unreachable. No listener, tunnel, device enrollment,
certificate, credential, transport or dependency was enabled or added.

## Recommendation and alternatives

| Candidate                                                                            | What it proves                                                          | Limitation                                                                           | Disposition                                                    |
| ------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | -------------------------------------------------------------- |
| TLS with mutually authenticated device certificates, plus WebAuthn step-up           | Channel peer possession separately from a fresh user-verified assertion | Certificate lifecycle and native/mobile/RP-origin compatibility need proof           | Recommended for further design review                          |
| HTTP Message Signatures (RFC 9421), plus TLS and separate user-verification ceremony | Standard request authentication over selected covered components        | More canonicalization/replay machinery; signatures alone do not establish presence   | Keep as an alternative if HTTP/offline requirements justify it |
| Raw Ed25519 over `step_up_message`, or the test HMAC double                          | Key possession over application-defined bytes                           | Does not establish user verification; promotes an unreviewed custom signing protocol | Do not use in production                                       |
| A bearer token or QR secret as persistent device identity                            | Possession of a copyable secret                                         | Theft/replay and no per-action user verification                                     | Reject as sole production authentication                       |

Use standard protocol implementations and audited cryptographic libraries. Do not implement
signature algorithms, X.509 validation, WebAuthn assertion verification, PAKE or TLS by hand.
Transport selection remains DN-05 and belongs to Santiago; this recommendation does not resolve it.

## Standard boundaries

[TLS 1.3](https://www.rfc-editor.org/rfc/rfc8446) supplies an authenticated encrypted channel;
[RFC 9266](https://www.rfc-editor.org/rfc/rfc9266) defines its exporter channel binding.
Disable early data for effectful requests. Use strict peer certificate/identity validation and the
host's device revocation state on every request. Source IP, headers and successful TLS resumption
are never a grant. Before implementation, freeze the applicable current TLS specification and
errata supported by the chosen maintained library; RFC 8446 is a protocol baseline, not a claim
that it is the latest standards revision.

[WebAuthn](https://www.w3.org/TR/webauthn-2/) separates user presence from user verification.
Require verification, correct RP ID and exact allowed origin, type, fresh challenge and registered
credential; let a reviewed library validate the full assertion. Do not treat an arbitrary device
signature as a WebAuthn assertion. A syncable passkey also does not prove one physical device:
certificate identity and WebAuthn user verification must be separate, with a reviewed policy for
credential backup/synchronization and supported authenticators.

For step-up, retain an immutable **server-side** challenge record bound to the authenticated device,
channel, request ID, operation, target, object revision and digest of the exact decoded request.
Use the library's ordinary random challenge/assertion flow; after verification consume the record
atomically and revalidate the request, grants and revision immediately before dispatch. No custom
signature envelope or new cryptographic transcript is specified here. The existing
`step_up_message` and boolean `StepUpVerifier` are policy-test seams, not a sufficient production
WebAuthn API. A future reviewed adapter must carry and validate full assertion/ceremony state;
merely swapping the test HMAC for an asymmetric verifier would not meet this decision.

The existing 60-second challenge, per-operation grants, journal-before-effect, replay, limiter,
suspect/revoked transitions and no `terminal.write` remain. Authentication never grants an operation
by itself. Recheck permission after slow verification and immediately before the effect. Persist
revocations/replay state or invalidate all old channels/challenges on restart; the in-memory
gateway alone is not production restart protection. No remote path may bypass Managed mediation.

[RFC 9421](https://www.rfc-editor.org/rfc/rfc9421) is the standard alternative if HTTP message
signatures become necessary. Its standard signature base would replace application-defined signing,
with an explicit reviewed covered-component profile and digest/replay policy; it would not replace
TLS, grants, host confirmation or user verification. Do not invent that profile before selecting
the transport and threat model.

## Library and audit evidence, not dependency approval

- [rustls](https://github.com/rustls/rustls) implements TLS with selectable cryptographic providers;
  its [security policy](https://github.com/rustls/rustls/blob/main/SECURITY.md) separates library
  and provider responsibilities. Evaluate a maintained release and its specific provider, verifier,
  audit reports, fixes, advisories, licenses, build impact and supported platforms together. An audit
  of one version/provider does not certify another. No version is pinned by this document.
- [webauthn-rs](https://github.com/kanidm/webauthn-rs) reports a SUSE product-security audit. This is
  upstream-reported evidence; the exact audited revision/scope and fixes must be obtained before
  dependency approval. Its [safe API documentation](https://docs.rs/webauthn-rs/latest/webauthn_rs/)
  requires ceremony state on the server and warns against unsafe state serialization. Evaluate
  the high-level safe API, keeping dangerous feature flags off. Do not claim current-version audit
  coverage from a project-level audit statement.

These primary sources were fetched on 2026-10-20. The recommendation is to require audited standard
cryptography; exact selected-version audit coverage is still **NOT_PROVEN**, so production enabling
remains blocked. `Cargo.toml`, `Cargo.lock` and all gateway source stayed unchanged.

## Required review, tests and owner-only decisions

Before code: specify RP origin and certificate trust/renewal/revocation, hardware-bound versus
syncable credential policy, recovery after lost devices, TLS termination/channel-binding ownership,
challenge persistence and exact execution binding. Native/mobile key-store and WebAuthn support
must be observed on target devices, not inferred from library support. A terminating proxy cannot
provide channel identity through an unauthenticated header.

Negative acceptance must cover wrong RP/origin/type/credential, missing UV, replay/expiry, changed
request/target/revision/channel/device, revoked issuer during verification, reconnect/restart,
duplicate or reordered HTTP fields if used, invalid certificates and TLS early data. Test full
audit storage, limiter saturation and failure to persist revocation without executing an effect.
Independently review the selected protocol/library adapter and enrollment/recovery flow.

Only Santiago approves DN-05, dependency additions and exact versions/features, enrollment of a
real device, reachable transport/listener and network exposure, and any expansion of grants.
Until those decisions and proofs exist: production authentication **NOT_PROVEN**; listener,
transport and real-device tests **NOT_RUN**. Preserve the two reviewed gateway passes in ADR 0049.
