//! Cross-checks: SQL against an in-memory mirror vs in-memory evaluation,
//! NSP round trips, ordering, defaults.

use proptest::prelude::*;
use rusqlite::{params_from_iter, Connection};

use super::*;
use crate::api::{FilterCapability, OfflineState, SonicAttributes};

const NOW: f64 = 1_750_000_000_000.0; // 2025-06-15T14:13:20Z
const DAY: f64 = dates::MS_PER_DAY;

fn t(id: &str) -> (Track, LocalFacts) {
    (Track { id: id.into(), server_id: "s1".into(), title: format!("Track {id}"), duration_ms: 200_000, ..Default::default() }, LocalFacts::default())
}

/// A small corpus with variety on every field.
fn corpus() -> Vec<(Track, LocalFacts)> {
    let mut rows = Vec::new();
    let (mut a, mut fa) = t("a");
    a.title = "Love Song".into();
    a.album = Some("Best Of".into());
    a.artist = Some("U2".into());
    a.album_artist = Some("U2".into());
    a.genre = Some("Rock".into());
    a.year = Some(1985);
    a.rating = 5;
    a.loved = true;
    a.play_count = 12;
    a.last_played = Some(NOW - 2.0 * DAY);
    a.created = Some(NOW - 10.0 * DAY);
    a.bit_rate = Some(320);
    a.suffix = Some("flac".into());
    a.path = Some("U2/Best Of/01 Love Song.flac".into());
    a.cover_art = Some("al-1".into());
    a.comment = Some("this one".into());
    a.sonic = Some(SonicAttributes { bpm: Some(128.0), key: Some("Am".into()), energy: Some(0.8), mood: Some("happy".into()), ..Default::default() });
    a.offline = OfflineState::Downloaded;
    fa.has_lyrics = true;
    fa.local_play_count = 3;
    fa.local_last_played = Some(NOW - 1.0 * DAY);
    fa.changed = Some(NOW - 5.0 * DAY);
    fa.playlist_ids = vec!["pl1".into()];
    rows.push((a, fa));

    let (mut b, mut fb) = t("b");
    b.title = "hate_song 100%".into();
    b.album = Some("Other".into());
    b.artist = Some("Muse".into());
    b.genre = Some("Alternative".into());
    b.year = Some(2003);
    b.rating = 2;
    b.play_count = 0;
    b.created = Some(NOW - 60.0 * DAY);
    b.duration_ms = 65_000;
    b.bit_rate = Some(128);
    b.suffix = Some("mp3".into());
    b.offline = OfflineState::Cached;
    b.disc_number = Some(2);
    b.track_number = Some(7);
    fb.is_compilation = true;
    fb.playlist_ids = vec!["pl2".into()];
    rows.push((b, fb));

    let (mut c, fc) = t("c");
    c.title = "Untitled".into();
    c.year = Some(1990);
    c.rating = 0;
    c.play_count = 1;
    c.last_played = Some(NOW - 40.0 * DAY);
    c.created = Some(NOW - 0.5 * DAY);
    c.duration_ms = 600_000;
    c.sonic = Some(SonicAttributes { bpm: Some(90.0), energy: Some(0.2), ..Default::default() });
    rows.push((c, fc));

    let (mut d, mut fd) = t("d");
    d.title = "Émilie".into();
    d.artist = Some("Émilie Simon".into());
    d.album = Some("Best Of".into());
    d.year = Some(2006);
    d.loved = true;
    d.rating = 4;
    d.play_count = 30;
    d.last_played = Some(NOW - 100.0 * DAY);
    d.created = Some(NOW - 400.0 * DAY);
    d.cover_art = Some("al-2".into());
    d.comment = Some("this is a comment".into());
    fd.local_play_count = 9;
    fd.local_last_played = Some(NOW - 20.0 * DAY);
    fd.playlist_ids = vec!["pl1".into(), "pl2".into()];
    rows.push((d, fd));

    rows.push(t("e"));
    rows
}

