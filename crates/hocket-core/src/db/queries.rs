//! Typed query layer over the mirror, returning `api` types.
//!
//! Track listing accepts an optional raw [`WhereClause`] produced by the
//! filters module (SQL against the `tracks` columns in `schema.sql`), a
//! [`SortOrder`], and a page. Search is FTS5 with prefix matching so it is
//! instant as you type.

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, ToSql};

use crate::api::{self, Album, Artist, Genre, OfflineState, Page, Playlist, SortOrder, Track, TrackSummary};

use super::{Db, DbResult};

/// A raw SQL fragment over `tracks` with positional parameters. Built by the
/// filters module; the query layer only ever wraps it in parentheses and
/// ANDs it with `server_id = ?`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WhereClause {
    pub sql: String,
    pub params: Vec<Value>,
}

impl WhereClause {
    pub fn new(sql: impl Into<String>, params: Vec<Value>) -> Self {
        WhereClause { sql: sql.into(), params }
    }
    pub fn is_empty(&self) -> bool {
        self.sql.trim().is_empty()
    }
}

/// Everything needed to list tracks.
#[derive(Debug, Clone, Default)]
pub struct TrackQuery {
    pub server_id: String,
    pub filter: Option<WhereClause>,
    pub sort: SortOrder,
    pub descending: bool,
    pub page: Option<Page>,
}

/// Album listing arguments.
#[derive(Debug, Clone, Default)]
pub struct AlbumQuery {
    pub server_id: String,
    pub artist_id: Option<String>,
    pub genre: Option<String>,
    pub sort: SortOrder,
    pub descending: bool,
    pub page: Option<Page>,
}

pub fn offline_from_i64(v: i64) -> OfflineState {
    match v {
        1 => OfflineState::Cached,
        2 => OfflineState::Downloaded,
        3 => OfflineState::Downloading,
        _ => OfflineState::None,
    }
}

pub fn offline_to_i64(s: OfflineState) -> i64 {
    match s {
        OfflineState::None => 0,
        OfflineState::Cached => 1,
        OfflineState::Downloaded => 2,
        OfflineState::Downloading => 3,
    }
}

/// Column list for `SELECT ... FROM tracks`, in the order [`track_from_row`] reads.
pub const TRACK_COLUMNS: &str = "id, server_id, title, album_id, album, artist_id, artist, album_artist, track_number, disc_number, year, genre, duration_ms, bit_rate, sample_rate, bit_depth, channels, suffix, content_type, size_bytes, path, cover_art, rating, loved, play_count, last_played, created, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, bpm, key, energy, mood, danceability, valence, offline, music_brainz_id, explicit, comment";

fn opt_u32(row: &Row, idx: usize) -> rusqlite::Result<Option<u32>> {
    Ok(row.get::<_, Option<i64>>(idx)?.map(|v| v.max(0) as u32))
}

pub fn track_from_row(row: &Row) -> rusqlite::Result<Track> {
    let rg = api::ReplayGain {
        track_gain_db: row.get(27)?,
        track_peak: row.get(28)?,
        album_gain_db: row.get(29)?,
        album_peak: row.get(30)?,
    };
    let sonic = api::SonicAttributes {
        bpm: row.get(31)?,
        key: row.get(32)?,
        energy: row.get(33)?,
        mood: row.get(34)?,
        danceability: row.get(35)?,
        valence: row.get(36)?,
    };
    Ok(Track {
        id: row.get(0)?,
        server_id: row.get(1)?,
        title: row.get(2)?,
        album_id: row.get(3)?,
        album: row.get(4)?,
        artist_id: row.get(5)?,
        artist: row.get(6)?,
        album_artist: row.get(7)?,
        track_number: opt_u32(row, 8)?,
        disc_number: opt_u32(row, 9)?,
        year: opt_u32(row, 10)?,
        genre: row.get(11)?,
        duration_ms: row.get::<_, i64>(12)?.max(0) as u32,
        bit_rate: opt_u32(row, 13)?,
        sample_rate: opt_u32(row, 14)?,
        bit_depth: opt_u32(row, 15)?,
        channels: opt_u32(row, 16)?,
        suffix: row.get(17)?,
        content_type: row.get(18)?,
        size_bytes: row.get(19)?,
        path: row.get(20)?,
        cover_art: row.get(21)?,
        rating: row.get::<_, i64>(22)?.clamp(0, 5) as u32,
        loved: row.get::<_, i64>(23)? != 0,
        play_count: row.get::<_, i64>(24)?.max(0) as u32,
        last_played: row.get(25)?,
        created: row.get(26)?,
        replay_gain: if rg == api::ReplayGain::default() { None } else { Some(rg) },
        sonic: if sonic == api::SonicAttributes::default() { None } else { Some(sonic) },
        offline: offline_from_i64(row.get(37)?),
        music_brainz_id: row.get(38)?,
        explicit: row.get::<_, i64>(39)? != 0,
        comment: row.get(40)?,
    })
}

pub const ALBUM_COLUMNS: &str = "id, server_id, name, artist_id, artist, year, genre, song_count, duration_ms, cover_art, rating, loved, play_count, created, last_played, is_compilation, music_brainz_id, rg_album_gain, rg_album_peak, offline";

pub fn album_from_row(row: &Row) -> rusqlite::Result<Album> {
    let gain: Option<f64> = row.get(17)?;
    let peak: Option<f64> = row.get(18)?;
    Ok(Album {
        id: row.get(0)?,
        server_id: row.get(1)?,
        name: row.get(2)?,
        artist_id: row.get(3)?,
        artist: row.get(4)?,
        year: opt_u32(row, 5)?,
        genre: row.get(6)?,
        song_count: row.get::<_, i64>(7)?.max(0) as u32,
        duration_ms: row.get::<_, i64>(8)?.max(0) as u32,
        cover_art: row.get(9)?,
        rating: row.get::<_, i64>(10)?.clamp(0, 5) as u32,
        loved: row.get::<_, i64>(11)? != 0,
        play_count: row.get::<_, i64>(12)?.max(0) as u32,
        created: row.get(13)?,
        last_played: row.get(14)?,
        is_compilation: row.get::<_, i64>(15)? != 0,
        music_brainz_id: row.get(16)?,
        replay_gain: if gain.is_none() && peak.is_none() {
            None
        } else {
            Some(api::ReplayGain { album_gain_db: gain, album_peak: peak, ..Default::default() })
        },
        offline: offline_from_i64(row.get(19)?),
    })
}

pub const ARTIST_COLUMNS: &str = "id, server_id, name, album_count, song_count, cover_art, artist_image_url, loved, music_brainz_id, biography";

pub fn artist_from_row(row: &Row) -> rusqlite::Result<Artist> {
    Ok(Artist {
        id: row.get(0)?,
        server_id: row.get(1)?,
        name: row.get(2)?,
        album_count: row.get::<_, i64>(3)?.max(0) as u32,
        song_count: row.get::<_, i64>(4)?.max(0) as u32,
        cover_art: row.get(5)?,
        artist_image_url: row.get(6)?,
        loved: row.get::<_, i64>(7)? != 0,
        music_brainz_id: row.get(8)?,
        biography: row.get(9)?,
    })
}

pub const PLAYLIST_COLUMNS: &str =
    "id, server_id, name, comment, owner, public, song_count, duration_ms, cover_art, created, changed, is_smart, is_mine, offline";

pub fn playlist_from_row(row: &Row) -> rusqlite::Result<Playlist> {
    Ok(Playlist {
        id: row.get(0)?,
        server_id: row.get(1)?,
        name: row.get(2)?,
        comment: row.get(3)?,
        owner: row.get(4)?,
        public: row.get::<_, i64>(5)? != 0,
        song_count: row.get::<_, i64>(6)?.max(0) as u32,
        duration_ms: row.get::<_, i64>(7)?.max(0) as u32,
        cover_art: row.get(8)?,
        created: row.get(9)?,
        changed: row.get(10)?,
        is_smart: row.get::<_, i64>(11)? != 0,
        is_mine: row.get::<_, i64>(12)? != 0,
        offline: offline_from_i64(row.get(13)?),
    })
}

