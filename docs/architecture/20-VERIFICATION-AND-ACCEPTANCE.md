# 20 — Verification and acceptance

The master test plan. **No test was run to write it**, and none of the tests specified here
exists unless marked `[R]`. Each specification names what it holds, at which level, in which
phase it is written, and what would make it fail.

## 1. Levels and environments

| Level                     | What it is                                                                               | Where it runs                                                | Exists                                        |
| ------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------ | --------------------------------------------- |
| Unit                      | Pure functions: parsers, planners, resolution, classification                            | Rust and frontend suites                                     | `[R]` for today's code                        |
| Contract                  | Rust and TypeScript agree on a wire shape, from shared fixtures                          | Both suites, same fixture files                              | `[R]` (ADR 0008)                              |
| Integration               | A subsystem against real kernel objects: PTYs, files, inotify, a private service manager | Rust suite on temporary directories and private homes        | `[R]` for PTY and lifecycle                   |
| Adversarial filesystem    | Links, races, permissions, hostile names and sizes                                       | Rust suite on temporary trees                                | No                                            |
| Provider compatibility    | An adapter against fixtures that reproduce one provider version                          | Rust suite                                                   | `[R]` for the Codex protocol codec            |
| Interface                 | Views mounted with a fake backend                                                        | Frontend suite (jsdom)                                       | `[R]`                                         |
| Layout rules              | Properties of the stylesheet that jsdom cannot render                                    | Frontend suite, reading the stylesheet                       | `[R]` (`tests/view-visibility.test.ts`)       |
| Nested display            | The packaged application driven with real input, in an isolated display and account      | By hand or script, off the user's session                    | `[O]` procedure exists outside the repository |
| Real provider, end to end | A real agent, the owner's account, a throwaway project                                   | The owner's machine, **only with his authorisation per run** | `[O]` done once for Codex                     |
| Performance               | The procedures of [15 §3](15-PERFORMANCE-ENGINEERING.md)                                 | The reference machines                                       | No                                            |
| Long-running              | Hours of use, watching memory and descriptors                                            | The reference machines                                       | No                                            |

Rules that hold for every level (NFR-MNT-03): no test reads or writes the user's configuration,
home, units or authority state `[R]` for today's suite (README `:128-131`); tests are
deterministic; a test that needs a provider binary is separate from the default run.

## 2. What a credible test needs

The project's own method: a check is believed only with controls. For every guarantee:

1. the naive case must **fail** (the guarantee is not satisfied by accident);
2. a deliberately broken fixture must be **detected**;
3. the correct case must **pass**.

A regression test for a fixed defect is run once against the code before the fix and must fail
there; that run is recorded.

## 3. Configuration engine

### 3.1 Fixtures

One directory per provider and version, each a small home: `valid/`, `empty/`, `missing/`,
`malformed-json/`, `malformed-toml/`, `unknown-fields/`, `oversized/`, `link/`, `link-loop/`,
`unreadable/`, `generated/`, `conflict-copy/`, `secrets/` (seeded with marker values in every
secret class), `comments/` (TOML and JSONC with comments in awkward places), `odd-layout/`
(valid but formatted so that a round trip would change bytes). Beside each, the expected
inventory as a file. Fixtures are synthetic; none is copied from a real home.

### 3.2 Reading and inventory

