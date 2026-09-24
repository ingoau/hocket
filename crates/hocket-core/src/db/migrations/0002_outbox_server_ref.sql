-- 0002: the outbox records the server-side id a mutation produced (a created
-- playlist) before doing any local mirror work, so a replay after a crash can
-- reuse it instead of creating a second one.
ALTER TABLE outbox ADD COLUMN server_ref TEXT;
