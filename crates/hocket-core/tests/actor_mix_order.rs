//! AutoMix's order (`docs/automix.md` → *Order*): with AutoMix on and a
//! shuffled queue, each of the first entries is followed by the candidate
//! that blends best with it, decided ahead so the top of the queue is
//! settled before the listener gets there; an in-order queue is never
//! touched, and skipping through tracks, back and forth, never moves
//! anything.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::automix::order::{ORDER_AHEAD, SETTLE_MS};
use hocket_core::automix::store::fingerprint_of;
use hocket_core::automix::ANALYSIS_VERSION;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

/// Tracks in the small library: the whole queue is within the order's reach.
const FEW: usize = 8;
/// Tracks in the large one: the queue reaches past it.
const MANY: usize = 24;

async fn synced_core(name: &str) -> TestCore {
    synced_core_with(name, FEW).await
}

async fn synced_core_with(name: &str, n: usize) -> TestCore {
    let t = TestCore::start(name, seeded_server(n, 200.0)).await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t
}

/// Even tracks at 120 BPM, odd ones at 150: a pair of the same tempo blends,
/// a pair across tempos only fades.
fn bpm_of(id: &str) -> f64 {
    let n: u32 = id.trim_start_matches('t').parse().unwrap();
    if n % 2 == 0 {
        120.0
    } else {
        150.0
    }
}

async fn store_records(t: &TestCore) {
    store_records_of(t, FEW).await;
}

async fn store_records_of(t: &TestCore, count: usize) {
    for n in 0..count {
        store_record(t, &format!("t{n}")).await;
    }
}

async fn store_record(t: &TestCore, id: &str) {
    let track = t.open_db().track(id).unwrap().unwrap();
    let beat = 60_000.0 / bpm_of(id);
    let mut beats = vec![];
    let mut downbeats = vec![];
    let mut at = 250.0;
    let mut i = 0;
    while at < f64::from(track.duration_ms) - 1_000.0 {
        beats.push(at.round() as Ms);
        if i % 4 == 0 {
            downbeats.push(at.round() as Ms);
        }
        at += beat;
        i += 1;
    }
    let record = AnalysisRecord {
        version: ANALYSIS_VERSION,
        analyser: "test".into(),
        produced_by: AnalysisProducer::Desktop,
        source: fingerprint_of(&track),
        duration_ms: track.duration_ms,
        audible_end_ms: track.duration_ms - 1_000,
        bpm: Some(bpm_of(id) as f32),
        beats_ms: beats,
        downbeats_ms: downbeats,
        beat_confidence: 0.9,
        ..Default::default()
    };
    t.run(Command::StoreAnalysisRecord {
        track_id: id.into(),
        record: Box::new(record),
    })
    .await;
}

async fn start(t: &TestCore, shuffle: bool) {
    start_with(t, FEW, shuffle).await;
    t.run_for(SETTLED).await;
}

/// AutoMix on, every track's record stored, all `n` tracks playing.
async fn start_with(t: &TestCore, n: usize, shuffle: bool) {
    automix_on(t).await;
    store_records_of(t, n).await;
    play_n(t, n, shuffle).await;
}

async fn automix_on(t: &TestCore) {
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
}

/// Long enough for the current track to settle and the order to be decided.
const SETTLED: f64 = SETTLE_MS as f64 + 2_000.0;

async fn play_all(t: &TestCore, shuffle: bool) {
    play_n(t, FEW, shuffle).await;
}

async fn play_n(t: &TestCore, count: usize, shuffle: bool) {
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, (0..count).map(|n| format!("t{n}"))),
        start_index: 0,
        label: "Sel".into(),
        shuffle,
        start_position_ms: 0,
    })
    .await;
}

async fn upcoming(t: &TestCore) -> Vec<String> {
    t.snapshot()
        .await
        .session
        .expect("a session")
        .upcoming
        .iter()
        .map(|i| i.track.id.to_string())
        .collect()
}

async fn upcoming_items(t: &TestCore) -> Vec<QueueItem> {
    t.snapshot().await.session.expect("a session").upcoming
}