| ID        | Holds                                                                                                         | Level         | Phase                              | Fails when                                     |
| --------- | ------------------------------------------------------------------------------------------------------------- | ------------- | ---------------------------------- | ---------------------------------------------- |
| TS-CFG-01 | The inventory of `valid/` equals the expected file exactly, per provider version                              | Compatibility | F1                                 | Any item differs                               |
| TS-CFG-02 | Empty and missing sources give `absent` or `source_missing`, never an error for the whole provider            | Unit          | F1                                 | The provider's inventory is empty or fails     |
| TS-CFG-03 | A malformed source reports its position and no content; other sources are still listed (NFR-REL-04)           | Unit          | F1                                 | Content appears in the error, or others vanish |
| TS-CFG-04 | Unknown fields are counted and preserved through a read and a write of the document model                     | Unit          | F1                                 | A field is dropped                             |
| TS-CFG-05 | Every bound of C-11: size, count, depth, link hops, listing length; each exceeded by one                      | Unit          | F1                                 | The reader allocates or loops                  |
| TS-CFG-06 | Round trip: `valid/` is editable; `odd-layout/` is readable and marked not editable                           | Unit          | F1                                 | An odd layout is marked editable               |
| TS-CFG-07 | An outside change yields a record that names added, removed and changed items                                 | Integration   | F1                                 | A removed hook is not named                    |
| TS-CFG-08 | A change the provider is not documented to reload leaves running sessions `unknown` or `stale`, never `using` | Unit          | F1                                 | A session is shown as adopting it              |
| TS-CFG-09 | No plan can target a system, provider-owned or generated source                                               | Unit          | F1 (as "no write capability"), F2a | A plan is produced                             |

### 3.3 Filesystem, adversarial

| ID       | Holds                                                                                              | Phase | Fails when                  |
| -------- | -------------------------------------------------------------------------------------------------- | ----- | --------------------------- |
| TS-FS-01 | The set of paths an adapter can touch is exactly its documented list, for a given home and project | F1    | A path outside it is opened |
| TS-FS-02 | A provider directory with a group-writable ancestor is read and never written                      | F1    | A write is planned          |
| TS-FS-03 | A file swapped for a link between the check and the open is not followed                           | F1    | The link's target is read   |
| TS-FS-04 | A link chain longer than the bound, and a loop, end with `source_unsupported`                      | F1    | The reader hangs            |
| TS-FS-05 | An unreadable file is `unreadable`, distinct from `absent`; a file that reappears is read again    | F1    | It is reported as removed   |
| TS-FS-06 | During a write, the target replaced by a link to another file leaves that other file untouched     | F2a   | The other file changes      |

### 3.4 Mutation

| ID              | Holds                                                                                                                                       | Phase    | Fails when                              |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------- | --------------------------------------- |
| TS-CFG-10       | A plan applied after the file changed writes nothing and answers `changed_outside`                                                          | F2a      | Bytes change                            |
| TS-CFG-11       | A write whose read-back does not verify is recorded `unverified` and reported                                                               | F2a      | It is reported applied                  |
| TS-CFG-12       | Fault injection: the process stopped between every pair of steps of 14 §4 leaves the provider's file whole and the stores repairable        | F2a      | Any file is partial, or repair is wrong |
| TS-CFG-13       | A provider version without evidence refuses every mutation                                                                                  | F2a      | A plan is produced                      |
| TS-CFG-14       | No plan, for any operation on any fixture, changes a trust record                                                                           | F2a, F2b | The bytes of a trust record differ      |
| TS-CFG-15       | Every Codex plan carries a managed-effect verdict equal to what `zero_external_hooks` would decide on the resulting definitions             | F2b      | The two disagree on any fixture         |
| TS-CFG-16       | A TOML edit changes only the touched key: every comment and blank line of `comments/` survives                                              | F2b      | Any other byte differs                  |
| TS-CFG-17 to 20 | Authoring: a created entry validates; an edit keeps a restoration record; a delete is undoable; nothing typed in the page becomes a command | F5       | —                                       |

A table test covers every cell of action × provider × outcome of
[08](08-RULES-HOOKS-SKILLS-MCP.md): each has an expected file, an expected journal entry and
an expected message.

### 3.5 Recovery

| ID        | Holds                                                                             | Phase |
| --------- | --------------------------------------------------------------------------------- | ----- |
| TS-REC-01 | An orphan restoration record is removed at start and noted                        | F2a   |
| TS-REC-02 | A change whose journal entry is missing is recovered from the record              | F2a   |
| TS-REC-03 | A restore interrupted after the rename is completed at start                      | F2a   |
| TS-REC-04 | A store of a newer version is left untouched and its feature becomes read only    | F2a   |
| TS-REC-05 | Deleting the engine's data lists every entry that would become unrestorable first | F2a   |

