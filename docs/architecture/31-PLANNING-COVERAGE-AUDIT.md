# 31 — Planning coverage audit and documentation backlog

How much of the **final product** is planned on paper, subsystem by subsystem, as of 2026-10-13,
after the second pass on the interface specification ([13](13-UX-UI-SPECIFICATION.md)). And the
ordered list of documentation still to be written before the planning can be called complete.
Written by the author of the set; **unreviewed**. Revised the same day after the terminal pass on
[05](05-TERMINAL-AND-NATIVE-SESSIONS.md); §2.3 shows what changed in the score and why.

What this audit measures is **documentation**: whether a builder could work from the documents
without inventing a decision of the owner's or assuming an unobserved provider behaviour. It
does not measure implementation, which is prohibited (22, DN-16), and it does not measure
validation: no design here has been tested, and everything written after the two independent
reviews returned **REJECT** is unreviewed (25 §2, §8). A label of COMPLETE below never means
reviewed, accepted or verified. **The planning is not complete.**

## 1. Method

### 1.1 Dimensions

Each subsystem of the final product is judged on five dimensions, each from the documents cited
in its row.

| Dimension                  | Question                                                                                                                                                       |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Requirement coverage       | Does every capability the owner asked for in this area have a requirement identifier in [02](02-REQUIREMENTS.md), with its source?                             |
| Design depth               | Are interfaces, states with guards and failure transitions, failure behaviour and the user-visible behaviour written, without contradiction between documents? |
| Decision completeness      | Is every decision that belongs to the owner taken, or at least recorded with alternatives in [22](22-DECISIONS-AND-OPEN-QUESTIONS.md)?                         |
| Traceability               | Does every requirement of the area reach design, phase, test and acceptance in [23](23-TRACEABILITY-MATRIX.md)?                                                |
| Verification specification | Are its tests and acceptance criteria specified well enough to be written ([20](20-VERIFICATION-AND-ACCEPTANCE.md), [19](19-IMPLEMENTATION-ROADMAP.md))?       |

### 1.2 Labels

| Label            | Means, on a dimension                                                                                                      |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------- |
| **COMPLETE**     | Nothing left to write or decide on paper for this dimension. Not reviewed, not verified                                    |
| **PARTIAL**      | Something exists and something named is missing                                                                            |
| **BLOCKED**      | It cannot be finished on paper until a named owner decision or a named observation exists, however much is already written |
| **NOT DESIGNED** | Requirements or options only; no contract, states, tests or acceptance                                                     |

