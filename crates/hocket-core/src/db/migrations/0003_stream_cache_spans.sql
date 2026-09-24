-- 0003: the stream cache keeps what was read, not only complete files.
--   total_bytes  the server-reported length of the whole stream (NULL = unknown)
--   spans        the byte ranges the file holds, `start-end,start-end`
--                (half-open; a complete entry holds `0-total`)
--   source_tag   the track's server metadata when the bytes were fetched
--                (size, suffix, content type, changed, created); a library
--                sync that sees it differ drops the entry
ALTER TABLE cache_entries ADD COLUMN total_bytes REAL;
ALTER TABLE cache_entries ADD COLUMN spans TEXT NOT NULL DEFAULT '';
ALTER TABLE cache_entries ADD COLUMN source_tag TEXT;
UPDATE cache_entries SET total_bytes = bytes, spans = '0-' || CAST(bytes AS INTEGER) WHERE complete = 1;

-- Per-track facts stream-cache eviction weighs besides ratings and play
-- history: queued by autoplay, left early, primed but never played.
CREATE TABLE IF NOT EXISTS cache_signals (
    server_id   TEXT NOT NULL,
    track_id    TEXT NOT NULL,
    autoplay    INTEGER NOT NULL DEFAULT 0,
    skips       INTEGER NOT NULL DEFAULT 0,
    primed      INTEGER NOT NULL DEFAULT 0,
    updated_at  REAL NOT NULL,
    PRIMARY KEY (server_id, track_id)
);
