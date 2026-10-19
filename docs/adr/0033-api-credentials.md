# ADR 0033: API credentials kept sealed, never shown (F14)

Status: Accepted (owner authorisation of the whole roadmap, 2026-10-19)
Date: 2026-10-19

## Decision

1. A provider API key is sealed with the vault of ADR 0029 (ChaCha20-Poly1305; the key in the
   Secret Service; the record's id and a new kind, `credential`, bound as associated data, so a
   sealed body cannot be moved to another record or passed as another kind). One file per key in
   a 0700 directory, 0600, written atomically. With no key store nothing is kept, and nothing is
   kept unsealed in its place.
2. What the window sees is a name (1 to 60 characters), a service word (Anthropic, OpenAI,
   Google, OpenRouter, other), a 12-digit checksum (a prefix of the key's SHA-256) and two times.
   **No operation returns a key**: the window can list, add, replace, rename, delete and check
   ("the key opens and matches its checksum"). The page contract drops any field it does not know.
3. A key is typed in a native masked window the page cannot reach, asked once, zeroised after;
   deletion asks again in a native window.
4. At most 100 keys; a key has no whitespace or control characters and at most 4096 characters.
5. **Not decided here**: how a key reaches a provider. Using a key to start an agent needs its
   own decision (roadmap F14 exclusion). Until then a key can only be kept and managed.

## Verified

Unit tests: nothing on disk or in a list holds the key; permissions; a swapped record does not
open; a missing key store keeps nothing; a lost key is said. Not exercised in the real window,
because entering a key writes to the owner's real Secret Service.