These are the labels the owner asked for. They map onto those of [26 §1](26-READINESS-ASSESSMENT.md)
as follows: 26's COMPLETE and COMPLETE BUT UNVERIFIED are COMPLETE here on the design dimension;
26's PARTIALLY DESIGNED is PARTIAL or NOT DESIGNED here, by what exists; 26's two BLOCKED labels
are BLOCKED here on the decision dimension, and on the design dimension only where the missing
decision or evidence stops the design itself (the write protocol's failure path).

### 1.3 Score

A percentage is given in §2.2 because it was asked for. It is an arithmetic summary of the labels
of §2.1 and of nothing else:

| Element           | Value                                                                                                                                                             |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Label values      | COMPLETE 1; PARTIAL 0.5; BLOCKED 0.25 (work exists that cannot be finished); NOT DESIGNED 0                                                                       |
| Dimension weights | Design depth 30 %; decision completeness 25 %; verification specification 20 %; requirement coverage 15 %; traceability 10 %                                      |
| Why those weights | What a builder lacks most often is a contract or a decision, not a requirement identifier or a row in a matrix                                                    |
| Subsystem weight  | Equal. Each row is a part of the final product the owner asked for; weighting by requirement count would reward areas that happen to have many small requirements |
| What it ignores   | Review (0 % of the post-rejection set is independently reviewed), implementation, measurement, and whether any design is right                                    |

The same labels give other numbers under other choices; §2.2 shows three, so that no single
figure is read as exact.

## 2. The coverage matrix

### 2.1 By subsystem

Columns R, D, Dc, T, V are the five dimensions of §1.1, in that order.

| #   | Subsystem                                                | R        | D            | Dc       | T        | V            | Evidence                                                                                    | Unverified assumptions it rests on                                                                                                               | Outstanding work                                                                                                                                                     |
| --- | -------------------------------------------------------- | -------- | ------------ | -------- | -------- | ------------ | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Terminal, as built                                       | COMPLETE | COMPLETE     | COMPLETE | PARTIAL  | PARTIAL      | 05 §1 `[R]`; 02 §1.1; 20 §4                                                                 | Every performance figure (15 §4); FR-TERM-09 against a reference terminal                                                                        | Acceptance for FR-TERM-04, 07; TS-TERM-03, 04 automated (P1); the sentences of 13 §9.1 replacing raw words in the built view                                         |
| 2   | Terminal workspace: panes, search, keys, layout restore  | PARTIAL  | COMPLETE     | BLOCKED  | COMPLETE | COMPLETE     | 05 §10 to §21; 13 S1, S1.1, §5.2; 20 §4 (TS-TERM-05 to 26), §12.1 (AC-T1, AC-T2); 19 T1, T2 | A free leader chord (OQ-14); the key events the renderer and web view deliver (OQ-45); page reload behaviour (OQ-46); every number of M20 to M24 | DN-36, DN-39, DN-04; a terminal workspace ADR; OQ-14, OQ-45, OQ-46. Requirement coverage stays PARTIAL: restoring agent panes is held by DN-36, not by a requirement |
| 3   | Native launcher                                          | COMPLETE | COMPLETE     | BLOCKED  | COMPLETE | COMPLETE     | 05 §4, §17 (the launch contract); 09 §3.3; 13 S3; 20 §4 (TS-NAT-01 to 05, 10 to 16); 19 F3a | A start recipe per shell family (OQ-07)                                                                                                          | The launcher ADR (DN-02, DN-20, DN-22), for which 05 §17.6 lists what it must state; OQ-07 untested. C1 resolved                                                     |
| 4   | Native registry, supervision, overview                   | COMPLETE | COMPLETE     | BLOCKED  | COMPLETE | PARTIAL      | 05 §5 to §8, §6.1; 13 S2, S2.1; 20 §12.1 (AC-F3b-03, 04); 19 F3b                            | Sampling `/proc` costs nothing noticeable; processes started outside can be recognised                                                           | TS-SES-01, 02 are one line each; blocked only through the launcher ADR                                                                                               |
| 5   | Managed plane, as built                                  | COMPLETE | COMPLETE     | COMPLETE | COMPLETE | COMPLETE     | 10 §1 to §3 `[R]`; the existing suite                                                       | —                                                                                                                                                | None from this set                                                                                                                                                   |
| 6   | Managed Inference independent of hooks (F1b, G-INF)      | COMPLETE | PARTIAL      | BLOCKED  | COMPLETE | PARTIAL      | 10 §5; 19 F1b, G-INF; AC-GINF-01 to 08                                                      | Every interaction of 10 §5.9                                                                                                                     | Experiments E1 to E9 (not authorised, DN-24); DN-03; a security ADR                                                                                                  |
| 7   | Managed work at scale; a second managed agent (F8, F11b) | COMPLETE | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 10 §6; 19 later phases                                                                      | Cost and contention of several runtimes                                                                                                          | DN-08; a design extending ADRs 0012 and 0015; tests                                                                                                                  |
| 8   | Adapters and compatibility evidence                      | COMPLETE | PARTIAL      | PARTIAL  | PARTIAL  | PARTIAL      | 06 §2, §4, §6                                                                               | Provider documentation of 2026-10-12 describes the installed versions (AS6)                                                                      | DN-23; DN-13; the manifest is empty; acceptance for FR-ADP-01 to 05                                                                                                  |
| 9   | Configuration: read-only inventory and watcher (F1)      | COMPLETE | PARTIAL      | PARTIAL  | COMPLETE | COMPLETE     | 07 §1 to §4, §7, §8; 20 §3; 19 F1                                                           | The watcher's caps (15 M16); inotify on the owner's filesystems (AS3); the replication tool's names (OQ-13)                                      | Guard and failure columns for SM-SOURCE (26 §3); DN-10; fuzzing of the parsers specified                                                                             |
| 10  | Write protocol and concurrency (07 §5.4; 28)             | COMPLETE | BLOCKED      | BLOCKED  | COMPLETE | PARTIAL      | 07 §5.4; 28; 30                                                                             | How each provider writes (OQ-31, OQ-35); an atomic exchange on one filesystem (AS8)                                                              | L1, L2, L5 unresolved; DN-21 technically unverified; DN-28 gate closed; DN-31 open; B1; TV-09, TV-16, TV-29 not authorised                                           |
| 11  | Native switches (F2a)                                    | COMPLETE | PARTIAL      | BLOCKED  | COMPLETE | PARTIAL      | 08 §3, §5; 19 F2a                                                                           | Round trip of the owner's files (OQ-29); skill override semantics (OQ-15)                                                                        | DN-01, DN-21, DN-22, DN-23; everything of row 10                                                                                                                     |
| 12  | Protected vault, recovery copies, export (F2v, F2h)      | COMPLETE | PARTIAL      | BLOCKED  | PARTIAL  | PARTIAL      | 27; ADR 0029 (Proposed); 13 S4b, S4c; 19 F2v, F2h                                           | The keyring's password and its behaviour locked (OQ-38, TV-14); swap and core dumps (OQ-39)                                                      | ADR 0029 accepted after review; DN-25 behaviour unverified; DN-31; the contradictions C4 to C6 (§5); acceptance for FR-SYNC-03                                       |
| 13  | Codex mutations (F2b)                                    | COMPLETE | PARTIAL      | BLOCKED  | COMPLETE | PARTIAL      | 08 §3, §5; 10 §5; 19 F2b                                                                    | Where Codex stores a hook switch (OQ-01); reload (OQ-03)                                                                                         | DN-19; G-INF                                                                                                                                                         |
| 14  | Rules, hooks, skills, MCP: every action, authoring (F5)  | COMPLETE | PARTIAL      | BLOCKED  | PARTIAL  | PARTIAL      | 08; 13 S4.4                                                                                 | A place each provider does not read (OQ-23)                                                                                                      | Moving a rule file; MCP switch and add or remove through the provider's writer; authoring (DN-12, DN-13); TS-CFG-17 to 20; acceptance for FR-HRS-05, 07, 09, 10, 12  |
| 15  | OpenCode and Hermes adapters (F4a, F4b)                  | PARTIAL  | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 06 §3.3, §3.4; 17 §2                                                                        | Everything about Hermes beyond one page (OQ-11); OpenCode's skills and server (OQ-12)                                                            | Adapter specifications; DN-04 (JSONC); DN-13; fixtures. OpenCode has no requirement of its own beyond FR-ADP-01                                                      |
| 16  | Profiles, session and global (F3c, F5)                   | COMPLETE | PARTIAL      | BLOCKED  | PARTIAL  | PARTIAL      | 09 §1 to §5, §9; 13 S3, S6; 19 F3c                                                          | What leaving out the user source drops (OQ-02); memory-only files (OQ-34); Codex `-c` (OQ-24)                                                    | DN-20, DN-14; global profiles (FR-PRF-09) designed; acceptance for FR-PRF-02, 03, 05, 06, 07                                                                         |
| 17  | Orchestration and handoff (F11a)                         | PARTIAL  | PARTIAL      | PARTIAL  | PARTIAL  | PARTIAL      | 09 §7, §8; 13 S17; DN-15                                                                    | —                                                                                                                                                | A package format and states; the handoff interface (26 §3); whether orchestration beyond handoff is a requirement (§4, row 3)                                        |
| 18  | Models (F6a, F6b)                                        | COMPLETE | PARTIAL      | BLOCKED  | PARTIAL  | PARTIAL      | 12; 13 S7                                                                                   | What Codex reports per turn (OQ-10); most per-provider facts `[N]`                                                                               | DN-09 (an ADR); DN-38; per-provider evidence; acceptance for FR-MOD-01, 03, 05 to 10; a test for FR-MOD-08. §3.3                                                     |
| 19  | Data, persistence, recovery                              | COMPLETE | COMPLETE     | PARTIAL  | PARTIAL  | PARTIAL      | 14                                                                                          | The recovery tables hold under real interruption (14 §9)                                                                                         | Depends on DN-01, DN-21, DN-25 to DN-28, DN-31; acceptance for FR-DAT-01, 03, 04, 05; a test for FR-DAT-04 (bounds, OQ-22)                                           |
| 20  | Threat model                                             | COMPLETE | COMPLETE     | PARTIAL  | COMPLETE | PARTIAL      | 11                                                                                          | AS5 (no hostile second user); the owner's second account (OQ-41)                                                                                 | The residual risks of 11 §7 confirmed by the owner; DN-22; the stale row of 11 §5.2 (C5); TS-REM for CTL-38                                                          |
| 21  | Interface specification                                  | COMPLETE | PARTIAL      | PARTIAL  | COMPLETE | PARTIAL      | 13 (this pass); 20 §7                                                                       | What a native dialog can show (OQ-43); assistive technology through WebKitGTK (OQ-44)                                                            | DN-32 to DN-38; design contracts and static mockups (13 §13); S15 to S17 beyond entry points; an independent review                                                  |
| 22  | Several computers                                        | COMPLETE | PARTIAL      | PARTIAL  | PARTIAL  | PARTIAL      | 16 §2; 27 §9                                                                                | The replication tool's names and behaviour (OQ-13); that an ignore rule holds (TV-29)                                                            | DN-29 deferred; DN-31; the stale sentence of 16 §2.2 (C7); acceptance for FR-SYNC-03, 04                                                                             |
| 23  | Host-owned terminals (F9)                                | COMPLETE | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 16 §4.1; 19 F9                                                                              | —                                                                                                                                                | DN-05; a protocol, scrollback and several-client rules; TS-HOST-01 to 06 specified. §3.4                                                                             |
| 24  | Remote access and devices (F10a, F10b)                   | COMPLETE | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 16 §3 to §8; 13 S15, S16                                                                    | A private overlay is acceptable (OQ-18)                                                                                                          | DN-05; a threat model; pairing, recovery of a lost device, key rotation and grant expiry as protocols; step-up; TS-REM-01 to 13. §3.4                                |
| 25  | Wyss Agent and Hermes (F12)                              | PARTIAL  | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 17                                                                                          | Both sides known from documents only                                                                                                             | DN-06, DN-11; the comparison redone on an installed Hermes and on the original Wyss's code; a boundary design. §3.5                                                  |
| 26  | Distribution and maintenance (F13)                       | PARTIAL  | NOT DESIGNED | BLOCKED  | PARTIAL  | NOT DESIGNED | 18; ADR 0027                                                                                | —                                                                                                                                                | DN-07; licence, platforms, installation, update, migration, rollback, removal, vulnerability policy, onboarding; TS-DIST-01 to 06. §3.6                              |
| 27  | Performance engineering (P1)                             | COMPLETE | PARTIAL      | PARTIAL  | COMPLETE | PARTIAL      | 15; 13 §6                                                                                   | Every number                                                                                                                                     | OQ-26, OQ-27; budgets for the vault, the write protocol and the interface rules of 13 §6; corpora; a re-read of 15. §3.2                                             |
| 28  | Observability and diagnostics                            | COMPLETE | PARTIAL      | PARTIAL  | PARTIAL  | PARTIAL      | 14 §7; 13 S10, S19, §2.4                                                                    | —                                                                                                                                                | DN-35; the diagnostic report (FR-OBS-05) designed with F13; acceptance for FR-OBS-01, 03, 05                                                                         |
| 29  | Accessibility                                            | COMPLETE | PARTIAL      | PARTIAL  | PARTIAL  | PARTIAL      | 13 §5; 20 §7                                                                                | OQ-44                                                                                                                                            | DN-34; a test for NFR-A11Y-04; contrast computed per token pair (TS-A11Y-02)                                                                                         |

### 2.2 Summary

| Dimension                  | COMPLETE | PARTIAL | BLOCKED | NOT DESIGNED | Score  |
| -------------------------- | -------- | ------- | ------- | ------------ | ------ |
| Requirement coverage       | 24       | 5       | 0       | 0            | 91.4 % |
| Design depth               | 7        | 15      | 1       | 6            | 50.9 % |
| Decision completeness      | 2        | 10      | 17      | 0            | 38.8 % |
| Traceability               | 12       | 17      | 0       | 0            | 70.7 % |
| Verification specification | 4        | 19      | 0       | 6            | 46.6 % |

| Way of counting                                                    | Documentation coverage of the final product                             |
| ------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| The method of §1.3                                                 | **55.0 %** (52.6 % before the terminal pass)                            |
| The same, BLOCKED counted as 0                                     | 51.1 %                                                                  |
| The same, BLOCKED counted as 0.5                                   | 59.0 %                                                                  |
| Five dimensions weighted equally                                   | 59.7 %                                                                  |
| Independently reviewed, of what was written after the first REJECT | **0 %**                                                                 |
| Implemented, of the new subsystems                                 | 0 % (prohibited)                                                        |
| Validated by experiment                                            | One inspection and one probe (30 §6 to §8); every guarantee still `[N]` |

Reading: a little over half of the final product is planned to the depth a builder needs. Requirements
are nearly all written; decisions are the scarcest dimension, and seventeen of twenty-nine
subsystems cannot be finished on paper until the owner decides or an observation exists. The
figure is a documentation measure only.

### 2.3 What changed in the score

The method, the label values and the weights of §1.3 are unchanged. Only the labels of three rows
moved, each because a document now holds what the label's definition asks for.

| Row                   | Dimension    | Before       | After    | Evidence                                                                                                                                                                                |
| --------------------- | ------------ | ------------ | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 2, terminal workspace | Design       | PARTIAL      | COMPLETE | 05 §10 to §16: the tree with its invariants, focus, geometry, PTY size, SM-PANE with guards and failures, the key map and its conflicts, search, titles, layout and migration, recovery |
| 2                     | Traceability | PARTIAL      | COMPLETE | FR-TERM-03, 08, 10 and FR-UX-02 reach tests and acceptance criteria (23; 20 §12.1)                                                                                                      |
| 2                     | Verification | NOT DESIGNED | COMPLETE | TS-TERM-05 to 26 with what each holds and when it fails; AC-T1-01 to 07, AC-T2-01 to 04                                                                                                 |
| 3, native launcher    | Design       | PARTIAL      | COMPLETE | 05 §17: request, generated files reconciled with 09 §3.1 (C1), recipes, identity check                                                                                                  |
| 3                     | Verification | PARTIAL      | COMPLETE | TS-NAT-03 to 05 sharpened; TS-NAT-10 to 16 added                                                                                                                                        |
| 4, native registry    | Traceability | PARTIAL      | COMPLETE | FR-NAT-10, FR-SES-02, 03 have AC-F3b-03, 04                                                                                                                                             |

What did not move, and why: row 2's requirement coverage (restoring agent panes is a decision,
DN-36, not a requirement) and decisions (DN-36, DN-39, DN-04 open; OQ-14 unanswered); row 3's
decisions (the launcher ADR is not accepted); row 4's verification (TS-SES-01, 02 not revisited);
row 1's traceability and verification (FR-TERM-04, 07 still have no acceptance criterion; TS-TERM-03
and 04 are still manual). Overall: 52.6 % to 55.0 %.