/// The whole list, history to the end: what navigation must never change.
async fn whole_list(t: &TestCore) -> Vec<String> {
    let doc = t.snapshot().await.session.expect("a session");
    doc.history
        .iter()
        .chain(doc.current.as_ref())
        .chain(doc.upcoming.iter())
        .map(|i| i.track.id.to_string())
        .collect()
}

/// How many of the pairs the order decides ahead blend (share a tempo).
fn blends_ahead(up: &[String]) -> usize {
    up.windows(2)
        .take(ORDER_AHEAD)
        .filter(|w| bpm_of(&w[0]) == bpm_of(&w[1]))
        .count()
}

/// A shuffled queue's top is ordered at once, before the current track has
/// played a while: nothing there is left to move later.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_top_of_the_queue_is_ordered_at_once() {
    let t = synced_core_with("ahead", MANY).await;
    store_records_of(&t, MANY).await;
    play_n(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    // Without AutoMix: the shuffle as it came.
    let shuffled = upcoming(&t).await;
    automix_on(&t).await;
    t.run_for(500.0).await;
    let up = upcoming(&t).await;
    let mut sorted = up.clone();
    sorted.sort();
    let mut expected = shuffled.clone();
    expected.sort();
    assert_eq!(sorted, expected, "the same entries");
    assert!(
        blends_ahead(&up) >= ORDER_AHEAD - 2 && blends_ahead(&up) > blends_ahead(&shuffled),
        "{up:?} from {shuffled:?}"
    );
}

/// Once the top is ordered, playing on decides one more pair, past it:
/// what the listener sees near the top never moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn playing_on_moves_only_what_is_far_down() {
    let t = synced_core_with("far", MANY).await;
    start_with(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    for round in 0..4 {
        let before = upcoming(&t).await;
        if round % 2 == 0 {
            t.run(Command::Next).await;
            t.run_for(SETTLED).await;
        } else {
            // Through a mix: the track plays to its end.
            let cur = t.current_track_id().await;
            t.run_for(200_000.0).await;
            assert_ne!(t.current_track_id().await, cur, "it moved on");
        }
        let after = upcoming(&t).await;
        assert_eq!(
            after[..ORDER_AHEAD],
            before[1..=ORDER_AHEAD],
            "{before:?} → {after:?}"
        );
    }
}

/// After a skip a new decision waits until the track has played a while, so
/// a burst of skips moves nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skip_holds_new_decisions_until_the_track_settles() {
    let t = synced_core_with("unsettled", MANY).await;
    start_with(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    let list = whole_list(&t).await;
    t.run(Command::Next).await;
    t.run_for(500.0).await;
    assert_eq!(whole_list(&t).await, list, "not at once");
    t.run_for(f64::from(SETTLE_MS) - 3_000.0).await;
    assert_eq!(whole_list(&t).await, list, "not before it settles");
    t.run_for(5_000.0).await;
    // (With this seed, the new pair moves something.)
    assert_ne!(whole_list(&t).await, list, "once it has");
}

/// A burst of skips (loads deferred while it lasts) still decides the order
/// around the track it lands on, once that has settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_skip_burst_decides_where_it_lands() {
    let t = synced_core("burst").await;
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    store_records(&t).await;
    play_all(&t, true).await;
    t.run_for(500.0).await;
    let shuffled = whole_list(&t).await;
    for cmd in [Command::Next, Command::PreviousTrack, Command::Next] {
        t.run(cmd).await;
        t.run_for(50.0).await;
    }
    t.run_for(1_000.0).await;
    assert_eq!(t.current_track_id().await.as_ref(), Some(&shuffled[1]));
    assert_eq!(whole_list(&t).await, shuffled, "nothing moved in the burst");
    t.run_for(SETTLED).await;
    // Decided once it settled: a candidate of the next track's tempo follows it.
    let up = upcoming(&t).await;
    if up[1..].iter().take(6).any(|t| bpm_of(t) == bpm_of(&up[0])) {
        assert_eq!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
    }
}

/// Skipping through tracks, forward and back, leaves the list as it was.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn skipping_back_and_forth_never_reorders() {
    let t = synced_core_with("skips", MANY).await;
    start_with(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    let list = whole_list(&t).await;
    for cmd in [
        Command::Next,
        Command::Next,
        Command::Next,
        Command::PreviousTrack,
        Command::PreviousTrack,
        Command::Next,
        Command::PreviousTrack,
        Command::PreviousTrack,
    ] {
        t.run(cmd).await;
        t.run_for(1_500.0).await;
        assert_eq!(whole_list(&t).await, list);
    }
    // Back where it started, settled: the pairs it had are still decided.
    t.run_for(SETTLED).await;
    assert_eq!(whole_list(&t).await, list);
}

