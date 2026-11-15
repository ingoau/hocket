//! The InnerTube client against recorded fixtures (`fixtures/`, recorded
//! once from anonymous requests on 15 Nov 2026 and trimmed: no cookies, no
//! personal data, lyrics cut to four lines). No test touches the network.
//!
//! `playlist_mixed.json` is a public playlist page with two of its rows
//! replaced by a UGC and an OMV row from another public playlist, so it
//! holds hidden videos between songs.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::auth::Cookies;
use super::client::InnerTube;
use super::clients::ClientKind;
use super::fake::{endpoint, fake, fx, ok, players, SW_JS_DATA};
use super::http::{FakeYtmHttp, HttpResponse};
use super::parse::*;
use super::player::{JsAnswer, JsHost, JsRequest};
use super::provider::{shelf_id, sized_thumbnail, YtmProvider, MAX_RANGE};
use crate::provider::{LibraryTarget, Provider, ProviderError, ProviderWrite, StreamRequest};
use crate::util::FixedClock;

fn provider_with(http: FakeYtmHttp, signed_in: bool) -> YtmProvider {
    let clock = Arc::new(FixedClock::new(1_763_200_000_000.0));
    let cookies = signed_in.then(|| Cookies::parse("SAPISID=abc; SID=x").unwrap());
    let it = Arc::new(InnerTube::new(Arc::new(http), clock.clone(), cookies));
    YtmProvider::new(if signed_in { "UCme" } else { super::ANON }, it, clock)
}

fn mixed_entries() -> Vec<YtSong> {
    parse_playlist(&fx("playlist_mixed"), "PLmixed").entries
}

// ---------------------------------------------------------------------------
// parsers
// ---------------------------------------------------------------------------

#[test]
fn search_songs_parse_with_artists_album_duration_and_type() {
    let r = parse_search(&fx("search_songs"));
    assert!(r.songs.len() >= 5, "{}", r.songs.len());
    let s = &r.songs[0];
    assert_eq!(s.video_id, "4D7u5KF7SP8");
    assert!(s.title.starts_with("Get Lucky"));
    assert_eq!(s.artists[0].name, "Daft Punk");
    assert_eq!(s.artists[0].id.as_deref(), Some("UCRr1xG_2WIDs18a6cIiCxeA"));
    assert_eq!(s.artists.len(), 3);
    let album = s.album.as_ref().unwrap();
    assert_eq!(album.name, "Random Access Memories");
    assert_eq!(album.id.as_deref(), Some("MPREb_K8qWMWVqXGi"));
    assert_eq!(s.duration_s, Some(370));
    assert_eq!(s.video_type, Some(VideoType::Atv));
    assert!(s.is_song());
    assert!(s
        .thumbnail
        .as_deref()
        .is_some_and(|t| t.starts_with("https://")));
}

#[test]
fn search_albums_and_artists_parse() {
    let albums = parse_search(&fx("search_albums")).albums;
    assert!(!albums.is_empty());
    assert!(albums.iter().all(|a| a.browse_id.starts_with("MPRE")));
    assert!(albums
        .iter()
        .any(|a| a.artists.iter().any(|x| x.name == "Daft Punk")));
    assert!(albums.iter().any(|a| a.year.is_some()));
    let artists = parse_search(&fx("search_artists")).artists;
    assert_eq!(artists[0].name, "Daft Punk");
    assert!(artists[0].browse_id.starts_with("UC"));
    // Unfiltered search mixes kinds.
    let all = parse_search(&fx("search_all"));
    assert!(!all.songs.is_empty() || !all.albums.is_empty());
}

#[test]
fn album_page_parses_header_and_tracks_as_songs() {
    let (album, tracks) = parse_album(&fx("album"), "MPREb_K8qWMWVqXGi").unwrap();
    assert_eq!(album.title, "Random Access Memories");
    assert_eq!(album.year, Some(2013));
    assert_eq!(album.artists[0].name, "Daft Punk");
    assert_eq!(album.kind.as_deref(), Some("Album"));
    assert!(album
        .playlist_id
        .as_deref()
        .is_some_and(|p| p.starts_with("OLAK")));
    assert_eq!(tracks.len(), 6);
    assert_eq!(tracks[0].title, "Give Life Back to Music");
    // YouTube Music labels these album tracks as music videos; on an album
    // page they are songs.
    assert!(tracks.iter().all(YtSong::is_song));
    assert!(tracks.iter().all(|t| t.duration_s.is_some()));
    assert!(tracks
        .iter()
        .all(|t| t.album.as_ref().unwrap().id.as_deref() == Some("MPREb_K8qWMWVqXGi")));
}

