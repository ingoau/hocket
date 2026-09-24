//! Client tests against the scripted [`FakeTransport`] and embedded Navidrome
//! response fixtures.

use std::sync::Arc;

use url::Url;

use super::auth::{AuthMode, Credential};
use super::client::{AlbumListType, Client, ClientConfig, RetryPolicy, Search3Page, StreamOptions};
use super::convert::{album_from_id3, playlist_from_body, track_from_child};
use super::native::NativePlaylistUpdate;
use super::transport::{
    endpoint_of, headers_map, query_param, query_params, FakeReply, FakeTransport, Method,
};
use super::{PlaylistUpdate, StarTarget, SubsonicApi, SubsonicError};

const PING: &str = include_str!("fixtures/ping.json");
const PING_ERROR: &str = include_str!("fixtures/ping_error.json");
const EXTENSIONS: &str = include_str!("fixtures/extensions.json");
const SEARCH3: &str = include_str!("fixtures/search3.json");
const SEARCH3_EMPTY: &str = include_str!("fixtures/search3_empty.json");
const LYRICS_V2: &str = include_str!("fixtures/lyrics_v2.json");
const LYRICS_ENHANCED: &str = include_str!("fixtures/lyrics_enhanced.json");
const PLAYLIST: &str = include_str!("fixtures/playlist.json");
const PLAYLISTS: &str = include_str!("fixtures/playlists.json");
const SONIC: &str = include_str!("fixtures/sonic_similar.json");
const SCAN: &str = include_str!("fixtures/scan_status.json");
const NATIVE_PLAYLIST: &str = include_str!("fixtures/native_playlist.json");
const NATIVE_LOGIN: &str = include_str!("fixtures/native_login.json");
const ARTISTS: &str = include_str!("fixtures/artists.json");
const ALBUM_LIST2: &str = include_str!("fixtures/album_list2.json");
const GENRES: &str = include_str!("fixtures/genres.json");

fn ok_empty() -> FakeReply {
    FakeReply::Json(PING.to_string())
}

fn client(t: &FakeTransport) -> Client {
    let mut cfg = ClientConfig::new(
        "srv1",
        Url::parse("https://music.example.org/nd/").unwrap(),
        AuthMode::Password {
            username: "alice".into(),
            password: Credential::new("sesame"),
        },
    );
    cfg.retry = RetryPolicy::none();
    Client::new(cfg, Arc::new(t.clone()))
}

fn client_with_retries(t: &FakeTransport, attempts: u32) -> Client {
    let mut cfg = ClientConfig::new(
        "srv1",
        Url::parse("https://music.example.org").unwrap(),
        AuthMode::Password {
            username: "alice".into(),
            password: Credential::new("sesame"),
        },
    );
    cfg.retry = RetryPolicy {
        max_attempts: attempts,
        initial_backoff: std::time::Duration::ZERO,
        max_backoff: std::time::Duration::ZERO,
    };
    Client::new(cfg, Arc::new(t.clone()))
}

#[tokio::test]
async fn ping_sends_token_salt_and_protocol_params_never_plaintext() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", ok_empty());
    let c = client(&t);
    let r = c.ping().await.unwrap();
    assert_eq!(r.server_version.as_deref(), Some("0.63.1 (abcd1234)"));
    let req = &t.requests_to("ping")[0];
    assert_eq!(req.url.path(), "/nd/rest/ping");
    assert_eq!(query_param(req, "u").as_deref(), Some("alice"));
    assert_eq!(query_param(req, "v").as_deref(), Some("1.16.1"));
    assert_eq!(query_param(req, "c").as_deref(), Some("hocket"));
    assert_eq!(query_param(req, "f").as_deref(), Some("json"));
    let salt = query_param(req, "s").unwrap();
    assert_eq!(
        query_param(req, "t").unwrap(),
        super::auth::token("sesame", &salt)
    );
    assert!(query_param(req, "p").is_none());
    assert!(query_param(req, "jwt").is_none());
}

#[tokio::test]
async fn salt_changes_between_requests() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", ok_empty());
    let c = client(&t);
    c.ping().await.unwrap();
    c.ping().await.unwrap();
    let reqs = t.requests_to("ping");
    assert_ne!(query_param(&reqs[0], "s"), query_param(&reqs[1], "s"));
}

#[tokio::test]
async fn api_key_mode_after_probe() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", ok_empty());
    let c = client(&t);
    c.set_api_key(Credential::new("key-1"));
    c.ping().await.unwrap();
    let req = &t.requests_to("ping")[0];
    assert_eq!(query_param(req, "apiKey").as_deref(), Some("key-1"));
    assert!(query_param(req, "u").is_none());
    assert!(query_param(req, "t").is_none());
}

#[tokio::test]
async fn failed_status_maps_to_typed_error() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING_ERROR.into()));
    let c = client(&t);
    let e = c.ping().await.unwrap_err();
    assert!(matches!(e, SubsonicError::Auth(_)));
    assert_eq!(e.kind(), crate::api::ErrorKind::Auth);
}

#[tokio::test]
async fn http_statuses_map_to_errors() {
    let t = FakeTransport::new();
    t.on_endpoint("getSong", FakeReply::Status(404));
    t.on_endpoint("getAlbum", FakeReply::Status(401));
    t.on_endpoint("getArtist", FakeReply::Status(500));
    t.on_endpoint(
        "getGenres",
        FakeReply::Bytes {
            content_type: "text/html".into(),
            body: bytes::Bytes::from_static(b"<html>"),
        },
    );
    let c = client(&t);
    assert!(matches!(
        c.song("x").await.unwrap_err(),
        SubsonicError::NotFound(_)
    ));
    assert!(matches!(
        c.album("x").await.unwrap_err(),
        SubsonicError::Auth(_)
    ));
    assert!(matches!(
        c.artist("x").await.unwrap_err(),
        SubsonicError::Server { code: 500, .. }
    ));
    assert!(matches!(
        c.genres().await.unwrap_err(),
        SubsonicError::Protocol(_)
    ));
}

