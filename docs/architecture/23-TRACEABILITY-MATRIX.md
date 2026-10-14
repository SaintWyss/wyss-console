# 23 — Traceability matrix

Requirement → design → component → phase → test → acceptance. Requirements are those of
[02](02-REQUIREMENTS.md); design references are document and section; components are paths of
[04 §6](04-TARGET-ARCHITECTURE.md), existing ones marked `[R]`; phases are those of
[19](19-IMPLEMENTATION-ROADMAP.md); tests those of [20](20-VERIFICATION-AND-ACCEPTANCE.md).
"—" means none is defined yet, which is itself a finding (see §3).

Updated on 2026-10-12 after the owner's policy on secrets, proposed ADR 0029 and the independent
review ([25](25-INDEPENDENT-REVIEW.md)). Rows that changed: FR-CFG-05, 07 to 10, 15; FR-HRS-02,
03; FR-SEC-01, 08; FR-DAT-02, 05; FR-SYNC-03; FR-PRF-04; NFR-REL-01.

Updated again on 2026-10-13 after the owner's decision that a protected private backup is
allowed ([27](27-PROTECTED-RESTORATION-VAULT.md)). Rows that changed: FR-CFG-05, 09, 15;
FR-HRS-02, 03; FR-SEC-08; FR-DAT-05; FR-SYNC-03. Rows added: FR-SEC-10, FR-DAT-06, FR-CFG-17.

Updated a third time on 2026-10-13 after the second pass on the interface specification
([13](13-UX-UI-SPECIFICATION.md)) and the coverage audit ([31](31-PLANNING-COVERAGE-AUDIT.md)).
Rows that changed: FR-TERM-03, 08, 10; FR-NAT-10, 11; FR-CFG-03; FR-HRS-02; FR-PRF-01;
FR-MOD-01, 02, 04, 06; FR-SES-01, 03, 04; FR-OBS-04, 05; FR-SEC-01; FR-DAT-06; FR-REM-01 to 08;
FR-DIST-02 to 06; FR-UX-01 to 06; NFR-A11Y-01 to 03. Interface tests TS-UX-06 to 11 were added
(20 §7).

Updated a fourth time on 2026-10-13 after the terminal pass ([05](05-TERMINAL-AND-NATIVE-SESSIONS.md)
§10 to §21). Rows that changed: FR-TERM-03, 05, 07, 08, 10; FR-NAT-01, 04, 05, 10, 11; FR-SES-02,
03; FR-PRF-04; FR-UX-02. Tests TS-TERM-05 to 26 and TS-NAT-10 to 16 were specified, and acceptance
criteria AC-T1-01 to 07, AC-T2-01 to 04 and AC-F3b-03, 04 were defined (20 §4, §12.1).

Updated a fifth time on 2026-10-13 after the performance pass ([15](15-PERFORMANCE-ENGINEERING.md)).
Rows that changed: NFR-PERF-01 to 08, 10, 11; FR-OBS-06. Tests TS-PERF-15 to 20 and acceptance
criteria AC-P1-01 to 05 were added (20 §9, §12.1).

Updated a sixth time on 2026-10-13 after the model pass ([12](12-MODEL-MANAGEMENT.md)). Rows that
changed: FR-MOD-01 to 10. Tests TS-MOD-01 to 16 and acceptance criteria AC-F6a-01 to 05 and
AC-F6b-01 to 04 were specified (20 §6, §12.2).

Updated a seventh time on 2026-10-13 after the remote pass ([16](16-MULTI-DEVICE-AND-REMOTE.md)).
Rows that changed: FR-TERM-11; FR-SEC-09; FR-SYNC-01, 03, 04; FR-REM-01 to 09. Tests TS-HOST-01 to
12, TS-REM-01 to 18 and TS-SYNC-05, 06 and acceptance criteria AC-F9-01 to 06, AC-F10a-01 to 08,
AC-F10b-01 to 05 and AC-SYNC-01 to 03 were specified (20 §10, §12.3).

Updated an eighth time on 2026-10-13 after the Wyss Agent pass ([17](17-WYSS-AGENT-AND-HERMES.md),
06 §3.4). Rows that changed: FR-AGT-01 to 03. Tests TS-HRM-01 to 10 and TS-AGT-01 to 05 and
acceptance criteria AC-F4b-01 to 04 and AC-F12-01 to 03 were specified (20 §6.1, §12.4).

Updated a ninth time on 2026-10-13 after the distribution pass ([18](18-DISTRIBUTION-AND-EXTENSIBILITY.md)).
Rows that changed: FR-OBS-05; FR-DIST-01, 02 to 06; NFR-MNT-02; NFR-PORT-02. Tests TS-DIST-01 to
15 and acceptance criteria AC-F13-01 to 08 were specified (20 §10.1, §12.5).

## 1. Functional requirements