## 3. What each kind of gap is

### 3.1 Requirements that are only goals or options

| Requirement                                       | What exists                                     | Missing                             |
| ------------------------------------------------- | ----------------------------------------------- | ----------------------------------- |
| FR-TERM-11 (a terminal outlives the window)       | Consequences (16 §4.1)                          | A host service design (DN-05)       |
| FR-MAN-08, FR-MAN-09                              | Requirements and risks (10 §6)                  | DN-08; an execution adapter design  |
| FR-REM-01 to 09, FR-SEC-09                        | Options and a scheme of grants (16 §5)          | Everything of §3.4 below            |
| FR-AGT-02, 03                                     | An inventory from documents (17)                | DN-06                               |
| FR-DIST-02 to 06, NFR-PORT-02                     | A list of requirements (18 §3, §4)              | DN-07                               |
| FR-SES-04, 05 (handoff)                           | The package's parts (09 §7.1); a sheet (13 S17) | A format, states, an interface      |
| FR-PRF-09 (global profiles)                       | One row (09 §1)                                 | A design (23 §3)                    |
| FR-MOD-06 (switch the model of a running session) | "The user types it" (12 §5)                     | DN-38 (§4)                          |
| FR-MOD-08 (usage and cost)                        | A rule (12 §6)                                  | Any provider figure to show; a test |
| FR-OBS-05 (diagnostic report)                     | A rule (18 §3; 13 S19)                          | A design with F13                   |

