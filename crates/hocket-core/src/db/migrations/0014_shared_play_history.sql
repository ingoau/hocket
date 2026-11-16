-- 0014: listening history shared between devices through the room's play log.
--
-- `remote` marks a play this device learnt from another one and never reached
-- itself. Reaching it here later clears the flag (and is then judged for a
-- scrobble like any play of its own).
ALTER TABLE play_history ADD COLUMN remote INTEGER NOT NULL DEFAULT 0;

-- Which room play logs (by log id) are known to hold a play: this device read
-- it from that log, or the room acknowledged it. A play is pushed to a room
-- until it has a row here for that room's log.
CREATE TABLE IF NOT EXISTS play_relays (
    history_id INTEGER NOT NULL,
    log_id     TEXT NOT NULL,
    PRIMARY KEY (history_id, log_id)
) WITHOUT ROWID;