/// ORDER BY for tracks. Ties break on album/disc/track so paging is stable.
pub fn track_order(sort: SortOrder, descending: bool, seed: i64) -> String {
    let dir = if descending { "DESC" } else { "ASC" };
    let tail = "album COLLATE NOCASE ASC, disc_number ASC, track_number ASC, id ASC";
    match sort {
        SortOrder::Default => format!("album_artist COLLATE NOCASE {dir}, year {dir}, {tail}"),
        SortOrder::Title => format!("title COLLATE NOCASE {dir}, id ASC"),
        SortOrder::Artist => format!("artist COLLATE NOCASE {dir}, {tail}"),
        SortOrder::Album => format!("album COLLATE NOCASE {dir}, disc_number {dir}, track_number {dir}, id ASC"),
        SortOrder::Year => format!("year {dir}, {tail}"),
        SortOrder::DateAdded => format!("created {dir}, {tail}"),
        SortOrder::Rating => format!("rating {dir}, {tail}"),
        SortOrder::PlayCount => format!("play_count {dir}, {tail}"),
        SortOrder::Duration => format!("duration_ms {dir}, id ASC"),
        SortOrder::Bpm => format!("bpm IS NULL, bpm {dir}, {tail}"),
        SortOrder::Energy => format!("energy IS NULL, energy {dir}, {tail}"),
        // Stable pseudo-random: a hash of the rowid with a per-process seed.
        SortOrder::Random => format!("((rowid * 2654435761 + {seed}) % 2147483647) {dir}, id ASC"),
    }
}

pub fn album_order(sort: SortOrder, descending: bool, seed: i64) -> String {
    let dir = if descending { "DESC" } else { "ASC" };
    match sort {
        SortOrder::Default | SortOrder::Album | SortOrder::Title => format!("name COLLATE NOCASE {dir}, id ASC"),
        SortOrder::Artist => format!("artist COLLATE NOCASE {dir}, year ASC, name COLLATE NOCASE ASC, id ASC"),
        SortOrder::Year => format!("year {dir}, name COLLATE NOCASE ASC, id ASC"),
        SortOrder::DateAdded => format!("created {dir}, id ASC"),
        SortOrder::Rating => format!("rating {dir}, name COLLATE NOCASE ASC, id ASC"),
        SortOrder::PlayCount => format!("play_count {dir}, name COLLATE NOCASE ASC, id ASC"),
        SortOrder::Duration => format!("duration_ms {dir}, id ASC"),
        SortOrder::Bpm | SortOrder::Energy => format!("name COLLATE NOCASE {dir}, id ASC"),
        SortOrder::Random => format!("((rowid * 2654435761 + {seed}) % 2147483647) {dir}, id ASC"),
    }
}

fn page_sql(page: &Option<Page>) -> String {
    match page {
        Some(p) => format!(" LIMIT {} OFFSET {}", p.limit, p.offset),
        None => String::new(),
    }
}

/// Build an FTS5 MATCH expression with prefix matching on every term.
/// Returns `None` when the query has no usable terms.
pub fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split(|c: char| c.is_whitespace() || c == '"' || c == '*' || c == '(' || c == ')' || c == ':' || c == '^')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

impl Db {
    // -- tracks -------------------------------------------------------------

    pub fn tracks(&self, q: &TrackQuery) -> DbResult<Vec<Track>> {
        let seed = self.random_seed();
        self.with_conn(|c| {
            let (where_sql, params) = track_where(&q.server_id, &q.filter);
            let sql = format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE {where_sql} ORDER BY {}{}",
                track_order(q.sort, q.descending, seed),
                page_sql(&q.page)
            );
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(params.iter()), track_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn track_page(&self, q: &TrackQuery) -> DbResult<api::TrackPage> {
        let items = self.tracks(q)?;
        let total = self.track_count(&q.server_id, &q.filter)?;
        Ok(api::TrackPage { items, offset: q.page.as_ref().map(|p| p.offset).unwrap_or(0), total })
    }

    pub fn track_count(&self, server_id: &str, filter: &Option<WhereClause>) -> DbResult<u32> {
        self.with_conn(|c| {
            let (where_sql, params) = track_where(server_id, filter);
            let n: i64 = c.query_row(
                &format!("SELECT count(*) FROM tracks WHERE {where_sql}"),
                params_from_iter(params.iter()),
                |r| r.get(0),
            )?;
            Ok(n.max(0) as u32)
        })
    }

    /// Ids matching a filter, in sort order — "select all is a predicate".
    pub fn track_ids(&self, q: &TrackQuery) -> DbResult<Vec<String>> {
        let seed = self.random_seed();
        self.with_conn(|c| {
            let (where_sql, params) = track_where(&q.server_id, &q.filter);
            let sql = format!(
                "SELECT id FROM tracks WHERE {where_sql} ORDER BY {}{}",
                track_order(q.sort, q.descending, seed),
                page_sql(&q.page)
            );
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(params.iter()), |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn track(&self, id: &str) -> DbResult<Option<Track>> {
        self.with_conn(|c| {
            Ok(c.query_row(&format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1"), [id], track_from_row).optional()?)
        })
    }

    /// Tracks by id, in the order requested; missing ids are skipped.
    pub fn tracks_by_ids(&self, ids: &[String]) -> DbResult<Vec<Track>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        self.with_conn(|c| {
            let mut out = Vec::with_capacity(ids.len());
            let mut st = c.prepare_cached(&format!("SELECT {TRACK_COLUMNS} FROM tracks WHERE id = ?1"))?;
            for id in ids {
                if let Some(t) = st.query_row([id], track_from_row).optional()? {
                    out.push(t);
                }
            }
            Ok(out)
        })
    }

    pub fn summaries_by_ids(&self, ids: &[String]) -> DbResult<Vec<TrackSummary>> {
        Ok(self.tracks_by_ids(ids)?.iter().map(crate::subsonic::convert::summary_of).collect())
    }

    pub fn album_tracks(&self, album_id: &str) -> DbResult<Vec<Track>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE album_id = ?1 ORDER BY disc_number ASC, track_number ASC, title COLLATE NOCASE ASC"
            ))?;
            let rows = st.query_map([album_id], track_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn artist_tracks(&self, artist_id: &str) -> DbResult<Vec<Track>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE artist_id = ?1 ORDER BY year ASC, album COLLATE NOCASE ASC, disc_number ASC, track_number ASC"
            ))?;
            let rows = st.query_map([artist_id], track_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn genre_tracks(&self, server_id: &str, genre: &str) -> DbResult<Vec<Track>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {TRACK_COLUMNS} FROM tracks WHERE server_id = ?1 AND genre = ?2 ORDER BY album_artist COLLATE NOCASE, album COLLATE NOCASE, disc_number, track_number"
            ))?;
            let rows = st.query_map([server_id, genre], track_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Insert or update tracks from the server. Local-only columns (`offline`,
    /// `local_play_count`, `local_last_played`, sonic fields the server didn't
    /// send) are preserved. `sync_gen` marks the row seen for mark-and-sweep.
    pub fn upsert_tracks(&self, tracks: &[Track], changed_ms: &[Option<f64>], sync_gen: i64) -> DbResult<usize> {
        self.with_tx(|tx| upsert_tracks_in(tx, tracks, changed_ms, sync_gen))
    }

    pub fn delete_tracks(&self, server_id: &str, ids: &[String]) -> DbResult<usize> {
        self.with_tx(|tx| {
            let mut n = 0;
            let mut st = tx.prepare_cached("DELETE FROM tracks WHERE server_id = ?1 AND id = ?2")?;
            for id in ids {
                n += st.execute([server_id, id])?;
            }
            Ok(n)
        })
    }

    /// Local rating/loved updates (optimistic, before the outbox flushes).
    pub fn set_track_rating(&self, id: &str, rating: u32) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE tracks SET rating = ?2 WHERE id = ?1", params![id, rating.min(5)])? > 0))
    }

    pub fn set_track_loved(&self, id: &str, loved: bool) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE tracks SET loved = ?2 WHERE id = ?1", params![id, loved as i64])? > 0))
    }

    pub fn set_album_rating(&self, id: &str, rating: u32) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE albums SET rating = ?2 WHERE id = ?1", params![id, rating.min(5)])? > 0))
    }

    pub fn set_album_loved(&self, id: &str, loved: bool) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE albums SET loved = ?2 WHERE id = ?1", params![id, loved as i64])? > 0))
    }