### 3.2 Requirements that need the owner

Every open decision, by subsystem; full records in 22 §2.

| Subsystem (row of §2.1)                                               | Decisions open                                                                         |
| --------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Native switches, vault, write protocol (10 to 12)                     | ADR 0029 and ADR 0030 acceptance; DN-01; DN-21; DN-22; DN-23; DN-25 (behaviour); DN-31 |
| Codex (13), G-INF (6)                                                 | DN-19; DN-03; DN-24                                                                    |
| Launcher, registry, profiles (3, 4, 16)                               | DN-02; DN-20; DN-14                                                                    |
| Entries and authoring (14), OpenCode and Hermes (15)                  | DN-12; DN-13; DN-04                                                                    |
| Models (18)                                                           | DN-09; DN-38                                                                           |
| Interface (21), observability (28), accessibility (29), workspace (2) | DN-32 to DN-37; DN-39 (the key model)                                                  |
| Host, remote (23, 24)                                                 | DN-05; OQ-18                                                                           |
| Wyss Agent (25)                                                       | DN-06; DN-11                                                                           |
| Distribution (26)                                                     | DN-07                                                                                  |
| Managed at scale (7)                                                  | DN-08                                                                                  |
| Orchestration (17)                                                    | DN-15                                                                                  |
| Configuration in F1 (9)                                               | DN-10                                                                                  |
| Several computers (22)                                                | DN-29 (deferred by direction)                                                          |