fn mirror(rows: &[(Track, LocalFacts)]) -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    conn.execute_batch(
        "CREATE TABLE tracks (
            id TEXT NOT NULL, server_id TEXT NOT NULL, title TEXT NOT NULL, album_id TEXT, album TEXT,
            artist_id TEXT, artist TEXT, album_artist TEXT, track_number INTEGER, disc_number INTEGER,
            year INTEGER, genre TEXT, duration_ms INTEGER NOT NULL DEFAULT 0, bit_rate INTEGER,
            sample_rate INTEGER, bit_depth INTEGER, channels INTEGER, suffix TEXT, content_type TEXT,
            size_bytes REAL, path TEXT, cover_art TEXT, rating INTEGER NOT NULL DEFAULT 0,
            loved INTEGER NOT NULL DEFAULT 0, play_count INTEGER NOT NULL DEFAULT 0, last_played REAL,
            created REAL, changed REAL, rg_track_gain REAL, rg_track_peak REAL, rg_album_gain REAL,
            rg_album_peak REAL, bpm REAL, key TEXT, energy REAL, mood TEXT, danceability REAL,
            valence REAL, offline INTEGER NOT NULL DEFAULT 0, music_brainz_id TEXT,
            explicit INTEGER NOT NULL DEFAULT 0, comment TEXT, has_lyrics INTEGER NOT NULL DEFAULT 0,
            local_play_count INTEGER NOT NULL DEFAULT 0, local_last_played REAL,
            is_compilation INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (server_id, id));
         CREATE TABLE playlist_tracks (server_id TEXT NOT NULL, playlist_id TEXT NOT NULL,
            position INTEGER NOT NULL, track_id TEXT NOT NULL, PRIMARY KEY (server_id, playlist_id, position));",
    )
    .expect("schema");
    for (tr, facts) in rows {
        conn.execute(
            "INSERT INTO tracks (id, server_id, title, album, artist, album_artist, track_number, disc_number, year, genre,
                duration_ms, bit_rate, suffix, path, cover_art, rating, loved, play_count, last_played, created, changed,
                bpm, key, energy, mood, offline, comment, has_lyrics, local_play_count, local_last_played, is_compilation)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31)",
            rusqlite::params![
                tr.id,
                tr.server_id,
                tr.title,
                tr.album,
                tr.artist,
                tr.album_artist,
                tr.track_number,
                tr.disc_number,
                tr.year,
                tr.genre,
                tr.duration_ms,
                tr.bit_rate,
                tr.suffix,
                tr.path,
                tr.cover_art,
                tr.rating,
                tr.loved as i32,
                tr.play_count,
                tr.last_played,
                tr.created,
                facts.changed,
                tr.sonic.as_ref().and_then(|s| s.bpm),
                tr.sonic.as_ref().and_then(|s| s.key.clone()),
                tr.sonic.as_ref().and_then(|s| s.energy),
                tr.sonic.as_ref().and_then(|s| s.mood.clone()),
                tr.offline as i32,
                tr.comment,
                facts.has_lyrics as i32,
                facts.local_play_count,
                facts.local_last_played,
                facts.is_compilation as i32,
            ],
        )
        .expect("insert track");
        for (pos, pl) in facts.playlist_ids.iter().enumerate() {
            conn.execute(
                "INSERT INTO playlist_tracks (server_id, playlist_id, position, track_id) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![tr.server_id, pl, pos as i64, tr.id],
            )
            .expect("insert membership");
        }
    }
    conn
}

fn sql_ids(conn: &Connection, node: &FilterNode, sort: SortOrder, descending: bool, limit: Option<u32>) -> Vec<String> {
    let q = select_for_node(Some(node), sort, descending, limit, None, "s1", "tracks.id", NOW).expect("query");
    let mut stmt = conn.prepare(&q.sql).unwrap_or_else(|e| panic!("prepare {}: {e}", q.sql));
    let rows = stmt.query_map(params_from_iter(q.params.iter()), |r| r.get::<_, String>(0)).expect("query");
    rows.map(|r| r.expect("row")).collect()
}