## 4. Terminal and native sessions

| ID               | Holds                                                                                             | Level             | Phase | Exists                            |
| ---------------- | ------------------------------------------------------------------------------------------------- | ----------------- | ----- | --------------------------------- |
| TS-TERM-01       | Spawn, write, read, resize, exit codes, close, on real PTYs                                       | Integration       | —     | `[R]`                             |
| TS-TERM-02       | The limits of C-06, each exceeded by one                                                          | Integration       | —     | `[R]`                             |
| TS-TERM-03       | The shell's first output is produced at the final size (no resize after start)                    | Nested display    | P1    | Done by hand `[O]`; not automated |
| TS-TERM-04       | Full-screen programs: alternate screen, mouse, colours, wide characters, with a reference capture | Nested display    | P1    | No                                |
| TS-TERM-05 to 08 | Panes, search, key map, titles                                                                    | Interface, nested | T1    | No                                |
| TS-TERM-09       | Layout restore                                                                                    | Integration       | T2    | No                                |
| TS-NAT-01        | Catalogue discovery against a fake `PATH`                                                         | Unit              | F3a   | No                                |
| TS-NAT-02        | Arguments and variables derived from a profile, as pure functions                                 | Unit              | F3a   | No                                |
| TS-NAT-03        | Each shell recipe passes arguments with spaces, quotes and newlines intact, to a fake agent       | Integration       | F3a   | No                                |
| TS-NAT-04        | The fake agent's environment equals the one an interactive shell gives                            | Integration       | F3a   | No                                |
| TS-NAT-05        | The start request type has no free command, argument, variable or path field                      | Unit              | F3a   | No                                |
| TS-NAT-06        | Exit codes recorded; a crash of the window yields `lost`                                          | Integration       | F3b   | No                                |
| TS-NAT-07        | A PID reused by another process is never taken for the session                                    | Integration       | F3b   | No                                |
| TS-NAT-08        | No sampling runs while the overview is hidden                                                     | Interface         | F3b   | No                                |
| TS-NAT-09        | An agent started by hand is listed as "started outside the Console"                               | Integration       | F3b   | No                                |

## 5. Security

| ID        | Holds                                                                                                                                       | Phase                | Fails when                                               |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | -------------------------------------------------------- |
| TS-SEC-01 | With `secrets/`, no marker value appears in any serialised answer to the window                                                             | F1                   | A marker is found                                        |
| TS-SEC-02 | After every operation on `secrets/`, no marker appears in any store, log or diagnostic                                                      | F2a                  | A marker is found                                        |
| TS-SEC-03 | The configuration module and the control module do not reference each other; the authority binary does not contain the configuration module | F1                   | A reference exists                                       |
| TS-SEC-04 | Every window command's request type is enumerated; none has a free path, content, command, argument or environment field                    | F1, then every phase | A new field appears without review                       |
| TS-SEC-05 | `apply` on a plan classed `weakens_security` or `executes_code`, with the dialog refusing, writes nothing                                   | F2a                  | Bytes change                                             |
| TS-SEC-06 | An imported file with unknown, oversized and hostile fields is reduced to the schema and applies nothing                                    | F2a, F3c             | Anything is applied                                      |
| TS-SEC-07 | The classifier of 11 §5.4 on a labelled corpus: the rate of secret-bearing entries it passes is reported                                    | F2a                  | The rate exceeds the threshold the owner sets with DN-01 |

The existing suite of the managed plane (isolation, attestation, sabotage, crash matrix, ADR
0018 `:358-378`) `[R]` is run unchanged in every phase; a phase that needs to change one of its
tests needs an ADR.

## 6. Sessions, profiles, models

