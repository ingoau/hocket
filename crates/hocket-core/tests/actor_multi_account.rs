//! Two Navidrome accounts on one core (docs/providers.md, Phase 1): one
//! library, a queue mixing both, each track played, downloaded and
//! scrobbled through its own account, and an outbox that keeps flushing to
//! one account while the other is offline.
//!
//! Both servers have a song with id `t0`: track references keep them apart.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server_with_id, TestCore};
use hocket_core::subsonic::fake::FakeServer;

const A: &str = "srv-a";
const B: &str = "srv-b";

/// Server B: `t0` (same id as A's first song, another song) and `b1`..`b3`.
fn server_b() -> FakeServer {
    let s = FakeServer::new(B, "bob");
    for (i, id) in ["t0", "b1", "b2", "b3"].iter().enumerate() {
        let mut c = FakeServer::song(id, &format!("B song {i}"), "bal0", "bar0", 200.0);
        c.track = Some(i as u32 + 1);
        s.add_song(c);
        s.set_media(id, vec![7u8; 4096]);
    }
    s
}

async fn two_accounts(name: &str) -> (TestCore, FakeServer) {
    let a = seeded_server_with_id(A, 4, 200.0);
    let b = server_b();
    let t = TestCore::start_accounts(name, a, vec![b.clone()]).await;
    // Both launch syncs finish (they run one after the other).
    t.wait_until(60_000.0, |t| {
        let done: Vec<String> = t
            .events
            .all()
            .iter()
            .filter_map(|e| match e {
                Event::SyncProgress { progress } if progress.finished => {
                    Some(progress.server_id.clone())
                }
                _ => None,
            })
            .collect();
        done.iter().any(|s| s == A) && done.iter().any(|s| s == B)
    })
    .await;
    (t, b)
}