### 3.3 Requirements that need external or technical evidence

| Kind of evidence                                            | Questions                                                                         | Authorised now                                          |
| ----------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------- |
| A real provider on a throwaway home                         | OQ-01 to 06, 15, 19, 23, 24, 28, 31, 34, 35; RS1; E1 to E9                        | **No** (DN-24)                                          |
| Local inspection or probe on the owner's computers          | OQ-13, 29, 30, 33, 38 (second half), 39, 40 (exchange), 41, 42; TV-09, 14, 16, 29 | Only what the owner authorises, case by case; TV-29 not |
| Measurement                                                 | OQ-17, 22, 26, 27; every metric of 15                                             | No (P1 is not started)                                  |
| The framework's documentation, then a spike when authorised | OQ-36, OQ-43, OQ-44, OQ-45, OQ-46                                                 | Documentation reading only                              |
| Reading another repository or installing a program          | OQ-11 (Hermes); the original Wyss's code (17 §8)                                  | No                                                      |
| Asking the owner                                            | OQ-18; OQ-25 (folded into DN-32)                                                  | Yes                                                     |

### 3.4 Capabilities without acceptance criteria

Fifty-five of the 153 rows of 23 §1 and §2 name no acceptance criterion (fifty-eight before the terminal
pass; the first version of this audit said 159 rows, a miscount that included the rows of 23 §3);
nine name no test.
Rows marked Built are accepted by the existing suites `[R]` and are not counted as gaps below.

| Area             | Requirements with no acceptance criterion (not built)                                   |
| ---------------- | --------------------------------------------------------------------------------------- |
| Native, sessions | None since 2026-10-13: FR-NAT-10, FR-SES-02, 03 have AC-F3b-03, 04                      |
| Configuration    | FR-CFG-11, 13; FR-HRS-05, 07, 09, 10, 12; FR-SYNC-03, 04                                |
| Adapters         | FR-ADP-01 to 05                                                                         |
| Profiles         | FR-PRF-02, 03, 05, 06, 07                                                               |
| Models           | FR-MOD-01, 03, 05 to 10                                                                 |
| Observability    | FR-OBS-03, 05                                                                           |
| Security, data   | FR-SEC-03, 05; FR-DAT-01, 03, 04, 05; NFR-SEC-02; NFR-REL-04                            |
| Others           | FR-AGT-03; FR-DIST-01; FR-UX-04 (now TS-UX-07); NFR-MNT-01 to 03; NFR-PORT-02           |
| No test at all   | FR-MAN-08, 09; FR-PRF-09; FR-MOD-08; FR-DAT-04; FR-AGT-02, 03; NFR-PORT-02; NFR-A11Y-04 |

Acceptance criteria are written per phase when the phase is planned (19 §3); these are the rows
that phase planning must fill.

## 4. Capabilities weakened, lost or never stated

Checked against the owner's scope (PRODUCT.md; the brief of 2026-10-13), 29 §2 and §4, and every
domain document. Nothing was found removed without a record **except** the first three rows,
which are now recorded.

| #   | Capability                                                 | What the documents did                                                                                                                                                                     | Silent?                                                      | Now                                                                                                           |
| --- | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| 1   | Changing the model of a running native session (FR-MOD-06) | 12 §5: "The user types the provider's own command. The Console does not type for him." No decision recorded it                                                                             | Yes, until this pass                                         | DN-38, open, with alternatives (13 §12)                                                                       |
| 2   | Workspace restoration of agent sessions                    | FR-TERM-10 and 05 TF4 restore layout with new shells; what a pane that ran an agent becomes was nowhere                                                                                    | Yes, by omission                                             | DN-36, open (13 S1, J22)                                                                                      |
| 3   | Agent orchestration beyond handoff                         | The owner's scope names "agent orchestration and handoff". Handoff has requirements (FR-SES-04, 05); automatic coordination has none, only DN-15 ("none until handoff exists and is used") | Not silent, but held by a decision only, with no requirement | The owner is asked to confirm whether orchestration beyond handoff is a requirement of the final product (§7) |
| 4   | Notifications outside the window                           | 14 §7: "a later option and would be a preference"                                                                                                                                          | No                                                           | DN-35                                                                                                         |
| 5   | Changing a hook while sessions of that agent run           | Refused under strict mode until observed (28 §5)                                                                                                                                           | No (29 §4)                                                   | Unchanged                                                                                                     |
| 6   | Switching a Claude Code MCP server; adding and removing    | Not designed (29 §2, row 7)                                                                                                                                                                | No                                                           | Unchanged; backlog §6.7                                                                                       |
| 7   | Disabling a rule file                                      | Not offered until OQ-23 (08 §4)                                                                                                                                                            | No                                                           | Unchanged                                                                                                     |
| 8   | Restoring on the other computer an entry held in a vault   | Only through a deferred export (DN-29)                                                                                                                                                     | No (29 §4)                                                   | Unchanged                                                                                                     |
| 9   | Recovering a whole earlier file                            | Not offered (29 §4)                                                                                                                                                                        | No                                                           | Unchanged                                                                                                     |
| 10  | Several managed runtimes at once                           | PRODUCT.md "Not now"; DN-08                                                                                                                                                                | No                                                           | Unchanged                                                                                                     |
| 11  | Readable error sentences in the built terminal view        | The built view shows the raw word (`TerminalPanel.svelte:41`) `[R]`, against FR-UX-04 and 13 U7                                                                                            | Not a capability; a defect of what is built                  | 13 §9.1 gives the sentences; to be applied when that view is next changed                                     |