| ID              | Holds                                                                            | Phase |
| --------------- | -------------------------------------------------------------------------------- | ----- |
| TS-SES-01       | The overview with each owner absent, stale and inconsistent                      | F3b   |
| TS-SES-02       | Two sessions on one folder produce the warning                                   | F3b   |
| TS-SES-03       | Handoff content reaches the receiver as quoted material and changes no authority | F11a  |
| TS-SES-04       | "Continue as managed" creates a new session and never reuses an identifier       | F11a  |
| TS-PRF-01       | Resolution table per provider                                                    | F3c   |
| TS-PRF-02       | Inheritance, exclusion, depth bound, cycles                                      | F3c   |
| TS-PRF-03       | A required item that is not applicable refuses the start                         | F3c   |
| TS-PRF-04       | Generated files are removed at exit and after a simulated crash                  | F3c   |
| TS-PRF-05       | No resolution writes into a provider's directory                                 | F3c   |
| TS-PRF-06       | Versions and pointer; a save over a newer version is refused                     | F3c   |
| TS-PRF-07       | Import and export round trip; no secret field survives                           | F3c   |
| TS-MOD-01 to 04 | The projection carries exactly what the provider reported; absent stays absent   | F6a   |
| TS-MOD-05, 06   | A model chosen at start is the one passed; an unlisted one is refused            | F6b   |

## 7. Interface and accessibility

| ID         | Holds                                                                                    | Phase      |
| ---------- | ---------------------------------------------------------------------------------------- | ---------- |
| TS-UX-01   | Every view in every state of 13 §3 renders its fixed sentence and no raw code            | Each phase |
| TS-UX-02   | No control exists for a capability the adapter does not declare                          | F1         |
| TS-UX-03   | With focus in the terminal, only the documented chords are not delivered to the program  | T1         |
| TS-UX-04   | A native session never shows the words used for mediation; every session shows its plane | F3a        |
| TS-UX-05   | An instruction file is never labelled as a control                                       | F1         |
| TS-A11Y-01 | Keyboard reachability of every control, per view                                         | Each phase |
| TS-A11Y-02 | Contrast computed for every text and background pair of the tokens                       | Each phase |
| TS-A11Y-03 | Every state indicator has a text alternative                                             | Each phase |

`[R]` today: a test visits every view in four authority states and refuses development
vocabulary and raw codes (ADR 0026 `:181-182`); it is extended, not replaced.

## 8. Regression, every phase

| ID        | Holds                                                                                                                  |
| --------- | ---------------------------------------------------------------------------------------------------------------------- |
| TS-REG-01 | The terminal starts and works with the configuration engine failing to initialise                                      |
| TS-REG-02 | The whole managed suite passes unchanged                                                                               |
| TS-REG-03 | The authority lifecycle suite passes unchanged                                                                         |
| TS-REG-04 | The window's capability list equals the expected list exactly `[R]` (`tests/terminal-view.test.ts`)                    |
| TS-REG-05 | No existing test was weakened: a diff of the test tree shows only additions, or each change is justified in the review |

## 9. Performance and reliability

| ID               | Holds                                                                                            | Phase        |
| ---------------- | ------------------------------------------------------------------------------------------------ | ------------ |
| TS-PERF-01 to 12 | The metrics M01 to M19 of 15 §2, as scripts                                                      | P1           |
| TS-PERF-13       | M03, M05, M16 after each phase against the baseline                                              | From F1      |
| TS-LONG-01       | Eight hours with the maximum of terminals: memory and open descriptors do not grow without bound | P1, then F3b |
| TS-LONG-02       | 10,000 outside changes: the watcher neither leaks nor misses the final state                     | F1           |
| TS-CONC-01       | Two windows planning and applying against one file: exactly one write per plan, none lost        | F2a          |

Thresholds are the budgets of 15 §4 once restated after P1; until then these tests report and
do not fail.

## 10. Several computers and remote