#[test]
fn artist_page_parses_shelves() {
    let (artist, shelves) = parse_artist(&fx("artist"), "UCRr1xG_2WIDs18a6cIiCxeA");
    assert_eq!(artist.name, "Daft Punk");
    let titles: Vec<&str> = shelves.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"Top songs"), "{titles:?}");
    assert!(titles.contains(&"Albums"), "{titles:?}");
    let albums = shelves.iter().find(|s| s.title == "Albums").unwrap();
    assert!(albums.items.iter().all(|i| matches!(i, Item::Album(_))));
    let top = shelves.iter().find(|s| s.title == "Top songs").unwrap();
    assert!(top.items.iter().all(|i| matches!(i, Item::Song(_))));
    assert!(top.more.as_deref().is_some_and(|m| m.starts_with("VL")));
}

#[test]
fn playlist_keeps_hidden_videos_with_their_set_video_ids() {
    let page = parse_playlist(&fx("playlist_mixed"), "VLPLmixed");
    assert_eq!(page.playlist.id, "PLmixed");
    assert!(!page.playlist.title.is_empty());
    assert_eq!(page.entries.len(), 6);
    let visible: Vec<bool> = page.entries.iter().map(YtSong::is_song).collect();
    assert_eq!(visible, vec![true, false, true, true, false, true]);
    assert_eq!(page.entries[1].video_type, Some(VideoType::Ugc));
    assert_eq!(page.entries[4].video_type, Some(VideoType::Omv));
    assert!(page.entries.iter().all(|e| e.set_video_id.is_some()));
    assert_eq!(page.continuation, None);
}

#[test]
fn home_parses_shelves() {
    let shelves = parse_shelves(&fx("home"));
    assert!(shelves.len() >= 2);
    assert_eq!(shelves[0].title, "Quick picks");
    assert!(shelves[0].items.iter().all(|i| matches!(i, Item::Song(_))));
    assert!(shelves[1]
        .items
        .iter()
        .any(|i| matches!(i, Item::Playlist(_))));
}

#[test]
fn next_parses_the_radio_queue_and_the_lyrics_id() {
    let (queue, lyrics) = parse_next(&fx("next_radio"));
    assert_eq!(lyrics.as_deref(), Some("MPLYt_K8qWMWVqXGi-8"));
    assert!(queue.len() >= 5);
    assert_eq!(queue[0].video_id, "4D7u5KF7SP8");
    assert!(queue[1].duration_s.is_some());
    assert!(queue[1].artists.iter().any(|a| a.name == "Daft Punk"));
}

#[test]
fn lyrics_parse_with_their_source() {
    let (text, footer) = parse_lyrics(&fx("lyrics")).unwrap();
    assert_eq!(text.lines().count(), 4);
    assert!(text.starts_with("Like the legend of the phoenix"));
    assert_eq!(footer.as_deref(), Some("Source: LyricFind"));
}

#[test]
fn player_answers_parse_per_client() {
    let v = parse_player(&fx("player_visionos"));
    assert_eq!(v.status, "OK");
    let f140 = v.formats.iter().find(|f| f.itag == 140).unwrap();
    assert!(f140
        .url
        .as_deref()
        .is_some_and(|u| u.contains("googlevideo.com")));
    assert!(f140.mime.starts_with("audio/mp4"));
    assert_eq!(f140.content_length, Some(5_983_881));
    assert_eq!((f140.loudness_db.unwrap() * 100.0).round(), 314.0);
    assert_eq!(v.expires_in_s, Some(21_540));
    assert!(!v.sabr_only);
    // Video formats are not audio formats.
    assert!(v.formats.iter().all(|f| f.mime.starts_with("audio/")));
    let ios = parse_player(&fx("player_ios"));
    assert_eq!(ios.status, "OK");
    assert!(ios.sabr_only, "iOS answers without URLs anonymously");
    let vr = parse_player(&fx("player_android_vr"));
    assert!(vr.formats.iter().any(|f| f.itag == 140 && f.url.is_some()));
}