#[tokio::test]
async fn retries_transient_failures_then_succeeds() {
    let t = FakeTransport::new();
    t.on_endpoint_seq(
        "ping",
        vec![
            FakeReply::Network,
            FakeReply::Status(503),
            FakeReply::Json(PING.into()),
        ],
    );
    let c = client_with_retries(&t, 4);
    c.ping().await.unwrap();
    assert_eq!(t.requests_to("ping").len(), 3);
}

#[tokio::test]
async fn gives_up_after_max_attempts() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Timeout);
    let c = client_with_retries(&t, 3);
    let e = c.ping().await.unwrap_err();
    assert!(matches!(e, SubsonicError::Network(_)));
    assert_eq!(t.requests_to("ping").len(), 3);
}

#[tokio::test]
async fn auth_errors_are_not_retried() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING_ERROR.into()));
    let c = client_with_retries(&t, 4);
    c.ping().await.unwrap_err();
    assert_eq!(t.requests_to("ping").len(), 1);
}

#[test]
fn backoff_grows_and_caps() {
    let p = RetryPolicy::default();
    assert_eq!(p.backoff_for(1), std::time::Duration::from_millis(500));
    assert_eq!(p.backoff_for(2), std::time::Duration::from_millis(1000));
    assert_eq!(p.backoff_for(3), std::time::Duration::from_millis(2000));
    assert_eq!(p.backoff_for(10), std::time::Duration::from_secs(8));
}

#[tokio::test]
async fn search3_parses_navidrome_sample_and_converts() {
    let t = FakeTransport::new();
    t.on_endpoint("search3", FakeReply::Json(SEARCH3.into()));
    let c = client(&t);
    let r = c.search3("", Search3Page::songs(500, 1000)).await.unwrap();
    let req = &t.requests_to("search3")[0];
    assert_eq!(query_param(req, "query").as_deref(), Some(""));
    assert_eq!(query_param(req, "songCount").as_deref(), Some("500"));
    assert_eq!(query_param(req, "songOffset").as_deref(), Some("1000"));
    assert_eq!(query_param(req, "albumCount").as_deref(), Some("0"));
    assert_eq!(r.song.len(), 2);
    let s1 = track_from_child("srv1", &r.song[0]);
    assert_eq!(s1.id, "s1");
    assert_eq!(s1.title, "Roygbiv");
    assert_eq!(s1.duration_ms, 151_200);
    assert_eq!(s1.rating, 5);
    assert!(s1.loved);
    assert_eq!(s1.play_count, 7);
    assert_eq!(s1.bit_rate, Some(900));
    assert_eq!(s1.sample_rate, Some(44100));
    assert_eq!(s1.bit_depth, Some(16));
    assert_eq!(s1.channels, Some(2));
    assert_eq!(s1.album_artist.as_deref(), Some("Boards of Canada"));
    assert_eq!(s1.genre.as_deref(), Some("IDM"));
    assert_eq!(s1.replay_gain.as_ref().unwrap().track_gain_db, Some(-6.2));
    assert_eq!(s1.replay_gain.as_ref().unwrap().album_peak, Some(0.98));
    assert_eq!(s1.sonic.as_ref().unwrap().bpm, Some(81.0));
    assert_eq!(s1.sonic.as_ref().unwrap().mood.as_deref(), Some("chill"));
    assert_eq!(s1.comment.as_deref(), Some("a comment"));
    assert_eq!(s1.music_brainz_id.as_deref(), Some("mb-s1"));
    assert!(!s1.explicit);
    assert!(s1.created.is_some());
    assert!(s1.last_played.is_some());
    let s2 = track_from_child("srv1", &r.song[1]);
    assert!(s2.explicit);
    assert_eq!(s2.rating, 0);
    assert!(!s2.loved);
    assert_eq!(s2.bit_depth, None, "zero bit depth is unknown");
    assert!(
        s2.sonic.is_none(),
        "bpm 0 and no moods → no sonic attributes"
    );
    let a = album_from_id3("srv1", &r.album[0]);
    assert_eq!(a.song_count, 18);
    assert_eq!(a.duration_ms, 4_200_000);
    assert_eq!(a.rating, 4);
    assert!(a.loved);
    assert_eq!(a.replay_gain.as_ref().unwrap().album_gain_db, Some(-7.5));
    assert_eq!(r.artist[0].name, "Boards of Canada");
}

#[tokio::test]
async fn empty_search_result_is_ok() {
    let t = FakeTransport::new();
    t.on_endpoint("search3", FakeReply::Json(SEARCH3_EMPTY.into()));
    let c = client(&t);
    let r = c.search3("", Search3Page::songs(500, 0)).await.unwrap();
    assert!(r.song.is_empty() && r.album.is_empty() && r.artist.is_empty());
}

#[tokio::test]
async fn structured_lyrics_v2_round_trip() {
    let t = FakeTransport::new();
    t.on_endpoint("getLyricsBySongId", FakeReply::Json(LYRICS_V2.into()));
    let c = client(&t);
    let l = c.lyrics_by_song_id("s1").await.unwrap();
    assert_eq!(l.len(), 2);
    let main = &l[0];
    assert!(main.synced);
    assert_eq!(main.lang, "eng");
    assert_eq!(main.offset, Some(100));
    assert_eq!(main.kind.as_deref(), Some("main"));
    assert_eq!(main.line.len(), 3);
    assert_eq!(main.line[1].start, Some(4000));
    assert_eq!(main.agents.len(), 2);
    assert_eq!(main.agents[1].role, "bg");
    assert_eq!(main.cue_line.len(), 2);
    assert_eq!(main.cue_line[0].agent_id.as_deref(), Some("v1"));
    assert_eq!(main.cue_line[0].cue.len(), 3);
    assert_eq!(main.cue_line[0].cue[1].value, "orange");
    assert_eq!(main.cue_line[0].cue[1].byte_start, Some(4));
    assert_eq!(main.cue_line[0].cue[1].end, Some(2900));
    assert!(!l[1].synced);
    assert_eq!(l[1].lang, "und");
    assert!(l[1].line[0].start.is_none());
}

