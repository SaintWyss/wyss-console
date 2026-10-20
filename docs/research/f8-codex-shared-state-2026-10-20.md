# F8: Codex 0.159.0 shared-state classification (read-only source review)

2026-10-20. Source read: `openai/codex` tag `rust-v0.159.0` (`codex-rs/state/src/runtime.rs`,
`codex-rs/login/src/auth/manager.rs`), plus the earlier `/proc` descriptor observation. **No provider was
started, no credential or database was read, no second process was launched.** Excerpts were
partial; every "not seen" below is NOT_PROVEN, not safe. Capacity therefore **stays 1**.

| Surface                                            | Evidence                                                                                                   | Class                                         | F8 rule                                                                                                          |
| -------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `auth.json` (access/refresh token, in-place write) | `persist_tokens` is load → overwrite fields → save; only in-process semaphores; no cross-process lock seen | **Unsafe if shared by concurrent refreshers** | Never give two live processes the same credential file. Refresh-token reuse is terminal (`refresh_token_reused`) |
| State/queue/logs/goals/memories SQLite files       | Several separate DB files "to reduce lock contention"; locking delegated to SQLite; journal mode not seen  | Serialized-shared **if** WAL+busy handling    | Unproven for two writers; per-launch private `CODEX_HOME` would make it private but needs a credential           |
| Thread rollout/history files                       | Not read                                                                                                   | NOT_PROVEN                                    | One owning session/launch per provider thread (authority-enforced)                                               |
| Model catalogue / cache                            | Per-launch immutable catalogue designed (`f8::staging`)                                                    | Private (by construction)                     | Exclusive per-instance directory, read-only file                                                                 |
| User `config.toml`, rules, hooks, MCP              | Hidden from generations by the existing isolation layer                                                    | Immutable-shared (by existing policy)         | Unchanged                                                                                                        |
| In-memory auth snapshot                            | "External modifications … NOT observed until `reload()`"                                                   | Private per process                           | A sibling's refresh makes this process's snapshot stale                                                          |

## Consequence

The decisive blocker is the credential: two processes each refreshing a rotating token can
invalidate each other. Private homes are forbidden without copying credentials (not allowed), so
there is no safe route to more than one authenticated provider process yet. Required before any
increase: exact-version lock/refresh protocol proof from the provider owner or an owner-authorized
controlled experiment. `max_active_runtimes = 1` is unchanged; F8 concurrency is **NOT_PROVEN**.
