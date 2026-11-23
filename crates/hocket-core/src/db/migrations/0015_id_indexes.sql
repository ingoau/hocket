-- 0015: index the bare `id` of tracks, albums and artists.
--
-- Their primary keys lead with `server_id`, so lookups by id alone (track(),
-- set_loved/set_rating, albums_by_ids, the outbox) scanned the whole table.
CREATE INDEX IF NOT EXISTS idx_tracks_id  ON tracks(id);
CREATE INDEX IF NOT EXISTS idx_albums_id  ON albums(id);
CREATE INDEX IF NOT EXISTS idx_artists_id ON artists(id);