| Requirement      | Design                                                                | Component                                                    | Phase                        | Tests                                                      | Acceptance                                     |
| ---------------- | --------------------------------------------------------------------- | ------------------------------------------------------------ | ---------------------------- | ---------------------------------------------------------- | ---------------------------------------------- |
| FR-TERM-01       | 05 §1                                                                 | `terminal/` `[R]`                                            | Built                        | TS-TERM-01, TS-REG-01                                      | J1                                             |
| FR-TERM-02       | 05 §1                                                                 | `terminal/`, `features/terminal/` `[R]`                      | Built                        | TS-TERM-01, 02                                             | J1                                             |
| FR-TERM-03       | 05 §2 TF1, §10, §11; 13 S1                                            | `features/terminal/`                                         | T1                           | TS-TERM-05, 10 to 14                                       | AC-T1-01, 04                                   |
| FR-TERM-04       | 05 §1                                                                 | `settings/` `[R]`                                            | Built                        | Existing settings tests                                    | —                                              |
| FR-TERM-05       | 05 §1, §11.3                                                          | `features/terminal/controller.ts` `[R]`                      | Built                        | TS-TERM-03, 11                                             | P1; AC-T1-01                                   |
| FR-TERM-06       | 05 §1                                                                 | `features/terminal/renderer.ts` `[R]`                        | Built                        | TS-TERM-04 (manual today)                                  | P1                                             |
| FR-TERM-07       | 05 §1                                                                 | `terminal/contracts.rs` `[R]`                                | Built                        | TS-TERM-02, 16                                             | —                                              |
| FR-TERM-08       | 05 §2 TF2, §13; 13 S1.1                                               | `features/terminal/`                                         | T1                           | TS-TERM-06                                                 | AC-T1-05                                       |
| FR-TERM-09       | 05 §1                                                                 | xterm `[R]`                                                  | Built                        | TS-TERM-04                                                 | P1                                             |
| FR-TERM-10       | 05 §2 TF4, TF5, §14, §15; 13 S1, J22; DN-36                           | layout store                                                 | T2                           | TS-TERM-08, 09, 19, 20, 26                                 | AC-T2-01 to 03                                 |
| FR-TERM-11       | 16 §4 (IF-HOST, SM-HOST, SM-HTERM, SM-ATTACH, IO1 to IO8, RZ1 to RZ5) | host service                                                 | F9                           | TS-HOST-01 to 12                                           | AC-F9-01 to 06                                 |
| FR-NAT-01        | 05 §4, §17                                                            | `native/`                                                    | F3a                          | TS-NAT-01, 03, 05, 16                                      | AC-F3a-01                                      |
| FR-NAT-02        | 05 §4.2, 09 §5                                                        | `native/`, `desktop/workspace.rs` `[R]`                      | F3a                          | TS-NAT-05                                                  | AC-F3a-01                                      |
| FR-NAT-03        | 09 §3, 12 §5                                                          | `profiles/`                                                  | F3c, F6b                     | TS-PRF-01, TS-MOD-05                                       | AC-F3c-01                                      |
| FR-NAT-04        | 05 §4.3, §17.4                                                        | `native/`                                                    | F3a                          | TS-NAT-02, 04                                              | AC-F3a-01                                      |
| FR-NAT-05        | 05 §5, §6                                                             | `native/`                                                    | F3b                          | TS-NAT-06, 07, 13                                          | AC-F3b-02                                      |
| FR-NAT-06        | 05 §5, 14 §3                                                          | native registry                                              | F3b                          | TS-NAT-06                                                  | AC-F3b-02                                      |
| FR-NAT-07        | 05 §6                                                                 | `native/`                                                    | F3b                          | TS-NAT-08                                                  | AC-F3b-01                                      |
| FR-NAT-08        | 13 S2                                                                 | `features/sessions/`                                         | F3b                          | TS-SES-01                                                  | AC-F3b-01                                      |
| FR-NAT-09        | 09 §6                                                                 | `native/`                                                    | F3a                          | TS-NAT-06                                                  | J8                                             |
| FR-NAT-10        | 05 §6, §6.1; 13 S2                                                    | `native/`                                                    | F3b                          | TS-NAT-09, TS-TERM-25                                      | AC-F3b-03                                      |
| FR-NAT-11        | 05 §4.2; 13 §2.5, S2.1, S3                                            | `features/sessions/`                                         | F3a                          | TS-UX-04, TS-NAT-15                                        | AC-F3a-03                                      |
| FR-MAN-01        | 10 §3                                                                 | `sessions/`, `control/` `[R]`                                | Built                        | Managed suite `[R]`                                        | J9                                             |
| FR-MAN-02        | 10 §2 SG-03                                                           | `adapters/codex/` `[R]`                                      | Built                        | Managed suite                                              | J9                                             |
| FR-MAN-03        | 10 §3.1                                                               | `control/mediation.rs` `[R]`                                 | Built                        | Managed suite                                              | J10                                            |
| FR-MAN-04        | 10 §2 SG-02                                                           | `control/leases.rs` `[R]`                                    | Built                        | Managed suite                                              | —                                              |
| FR-MAN-05        | 10 §1                                                                 | `control/events.rs` `[R]`                                    | Built                        | Managed suite                                              | —                                              |
| FR-MAN-06        | 10 §2 SG-11                                                           | `lifecycle/` `[R]`                                           | Built                        | Lifecycle suite `[R]`                                      | —                                              |
| FR-MAN-07        | 10 §5                                                                 | `adapters/codex/`, `control/`                                | G-INF                        | AC-GINF-01 to 08                                           | AC-GINF-01, 02                                 |
| FR-MAN-08        | 10 §6                                                                 | `control/`                                                   | F8                           | —                                                          | F8                                             |
| FR-MAN-09        | 06 §5                                                                 | `adapters/`                                                  | F11b                         | —                                                          | F11b                                           |
| FR-CFG-01        | 07 §4.1                                                               | `agentconfig/providers/`                                     | F1                           | TS-CFG-01, TS-FS-01                                        | AC-F1-01                                       |
| FR-CFG-02        | 07 §4.2                                                               | `agentconfig/`                                               | F1                           | TS-CFG-01 to 04                                            | AC-F1-01                                       |
| FR-CFG-03        | 07 §3; 13 §1.1.1                                                      | `agentconfig/model.rs`                                       | F1                           | TS-CFG-08, TS-UX-06                                        | AC-F1-04                                       |
| FR-CFG-04        | 07 §7                                                                 | `agentconfig/watch.rs`                                       | F1                           | TS-CFG-07, TS-LONG-02                                      | AC-F1-02                                       |
| FR-CFG-05        | 07 §5, 08 §3, 27                                                      | `agentconfig/mutate.rs`, `agentconfig/vault/`                | F2a, F2v, F2b                | TS-CFG-10 to 12, TS-WR-01 to 20, TS-VLT-17, 18, table test | AC-F2a-01, 02, AC-F2v-01, 02, AC-F2b-01        |
| FR-CFG-06        | 08 §3.4                                                               | `agentconfig/mutate.rs`                                      | F5                           | TS-CFG-17 to 20                                            | F5                                             |
| FR-CFG-07        | 07 §5.4, §5.5                                                         | `agentconfig/mutate.rs`                                      | F2a                          | TS-CFG-10, TS-WR-01 to 05, TS-CONC-01                      | AC-F2a-03, 07                                  |
| FR-CFG-08        | 07 §5.4                                                               | `agentconfig/mutate.rs`                                      | F2a                          | TS-CFG-11, 12, TS-WR-11, 12                                | AC-F2a-02, 07                                  |
| FR-CFG-09        | 07 §5.7, 08 §6, 27 §8                                                 | `agentconfig/mutate.rs`, `agentconfig/vault/`                | F2a, F2v                     | TS-REC-01 to 03, TS-VLT-18                                 | AC-F2a-02, AC-F2v-02                           |
| FR-CFG-10        | 07 §9, 14 §3                                                          | `agentconfig/journal.rs`                                     | F2a                          | TS-SEC-02, TS-REC-01 to 03                                 | AC-F2a-05                                      |
| FR-CFG-11        | 07 §2                                                                 | `agentconfig/providers/`                                     | F1, F2a                      | TS-CFG-01                                                  | —                                              |
| FR-CFG-12        | 07 §2                                                                 | `agentconfig/providers/`                                     | F1                           | TS-CFG-09                                                  | AC-F1-04                                       |
| FR-CFG-13        | 07 §4.3, §4.4                                                         | `agentconfig/`                                               | F1                           | TS-CFG-09, TS-FS-03, 04                                    | —                                              |
| FR-CFG-14        | 10 §4, ADR 0028 §6                                                    | `agentconfig/providers/codex.rs`                             | F1                           | TS-CFG-15                                                  | AC-F1-04                                       |
| FR-CFG-15        | 27 §9.3; 16 §2.3                                                      | `agentconfig/vault/`                                         | After F2v (DN-29)            | TS-VLT-09                                                  | Designed; not offered in the first vault phase |
| FR-CFG-16        | 06 §4.2                                                               | `agentconfig/` (redaction)                                   | F1                           | TS-SEC-01                                                  | AC-F1-01                                       |
| FR-CFG-17        | 27 §3.6; 08 §2, §3.3                                                  | `agentconfig/vault/`                                         | F2h                          | TS-VLT-21 to 23                                            | AC-F2h-01 to 03                                |
| FR-HRS-01        | 08 §3.1                                                               | `agentconfig/providers/`                                     | F1                           | TS-CFG-01                                                  | AC-F1-01                                       |
| FR-HRS-02        | 08 §3.2, §3.3; 27; 13 S4.4                                            | `agentconfig/mutate.rs`, `agentconfig/vault/`                | F2v; rung 2 after RS1; G-INF | Table test, TS-CFG-15, TS-WR-01 to 20, TS-VLT-15 to 18     | AC-F2v-01, AC-GINF-08                          |
| FR-HRS-03        | 08 §3.3; 27 §3.2, §8; ADR 0029 §3                                     | `agentconfig/vault/`                                         | F2v                          | TS-VLT-17, 18, 20, TS-SYNC-03                              | AC-F2v-02, 05                                  |
| FR-HRS-04        | 08 §3.4                                                               | `agentconfig/mutate.rs`                                      | F5                           | TS-CFG-17 to 20                                            | F5                                             |
| FR-HRS-05        | 08 §3.1                                                               | `agentconfig/providers/`                                     | F1, F5                       | TS-CFG-01                                                  | —                                              |
| FR-HRS-06        | 08 §4                                                                 | `agentconfig/providers/`                                     | F1                           | TS-CFG-01                                                  | AC-F1-01                                       |
| FR-HRS-07        | 08 §4                                                                 | `agentconfig/mutate.rs`                                      | F2a, F5                      | Table test                                                 | —                                              |
| FR-HRS-08        | 08 §5                                                                 | `agentconfig/`                                               | F1, F2a                      | TS-CFG-01, table test                                      | AC-F1-01                                       |
| FR-HRS-09        | 08 §5                                                                 | `agentconfig/`                                               | F1, F2b, F5                  | TS-CFG-01                                                  | —                                              |
| FR-HRS-10        | 08 §5                                                                 | `agentconfig/`                                               | F1, F2a                      | TS-CFG-01                                                  | —                                              |
| FR-HRS-11        | 08 §3.5, 09 §4                                                        | `profiles/`                                                  | F3c                          | TS-PRF-01                                                  | AC-F3c-01                                      |
| FR-HRS-12        | 08 §8, C-08                                                           | `agentconfig/providers/codex.rs`                             | F1                           | TS-CFG-14                                                  | —                                              |
| FR-ADP-01        | 06 §4                                                                 | `agentconfig/providers/`                                     | F1                           | TS-CFG-01                                                  | —                                              |
| FR-ADP-02        | 06 §2                                                                 | `agentconfig/model.rs`                                       | F1                           | TS-UX-02                                                   | —                                              |
| FR-ADP-03        | 06 §1                                                                 | —                                                            | F1                           | TS-SEC-03                                                  | —                                              |
| FR-ADP-04        | 06 §6                                                                 | `agentconfig/`                                               | F1, F2a                      | TS-CFG-13                                                  | —                                              |
| FR-ADP-05        | 06 §4                                                                 | `agentconfig/model.rs`                                       | F1                           | TS-CFG-03, TS-UX-01                                        | —                                              |
| FR-PRF-01        | 09 §4.1; 13 S6                                                        | `profiles/`                                                  | F3c                          | TS-PRF-01                                                  | AC-F3c-01                                      |
| FR-PRF-02        | 09 §4.6                                                               | `profiles/`                                                  | F3c                          | TS-PRF-01                                                  | —                                              |
| FR-PRF-03        | 09 §4.3                                                               | `profiles/`                                                  | F3c                          | TS-PRF-02                                                  | —                                              |
| FR-PRF-04        | 09 §2, §3, §3.1; 05 §17.3                                             | `profiles/`, `native/`                                       | F3c, in part after DN-20     | TS-PRF-05, 08, TS-NAT-10 to 12                             | AC-F3c-01, 03                                  |
| FR-PRF-05        | 09 §4.4                                                               | `profiles/`                                                  | F3c                          | TS-PRF-06                                                  | —                                              |
| FR-PRF-06        | 09 §4.5                                                               | `profiles/`                                                  | F3c                          | TS-PRF-07, TS-SEC-06                                       | —                                              |
| FR-PRF-07        | 09 §5                                                                 | `profiles/`                                                  | F3c                          | TS-PRF-01                                                  | —                                              |
| FR-PRF-08        | 09 §4.2                                                               | `profiles/`                                                  | F3c                          | TS-PRF-03                                                  | AC-F3c-02                                      |
| FR-PRF-09        | 09 §1                                                                 | `agentconfig/mutate.rs`                                      | F5                           | —                                                          | F5                                             |
| FR-MOD-01        | 12 §1.1, §4 (MV-01); 13 S2.1, S7                                      | `features/sessions/`                                         | F3b                          | TS-SES-01; TS-MOD-12                                       | AC-F6a-04                                      |
| FR-MOD-02        | 12 §4 (MV-02, MV-03, MV-11); 13 S7                                    | `authority/view.rs` `[R]`, window                            | F6a                          | TS-MOD-05, 06                                              | AC-F6a-02; J11                                 |
| FR-MOD-03        | 12 §1.10, §4 (MV-04, MV-05)                                           | `agentconfig/`                                               | F1, F6a                      | TS-CFG-01; TS-MOD-11                                       | AC-F6a-03                                      |
| FR-MOD-04        | 12 §1.1, §4 (MV-07 to MV-10); ADR 0031 (Proposed); 13 S7              | `adapters/codex/normalize.rs`, `safety.rs` `[R]`, `control/` | F6a                          | TS-MOD-01 to 04, 07 to 09, 12                              | AC-F6a-01, 04; J11                             |
| FR-MOD-05        | 12 §5.3 (SP2); 05 §17.2; 09 §1                                        | `profiles/`, launcher                                        | F6b                          | TS-MOD-14, 15                                              | AC-F6b-01, 02                                  |
| FR-MOD-06        | 12 §5.1, §5.2, §5.4 (SP1, SP4); 13 S7; DN-38                          | window (hint only)                                           | F6b                          | TS-MOD-16                                                  | AC-F6b-03                                      |
| FR-MOD-07        | 12 §1.5, §3 (MC6), §4 (MV-04, MV-10, MV-11), §5.3                     | `agentconfig/`, window                                       | F6a, F6b                     | TS-MOD-11, 14                                              | AC-F6a-03; AC-F6b-01                           |
| FR-MOD-08        | 12 §6 (UR1 to UR7); DN-43                                             | —                                                            | F6a                          | TS-MOD-13                                                  | AC-F6a-05                                      |
| FR-MOD-09        | 12 §4 (rule 4); ADR 0031 option C                                     | `control/`                                                   | F6a                          | TS-MOD-10, 16                                              | AC-F6a-03                                      |
| FR-MOD-10        | 12 §1.2, §1.8                                                         | window                                                       | F6a                          | TS-UX-01; TS-MOD-06                                        | AC-F6a-02, 05                                  |
| FR-SES-01        | 13 S2, §2.5                                                           | `features/sessions/`                                         | F3b                          | TS-SES-01                                                  | AC-F3b-01                                      |
| FR-SES-02        | 05 §5, 10 §3                                                          | `native/`; `control/` `[R]`                                  | F3b                          | TS-NAT-06                                                  | AC-F3b-04                                      |
| FR-SES-03        | 09 §7; 13 §2.3, S15                                                   | window                                                       | F3b                          | TS-UX-01                                                   | AC-F3b-04                                      |
| FR-SES-04        | 09 §7.1; 13 S17                                                       | handoff                                                      | F11a                         | TS-SES-03                                                  | F11a                                           |
| FR-SES-05        | 09 §7.2                                                               | handoff                                                      | F11a                         | TS-SES-04                                                  | F11a                                           |
| FR-SES-06        | 14 §5                                                                 | `native/`; `control/` `[R]`                                  | F3b                          | TS-NAT-06                                                  | AC-F3b-02                                      |
| FR-OBS-01        | 13 S10                                                                | `events/` `[R]`                                              | Built                        | Existing events tests                                      | —                                              |
| FR-OBS-02        | 13 S10                                                                | `agentconfig/journal.rs`                                     | F2a                          | TS-SEC-02                                                  | AC-F2a-05                                      |
| FR-OBS-03        | 13 S10                                                                | native registry                                              | F3b                          | TS-NAT-06                                                  | —                                              |
| FR-OBS-04        | 14 §7; 13 §2.4, §5.3                                                  | window                                                       | Built, F1                    | TS-CFG-07, TS-UX-09                                        | AC-F1-02                                       |
| FR-OBS-05        | 18 §5 (OB10); 13 S19                                                  | —                                                            | F13                          | TS-SEC-02                                                  | AC-F13-08                                      |
| FR-OBS-06        | 15 §1.2, §2, §11                                                      | benchmarks                                                   | P1                           | TS-PERF-01 to 20                                           | AC-P1-01, 02                                   |
| FR-SEC-01        | 11 §4; 09 §3.1; 13 S4c                                                | native dialog                                                | F2a, F3c                     | TS-SEC-05, TS-PRF-08                                       | AC-F2a-04, AC-F3c-03                           |
| FR-SEC-02        | 11 §4                                                                 | `agentconfig/mutate.rs`                                      | F2a                          | TS-SEC-05                                                  | AC-F2a-04                                      |
| FR-SEC-03        | 04 C-01                                                               | `desktop/agentconfig.rs`                                     | F1                           | TS-SEC-04                                                  | —                                              |
| FR-SEC-04        | 06 §4.2                                                               | `agentconfig/`                                               | F1                           | TS-SEC-01                                                  | AC-F1-01                                       |
| FR-SEC-05        | 04 C-08                                                               | `agentconfig/`                                               | F1                           | TS-CFG-14                                                  | —                                              |
| FR-SEC-06        | 04 C-09, 10 §5.4                                                      | `agentconfig/`                                               | F1                           | TS-CFG-09                                                  | AC-GINF-05                                     |
| FR-SEC-07        | 10 §2                                                                 | `control/`, `adapters/codex/` `[R]`                          | Always                       | TS-REG-02                                                  | Every phase                                    |
| FR-SEC-08        | 11 §5; ADR 0029 §2 (proposed)                                         | `agentconfig/mutate.rs`, `journal.rs`, `agentconfig/vault/`  | F2a, F2v                     | TS-SEC-02, TS-WR-12, TS-VLT-01                             | AC-F2a-06, AC-F2v-03                           |
| FR-SEC-09        | 16 §5.2, §5.3                                                         | gateway                                                      | F10a                         | TS-REM-05 to 07, 15                                        | AC-F10a-06                                     |
| FR-SEC-10        | 27 §4 to §7; 11 §5, CTL-28 to CTL-34                                  | `agentconfig/vault/`                                         | F2v                          | TS-VLT-01 to 08, 15, 16                                    | AC-F2v-03, 04                                  |
| FR-DAT-01        | 14 §2                                                                 | —                                                            | Always                       | Review                                                     | —                                              |
| FR-DAT-02        | 14 §4; 07 §5.4                                                        | `agentconfig/mutate.rs`                                      | F2a                          | TS-CFG-12, TS-REC-01 to 03, TS-WR-12                       | AC-F2a-07                                      |
| FR-DAT-03        | 14 §3                                                                 | every store                                                  | Always                       | TS-REC-04                                                  | —                                              |
| FR-DAT-04        | 14 §3                                                                 | every store                                                  | F2a                          | —                                                          | —                                              |
| FR-DAT-05        | 14 §8                                                                 | `agentconfig/`                                               | F2a, F2v                     | TS-REC-05, TS-VLT-13                                       | —                                              |
| FR-DAT-06        | 27 §6, §8; 13 S4b, S4c; 14 §8                                         | `agentconfig/vault/`                                         | F2v                          | TS-VLT-06, 13                                              | AC-F2v-02, 04                                  |
| FR-SYNC-01       | 16 §2.1, §2.5                                                         | `agentconfig/watch.rs`                                       | F1                           | TS-SYNC-01, 05                                             | AC-F1-02; AC-SYNC-01                           |
| FR-SYNC-02       | 16 §2.1                                                               | `agentconfig/`                                               | F1, F2a                      | TS-SYNC-02                                                 | AC-F1-03                                       |
| FR-SYNC-03       | 27 §9; 16 §2.2, §2.4                                                  | `agentconfig/vault/`                                         | F2v                          | TS-VLT-03, 20, TS-SYNC-03, 04, 06                          | AC-SYNC-02                                     |
| FR-SYNC-04       | 16 §2.1                                                               | `agentconfig/watch.rs`                                       | F1                           | TS-SYNC-01                                                 | AC-SYNC-03                                     |
| FR-REM-01 to 08  | 16 §4, §5, §6, §7; 13 S15, S16, §9.7                                  | gateway, host service                                        | F10a                         | TS-REM-01 to 16; TS-HOST-01 to 12                          | AC-F10a-01 to 08; J14                          |
| FR-REM-09        | 16 §8                                                                 | phone client                                                 | F10b                         | TS-REM-17, 18                                              | AC-F10b-01 to 05; J15                          |
| FR-AGT-01        | 06 §3.4, §3.4.1, §3.4.2; 17 §4, §7                                    | `agentconfig/providers/hermes.rs`                            | F4b                          | TS-HRM-01 to 10                                            | AC-F4b-01 to 04                                |
| FR-AGT-02        | 17 §3 to §5                                                           | —                                                            | F12                          | TS-AGT-02                                                  | AC-F12-01                                      |
| FR-AGT-03        | 17 §6, §7                                                             | —                                                            | F12                          | TS-AGT-01, 03 to 05                                        | AC-F12-02, 03                                  |
| FR-DIST-01       | 18 §2                                                                 | everywhere                                                   | Always                       | TS-DIST-01                                                 | AC-F13-01                                      |
| FR-DIST-02 to 06 | 18 §3 to §9; 13 S18, S19                                              | packages; `lifecycle/`                                       | F13                          | TS-DIST-02 to 15                                           | AC-F13-01 to 07                                |
| FR-UX-01         | 13 §2.2, §2.3; DN-32                                                  | `shell/nav.ts` `[R]`                                         | Built                        | Existing shell tests                                       | —                                              |
| FR-UX-02         | 13 S12, §5.2; 05 §12                                                  | window                                                       | T1                           | TS-UX-03, TS-A11Y-01, TS-TERM-07, 17, 23                   | AC-T1-02, 03                                   |
| FR-UX-03         | 13 §1.1                                                               | window                                                       | F1                           | TS-UX-01, 06                                               | AC-F1-04                                       |
| FR-UX-04         | 13 S13, §9                                                            | window                                                       | Always                       | TS-UX-01, 07                                               | —                                              |
| FR-UX-05         | 13 §1 U6, §1.1.6; DN-33                                               | window                                                       | F1                           | TS-UX-02                                                   | AC-F1-05                                       |
| FR-UX-06         | 13 §1 U2, §2.5                                                        | window                                                       | F1                           | TS-UX-04                                                   | AC-F3a-03                                      |

