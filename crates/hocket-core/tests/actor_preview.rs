//! Song preview: a second player that gives this device's playback back
//! and leaves a Connect peer alone.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn synced_core(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(8, 200.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

fn preview_of(events: &[Event]) -> Option<PreviewState> {
    events.iter().rev().find_map(|e| match e {
        Event::PreviewChanged { preview } => Some(preview.clone()),
        _ => None,
    })?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_pauses_here_and_resumes_at_the_same_position() {
    let t = synced_core("preview").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0", "t1"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(3_000.0).await;
    let before = t.transport().await;
    let position = before.position.position_ms;
    assert!(before.position.is_playing);
    assert!(position >= 2_000, "playback moved, at {position}");

    t.events.clear();
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t3"),
        clip: false,
    })
    .await;
    let transport = t.transport().await;
    assert!(!transport.position.is_playing, "main playback pauses");
    assert!(
        transport.position.position_ms.abs_diff(position) < 500,
        "position held: was {position}, now {}",
        transport.position.position_ms
    );
    let preview = preview_of(&t.events.all()).expect("preview started");
    assert_eq!(preview.track_id, "t3");
    assert!(preview.playing);
    // 200 s song, opened 30% in.
    assert_eq!(preview.position_ms, 60_000);
    assert!(!t.backend.is_playing(), "the main player is paused");
    assert!(
        t.backend.log().iter().any(|c| matches!(
            c,
            ScriptedCall::PreviewLoad {
                position_ms: 60_000,
                ..
            }
        )),
        "preview loads at 30%"
    );
    // The main item is still the one that was playing, not the preview.
    let (key, _) = t.backend.current().expect("main item stays loaded");
    assert_ne!(key, "preview:t3");

    let history: i64 = t
        .open_db()
        .with_conn(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM play_history WHERE track_id = 't3'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(history, 0, "a preview is not a play");

    t.run(Command::PreviewToggle).await;
    let paused = preview_of(&t.events.all()).expect("still previewing");
    assert!(!paused.playing);

    t.run(Command::PreviewSeek {
        position_ms: 80_000,
    })
    .await;
    let sought = preview_of(&t.events.all()).expect("seek");
    assert_eq!(sought.position_ms, 80_000);

    t.run(Command::PreviewStop).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    let after = t.transport().await;
    assert!(after.position.is_playing, "main playback resumes");
    assert!(
        after.position.position_ms.abs_diff(position) < 800,
        "resumed at {position}, got {}",
        after.position.position_ms
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(t.queue().await.history_count == 0, "the queue did not move");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_on_a_peer_leaves_the_player_alone() {
    let server = seeded_server(6, 200.0);
    let net = hocket_core::core::io::memory::MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server, Some(net), 2, clock).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    a.run(Command::PlayTracks {
        tracks: TrackRef::list(&a.server_id, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());
    assert!(!b.backend.is_playing());
    let owner = a.snapshot().await.transport.lease.owner.clone();
    assert_eq!(owner.as_deref(), Some(a.device_id.as_str()));

    b.events.clear();
    b.run(Command::PreviewStart {
        track: TrackRef::new(b.server_id.clone(), "t2"),
        clip: false,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;

    assert!(a.backend.is_playing(), "the peer keeps playing");
    assert_eq!(
        a.snapshot().await.transport.lease.owner.as_deref(),
        Some(a.device_id.as_str())
    );
    let preview = preview_of(&b.events.all()).expect("b previews locally");
    assert_eq!(preview.track_id, "t2");
    assert!(preview.playing);
    assert!(
        b.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::PreviewLoad { .. })),
        "the preview is played on this device"
    );
    // Nothing was asked of the owner's player beyond what it was already doing.
    assert!(
        !a.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::Pause | ScriptedCall::Stop)),
        "the peer was not paused"
    );
}

/// The transport shows paused during a preview, so play (toggle) must start
/// it, and pause must not play a moment of audio on the way to staying paused.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_commands_during_a_preview_follow_what_the_user_hears() {
    let t = synced_core("preview-transport").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(1_000.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t3"),
        clip: false,
    })
    .await;
    assert!(!t.transport().await.position.is_playing);

    t.backend.clear_log();
    t.run(Command::Pause).await;
    assert!(preview_of(&t.events.all()).is_none());
    assert!(!t.transport().await.position.is_playing);
    assert!(
        !t.backend
            .log()
            .iter()
            .any(|c| matches!(c, ScriptedCall::Play)),
        "pause must not restart the main player"
    );

    t.run(Command::Play).await;
    t.run_for(500.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t4"),
        clip: false,
    })
    .await;
    t.run(Command::TogglePlay).await;
    assert!(preview_of(&t.events.all()).is_none());
    assert!(
        t.transport().await.position.is_playing,
        "play, while the preview had main looking paused, starts it"
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_end_resumes_and_an_interruption_does_not() {
    let t = synced_core("preview-end").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t1"),
        clip: false,
    })
    .await;
    // 200 s song. Land just before the end and let the preview finish.
    t.run(Command::PreviewSeek {
        position_ms: 199_500,
    })
    .await;
    t.run_for(1_000.0).await;
    assert!(
        preview_of(&t.events.all()).is_none(),
        "the song ending stops the preview"
    );
    assert!(
        t.transport().await.position.is_playing,
        "main playback resumes"
    );

    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t2"),
        clip: false,
    })
    .await;
    t.backend.finish_preview(true);
    t.core.settle().await;
    assert!(preview_of(&t.events.all()).is_none());
    assert!(
        !t.transport().await.position.is_playing,
        "another app taking the output does not resume main playback"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preview_that_cannot_load_gives_playback_back() {
    let t = synced_core("preview-fail").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.backend.fail_key("preview:t5");
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t5"),
        clip: false,
    })
    .await;
    assert!(preview_of(&t.events.all()).is_none());
    assert!(t.transport().await.position.is_playing);
    assert!(t.backend.is_playing());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_preview_keeps_the_hold_and_a_paused_track_stays_paused() {
    let t = synced_core("preview-hold").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t1"),
        clip: false,
    })
    .await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t2"),
        clip: false,
    })
    .await;
    let preview = preview_of(&t.events.all()).expect("switched preview");
    assert_eq!(preview.track_id, "t2");
    assert!(!t.transport().await.position.is_playing);
    t.run(Command::PreviewStop).await;
    assert!(
        t.transport().await.position.is_playing,
        "stopping the second preview still resumes the original track"
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));

    t.run(Command::Pause).await;
    assert!(!t.transport().await.position.is_playing);
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t3"),
        clip: false,
    })
    .await;
    t.run(Command::PreviewStop).await;
    assert!(
        !t.transport().await.position.is_playing,
        "a track that was paused stays paused"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_after_pausing_a_buffering_track_stays_paused() {
    let t = synced_core("preview-paused-buffering").await;
    let sid = t.server_id.clone();
    // Ready has not arrived, so the track is still marked buffering after pause.
    t.backend.set_load_latency_ms(5_000.0);
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0", "t1"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run(Command::Pause).await;
    let transport = t.transport().await;
    assert!(!transport.position.is_playing);
    assert!(
        transport.buffering,
        "the pause landed while the track was still buffering"
    );
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t2"),
        clip: false,
    })
    .await;
    assert!(preview_of(&t.events.all()).is_some());
    t.run(Command::PreviewStop).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    assert!(
        !t.transport().await.position.is_playing,
        "pausing while the track is still buffering stays paused after the preview"
    );
    t.backend.set_load_latency_ms(0.0);
    t.run_for(1_000.0).await;
    assert!(!t.transport().await.position.is_playing);
    assert!(!t.backend.is_playing());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_while_the_track_is_still_loading_resumes_it() {
    let t = synced_core("preview-loading").await;
    let sid = t.server_id.clone();
    t.backend.set_load_latency_ms(5_000.0);
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t1"),
        clip: false,
    })
    .await;
    assert!(preview_of(&t.events.all()).is_some());
    assert!(
        !t.backend.is_playing(),
        "the loading track does not play over the preview"
    );
    t.run(Command::PreviewStop).await;
    t.backend.set_load_latency_ms(0.0);
    t.run_for(1_000.0).await;
    assert!(
        t.transport().await.position.is_playing,
        "the track that was loading resumes after the preview"
    );
}

