# Engineering review and model execution

Use the current source and canonical documents together. The goal is precise, testable
decisions and small reviewable implementations. Model names do not establish competence,
independent review, test coverage or product acceptance.

## Evidence and document ownership

| Claim                    | Required evidence                                                                           | Canonical location                                    |
| ------------------------ | ------------------------------------------------------------------------------------------- | ----------------------------------------------------- |
| Target behavior          | User journey, explicit invariant and observable acceptance criterion.                       | REQUIREMENTS                                          |
| Present implementation   | Exact source path/symbol and active entry/feature/gate, not a stale comment.                | ARCHITECTURE and STATUS                               |
| Confirmed discrepancy    | Both conflicting definitions, affected route and what was/was not reproduced.               | STATUS; remaining correction in ROADMAP               |
| Engineering verification | Actual command, build/environment, result, negative control and coverage limit.             | Run/PR artifacts; concise STATUS summary              |
| Real qualification       | Exact provider/build or hardware, normal environment and relevant journeys.                 | STATUS and narrowly necessary qualification reference |
| Design choice            | Chosen rule, reason, rejected alternative when material, compatibility and evidence needed. | DECISIONS and owning contract                         |

Source comments, tests and prose can each be stale. Executable behavior is evidence of
what exists, not automatic proof of what is correct. Resolve a conflict against accepted
requirements and observable behavior; never edit the requirement merely to make code pass.
Search results identify locations; reading a header is not reviewing a whole subsystem.

## Review order and completion

| Order | Review                    | Required result                                                                                                                                                    |
| ----- | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 0     | Product/state baseline    | Pin source; retain complete personal scope; separate implementation, qualification and adoption.                                                                   |
| 1     | Architecture              | Trace actual composition, process/state owners, permissions, storage, effects and failure paths. Resolve inaccurate claims; specify remaining corrections.         |
| 2     | Requirements and journeys | Check every FP/AC group for missing user/error/recovery states; split compound criteria into independently observable cases. Feed findings back into architecture. |
| 3     | Domain contracts          | Close exact input/output/errors, limits, version/gate, invariants and migration/recovery decisions for the next slice.                                             |
| 4     | Verification              | Map each case to an existing test, new justified test or real acceptance procedure; name evidence still missing.                                                   |
| 5     | Implementation roadmap    | Publish only unfinished outcomes, dependencies and executable work. Remove completed tasks; update state in its owner document.                                    |

Repeat architecture/requirements/contracts within each domain when evidence changes.
Do not freeze speculative details for distant transport/provider work before its feasibility
is known. Keep the product roadmap current while refining its next actionable slices.

## Work states

- **READY:** scope, design, dependencies, invariants and verification are specified;
  no unresolved choice changes the implementation. Existing unrun engineering tests do not
  make the task verified; the implementer must run the relevant checks.
- **DISCOVERY:** research, source tracing, benchmark or controlled experiment must resolve
  a named question. Its deliverable is a decision/evidence, not production enablement.
- **BLOCKED:** a named external capability, qualification or prior task is missing. Record
  the exact condition; do not invent a safe default or an automatic alternate execution mode.

Implemented, tested, qualified, deployed and accepted are separate dimensions, not a single
progress percentage. Passing fixtures must never be counted as missing real-device evidence.

## Assignment by role

| Role                  | Intended assignment                                                                          | Required output                                                                                |
| --------------------- | -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| Design and evaluation | Sol, or another model qualified for the problem's reasoning/review needs.                    | Source-based diagnosis, selected architecture/contract, residual uncertainty and a READY task. |
| Implementation        | Haiku, Luna or Terra, assigned according to the slice's complexity and demonstrated results. | Scoped code, relevant checks, exact evidence and canonical-document updates.                   |
| Verification          | A separate review pass with sufficient domain competence.                                    | Requirement/diff review, adversarial cases and independent reproduction where required.        |

A different turn/model is useful separation of work but does not by itself satisfy an
independent security review requirement. Evaluate the output; do not assume correctness
from the model tier or promise failure-free implementation.

## Frontier-model responsibilities

Trace code and required behavior, compare realistic alternatives, select and justify the
smallest adequate design, write precise contracts and define negative/recovery cases.
Resolve technical choices within the delegated product scope; do not repeatedly ask the
owner to decide implementation details. Preserve uncertainty where facts require an actual
experiment, credential or device. A delegated decision cannot manufacture independent
security review or provider evidence, and does not itself make a closed production gate true.

Before handing off, record: stable task ID; objective; relevant source/contract/FP/AC;
dependencies; chosen change and boundaries; invariants; error/recovery behavior; necessary
checks and evidence; completion condition. Estimate risk from effects and uncertainty,
not from the model assigned. Avoid boilerplate tests that only repeat the implementation.

## Implementer responsibilities

Read only the current task plus its linked owners first, then inspect affected source.
Implement the agreed behavior in a small change. Choose local names/helpers freely while
preserving public contracts, scope and invariants. Reuse existing tests; add tests for real
behavioral risks. Run appropriate checks, report exact results and update the canonical
documents. Do not enable a blocked provider, raise capacity or migrate a live store as a
convenience for completing another task.

An unexpected dependency, contract conflict or required scope expansion returns to design
with source evidence and a concrete recommendation. Routine local choices do not require
escalation. Do not invent requirements, broaden permissions or reinterpret an unknown effect
as failure/no-effect. A reviewer checks the final diff and behavior against the contract;
the author model's confidence is not evidence or independent security review.

## Keeping the repository compact

ARCHITECTURE owns the component map; contracts own exact domain rules; REQUIREMENTS owns
behavior; STATUS owns current evidence/gaps; ROADMAP owns pending work; DEVELOPMENT owns
commands. This page owns execution discipline. Link those owners instead of copying them.
Keep detailed next-slice specifications inline in ROADMAP or the owning contract. Retire
completed task detail and keep raw reports/logs outside the source tree, except necessary
runtime provenance or executable inputs. No new chronological audit archive.
