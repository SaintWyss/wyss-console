-- Run against a schema-4 library while Sync is stopped.
-- Offers with several revisions are rebuilt from the journal on the next reconciliation.
BEGIN IMMEDIATE;
ALTER TABLE sync_incoming RENAME TO sync_incoming_v4;
CREATE TABLE sync_incoming (
    object TEXT PRIMARY KEY,
    record TEXT NOT NULL,
    records TEXT NOT NULL,
    device TEXT NOT NULL,
    host TEXT NOT NULL,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    change TEXT NOT NULL,
    files TEXT NOT NULL,
    scripts INTEGER NOT NULL,
    received_at INTEGER NOT NULL
);
INSERT INTO sync_incoming SELECT * FROM sync_incoming_v4
    WHERE object IN (
        SELECT object FROM sync_incoming_v4 GROUP BY object HAVING count(*) = 1
    );
DROP TABLE sync_incoming_v4;
PRAGMA user_version = 3;
COMMIT;