## 2. Non-functional requirements

| Requirement       | Design                                      | Component                        | Phase            | Tests                        | Acceptance                |
| ----------------- | ------------------------------------------- | -------------------------------- | ---------------- | ---------------------------- | ------------------------- |
| NFR-PERF-01, 02   | 15 §2 M01, M02, M21, M25; §9 area 1         | whole application                | P1               | TS-PERF-01 to 12, 15         | AC-P1-01, 03, 05          |
| NFR-PERF-03, 04   | 15 §2 M03 to M05, M18, M23; §6 PR-01, PR-05 | whole application                | P1               | TS-PERF-01 to 12, 13, 14     | AC-P1-01, 04, 05          |
| NFR-PERF-05       | 15 §2 M06, M12, M24; DN-40                  | terminal                         | P1               | TS-PERF-01 to 12, 14         | AC-P1-01, 04              |
| NFR-PERF-06       | 15 §2 M08                                   | terminal                         | P1               | TS-PERF-01 to 12, 20         | AC-P1-01, 05              |
| NFR-PERF-07       | 15 §2 M09 to M11                            | terminal                         | P1               | TS-PERF-01 to 12, 20         | AC-P1-01, 05              |
| NFR-PERF-08       | 15 §2 M13, M20, M33                         | terminal                         | P1               | TS-PERF-01 to 12, 14, 19     | AC-P1-01; AC-T1-01        |
| NFR-PERF-09       | 07 §7, 15 M16                               | `agentconfig/watch.rs`           | F1               | TS-PERF-13, TS-LONG-02       | F1 performance check      |
| NFR-PERF-10       | 15 §2 M14, M31; §10                         | `terminal/` `[R]`                | Built            | TS-PERF-01 to 12, TS-LONG-01 | AC-P1-01                  |
| NFR-PERF-11       | 15 §2 M17, M23, M27                         | whole application                | F3b              | TS-PERF-14, 16               | AC-F3b-01                 |
| NFR-SEC-01        | 04 C-01                                     | capability list `[R]`            | Built            | TS-REG-04, TS-SEC-04         | Every phase               |
| NFR-SEC-02        | 04 C-06, C-11                               | every reader                     | Built, per phase | TS-CFG-05, TS-TERM-02        | —                         |
| NFR-SEC-03        | 10 §2                                       | `adapters/codex/safety.rs` `[R]` | Built            | Managed suite                | —                         |
| NFR-SEC-04        | 11 §5.5                                     | every store                      | Built, per phase | TS-SEC-01, 02                | AC-F2a-06                 |
| NFR-REL-01        | 14 §4; 07 §5.4                              | `agentconfig/mutate.rs`          | F2a              | TS-CFG-12, TS-WR-12          | AC-F2a-07                 |
| NFR-REL-02        | 10 §2 SG-10                                 | `lifecycle/` `[R]`               | Built            | Lifecycle suite              | —                         |
| NFR-REL-03        | 04 card A                                   | `terminal/` `[R]`                | Built            | TS-REG-01                    | Every phase               |
| NFR-REL-04        | 07 §4.2                                     | `agentconfig/`                   | F1               | TS-CFG-03                    | —                         |
| NFR-MNT-01        | 06 §6                                       | `agentconfig/providers/`         | F1               | TS-CFG-01, 13                | —                         |
| NFR-MNT-02        | ADR 0028 §10; 18 §8                         | `Cargo.toml`, `package.json`     | Always           | Review; TS-DIST-14           | AC-F13-05                 |
| NFR-MNT-03        | 20 §1                                       | test suites                      | Always           | TS-REG-05                    | —                         |
| NFR-PORT-01       | 18 §4                                       | `lifecycle/` `[R]`               | Built            | Lifecycle suite              | —                         |
| NFR-PORT-02       | 18 §3.3, §3.4                               | —                                | F13              | TS-DIST-04                   | AC-F13-02                 |
| NFR-A11Y-01 to 03 | 13 §5, §11                                  | window                           | Always           | TS-A11Y-01 to 03, TS-UX-06   | Every phase with a screen |
| NFR-A11Y-04       | 13 §5                                       | `app.css` `[R]`                  | Built            | —                            | —                         |

