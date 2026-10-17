-- 0006: sorting artists by plays or by when their newest album was added reads each artist's
-- albums; this index covers both, so the sort never touches the album rows themselves.
CREATE INDEX IF NOT EXISTS idx_albums_artist_sort ON albums(server_id, artist_id, created, play_count);
