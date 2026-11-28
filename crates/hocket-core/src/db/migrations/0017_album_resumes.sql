-- 0017: album resume points, one row per album (they were one JSON list per
-- scope in `saved_state`, rewritten whole every 5 s while an album played).
--
-- `json` is the `AlbumResumeEntry`, tombstones included. The old lists are
-- dropped, not moved (no users to keep them for).
CREATE TABLE IF NOT EXISTS album_resumes (
    scope     TEXT NOT NULL,
    album_id  TEXT NOT NULL,
    json      TEXT NOT NULL,
    PRIMARY KEY (scope, album_id)
) WITHOUT ROWID;

DELETE FROM saved_state WHERE key LIKE 'album-resume:%';
