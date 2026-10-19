# F14 — Protected credentials management (proposed specification)

**Status:** Requested for future implementation by owner; not implemented. F14 extends the original roadmap. Source contracts: ADR 0029 as reported, architecture 11 (security), 14 (persistence), 18 (distribution), 13 (UI).

## Scope

A dedicated protected Credentials view stores and manages API credentials of supported native providers. Managed authority credentials remain outside scope until an accepted ADR. The provider's own sign-in should be preferred when suitable.

**Journeys:** Add credential with provider and masked label; replace/rotate; remove with verification; inspect sessions/profiles referencing an opaque ID; explicit encrypted export/import if supported; correctly handle a locked/missing/unavailable KWallet.

## Storage model

| Object | Permitted contents | Store |
| --- | --- | --- |
| Metadata | ID, provider, masked label, status, dates; no value | Private Console state |
| Secret | Raw credential bytes | KWallet/Secret Service or authenticated encryption keyed through it |
| Use reference | Credential ID, consent/grant scope | Profile/session; no copied secret |
| Audit | Closed event words, hashed ID, outcome, timestamp | No secret, command or header |

Use a versioned AEAD envelope, nonce uniqueness and AAD binding identity/provider/version. Reusing restoration crypto primitives does not make the restoration format an acceptable vault format automatically. Never fall back to plaintext if KWallet is unavailable. Differentiate locked, absent, permission denied, backend unavailable, corruption and unknown.

Wallet key loss may make secrets irrecoverable without an explicit owner-held export. Inform the user before first storage, import or migration.

## Trust boundaries

- A generic webview inventory or diagnostics request **must never return secret bytes**. Secret entry is a dedicated native, authorized operation.
- No "reveal" by default. A future reveal needs step-up and a documented clipboard/screenshot limitation.
- Launchers deliver only explicitly granted credentials through verified provider-supported mechanisms. Never put secrets on argv, terminal echo, logs, shell commands or generic profile content.
- Inherited environment variables can leak to child processes; scrub unrelated session secrets. Do not add a generic arbitrary-command RPC.
- Managed authority security contracts remain unchanged; no authority key path or wallet handle is exposed.
- Imported profiles cannot import or silently grant credentials.

## UI contract

Provider, masked label, credential state (Available/Locked/Missing/Unverified/In use), last rotation, actions Add/Replace/Remove/Inspect references. Display whether provider-native login makes raw API-key storage unnecessary.

Example: "KWallet is locked. No change was made. Unlock it and retry." Never display raw exception details that may contain secret values.

## Acceptance and security regression

1. Marker secrets never appear in UI projection, history, journal, log, crash report, ordinary filesystem files, exports or process invocation.
2. Wallet unavailable/locked fails closed; no false success and no plaintext fallback.
3. Tampering with nonce/AAD/ciphertext/tag produces typed integrity failure, no partial secret.
4. Concurrent rotate/delete doesn't reinstate stale values; wrong passphrase/import version leaves state unchanged.
5. Lost wallet key on another device produces an honest recovery limitation; optional passphrase export requires explicit user action.
6. Launch without a credential reference never inherits unrelated secret bytes.
7. Delete removes key-store item and owned metadata; migration/uninstall respect owner retention choices.
8. GUI + KWallet + provider credential delivery tested end to end, with focused independent security review.

## Decisions requiring owner acceptance

- Keychain-native value versus a separate versioned ciphertext store with wallet-protected master key.
- Verified provider-specific authentication paths, especially OAuth-only providers.
- Whether reveal/individual export is permitted (default proposal: **no reveal**).
- Remote exposure (default proposal: **metadata only**, no secret export).
- Reauthentication/idle lock consistent with real KWallet support.

Do not collect Santiago's real API keys for development tests. This is a draft, not authorization to access or store them.