fn mem_ids(rows: &[(Track, LocalFacts)], node: &FilterNode, sort: SortOrder, descending: bool, limit: Option<u32>) -> Vec<String> {
    let f = Filter { id: "x".into(), name: "x".into(), root: node.clone(), sort, descending, limit };
    static_playlist_ids(&f, rows.iter().map(|(t, f)| (t, f)), NOW, 1)
}

fn r(field: FilterField, op: FilterOp, value: FilterValue) -> FilterNode {
    FilterNode::Rule(FilterRule { field, op, value })
}
fn text(s: &str) -> FilterValue {
    FilterValue::Text(s.into())
}
fn n(x: f64) -> FilterValue {
    FilterValue::Number(x)
}

fn cases() -> Vec<(FilterNode, Vec<&'static str>)> {
    use FilterField as F;
    use FilterOp as O;
    vec![
        (r(F::Title, O::Contains, text("love")), vec!["a"]),
        (r(F::Title, O::Contains, text("_")), vec!["b"]),
        (r(F::Title, O::Contains, text("%")), vec!["b"]),
        (r(F::Title, O::NotContains, text("song")), vec!["c", "d", "e"]),
        (r(F::Title, O::StartsWith, text("un")), vec!["c"]),
        (r(F::Title, O::EndsWith, text("SONG")), vec!["a"]),
        (r(F::Title, O::Is, text("love song")), vec!["a"]),
        (r(F::Title, O::IsNot, text("Love Song")), vec!["b", "c", "d", "e"]),
        (r(F::Artist, O::Is, FilterValue::List(vec!["u2".into(), "Muse".into()])), vec!["a", "b"]),
        (r(F::Artist, O::IsNot, FilterValue::List(vec!["u2".into()])), vec!["b", "d"]),
        (r(F::Artist, O::IsNot, text("U2")), vec!["b", "d"]),
        (r(F::Album, O::Is, text("best of")), vec!["a", "d"]),
        (r(F::AlbumArtist, O::Contains, text("u")), vec!["a"]),
        (r(F::Genre, O::Is, text("rock")), vec!["a"]),
        (r(F::Year, O::InTheRange, FilterValue::Range { low: 1980.0, high: 1990.0 }), vec!["a", "c"]),
        (r(F::Year, O::Gt, n(2000.0)), vec!["b", "d"]),
        (r(F::Year, O::Lt, n(1990.0)), vec!["a"]),
        (r(F::Year, O::Is, n(2003.0)), vec!["b"]),
        (r(F::Year, O::IsNot, n(2003.0)), vec!["a", "c", "d"]),
        (r(F::Rating, O::Gt, n(3.0)), vec!["a", "d"]),
        (r(F::PlayCount, O::Is, n(0.0)), vec!["b", "e"]),
        (r(F::Loved, O::IsTrue, FilterValue::Bool(true)), vec!["a", "d"]),
        (r(F::Loved, O::IsFalse, FilterValue::Bool(true)), vec!["b", "c", "e"]),
        (r(F::Duration, O::Gt, n(300.0)), vec!["c"]),
        (r(F::Duration, O::Lt, n(100.0)), vec!["b"]),
        (r(F::BitRate, O::Gt, n(200.0)), vec!["a"]),
        (r(F::FilePath, O::StartsWith, text("u2/")), vec!["a"]),
        (r(F::FileType, O::Is, text("mp3")), vec!["b"]),
        (r(F::Comment, O::StartsWith, text("this")), vec!["a", "d"]),
        (r(F::Lyrics, O::IsTrue, FilterValue::Bool(true)), vec!["a"]),
        (r(F::HasCoverArt, O::IsFalse, FilterValue::Bool(true)), vec!["b", "c", "e"]),
        (r(F::Compilation, O::IsTrue, FilterValue::Bool(true)), vec!["b"]),
        (r(F::DiscNumber, O::Is, n(2.0)), vec!["b"]),
        (r(F::TrackNumber, O::Gt, n(5.0)), vec!["b"]),
        (r(F::Bpm, O::Gt, n(100.0)), vec!["a"]),
        (r(F::Key, O::Is, text("am")), vec!["a"]),
        (r(F::Energy, O::Lt, n(0.5)), vec!["c"]),
        (r(F::Mood, O::Contains, text("hap")), vec!["a"]),
        (r(F::Downloaded, O::IsTrue, FilterValue::Bool(true)), vec!["a"]),
        (r(F::Cached, O::IsTrue, FilterValue::Bool(true)), vec!["b"]),
        (r(F::Downloaded, O::IsFalse, FilterValue::Bool(true)), vec!["b", "c", "d", "e"]),
        (r(F::LocalPlayCount, O::Gt, n(5.0)), vec!["d"]),
        (r(F::LastPlayed, O::InTheLast, FilterValue::Days(7)), vec!["a"]),
        (r(F::LastPlayed, O::NotInTheLast, FilterValue::Days(7)), vec!["b", "c", "d", "e"]),
        (r(F::LastPlayed, O::NotInTheLast, FilterValue::Days(365)), vec!["b", "e"]),
        (r(F::LocalLastPlayed, O::InTheLast, FilterValue::Days(30)), vec!["a", "d"]),
        (r(F::DateAdded, O::Before, FilterValue::Date("2025-01-01".into())), vec!["d"]),
        (r(F::DateAdded, O::After, FilterValue::Date("2025-06-01".into())), vec!["a", "c"]),
        (r(F::DateAdded, O::InTheRange, FilterValue::List(vec!["2025-04-01".into(), "2025-06-10".into()])), vec!["a", "b"]),
        (r(F::DateModified, O::InTheLast, FilterValue::Days(7)), vec!["a"]),
        (r(F::InPlaylist, O::Is, text("pl1")), vec!["a", "d"]),
        (r(F::InPlaylist, O::IsNot, text("pl1")), vec!["b", "c", "e"]),
        (r(F::InPlaylist, O::Is, FilterValue::List(vec!["pl1".into(), "pl2".into()])), vec!["a", "b", "d"]),
        (
            FilterNode::All(vec![
                r(F::Loved, O::IsTrue, FilterValue::Bool(true)),
                FilterNode::Any(vec![r(F::Year, O::Lt, n(1990.0)), r(F::Rating, O::Is, n(4.0))]),
            ]),
            vec!["a", "d"],
        ),
        (FilterNode::All(vec![]), vec!["a", "b", "c", "d", "e"]),
        (FilterNode::Any(vec![]), vec![]),
    ]
}

