//! AutoMix in the core (`docs/automix.md` → *Core behaviour*): the plans the
//! actor attaches to the sources it hands the scripted backend, the
//! transition reports, Next during a mix, lock-in and late edits, the
//! setting and the record store.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::core::test_support::{seeded_server, TestCore};

/// 200 s tracks; t0..t3 are album al0 tracks 1..4, t4..t7 album al1 5..8.
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

async fn set_mode(t: &TestCore, mode: TransitionMode) {
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode,
            crossfade_seconds: 6.0,
        },
    })
    .await;
}

/// Plays `ids` (as a selection, in order) from the start.
async fn play(t: &TestCore, ids: &[&str]) {
    t.run(Command::PlayTracks {
        server_id: t.server_id.clone(),
        track_ids: ids.iter().map(|s| s.to_string()).collect(),
        start_index: 0,
        label: "Sel".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(500.0).await;
}

fn loads(t: &TestCore) -> usize {
    t.backend
        .log()
        .iter()
        .filter(|c| matches!(c, ScriptedCall::Load { .. }))
        .count()
}

fn transition_started(t: &TestCore) -> bool {
    t.backend.is_mixing()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_setting_round_trips_and_defaults_to_off() {
    let t = synced_core("set").await;
    match t.query(Query::Transitions).await {
        QueryResult::Transitions(s) => assert_eq!(s.mode, TransitionMode::Off),
        other => panic!("{other:?}"),
    }
    set_mode(&t, TransitionMode::AutoMix).await;
    match t.query(Query::Transitions).await {
        QueryResult::Transitions(s) => {
            assert_eq!(s.mode, TransitionMode::AutoMix);
            assert_eq!(s.crossfade_seconds, 6.0);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn off_hands_over_no_plans() {
    let t = synced_core("off").await;
    play(&t, &["t0", "t4", "t1"]).await;
    assert_eq!(t.backend.current_source().unwrap().mix_out_ms, None);
    if let Some(next) = t.backend.next_source() {
        assert!(next.mix_in.is_none());
        assert!(next.mix_out_ms.is_none());
    }
}

/// A crossfade across albums: the follow-up carries the plan, the current
/// source its mix-out; the backend reports the start, and the session moves
/// to the incoming track at the handover, already some way in, without a load.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn crossfade_moves_the_session_at_the_handover() {
    let t = synced_core("xf").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;

    let cur = t.backend.current_source().unwrap();
    let next = t.backend.next_source().expect("the follow-up is preloaded");
    assert_eq!(next.track.id, "t4");
    let plan = next.mix_in.clone().expect("t0 → t4 is mixed");
    assert_eq!(plan.kind, TransitionKind::Fade);
    assert_eq!(plan.tail_len_ms, 6_000);
    assert_eq!(plan.tail_start_ms, 194_000);
    assert_eq!(cur.mix_out_ms, Some(plan.tail_start_ms));
    // t4's own mix-out, planned from what follows it (t1, another album).
    assert!(next.mix_out_ms.is_some());
    let loads_before = loads(&t);

    // Into the tail, before the handover.
    t.run_for(194_000.0).await;
    assert!(transition_started(&t), "the tail plays");
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));

    // Past the handover (3 s into the overlap).
    t.run_for(3_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    let pos = t.snapshot().await.transport.position.position_ms;
    assert!(
        (2_000..6_000).contains(&pos),
        "the incoming track is already some way in: {pos}"
    );
    assert_eq!(loads(&t), loads_before, "no reload at the handover");
    // The next follow-up (t1) is preloaded with its own plan.
    let after = t.backend.next_source().expect("t1 preloaded");
    assert_eq!(after.track.id, "t1");
    assert!(after.mix_in.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_album_in_track_order_is_not_mixed() {
    let t = synced_core("alb").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t1", "t4"]).await;
    let next = t.backend.next_source().expect("preloaded");
    assert_eq!(next.track.id, "t1");
    assert!(next.mix_in.is_none());
    assert_eq!(t.backend.current_source().unwrap().mix_out_ms, None);
    // t1 → t4 crosses albums: t1's mix-out is planned.
    assert!(next.mix_out_ms.is_some());
}

/// Next before the handover completes the transition: no reload, the
/// incoming track carries on where the mix had it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_during_a_transition_completes_it() {
    let t = synced_core("nxt").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    t.run_for(194_700.0 - 500.0).await;
    assert!(transition_started(&t));
    let loads_before = loads(&t);
    t.backend.clear_log();

    t.run(Command::Next).await;
    t.run_for(200.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    let log = t.backend.log();
    assert!(
        log.iter()
            .any(|c| matches!(c, ScriptedCall::CompleteTransition)),
        "{log:?}"
    );
    assert_eq!(loads(&t), 0, "no load (before: {loads_before})");
    assert!(!t.backend.is_mixing());
    let pos = t.snapshot().await.transport.position.position_ms;
    assert!(pos < 3_000, "B is where the mix had it: {pos}");

    // Next after the handover is an ordinary skip.
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));
}

/// A change to what plays next after lock-in keeps the committed tail start
/// and mixes into the new follow-up with a short fade.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_edit_keeps_the_tail_start() {
    let t = synced_core("late").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    let committed = t.backend.current_source().unwrap().mix_out_ms.unwrap();

    // Within 30 s of the tail start.
    t.run_for(f64::from(committed) - 20_000.0).await;
    t.run(Command::PlayNext {
        server_id: t.server_id.clone(),
        track_ids: vec!["t5".into()],
        keys: vec![],
    })
    .await;
    t.run_for(200.0).await;
    let next = t.backend.next_source().unwrap();
    assert_eq!(next.track.id, "t5");
    let plan = next.mix_in.expect("still mixed");
    assert_eq!(plan.kind, TransitionKind::Fade);
    assert_eq!(plan.tail_start_ms, committed, "the tail start is kept");
    assert_eq!(plan.tail_len_ms, 3_000, "max(2 s, crossfade / 2)");

    t.run_for(25_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t5"));
}

/// Before lock-in a new follow-up gets a fresh plan from the same tail start.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_early_edit_plans_afresh() {
    let t = synced_core("early").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    t.run(Command::PlayNext {
        server_id: t.server_id.clone(),
        track_ids: vec!["t5".into()],
        keys: vec![],
    })
    .await;
    t.run_for(200.0).await;
    let plan = t.backend.next_source().unwrap().mix_in.unwrap();
    assert_eq!(plan.tail_len_ms, 6_000);
    assert_eq!(plan.tail_start_ms, 194_000);
}

/// Turning transitions on mid-track plans the preloaded follow-up; turning
/// them off drops the plan.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn changing_the_setting_replans_the_follow_up() {
    let t = synced_core("chg").await;
    play(&t, &["t0", "t4", "t1"]).await;
    set_mode(&t, TransitionMode::Crossfade).await;
    t.run_for(200.0).await;
    let next = t.backend.next_source().expect("preloaded once on");
    assert!(next.mix_in.is_some());

    set_mode(&t, TransitionMode::Off).await;
    t.run_for(200.0).await;
    if let Some(next) = t.backend.next_source() {
        assert!(next.mix_in.is_none());
    }
    // And the track ends without a mix.
    t.run_for(200_000.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    assert!(t.snapshot().await.transport.position.position_ms < 5_000);
}

/// Repeat-one never mixes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeat_one_is_not_mixed() {
    let t = synced_core("rep").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4"]).await;
    t.run(Command::SetRepeat {
        mode: RepeatMode::One,
    })
    .await;
    t.run_for(200.0).await;
    if let Some(next) = t.backend.next_source() {
        assert!(next.mix_in.is_none(), "{next:?}");
    }
}

/// A seek during the transition cancels it: A plays on from the seek.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seek_cancels_the_transition() {
    let t = synced_core("seek").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    t.run_for(194_700.0 - 500.0).await;
    assert!(transition_started(&t));
    t.run(Command::SeekTo {
        position_ms: 100_000,
    })
    .await;
    t.run_for(5_000.0).await;
    assert!(!t.backend.is_mixing());
    assert_eq!(t.current_track_id().await.as_deref(), Some("t0"));
    // It mixes again at the tail start.
    t.run_for(91_000.0).await;
    assert!(transition_started(&t));
}

/// A stored record is kept, can be read back and is used by the planner.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stored_record_shapes_the_plan() {
    let t = synced_core("rec").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    let track = t.open_db().track("t0").unwrap().unwrap();
    let record = AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: AnalysisProducer::Device,
        source: fingerprint_of(&track),
        duration_ms: track.duration_ms,
        audible_end_ms: 180_000,
        ..Default::default()
    };
    t.run(Command::StoreAnalysisRecord {
        track_id: "t0".into(),
        record: Box::new(record),
    })
    .await;
    match t
        .query(Query::TrackAnalysis {
            track_id: "t0".into(),
        })
        .await
    {
        QueryResult::Analysis(Some(r)) => assert_eq!(r.audible_end_ms, 180_000),
        other => panic!("{other:?}"),
    }

    play(&t, &["t0", "t4"]).await;
    let plan = t.backend.next_source().unwrap().mix_in.unwrap();
    assert_eq!(plan.tail_start_ms, 174_000, "fades out by the audible end");
}

