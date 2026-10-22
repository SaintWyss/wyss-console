# Documentation map

Choose the page for your task; routine work does not require reading the whole set.

## Active canonical documentation

| Question / information owner                                      | Canonical page                                                    |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| Why this product; approved scope and requirement entry point      | [Product vision and requirements](../PRODUCT.md)                  |
| What users do; screens, states, errors and FP acceptance criteria | [Functional behaviour and workflows](functional-specification.md) |
| Who owns state; interfaces, persistence and security boundaries   | [Architecture](architecture.md)                                   |
| What is implemented and what evidence proves it                   | [Implementation and acceptance status](implementation-status.md)  |
| What remains; dependencies, milestones and owner decisions        | [Roadmap](roadmap.md)                                             |
| How to develop, validate and maintain these docs                  | [Development and maintenance](../CHECKPOINT.md)                   |

For an engineering task, read status and the relevant roadmap item, then only the
functional requirement and architecture contract it touches. Product goals never
supersede accepted security ADRs; planned behaviour is not implementation evidence.

## References on demand

[Provider matrix](provider-matrix.md): versioned capability detail.
[Traceability](traceability.md): all 31 FP/AC and 208 legacy requirements, reconciliation
and dated Phase1 checks. [Inventory](documentation-inventory.md): 519 baseline artifacts.
[Independent review](independent-review.md): dated findings and resolutions.

[ADRs](../adr/) retain their individual Accepted/Proposed status and binding decisions.
[Detailed architecture](../architecture/00-INDEX.md), [acceptance reports](../acceptance/),
[research](../research/), [drafts](../drafts/2026-10-19/README-INTEGRATION.md) and
[history](../history/) are lookup material, not another current roadmap. Dated evidence
remains valid for its recorded slice; current interpretation belongs to the status page.

Update the canonical owner before creating a document; see the
[maintenance policy](../CHECKPOINT.md#documentation-maintenance).