#[test]
fn sql_and_memory_agree_on_every_case() {
    let rows = corpus();
    let conn = mirror(&rows);
    for (node, expected) in cases() {
        let mut got_sql = sql_ids(&conn, &node, SortOrder::Title, false, None);
        let mut got_mem = mem_ids(&rows, &node, SortOrder::Title, false, None);
        got_sql.sort();
        got_mem.sort();
        let expected: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(got_sql, expected, "sql mismatch for {node:?}");
        assert_eq!(got_mem, expected, "memory mismatch for {node:?}");
    }
}

#[test]
fn ordering_matches_between_sql_and_memory() {
    let rows = corpus();
    let conn = mirror(&rows);
    let all = FilterNode::All(vec![]);
    for sort in [
        SortOrder::Default,
        SortOrder::Title,
        SortOrder::Artist,
        SortOrder::Album,
        SortOrder::Year,
        SortOrder::DateAdded,
        SortOrder::Rating,
        SortOrder::PlayCount,
        SortOrder::Duration,
        SortOrder::Bpm,
        SortOrder::Energy,
    ] {
        for desc in [false, true] {
            for limit in [None, Some(2)] {
                let s = sql_ids(&conn, &all, sort, desc, limit);
                let m = mem_ids(&rows, &all, sort, desc, limit);
                assert_eq!(s, m, "order mismatch sort={sort:?} desc={desc} limit={limit:?}");
            }
        }
    }
    let random = sql_ids(&conn, &all, SortOrder::Random, false, Some(3));
    assert_eq!(random.len(), 3);
    let mem_random = mem_ids(&rows, &all, SortOrder::Random, false, Some(3));
    assert_eq!(mem_random.len(), 3);
}

