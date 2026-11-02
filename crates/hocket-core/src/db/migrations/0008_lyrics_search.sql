-- 0008: full-text index over lyrics already in lyrics_cache. Subsonic has no lyrics
-- search, so this only ever covers songs whose lyrics were fetched (played, or
-- opened). The body is the folded words; `lines` is the original text plus
-- timestamps, so a hit can show the line and seek to it.
--
-- lyrics_cache deletes and "known empty" updates drop the index row. That
-- delete fires lyrics_search_ad, which removes the FTS entry. Doing both here
-- would delete the FTS row twice and corrupt the index. Inserts and lyric
-- replacements are written by the core (it has to read the JSON).

CREATE TABLE IF NOT EXISTS lyrics_search (
    server_id TEXT NOT NULL,
    track_id  TEXT NOT NULL,
    source    TEXT NOT NULL,
    body      TEXT NOT NULL,
    lines     TEXT NOT NULL,
    PRIMARY KEY (server_id, track_id, source)
);

CREATE VIRTUAL TABLE IF NOT EXISTS lyrics_search_fts USING fts5(
    body,
    content='lyrics_search',
    content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3 4'
);

CREATE TRIGGER IF NOT EXISTS lyrics_search_ai AFTER INSERT ON lyrics_search BEGIN
    INSERT INTO lyrics_search_fts(rowid, body) VALUES (new.rowid, new.body);
END;
CREATE TRIGGER IF NOT EXISTS lyrics_search_ad AFTER DELETE ON lyrics_search BEGIN
    INSERT INTO lyrics_search_fts(lyrics_search_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
END;
CREATE TRIGGER IF NOT EXISTS lyrics_search_au AFTER UPDATE OF body ON lyrics_search BEGIN
    INSERT INTO lyrics_search_fts(lyrics_search_fts, rowid, body) VALUES ('delete', old.rowid, old.body);
    INSERT INTO lyrics_search_fts(rowid, body) VALUES (new.rowid, new.body);
END;

CREATE TRIGGER IF NOT EXISTS lyrics_cache_ad AFTER DELETE ON lyrics_cache BEGIN
    DELETE FROM lyrics_search
        WHERE server_id = old.server_id AND track_id = old.track_id AND source = old.source;
END;

CREATE TRIGGER IF NOT EXISTS lyrics_cache_au_clear AFTER UPDATE OF json ON lyrics_cache
WHEN new.json = '' BEGIN
    DELETE FROM lyrics_search
        WHERE server_id = old.server_id AND track_id = old.track_id AND source = old.source;
END;