/// OpenSubsonic songLyrics v2 only includes `agents`/`cueLine` when the
/// request carries `enhanced=true` (Navidrome ≥ 0.63; a v1 server ignores
/// it). Without it every answer is line-level and the syllable tier never
/// appears. The fixture is a real Navidrome 0.64 answer: 44 lines, 57 cue
/// lines (13 background-vocal ones under a `bg` agent), inclusive byte
/// offsets, and it adapts the same way the actor does it (through the
/// `lyrics::raw` types) to syllable lyrics with the bg lines as sub-voices.
#[tokio::test]
async fn lyrics_request_asks_for_enhanced_and_parses_navidrome_cue_lines() {
    use crate::api::LyricsTier;
    let t = FakeTransport::new();
    t.on_endpoint("getLyricsBySongId", FakeReply::Json(LYRICS_ENHANCED.into()));
    let c = client(&t);
    let l = c.lyrics_by_song_id("s1").await.unwrap();
    let req = &t.requests_to("getLyricsBySongId")[0];
    assert_eq!(query_param(req, "id").as_deref(), Some("s1"));
    assert_eq!(query_param(req, "enhanced").as_deref(), Some("true"));

    assert_eq!(l.len(), 1);
    let e = &l[0];
    assert!(e.synced);
    assert_eq!(e.kind.as_deref(), Some("main"));
    assert_eq!(e.line.len(), 44);
    assert_eq!(e.cue_line.len(), 57);
    let agents: Vec<(&str, &str)> = e
        .agents
        .iter()
        .map(|a| (a.id.as_str(), a.role.as_str()))
        .collect();
    assert_eq!(agents, vec![("v1", "main"), ("__nd_bg__|v1", "bg")]);
    assert_eq!(
        e.cue_line
            .iter()
            .filter(|c| c.agent_id.as_deref() == Some("__nd_bg__|v1"))
            .count(),
        13
    );
    let first = &e.cue_line[0];
    assert_eq!(first.index, 0);
    assert_eq!((first.start, first.end), (Some(13750), Some(15350)));
    assert_eq!(first.cue[0].value, "I");
    assert_eq!(
        (first.cue[0].byte_start, first.cue[0].byte_end),
        (Some(0), Some(0))
    );
    assert_eq!(first.cue[5].value, "ti");
    assert_eq!(first.cue[6].value, "tle");
    assert_eq!(
        (first.cue[6].byte_start, first.cue[6].byte_end),
        (Some(21), Some(23))
    );

    // The actor's conversion path: serde round-trip into the lyrics types.
    let raw: Vec<crate::lyrics::raw::StructuredLyrics> =
        serde_json::from_value(serde_json::to_value(&l).unwrap()).unwrap();
    let adapted = crate::lyrics::adapt_list("s1", &raw, crate::api::LyricsSource::Server).unwrap();
    assert_eq!(adapted.tier, LyricsTier::Syllable);
    assert_eq!(adapted.lines.len(), 57);
    assert_eq!(adapted.lines.iter().filter(|x| x.background).count(), 13);
    assert_eq!(adapted.lines[0].syllables.len(), 7);
    assert!(adapted.lines[0].syllables[5].joined, "ti+tle");
    assert!(!adapted.lines[0].syllables[0].joined, "I lost");
    assert_eq!(adapted.lines[3].text, "(Yeah, yeah)");
    assert!(adapted.lines[3].background);
}

/// The in-process fake (actor tests) serves cue lines the way a server asked
/// with `enhanced=true` does.
#[tokio::test]
async fn fake_server_serves_enhanced_cue_lines() {
    use super::fake::FakeServer;
    let s = FakeServer::new("srv", "alice");
    s.set_lyrics_json("t", LYRICS_ENHANCED).unwrap();
    let l = s.lyrics_by_song_id("t").await.unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].cue_line.len(), 57);
    assert_eq!(l[0].agents.len(), 2);
    assert!(l[0].cue_line[0].cue.iter().all(|c| c.start.is_some()));
    assert!(s.lyrics_by_song_id("other").await.unwrap().is_empty());
    assert!(s.set_lyrics_json("bad", "not json").is_err());
}

#[tokio::test]
async fn playlists_and_playlist_parse() {
    let t = FakeTransport::new();
    t.on_endpoint("getPlaylists", FakeReply::Json(PLAYLISTS.into()));
    t.on_endpoint("getPlaylist", FakeReply::Json(PLAYLIST.into()));
    let c = client(&t);
    let ps = c.playlists().await.unwrap();
    assert_eq!(ps.len(), 2);
    let mine = playlist_from_body("srv1", Some("alice"), &ps[0]);
    assert!(mine.is_mine && !mine.is_smart);
    let smart = playlist_from_body("srv1", Some("alice"), &ps[1]);
    assert!(!smart.is_mine && smart.is_smart);
    assert_eq!(smart.duration_ms, 9_000_000);
    let p = c.playlist("pl1").await.unwrap();
    assert_eq!(p.entry.len(), 2);
    assert_eq!(p.playlist.song_count, 2);
}