## 3. Gaps the matrix shows

| Gap                                                                                                                                                                                                                                                                                   | Where it is taken up                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------- |
| FR-MOD-08 (usage and cost) is met by a rule and a test that nothing unreported is shown; no figure exists to show until DN-43                                                                                                                                                         | 12 §6; DN-43                        |
| No test for FR-DAT-04 (bounds on stores): bounds are not final                                                                                                                                                                                                                        | OQ-22                               |
| FR-MAN-08, FR-MAN-09 have no tests: they have no design yet                                                                                                                                                                                                                           | DN-08; F8, F11b                     |
| FR-AGT-02, 03 trace to a decision (DN-06); their tests and acceptance wait for it, and TS-AGT-03 to 05 apply only under option D                                                                                                                                                      | DN-06                               |
| FR-DIST-* trace to one phase whose design (18) waits for DN-07, DN-49 to DN-51                                                                                                                                                                                                        | DN-07                               |
| Several built requirements are held only by a manual check (FR-TERM-05, 06, 09)                                                                                                                                                                                                       | P1 automates them                   |
| NFR-A11Y-04 has no test                                                                                                                                                                                                                                                               | To be added with the next screen    |
| FR-PRF-09 (global profiles) has no test and little design                                                                                                                                                                                                                             | F5's planning                       |
| The action the product was first asked for, disabling one Claude Code hook (FR-HRS-02, 03), has a designed path, the vault, that no reviewer has read; it waits for decisions, not for research                                                                                       | ADR 0029; DN-25 to DN-28; 19, F2v   |
| Undo of an edit or a delete (FR-CFG-06, FR-HRS-04) has a proposed design, a record of the previous bytes in the vault, not yet adopted                                                                                                                                                | DN-12; 27 §3.1                      |
| TS-DIST-01 to 15 are specified (20 §10.1); none can run before a package exists                                                                                                                                                                                                       | 20 §10.1; DN-07                     |
| FR-CFG-15 (export and import) is designed and deferred; a hook held on one computer cannot be restored on the other until it is offered                                                                                                                                               | DN-29                               |
| FR-MOD-06 (switching the model of a running session) is designed only as a hint (SP1); visual switching waits for the owner                                                                                                                                                           | DN-38; 12 §5                        |
| Workspace restoration of panes that ran an agent was nowhere specified                                                                                                                                                                                                                | DN-36; 13 J22                       |
| Automatic orchestration beyond handoff, which the owner's scope names, has no requirement identifier; DN-15 alone holds it                                                                                                                                                            | 31 §4, §7 (a question to the owner) |
| Forty rows of §1 and §2 name no acceptance criterion (forty-four before the distribution pass, forty-five before the Wyss Agent pass, forty-seven before the remote pass, fifty-five before the model pass, fifty-eight before the terminal pass); they are listed by area in 31 §3.4 | Each phase's planning (19 §3)       |