/// An entry's follow-up is decided once: going back to the track before it
/// and letting it settle again doesn't move anything, even when the records
/// that arrived since would now choose differently.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_revisited_track_keeps_the_order_it_had() {
    let t = synced_core("revisit").await;
    t.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    play_all(&t, true).await;
    t.run(Command::Next).await;
    // Only the next track's record: its follow-up is decided blind (kept;
    // the others' records aren't coming), the rest stay undecided.
    let next = upcoming(&t).await[0].clone();
    store_record(&t, &next).await;
    t.run_for(SETTLED).await;
    store_records(&t).await;
    t.run(Command::Next).await;
    t.run_for(SETTLED).await;
    let list = whole_list(&t).await;
    let here = t.current_track_id().await;
    t.run(Command::PreviousTrack).await;
    t.run_for(SETTLED).await;
    assert_ne!(t.current_track_id().await, here, "went back");
    assert_eq!(whole_list(&t).await, list, "back a track");
    t.run(Command::Next).await;
    t.run_for(SETTLED).await;
    assert_eq!(t.current_track_id().await, here);
    assert_eq!(whole_list(&t).await, list, "and forward again");
}

/// A decided pair stays when the records that would now choose differently
/// arrive: what was settled near the top isn't moved for them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_decided_pair_stays_when_more_records_arrive() {
    let t = synced_core("stays0").await;
    automix_on(&t).await;
    play_all(&t, true).await;
    t.run_for(1_000.0).await;
    // Only the next track's record: its follow-up is decided blind (kept),
    // the others have nothing to judge by.
    let next = upcoming(&t).await[0].clone();
    store_record(&t, &next).await;
    t.run_for(500.0).await;
    let list = whole_list(&t).await;
    store_records(&t).await;
    t.run_for(500.0).await;
    let up = upcoming(&t).await;
    // (With this seed, the follow-up the shuffle gave doesn't blend and one
    // that would is among the candidates.)
    assert_ne!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
    assert!(up[1..7].iter().any(|i| bpm_of(i) == bpm_of(&up[0])));
    assert_eq!(whole_list(&t).await[..3], list[..3], "{up:?}");
}

/// A song the user drags stays where they put it, also when the order
/// reaches it later: it is settled.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dragged_entry_stays_where_the_user_put_it() {
    let t = synced_core_with("drag", MANY).await;
    start_with(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    let items = upcoming_items(&t).await;
    // Past what is decided, after an entry it doesn't blend with.
    let after = &items[ORDER_AHEAD + 1];
    let moved = items[ORDER_AHEAD + 3..]
        .iter()
        .find(|i| bpm_of(&i.track.id) != bpm_of(&after.track.id))
        .expect("a song of the other tempo");
    t.run(Command::MoveQueueItem {
        key: moved.key.clone(),
        after: after.key.clone(),
        in_group: false,
    })
    .await;
    // Play on until the order has decided past it.
    for _ in 0..4 {
        t.run(Command::Next).await;
        t.run_for(SETTLED).await;
    }
    let up = upcoming_items(&t).await;
    let at = |key: &str| up.iter().position(|i| i.key == key).expect("still queued");
    assert_eq!(
        at(&moved.key),
        at(&after.key) + 1,
        "{:?}",
        up.iter().map(|i| &i.track.id).collect::<Vec<_>>()
    );
    assert!(at(&after.key) < ORDER_AHEAD, "the order reached it");
}

