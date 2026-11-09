-- 0009: AutoMix analysis records (docs/automix.md), the best one known per track.
-- Durable: a desktop's analysis takes minutes of CPU to redo. `json` is the
-- compacted `AnalysisRecord`; `version` and `produced_by` are copied out of it
-- for inspection.

CREATE TABLE IF NOT EXISTS automix_records (
    server_id   TEXT NOT NULL,
    track_id    TEXT NOT NULL,
    version     INTEGER NOT NULL,
    produced_by TEXT NOT NULL,
    json        TEXT NOT NULL,
    updated_at  REAL NOT NULL,
    PRIMARY KEY (server_id, track_id)
);
