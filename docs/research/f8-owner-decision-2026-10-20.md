# F8 owner decision: activating per-session runtimes (not requested to proceed; prepared)

Nothing below has been done. Production still runs protocol 1, schema 7, one shared runtime.

**What changes if accepted:** protocol 2 + schema 8; every runtime request names its session and
launch; after an authority restart nothing is launched implicitly (explicit `runtime/start`, next
generation); runtime loss ends that session's attachment, approvals and admission, and a fresh lease is
needed; GUI and authority must ship together (old authority stays alive as "update pending").

**Sessions interrupted:** the installed migration needs a quiescent store (no active turn, pending
approval, active lease or reserved operation) and an authority restart; the running Codex runtime and
managed sessions are restarted once. Terminal host sessions are not touched by the migration itself;
installer rollback stops the host and must not run unattended.

**What stays valid:** all V1–V7 events, receipts and rows byte for byte; the previous runtime row
becomes an immutable legacy record; no historical generation is invented.

**What rollback loses:** SQL has no downgrade. Rollback = stop the new authority through its lifecycle
and restore the verified pre-migration snapshot (SQLite backup, matching old binaries); everything
written after the snapshot is lost.

**Preserved:** SG-12, lease/approval gates, audit integrity, same-UID is not authority, no model or
effort API, capacity 1.

**NOT_PROVEN:** provider concurrency (credential refresh race, see the shared-state note), descendant
containment, installed migration time/WAL behaviour, live GUI acceptance, scheduler under real load.

**Decision requested (only when Santiago wants F8 live):** accept ADR 0051's restart/fresh-lease/
protocol tradeoffs and authorize one quiescent maintenance window for the migration with snapshot.
Increasing capacity beyond 1 needs separate provider evidence and is not part of this decision.