## 5. Contradictions between documents

Found while re-deriving 13; each says which text rules. "Fixed" means corrected in this pass.

| #   | Where                                                                                                                 | What                                                                                                                                                                                                                     | Which rules                                         | State                                                                                        |
| --- | --------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| C1  | 05 §4.4 (the launch sequence) and §8                                                                                  | Generated profile files are written "under Wyss state" and removed at next start. 09 §3.1, 14 §3 and 11 §5.5 say never in the Console's state directory: the per-login runtime directory or a memory-only file, by DN-20 | 09 §3.1 (revised after the reviews)                 | **Fixed** (05 §4.4, §8, §17.3), 2026-10-13                                                   |
| C2  | 13 §1.1 before this pass                                                                                              | "Disabled, kept" meant kept in the provider's file only; "Verified" meant "the provider reported it". 08 §2 has `vault_disabled`, and three labels are never merged (25, IR-10)                                          | 08 §2; 07 §3                                        | **Fixed** (13 §1.1)                                                                          |
| C3  | 13 S5 before this pass                                                                                                | "Applied to settings.json." for every change. 28 §5: a strengthening change reads "written" until observed again                                                                                                         | 28 §5                                               | **Fixed** (13 §1.1.3, S5.1)                                                                  |
| C4  | 07 §8 and 08 §3.2 against 27 §6                                                                                       | The same conditions are spelled `vault_locked`, `vault_no_custody`, `vault_key_missing` in one and `locked`, `no_custody`, `key_missing` in the other                                                                    | Undecided: a naming choice of the vault's interface | Open; 13 §9 maps both spellings to one sentence                                              |
| C5  | 27 §2 ("Location") and 11 §5.2 (row "A replication tool")                                                             | The vault "is refused" under the marker of a replication folder. 27 §9.1 and DN-31: a marker is not proof, and the criterion is open                                                                                     | 27 §9.1; DN-31                                      | Open. Not material to another domain; the vault design is not reopened here                  |
| C6  | 08 §3.3 ("kept a grace period beyond that") and 28 §4.1 case 8 ("its grace period"); 27 §8.3 row "The record expired" | A grace period and an expiry are named. DN-27, decided: no record of any class is removed on age; a restored record is kept until the user deletes it                                                                    | 27 §8.3 retention table; DN-27                      | Open; wording only                                                                           |
| C7  | 16 §2.2                                                                                                               | "On the owner's machines that directory is already excluded from replication `[O]`". 30 §6 and DN-31: it is named by an ignore rule, which is evidence and not a guarantee, and it is inside snapshots                   | 30 §6; DN-31                                        | Open (§6.4)                                                                                  |
| C8  | 18 §3, row "Configuration portability"                                                                                | "A disabled entry travels with the provider's own file". On rung 3 it does not: the vault is per computer (27 §9.2)                                                                                                      | 27 §9.2                                             | Open (§6.6)                                                                                  |
| C9  | 01 §6 (J3, J4, J5 "F2a") and 01 §5 ("Visual … (13 §5)")                                                               | Disabling and restoring one Claude Code hook need F2v (19; 29 §3). The reference for "visual" is 13 §1.1 and S4, not the accessibility section                                                                           | 19; 29                                              | Open; 01 was not re-read by its author (26 §7)                                               |
| C10 | 20 §7, TS-UX-01 before this pass                                                                                      | Enumerated the error words of IF-CONFIG, IF-NATIVE and IF-PROFILE only; 13 S13 names IF-VAULT and the terminal too                                                                                                       | 13 §9                                               | **Fixed** (20 §7)                                                                            |
| C11 | 26 §2 and 29 §5 on the interface                                                                                      | Described the interface as fifteen screens not re-derived                                                                                                                                                                | This pass                                           | **Fixed** (26, 29)                                                                           |
| C12 | 13 U13 ("a view does no work while it is hidden: no timer") against ADR 0007                                          | Hidden terminals keep reading their programs' output, with the built transport's idle polling, so that programs do not block                                                                                             | ADR 0007 (accepted)                                 | **Fixed**: U13 now says terminal transport is not view work (13 §1; 05 §18); its cost is M23 |
| C13 | 14 §3, row "Layout"                                                                                                   | Listed tabs, panes, titles and folders; 05 §15.1 also keeps remembered agents by identifier and forbids content                                                                                                          | 05 §15.1                                            | **Fixed** (14 §3)                                                                            |

## 6. Documentation backlog

Ordered. Each item is documentation only; none authorises implementation, an experiment or a
review. "Acceptance" is what makes the item's documentation complete, not what accepts a phase.

### 6.0 Before any domain: what only the owner can start