#[tokio::test]
async fn playlist_mutations_send_correct_params() {
    let t = FakeTransport::new();
    t.on_endpoint("updatePlaylist", ok_empty());
    t.on_endpoint("createPlaylist", FakeReply::Json(PLAYLIST.into()));
    t.on_endpoint("deletePlaylist", ok_empty());
    let c = client(&t);
    c.update_playlist(
        "pl1",
        PlaylistUpdate {
            name: Some("New".into()),
            public: Some(true),
            song_ids_to_add: vec!["a".into(), "b".into()],
            song_indices_to_remove: vec![3, 1],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let req = &t.requests_to("updatePlaylist")[0];
    assert_eq!(query_param(req, "playlistId").as_deref(), Some("pl1"));
    assert_eq!(query_param(req, "name").as_deref(), Some("New"));
    assert_eq!(query_param(req, "public").as_deref(), Some("true"));
    assert_eq!(query_params(req, "songIdToAdd"), vec!["a", "b"]);
    assert_eq!(query_params(req, "songIndexToRemove"), vec!["3", "1"]);

    c.replace_playlist("pl1", &["x".into(), "y".into()])
        .await
        .unwrap();
    let req = &t.requests_to("createPlaylist")[0];
    assert_eq!(query_param(req, "playlistId").as_deref(), Some("pl1"));
    assert_eq!(query_params(req, "songId"), vec!["x", "y"]);
    c.create_playlist("Fresh", &["z".into()]).await.unwrap();
    let req = &t.requests_to("createPlaylist")[1];
    assert_eq!(query_param(req, "name").as_deref(), Some("Fresh"));
    c.delete_playlist("pl1").await.unwrap();
    assert_eq!(
        query_param(&t.requests_to("deletePlaylist")[0], "id").as_deref(),
        Some("pl1")
    );
}

#[tokio::test]
async fn star_rating_scrobble_params() {
    let t = FakeTransport::new();
    t.on_endpoint("star", ok_empty());
    t.on_endpoint("unstar", ok_empty());
    t.on_endpoint("setRating", ok_empty());
    t.on_endpoint("scrobble", ok_empty());
    let c = client(&t);
    c.star(&[
        StarTarget::Song("s1".into()),
        StarTarget::Album("al1".into()),
        StarTarget::Artist("ar1".into()),
    ])
    .await
    .unwrap();
    let req = &t.requests_to("star")[0];
    assert_eq!(query_param(req, "id").as_deref(), Some("s1"));
    assert_eq!(query_param(req, "albumId").as_deref(), Some("al1"));
    assert_eq!(query_param(req, "artistId").as_deref(), Some("ar1"));
    c.unstar(&[StarTarget::Song("s1".into())]).await.unwrap();
    c.set_rating("s1", 9).await.unwrap();
    assert_eq!(
        query_param(&t.requests_to("setRating")[0], "rating").as_deref(),
        Some("5"),
        "clamped"
    );
    c.scrobble("s1", Some(1_714_566_896_789.4), true)
        .await
        .unwrap();
    let req = &t.requests_to("scrobble")[0];
    assert_eq!(query_param(req, "submission").as_deref(), Some("true"));
    assert_eq!(query_param(req, "time").as_deref(), Some("1714566896789"));
    c.scrobble("s1", None, false).await.unwrap();
    let req = &t.requests_to("scrobble")[1];
    assert_eq!(query_param(req, "submission").as_deref(), Some("false"));
    assert!(query_param(req, "time").is_none());
}

#[tokio::test]
async fn album_list2_types() {
    let t = FakeTransport::new();
    t.on_endpoint("getAlbumList2", FakeReply::Json(ALBUM_LIST2.into()));
    let c = client(&t);
    let albums = c
        .album_list2(
            AlbumListType::ByYear {
                from_year: 1990,
                to_year: 2000,
            },
            50,
            100,
        )
        .await
        .unwrap();
    assert_eq!(albums.len(), 2);
    assert!(album_from_id3("srv1", &albums[1]).is_compilation);
    let req = &t.requests_to("getAlbumList2")[0];
    assert_eq!(query_param(req, "type").as_deref(), Some("byYear"));
    assert_eq!(query_param(req, "fromYear").as_deref(), Some("1990"));
    assert_eq!(query_param(req, "size").as_deref(), Some("50"));
    assert_eq!(query_param(req, "offset").as_deref(), Some("100"));
    c.album_list2(AlbumListType::ByGenre("IDM".into()), 10, 0)
        .await
        .unwrap();
    let req = &t.requests_to("getAlbumList2")[1];
    assert_eq!(query_param(req, "type").as_deref(), Some("byGenre"));
    assert_eq!(query_param(req, "genre").as_deref(), Some("IDM"));
}

#[tokio::test]
async fn artists_and_genres_parse() {
    let t = FakeTransport::new();
    t.on_endpoint("getArtists", FakeReply::Json(ARTISTS.into()));
    t.on_endpoint("getGenres", FakeReply::Json(GENRES.into()));
    let c = client(&t);
    let idx = c.artists().await.unwrap();
    assert_eq!(idx.index.len(), 2);
    assert_eq!(
        idx.index[1].artist[0].starred.as_deref(),
        Some("2024-01-01T00:00:00Z")
    );
    let g = c.genres().await.unwrap();
    assert_eq!(g[0].value, "IDM");
    assert_eq!(g[0].song_count, 120);
}

#[tokio::test]
async fn sonic_similarity_requires_capability_and_parses() {
    let t = FakeTransport::new();
    t.on_endpoint("getSonicSimilarTracks", FakeReply::Json(SONIC.into()));
    let c = client(&t);
    assert!(matches!(
        c.sonic_similar_tracks("s1", 10).await.unwrap_err(),
        SubsonicError::Unsupported(_)
    ));
    assert!(
        t.requests_to("getSonicSimilarTracks").is_empty(),
        "never probe by trying"
    );
    c.set_capabilities(crate::api::ServerCapabilities {
        sonic_similarity: true,
        ..Default::default()
    });
    let m = c.sonic_similar_tracks("s1", 10).await.unwrap();
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].entry.id, "s2");
    assert_eq!(m[0].similarity, 0.93);
    let req = &t.requests_to("getSonicSimilarTracks")[0];
    assert_eq!(query_param(req, "count").as_deref(), Some("10"));
}

#[tokio::test]
async fn scan_status_parses() {
    let t = FakeTransport::new();
    t.on_endpoint("getScanStatus", FakeReply::Json(SCAN.into()));
    let c = client(&t);
    let s = c.scan_status().await.unwrap();
    assert!(!s.scanning);
    assert_eq!(s.count, Some(12345.0));
    assert_eq!(
        super::types::parse_iso_ms(s.last_scan.as_deref().unwrap()),
        Some(1_718_154_123_456.0)
    );
}