## 4. Reverse check

Every design decision AD-01 to AD-18, every constraint C-01 to C-13 and every guarantee SG-01
to SG-12 is referenced by at least one requirement above, except: AD-17 (measure before
changing technology) and AD-18 (one store, one file), which are rules of method and trace to
phases P1 and "Always". Every control CTL-01 to CTL-40 names its test in
[11 §3.1](11-SECURITY-THREAT-MODEL.md). This check was done by reading, not by a tool; a
script that verifies it is a small task of F1 `[N]`.

## 5. Owner final-product expansion — direct traceability (not implemented)

Historical rows in §1 and §2 remain unchanged unless superseded by an
accepted ADR. New targets in [02 §4](02-REQUIREMENTS.md) trace as follows;
a named test is a *specification*, not an executed check. The exact
pass/fail criteria are in [20 §14](20-VERIFICATION-AND-ACCEPTANCE.md).
DN-05 and DN-31 remain unresolved; a design link does not equal acceptance.

| Requirement | Design contract | Candidate phase | Test | Acceptance |
| --- | --- | --- | --- | --- |
| FR-ORC-01 | 34 §1–4 | F11b | TS-ORC-01 | AC-FINAL-01 |
| FR-ORC-02 | 34 §1–4 | F11b | TS-ORC-03 | AC-FINAL-02 |
| FR-ORC-03 | 34 §3–4 | F11b | TS-ORC-02 | AC-FINAL-03 |
| FR-ORC-04 | 34 §2/4 | F11b | TS-ORC-05 | AC-FINAL-04 |
| FR-ORC-05 | 34 §3/5 | F11b | TS-ORC-07 | AC-FINAL-05 |
| FR-ORC-06 | 34 §2/4 | F8/F11b | TS-ORC-04 | AC-FINAL-06 |
| FR-ORC-07 | 34 §1/6 | F11b | TS-ORC-09 | AC-FINAL-07 |
| FR-ORC-08 | 34 §2/7 | F11b | TS-ORC-13 | AC-FINAL-08 |
| FR-REM-10 | 35 §7 | F10b | TS-HYB-05 | AC-FINAL-09 |
| FR-REM-11 | 35 §3/6 | F10a | TS-HYB-01 | AC-FINAL-10 |
| FR-REM-12 | 35 §6 | F10a | TS-HYB-03 | AC-FINAL-11 |
| FR-REM-13 | 35 §4 | F9 | TS-HYB-06 | AC-FINAL-12 |
| FR-REM-14 | 35 §5/6 | F9/F10a | TS-HYB-09 | AC-FINAL-13 |
| FR-REM-15 | 35 §7 | F10b/F5 | TS-HYB-11 | AC-FINAL-14 |
| FR-REM-16 | 35 §6/7 | F10a/F10b | TS-HYB-04 | AC-FINAL-15 |
| FR-REM-17 | 35 §4/7 | F9 | TS-HYB-14 | AC-FINAL-16 |
| FR-TERM-12 | 35 §5 | T1/P1 | TS-FINAL-01 | AC-FINAL-17 |
| FR-TERM-13 | 35 §5 | F9 | TS-HYB-08 | AC-FINAL-18 |
| FR-TERM-14 | 34 §5; 35 §4 | T2/F9 | TS-FINAL-02 | AC-FINAL-19 |
| FR-TERM-15 | 35 §5 | F9/P1 | TS-FINAL-03 | AC-FINAL-20 |
| FR-MOD-11 | 33 RC-05; 12 §5 | F6b | TS-FINAL-04 | AC-FINAL-21 |
| FR-MOD-12 | 33 RC-05; ADR 0031 | F6a/F6b | TS-FINAL-05 | AC-FINAL-22 |
| FR-MOD-13 | ADR 0031 §3–4 | F6a | TS-FINAL-06 | AC-FINAL-23 |
| FR-MOD-14 | 12 §5; 32 DN-38 | F6b | TS-FINAL-07 | AC-FINAL-24 |
| FR-MOD-15 | 33 RC-05; 12 §6 | F6a | TS-FINAL-08 | AC-FINAL-25 |
| FR-UX-07 | 32 DN-32; 13 §2 | F3b/F10a | TS-FINAL-09 | AC-FINAL-26 |
| FR-UX-08 | 32 DN-33; 13 §2 | F1/F5 | TS-FINAL-10 | AC-FINAL-27 |
| FR-UX-09 | 32 DN-34; 13 §13 | F13/T1 | TS-FINAL-11 | AC-FINAL-28 |
| FR-UX-10 | 32 DN-35; 33 RC-13 | F10a | TS-FINAL-12 | AC-FINAL-29 |
| FR-UX-11 | 32 DN-39; 05 §12 | T1 | TS-FINAL-13 | AC-FINAL-30 |
| FR-UX-12 | 32 DN-37; 27 | F2v | TS-FINAL-14 | AC-FINAL-31 |
| FR-PRF-10 | 32 DN-20; 09 §3 | F3a/F3c | TS-ORC-08 | AC-FINAL-32 |
| FR-PRF-11 | 32 DN-14; 09 §3 | F3c | TS-FINAL-15 | AC-FINAL-33 |
| FR-CFG-18 | 32 DN-12; 33 RC-12 | F5 | TS-FINAL-16 | AC-FINAL-34 |
| FR-CFG-19 | 32 DN-13; 33 RC-12 | F4b/F5 | TS-FINAL-17 | AC-FINAL-35 |
| FR-SEC-11 | 32 DN-22; 33 RC-02 | F5 | TS-FINAL-18 | AC-FINAL-36 |
| FR-AGT-04 | 17 §6; 33 RC-04 | F12 | TS-ORC-15 | AC-FINAL-37 |
| FR-AGT-05 | 17 §1; 32 DN-11 | F12 | TS-FINAL-19 | AC-FINAL-38 |
| FR-OBS-07 | 33 RC-05/RC-13; 12 §6 | F6a | TS-FINAL-20 | AC-FINAL-39 |
| FR-DIST-07 | 32 DN-07; 36 §4 | F13-personal | TS-FINAL-21 | AC-FINAL-40 |

