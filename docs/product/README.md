# Final personal product — Phase 1 consolidation

Baseline: `main` at `e480d479fdbd8a215612b1d809862a62aa914eb5`, 2026-10-22.
This is a documentation deliverable. No implementation, service operation, provider run,
credential access, installed migration or product acceptance is performed by this phase.

Start with [PRODUCT](../PRODUCT.md), then:

| Document                                                | Purpose                                                                        |
| ------------------------------------------------------- | ------------------------------------------------------------------------------ |
| [Functional specification](functional-specification.md) | End-to-end target behaviour, screens, journeys, states, acceptance cases       |
| [Architecture](architecture.md)                         | Ownership, identity, security boundaries, persistence and interface deltas     |
| [Provider matrix](provider-matrix.md)                   | Claude Code and Codex, separately by mode, capability and evidence             |
| [Implementation status](implementation-status.md)       | Source/evidence gap register at the baseline                                   |
| [Roadmap](roadmap.md)                                   | Dependency order, delivery gates, rollback, outstanding decisions              |
| [Traceability](traceability.md)                         | Current requirements, legacy requirement crosswalk and consistency resolutions |
| [Inventory](documentation-inventory.md)                 | Every tracked documentation/evidence path, its role and baseline digest        |
| [Independent review](independent-review.md)             | Reviewer findings, resolutions and scope of the verdict                        |

## Reading rules

Owner decisions **WD-01 full hybrid** and **WD-02 project-centered workspace** are approved
product directions. The detailed design here is proposed for engineering review. It does
not accept any technical ADR. The complete personal product is the target, not an MVP.

For **current status**, use the gap register and the latest applicable acceptance record,
not a historical present-tense sentence. For **target behaviour**, this set consolidates
the older architecture and drafts; unchanged detailed contracts continue to apply.
For **security enforcement**, accepted ADRs prevail until explicitly extended by another
accepted ADR. Source existence, a passing fixture, production observation and an owner
decision are different kinds of evidence. Neither product scope nor this PR erases an
earlier security review's REJECT verdict.

F8 is production-active, real Codex 0.159.0, protocol 2, SQLite V8, ADR 0051 Accepted.
Active runtime capacity is **one**. Protocol 1/V7 remains available with its own store.
This is neither Claude Code Managed support nor concurrent Managed execution nor OL3.

Historical documents and evidence remain at their original paths. Navigation/status
overlays identify where to find the consolidation; accepted ADR bodies are unchanged.
The inventory distinguishes current contracts, proposed contracts, dated observations,
drafts and the original Wyss archive. Historical procedures are not current permission
to run them.