#[test]
fn url_builders() {
    let t = FakeTransport::new();
    let c = client(&t);
    let u = c.cover_art_url("al-1", Some(320));
    assert_eq!(endpoint_of(&u), "getCoverArt");
    assert!(u.query().unwrap().contains("size=320"));
    assert!(u.query().unwrap().contains("id=al-1"));
    assert!(u.query().unwrap().contains("t="));

    let raw = c.stream_url("s1", &StreamOptions::default());
    assert!(raw.query().unwrap().contains("format=raw"));
    let opts = StreamOptions {
        format: Some("opus".into()),
        max_bit_rate: Some(128),
        time_offset_s: Some(30),
        estimate_content_length: true,
    };
    let u = c.stream_url("s1", &opts);
    let q = u.query().unwrap();
    assert!(
        q.contains("format=opus")
            && q.contains("maxBitRate=128")
            && q.contains("estimateContentLength=true")
    );
    assert!(
        !q.contains("timeOffset"),
        "timeOffset only with the transcodeOffset capability"
    );
    c.set_capabilities(crate::api::ServerCapabilities {
        transcode_offset: true,
        ..Default::default()
    });
    assert!(c
        .stream_url("s1", &opts)
        .query()
        .unwrap()
        .contains("timeOffset=30"));
    let bitrate_only = c.stream_url(
        "s1",
        &StreamOptions {
            max_bit_rate: Some(192),
            ..Default::default()
        },
    );
    assert!(!bitrate_only.query().unwrap().contains("format="));
    assert_eq!(endpoint_of(&c.download_url("s1")), "download");
}

#[tokio::test]
async fn probe_folds_ping_extensions_and_native_api() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING.into()));
    t.on_endpoint(
        "getOpenSubsonicExtensions",
        FakeReply::Json(EXTENSIONS.into()),
    );
    t.on_path(
        "/nd/auth/login",
        Some(Method::Post),
        FakeReply::Json(NATIVE_LOGIN.into()),
    );
    t.on_path(
        "/nd/api/keepalive",
        Some(Method::Get),
        FakeReply::Json(r#"{"response":"ok","id":"keepalive"}"#.into()),
    );
    let c = client(&t);
    let caps = c.probe().await.unwrap();
    assert_eq!(caps.server_version.as_deref(), Some("0.63.1 (abcd1234)"));
    assert!(caps.open_subsonic && caps.meets_floor);
    assert!(caps.transcode_offset && caps.form_post && caps.song_lyrics && caps.sonic_similarity);
    assert!(!caps.api_key_authentication);
    assert!(caps.native_api);
    assert_eq!(caps.extensions.len(), 6);
    assert_eq!(c.capabilities(), caps);
    // login body carried the password as JSON, never as a query param
    let login = t
        .requests()
        .into_iter()
        .find(|r| r.url.path() == "/nd/auth/login")
        .unwrap();
    let body = String::from_utf8(login.body.unwrap().to_vec()).unwrap();
    assert!(body.contains("\"password\":\"sesame\""));
    assert!(login.url.query().is_none());
    let ka = t
        .requests()
        .into_iter()
        .find(|r| r.url.path().starts_with("/nd/api/keepalive"))
        .unwrap();
    assert_eq!(
        headers_map(&ka)["x-nd-authorization"],
        "Bearer eyJhbGciOiJIUzI1NiJ9.e30.abc"
    );
}

#[tokio::test]
async fn probe_without_native_api_or_extensions() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING.into()));
    t.on_endpoint("getOpenSubsonicExtensions", FakeReply::Status(500));
    t.on_path("/nd/auth/login", Some(Method::Post), FakeReply::Status(404));
    let c = client(&t);
    let caps = c.probe().await.unwrap();
    assert!(
        caps.open_subsonic
            && caps.extensions.is_empty()
            && !caps.native_api
            && !caps.sonic_similarity
    );
}

#[tokio::test]
async fn probe_old_server_fails_floor() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING.replace("0.63.1", "0.62.0")));
    t.on_endpoint(
        "getOpenSubsonicExtensions",
        FakeReply::Json(EXTENSIONS.into()),
    );
    t.on_path("/nd/auth/login", Some(Method::Post), FakeReply::Status(404));
    let c = client(&t);
    assert!(!c.probe().await.unwrap().meets_floor);
}

#[tokio::test]
async fn probe_propagates_auth_failure() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING_ERROR.into()));
    let c = client(&t);
    assert!(matches!(
        c.probe().await.unwrap_err(),
        SubsonicError::Auth(_)
    ));
}

#[tokio::test]
async fn native_playlist_rules_read_write_and_relogin_on_401() {
    let t = FakeTransport::new();
    t.on_path(
        "/nd/auth/login",
        Some(Method::Post),
        FakeReply::Json(NATIVE_LOGIN.into()),
    );
    t.on_path(
        "/nd/api/playlist/pl2",
        Some(Method::Get),
        FakeReply::Json(NATIVE_PLAYLIST.into()),
    );
    t.on_path(
        "/nd/api/playlist/pl2",
        Some(Method::Put),
        FakeReply::Json(NATIVE_PLAYLIST.into()),
    );
    let c = client(&t);
    let p = c.native_playlist("pl2").await.unwrap();
    assert_eq!(p.name, "Loved (smart)");
    let rules = p.rules.unwrap();
    assert_eq!(rules["all"][0]["is"]["loved"], serde_json::json!(true));
    assert_eq!(rules["limit"], serde_json::json!(100));
    // one login then the GET with the bearer header
    let reqs = t.requests();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.url.path() == "/nd/auth/login")
            .count(),
        1
    );
    let get = reqs
        .iter()
        .find(|r| r.url.path() == "/nd/api/playlist/pl2")
        .unwrap();
    assert!(headers_map(get)["x-nd-authorization"].starts_with("Bearer "));

    let update = NativePlaylistUpdate {
        rules: Some(serde_json::json!({"any": [{"contains": {"title": "x"}}]})),
        ..Default::default()
    };
    c.native_update_playlist("pl2", update.clone())
        .await
        .unwrap();
    let put = t
        .requests()
        .into_iter()
        .find(|r| r.method == Method::Put)
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&put.body.unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({"rules": {"any": [{"contains": {"title": "x"}}]}})
    );

    // token expiry: 401 once → re-login → retry
    t.clear_requests();
    t.on_path(
        "/nd/api/playlist/pl2",
        Some(Method::Get),
        FakeReply::Status(401),
    );
    t.on(
        |r| r.url.path() == "/nd/api/playlist/pl2" && r.method == Method::Get,
        vec![
            FakeReply::Status(401),
            FakeReply::Json(NATIVE_PLAYLIST.into()),
        ],
    );
    c.native_playlist("pl2").await.unwrap();
    let reqs = t.requests();
    assert_eq!(
        reqs.iter()
            .filter(|r| r.url.path() == "/nd/auth/login")
            .count(),
        1
    );
    assert_eq!(
        reqs.iter()
            .filter(|r| r.url.path() == "/nd/api/playlist/pl2")
            .count(),
        2
    );
}