/// The order's decisions are marks in the document: the top of the queue is
/// settled for every device, so a new owner doesn't decide it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_decided_top_is_settled_in_the_document() {
    let t = synced_core_with("marks", MANY).await;
    start_with(&t, MANY, true).await;
    t.run_for(2_000.0).await;
    let up = upcoming_items(&t).await;
    // The follow-ups of the first entries; past them, nothing is decided yet.
    assert!(up[1..=ORDER_AHEAD].iter().all(|i| i.settled), "{up:?}");
    assert!(up[ORDER_AHEAD + 1..].iter().all(|i| !i.settled), "{up:?}");
    // A reshuffle is a new order: nothing in it is settled, and the top is
    // decided again.
    t.run(Command::SetShuffle { enabled: false }).await;
    t.run(Command::SetShuffle { enabled: true }).await;
    t.run_for(500.0).await;
    let again = upcoming_items(&t).await;
    assert_ne!(
        again.iter().map(|i| &i.key).collect::<Vec<_>>(),
        up.iter().map(|i| &i.key).collect::<Vec<_>>()
    );
    // (A pair this device decided before, that the reshuffle happened to
    // form again, counts as decided.)
    let settled = again[1..=ORDER_AHEAD].iter().filter(|i| i.settled).count();
    assert!(settled >= ORDER_AHEAD - 1, "{again:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shuffle_puts_a_blendable_track_after_the_next() {
    let t = synced_core("order").await;
    start(&t, true).await;
    let up = upcoming(&t).await;
    assert_eq!(up.len(), 7, "{up:?}");
    // Every other candidate shares the next track's tempo: one of them follows it.
    assert_eq!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
    // And the preloaded next track's mix-out is planned against it: a blend.
    t.run_for(200.0).await;
    let next = t.backend.next_source().expect("preloaded");
    assert_eq!(next.track.id.to_string(), up[0]);
    assert!(next.mix_out_ms.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_in_order_queue_is_left_alone() {
    let t = synced_core("inorder").await;
    start(&t, false).await;
    assert_eq!(
        upcoming(&t).await,
        ["t1", "t2", "t3", "t4", "t5", "t6", "t7"]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_users_group_is_never_reordered() {
    let t = synced_core("group").await;
    start(&t, true).await;
    let before = upcoming(&t).await;
    // Queue two songs that don't blend with each other (150 → 120 BPM).
    let (a, b) = if bpm_of(&before[0]) == 120.0 {
        (
            before[0].clone(),
            before.iter().find(|i| bpm_of(i) == 150.0).unwrap().clone(),
        )
    } else {
        (
            before[0].clone(),
            before.iter().find(|i| bpm_of(i) == 120.0).unwrap().clone(),
        )
    };
    t.run(Command::PlayNext {
        tracks: TrackRef::list(&t.server_id, [a.clone(), b.clone()]),
        keys: vec![],
    })
    .await;
    t.run_for(500.0).await;
    let up = upcoming(&t).await;
    assert_eq!(up[..2], [a, b], "{up:?}");
}

/// Over a room: the promotion follows the op that changed the next entry, so
/// the room takes both, every peer sees the same order and the skips stand.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn promotions_reach_the_room_after_the_skip_that_asked_for_them() {
    let server = seeded_server(8, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let room = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let a = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&room, &a], 8_000.0).await;
    assert!(a.snapshot().await.connection.connected);
    a.run(Command::SetAutoplay { enabled: false }).await;
    a.run(Command::SetTransitionSettings {
        settings: TransitionSettings {
            mode: TransitionMode::AutoMix,
            crossfade_seconds: 6.0,
        },
    })
    .await;
    store_records(&a).await;
    play_all(&a, true).await;
    TestCore::run_all_for(&[&room, &a], SETTLED).await;
    for _ in 0..3 {
        let before = upcoming(&a).await;
        a.run(Command::Next).await;
        TestCore::run_all_for(&[&room, &a], SETTLED).await;
        let cur = a.current_track_id().await.expect("playing");
        assert_eq!(cur, before[0], "the skip stood");
        let up = upcoming(&a).await;
        if up.len() >= 3 && up[1..].iter().any(|t| bpm_of(t) == bpm_of(&up[0])) {
            assert_eq!(bpm_of(&up[0]), bpm_of(&up[1]), "{up:?}");
        }
        assert_eq!(
            upcoming(&room).await,
            up,
            "the room's peer has the same order"
        );
        let marks = |items: Vec<QueueItem>| items.iter().map(|i| i.settled).collect::<Vec<_>>();
        assert_eq!(
            marks(upcoming_items(&room).await),
            marks(upcoming_items(&a).await),
            "and the same marks"
        );
    }
    a.core.shutdown().await;
    room.core.shutdown().await;
}