#[test]
fn like_special_characters_are_bound_not_spliced() {
    let node = r(FilterField::Title, FilterOp::Contains, text("'; DROP TABLE tracks; --"));
    let clause = where_clause(&node, NOW).unwrap();
    assert!(!clause.sql.contains("DROP"));
    assert_eq!(clause.params.len(), 1);
    let rows = corpus();
    let conn = mirror(&rows);
    assert!(sql_ids(&conn, &node, SortOrder::Title, false, None).is_empty());
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0)).unwrap();
    assert_eq!(count, 5);
}

#[test]
fn invalid_rules_are_refused_by_sql() {
    let node = r(FilterField::Title, FilterOp::Gt, n(1.0));
    assert!(matches!(where_clause(&node, NOW), Err(FilterError::InvalidOp { .. })));
    assert!(!matches(&node, &t("x").0, NOW));
}

#[test]
fn count_and_limit_clauses() {
    let rows = corpus();
    let conn = mirror(&rows);
    let q = count_for_node(Some(&r(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true))), "s1", NOW).unwrap();
    let c: i64 = conn.query_row(&q.sql, params_from_iter(q.params.iter()), |r| r.get(0)).unwrap();
    assert_eq!(c, 2);
    assert_eq!(limit_clause(None, None), "");
    assert_eq!(limit_clause(Some(10), None), "LIMIT 10");
    assert_eq!(limit_clause(Some(10), Some(5)), "LIMIT 10 OFFSET 5");
    assert_eq!(limit_clause(None, Some(5)), "LIMIT -1 OFFSET 5");
    let q = select_for_node(None, SortOrder::Title, false, Some(2), Some(1), "s1", "tracks.id", NOW).unwrap();
    let mut stmt = conn.prepare(&q.sql).unwrap();
    let ids: Vec<String> = stmt.query_map(params_from_iter(q.params.iter()), |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
    // Title order: "Émilie", "hate_song 100%", "Love Song", "Track e", "Untitled" -> offset 1, limit 2.
    assert_eq!(ids, vec!["b".to_string(), "a".to_string()]);
}

#[test]
fn nsp_export_matches_navidrome_shape() {
    let filter = Filter {
        id: "f".into(),
        name: "80s Top".into(),
        root: FilterNode::All(vec![
            FilterNode::Any(vec![
                r(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true)),
                r(FilterField::Rating, FilterOp::Gt, n(3.0)),
            ]),
            r(FilterField::Year, FilterOp::InTheRange, FilterValue::Range { low: 1981.0, high: 1990.0 }),
            r(FilterField::LastPlayed, FilterOp::InTheLast, FilterValue::Days(30)),
            r(FilterField::DateAdded, FilterOp::Before, FilterValue::Date("2024-01-01".into())),
            r(FilterField::Title, FilterOp::Contains, text("love")),
            r(FilterField::Lyrics, FilterOp::IsTrue, FilterValue::Bool(true)),
            r(FilterField::Duration, FilterOp::Gt, n(90.5)),
        ]),
        sort: SortOrder::Year,
        descending: true,
        limit: Some(25),
    };
    let doc = to_nsp(&filter, ServerCaps::default()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(v["name"], "80s Top");
    assert_eq!(v["sort"], "year");
    assert_eq!(v["order"], "desc");
    assert_eq!(v["limit"], 25);
    let all = v["all"].as_array().unwrap();
    assert_eq!(all[0]["any"][0], serde_json::json!({ "is": { "loved": true } }));
    assert_eq!(all[0]["any"][1], serde_json::json!({ "gt": { "rating": 3 } }));
    assert_eq!(all[1], serde_json::json!({ "inTheRange": { "year": [1981, 1990] } }));
    assert_eq!(all[2], serde_json::json!({ "inTheLast": { "lastplayed": 30 } }));
    assert_eq!(all[3], serde_json::json!({ "before": { "dateadded": "2024-01-01" } }));
    assert_eq!(all[4], serde_json::json!({ "contains": { "title": "love" } }));
    assert_eq!(all[5], serde_json::json!({ "isPresent": { "lyrics": true } }));
    assert_eq!(all[6], serde_json::json!({ "gt": { "duration": 90.5 } }));
}

