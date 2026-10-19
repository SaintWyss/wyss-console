# ADR 0046: Passphrase-sealed instruction templates (F5)

Status: Accepted within Santiago's 2026-10-19 product-completion authorisation
Date: 2026-10-19

Extend ADR 0045 with a portable **text template**, distinct from a restoration record.
The template has no provider root, destination path, trust setting or permission grant.

- A native file chooser and native passphrase entry export the selected existing
  instruction file. Format `wyss-instruction-template`, envelope/payload version 1;
  the authenticated payload holds only `kind` (`rule` or `skill`) and Markdown text.
- Reuse the vault's random salt, PBKDF2-HMAC-SHA256 (600,000 rounds) and
  ChaCha20-Poly1305. Bind it to its random ID and a new associated-data domain,
  `instruction-template`. It cannot open as a restoration, restoration export or API key.
  The export is always ciphertext; there is no plaintext option or stored passphrase.
- An import names an **existing inventory item**, independently of the file. A native
  chooser and passphrase open the template. Reject unknown fields/versions, invalid IDs,
  wrong passphrases, tampering, a kind mismatch, invalid text, files over 512 KiB and
  decoded text over 64 KiB. The inherited vault bounds apply before expensive derivation.
- Import is not activation. Show all text in the native editor, explicitly labelled
  untrusted, and name the actual destination. The user can edit or cancel. Only its
  response can become an ordinary document-edit plan, followed by the independent native
  confirmation. Compare against the original destination bytes captured before import.
- The page sends only an opaque item ID and receives only Done/a redacted Plan. It never
  receives template text, a passphrase or a filesystem path. Two exact capabilities are
  added; no generic filesystem, process or shell capability is granted.
- An applied import uses the same always-sealed exact inverse, journal and recovery as
  any instruction edit. Cross-provider rule text is portable; adoption/reload by the
  destination provider remains unobserved. No authority or Managed contract changes.

This increment targets existing instruction files. Creation/deletion, skill-directory
migration and hook/MCP template authoring remain separate outstanding engineering work.
Real GUI evidence and limitations are in the F5 acceptance record.
