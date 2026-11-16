//! Recorded InnerTube answers for tests (`fixtures/`, recorded once from
//! anonymous requests on 15 Nov 2026 and trimmed: no cookies, no personal
//! data, lyrics cut to four lines), and a fake transport that serves them.
//! The library and write answers are hand-made in the shapes the web client
//! gets (nothing signed in was recorded).

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::http::{FakeYtmHttp, HttpRequest, HttpResponse};

/// A signed-in account menu (hand-made): "Test User", channel `UCme`, no
/// Premium upsell.
pub fn account_menu() -> Value {
    json!({"actions": [{"openPopupAction": {"popup": {"multiPageMenuRenderer": {
        "header": {"activeAccountHeaderRenderer": {
            "accountName": {"runs": [{"text": "Test User"}]},
            "channelHandle": {"runs": [{"text": "@testuser"}]}}},
        "sections": [{"multiPageMenuSectionRenderer": {"items": [
            {"compactLinkRenderer": {"title": {"runs": [{"text": "Your channel"}]},
                "navigationEndpoint": {"browseEndpoint": {"browseId": "UCme"}}}}]}}]}}}}]})
}

pub fn fx(name: &str) -> Value {
    let s = match name {
        "search_songs" => include_str!("fixtures/search_songs.json"),
        "search_albums" => include_str!("fixtures/search_albums.json"),
        "search_artists" => include_str!("fixtures/search_artists.json"),
        "search_all" => include_str!("fixtures/search_all.json"),
        "album" => include_str!("fixtures/album.json"),
        "artist" => include_str!("fixtures/artist.json"),
        "home" => include_str!("fixtures/home.json"),
        "next_radio" => include_str!("fixtures/next_radio.json"),
        "lyrics" => include_str!("fixtures/lyrics.json"),
        "playlist_mixed" => include_str!("fixtures/playlist_mixed.json"),
        "player_visionos" => include_str!("fixtures/player_visionos.json"),
        "player_android_vr" => include_str!("fixtures/player_android_vr.json"),
        "player_ios" => include_str!("fixtures/player_ios.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(s).unwrap()
}

pub const SW_JS_DATA: &str = include_str!("fixtures/sw_js_data.txt");

pub fn ok(v: &Value) -> Option<HttpResponse> {
    Some(HttpResponse::json(200, &v.to_string()))
}

pub fn endpoint(r: &HttpRequest) -> &str {
    r.url
        .split('?')
        .next()
        .and_then(|p| p.rsplit_once("/youtubei/v1/"))
        .map(|(_, e)| e)
        .unwrap_or("")
}

/// Answers every InnerTube call from the fixtures; `player` by client name
/// through `players` (what each client answers).
pub fn fake(players: Arc<Mutex<Vec<(&'static str, Value)>>>) -> FakeYtmHttp {
    FakeYtmHttp::new(move |r| {
        if r.url.contains("sw.js_data") {
            return Some(HttpResponse::json(200, SW_JS_DATA));
        }
        let body = r.json();
        if body.get("continuation").is_some() {
            return ok(&json!({}));
        }
        match endpoint(r) {
            "player" => {
                let client = body["context"]["client"]["clientName"]
                    .as_str()
                    .unwrap_or("");
                players
                    .lock()
                    .iter()
                    .find(|(c, _)| *c == client)
                    .and_then(|(_, v)| ok(v))
            }
            "search" => match body["params"].as_str() {
                Some(p) if p.starts_with("EgWKAQIIAW") => ok(&fx("search_songs")),
                Some(p) if p.starts_with("EgWKAQIYAW") => ok(&fx("search_albums")),
                Some(p) if p.starts_with("EgWKAQIgAW") => ok(&fx("search_artists")),
                _ => ok(&fx("search_all")),
            },
            "browse" => match body["browseId"].as_str().unwrap_or("") {
                "MPREb_K8qWMWVqXGi" => ok(&fx("album")),
                "UCRr1xG_2WIDs18a6cIiCxeA" => ok(&fx("artist")),
                "FEmusic_home" => ok(&fx("home")),
                "MPLYt_K8qWMWVqXGi-8" => ok(&fx("lyrics")),
                "VLPLmixed" | "VLLM" => ok(&fx("playlist_mixed")),
                "FEmusic_liked_videos" => ok(&fx("search_songs")),
                "FEmusic_liked_albums" => ok(&fx("search_albums")),
                "FEmusic_library_corpus_track_artists" => ok(&fx("search_artists")),
                "FEmusic_liked_playlists" => ok(&json!({"contents": {"x": [
                    {"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Liked Music"}]},
                        "navigationEndpoint": {"browseEndpoint": {"browseId": "VLLM"}}}},
                    {"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Road trip"}]},
                        "subtitle": {"runs": [{"text": "Playlist"}, {"text": " • "}, {"text": "12 songs"}]},
                        "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLmixed",
                            "browseEndpointContextSupportedConfigs": {"browseEndpointContextMusicConfig": {"pageType": "MUSIC_PAGE_TYPE_PLAYLIST"}}}}}}
                ]}})),
                _ => None,
            },
            "next" => ok(&fx("next_radio")),
            "browse/edit_playlist" => ok(
                &json!({"status": "STATUS_SUCCEEDED", "playlistEditResults": [
                {"playlistEditVideoAddedResultData": {"videoId": "newvid00001", "setVideoId": "NEWSET0001"}}]}),
            ),
            "playlist/create" => ok(&json!({"playlistId": "PLcreated"})),
            "account/account_menu" => ok(&account_menu()),
            "like/like" | "like/removelike" | "playlist/delete" | "subscription/subscribe" => {
                ok(&json!({}))
            }
            _ => None,
        }
    })
}

pub fn players(list: &[(&'static str, &str)]) -> Arc<Mutex<Vec<(&'static str, Value)>>> {
    Arc::new(Mutex::new(list.iter().map(|(c, f)| (*c, fx(f))).collect()))
}