    pub fn set_artist_loved(&self, id: &str, loved: bool) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE artists SET loved = ?2 WHERE id = ?1", params![id, loved as i64])? > 0))
    }

    pub fn set_track_offline(&self, id: &str, state: OfflineState) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.execute("UPDATE tracks SET offline = ?2 WHERE id = ?1", params![id, offline_to_i64(state)])? > 0)
        })
    }

    pub fn set_track_has_lyrics(&self, id: &str, has: bool) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE tracks SET has_lyrics = ?2 WHERE id = ?1", params![id, has as i64])? > 0))
    }

    /// Sonic attributes from an analysis source (AudioMuse etc.).
    pub fn set_track_sonic(&self, id: &str, sonic: &api::SonicAttributes) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE tracks SET bpm = COALESCE(?2, bpm), key = COALESCE(?3, key), energy = COALESCE(?4, energy), mood = COALESCE(?5, mood), danceability = COALESCE(?6, danceability), valence = COALESCE(?7, valence) WHERE id = ?1",
                params![id, sonic.bpm, sonic.key, sonic.energy, sonic.mood, sonic.danceability, sonic.valence],
            )? > 0)
        })
    }

    // -- albums -------------------------------------------------------------

    pub fn albums(&self, q: &AlbumQuery) -> DbResult<Vec<Album>> {
        let seed = self.random_seed();
        self.with_conn(|c| {
            let (where_sql, params) = album_where(q);
            let sql = format!(
                "SELECT {ALBUM_COLUMNS} FROM albums WHERE {where_sql} ORDER BY {}{}",
                album_order(q.sort, q.descending, seed),
                page_sql(&q.page)
            );
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map(params_from_iter(params.iter()), album_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn album_page(&self, q: &AlbumQuery) -> DbResult<api::AlbumPage> {
        let items = self.albums(q)?;
        let total = self.album_count(q)?;
        Ok(api::AlbumPage { items, offset: q.page.as_ref().map(|p| p.offset).unwrap_or(0), total })
    }

    pub fn album_count(&self, q: &AlbumQuery) -> DbResult<u32> {
        self.with_conn(|c| {
            let (where_sql, params) = album_where(q);
            let n: i64 =
                c.query_row(&format!("SELECT count(*) FROM albums WHERE {where_sql}"), params_from_iter(params.iter()), |r| r.get(0))?;
            Ok(n.max(0) as u32)
        })
    }

    pub fn album(&self, id: &str) -> DbResult<Option<Album>> {
        self.with_conn(|c| {
            Ok(c.query_row(&format!("SELECT {ALBUM_COLUMNS} FROM albums WHERE id = ?1"), [id], album_from_row).optional()?)
        })
    }

    pub fn albums_by_ids(&self, ids: &[String]) -> DbResult<Vec<Album>> {
        self.with_conn(|c| {
            let mut out = vec![];
            let mut st = c.prepare_cached(&format!("SELECT {ALBUM_COLUMNS} FROM albums WHERE id = ?1"))?;
            for id in ids {
                if let Some(a) = st.query_row([id], album_from_row).optional()? {
                    out.push(a);
                }
            }
            Ok(out)
        })
    }

    pub fn upsert_albums(&self, albums: &[Album], changed_ms: &[Option<f64>], sync_gen: i64) -> DbResult<usize> {
        self.with_tx(|tx| upsert_albums_in(tx, albums, changed_ms, sync_gen))
    }

    // -- artists ------------------------------------------------------------

    pub fn artists(&self, server_id: &str, page: Option<Page>) -> DbResult<Vec<Artist>> {
        self.with_conn(|c| {
            let sql = format!("SELECT {ARTIST_COLUMNS} FROM artists WHERE server_id = ?1 ORDER BY name COLLATE NOCASE ASC, id ASC{}", page_sql(&page));
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map([server_id], artist_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn artist_page(&self, server_id: &str, page: Page) -> DbResult<api::ArtistPage> {
        let items = self.artists(server_id, Some(page.clone()))?;
        let total = self.artist_count(server_id)?;
        Ok(api::ArtistPage { items, offset: page.offset, total })
    }

    pub fn artist_count(&self, server_id: &str) -> DbResult<u32> {
        self.with_conn(|c| {
            let n: i64 = c.query_row("SELECT count(*) FROM artists WHERE server_id = ?1", [server_id], |r| r.get(0))?;
            Ok(n.max(0) as u32)
        })
    }

    pub fn artist(&self, id: &str) -> DbResult<Option<Artist>> {
        self.with_conn(|c| {
            Ok(c.query_row(&format!("SELECT {ARTIST_COLUMNS} FROM artists WHERE id = ?1"), [id], artist_from_row).optional()?)
        })
    }

    pub fn artists_by_ids(&self, ids: &[String]) -> DbResult<Vec<Artist>> {
        self.with_conn(|c| {
            let mut out = vec![];
            let mut st = c.prepare_cached(&format!("SELECT {ARTIST_COLUMNS} FROM artists WHERE id = ?1"))?;
            for id in ids {
                if let Some(a) = st.query_row([id], artist_from_row).optional()? {
                    out.push(a);
                }
            }
            Ok(out)
        })
    }

    pub fn upsert_artists(&self, artists: &[Artist], sync_gen: i64) -> DbResult<usize> {
        self.with_tx(|tx| upsert_artists_in(tx, artists, sync_gen))
    }

    pub fn set_artist_biography(&self, id: &str, bio: Option<&str>) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE artists SET biography = ?2 WHERE id = ?1", params![id, bio])? > 0))
    }

    // -- genres -------------------------------------------------------------

    pub fn genres(&self, server_id: &str) -> DbResult<Vec<Genre>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT name, song_count, album_count FROM genres WHERE server_id = ?1 ORDER BY name COLLATE NOCASE ASC",
            )?;
            let rows = st.query_map([server_id], |r| {
                Ok(Genre {
                    name: r.get(0)?,
                    song_count: r.get::<_, i64>(1)?.max(0) as u32,
                    album_count: r.get::<_, i64>(2)?.max(0) as u32,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn replace_genres(&self, server_id: &str, genres: &[Genre], sync_gen: i64) -> DbResult<()> {
        self.with_tx(|tx| {
            let mut st = tx.prepare_cached(
                "INSERT INTO genres(server_id, name, song_count, album_count, sync_gen) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(server_id, name) DO UPDATE SET song_count = excluded.song_count, album_count = excluded.album_count, sync_gen = excluded.sync_gen",
            )?;
            for g in genres {
                st.execute(params![server_id, g.name, g.song_count, g.album_count, sync_gen])?;
            }
            tx.execute("DELETE FROM genres WHERE server_id = ?1 AND sync_gen < ?2", params![server_id, sync_gen])?;
            Ok(())
        })
    }

    // -- playlists ----------------------------------------------------------

    pub fn playlists(&self, server_id: &str) -> DbResult<Vec<Playlist>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT {PLAYLIST_COLUMNS} FROM playlists WHERE server_id = ?1 ORDER BY name COLLATE NOCASE ASC, id ASC"
            ))?;
            let rows = st.query_map([server_id], playlist_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn playlist(&self, id: &str) -> DbResult<Option<Playlist>> {
        self.with_conn(|c| {
            Ok(c.query_row(&format!("SELECT {PLAYLIST_COLUMNS} FROM playlists WHERE id = ?1"), [id], playlist_from_row)
                .optional()?)
        })
    }

    /// Track ids of a playlist in position order.
    pub fn playlist_track_ids(&self, playlist_id: &str) -> DbResult<Vec<String>> {
        self.with_conn(|c| {
            let mut st =
                c.prepare_cached("SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 ORDER BY position ASC")?;
            let rows = st.query_map([playlist_id], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn playlist_tracks(&self, playlist_id: &str, page: Option<Page>) -> DbResult<Vec<Track>> {
        self.with_conn(|c| {
            let sql = format!(
                "SELECT {} FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id AND t.server_id = pt.server_id WHERE pt.playlist_id = ?1 ORDER BY pt.position ASC{}",
                TRACK_COLUMNS.split(", ").map(|col| format!("t.{col}")).collect::<Vec<_>>().join(", "),
                page_sql(&page)
            );
            let mut st = c.prepare_cached(&sql)?;
            let rows = st.query_map([playlist_id], track_from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn playlist_track_page(&self, playlist_id: &str, page: Page) -> DbResult<api::TrackPage> {
        let items = self.playlist_tracks(playlist_id, Some(page.clone()))?;
        let total: u32 = self.with_conn(|c| {
            let n: i64 = c.query_row("SELECT count(*) FROM playlist_tracks WHERE playlist_id = ?1", [playlist_id], |r| r.get(0))?;
            Ok(n.max(0) as u32)
        })?;
        Ok(api::TrackPage { items, offset: page.offset, total })
    }

    /// Whether the playlist's membership has been synced at least once.
    pub fn playlist_tracks_synced(&self, playlist_id: &str) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT tracks_synced FROM playlists WHERE id = ?1", [playlist_id], |r| r.get::<_, i64>(0))
                .optional()?
                .is_some_and(|v| v != 0))
        })
    }

    pub fn upsert_playlists(&self, playlists: &[Playlist], sync_gen: i64) -> DbResult<usize> {
        self.with_tx(|tx| upsert_playlists_in(tx, playlists, sync_gen))
    }

    /// Replace a playlist's membership.
    pub fn set_playlist_tracks(&self, server_id: &str, playlist_id: &str, track_ids: &[String]) -> DbResult<()> {
        self.with_tx(|tx| set_playlist_tracks_in(tx, server_id, playlist_id, track_ids))
    }

    pub fn set_playlist_rules(&self, playlist_id: &str, rules_json: Option<&str>) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.execute("UPDATE playlists SET rules = ?2, is_smart = CASE WHEN ?2 IS NULL THEN is_smart ELSE 1 END WHERE id = ?1", params![playlist_id, rules_json])? > 0)
        })
    }

    pub fn playlist_rules(&self, playlist_id: &str) -> DbResult<Option<String>> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT rules FROM playlists WHERE id = ?1", [playlist_id], |r| r.get::<_, Option<String>>(0))
                .optional()?
                .flatten())
        })
    }

    pub fn delete_playlist(&self, playlist_id: &str) -> DbResult<bool> {
        self.with_tx(|tx| {
            tx.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", [playlist_id])?;
            Ok(tx.execute("DELETE FROM playlists WHERE id = ?1", [playlist_id])? > 0)
        })
    }

    /// Playlists containing a track (for the `InPlaylist` filter and UI).
    pub fn playlists_containing(&self, track_id: &str) -> DbResult<Vec<String>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT DISTINCT playlist_id FROM playlist_tracks WHERE track_id = ?1")?;
            let rows = st.query_map([track_id], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    // -- search -------------------------------------------------------------

    /// Local-first instant search: FTS5 prefix match over tracks (title,
    /// album, artist), albums (name, artist), artists (name), plus playlists
    /// by name. Ranked by FTS `bm25`.
    pub fn search(&self, server_id: &str, query: &str, limit: u32) -> DbResult<api::SearchResults> {
        let mut results = api::SearchResults { query: query.to_string(), ..Default::default() };
        let Some(match_expr) = fts_query(query) else {
            return Ok(results);
        };
        let lim = limit.max(1) as i64;
        self.with_conn(|c| {
            let track_cols = TRACK_COLUMNS.split(", ").map(|col| format!("t.{col}")).collect::<Vec<_>>().join(", ");
            let mut st = c.prepare_cached(&format!(
                "SELECT {track_cols} FROM tracks_fts f JOIN tracks t ON t.rowid = f.rowid WHERE tracks_fts MATCH ?1 AND t.server_id = ?2 ORDER BY bm25(tracks_fts, 3.0, 1.0, 2.0), t.title COLLATE NOCASE LIMIT ?3"
            ))?;
            let rows = st.query_map(params![match_expr, server_id, lim], track_from_row)?;
            results.tracks = rows.collect::<Result<Vec<_>, _>>()?.iter().map(crate::subsonic::convert::summary_of).collect();

            let album_cols = ALBUM_COLUMNS.split(", ").map(|col| format!("a.{col}")).collect::<Vec<_>>().join(", ");
            let mut st = c.prepare_cached(&format!(
                "SELECT {album_cols} FROM albums_fts f JOIN albums a ON a.rowid = f.rowid WHERE albums_fts MATCH ?1 AND a.server_id = ?2 ORDER BY bm25(albums_fts, 3.0, 1.0), a.name COLLATE NOCASE LIMIT ?3"
            ))?;
            let rows = st.query_map(params![match_expr, server_id, lim], album_from_row)?;
            results.albums = rows.collect::<Result<Vec<_>, _>>()?;

            let artist_cols = ARTIST_COLUMNS.split(", ").map(|col| format!("a.{col}")).collect::<Vec<_>>().join(", ");
            let mut st = c.prepare_cached(&format!(
                "SELECT {artist_cols} FROM artists_fts f JOIN artists a ON a.rowid = f.rowid WHERE artists_fts MATCH ?1 AND a.server_id = ?2 ORDER BY bm25(artists_fts), a.name COLLATE NOCASE LIMIT ?3"
            ))?;
            let rows = st.query_map(params![match_expr, server_id, lim], artist_from_row)?;
            results.artists = rows.collect::<Result<Vec<_>, _>>()?;

            let mut st = c.prepare_cached(&format!(
                "SELECT {PLAYLIST_COLUMNS} FROM playlists WHERE server_id = ?1 AND name LIKE '%' || ?2 || '%' ESCAPE '\\' ORDER BY name COLLATE NOCASE LIMIT ?3"
            ))?;
            let like = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            let rows = st.query_map(params![server_id, like, lim], playlist_from_row)?;
            results.playlists = rows.collect::<Result<Vec<_>, _>>()?;
            Ok(())
        })?;
        Ok(results)
    }

    // -- play history -------------------------------------------------------

    /// Record a play locally and bump `local_play_count`/`local_last_played`.
    pub fn record_play(
        &self,
        server_id: &str,
        track_id: &str,
        played_at: f64,
        played_ms: u32,
        scrobbled: bool,
        device_id: &str,
    ) -> DbResult<i64> {
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO play_history(server_id, track_id, played_at, played_ms, scrobbled, device_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![server_id, track_id, played_at, played_ms, scrobbled as i64, device_id],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "UPDATE tracks SET local_play_count = local_play_count + 1, local_last_played = ?3 WHERE server_id = ?1 AND id = ?2",
                params![server_id, track_id, played_at],
            )?;
            Ok(id)
        })
    }

    pub fn mark_scrobbled(&self, history_id: i64) -> DbResult<bool> {
        self.with_conn(|c| Ok(c.execute("UPDATE play_history SET scrobbled = 1 WHERE id = ?1", [history_id])? > 0))
    }

    /// Recent plays, newest first, with resolved track summaries.
    pub fn recently_played(&self, limit: u32) -> DbResult<Vec<api::PlayHistoryEntry>> {
        let rows: Vec<(String, f64, u32, bool, String)> = self.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT track_id, played_at, played_ms, scrobbled, device_id FROM play_history ORDER BY played_at DESC, id DESC LIMIT ?1",
            )?;
            let rows = st.query_map([limit as i64], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, f64>(1)?,
                    r.get::<_, i64>(2)?.max(0) as u32,
                    r.get::<_, i64>(3)? != 0,
                    r.get::<_, String>(4)?,
                ))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
        let tracks = self.tracks_by_ids(&ids)?;
        let mut out = Vec::with_capacity(rows.len());
        for (track_id, played_at, played_ms, scrobbled, device_id) in rows {
            let track = match tracks.iter().find(|t| t.id == track_id) {
                Some(t) => crate::subsonic::convert::summary_of(t),
                None => TrackSummary { id: track_id.clone(), title: track_id.clone(), ..Default::default() },
            };
            out.push(api::PlayHistoryEntry { track, played_at, played_ms, scrobbled, device_id });
        }
        Ok(out)
    }

    // -- servers ------------------------------------------------------------

    pub fn upsert_server(&self, info: &api::ServerInfo, created_at: f64) -> DbResult<()> {
        let caps = serde_json::to_string(&info.capabilities)?;
        self.with_tx(|tx| {
            tx.execute(
                "INSERT INTO servers(id, url, username, name, last_sync, capabilities, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(id) DO UPDATE SET url = excluded.url, username = excluded.username, name = excluded.name, last_sync = COALESCE(excluded.last_sync, servers.last_sync), capabilities = excluded.capabilities",
                params![info.id, info.url, info.username, info.name, info.last_sync, caps, created_at],
            )?;
            tx.execute("DELETE FROM capabilities WHERE server_id = ?1", [&info.id])?;
            let mut st = tx.prepare_cached("INSERT INTO capabilities(server_id, name, enabled) VALUES (?1, ?2, ?3)")?;
            let c = &info.capabilities;
            for (name, enabled) in [
                ("openSubsonic", c.open_subsonic),
                ("transcodeOffset", c.transcode_offset),
                ("formPost", c.form_post),
                ("songLyrics", c.song_lyrics),
                ("sonicSimilarity", c.sonic_similarity),
                ("apiKeyAuthentication", c.api_key_authentication),
                ("transcoding", c.transcoding_extension),
                ("nativeApi", c.native_api),
                ("meetsFloor", c.meets_floor),
            ] {
                st.execute(params![info.id, name, enabled as i64])?;
            }
            Ok(())
        })
    }

    pub fn servers(&self) -> DbResult<Vec<api::ServerInfo>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached("SELECT id, url, username, name, last_sync, capabilities FROM servers ORDER BY created_at ASC")?;
            let rows = st.query_map([], |r| {
                let caps: String = r.get(5)?;
                Ok(api::ServerInfo {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    username: r.get(2)?,
                    name: r.get(3)?,
                    capabilities: serde_json::from_str(&caps).unwrap_or_default(),
                    last_sync: r.get(4)?,
                    reachable: false,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    pub fn set_server_last_sync(&self, server_id: &str, at: f64) -> DbResult<()> {
        self.with_conn(|c| {
            c.execute("UPDATE servers SET last_sync = ?2 WHERE id = ?1", params![server_id, at])?;
            Ok(())
        })
    }

    pub fn delete_server(&self, server_id: &str) -> DbResult<()> {
        self.clear_mirror(server_id)?;
        self.with_tx(|tx| {
            for t in ["pins", "pin_tracks", "outbox", "play_history"] {
                tx.execute(&format!("DELETE FROM {t} WHERE server_id = ?1"), [server_id])?;
            }
            tx.execute("DELETE FROM servers WHERE id = ?1", [server_id])?;
            Ok(())
        })
    }
}

fn track_where(server_id: &str, filter: &Option<WhereClause>) -> (String, Vec<Value>) {
    let mut params = vec![Value::Text(server_id.to_string())];
    let mut sql = "server_id = ?1".to_string();
    if let Some(f) = filter {
        if !f.is_empty() {
            // Renumber: filter params are appended after server_id, so its `?` must be positional-unnumbered.
            sql.push_str(" AND (");
            sql.push_str(&f.sql);
            sql.push(')');
            params.extend(f.params.iter().cloned());
        }
    }
    (sql, params)
}

fn album_where(q: &AlbumQuery) -> (String, Vec<Value>) {
    let mut params = vec![Value::Text(q.server_id.clone())];
    let mut sql = "server_id = ?1".to_string();
    if let Some(a) = &q.artist_id {
        params.push(Value::Text(a.clone()));
        sql.push_str(&format!(" AND artist_id = ?{}", params.len()));
    }
    if let Some(g) = &q.genre {
        params.push(Value::Text(g.clone()));
        sql.push_str(&format!(" AND genre = ?{}", params.len()));
    }
    (sql, params)
}

pub(crate) fn upsert_tracks_in(tx: &Connection, tracks: &[Track], changed_ms: &[Option<f64>], sync_gen: i64) -> DbResult<usize> {
    let mut st = tx.prepare_cached(
        "INSERT INTO tracks(id, server_id, title, album_id, album, artist_id, artist, album_artist, track_number, disc_number, year, genre, duration_ms, bit_rate, sample_rate, bit_depth, channels, suffix, content_type, size_bytes, path, cover_art, rating, loved, play_count, last_played, created, changed, rg_track_gain, rg_track_peak, rg_album_gain, rg_album_peak, bpm, mood, music_brainz_id, explicit, comment, is_compilation, sync_gen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, COALESCE((SELECT is_compilation FROM albums WHERE albums.server_id = ?2 AND albums.id = ?4), 0), ?38)
         ON CONFLICT(server_id, id) DO UPDATE SET
           title = excluded.title, album_id = excluded.album_id, album = excluded.album, artist_id = excluded.artist_id, artist = excluded.artist, album_artist = excluded.album_artist,
           track_number = excluded.track_number, disc_number = excluded.disc_number, year = excluded.year, genre = excluded.genre, duration_ms = excluded.duration_ms,
           bit_rate = excluded.bit_rate, sample_rate = excluded.sample_rate, bit_depth = excluded.bit_depth, channels = excluded.channels, suffix = excluded.suffix,
           content_type = excluded.content_type, size_bytes = excluded.size_bytes, path = excluded.path, cover_art = excluded.cover_art,
           rating = excluded.rating, loved = excluded.loved, play_count = excluded.play_count, last_played = excluded.last_played, created = excluded.created, changed = excluded.changed,
           rg_track_gain = excluded.rg_track_gain, rg_track_peak = excluded.rg_track_peak, rg_album_gain = excluded.rg_album_gain, rg_album_peak = excluded.rg_album_peak,
           bpm = COALESCE(excluded.bpm, tracks.bpm), mood = COALESCE(excluded.mood, tracks.mood),
           music_brainz_id = excluded.music_brainz_id, explicit = excluded.explicit, comment = excluded.comment,
           is_compilation = excluded.is_compilation, sync_gen = excluded.sync_gen",
    )?;
    let mut n = 0;
    for (i, t) in tracks.iter().enumerate() {
        let rg = t.replay_gain.clone().unwrap_or_default();
        let sonic = t.sonic.clone().unwrap_or_default();
        let changed = changed_ms.get(i).copied().flatten();
        n += st.execute(params![
            t.id,
            t.server_id,
            t.title,
            t.album_id,
            t.album,
            t.artist_id,
            t.artist,
            t.album_artist,
            t.track_number,
            t.disc_number,
            t.year,
            t.genre,
            t.duration_ms,
            t.bit_rate,
            t.sample_rate,
            t.bit_depth,
            t.channels,
            t.suffix,
            t.content_type,
            t.size_bytes,
            t.path,
            t.cover_art,
            t.rating.min(5),
            t.loved as i64,
            t.play_count,
            t.last_played,
            t.created,
            changed,
            rg.track_gain_db,
            rg.track_peak,
            rg.album_gain_db,
            rg.album_peak,
            sonic.bpm,
            sonic.mood,
            t.music_brainz_id,
            t.explicit as i64,
            t.comment,
            sync_gen,
        ])?;
    }
    Ok(n)
}

pub(crate) fn upsert_albums_in(tx: &Connection, albums: &[Album], changed_ms: &[Option<f64>], sync_gen: i64) -> DbResult<usize> {
    let mut st = tx.prepare_cached(
        "INSERT INTO albums(id, server_id, name, artist_id, artist, year, genre, song_count, duration_ms, cover_art, rating, loved, play_count, created, changed, last_played, is_compilation, music_brainz_id, rg_album_gain, rg_album_peak, sync_gen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)
         ON CONFLICT(server_id, id) DO UPDATE SET
           name = excluded.name, artist_id = excluded.artist_id, artist = excluded.artist, year = excluded.year, genre = excluded.genre,
           song_count = excluded.song_count, duration_ms = excluded.duration_ms, cover_art = excluded.cover_art, rating = excluded.rating, loved = excluded.loved,
           play_count = excluded.play_count, created = excluded.created, changed = excluded.changed, last_played = excluded.last_played, is_compilation = excluded.is_compilation,
           music_brainz_id = excluded.music_brainz_id, rg_album_gain = excluded.rg_album_gain, rg_album_peak = excluded.rg_album_peak, sync_gen = excluded.sync_gen",
    )?;
    let mut comp = tx.prepare_cached("UPDATE tracks SET is_compilation = ?3 WHERE server_id = ?1 AND album_id = ?2 AND is_compilation != ?3")?;
    let mut n = 0;
    for (i, a) in albums.iter().enumerate() {
        let rg = a.replay_gain.clone().unwrap_or_default();
        let changed = changed_ms.get(i).copied().flatten();
        n += st.execute(params![
            a.id,
            a.server_id,
            a.name,
            a.artist_id,
            a.artist,
            a.year,
            a.genre,
            a.song_count,
            a.duration_ms,
            a.cover_art,
            a.rating.min(5),
            a.loved as i64,
            a.play_count,
            a.created,
            changed,
            a.last_played,
            a.is_compilation as i64,
            a.music_brainz_id,
            rg.album_gain_db,
            rg.album_peak,
            sync_gen,
        ])?;
        comp.execute(params![a.server_id, a.id, a.is_compilation as i64])?;
    }
    Ok(n)
}

pub(crate) fn upsert_artists_in(tx: &Connection, artists: &[Artist], sync_gen: i64) -> DbResult<usize> {
    let mut st = tx.prepare_cached(
        "INSERT INTO artists(id, server_id, name, album_count, song_count, cover_art, artist_image_url, loved, music_brainz_id, biography, sync_gen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(server_id, id) DO UPDATE SET
           name = excluded.name, album_count = excluded.album_count, song_count = CASE WHEN excluded.song_count > 0 THEN excluded.song_count ELSE artists.song_count END,
           cover_art = excluded.cover_art, artist_image_url = excluded.artist_image_url, loved = excluded.loved, music_brainz_id = excluded.music_brainz_id,
           biography = COALESCE(excluded.biography, artists.biography), sync_gen = excluded.sync_gen",
    )?;
    let mut n = 0;
    for a in artists {
        n += st.execute(params![
            a.id,
            a.server_id,
            a.name,
            a.album_count,
            a.song_count,
            a.cover_art,
            a.artist_image_url,
            a.loved as i64,
            a.music_brainz_id,
            a.biography,
            sync_gen,
        ])?;
    }
    Ok(n)
}

pub(crate) fn upsert_playlists_in(tx: &Connection, playlists: &[Playlist], sync_gen: i64) -> DbResult<usize> {
    let mut st = tx.prepare_cached(
        "INSERT INTO playlists(id, server_id, name, comment, owner, public, song_count, duration_ms, cover_art, created, changed, is_smart, is_mine, sync_gen)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(server_id, id) DO UPDATE SET
           name = excluded.name, comment = excluded.comment, owner = excluded.owner, public = excluded.public, song_count = excluded.song_count,
           duration_ms = excluded.duration_ms, cover_art = excluded.cover_art, created = excluded.created, changed = excluded.changed,
           is_smart = excluded.is_smart, is_mine = excluded.is_mine, sync_gen = excluded.sync_gen",
    )?;
    let mut n = 0;
    for p in playlists {
        n += st.execute(params![
            p.id,
            p.server_id,
            p.name,
            p.comment,
            p.owner,
            p.public as i64,
            p.song_count,
            p.duration_ms,
            p.cover_art,
            p.created,
            p.changed,
            p.is_smart as i64,
            p.is_mine as i64,
            sync_gen,
        ])?;
    }
    Ok(n)
}

pub(crate) fn set_playlist_tracks_in(tx: &Connection, server_id: &str, playlist_id: &str, track_ids: &[String]) -> DbResult<()> {
    tx.execute("DELETE FROM playlist_tracks WHERE server_id = ?1 AND playlist_id = ?2", [server_id, playlist_id])?;
    let mut st = tx.prepare_cached("INSERT INTO playlist_tracks(server_id, playlist_id, position, track_id) VALUES (?1, ?2, ?3, ?4)")?;
    for (i, id) in track_ids.iter().enumerate() {
        st.execute(params![server_id, playlist_id, i as i64, id])?;
    }
    tx.execute(
        "UPDATE playlists SET tracks_synced = 1, song_count = ?3 WHERE server_id = ?1 AND id = ?2",
        params![server_id, playlist_id, track_ids.len() as i64],
    )?;
    Ok(())
}

/// Mark-and-sweep: delete rows of `table` for the server not seen in `gen`.
pub(crate) fn sweep(tx: &Connection, table: &str, server_id: &str, sync_gen: i64) -> DbResult<usize> {
    let n = tx.execute(&format!("DELETE FROM {table} WHERE server_id = ?1 AND sync_gen < ?2"), params![server_id, sync_gen])?;
    if table == "playlists" {
        tx.execute(
            "DELETE FROM playlist_tracks WHERE server_id = ?1 AND playlist_id NOT IN (SELECT id FROM playlists WHERE server_id = ?1)",
            [server_id],
        )?;
    }
    Ok(n)
}

impl From<Vec<Value>> for WhereClause {
    fn from(params: Vec<Value>) -> Self {
        WhereClause { sql: String::new(), params }
    }
}

#[allow(dead_code)]
fn _assert_tosql(_: &dyn ToSql) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::WallClock;

    fn t(id: &str, title: &str, album: &str, artist: &str) -> Track {
        Track {
            id: id.into(),
            server_id: "s".into(),
            title: title.into(),
            album_id: Some(format!("al-{album}")),
            album: Some(album.into()),
            artist_id: Some(format!("ar-{artist}")),
            artist: Some(artist.into()),
            album_artist: Some(artist.into()),
            duration_ms: 1000,
            created: Some(1.0),
            ..Default::default()
        }
    }

    fn seeded() -> Db {
        let db = Db::open_in_memory().unwrap();
        let mut a = t("1", "Roygbiv", "Music Has the Right", "Boards of Canada");
        a.rating = 5;
        a.year = Some(1998);
        a.play_count = 10;
        a.sonic = Some(api::SonicAttributes { bpm: Some(81.0), ..Default::default() });
        let mut b = t("2", "Aquarius", "Music Has the Right", "Boards of Canada");
        b.rating = 3;
        b.year = Some(1998);
        b.loved = true;
        let mut c = t("3", "Smooth Operator", "Diamond Life", "Sade");
        c.year = Some(1984);
        c.genre = Some("Soul".into());
        db.upsert_tracks(&[a, b, c], &[None, None, Some(5.0)], 1).unwrap();
        db.upsert_albums(
            &[
                Album { id: "al-Music Has the Right".into(), server_id: "s".into(), name: "Music Has the Right".into(), artist: Some("Boards of Canada".into()), artist_id: Some("ar-Boards of Canada".into()), year: Some(1998), ..Default::default() },
                Album { id: "al-Diamond Life".into(), server_id: "s".into(), name: "Diamond Life".into(), artist: Some("Sade".into()), artist_id: Some("ar-Sade".into()), year: Some(1984), genre: Some("Soul".into()), is_compilation: true, ..Default::default() },
            ],
            &[None, None],
            1,
        )
        .unwrap();
        db.upsert_artists(
            &[
                Artist { id: "ar-Boards of Canada".into(), server_id: "s".into(), name: "Boards of Canada".into(), album_count: 1, ..Default::default() },
                Artist { id: "ar-Sade".into(), server_id: "s".into(), name: "Sade".into(), album_count: 1, ..Default::default() },
            ],
            1,
        )
        .unwrap();
        db.upsert_playlists(&[Playlist { id: "pl".into(), server_id: "s".into(), name: "Evening Mix".into(), is_mine: true, ..Default::default() }], 1).unwrap();
        db.set_playlist_tracks("s", "pl", &["3".into(), "1".into()]).unwrap();
        db
    }

    #[test]
    fn upsert_roundtrips_every_column() {
        let db = Db::open_in_memory().unwrap();
        let full = Track {
            id: "x".into(),
            server_id: "s".into(),
            title: "T".into(),
            album_id: Some("a".into()),
            album: Some("A".into()),
            artist_id: Some("r".into()),
            artist: Some("R".into()),
            album_artist: Some("AR".into()),
            track_number: Some(3),
            disc_number: Some(2),
            year: Some(2001),
            genre: Some("G".into()),
            duration_ms: 123_456,
            bit_rate: Some(320),
            sample_rate: Some(48000),
            bit_depth: Some(24),
            channels: Some(2),
            suffix: Some("flac".into()),
            content_type: Some("audio/flac".into()),
            size_bytes: Some(9.5e6),
            path: Some("p/q.flac".into()),
            cover_art: Some("ca".into()),
            rating: 4,
            loved: true,
            play_count: 9,
            last_played: Some(1.5e12),
            created: Some(1.4e12),
            replay_gain: Some(api::ReplayGain { track_gain_db: Some(-1.0), track_peak: Some(0.9), album_gain_db: Some(-2.0), album_peak: Some(0.8) }),
            sonic: Some(api::SonicAttributes { bpm: Some(120.0), mood: Some("happy".into()), ..Default::default() }),
            offline: OfflineState::None,
            music_brainz_id: Some("mb".into()),
            explicit: true,
            comment: Some("c".into()),
        };
        db.upsert_tracks(std::slice::from_ref(&full), &[Some(7.0)], 1).unwrap();
        let back = db.track("x").unwrap().unwrap();
        assert_eq!(back, full);
        // local columns survive a re-upsert
        db.set_track_offline("x", OfflineState::Downloaded).unwrap();
        db.set_track_sonic("x", &api::SonicAttributes { energy: Some(0.7), key: Some("Am".into()), ..Default::default() }).unwrap();
        db.record_play("s", "x", 2.0e12, 100_000, false, "dev").unwrap();
        let mut again = full.clone();
        again.title = "T2".into();
        again.sonic = None;
        db.upsert_tracks(&[again], &[None], 2).unwrap();
        let back = db.track("x").unwrap().unwrap();
        assert_eq!(back.title, "T2");
        assert_eq!(back.offline, OfflineState::Downloaded);
        let sonic = back.sonic.unwrap();
        assert_eq!(sonic.bpm, Some(120.0), "server sent no bpm this time; keep");
        assert_eq!(sonic.energy, Some(0.7));
        assert_eq!(sonic.key.as_deref(), Some("Am"));
        let (lpc, llp): (i64, Option<f64>) = db
            .with_conn(|c| Ok(c.query_row("SELECT local_play_count, local_last_played FROM tracks WHERE id='x'", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!(lpc, 1);
        assert_eq!(llp, Some(2.0e12));
    }

    #[test]
    fn listing_sorting_paging_and_counting() {
        let db = seeded();
        let q = TrackQuery { server_id: "s".into(), sort: SortOrder::Title, ..Default::default() };
        let titles: Vec<String> = db.tracks(&q).unwrap().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, vec!["Aquarius", "Roygbiv", "Smooth Operator"]);
        let q = TrackQuery { server_id: "s".into(), sort: SortOrder::Rating, descending: true, page: Some(Page { offset: 0, limit: 2 }), ..Default::default() };
        let page = db.track_page(&q).unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id, "1");
        assert_eq!(page.items[1].id, "2");
        let q = TrackQuery { server_id: "s".into(), sort: SortOrder::Year, ..Default::default() };
        assert_eq!(db.tracks(&q).unwrap()[0].id, "3");
        let q = TrackQuery { server_id: "s".into(), sort: SortOrder::Bpm, descending: true, ..Default::default() };
        assert_eq!(db.tracks(&q).unwrap()[0].id, "1", "nulls last even when descending");
        let q = TrackQuery { server_id: "other".into(), ..Default::default() };
        assert!(db.tracks(&q).unwrap().is_empty());
        assert_eq!(db.track_count("s", &None).unwrap(), 3);
    }

    #[test]
    fn random_sort_is_stable_across_pages() {
        let db = seeded();
        let all = TrackQuery { server_id: "s".into(), sort: SortOrder::Random, ..Default::default() };
        let full = db.track_ids(&all).unwrap();
        let p1 = db.track_ids(&TrackQuery { page: Some(Page { offset: 0, limit: 2 }), ..all.clone() }).unwrap();
        let p2 = db.track_ids(&TrackQuery { page: Some(Page { offset: 2, limit: 2 }), ..all.clone() }).unwrap();
        assert_eq!([p1, p2].concat(), full);
    }

    #[test]
    fn raw_where_clause_from_filters_module() {
        let db = seeded();
        let filter = WhereClause::new("rating >= ? AND (genre IS NULL OR genre != ?)", vec![Value::Integer(3), Value::Text("Soul".into())]);
        let q = TrackQuery { server_id: "s".into(), filter: Some(filter.clone()), sort: SortOrder::Title, ..Default::default() };
        let ids = db.track_ids(&q).unwrap();
        assert_eq!(ids, vec!["2", "1"]);
        assert_eq!(db.track_count("s", &Some(filter)).unwrap(), 2);
        // local-only fields are queryable too
        db.record_play("s", "3", 1.0, 10, false, "d").unwrap();
        let f = WhereClause::new("local_play_count > ?", vec![Value::Integer(0)]);
        assert_eq!(db.track_ids(&TrackQuery { server_id: "s".into(), filter: Some(f), ..Default::default() }).unwrap(), vec!["3"]);
        let f = WhereClause::new("is_compilation = 1", vec![]);
        assert_eq!(db.track_ids(&TrackQuery { server_id: "s".into(), filter: Some(f), ..Default::default() }).unwrap(), vec!["3"], "denormalised from album");
        let f = WhereClause::new("id IN (SELECT track_id FROM playlist_tracks WHERE playlist_id = ?)", vec![Value::Text("pl".into())]);
        let mut ids = db.track_ids(&TrackQuery { server_id: "s".into(), filter: Some(f), ..Default::default() }).unwrap();
        ids.sort();
        assert_eq!(ids, vec!["1", "3"]);
    }

    #[test]
    fn by_ids_preserves_order_and_skips_missing() {
        let db = seeded();
        let ids: Vec<String> = ["3", "nope", "1"].iter().map(|s| s.to_string()).collect();
        let got: Vec<String> = db.tracks_by_ids(&ids).unwrap().into_iter().map(|t| t.id).collect();
        assert_eq!(got, vec!["3", "1"]);
        assert_eq!(db.summaries_by_ids(&ids).unwrap().len(), 2);
    }

    #[test]
    fn albums_artists_genres_playlists() {
        let db = seeded();
        let q = AlbumQuery { server_id: "s".into(), sort: SortOrder::Year, ..Default::default() };
        let page = db.album_page(&AlbumQuery { page: Some(Page { offset: 0, limit: 10 }), ..q.clone() }).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].name, "Diamond Life");
        let q = AlbumQuery { server_id: "s".into(), artist_id: Some("ar-Sade".into()), ..Default::default() };
        assert_eq!(db.album_count(&q).unwrap(), 1);
        let q = AlbumQuery { server_id: "s".into(), genre: Some("Soul".into()), ..Default::default() };
        assert_eq!(db.albums(&q).unwrap()[0].id, "al-Diamond Life");
        assert!(db.album("al-Diamond Life").unwrap().unwrap().is_compilation);
        assert_eq!(db.album_tracks("al-Music Has the Right").unwrap().len(), 2);
        assert_eq!(db.artist_tracks("ar-Sade").unwrap().len(), 1);

        let artists = db.artist_page("s", Page { offset: 0, limit: 1 }).unwrap();
        assert_eq!(artists.total, 2);
        assert_eq!(artists.items[0].name, "Boards of Canada");
        db.set_artist_biography("ar-Sade", Some("bio")).unwrap();
        assert_eq!(db.artist("ar-Sade").unwrap().unwrap().biography.as_deref(), Some("bio"));

        db.replace_genres("s", &[Genre { name: "Soul".into(), song_count: 1, album_count: 1 }, Genre { name: "IDM".into(), song_count: 2, album_count: 1 }], 1).unwrap();
        let g = db.genres("s").unwrap();
        assert_eq!(g.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["IDM", "Soul"]);
        db.replace_genres("s", &[Genre { name: "Soul".into(), song_count: 1, album_count: 1 }], 2).unwrap();
        assert_eq!(db.genres("s").unwrap().len(), 1);
        assert_eq!(db.genre_tracks("s", "Soul").unwrap().len(), 1);

        let pls = db.playlists("s").unwrap();
        assert_eq!(pls.len(), 1);
        assert_eq!(pls[0].song_count, 2, "set_playlist_tracks updates count");
        assert_eq!(db.playlist_track_ids("pl").unwrap(), vec!["3", "1"]);
        let pt = db.playlist_track_page("pl", Page { offset: 1, limit: 5 }).unwrap();
        assert_eq!(pt.total, 2);
        assert_eq!(pt.items[0].id, "1");
        assert!(db.playlist_tracks_synced("pl").unwrap());
        assert_eq!(db.playlists_containing("3").unwrap(), vec!["pl"]);
        db.set_playlist_rules("pl", Some(r#"{"all":[]}"#)).unwrap();
        assert!(db.playlist("pl").unwrap().unwrap().is_smart);
        assert_eq!(db.playlist_rules("pl").unwrap().as_deref(), Some(r#"{"all":[]}"#));
        assert!(db.delete_playlist("pl").unwrap());
        assert!(db.playlist_track_ids("pl").unwrap().is_empty());
    }

    #[test]
    fn fts_search_prefix_and_ranking() {
        let db = seeded();
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("boards can\"ada"), Some("\"boards\"* \"can\"* \"ada\"*".into()));
        let r = db.search("s", "roy", 10).unwrap();
        assert_eq!(r.tracks.len(), 1);
        assert_eq!(r.tracks[0].title, "Roygbiv");
        let r = db.search("s", "boards", 10).unwrap();
        assert_eq!(r.tracks.len(), 2, "artist column is indexed");
        assert_eq!(r.albums.len(), 1);
        assert_eq!(r.artists.len(), 1);
        let r = db.search("s", "music right", 10).unwrap();
        assert_eq!(r.albums.len(), 1, "all terms must match, any order");
        let r = db.search("s", "even", 10).unwrap();
        assert_eq!(r.playlists.len(), 1);
        let r = db.search("s", "zzz", 10).unwrap();
        assert!(r.tracks.is_empty() && r.albums.is_empty() && r.artists.is_empty() && r.playlists.is_empty());
        let r = db.search("s", "sm", 1).unwrap();
        assert_eq!(r.tracks.len(), 1);
        // updates keep the index in step
        db.with_conn(|c| {
            c.execute("UPDATE tracks SET title = 'Renamed Song' WHERE id = '1'", [])?;
            Ok(())
        })
        .unwrap();
        assert!(db.search("s", "roy", 10).unwrap().tracks.is_empty());
        assert_eq!(db.search("s", "renamed", 10).unwrap().tracks.len(), 1);
        db.delete_tracks("s", &["1".into()]).unwrap();
        assert!(db.search("s", "renamed", 10).unwrap().tracks.is_empty());
        // odd characters never break the MATCH expression
        for q in ["\"", "a:b", "(", "^x", "*", "foo OR bar", "NEAR(a b)"] {
            db.search("s", q, 5).unwrap();
        }
    }

    #[test]
    fn play_history_and_recently_played() {
        let db = seeded();
        let id = db.record_play("s", "1", 100.0, 60_000, false, "dev").unwrap();
        db.record_play("s", "3", 200.0, 30_000, true, "dev").unwrap();
        db.record_play("s", "gone", 300.0, 30_000, true, "dev").unwrap();
        assert!(db.mark_scrobbled(id).unwrap());
        let h = db.recently_played(10).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].track.id, "gone");
        assert_eq!(h[0].track.title, "gone", "unknown track degrades to its id");
        assert_eq!(h[1].track.title, "Smooth Operator");
        assert!(h[2].scrobbled);
        assert_eq!(h[2].played_ms, 60_000);
        assert_eq!(db.recently_played(1).unwrap().len(), 1);
    }

    #[test]
    fn servers_roundtrip() {
        let db = seeded();
        let info = api::ServerInfo {
            id: "s".into(),
            url: "https://x".into(),
            username: "alice".into(),
            name: "Home".into(),
            capabilities: api::ServerCapabilities { sonic_similarity: true, native_api: true, ..Default::default() },
            last_sync: None,
            reachable: true,
        };
        db.upsert_server(&info, 1.0).unwrap();
        db.set_server_last_sync("s", 42.0).unwrap();
        let s = db.servers().unwrap();
        assert_eq!(s.len(), 1);
        assert!(s[0].capabilities.sonic_similarity && s[0].capabilities.native_api);
        assert_eq!(s[0].last_sync, Some(42.0));
        assert!(!s[0].reachable, "reachability is runtime state, not stored");
        let n: i64 = db.with_conn(|c| Ok(c.query_row("SELECT count(*) FROM capabilities WHERE server_id='s' AND enabled=1", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 2);
        db.delete_server("s").unwrap();
        assert!(db.servers().unwrap().is_empty());
        assert_eq!(db.track_count("s", &None).unwrap(), 0);
    }

    #[test]
    fn mirror_clear_and_saved_state_clock() {
        let db = seeded();
        db.saved_state_set("session", &serde_json::json!({"rev": 3}), &WallClock).unwrap();
        db.clear_mirror("s").unwrap();
        assert_eq!(db.track_count("s", &None).unwrap(), 0);
        assert!(db.albums(&AlbumQuery { server_id: "s".into(), ..Default::default() }).unwrap().is_empty());
        let v: serde_json::Value = db.saved_state_get("session").unwrap().unwrap();
        assert_eq!(v["rev"], 3, "durable state untouched by mirror clear");
    }
}