/// Positions the core stamped (`TransportChanged`) since the last clear.
fn stamped_positions(t: &TestCore) -> Vec<Ms> {
    t.events
        .all()
        .iter()
        .filter_map(|e| match e {
            Event::TransportChanged { transport } => Some(transport.position.position_ms),
            _ => None,
        })
        .collect()
}

/// A follow-up with a plan that the backend joined gaplessly (the seek went
/// past the tail start, so no mix started) starts at 0, not at the plan's
/// handover position.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skipped_mix_joins_from_the_start() {
    let t = synced_core("skip").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    assert!(t.backend.next_source().unwrap().mix_in.is_some());
    t.run(Command::SeekTo {
        position_ms: 197_000,
    })
    .await;
    t.events.clear();
    t.run_for(4_000.0).await;
    assert!(!transition_started(&t));
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    let stamps = stamped_positions(&t);
    assert!(
        stamps.iter().all(|p| !(1_500..190_000).contains(p)),
        "B was never placed past its start: {stamps:?}"
    );
}

/// Stores a record for `id` whose audio ends `audible_end_ms` in.
async fn store_audible_end(t: &TestCore, id: &str, audible_end_ms: Ms) {
    let track = t.open_db().track(id).unwrap().unwrap();
    let record = AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: AnalysisProducer::Device,
        source: fingerprint_of(&track),
        duration_ms: track.duration_ms,
        audible_end_ms,
        ..Default::default()
    };
    t.run(Command::StoreAnalysisRecord {
        track_id: id.into(),
        record: Box::new(record),
    })
    .await;
}

