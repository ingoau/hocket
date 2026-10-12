-- 0005: every artist on a song (OpenSubsonic `artists`), as JSON (`[{"id":..,"name":..}]`; NULL
-- for rows synced before it, read as the one artist `artist_id` + `artist` name). The mirror only
-- learns the list from the server: make each server's next sync a full one (it refetches every
-- song), keeping its "synced once" state so the library stays usable meanwhile.
ALTER TABLE tracks ADD COLUMN artists TEXT;

UPDATE saved_state SET json = json_set(json, '$.lastFullAt', 0)
WHERE key LIKE 'sync:%' AND json_extract(json, '$.lastFullAt') IS NOT NULL;
