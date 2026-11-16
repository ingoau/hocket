//! YouTube Music on a Cast device (docs/providers.md, Phase 5): the receiver
//! gets the direct googlevideo URL, fetched when the track is about to be
//! handed over; the follow-up's at preload time; anew after a long pause;
//! and a failure re-resolves once, then skips. The InnerTube transport is a
//! fake answering from recorded fixtures; the backend stands in for the
//! Cast receiver.

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::test_support::TestCore;
use hocket_core::subsonic::fake::FakeServer;
use hocket_core::ytm::fake::{fake, players};
use hocket_core::ytm::http::FakeYtmHttp;

const A: &str = "4D7u5KF7SP8";
const B: &str = "Rgrt_8mXrK8";

fn navidrome() -> FakeServer {
    let s = FakeServer::new("srv", "alice");
    s.add_song(FakeServer::song(
        "n1",
        "Navidrome Song",
        "al1",
        "ar1",
        200.0,
    ));
    s
}

fn living_room() -> Renderer {
    Renderer {
        id: "cast-1".into(),
        name: "Living Room TV".into(),
    }
}

/// Anonymous YouTube Music with search results in the catalogue, casting.
async fn casting() -> (TestCore, FakeYtmHttp) {
    let http = fake(players(&[("VISIONOS", "player_visionos")]));
    let t = TestCore::start_with_ytm("cast", navidrome(), Arc::new(http.clone())).await;
    t.run(Command::AddAccount {
        setup: AccountSetup::YoutubeMusic {
            cookies: None,
            js_host: false,
        },
    })
    .await;
    let _ = t
        .query(Query::Search {
            server_id: String::new(),
            query: "daft punk get lucky".into(),
            limit: 10,
            include_server: true,
            request_id: "r1".into(),
        })
        .await;
    t.wait_until(10_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SearchResults { results } if results.from_server))
    })
    .await;
    // As on Android: Media3 reads through the core's stream reader.
    t.run(Command::SetBackendCapabilities { core_stream: true })
        .await;
    t.run(Command::SetRenderer {
        renderer: Some(living_room()),
    })
    .await;
    (t, http)
}

fn player_calls(http: &FakeYtmHttp) -> usize {
    http.endpoints().iter().filter(|e| *e == "player").count()
}

async fn play(t: &TestCore, tracks: &[(&str, &str)]) {
    t.run(Command::PlayTracks {
        tracks: tracks
            .iter()
            .map(|(s, id)| TrackRef::new(*s, *id))
            .collect(),
        start_index: 0,
        label: "test".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
}

fn is_googlevideo(s: &MediaSource) -> bool {
    s.url.contains(".googlevideo.com/videoplayback")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mixed_queue_casts_with_direct_urls_and_the_follow_up_is_fetched_at_preload_time() {
    let (t, http) = casting().await;
    play(&t, &[("ytm", A), ("srv", "n1"), ("ytm", B)]).await;

    // The YouTube Music track: the googlevideo URL itself, as AAC in MP4.
    let current = t.backend.current_source().expect("loaded");
    assert!(is_googlevideo(&current), "{}", current.url);
    assert_eq!(current.mime_type.as_deref(), Some("audio/mp4"));
    assert!(!current.transcoded && !current.local);
    assert!(current.headers.is_empty(), "a receiver can't send headers");
    assert_eq!(current.track.id, A);
    // The Navidrome follow-up: the server's own stream URL.
    let next = t.backend.next_source().expect("follow-up");
    assert_eq!(next.track.id, "n1");
    assert!(next.url.contains("/rest/stream"), "{}", next.url);
    assert_eq!(player_calls(&http), 1);

    // On to the Navidrome track: B is the follow-up now, and its URL is
    // fetched straight away, minutes before the receiver needs it.
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    let current = t.backend.current_source().expect("loaded");
    assert_eq!(current.track.id, "n1");
    let next = t.backend.next_source().expect("follow-up");
    assert_eq!(next.track.id, B);
    assert!(is_googlevideo(&next), "{}", next.url);
    assert_eq!(player_calls(&http), 2);

    // Back on this device, YouTube Music plays through the stream reader again.
    t.run(Command::SetRenderer { renderer: None }).await;
    t.run_for(200.0).await;
    let next = t.backend.next_source().expect("follow-up");
    assert!(next.url.starts_with("hocket-stream://"), "{}", next.url);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resuming_after_a_long_pause_fetches_the_urls_anew() {
    let (t, http) = casting().await;
    play(&t, &[("ytm", A), ("ytm", B)]).await;
    assert_eq!(player_calls(&http), 2, "current and follow-up");
    t.run_for(5_000.0).await;

    // A short pause resumes what the receiver holds.
    t.run(Command::Pause).await;
    t.run_for(60_000.0).await;
    t.backend.clear_log();
    t.run(Command::Play).await;
    t.run_for(500.0).await;
    assert_eq!(player_calls(&http), 2);
    assert!(t
        .backend
        .log()
        .iter()
        .all(|c| !matches!(c, hocket_core::audio::scripted::ScriptedCall::Load { .. })));

    // A long one fetches both anew and reloads at the position.
    t.run(Command::Pause).await;
    let paused_at = t.transport().await.position.position_ms;
    t.clock.advance(11.0 * 60_000.0);
    t.run_for(250.0).await;
    t.backend.clear_log();
    t.run(Command::Play).await;
    t.run_for(500.0).await;
    assert_eq!(player_calls(&http), 4, "current and follow-up again");
    let load = t
        .backend
        .log()
        .into_iter()
        .find_map(|c| match c {
            hocket_core::audio::scripted::ScriptedCall::Load {
                position_ms, play, ..
            } => Some((position_ms, play)),
            _ => None,
        })
        .expect("reloaded");
    assert!(load.1, "playing");
    assert!(
        (i64::from(load.0) - i64::from(paused_at)).abs() < 1_000,
        "{load:?} vs {paused_at}"
    );
    assert!(t.backend.next_source().is_some_and(|n| n.track.id == B));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receiver_failure_re_resolves_once_then_skips_with_a_notice() {
    let (t, http) = casting().await;
    t.backend.fail_track(A);
    play(&t, &[("ytm", A), ("srv", "n1")]).await;
    t.run_for(2_000.0).await;

    assert_eq!(
        player_calls(&http),
        2,
        "resolved, then once more for the retry"
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("n1"));
    assert!(t.events.all().iter().any(|e| matches!(
        e,
        Event::PlayerNotice {
            code: Some(PlayerNoticeCode::CouldNotPlaySkipped),
            ..
        }
    )));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_receiver_crash_on_a_youtube_music_track_refetches_rather_than_transcodes() {
    let (t, http) = casting().await;
    play(&t, &[("ytm", A)]).await;
    assert_eq!(player_calls(&http), 1);

    // The session died on it; the platform reconnects.
    t.run(Command::RendererFailed { track_id: A.into() }).await;
    t.run(Command::SetRenderer {
        renderer: Some(living_room()),
    })
    .await;
    t.run_for(500.0).await;
    assert_eq!(player_calls(&http), 2);
    let s = t.backend.current_source().expect("reloaded");
    assert!(is_googlevideo(&s) && !s.transcoded, "{}", s.url);
}