| ID               | Holds                                                                                           | Phase   |
| ---------------- | ----------------------------------------------------------------------------------------------- | ------- |
| TS-SYNC-01       | A file replaced the way a replication tool does is one outside change                           | F1      |
| TS-SYNC-02       | A conflict copy marks the source and blocks mutation                                            | F1, F2a |
| TS-SYNC-03       | Two private states and one shared file: disable on A, restore on B from last-seen, A reconciles | F2a     |
| TS-SYNC-04       | A second computer that never saw the entry offers import only                                   | F2a     |
| TS-HOST-01 to 06 | Terminals survive the window; two clients; resize rule; scrollback bound; crash of the host     | F9      |
| TS-REM-01 to 10  | Pairing, grants, revocation on every request, step-up, replay, drop and reconnect, audit        | F10a    |
| TS-REM-11 to 13  | The phone client                                                                                | F10b    |

## 11. Real-provider acceptance

Run by hand or by script on the owner's machine, each run authorised by him, on a throwaway
project and, for configuration, first on a copy of a home and then on the real one.

| Run                                                                         | Phase           | What is recorded                                                  |
| --------------------------------------------------------------------------- | --------------- | ----------------------------------------------------------------- |
| Managed Codex: Inference, Local Tools, approve, deny, cancel, resume, close | Done once `[O]` | Captures and the event list                                       |
| Inventory against the real configuration                                    | F1              | Hashes of every provider file before and after: identical         |
| Disable and restore a real hook                                             | F2a             | The provider's own listing before, between and after; file hashes |
| A native agent started with the real environment                            | F3a             | Its environment compared with a hand-typed start                  |
| Two sessions with different profiles                                        | F3c             | Global file hashes unchanged                                      |
| Isolated Inference with a real hook                                         | G-INF           | AC-GINF-01, 02, 05                                                |

## 12. Acceptance matrix

Each phase is accepted when its tests pass, its acceptance criteria (19 §3) are met in front
of the owner, its review gate is passed, and the regression set of §8 is green.

| Phase | Tests                                                                                                             | Acceptance criteria                    | Gate                            |
| ----- | ----------------------------------------------------------------------------------------------------------------- | -------------------------------------- | ------------------------------- |
| P1    | TS-PERF-01 to 12, TS-LONG-01, TS-TERM-03, 04                                                                      | A baseline for every metric            | Owner                           |
| F1    | TS-CFG-01 to 09, TS-FS-01 to 05, TS-SEC-01, 03, 04, TS-UX-01, 02, 05, TS-SYNC-01, 02, TS-LONG-02, TS-REG-01 to 05 | AC-F1-01 to 06                         | External review; owner          |
| F1b   | Specifications only                                                                                               | A recorded result per experiment       | Owner                           |
| F2a   | TS-CFG-10 to 14, TS-FS-06, TS-SEC-02, 05, 06, 07, TS-REC-01 to 05, TS-CONC-01, TS-SYNC-03, 04                     | AC-F2a-01 to 06                        | External security review; owner |
| F2b   | TS-CFG-15, 16                                                                                                     | AC-F2b-01 to 03                        | External review; owner          |
| G-INF | AC-GINF-01 to 08 automated; TS-REG-02                                                                             | AC-GINF-01, 02, 05 on the real machine | External security review; owner |
| F3a   | TS-NAT-01 to 05, TS-UX-04                                                                                         | AC-F3a-01 to 03                        | Owner                           |
| F3b   | TS-NAT-06 to 09, TS-SES-01, 02                                                                                    | AC-F3b-01, 02                          | Owner                           |
| F3c   | TS-PRF-01 to 07                                                                                                   | AC-F3c-01, 02                          | Owner                           |
| Later | As listed in 19 §3                                                                                                | Written when the phase is planned      | As listed                       |

## 13. Not designed yet

Tests for F5's authoring contract, for the remote gateway's protocol, for the phone client and
for distribution are named and not specified: they depend on decisions DN-05, DN-07 and DN-12.
Fuzzing of the parsers is recommended from F1 and not specified here.
