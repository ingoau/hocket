-- 0010: ReplayGain measured on this file by the background scan (tracks the
--   server left without a track gain; the scan stores the result on the
--   complete entry, and `Downloads::gain` falls back to it for plays that
--   never had a pin). NULL until the entry is scanned or the file undecodable.
ALTER TABLE cache_entries ADD COLUMN gain_db REAL;
ALTER TABLE cache_entries ADD COLUMN peak REAL;
