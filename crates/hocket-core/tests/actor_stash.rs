//! The song stash through full cores: stash from the registry, the toasts and their Undo, acting on
//! the stash without emptying it, surviving a restart, and two devices on the in-memory LAN
//! sharing it.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;

async fn stash(t: &TestCore) -> Vec<StashItem> {
    match t.query(Query::Stash).await {
        QueryResult::StashItems(items) => items,
        other => panic!("{other:?}"),
    }
}

async fn stashed(t: &TestCore) -> Vec<String> {
    stash(t).await.into_iter().map(|i| i.track.id).collect()
}

fn toasts(t: &TestCore) -> Vec<Toast> {
    t.events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::Toast { toast } => Some(toast),
            _ => None,
        })
        .collect()
}

fn last_stash_event(t: &TestCore) -> Option<Vec<String>> {
    t.events.all().into_iter().rev().find_map(|e| match e {
        Event::StashChanged { items } => Some(items.into_iter().map(|i| i.track.id).collect()),
        _ => None,
    })
}

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stash_act_on_it_and_keep_it() {
    let t = TestCore::start("a", seeded_server(6, 200.0)).await;
    t.run(Command::RunAction {
        action_id: "stash".into(),
        target: ActionTarget::Tracks {
            ids: ids(&["t0", "t1"]),
        },
    })
    .await;
    let items = stash(&t).await;
    assert_eq!(
        items
            .iter()
            .map(|i| i.track.id.as_str())
            .collect::<Vec<_>>(),
        ["t0", "t1"]
    );
    assert!(!items[0].track.title.is_empty(), "resolved from the mirror");
    assert_eq!(last_stash_event(&t), Some(ids(&["t0", "t1"])));
    assert_eq!(toasts(&t).last().unwrap().message, "Stashed 2 songs");

    // Stashing a song that's there already is a no-op.
    t.events.clear();
    t.run(Command::StashAdd {
        track_ids: ids(&["t1"]),
    })
    .await;
    assert_eq!(stashed(&t).await, ids(&["t0", "t1"]));
    assert_eq!(last_stash_event(&t), None);
    assert_eq!(toasts(&t).last().unwrap().message, "Already in the stash");

    // Queue it: the stash stays.
    t.run(Command::StashAdd {
        track_ids: ids(&["t2"]),
    })
    .await;
    t.run(Command::RunAction {
        action_id: "playLater".into(),
        target: ActionTarget::Tracks {
            ids: stashed(&t).await,
        },
    })
    .await;
    assert_eq!(stashed(&t).await, ids(&["t0", "t1", "t2"]));

    // Reorder, remove with Undo.
    let items = stash(&t).await;
    t.run(Command::StashMove {
        entry_id: items[2].entry_id.clone(),
        to_index: 0,
    })
    .await;
    assert_eq!(stashed(&t).await, ids(&["t2", "t0", "t1"]));
    t.events.clear();
    t.run(Command::StashRemove {
        entry_ids: vec![items[0].entry_id.clone()],
    })
    .await;
    assert_eq!(stashed(&t).await, ids(&["t2", "t1"]));
    let toast = toasts(&t).pop().unwrap();
    assert_eq!(toast.message, "Removed 1 song from the stash");
    let undo: Command = serde_json::from_str(toast.action_command.as_deref().unwrap()).unwrap();
    t.run(undo).await;
    assert_eq!(stashed(&t).await, ids(&["t2", "t0", "t1"]));

    // Clear, with Undo.
    t.events.clear();
    t.run(Command::RunAction {
        action_id: "clearStash".into(),
        target: ActionTarget::None,
    })
    .await;
    assert!(stashed(&t).await.is_empty());
    let toast = toasts(&t).pop().unwrap();
    assert_eq!(toast.message, "Stash cleared");
    let undo: Command = serde_json::from_str(toast.action_command.as_deref().unwrap()).unwrap();
    t.run(undo).await;
    assert_eq!(stashed(&t).await, ids(&["t2", "t0", "t1"]));

    // And it's still there after a restart.
    let t = t.restart("a").await;
    assert_eq!(stashed(&t).await, ids(&["t2", "t0", "t1"]));
    assert_eq!(last_stash_event(&t), Some(ids(&["t2", "t0", "t1"])));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stash_an_album() {
    let t = TestCore::start("a", seeded_server(6, 200.0)).await;
    let album = match t.query(Query::Track { id: "t0".into() }).await {
        QueryResult::TrackDetail(Some(tr)) => tr.album_id.expect("album"),
        other => panic!("{other:?}"),
    };
    t.run(Command::RunAction {
        action_id: "stash".into(),
        target: ActionTarget::Albums { ids: vec![album] },
    })
    .await;
    assert!(stashed(&t).await.contains(&"t0".to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_share_the_stash() {
    let server = seeded_server(6, 200.0);
    let net = MemoryNet::new();
    let clock = SimTime::new(1_700_000_000_000.0);
    let a = TestCore::start_on("a", server.clone(), Some(net.clone()), 1, clock.clone()).await;
    let b = TestCore::start_on("b", server.clone(), Some(net.clone()), 2, clock.clone()).await;
    TestCore::run_all_for(&[&a, &b], 8_000.0).await;
    assert!(a.snapshot().await.connection.connected);

    a.run(Command::StashAdd {
        track_ids: ids(&["t0", "t1"]),
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(stashed(&b).await, ids(&["t0", "t1"]));
    assert_eq!(last_stash_event(&b), Some(ids(&["t0", "t1"])));

    b.run(Command::StashAdd {
        track_ids: ids(&["t3"]),
    })
    .await;
    let first = stash(&b).await[0].entry_id.clone();
    b.run(Command::StashRemove {
        entry_ids: vec![first],
    })
    .await;
    TestCore::run_all_for(&[&a, &b], 1_000.0).await;
    assert_eq!(stashed(&a).await, ids(&["t1", "t3"]));
    assert_eq!(stashed(&b).await, ids(&["t1", "t3"]));
}