/// Right after a handover onto a track whose tail start is near, no plan
/// into the track after has been handed to the backend yet: it gets a full
/// plan, not a late-edit fade.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_track_after_a_handover_gets_a_full_plan() {
    let t = synced_core("short").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    store_audible_end(&t, "t4", 30_000).await;
    play(&t, &["t0", "t4", "t1"]).await;
    t.run_for(194_000.0 + 3_500.0).await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t4"));
    let next = t.backend.next_source().expect("t1 preloaded");
    assert_eq!(next.track.id, "t1");
    let plan = next.mix_in.expect("t4 → t1 is mixed");
    assert_eq!(plan.tail_len_ms, 6_000, "a full plan: {plan:?}");
    assert_eq!(plan.tail_start_ms, 24_000);
}

/// After lock-in a context rule (here the sleep timer's end-of-track) still
/// drops the committed plan: A plays out and nothing is mixed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_context_rule_after_lock_in_drops_the_plan() {
    let t = synced_core("ctx").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    let committed = t.backend.current_source().unwrap().mix_out_ms.unwrap();
    t.run_for(f64::from(committed) - 20_000.0).await;
    t.run(Command::SetSleepTimer {
        timer: Some(SleepTimer {
            ends_at: None,
            stop_at_end_of_track: true,
        }),
    })
    .await;
    t.run_for(200.0).await;
    if let Some(next) = t.backend.next_source() {
        assert!(next.mix_in.is_none(), "{next:?}");
    }
    t.run_for(25_000.0).await;
    assert!(!transition_started(&t));
}

/// After lock-in a new follow-up the pair rules would not mix (here the next
/// track of the same album) still gets the late-edit fade: the current
/// source may already be clipped at the committed tail start.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_edit_the_pair_rules_refuse_still_fades() {
    let t = synced_core("latealb").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t2"]).await;
    let committed = t.backend.current_source().unwrap().mix_out_ms.unwrap();
    t.run_for(f64::from(committed) - 20_000.0).await;
    t.run(Command::PlayNext {
        server_id: t.server_id.clone(),
        track_ids: vec!["t1".into()],
        keys: vec![],
    })
    .await;
    t.run_for(200.0).await;
    let next = t.backend.next_source().unwrap();
    assert_eq!(next.track.id, "t1");
    let plan = next.mix_in.expect("the edit wins: still faded");
    assert_eq!(plan.kind, TransitionKind::Fade);
    assert_eq!(plan.tail_start_ms, committed);
    assert_eq!(plan.tail_len_ms, 3_000);
}

/// A record that doesn't match the track's file is dropped, and never
/// replaces a usable one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_record_for_another_file_is_not_stored() {
    let t = synced_core("fp").await;
    store_audible_end(&t, "t0", 180_000).await;
    let track = t.open_db().track("t0").unwrap().unwrap();
    let mut other = fingerprint_of(&track);
    other.duration_ms += 30_000;
    let record = AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: AnalysisProducer::Worker,
        source: other,
        duration_ms: track.duration_ms + 30_000,
        audible_end_ms: 150_000,
        ..Default::default()
    };
    t.run(Command::StoreAnalysisRecord {
        track_id: "t0".into(),
        record: Box::new(record),
    })
    .await;
    match t
        .query(Query::TrackAnalysis {
            track_id: "t0".into(),
        })
        .await
    {
        QueryResult::Analysis(Some(r)) => assert_eq!(r.audible_end_ms, 180_000),
        other => panic!("{other:?}"),
    }
}

/// The follow-up failing for good during its mix ends the transition: a
/// later edit preloads a new follow-up.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_follow_up_failing_mid_mix_ends_the_transition() {
    let t = synced_core("fail").await;
    set_mode(&t, TransitionMode::Crossfade).await;
    play(&t, &["t0", "t4", "t1"]).await;
    t.run_for(194_000.0).await;
    assert!(transition_started(&t));
    let next = t.backend.next_source().unwrap();
    t.backend.report(BackendReport::Error {
        key: next.key.clone(),
        message: "gone".into(),
        fatal: true,
    });
    t.run_for(100.0).await;
    t.backend.clear_log();
    t.run(Command::PlayNext {
        server_id: t.server_id.clone(),
        track_ids: vec!["t5".into()],
        keys: vec![],
    })
    .await;
    t.run_for(200.0).await;
    let log = t.backend.log();
    assert!(
        log.iter()
            .any(|c| matches!(c, ScriptedCall::SetNext { next: Some(_) })),
        "{log:?}"
    );
}