#[test]
fn nsp_export_refuses_local_only_and_gates_sonic() {
    let mk = |field| Filter {
        id: "f".into(),
        name: "x".into(),
        root: FilterNode::All(vec![r(field, FilterOp::IsTrue, FilterValue::Bool(true))]),
        sort: SortOrder::Default,
        descending: false,
        limit: None,
    };
    assert_eq!(
        to_nsp(&mk(FilterField::Downloaded), ServerCaps::default()),
        Err(FilterError::NotServerExpressible(vec![FilterField::Downloaded]))
    );
    let sonic = Filter {
        root: FilterNode::All(vec![r(FilterField::Bpm, FilterOp::Gt, n(120.0))]),
        ..mk(FilterField::Loved)
    };
    assert!(to_nsp(&sonic, ServerCaps::default()).is_err());
    let doc = to_nsp(&sonic, ServerCaps { sonic_attributes: true, native_api: false }).unwrap();
    assert!(doc.contains("\"bpm\": 120"));
    let cap: FilterCapability = capability(&sonic, ServerCaps::default());
    assert!(!cap.server_expressible);
}

#[test]
fn nsp_import_of_navidrome_examples() {
    let f = from_nsp(
        r#"{ "name": "Recently Played", "comment": "x", "all": [{ "inTheLast": { "lastPlayed": 30 } }], "sort": "lastPlayed", "order": "desc", "limit": 100 }"#,
        "fallback",
    )
    .unwrap();
    assert_eq!(f.name, "Recently Played");
    assert_eq!(f.root, FilterNode::All(vec![r(FilterField::LastPlayed, FilterOp::InTheLast, FilterValue::Days(30))]));
    assert_eq!(f.limit, Some(100));
    // "lastplayed" is not a SortOrder we have; falls back to default.
    assert_eq!(f.sort, SortOrder::Default);

    let f = from_nsp(
        r#"{ "all": [ { "any": [{ "is": { "loved": true } }, { "gt": { "rating": 3 } }] }, { "inTheRange": { "year": [1981, 1990] } } ], "sort": "year", "order": "desc", "limit": 25 }"#,
        "80s",
    )
    .unwrap();
    assert_eq!(f.name, "80s");
    assert_eq!(f.sort, SortOrder::Year);
    assert!(f.descending);
    assert_eq!(
        f.root,
        FilterNode::All(vec![
            FilterNode::Any(vec![
                r(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true)),
                r(FilterField::Rating, FilterOp::Gt, n(3.0)),
            ]),
            r(FilterField::Year, FilterOp::InTheRange, FilterValue::Range { low: 1981.0, high: 1990.0 }),
        ])
    );

    let f = from_nsp(r#"{ "all": [{ "gt": { "playCount": -1 } }], "sort": "random" }"#, "r").unwrap();
    assert_eq!(f.sort, SortOrder::Random);
    assert_eq!(f.limit, None);

    let f = from_nsp(r#"{ "name": "2000s", "all": [{ "inTheRange": { "year": [2000, 2009] } }], "sort": "-year,-rating,title", "limit": 200 }"#, "x").unwrap();
    assert_eq!(f.sort, SortOrder::Year);
    assert!(f.descending);

    let f = from_nsp(r#"{ "any": [{ "inPlaylist": { "id": "abc" } }, { "notInPlaylist": { "id": "def" } }, { "isNot": { "compilation": "true" } }, { "isMissing": { "lyrics": true } }] }"#, "x").unwrap();
    assert_eq!(
        f.root,
        FilterNode::Any(vec![
            r(FilterField::InPlaylist, FilterOp::Is, text("abc")),
            r(FilterField::InPlaylist, FilterOp::IsNot, text("def")),
            r(FilterField::Compilation, FilterOp::IsFalse, FilterValue::Bool(false)),
            r(FilterField::Lyrics, FilterOp::IsFalse, FilterValue::Bool(false)),
        ])
    );
}

#[test]
fn nsp_import_rejects_what_we_cannot_represent() {
    assert!(matches!(from_nsp("not json", "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "sort": "title" }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [], "any": [] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "is": { "albumrating": 3 } }] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "inPlaylist": { "path": "a.nsp" } }] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "frobnicate": { "title": "a" } }] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "gt": { "title": "a" } }] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "is": { "downloaded": true } }] }"#, "x"), Err(FilterError::Nsp(_))));
    assert!(matches!(from_nsp(r#"{ "all": [{ "before": { "lastplayed": "yesterday" } }] }"#, "x"), Err(FilterError::InvalidValue { .. })));
}

fn expressible_rule_strategy() -> impl Strategy<Value = FilterNode> {
    use FilterField as F;
    let word = "[a-z]{1,6}";
    prop_oneof![
        (prop_oneof![Just(F::Title), Just(F::Album), Just(F::Artist), Just(F::Genre), Just(F::Comment), Just(F::FilePath)], 0..6usize, word)
            .prop_map(|(f, i, w)| r(f, ops_for_field(f)[i], text(&w))),
        (prop_oneof![Just(F::Year), Just(F::PlayCount), Just(F::Duration), Just(F::BitRate), Just(F::TrackNumber)], 0..4usize, 0..3000u32)
            .prop_map(|(f, i, x)| r(f, ops_for_field(f)[i], n(f64::from(x)))),
        (0..1000u32, 0..1000u32).prop_map(|(a, b)| r(F::Year, FilterOp::InTheRange, FilterValue::Range { low: f64::from(a.min(b)), high: f64::from(a.max(b)) })),
        (0..=5u32, 0..4usize).prop_map(|(x, i)| r(F::Rating, ops_for_field(F::Rating)[i], n(f64::from(x)))),
        (prop_oneof![Just(F::Loved), Just(F::HasCoverArt), Just(F::Compilation), Just(F::Lyrics)], any::<bool>())
            .prop_map(|(f, b)| r(f, if b { FilterOp::IsTrue } else { FilterOp::IsFalse }, FilterValue::Bool(b))),
        (prop_oneof![Just(F::LastPlayed), Just(F::DateAdded), Just(F::DateModified)], 1..400u32, any::<bool>())
            .prop_map(|(f, d, last)| r(f, if last { FilterOp::InTheLast } else { FilterOp::NotInTheLast }, FilterValue::Days(d))),
        (prop_oneof![Just(F::LastPlayed), Just(F::DateAdded)], 2000..2030i32, 1..=12u32, 1..=28u32, any::<bool>()).prop_map(|(f, y, m, d, before)| {
            r(f, if before { FilterOp::Before } else { FilterOp::After }, FilterValue::Date(format!("{y:04}-{m:02}-{d:02}")))
        }),
    ]
}

fn expressible_tree_strategy() -> impl Strategy<Value = FilterNode> {
    expressible_rule_strategy().prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(FilterNode::All),
            prop::collection::vec(inner, 0..4).prop_map(FilterNode::Any),
        ]
    })
}

fn local_rule_strategy() -> impl Strategy<Value = FilterNode> {
    use FilterField as F;
    prop_oneof![
        (prop_oneof![Just(F::Downloaded), Just(F::Cached)], any::<bool>())
            .prop_map(|(f, b)| r(f, if b { FilterOp::IsTrue } else { FilterOp::IsFalse }, FilterValue::Bool(b))),
        (0..12u32, 0..4usize).prop_map(|(x, i)| r(F::LocalPlayCount, ops_for_field(F::LocalPlayCount)[i], n(f64::from(x)))),
        (1..60u32, any::<bool>()).prop_map(|(d, last)| r(F::LocalLastPlayed, if last { FilterOp::InTheLast } else { FilterOp::NotInTheLast }, FilterValue::Days(d))),
        (prop_oneof![Just("pl1"), Just("pl2"), Just("pl3")], any::<bool>())
            .prop_map(|(p, is)| r(F::InPlaylist, if is { FilterOp::Is } else { FilterOp::IsNot }, text(p))),
        (prop_oneof![Just(F::Bpm), Just(F::Energy)], 0..3usize, any::<bool>()).prop_map(|(f, i, hi)| {
            let v = if f == F::Energy { if hi { 0.5 } else { 0.1 } } else if hi { 120.0 } else { 60.0 };
            r(f, ops_for_field(f)[i], n(v))
        }),
        (prop_oneof![Just(F::Key), Just(F::Mood)], prop_oneof![Just("am"), Just("happy"), Just("x")]).prop_map(|(f, w)| r(f, FilterOp::Contains, text(w))),
    ]
}

fn any_tree_strategy() -> impl Strategy<Value = FilterNode> {
    prop_oneof![expressible_rule_strategy(), local_rule_strategy()].prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(FilterNode::All),
            prop::collection::vec(inner, 0..4).prop_map(FilterNode::Any),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn sql_and_memory_agree_on_random_trees(node in any_tree_strategy(), sort_idx in 0..11usize, desc in any::<bool>()) {
        let sorts = [SortOrder::Default, SortOrder::Title, SortOrder::Artist, SortOrder::Album, SortOrder::Year,
            SortOrder::DateAdded, SortOrder::Rating, SortOrder::PlayCount, SortOrder::Duration, SortOrder::Bpm, SortOrder::Energy];
        let rows = corpus();
        let conn = mirror(&rows);
        prop_assert!(validate_node(&node).is_ok());
        let s = sql_ids(&conn, &node, sorts[sort_idx], desc, None);
        let m = mem_ids(&rows, &node, sorts[sort_idx], desc, None);
        prop_assert_eq!(s, m);
    }

    #[test]
    fn nsp_round_trips(node in expressible_tree_strategy(), sort_idx in 0..12usize, desc in any::<bool>(), limit in prop::option::of(1..500u32)) {
        let sorts = [SortOrder::Default, SortOrder::Title, SortOrder::Artist, SortOrder::Album, SortOrder::Year, SortOrder::DateAdded,
            SortOrder::Rating, SortOrder::PlayCount, SortOrder::Duration, SortOrder::Random, SortOrder::Bpm, SortOrder::Energy];
        let filter = Filter { id: "id".into(), name: "Round trip".into(), root: node, sort: sorts[sort_idx], descending: desc, limit };
        let caps = ServerCaps { sonic_attributes: true, native_api: false };
        let doc = to_nsp(&filter, caps).unwrap();
        let back = from_nsp(&doc, "x").unwrap();
        // A bare rule at the root is wrapped in `all` on export.
        let expected_root = match &filter.root {
            FilterNode::Rule(_) => FilterNode::All(vec![filter.root.clone()]),
            other => other.clone(),
        };
        prop_assert_eq!(&back.root, &expected_root);
        prop_assert_eq!(&back.name, &filter.name);
        prop_assert_eq!(back.sort, filter.sort);
        // Random order has no direction.
        prop_assert_eq!(back.descending, if filter.sort == SortOrder::Random { false } else { filter.descending });
        prop_assert_eq!(back.limit, filter.limit);
        let again = to_nsp(&back, caps).unwrap();
        prop_assert_eq!(again, doc);
    }
}

#[test]
fn defaults_are_valid_and_evaluate() {
    let rows = corpus();
    let conn = mirror(&rows);
    for f in default_filters() {
        assert!(is_builtin(&f.id));
        validate_filter(&f).unwrap_or_else(|e| panic!("{}: {e}", f.name));
        let s = sql_ids(&conn, &f.root, f.sort, f.descending, f.limit);
        let m = mem_ids(&rows, &f.root, f.sort, f.descending, f.limit);
        if f.sort != SortOrder::Random {
            assert_eq!(s, m, "{}", f.name);
        } else {
            let (mut s, mut m) = (s, m);
            s.sort();
            m.sort();
            assert_eq!(s, m, "{}", f.name);
        }
    }
    let downloaded = default_filters().into_iter().find(|f| f.id.ends_with("downloaded")).unwrap();
    assert_eq!(mem_ids(&rows, &downloaded.root, downloaded.sort, false, None), vec!["a"]);
    assert!(!capability(&downloaded, ServerCaps::default()).server_expressible);
}