| Item                                                                               | Why first                                                                                               |
| ---------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| An independent review of 13 and of this audit, with the security review of 25 §7.1 | Nothing written after the rejections has been read by anyone but its author (26 §5). Not authorised now |
| The owner's answers to DN-32 to DN-38 and to §7                                    | They decide the shape of the next interface work and of 05                                              |

### 6.1 Terminal interaction and keys (05) — written on 2026-10-13, unreviewed

Done in 05 §10 to §21, with C1 and X12. What remains of this item: DN-36, DN-39 and DN-04; OQ-14,
OQ-45, OQ-46; the terminal workspace ADR and the launcher ADR; a review. The table below is kept
as the record of what was asked.

| Field                | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | The split tree as a model: nodes, leaves, ratios, the minimum pane size, limits counted by leaves; focus rules (which pane receives keys, focus after close, after zoom); resize rules (window, divider, keyboard step) and how a PTY's size follows; the key map: the leader chord, every Console action under it, the chords that leave and enter the terminal, conflicts with full-screen programs, shells, the input method and the desktop; search (TF2) with its options; titles (TF5); layout restore (TF4): the file, its version, what it keeps, a folder that is gone, DN-36; a state machine for panes with guards and failure transitions; the guard and failure columns of SM-TERMINAL; the correction of C1; a closed word for an unreadable registry |
| Dependencies         | 13 S1, S1.1, §5.2 (this pass); 15 for the cost at rest; DN-04 for the search add-on                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Owner decisions      | DN-36; DN-04 (search add-on); DN-32 if the palette's place changes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Evidence required    | OQ-14: free chords under the owner's input method and desktop. Reading desktop and input-method documentation is allowed; trying chords in the nested display is a run of the application and needs authorisation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Acceptance           | Every key of the map has its action, its scope and its conflict analysis; TS-TERM-05 to 09 are specified as 20 §3 specifies TS-CFG; SM-TERMINAL and the pane machine have guard and failure columns; 13 S1 and 05 agree line by line; no open `[N]` other than OQ-14                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| Also in 05           | The launcher ADR draft (DN-02, DN-20) is a separate deliverable on the same document; it waits for the owner's word and for nothing technical but OQ-07                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |

### 6.2 Performance engineering (15) — second

| Field                | Content                                                                                                                                                                                                                                                                                                               |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | A full re-read; metrics and budgets for the vault and the write protocol on the paths they touch (nothing on start-up); for the interface rules of 13 §6 (sampling, strict-mode strip, palette index); for panes and search (from 6.1); the benchmark corpora; the decision record for OQ-26 and the method for OQ-27 |
| Dependencies         | 13 §6; 27 §5.2; 07 §5.4; 6.1                                                                                                                                                                                                                                                                                          |
| Owner decisions      | OQ-26 (a timestamp hook in packaged builds); the external latency method (OQ-27)                                                                                                                                                                                                                                      |
| Evidence required    | None to write; every number is P1's, which is not started                                                                                                                                                                                                                                                             |
| Acceptance           | Every metric has a procedure and a placeholder budget marked as such; every interface rule of 13 §6 has a metric; TS-PERF-01 to 13 name their metric                                                                                                                                                                  |

### 6.3 Model management (12) — third