#[test]
fn small_helpers() {
    assert_eq!(parse_duration("6:10"), Some(370));
    assert_eq!(parse_duration("1:02:03"), Some(3723));
    assert_eq!(parse_duration("2013"), None);
    assert_eq!(parse_duration("a:bc"), None);
    assert_eq!(shelf_id("Quick picks"), "quick-picks");
    assert_eq!(shelf_id("Albums & EPs!"), "albums-eps");
    assert_eq!(
        sized_thumbnail(
            "https://lh3.googleusercontent.com/abc=w120-h120-l90-rj",
            Some(544)
        ),
        "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj"
    );
    assert_eq!(
        sized_thumbnail("https://i.ytimg.com/vi/x/hq.jpg", Some(544)),
        "https://i.ytimg.com/vi/x/hq.jpg"
    );
    assert_eq!(
        super::client::parse_visitor_data(SW_JS_DATA).as_deref(),
        Some("CgtGQUtFVklTSVRPUijA_test%3D%3D")
    );
    let edit = json!({"status": "STATUS_SUCCEEDED", "playlistEditResults": [
        {"playlistEditVideoAddedResultData": {"videoId": "v", "setVideoId": "S"}}]});
    assert_eq!(
        parse_edit_result(&edit).unwrap(),
        vec![("v".into(), "S".into())]
    );
    assert!(parse_edit_result(&json!({"status": "STATUS_FAILED"})).is_err());
    // Continuations: a shelf's own, and an append action's.
    let shelf = json!({"musicShelfRenderer": {"contents": [], "continuations": [{"nextContinuationData": {"continuation": "T1"}}]}});
    assert_eq!(continuation(&shelf).as_deref(), Some("T1"));
    let append = json!({"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
        {"musicResponsiveListItemRenderer": {}},
        {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "T2"}}}}]}}]});
    assert_eq!(continuation(&append).as_deref(), Some("T2"));
}

// ---------------------------------------------------------------------------
// the provider
// ---------------------------------------------------------------------------

#[tokio::test]
async fn visionos_answers_first_and_its_stream_is_reused() {
    let http = fake(players(&[("VISIONOS", "player_visionos")]));
    let p = provider_with(http.clone(), false);
    let s = p
        .resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    assert!(s.url.as_str().contains("googlevideo.com"));
    assert_eq!(s.variant, "140");
    assert_eq!(s.content_length, Some(5_983_881));
    assert_eq!(s.max_range, Some(MAX_RANGE));
    assert!(s.range_capable && !s.transcoded);
    assert!(
        (s.gain_db.unwrap() * 100.0).round() == -314.0,
        "loudnessDb maps to track gain"
    );
    assert!(s.headers["User-Agent"].contains("Safari"));
    assert_eq!(p.chooser().last_good(), Some(ClientKind::VisionOs));
    p.resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    assert_eq!(
        http.endpoints().iter().filter(|e| *e == "player").count(),
        1,
        "the cached URL is reused until it nears expiry"
    );
}