### 5.1 Items that cannot be counted as verified

- External review of `wyss-control` concurrency and the separate managed-model-selection ADR.
- Acceptance of ADR 0031, reroute classifier protocol observations, analytics privacy/retention policy.
- Remote transport DN-05, phone keying, gateway/pairing, relay confidentiality, host screen parser.
- Provider write protocol L1/L2/L5, DN-31 snapshot and sync exclusion, vault keyring and B1.
- Hermes process containment and effectful-tool proof (OQ-55–57).
- All performance numbers and reference device runs; independence of post-REJECT review.

## 6. Existing-requirement gap crosswalk — handoff, adapters, UX, analytics

[37](37-HANDOFF-ADAPTERS-UX-AND-DATA-CONTRACTS.md) gives
candidate contract detail to existing requirements whose trace was
too shallow. Acceptance criteria and **NOT RUN** status are in
[20 §15](20-VERIFICATION-AND-ACCEPTANCE.md).
These rows supplement the historical matrix; they do not
retroactively change accepted ADRs or implemented evidence.

| Requirement | Candidate contract | Tests | Acceptance |
| --- | --- | --- | --- |
| FR-SES-04 | 37 §1 explicit handoff and state guards | TS-HAND-01/02/03/05 | AC-HAND-01 |
| FR-SES-05 | 37 §1.2 plane boundary and new session | TS-HAND-04/05 | AC-HAND-02 |
| FR-ADP-01..05 | 37 §2 exact-version descriptors for OpenCode and Hermes | TS-ADP-OC-01..09; TS-ADP-HR-01..05 | AC-ADP-OC-01; AC-ADP-HR-01 |
| FR-HRS-04/05 | 37 §3 native authoring, review and failure states | TS-CFG-17..20 (original); TS-CFG-21..24 (additional) | AC-CFG-AUTH-01; AC-CFG-AUTH-02 |
| FR-HRS-09 | 37 §3 closed provider-native operations | TS-CFG-24 | AC-CFG-AUTH-02 |
| FR-MOD-08/09 | 37 §5 source-grain metadata and observed model events | TS-MET-01..08 | AC-MET-01 |
| FR-UX-01/02/03/05 | 37 §4/6 Android screens and desktop capability segregation | TS-UIX-01..07 | AC-UIX-01 |
| FR-REM-09 | 37 §4 native Android ten screen states | TS-UIX-04/07 | AC-UIX-01 |
| NFR-A11Y-02/03 | 37 §6 accessible themes and state | TS-UIX-01/03 | AC-UIX-01 |
| NFR-A11Y-04 | 37 §6 reduced-motion acceptance | TS-UIX-08 | AC-UIX-02 |
| FR-OBS-01 | 20 §15 event reader acceptance | TS-OBS-01 | AC-OBS-01 |
| FR-OBS-03 | 20 §15 native lifecycle acceptance | TS-OBS-02 | AC-OBS-02 |
| FR-DAT-04 | 20 §15 persistence/quota acceptance | TS-DAT-04 | AC-DAT-04 |

