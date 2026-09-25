-- 0004: a play is recorded once. A play's identity is its track and its
-- original start (`played_at`, the session clock's `startedAt`, carried
-- unchanged across handoffs). A play that moved back to a device that had
-- already recorded it (quick handoffs back and forth) reached its threshold
-- there again and was recorded twice, and the Android Home screen, keyed by
-- start and track, crashed on the pair.
--
-- Keep the first row of each play (scrobbled if any copy was), take the
-- extra local play count bumps back, drop the copies, and let the index
-- refuse any new one.
UPDATE play_history SET scrobbled = 1
WHERE scrobbled = 0 AND EXISTS (
    SELECT 1 FROM play_history o
    WHERE o.server_id = play_history.server_id AND o.track_id = play_history.track_id
      AND o.played_at = play_history.played_at AND o.scrobbled = 1
);

UPDATE tracks SET local_play_count = MAX(0, local_play_count - (
    SELECT count(*) FROM play_history d
    WHERE d.server_id = tracks.server_id AND d.track_id = tracks.id
      AND EXISTS (
          SELECT 1 FROM play_history k
          WHERE k.server_id = d.server_id AND k.track_id = d.track_id
            AND k.played_at = d.played_at AND k.id < d.id
      )
))
WHERE EXISTS (
    SELECT 1 FROM play_history d JOIN play_history k
      ON k.server_id = d.server_id AND k.track_id = d.track_id
     AND k.played_at = d.played_at AND k.id < d.id
    WHERE d.server_id = tracks.server_id AND d.track_id = tracks.id
);

DELETE FROM play_history
WHERE EXISTS (
    SELECT 1 FROM play_history k
    WHERE k.server_id = play_history.server_id AND k.track_id = play_history.track_id
      AND k.played_at = play_history.played_at AND k.id < play_history.id
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_play_history_play ON play_history(server_id, track_id, played_at);
