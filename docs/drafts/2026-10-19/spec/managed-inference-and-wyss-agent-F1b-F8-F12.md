# F1b / G-INF / F8 / F11b / F12 — Managed security and Wyss Agent (proposed)

**Status:** Research/specification only; no changes to the Managed Inference authority are approved merely by this document. Sources: architecture 11, 17, 19 and current accepted authority ADRs. **F12 is a decision phase, not authorization to implement a new Wyss Agent.**

## 1. Native and Managed planes are different

Native sessions use installed CLIs in PTYs with provider-specific config, hooks and permissions. Managed work executes under the separate authority, with its attestations, narrow protocol, policy and approvals. Merely displaying or editing configuration in the desktop does not grant managed authority.

Never use a terminal fallback to bypass managed failure, present a desktop prediction as an authority security verdict, weaken attestation, impersonate a managed session or inject arbitrary model/agent/command data through UI.

## 2. F1b: eight canonical research experiments

The canonical roadmap requires E1–E8 in disposable provider homes, with a written contract covering architecture 10 §5.6. **Read the local canonical catalogue before executing: do not fabricate experiment definitions.**

Each experiment record should include:
- Exact canonical hypothesis/ID, provider version and tested protocol/schema.
- Source/test home, namespace, policy and credential isolation.
- Baseline controls and deliberately changed variable.
- Bounded, reproducible procedure, no owner credential copied.
- Safe observations and attestation evidence with no secret material.
- Falsification/conclusion and what DN-03 contract clause it affects.
- A deterministic regression fixture when feasible.

Compare I1, I2, I4 and "not now" from results. The current managed hook block remains if no candidate meets all accepted safety guarantees.

## 3. G-INF gate

Prerequisites: F1b complete, owner chooses DN-03, accepted security ADR, F2b ready and independent review. Implement only the contract the ADR accepts. Verify all canonical AC-GINF-01–08, the complete existing managed isolation suite and real provider behavior.

Authority attestation must bind the actual runtime generation, policy and provider process. A native hook being enabled is not evidence that managed execution is allowed. Migration and rollback must restore the previous blocking behavior without accepting stale grants.

No acceptance from UI-only tests, guessed hook state or undocumented provider behavior.

## 4. F8: concurrent managed runtimes

Decision DN-08 must define one-runtime/multi-turn vs pool vs per-session independent runtime; compare resource cost, recovery, credentials, isolated identity, policy bindings and failure propagation.

Concurrency is not permission to share authority attestations across sessions. Each generation maintains its own trusted runtime process, policy version, isolation evidence, cancellation, resource limits and audit identity.

Required regressions: two simultaneous turns, one runtime crash while another runs, cancellation isolation, wrong-generation response rejection, secret/memory isolation and clean bounded restart. Schema migration, explicit backup and rollback are required if persisted authority state changes.

## 5. F11b: second managed agent

Separate from F11a's reviewed handoff. Needs F8 and an accepted adapter-specific ADR; provider must offer an attestable protocol compatible with the authority. A native Hermes/OpenCode CLI cannot simply be reclassified as managed. Unsupported security contracts mean no managed adapter, even if a process can launch.

## 6. F12: choose Wyss Agent strategy

After F4b, compare **actual installed Hermes** and original Wyss code/behavior. Possible directions include upstream Hermes configuration, constrained fork, growing original Wyss or separate systems. None is accepted yet (DN-06).

Decision must weigh deterministic policy, risk assignment, permissions, model gateway, memory, scheduler, personal data boundaries, home automation, mobile connection, maintenance and migration. Preserve the stronger verified safety boundary. Do not treat archived project claims as runtime proof.

Outputs: comparison evidence, selected strategy and accepted ADR if implementation is desired. Product name Wyss Console remains distinct from Wyss Agent.

## 7. Review and done criteria

- F1b: E1–E8 documented with controls and outcomes, not just ideas.
- G-INF: owner ADR plus focused external security review; all managed attestations intact.
- F8: isolated concurrent managed turns verified, failure and downgrade paths proved.
- F11b: attestable managed adapter operating under its own approved contract.
- F12: an owner-selected strategy based on live provider/code evidence (a written decision, not a code release).

Do not unlock a dependent managed feature while any prerequisite remains unresolved.