Acceptance IDs are *definitions of required outcomes*, not evidence
that their tests were carried out. Release readiness still requires
the independent review and blocked technical evidence of 36.

## 7. REJECT corrective crosswalk (NOT RUN)

The partially independent review's IR-01–15 findings
in [reviews/2026-10-13-independent-post-owner-review.md](reviews/2026-10-13-independent-post-owner-review.md)
are mapped into
[42](42-POST-REJECT-CORRECTIVE-PLAN.md) and
[20 §16](20-VERIFICATION-AND-ACCEPTANCE.md).
These are **additional acceptance criteria for the existing
final-product target**, not evidence that runtime tests passed.

| Finding | Affected targets / safety domain | Test / acceptance |
| --- | --- | --- |
| IR-01 | FR-ORC-01/02/04/06, FR-SEC-11; same-UID auth | TS-SEC-OL3-01 / AC-IR-01 |
| IR-02 | FR-REM-10/13/16; LP4 and revocation | TS-SEC-REM-01/02 / AC-IR-02 |
| IR-03 | FR-ORC-01/02/03, FR-SES-04; handoff and launch | TS-SEC-OL3-02 / AC-IR-03 |
| IR-04 | FR-ORC-06, FR-MOD-11, FR-MAN-09; managed serialization | TS-SEC-MAN-01 / AC-IR-04 |
| IR-05 | FR-AGT-04; original Wyss policy | TS-SEC-AGT-01 / AC-IR-05 |
| IR-06 | FR-REM-12/16; gateway idempotence | TS-SEC-REM-03 / AC-IR-06 |
| IR-07 | FR-REM-17, FR-TERM-14; native orphan effects | TS-SEC-HOST-01 / AC-IR-07 |
| IR-08 | FR-REM-13, FR-ORC-05; host idle/keepalive | TS-SEC-HOST-03 / AC-IR-08 |
| IR-09 | FR-TERM-13/15, FR-REM-14; parser containment | TS-SEC-HOST-02 / AC-IR-09 |
| IR-10 | FR-REM-10/16; Android presence key | TS-SEC-MOB-01 / AC-IR-10 |
| IR-11 | FR-ORC-05, FR-REM-16; device revocation | TS-SEC-ORC-01 / AC-IR-11 |
| IR-12 | FR-CFG-18/19, FR-REM-15, FR-DAT-06; vault locked | TS-SEC-CFG-01 / AC-IR-12 |
| IR-13 | FR-HRS-04, FR-CFG-18/19; original TS-CFG preserved | TS-SEC-TRACE-01 / AC-IR-13 |
| IR-14 | Architecture/mockup identifier namespaces | TS-SEC-TRACE-02 / AC-IR-14 |
| IR-15 | DN-31, provider write safety | TS-SEC-EVID-01 / AC-IR-15 |