/// A Connect peer's play or pause reaches this device without passing
/// through its own command handler. Play takes the output back from the
/// preview; pause is kept when the preview closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_peers_transport_commands_during_a_preview_are_kept() {
    let server = seeded_server(6, 200.0);
    let net = hocket_core::core::io::memory::MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server, Some(net), 2, clock).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    a.run(Command::PlayTracks {
        tracks: TrackRef::list(&a.server_id, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(a.backend.is_playing());

    // Pause from the peer, then close the preview: main stays paused.
    a.run(Command::PreviewStart {
        track: TrackRef::new(a.server_id.clone(), "t2"),
        clip: false,
    })
    .await;
    assert!(!a.backend.is_playing());
    b.run(Command::Pause).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(
        preview_of(&a.events.all()).is_some(),
        "a pause leaves the preview playing"
    );
    a.run(Command::PreviewStop).await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert!(
        !a.backend.is_playing(),
        "closing the preview does not undo the peer's pause"
    );
    assert!(!a.transport().await.position.is_playing);

    // Play from the peer stops the preview and plays main, without the
    // preview pausing it again.
    a.run(Command::PreviewStart {
        track: TrackRef::new(a.server_id.clone(), "t3"),
        clip: false,
    })
    .await;
    a.events.clear();
    b.run(Command::Play).await;
    TestCore::run_all_for(&[&a, &b], 2_000.0).await;
    assert!(preview_of(&a.events.all()).is_none(), "the preview closed");
    assert!(a.backend.is_playing(), "main plays on the owner");
    assert!(a.transport().await.position.is_playing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_sleep_timer_stopping_during_a_preview_stays_stopped() {
    let t = synced_core("preview-sleep").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::SetSleepTimer {
        timer: Some(SleepTimer {
            ends_at: Some(t.clock.now_ms() + 2_000.0),
            stop_at_end_of_track: false,
        }),
    })
    .await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t1"),
        clip: false,
    })
    .await;
    t.run_for(3_000.0).await;
    assert!(
        preview_of(&t.events.all()).is_none(),
        "the timer stops the preview too"
    );
    t.run(Command::PreviewStop).await;
    t.run_for(500.0).await;
    assert!(!t.backend.is_playing(), "main playback stays stopped");
    assert!(!t.transport().await.position.is_playing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preview_that_fails_while_playing_says_so_and_gives_playback_back() {
    let t = synced_core("preview-fail-late").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(t.server_id.clone(), "t1"),
        clip: false,
    })
    .await;
    t.events.clear();
    t.backend.fail_preview();
    t.core.settle().await;
    assert!(preview_of(&t.events.all()).is_none());
    assert!(
        t.events.all().iter().any(|e| matches!(
            e,
            Event::Toast { toast } if toast.message == "Couldn't preview this song"
        )),
        "the failure is shown"
    );
    assert!(t.transport().await.position.is_playing);
    assert!(t.backend.is_playing());
}

/// Started with a song playing and a preview of `t3` on for two seconds.
async fn previewing_t3() -> (TestCore, u32) {
    let t = synced_core("handoff").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0", "t1"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(2_000.0).await;
    t.run(Command::PreviewStart {
        track: TrackRef::new(sid, "t3"),
        clip: false,
    })
    .await;
    t.run_for(2_000.0).await;
    let at = preview_of(&t.events.all()).expect("previewing").position_ms;
    assert!(at >= 61_000, "the preview moved on, at {at}");
    (t, at)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playing_the_previewed_song_carries_it_on_from_the_preview() {
    let (t, at) = previewing_t3().await;
    t.backend.set_load_latency_ms(400.0);
    let sid = t.server_id.clone();
    t.events.clear();
    let log_from = t.backend.log().len();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t2", "t3", "t4"]),
        start_index: 1,
        label: "Album".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    // Loaded paused where the preview is; the preview plays on meanwhile.
    let log = t.backend.log()[log_from..].to_vec();
    let load = log
        .iter()
        .find_map(|c| match c {
            ScriptedCall::Load {
                key,
                position_ms,
                play,
                ..
            } => Some((key.clone(), *position_ms, *play)),
            _ => None,
        })
        .expect("the song loads");
    assert!(!load.2, "loaded paused for the handover");
    assert!(
        load.1.abs_diff(at) < 600,
        "loaded at the preview's {at}, got {}",
        load.1
    );
    assert!(
        !log.iter().any(|c| matches!(c, ScriptedCall::PreviewStop)),
        "the preview plays on while the song loads"
    );
    assert!(
        t.transport().await.position.is_playing,
        "playing, as far as anyone is told"
    );

    t.run_for(1_000.0).await;
    let log = t.backend.log()[log_from..].to_vec();
    let play = log
        .iter()
        .position(|c| matches!(c, ScriptedCall::Play))
        .expect("started once ready");
    let seek = log
        .iter()
        .position(|c| matches!(c, ScriptedCall::Seek { .. }))
        .expect("moved to where the preview got to");
    let stop = log
        .iter()
        .position(|c| matches!(c, ScriptedCall::PreviewStop))
        .expect("the preview stops");
    assert!(
        seek < play && play < stop,
        "seek, play, then the preview stops: {log:?}"
    );
    if let ScriptedCall::Seek { position_ms } = log[seek] {
        // The load's 400 ms later, plus the lead.
        assert!(
            position_ms >= load.1 + 300 && position_ms <= load.1 + 700,
            "caught up with the preview: loaded at {}, sought to {position_ms}",
            load.1
        );
    }
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    assert_eq!(t.current_track_id().await.as_deref(), Some("t3"));
    let transport = t.transport().await;
    assert!(transport.position.is_playing);
    assert!(t.backend.is_playing());
    assert!(
        transport.position.position_ms > at,
        "on from the preview's {at}, at {}",
        transport.position.position_ms
    );
    // Never told anyone it paused: a backend reports Paused with the paused
    // load's Ready, after the handover has started it.
    assert!(
        !t.events.all().iter().any(
            |e| matches!(e, Event::TransportChanged { transport } if !transport.position.is_playing)
        ),
        "the transport stayed playing"
    );
    // Nothing was given back to the song the preview paused.
    assert!(
        !t.events.all().iter().any(|e| matches!(e, Event::TransportChanged { transport } if transport.position.position_ms < 10_000 && transport.position.is_playing)),
        "t0 did not come back"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playing_a_paused_preview_starts_where_it_was() {
    let (t, _) = previewing_t3().await;
    t.run(Command::PreviewToggle).await;
    let at = preview_of(&t.events.all()).expect("paused").position_ms;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t3", "t4"]),
        start_index: 0,
        label: "Album".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    assert!(t.backend.log().iter().any(|c| matches!(c, ScriptedCall::Load { position_ms, play: true, .. } if position_ms.abs_diff(at) < 50)));
    let transport = t.transport().await;
    assert!(transport.position.is_playing);
    assert_eq!(t.current_track_id().await.as_deref(), Some("t3"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playing_another_song_stops_the_preview_and_starts_at_the_top() {
    let (t, _) = previewing_t3().await;
    let sid = t.server_id.clone();
    let log_from = t.backend.log().len();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t2", "t3"]),
        start_index: 0,
        label: "Album".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    let log = t.backend.log()[log_from..].to_vec();
    assert!(
        log.iter().any(|c| matches!(
            c,
            ScriptedCall::Load {
                position_ms: 0,
                play: true,
                ..
            }
        )),
        "{log:?}"
    );
    assert_eq!(t.current_track_id().await.as_deref(), Some("t2"));
    assert!(t.transport().await.position.is_playing);
}

/// The handover started loading the song (paused, 400 ms to go).
async fn handing_over() -> (TestCore, u32) {
    let (t, at) = previewing_t3().await;
    t.backend.set_load_latency_ms(400.0);
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t3", "t4"]),
        start_index: 0,
        label: "Album".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    assert!(
        preview_of(&t.events.all()).is_some(),
        "still previewing while it loads"
    );
    (t, at)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preview_interrupted_while_handing_over_leaves_the_song_paused() {
    let (t, _) = handing_over().await;
    let log_from = t.backend.log().len();
    t.backend.finish_preview(true);
    t.run_for(1_000.0).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    assert!(
        !t.transport().await.position.is_playing,
        "not resumed into what interrupted it"
    );
    assert!(!t.backend.is_playing());
    assert!(
        !t.backend.log()[log_from..]
            .iter()
            .any(|c| matches!(c, ScriptedCall::Play)),
        "the song was never started"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pausing_while_handing_over_never_starts_the_song() {
    let (t, _) = handing_over().await;
    let log_from = t.backend.log().len();
    t.run(Command::Pause).await;
    t.run_for(1_000.0).await;
    assert!(preview_of(&t.events.all()).is_none(), "preview closed");
    assert!(!t.transport().await.position.is_playing);
    assert!(
        !t.backend.log()[log_from..]
            .iter()
            .any(|c| matches!(c, ScriptedCall::Play)),
        "no play before the pause"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_preview_closed_while_handing_over_starts_the_song_where_it_got_to() {
    let (t, at) = handing_over().await;
    t.run_for(100.0).await;
    t.run(Command::PreviewStop).await;
    t.run_for(1_000.0).await;
    assert!(preview_of(&t.events.all()).is_none());
    let transport = t.transport().await;
    assert!(transport.position.is_playing, "the song plays on");
    assert!(t.backend.is_playing());
    assert_eq!(t.current_track_id().await.as_deref(), Some("t3"));
    assert!(
        transport.position.position_ms > at,
        "on from the preview's {at}, at {}",
        transport.position.position_ms
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_clipped_preview_stops_after_thirty_seconds_and_gives_playback_back() {
    let t = synced_core("preview-clip").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&sid, ["t0"]),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
    t.events.clear();
    t.run(Command::PreviewStart {
        track: TrackRef::new(sid.clone(), "t1"),
        clip: true,
    })
    .await;
    let preview = preview_of(&t.events.all()).expect("preview started");
    // 200 s song: 30% in, for 30 s.
    assert_eq!(
        preview.clip,
        Some(PreviewClip {
            start_ms: 60_000,
            end_ms: 90_000
        })
    );
    t.run_for(25_000.0).await;
    assert!(preview_of(&t.events.all()).is_some(), "still in the clip");
    t.run_for(6_000.0).await;
    assert!(
        preview_of(&t.events.all()).is_none(),
        "the clip ending stops the preview"
    );
    assert!(
        t.transport().await.position.is_playing,
        "main playback resumes"
    );

    // Unclipped, the same preview plays on.
    t.run(Command::PreviewStart {
        track: TrackRef::new(sid, "t2"),
        clip: false,
    })
    .await;
    let preview = preview_of(&t.events.all()).expect("preview started");
    assert_eq!(preview.clip, None);
    t.run_for(31_000.0).await;
    assert!(preview_of(&t.events.all()).is_some(), "plays past 30 s");
}
