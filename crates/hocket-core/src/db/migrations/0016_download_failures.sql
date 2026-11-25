-- 0016: why a pin's track failed to download, so the core knows whether to
-- try it again unasked.
--
-- `failures` counts the failed attempts since the track last downloaded.
-- `permanent` marks a failure a retry can't fix by itself: the server said
-- no (not found, forbidden, any other 4xx, a bad answer) or the track isn't
-- known. Retries the core makes on its own (an account becoming reachable)
-- skip those and tracks that failed too often; the user's own retry doesn't.
ALTER TABLE pin_tracks ADD COLUMN failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pin_tracks ADD COLUMN permanent INTEGER NOT NULL DEFAULT 0;