#[tokio::test]
async fn native_api_impossible_with_api_key() {
    let t = FakeTransport::new();
    let c = client(&t);
    c.set_api_key(Credential::new("k"));
    assert!(matches!(
        c.native_playlist("pl2").await.unwrap_err(),
        SubsonicError::Unsupported(_)
    ));
}

#[tokio::test]
async fn download_to_file_writes_and_maps_status() {
    let t = FakeTransport::new();
    t.on_endpoint(
        "download",
        FakeReply::Bytes {
            content_type: "audio/flac".into(),
            body: bytes::Bytes::from_static(b"FLAC...."),
        },
    );
    t.on_endpoint("stream", FakeReply::Status(404));
    let c = client(&t);
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a/b/track.flac");
    let out = c
        .download_to_file(c.download_url("s1"), &dest)
        .await
        .unwrap();
    assert_eq!(out.bytes, 8);
    assert_eq!(std::fs::read(&dest).unwrap(), b"FLAC....");
    let e = c
        .download_to_file(
            c.stream_url("s1", &StreamOptions::default()),
            &dir.path().join("x"),
        )
        .await
        .unwrap_err();
    assert!(matches!(e, SubsonicError::NotFound(_)));
    assert!(!dir.path().join("x").exists());
}

#[tokio::test]
async fn concurrency_is_bounded() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Slow {
        inflight: AtomicUsize,
        max: AtomicUsize,
    }
    impl super::transport::HttpTransport for Slow {
        fn execute(
            &self,
            _r: super::transport::HttpRequest,
        ) -> futures::future::BoxFuture<
            '_,
            Result<super::transport::HttpResponse, super::transport::TransportError>,
        > {
            Box::pin(async move {
                let n = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max.fetch_max(n, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                self.inflight.fetch_sub(1, Ordering::SeqCst);
                Ok(super::transport::HttpResponse {
                    status: 200,
                    content_type: None,
                    body: bytes::Bytes::from(PING),
                })
            })
        }
        fn download(
            &self,
            _u: Url,
            _d: &std::path::Path,
            _p: Option<super::transport::ProgressFn>,
        ) -> futures::future::BoxFuture<
            '_,
            Result<super::transport::DownloadOutcome, super::transport::TransportError>,
        > {
            Box::pin(async {
                Ok(super::transport::DownloadOutcome {
                    status: 200,
                    content_type: None,
                    bytes: 0,
                })
            })
        }
    }
    let slow = Arc::new(Slow {
        inflight: AtomicUsize::new(0),
        max: AtomicUsize::new(0),
    });
    let mut cfg = ClientConfig::new(
        "s",
        Url::parse("https://x.example").unwrap(),
        AuthMode::ApiKey {
            api_key: Credential::new("k"),
        },
    );
    cfg.max_concurrent = 2;
    let c = Arc::new(Client::new(cfg, slow.clone()));
    let mut handles = vec![];
    for _ in 0..8 {
        let c = c.clone();
        handles.push(tokio::spawn(async move { c.ping().await.unwrap() }));
    }
    for h in handles {
        h.await.unwrap();
    }
    assert!(slow.max.load(Ordering::SeqCst) <= 2);
}

