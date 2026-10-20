# ADR 0049: Remote gateway core (F10a foundations, no transport)

Status: accepted as a **foundation**. Nothing is reachable, nothing listens, nothing is exposed.

## Decision

`src-tauri/src/remote/` holds the host-side **decisions** of the remote design (16 §5.2–5.3) with no
transport: grants and profiles, the device state machine, QR-secret pairing, the per-request check, the
challenge/step-up binding, a limiter, a replay cache, the content-free audit record, and revocation. It
lives in the desktop binary, never in the library the authority links (C-02, TS-SEC-03), and only its
tests construct the gateway.

- **Grants** are a closed set of ten words. `terminal.write` cannot be given for any lifetime and LP4
  cannot be given in part or whole until DN-44 is decided (GR2).
- **Devices**: active, suspended (30 days unseen or every grant ended), suspect (5 failed step-ups in
  10 minutes, or a reused challenge), revoked (final; the key is refused for ever). Only the host
  renews or reviews.
- **Pairing**: a 128-bit secret, 5 minutes, 3 attempts, consumed by its first proof, a MAC over the
  transcript, six comparison digits, the host's own yes within 2 minutes.
- **Per request**, in a fixed order: device state, replay, grant held, grant alive, step-up, journal
  room. Every refusal is one word of a closed vocabulary. What is allowed is journalled before it is
  served; a full journal refuses (`audit_unavailable`). Records carry ids and words, never content.
- **Step-up** binds channel, challenge, request id, operation, target, object revision and canonical
  bytes. A challenge is single use and lasts 60 seconds.

## Not decided, not built

No signature scheme: every signature check goes through `StepUpVerifier`, and the tests use an HMAC
stand-in that is plainly not one (the build has no asymmetric-crypto crate and none is added without
the owner's OK). No short-code pairing (needs a PAKE). No listener, no TLS, no relay (DN-05).
The foundations have received the two independent reviews below; they remain unreachable and are
**not production authentication**. The signature/transport integration has not been implemented or reviewed.

## Evidence

25 unit tests in `remote/tests.rs` (RFC 4231 HMAC vectors, each rule above, the refusal order,
tampering of every signed field, journal-full, 90-day retention). Clippy `-D warnings` clean.

## Independent review (2026-10-20)

A cold review (Codex, via `codex-gsd-check`) of the code against the rules above returned seven findings,
all fixed with tests that fail on the earlier code: `Allowed` could be built by any caller to revoke a
device (fields now private; only `handle` makes one); the paired key was not covered by the pairing MAC
(the proof and the six digits now cover transcript **and** key); an operation was not tied to its grant
(a closed per-grant table; outside it is not a request); the replay cache forgot ids after 1,024 requests
(now 24 hours and 65,536 entries, and a full cache refuses instead of forgetting); a replayed whole request
with an answered challenge did not make the device suspect; the transcript bound was applied after the
MAC work (now first, and it counts as an attempt); expiry sums could overflow (saturating).

## Second independent review (2026-10-20)

A second cold review of the corrected code returned six findings. Five are fixed with tests that fail on
the earlier code; one is declined with its reason.

- **Fixed — a decision outlived its issuer.** `apply_revoke` accepted a stored `Allowed` after the device
  that obtained it had been revoked, suspended or narrowed. It now takes the time and re-checks that the
  issuer is active and still holds an unexpired `devices.revoke`.
- **Fixed — pairing the same key again shed `suspect`.** A key already enrolled, in any state, is refused at
  the host's confirmation (as a revoked key already was); the host deals with the device it has.
- **Fixed — a clock set back revived expired things.** The gateway keeps the latest time it has been given
  and never decides with an earlier one. (A forward jump only expires things early. The caller's clock is
  the host's own: nothing remote supplies it.)
- **Fixed — the limiter forgot counters past 256 sources.** Past the bound, new sources share one counter;
  nothing counted is forgotten, so rotating names does not start a clean history.
- **Fixed — a target name in the journal.** The journal stores a short SHA-256 digest of the target, never
  the target, so a name from a file cannot be kept there.
- **Declined — refusal words distinguish an unknown id from a registered one.** The words
  (`device_revoked`, `device_suspended`…) are the design's (16 §5.2.7) and are for the holder of the
  device; device ids are 128 random bits, so telling them apart needs an id the prober already has.

The gateway remains unreachable. These two passes review its policy foundations; production authentication
and transport remain undecided (DN-05). See the
[authentication decision preparation](../research/f10-authentication-decision-2026-10-20.md): standard
cryptography candidates only, no added dependency, listener, transport or real-device enrollment.