| Field                | Content                                                                                                                                                                                                                                                                                                                                                                          |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | Per-provider evidence for every `[N]` of 12 §3 that documentation can settle; the ADR draft of DN-09 (where the observed model of a managed turn lives); the design of F6b (start flags per provider, validation against a catalogue, temporary profiles); DN-38's design for the alternative chosen; acceptance criteria for FR-MOD-01, 03, 05 to 10; TS-MOD-01 to 06 specified |
| Dependencies         | 13 S7; 09 §3 (temporary profiles); the authority's projection (ADR 0019)                                                                                                                                                                                                                                                                                                         |
| Owner decisions      | DN-09; DN-38; whether managed model selection is wanted at all (a change to SG-12's surface)                                                                                                                                                                                                                                                                                     |
| Evidence required    | Provider documentation (allowed); OQ-10 from the pinned protocol schema (allowed: reading); anything needing a real provider is not authorised                                                                                                                                                                                                                                   |
| Acceptance           | No field of 13 S7's table rests on `[N]` without saying so; FR-MOD rows of 23 have tests and acceptance; the ADR draft exists                                                                                                                                                                                                                                                    |

### 6.4 Remote access (16), with host-owned terminals — fourth

| Field                | Content                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | A threat model of its own (assets, boundaries, threats, controls, tests); the host service: protocol, scrollback bound, several-client rules for input and resize, crash and restart; pairing, lost-device recovery, key rotation and grant expiry as protocols with states; the step-up and what each scope may do to configuration and to the vault; the gateway's audit; the phone client's screens; TS-HOST-01 to 06 and TS-REM-01 to 13 specified; the correction of C7 |
| Dependencies         | F3b's registry (05 §5); 13 S15, S16; 27 §9.4; ADR 0002, 0011                                                                                                                                                                                                                                                                                                                                                                                                                 |
| Owner decisions      | DN-05 (transport, service shape, identity); OQ-18 (an overlay as a prerequisite); the default grants of ND-13 (13 S4c)                                                                                                                                                                                                                                                                                                                                                       |
| Evidence required    | A study of the original Wyss mobile bridge's design (reading, `wyss-agent-legacy/`); the overlay's documentation                                                                                                                                                                                                                                                                                                                                                             |
| Acceptance           | 19 §5's rule for F9 and F10 is met: decisions recorded, threat model, interface contract and acceptance tests written; then reviewed                                                                                                                                                                                                                                                                                                                                         |

### 6.5 Wyss Agent and Hermes (17) — fifth

| Field                | Content                                                                                                                                                                                           |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | Hermes's adapter specification (06 §3.4 completed); the comparison of 17 §4 redone on real material; a boundary design for whichever shape of 17 §6 is chosen; requirements for Wyss Agent itself |
| Dependencies         | 6.3 for models; 06 §4                                                                                                                                                                             |
| Owner decisions      | DN-06; DN-11; consent to install Hermes; access to the original Wyss's repository                                                                                                                 |
| Evidence required    | OQ-11 (Hermes installed with consent and read); the original Wyss's code read and its suite run in its own repository. **Neither is authorised**                                                  |
| Acceptance           | A written decision of the owner among 17 §6, and a design for it with the same depth as 06 §4                                                                                                     |

### 6.6 Distribution and maintenance (18) — last

| Field                | Content                                                                                                                                                                                                                                                                                                                                                                      |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Missing deliverables | Licence and dependency review; a platform list with what a tested build means; installation, update, migration, rollback and removal designs; the vulnerability policy and the provider-version support policy; onboarding (13 S18); the signed evidence manifest (06 §6.5); what a stranger's keyring means for the vault; TS-DIST-01 to 06 specified; the correction of C8 |
| Dependencies         | Everything else; ADR 0027                                                                                                                                                                                                                                                                                                                                                    |
| Owner decisions      | DN-07, after the product is accepted by its first user (18 §7)                                                                                                                                                                                                                                                                                                               |
| Evidence required    | Licences of every dependency (reading)                                                                                                                                                                                                                                                                                                                                       |
| Acceptance           | 18 §3's rows each have a design and a test                                                                                                                                                                                                                                                                                                                                   |

### 6.7 Cross-domain gaps

| #   | Gap                                                                                                                                                            | Where it belongs                   | When                                                              |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- | ----------------------------------------------------------------- |
| X1  | Corrections C4 to C9 (C1 done on 2026-10-13)                                                                                                                   | 05, 07, 08, 11, 16, 18, 27, 28, 01 | With each document's next pass; C5 and C6 with the vault's review |
| X2  | The handoff interface, package format and states                                                                                                               | 09 §7                              | Before F11a                                                       |
| X3  | Global profiles (FR-PRF-09)                                                                                                                                    | 09                                 | With F5's planning                                                |
| X4  | The contract of authoring (DN-12), MCP add and remove through the provider's writer (DN-13), and the Claude Code MCP switch with its definition held while off | 08; an ADR                         | Before F5                                                         |
| X5  | Moving a rule file: destination, importers, crash cases                                                                                                        | 08 §4; 14 §4                       | After OQ-23                                                       |
| X6  | SM-SOURCE guard and failure columns; a restore that waits for the provider's review (Codex trust)                                                              | 07 §7.1; 08 §3.3                   | Before F1, and before F2b                                         |
| X7  | Whether orchestration beyond handoff is a requirement (§4, row 3)                                                                                              | 02; 09 §8                          | The owner's answer                                                |
| X8  | Fuzzing of the parsers and of the vault's record reader specified                                                                                              | 20                                 | Before F1                                                         |
| X9  | `docs/spec/`: one specification per phase                                                                                                                      | New                                | When a phase is planned (26 §6.3)                                 |
| X10 | Managed work at scale and a second managed agent                                                                                                               | 10 §6                              | With DN-08                                                        |
| X11 | 01 and 03 re-read against the current set                                                                                                                      | 01, 03                             | Any time                                                          |
| X12 | A closed word for an unreadable native registry (`registry_unreadable`, proposed in 13 §9.2)                                                                   | 05 §7                              | Done (05 §7.2), 2026-10-13                                        |

### 6.8 Recommended order, and why

1. **Terminal interaction and keys (05)**, with C1 and X12. **Written on 2026-10-13**, unreviewed;
   it waits for DN-36, DN-39 and OQ-14.
2. **Performance (15)**, **next**: small, unblocks P1, and must absorb the rules of 13 §6 and the
   metrics M20 to M24 of 05 §18, among them the idle cost of the built output polling (M23).
3. **Models (12)**: decisions and documentation reading only; it completes the provider-aware
   controls of 13 S7.
4. **Remote access (16)** with host-owned terminals: the largest, decision-bound (DN-05); its
   entry points are fixed in 13 S15, S16.
5. **Wyss Agent and Hermes (17)**: blocked by evidence that is not authorised.
6. **Distribution and maintenance (18)**: by its own rule, after the product is accepted.

The cross-domain items X1 to X12 go with the domain named, or at the owner's word.

## 7. Questions for the owner from this audit

| #   | Question                                                                                             | Why                                                   |
| --- | ---------------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| Q1  | DN-32 to DN-39 (13 §12; 05 §12)                                                                      | They shape every screen and the next domain           |
| Q2  | Is automatic orchestration of agents, beyond a reviewed handoff, a requirement of the final product? | It has no requirement identifier; only DN-15 holds it |
| Q3  | May the review of 25 §7.1 include 13 and this audit?                                                 | 13 has never been a reviewer's priority (26 §5)       |
| Q4  | Is the order of §6.8 the one wanted?                                                                 | It decides the next session's work                    |

## 8. What this audit does not show

- That any label is right: it is one reading, by the author of the set, of that set and of the
  documents written before it, done in one pass.
- That a COMPLETE design would survive review, measurement or implementation.
- Anything about the code beyond the files cited `[R]`.
- A date by which the planning will be complete.