#[tokio::test]
async fn fake_server_behaves_like_a_server() {
    use super::fake::FakeServer;
    let s = FakeServer::new("srv", "alice");
    s.add_song(FakeServer::song("a", "Alpha", "al1", "ar1", 200.0));
    s.add_song(FakeServer::song("b", "Beta", "al1", "ar1", 100.0));
    s.add_playlist("p", "P", "alice", &["a", "b"], false);
    s.set_rating("a", 4).await.unwrap();
    assert_eq!(s.rating_of("a"), 4);
    s.star(&[StarTarget::Song("b".into())]).await.unwrap();
    assert!(s.starred("b"));
    s.update_playlist(
        "p",
        PlaylistUpdate {
            song_indices_to_remove: vec![0],
            song_ids_to_add: vec!["a".into()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(s.playlist_song_ids("p"), vec!["b", "a"]);
    s.fail_next(SubsonicError::Network("x".into()), 1);
    assert!(s.ping().await.is_err());
    assert!(s.ping().await.is_ok());
    let r = s.search3("", Search3Page::songs(1, 1)).await.unwrap();
    assert_eq!(r.song.len(), 1);
    assert_eq!(r.song[0].id, "b");
}

// ---------------------------------------------------------------------------
// Review fixes: H1/H2/M1/M2/L5 (server-facing)
// ---------------------------------------------------------------------------

fn client_at(t: &FakeTransport, base: &str) -> Client {
    let mut cfg = ClientConfig::new(
        "srv1",
        Url::parse(base).unwrap(),
        AuthMode::Password {
            username: "alice".into(),
            password: Credential::new("sesame"),
        },
    );
    cfg.retry = RetryPolicy::none();
    Client::new(cfg, Arc::new(t.clone()))
}

/// H2: the native login (plaintext password in the body) is never attempted
/// over plain `http://` to a non-loopback host; loopback and https are fine.
#[tokio::test]
async fn native_login_only_over_https_or_loopback() {
    for (base, expect_login) in [
        ("http://music.example.org/nd/", false),
        ("http://192.168.1.10:4533/", false),
        ("http://127.0.0.1:4533/", true),
        ("http://localhost:4533/", true),
        ("http://[::1]:4533/", true),
        ("https://music.example.org/", true),
    ] {
        let t = FakeTransport::new();
        t.on_endpoint("ping", FakeReply::Json(PING.into()));
        t.on_endpoint(
            "getOpenSubsonicExtensions",
            FakeReply::Json(EXTENSIONS.into()),
        );
        t.on(
            |r| r.url.path().ends_with("/auth/login"),
            vec![FakeReply::Json(NATIVE_LOGIN.into())],
        );
        t.on(
            |r| r.url.path().contains("/api/keepalive"),
            vec![FakeReply::Json(
                r#"{"response":"ok","id":"keepalive"}"#.into(),
            )],
        );
        let c = client_at(&t, base);
        let caps = c.probe().await.unwrap();
        let login_sent = t
            .requests()
            .iter()
            .any(|r| r.url.path().ends_with("/auth/login"));
        assert_eq!(login_sent, expect_login, "{base}: login attempted");
        assert_eq!(caps.native_api, expect_login, "{base}: native_api");
        if !expect_login {
            // nothing with the password ever left the client
            assert!(t.requests().iter().all(|r| r
                .body
                .as_ref()
                .is_none_or(|b| !b.windows(6).any(|w| w == b"sesame"))));
            // and a direct native call refuses rather than logging in
            assert!(matches!(
                c.native_playlist("pl2").await.unwrap_err(),
                SubsonicError::Unsupported(_)
            ));
        }
    }
}

/// M1: a base URL that cannot take a path is an error, never a panic.
#[tokio::test]
async fn cannot_be_a_base_url_is_an_error_not_a_panic() {
    let t = FakeTransport::new();
    t.on_endpoint("ping", FakeReply::Json(PING.into()));
    let c = client_at(&t, "mailto:x@y");
    assert!(matches!(
        c.rest_url("ping", &[]).unwrap_err(),
        SubsonicError::Protocol(_)
    ));
    assert!(matches!(
        c.ping().await.unwrap_err(),
        SubsonicError::Protocol(_)
    ));
    assert!(matches!(
        c.probe().await.unwrap_err(),
        SubsonicError::Protocol(_)
    ));
    assert!(matches!(
        c.native_playlist("x").await.unwrap_err(),
        SubsonicError::Protocol(_) | SubsonicError::Unsupported(_)
    ));
    // URL builders degrade to the base instead of panicking
    let _ = c.stream_url("s1", &StreamOptions::default());
    let _ = c.cover_art_url("s1", None);
    let _ = c.download_url("s1");
    // and try_new refuses up front
    let cfg = ClientConfig::new(
        "srv1",
        Url::parse("mailto:x@y").unwrap(),
        AuthMode::ApiKey {
            api_key: Credential::new("k"),
        },
    );
    assert!(Client::try_new(cfg, Arc::new(t.clone())).is_err());
}

#[test]
fn base_url_validation() {
    use super::validate_base_url;
    let ok = |s: &str| validate_base_url(&Url::parse(s).unwrap()).is_ok();
    assert!(ok("https://music.example.org/"));
    assert!(ok("http://192.168.1.2:4533/nd"));
    assert!(!ok("mailto:x@y"));
    assert!(!ok("data:text/plain,hi"));
    assert!(!ok("javascript:alert(1)"));
    assert!(!ok("ftp://music.example.org/"));
    assert!(!ok("file:///etc/passwd"));
    assert!(!ok("https://alice:pw@music.example.org/"));
    assert!(!ok("https://alice@music.example.org/"));
}

/// M2: a 2xx whose body is a Subsonic error envelope (or empty, or text)
/// is a typed error and leaves no file behind.
#[tokio::test]
async fn download_to_file_rejects_json_envelope_text_and_empty_bodies() {
    let t = FakeTransport::new();
    t.on_endpoint("download", FakeReply::Json(PING_ERROR.into()));
    t.on_endpoint(
        "stream",
        FakeReply::Bytes {
            content_type: "text/html; charset=utf-8".into(),
            body: bytes::Bytes::from_static(b"<html>login</html>"),
        },
    );
    t.on_endpoint(
        "getCoverArt",
        FakeReply::Bytes {
            content_type: "image/jpeg".into(),
            body: bytes::Bytes::new(),
        },
    );
    let c = client(&t);
    let dir = tempfile::tempdir().unwrap();

    let dest = dir.path().join("t.flac");
    let e = c
        .download_to_file(c.download_url("s1"), &dest)
        .await
        .unwrap_err();
    assert!(matches!(e, SubsonicError::Auth(_)), "{e}");
    assert!(!dest.exists(), "json envelope must not be kept as media");

    let dest = dir.path().join("t.opus");
    let e = c
        .download_to_file(c.stream_url("s1", &StreamOptions::default()), &dest)
        .await
        .unwrap_err();
    assert!(matches!(e, SubsonicError::Protocol(_)), "{e}");
    assert!(!dest.exists());

    let dest = dir.path().join("c.img");
    let e = c
        .download_to_file(c.cover_art_url("c", Some(320)), &dest)
        .await
        .unwrap_err();
    assert!(matches!(e, SubsonicError::Protocol(_)), "{e}");
    assert!(!dest.exists());
}

/// H1: the transport refuses a destination with `..` in it, and a root-bound
/// transport refuses anything outside its root.
#[tokio::test]
async fn download_refuses_traversing_destinations() {
    use super::transport::check_download_dest;
    use std::path::Path;
    let dir = tempfile::tempdir().unwrap();
    let t = FakeTransport::new();
    t.on_endpoint(
        "download",
        FakeReply::Bytes {
            content_type: "audio/flac".into(),
            body: bytes::Bytes::from_static(b"FLAC"),
        },
    );
    let c = client(&t);
    let evil = dir.path().join("downloads/srv/abc.../../../../evil");
    let e = c
        .download_to_file(c.download_url("s1"), &evil)
        .await
        .unwrap_err();
    assert!(matches!(e, SubsonicError::Io(_)), "{e}");
    assert!(!dir.path().join("evil").exists());
    assert!(!dir.path().join("downloads").exists());

    let root = Path::new("/data/hocket");
    assert!(check_download_dest(Path::new("/data/hocket/downloads/s/t.flac"), Some(root)).is_ok());
    assert!(check_download_dest(Path::new("/data/hocket/downloads/s/t.flac"), None).is_ok());
    assert!(check_download_dest(Path::new("/data/hocket/downloads/../../x"), Some(root)).is_err());
    assert!(check_download_dest(Path::new("/data/other/x"), Some(root)).is_err());
    assert!(check_download_dest(Path::new("/"), None).is_err());
}

/// A tiny one-shot HTTP server for the reqwest-level tests: answers every
/// connection with `response` and records the request line.
async fn one_shot_http(response: &'static str) -> (String, Arc<parking_lot::Mutex<Vec<String>>>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().push(head);
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://{addr}"), seen)
}

/// H2: the production transport never follows a redirect (which would
/// forward the auth query / native JWT / login body elsewhere), and never
/// puts the URL (with its `t=`/`s=` query) into an error message.
#[tokio::test]
async fn reqwest_transport_does_not_follow_redirects_or_leak_urls() {
    use super::transport::{HttpRequest, HttpTransport, ReqwestTransport, TransportError};
    let (base, seen) = one_shot_http(
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await;
    let tr = ReqwestTransport::new("hocket-test", std::time::Duration::from_secs(5)).unwrap();
    let url = Url::parse(&format!("{base}/rest/ping?t=secrettoken&s=salt")).unwrap();
    let resp = tr
        .execute(HttpRequest::get(url.clone()).with_header("x-nd-authorization", "Bearer j"))
        .await
        .unwrap();
    assert_eq!(resp.status, 302, "redirect surfaced, not followed");
    assert_eq!(seen.lock().len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("t.flac");
    let out = tr.download(url, &dest, None).await.unwrap();
    assert_eq!(out.status, 302);
    assert!(!dest.exists());
    assert_eq!(seen.lock().len(), 2, "no second request to the Location");

    // connection refused → error text must not carry the URL/query
    let dead = Url::parse("http://127.0.0.1:1/rest/ping?t=secrettoken&s=salt").unwrap();
    let e = tr
        .execute(HttpRequest::get(dead.clone()))
        .await
        .unwrap_err();
    let msg = e.to_string();
    assert!(matches!(e, TransportError::Network(_)), "{msg}");
    assert!(
        !msg.contains("secrettoken") && !msg.contains("127.0.0.1"),
        "{msg}"
    );
    let e = tr.download(dead, &dest, None).await.unwrap_err();
    assert!(!e.to_string().contains("secrettoken"));
}

/// L5: an API body above the cap is refused up front.
#[tokio::test]
async fn reqwest_transport_caps_response_bodies() {
    use super::transport::{HttpRequest, HttpTransport, ReqwestTransport, TransportError};
    let (base, _) = one_shot_http(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 99999999999\r\nConnection: close\r\n\r\n{",
    )
    .await;
    let tr = ReqwestTransport::new("hocket-test", std::time::Duration::from_secs(5)).unwrap();
    let url = Url::parse(&format!("{base}/rest/search3")).unwrap();
    let e = tr.execute(HttpRequest::get(url)).await.unwrap_err();
    assert!(matches!(e, TransportError::TooLarge), "{e}");
    assert!(matches!(SubsonicError::from(e), SubsonicError::Protocol(_)));
}

/// M8/H3 (transport half): a failed stream leaves no `.part` behind and a
/// successful one leaves only `dest`; temp names are unique per call.
#[tokio::test]
async fn reqwest_download_cleans_up_temp_files() {
    use super::transport::{part_path, HttpTransport, ReqwestTransport};
    use std::path::Path;
    let a = part_path(Path::new("/x/abc.flac"));
    let b = part_path(Path::new("/x/abc.flac"));
    assert_ne!(a, b);
    assert!(a.to_string_lossy().ends_with(".part"));
    assert!(a.to_string_lossy().contains("abc.flac."));
    assert_ne!(
        part_path(Path::new("/x/abc.mp3")).file_name(),
        a.file_name()
    );

    // body shorter than Content-Length → the stream errors mid-way
    let (base, _) = one_shot_http(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: 100\r\nConnection: close\r\n\r\nFLAC",
    )
    .await;
    let tr = ReqwestTransport::new("hocket-test", std::time::Duration::from_secs(5)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("dl/t.flac");
    let url = Url::parse(&format!("{base}/rest/download?id=1")).unwrap();
    assert!(tr.download(url, &dest, None).await.is_err());
    let left: Vec<_> = std::fs::read_dir(dir.path().join("dl"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(left.is_empty(), "no temp or dest left behind: {left:?}");

    let (base, _) = one_shot_http(
        "HTTP/1.1 200 OK\r\nContent-Type: audio/flac\r\nContent-Length: 4\r\nConnection: close\r\n\r\nFLAC",
    )
    .await;
    let url = Url::parse(&format!("{base}/rest/download?id=1")).unwrap();
    let out = tr.download(url, &dest, None).await.unwrap();
    assert_eq!(out.bytes, 4);
    let left: Vec<_> = std::fs::read_dir(dir.path().join("dl"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, vec!["t.flac".to_string()]);
}