fn current(q: &QueueView) -> Option<(String, String, String)> {
    q.current.as_ref().map(|e| {
        (
            e.item.track.source.clone(),
            e.item.track.id.clone(),
            e.track.title.clone(),
        )
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_accounts_share_one_library_and_one_mixed_queue() {
    let (t, b) = two_accounts("multi").await;

    // Accounts, in priority order; the session belongs to the first.
    let accounts = t.snapshot().await.accounts;
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![A, B]);
    assert!(accounts.iter().all(|a| a.kind == AccountKind::Navidrome));
    assert!(accounts[0].features.stream && accounts[0].features.report_play);

    // One library: an empty server id lists every account's songs.
    match t
        .query(Query::Tracks {
            server_id: String::new(),
            filter: None,
            sort: SortOrder::Title,
            descending: false,
            page: Page {
                offset: 0,
                limit: 100,
            },
        })
        .await
    {
        QueryResult::Tracks(p) => {
            assert_eq!(p.total, 8);
            let t0s: Vec<&str> = p
                .items
                .iter()
                .filter(|t| t.id == "t0")
                .map(|t| t.server_id.as_str())
                .collect();
            assert_eq!(t0s.len(), 2, "both t0s, one per account");
        }
        other => panic!("{other:?}"),
    }

    // Platforms name the session's account for the unified library: the
    // same list.
    match t
        .query(Query::TrackCount {
            server_id: A.into(),
            filter: None,
        })
        .await
    {
        QueryResult::Count(n) => assert_eq!(n, 8),
        other => panic!("{other:?}"),
    }

    // A mixed queue: A's t0 and t1, then B's t0 and b1.
    t.run(Command::PlayTracks {
        server_id: A.into(),
        track_ids: vec!["t0".into(), "t1".into()],
        start_index: 0,
        label: "Mixed".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run(Command::PlayLater {
        server_id: B.into(),
        track_ids: vec!["t0".into(), "b1".into()],
        keys: vec![],
    })
    .await;
    // A list named by the session's account with another account's song
    // in it: the song is the account's that has it.
    t.run(Command::PlayLater {
        server_id: A.into(),
        track_ids: vec!["b3".into()],
        keys: vec![],
    })
    .await;
    let q = t.queue().await;
    assert_eq!(current(&q), Some((A.into(), "t0".into(), "Track 0".into())));
    let upcoming: Vec<(String, String, String)> = q
        .upcoming()
        .map(|e| {
            (
                e.item.track.source.clone(),
                e.item.track.id.clone(),
                e.track.title.clone(),
            )
        })
        .collect();
    assert_eq!(
        upcoming,
        vec![
            (A.into(), "t1".into(), "Track 1".into()),
            (B.into(), "t0".into(), "B song 0".into()),
            (B.into(), "b1".into(), "B song 1".into()),
            (B.into(), "b3".into(), "B song 3".into()),
        ]
    );

    // Playback resolves each item through its own account.
    t.run(Command::Next).await;
    t.run(Command::Next).await;
    t.run_for(1_000.0).await;
    assert_eq!(
        current(&t.queue().await),
        Some((B.into(), "t0".into(), "B song 0".into()))
    );
    let current_key = t.queue().await.current.expect("current").item.key;
    let loaded = t
        .sources
        .lock()
        .iter()
        .rev()
        .find(|s| s.key == current_key)
        .cloned()
        .expect("the current item was loaded");
    assert_eq!(loaded.track.server_id, B);
    assert_eq!(loaded.track.title, "B song 0");

    // Scrobbles go to the account the song came from: B's t0 past its
    // threshold (100 s of a 200 s song) is B's, never A's.
    t.run_for(110_000.0).await;
    let b_submitted: Vec<String> = b
        .scrobbles()
        .into_iter()
        .filter(|s| s.submission)
        .map(|s| s.id)
        .collect();
    assert_eq!(b_submitted, vec!["t0".to_string()]);
    assert!(
        t.server.scrobbles().iter().all(|s| !s.submission),
        "A heard of no finished play"
    );
    match t.query(Query::RecentlyPlayed { limit: 10 }).await {
        QueryResult::History(h) => {
            assert_eq!(h.len(), 1);
            assert_eq!(
                (h[0].track.server_id.as_str(), h[0].track.title.as_str()),
                (B, "B song 0")
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_and_the_outbox_go_to_each_tracks_account() {
    let (t, b) = two_accounts("multi-dl").await;

    // A pin of B's song downloads from B.
    t.run(Command::Pin {
        target: PinTarget::Track { id: "b1".into() },
        transcode: false,
    })
    .await;
    t.wait_until(20_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::PinsChanged { pins } if pins.iter().any(|p| p.downloaded_count == 1))
        })
    })
    .await;
    assert_eq!(b.calls_to("download"), 1);
    assert_eq!(t.server.calls_to("download"), 0);
    match t.query(Query::Pins).await {
        QueryResult::Pins(pins) => {
            assert_eq!(pins.len(), 1);
            assert_eq!(pins[0].target, PinTarget::Track { id: "b1".into() });
        }
        other => panic!("{other:?}"),
    }

    // A goes offline. Loves for a song of each account: B's lands, A's
    // waits in the outbox.
    t.server.set_offline(true);
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track { id: "b2".into() }],
        loved: true,
    })
    .await;
    t.run(Command::SetLoved {
        targets: vec![RatingTarget::Track { id: "t1".into() }],
        loved: true,
    })
    .await;
    t.run_for(5_000.0).await;
    assert!(b.starred("b2"), "B's write went through");
    assert!(!t.server.starred("t1"), "A is offline");

    // A is back: the next flush delivers what waited.
    t.server.set_offline(false);
    t.wait_until(120_000.0, |t| t.server.starred("t1")).await;
    assert!(t.server.starred("t1"));
    assert!(!b.starred("t1"), "never sent to the other account");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reordering_moves_the_session_to_the_new_first_account() {
    let (t, _b) = two_accounts("multi-order").await;
    t.run(Command::ReorderAccounts {
        account_ids: vec![B.into(), A.into()],
    })
    .await;
    let accounts = t.snapshot().await.accounts;
    let ids: Vec<&str> = accounts.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, vec![B, A]);
    assert_eq!(accounts[0].priority, 0);
    // The session is B's now: its scope names B's server.
    let doc = t.snapshot().await.session.expect("a session");
    assert!(doc.scope.contains("srv-b"), "{}", doc.scope);

    t.run(Command::RemoveAccount {
        account_id: B.into(),
    })
    .await;
    let accounts = t.snapshot().await.accounts;
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, A);
    match t
        .query(Query::TrackCount {
            server_id: String::new(),
            filter: None,
        })
        .await
    {
        QueryResult::Count(n) => assert_eq!(n, 4, "B's mirror went with it"),
        other => panic!("{other:?}"),
    }
}