TS-CFG-17..20 remain authored under 20 §3.4;
the new negative tests from 37 §3 use TS-CFG-21..24.
Their approvals are respectively AC-CFG-AUTH-01
and AC-CFG-AUTH-02. No source implies these tests
were run or passed.

## 8. Codex/Sol High REJECT consistency delta (2026-10-14)

The latest adversarial audit
[reviews/2026-10-14-post-reject-corrections-external-audit.md](reviews/2026-10-14-post-reject-corrections-external-audit.md)
returns **REJECT**, not plan acceptance.
`FR-ORC-06 / TS-ORC-04 / AC-FINAL-06`
and `TS-ORC-14` are a **future, ADR-gated**
acceptance track for DN-08=A, not proofs of
today's multi-runtime capability. The accepted
ADR 0012/0015 base must first pass
`TS-SEC-MAN-01`: one shared App Server
and one globally active managed turn, with
honest queued/denied behaviour during OL3.

`FR-HRS-04/05, FR-CFG-18/19` need
all four original `TS-CFG-17..20`
positive/negative authoring + exact-undo
acceptances defined in [20 §16.1](20-VERIFICATION-AND-ACCEPTANCE.md).
`AC-CFG-AUTH-01` is *defined there*, and
`AC-CFG-AUTH-02` remains the additional
four negative cases `TS-CFG-21..24`.

`FR-REM-12/16` must also meet the
revised `TS-HYB-03/AC-FINAL-11`:
acknowledgement ambiguity fails as
`OUTCOME_UNKNOWN` with **no byte replay**,
not an invented global exactly-once guarantee.
Neither this trace nor the PR's presence
means any test or ADR passed.

## 9. CachyOS delegated Docker threat trace (2026-10-14)

These map **new product requirements**, not approved architecture.
Based on [47](47-CACHYOS-DOCKER-PRIVILEGE-AND-SANDBOX-DECISION.md),
the source [native fixture report](reviews/2026-10-14-cachyos-native-feasibility.md)
and proposed [test criteria §17](20-VERIFICATION-AND-ACCEPTANCE.md).

| Requirement | Contract | Phase gate | Test | Acceptance |
| --- | --- | --- | --- | --- |
| FR-SEC-12 | 47 §§2–3; 11 T-50 | G-INF / A trusted principal and approval ADR | TS-DOCKER-01, TS-DOCKER-02 | AC-DOCKER-01 |
| FR-SEC-13 | 47 §4; 43 B profile | F11b / OL3 native isolation, authority review | TS-DOCKER-03 | AC-DOCKER-02 |
| FR-PRF-12 | 47 §4; ADR 0018 credential capability; 43 auth | F3c/F11b / exact provider support | TS-DOCKER-04 | AC-DOCKER-03 |
| FR-REM-18 | 47 §§2/6; 35 LP4 remote | F10a/F10b / explicit host-root delegation risk | TS-DOCKER-06 | AC-DOCKER-04 |
| **Owner-only environmental decision (not FR)** | 47 §3 D1 vs D2, no automatic migration | Owner consent + explicit rollback | TS-DOCKER-05 | AC-DOCKER-05 |

All test results are **NOT RUN**. Since ordinary
native U may have Docker root-level host access, a
separate-UID A alone cannot satisfy FR-SEC-12.
Accepting a proposed interface without the actual
OS/daemon security boundary is not a PASS.

