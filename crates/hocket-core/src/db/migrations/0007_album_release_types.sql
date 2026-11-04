-- 0007: OpenSubsonic `releaseTypes` on albums (`album`, `ep`, `single`, `live`...), so an artist's
-- discography can be split by kind. Lowercased, comma-separated; empty when the server sends none
-- (an album with no types is an album). As in 0005 the mirror only learns them from the server:
-- make each server's next sync a full one, keeping its "synced once" state.
ALTER TABLE albums ADD COLUMN release_types TEXT NOT NULL DEFAULT '';

UPDATE saved_state SET json = json_set(json, '$.lastFullAt', 0)
WHERE key LIKE 'sync:%' AND json_extract(json, '$.lastFullAt') IS NOT NULL;