#[tokio::test]
async fn the_chain_falls_through_sabr_only_clients_and_skips_a_refused_one() {
    // visionOS answers without URLs today: android_vr is next.
    let answers = players(&[
        ("VISIONOS", "player_ios"),
        ("ANDROID_VR", "player_android_vr"),
        ("IOS", "player_ios"),
    ]);
    let http = fake(answers.clone());
    let p = provider_with(http.clone(), false);
    let s = p
        .resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    assert!(s.url.as_str().contains("c=ANDROID_VR"));
    let tried: Vec<String> = http
        .log
        .lock()
        .iter()
        .filter(|r| endpoint(r) == "player")
        .map(|r| {
            r.json()["context"]["client"]["clientName"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(tried, ["VISIONOS", "ANDROID_VR"]);
    // The client that worked is tried first next time.
    assert_eq!(
        p.chooser().order(false, false)[0].kind,
        ClientKind::AndroidVr
    );
    // googlevideo refuses its URL mid-stream: re-resolve skips android_vr.
    p.stream_failed("4D7u5KF7SP8", 403);
    http.log.lock().clear();
    let e = p
        .resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap_err();
    assert!(matches!(e, ProviderError::Unsupported(_)), "{e:?}");
    let tried: Vec<String> = http
        .log
        .lock()
        .iter()
        .filter(|r| endpoint(r) == "player")
        .map(|r| {
            r.json()["context"]["client"]["clientName"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(tried, ["VISIONOS", "IOS"]);
    // A 410 (expired) only forgets the URL; the client stays usable.
    answers.lock()[0] = ("VISIONOS", fx("player_visionos"));
    p.resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    p.stream_failed("4D7u5KF7SP8", 410);
    assert_eq!(
        p.chooser().order(false, false)[0].kind,
        ClientKind::VisionOs
    );
}

struct FakeJs(Arc<Mutex<Vec<JsRequest>>>);

impl JsHost for FakeJs {
    fn solve(
        &self,
        req: JsRequest,
    ) -> futures::future::BoxFuture<'static, Result<JsAnswer, String>> {
        self.0.lock().push(req.clone());
        Box::pin(async move {
            Ok(match req {
                JsRequest::Prepare { .. } => JsAnswer {
                    signature_timestamp: Some(20_400),
                    po_token: Some("POT/+=".into()),
                    urls: vec![],
                },
                JsRequest::Decipher { formats, .. } => JsAnswer {
                    urls: formats
                        .iter()
                        .map(|f| {
                            format!(
                                "https://rr1.googlevideo.com/videoplayback?signed=1&from={}",
                                f.len()
                            )
                        })
                        .collect(),
                    ..Default::default()
                },
            })
        })
    }
}

#[tokio::test]
async fn premium_takes_141_through_the_js_host_and_falls_back_to_140() {
    let mut web = fx("player_visionos");
    let formats = web["streamingData"]["adaptiveFormats"]
        .as_array_mut()
        .unwrap();
    let mut f141 = formats.iter().find(|f| f["itag"] == 140).unwrap().clone();
    f141["itag"] = 141.into();
    f141.as_object_mut().unwrap().remove("url");
    f141["signatureCipher"] =
        "s=ABC&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D141".into();
    formats.push(f141);
    let answers = Arc::new(Mutex::new(vec![
        ("WEB_REMIX", web),
        ("VISIONOS", fx("player_visionos")),
    ]));
    let http = fake(answers.clone());
    let p = provider_with(http.clone(), true);
    let asked = Arc::new(Mutex::new(vec![]));
    p.set_js_host(Some(Arc::new(FakeJs(asked.clone()))));
    p.set_premium(true);
    let s = p
        .resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    assert_eq!(s.variant, "141");
    assert!(s.url.as_str().contains("signed=1"));
    assert!(s.url.as_str().ends_with("&pot=POT%2F%2B%3D"));
    let player = http
        .log
        .lock()
        .iter()
        .find(|r| endpoint(r) == "player")
        .cloned()
        .unwrap();
    let body = player.json();
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
        20_400
    );
    assert_eq!(body["serviceIntegrityDimensions"]["poToken"], "POT/+=");
    assert!(player.header("Cookie").is_some() && player.header("Authorization").is_some());
    assert_eq!(asked.lock().len(), 2);
    // The JS path fails: visionOS 140.
    p.stream_failed("4D7u5KF7SP8", 403);
    answers.lock().retain(|(c, _)| *c != "WEB_REMIX");
    let s = p
        .resolve_stream("4D7u5KF7SP8", &StreamRequest::default())
        .await
        .unwrap();
    assert_eq!(s.variant, "140");
    // A free account never asks for 141, and plain clients never send the session.
    let free = provider_with(fake(players(&[("VISIONOS", "player_visionos")])), true);
    let s = free
        .resolve_stream("x", &StreamRequest::default())
        .await
        .unwrap();
    assert_eq!(s.variant, "140");
    let log = http.log.lock();
    let plain = log
        .iter()
        .filter(|r| {
            endpoint(r) == "player" && r.json()["context"]["client"]["clientName"] == "VISIONOS"
        })
        .collect::<Vec<_>>();
    assert!(!plain.is_empty() && plain.iter().all(|r| r.header("Cookie").is_none()));
}

#[tokio::test]
async fn catalogue_search_album_artist_and_track() {
    let http = fake(players(&[]));
    let p = provider_with(http.clone(), false);
    let r = p.search("daft punk get lucky", 10).await.unwrap();
    assert_eq!(r.tracks[0].id, "4D7u5KF7SP8");
    assert_eq!(r.tracks[0].server_id, "ytm");
    assert_eq!(r.tracks[0].suffix.as_deref(), Some("m4a"));
    assert_eq!(r.tracks[0].duration_ms, 370_000);
    assert!(!r.albums.is_empty() && !r.artists.is_empty());
    // A second search is served from the TTL cache.
    p.search("daft punk get lucky", 10).await.unwrap();
    assert_eq!(
        http.endpoints().iter().filter(|e| *e == "search").count(),
        3
    );
    let (album, tracks) = p.album("MPREb_K8qWMWVqXGi").await.unwrap();
    assert_eq!(album.song_count, 6);
    assert_eq!(tracks[2].track_number, Some(3));
    assert_eq!(tracks[0].album_id.as_deref(), Some("MPREb_K8qWMWVqXGi"));
    let (artist, albums) = p.artist("UCRr1xG_2WIDs18a6cIiCxeA").await.unwrap();
    assert_eq!(artist.name, "Daft Punk");
    assert!(!albums.is_empty());
    let t = p.track("4D7u5KF7SP8").await.unwrap();
    assert!(t.title.starts_with("Get Lucky"));
}

#[tokio::test]
async fn lyrics_radio_and_home() {
    let p = provider_with(fake(players(&[])), false);
    let l = p.lyrics("4D7u5KF7SP8").await.unwrap();
    assert_eq!(l.len(), 1);
    assert!(!l[0].synced);
    assert_eq!(l[0].line.len(), 4);
    let radio = p.radio("4D7u5KF7SP8", 3).await.unwrap();
    assert_eq!(radio.len(), 3);
    assert!(radio.iter().all(|t| t.id != "4D7u5KF7SP8"));
    let home = p.home_shelves().await.unwrap();
    assert_eq!(home[0].id, "quick-picks");
    assert!(!home[0].tracks.is_empty());
    assert!(p.capabilities().home_shelves);
}

#[tokio::test]
async fn anonymous_accounts_write_nothing() {
    let http = fake(players(&[]));
    let p = provider_with(http.clone(), false);
    let caps = p.capabilities();
    assert!(
        !caps.star && !caps.playlists && !caps.library_sync && !caps.library_edit && !caps.rate
    );
    let e = p
        .write(ProviderWrite::Star(vec![crate::api::RatingTarget::Track {
            id: "x".into(),
            source: "ytm".into(),
        }]))
        .await
        .unwrap_err();
    assert!(matches!(e, ProviderError::Unsupported(_)));
    assert!(p.library_page(0, 10).await.is_err());
    assert!(http.endpoints().is_empty());
}

#[tokio::test]
async fn likes_library_adds_and_playlist_create() {
    let http = fake(players(&[]));
    let p = provider_with(http.clone(), true);
    p.write(ProviderWrite::Star(vec![crate::api::RatingTarget::Track {
        id: "4D7u5KF7SP8".into(),
        source: "ytm".into(),
    }]))
    .await
    .unwrap();
    p.write(ProviderWrite::LibraryAdd(LibraryTarget::Album {
        id: "MPREb_K8qWMWVqXGi".into(),
    }))
    .await
    .unwrap();
    p.write(ProviderWrite::LibraryAdd(LibraryTarget::Artist {
        id: "UCx".into(),
    }))
    .await
    .unwrap();
    let created = p
        .write(ProviderWrite::CreatePlaylist {
            name: "New".into(),
            track_ids: vec!["a".into()],
        })
        .await
        .unwrap();
    assert_eq!(
        created,
        crate::provider::WriteOutcome::Created("PLcreated".into())
    );
    let log = http.log.lock();
    let likes: Vec<Value> = log
        .iter()
        .filter(|r| endpoint(r) == "like/like")
        .map(|r| r.json())
        .collect();
    assert_eq!(likes[0]["target"]["videoId"], "4D7u5KF7SP8");
    assert!(likes[1]["target"]["playlistId"]
        .as_str()
        .unwrap()
        .starts_with("OLAK"));
    let sub = log
        .iter()
        .find(|r| endpoint(r) == "subscription/subscribe")
        .unwrap()
        .json();
    assert_eq!(sub["channelIds"][0], "UCx");
    let create = log
        .iter()
        .find(|r| endpoint(r) == "playlist/create")
        .unwrap()
        .json();
    assert_eq!(create["videoIds"][0], "a");
    assert!(log.iter().filter(|r| endpoint(r) == "like/like").all(|r| r
        .header("Authorization")
        .is_some_and(|a| a.starts_with("SAPISIDHASH "))));
}

fn edits(http: &FakeYtmHttp) -> Vec<Value> {
    http.log
        .lock()
        .iter()
        .filter(|r| endpoint(r) == "browse/edit_playlist")
        .map(|r| r.json()["actions"].clone())
        .collect()
}

#[tokio::test]
async fn playlist_edits_land_in_place_around_hidden_videos() {
    // Entries: 0 song, 1 UGC (hidden), 2 song, 3 song, 4 OMV (hidden), 5 song.
    // Hocket lists [e0, e2, e3, e5].
    let e = mixed_entries();
    let set = |i: usize| e[i].set_video_id.clone().unwrap();
    let id = |i: usize| e[i].video_id.clone();
    let http = fake(players(&[]));
    let p = provider_with(http.clone(), true);
    let c = p.playlist("PLmixed").await.unwrap();
    assert_eq!(c.visible_ids(), vec![id(0), id(2), id(3), id(5)]);
    assert_eq!(c.playlist.song_count, 4);

    // Move the 4th listed song (e5) to 2nd place: before e2, past the hidden UGC.
    p.write(ProviderWrite::PlaylistMove {
        id: "PLmixed".into(),
        track_id: id(5),
        from_index: 3,
        to_index: 1,
    })
    .await
    .unwrap();
    // Move the 1st (e0) to the end: after e5, before the OMV that follows it? No:
    // the OMV sits before e5, so the end is after e5 (no successor).
    p.write(ProviderWrite::PlaylistMove {
        id: "PLmixed".into(),
        track_id: id(0),
        from_index: 0,
        to_index: 3,
    })
    .await
    .unwrap();
    // Remove the 3rd listed (e3): its own setVideoId, not the hidden e4's.
    p.write(ProviderWrite::PlaylistRemove {
        id: "PLmixed".into(),
        track_ids: vec![id(3)],
        indices: vec![2],
    })
    .await
    .unwrap();
    // Insert at listed position 2 (where e3 is): added, then moved before e3.
    p.write(ProviderWrite::PlaylistAdd {
        id: "PLmixed".into(),
        track_ids: vec!["newvid00001".into()],
        at_index: Some(2),
    })
    .await
    .unwrap();
    let got = edits(&http);
    assert_eq!(
        got,
        vec![
            json!([{"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": set(5), "movedSetVideoIdSuccessor": set(2)}]),
            json!([{"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": set(0)}]),
            json!([{"action": "ACTION_REMOVE_VIDEO", "removedVideoId": id(3), "setVideoId": set(3)}]),
            json!([{"action": "ACTION_ADD_VIDEO", "addedVideoId": "newvid00001", "dedupeOption": "DEDUPE_OPTION_SKIP"}]),
            json!([{"action": "ACTION_MOVE_VIDEO_BEFORE", "setVideoId": "NEWSET0001", "movedSetVideoIdSuccessor": set(3)}]),
        ]
    );
}

#[tokio::test]
async fn library_sync_walks_liked_songs_library_songs_albums_and_artists() {
    let http = fake(players(&[]));
    let p = provider_with(http.clone(), true);
    let page = p.library_page(0, 3).await.unwrap();
    assert_eq!(page.tracks.len(), 3);
    assert!(!page.albums.is_empty() && !page.artists.is_empty());
    // Liked Music first (songs only: the two hidden videos aren't songs), loved.
    let e = mixed_entries();
    assert_eq!(page.tracks[0].id, e[0].video_id);
    assert!(page.tracks[0].loved);
    let mut all = page.tracks.clone();
    let mut next = page.next_offset;
    while let Some(o) = next {
        let pg = p.library_page(o, 3).await.unwrap();
        assert!(pg.albums.is_empty());
        all.extend(pg.tracks);
        next = pg.next_offset;
    }
    assert!(all.iter().all(|t| t.server_id == "ytm"));
    let ids: std::collections::HashSet<_> = all.iter().map(|t| t.id.clone()).collect();
    assert_eq!(ids.len(), all.len(), "a song in two lists is listed once");
    assert!(!all
        .iter()
        .any(|t| t.id == e[1].video_id || t.id == e[4].video_id));
    // A liked song that is also a library song stays loved.
    assert!(all.iter().find(|t| t.id == "4D7u5KF7SP8").unwrap().loved);
    let playlists = p.playlists().await.unwrap();
    assert_eq!(playlists.len(), 1, "Liked Music isn't a playlist");
    assert_eq!(playlists[0].id, "PLmixed");
}

#[tokio::test]
async fn downloads_go_in_bounded_ranges_and_re_resolve_on_a_refusal() {
    // A 3 MiB stream; the first URL is refused after its first range.
    const LEN: usize = 3 * 1024 * 1024 + 123;
    let bytes: Arc<Vec<u8>> = Arc::new((0..LEN).map(|i| (i * 7 % 251) as u8).collect());
    let resolves = Arc::new(Mutex::new(0u32));
    let player = |n: u32| {
        json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"expiresInSeconds": "21540", "adaptiveFormats": [
            {"itag": 140, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "contentLength": LEN.to_string(),
             "url": format!("https://rr1.googlevideo.com/videoplayback?itag=140&n={n}"), "loudnessDb": 1.0}]}})
    };
    let served = Arc::new(Mutex::new(vec![]));
    let http = {
        let bytes = bytes.clone();
        let resolves = resolves.clone();
        let served = served.clone();
        FakeYtmHttp::new(move |r| {
            if r.url.contains("sw.js_data") {
                return Some(HttpResponse::json(200, SW_JS_DATA));
            }
            if endpoint(r) == "player" {
                let mut n = resolves.lock();
                *n += 1;
                return ok(&player(*n));
            }
            let range = r.header("Range")?.strip_prefix("bytes=")?.to_string();
            let (a, b) = range.split_once('-')?;
            let (a, b): (usize, usize) = (a.parse().ok()?, b.parse().ok()?);
            served.lock().push((r.url.clone(), a, b));
            // The first URL dies after one range.
            if r.url.ends_with("n=1") && a > 0 {
                return Some(HttpResponse::json(403, ""));
            }
            let b = b.min(LEN - 1);
            Some(HttpResponse {
                status: 206,
                body: bytes::Bytes::copy_from_slice(&bytes[a..=b]),
                content_range: Some(format!("bytes {a}-{b}/{LEN}")),
                content_type: Some("audio/mp4".into()),
            })
        })
    };
    let p = provider_with(http, false);
    let s = p
        .resolve_stream("vid", &StreamRequest::default())
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("x.m4a");
    let out = p.download_to_file(&s, &dest).await.unwrap();
    assert_eq!(out.bytes as usize, LEN);
    assert_eq!(std::fs::read(&dest).unwrap(), *bytes);
    assert_eq!(*resolves.lock(), 2, "re-resolved once, mid-download");
    let served = served.lock();
    assert!(served.iter().all(|(_, a, b)| b - a < MAX_RANGE as usize));
    assert_eq!(served[0].2, 1024 * 1024 - 1, "a small first range");
    assert!(
        served
            .iter()
            .any(|(u, a, _)| u.ends_with("n=2") && *a == 1024 * 1024),
        "resumed at the same offset"
    );
}
