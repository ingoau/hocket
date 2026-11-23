//! Offline mode, data saver on metered connections, and downloads that wait
//! for an unmetered connection (`downloads.wifiOnly`).

#![cfg(feature = "sim")]

mod stream_common;

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestCore};
use stream_common::*;

fn network(kind: NetworkKind, metered: bool) -> Command {
    Command::SetNetworkState {
        state: NetworkState {
            kind,
            metered,
            network_id: None,
        },
    }
}

fn setting(key: &str, value: &str) -> Command {
    Command::SetSetting {
        key: key.into(),
        value: value.into(),
    }
}

async fn started(name: &str) -> TestCore {
    let t = TestCore::start(name, seeded_server(4, 100.0)).await;
    core_stream(&t).await;
    t.run(Command::SetAutoplay { enabled: false }).await;
    t
}

async fn play_album(t: &TestCore) {
    t.run(Command::PlayTracks {
        tracks: TrackRef::list(&t.server_id, (0..4).map(|i| format!("t{i}"))),
        start_index: 0,
        label: "Album".into(),
        shuffle: false,
        start_position_ms: 0,
    })
    .await;
    t.run_for(1000.0).await;
}

async fn download_job(t: &TestCore) -> Job {
    t.snapshot()
        .await
        .jobs
        .into_iter()
        .find(|j| j.kind == JobKind::Download)
        .expect("a download job")
}

async fn wait_job_state(t: &TestCore, state: JobState) -> Job {
    for _ in 0..200 {
        let j = download_job(t).await;
        if j.state == state {
            return j;
        }
        t.run_for(250.0).await;
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "download never reached {state:?}: {:?}",
        download_job(t).await
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn data_saver_does_not_load_a_streamed_follow_up_ahead() {
    let t = started("saver-preload").await;
    t.run(network(NetworkKind::Cellular, true)).await;
    play_album(&t).await;
    assert!(
        t.backend.next_source().is_none(),
        "metered with data saver: t1 would stream, so it is not preloaded"
    );
    t.run(setting("network.dataSaver", "false")).await;
    t.run_for(500.0).await;
    assert!(
        t.backend.next_source().is_some(),
        "data saver off: gapless as usual"
    );
    t.run(setting("network.dataSaver", "true")).await;
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run_for(500.0).await;
    assert!(
        t.backend.next_source().is_some(),
        "unmetered: gapless as usual"
    );
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn data_saver_still_preloads_a_follow_up_on_the_device() {
    let t = started("saver-preload-local").await;
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t1".into() },
        transcode: false,
    })
    .await;
    wait_offline(&t, "t1", OfflineState::Downloaded).await;
    t.run(network(NetworkKind::Cellular, true)).await;
    play_album(&t).await;
    let next = t
        .backend
        .next_source()
        .expect("t1 is downloaded: preloaded");
    assert!(next.url.starts_with("file:") || next.local, "{next:?}");
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_wait_for_an_unmetered_connection_and_then_run() {
    let t = started("wifi-only").await;
    t.run(network(NetworkKind::Cellular, true)).await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.run_for(2000.0).await;
    let j = download_job(&t).await;
    assert_eq!(j.state, JobState::Queued);
    assert_eq!(j.waiting, Some(JobWait::Metered));
    assert_eq!(t.upstream.stream_requests("t0"), 0);
    // Wi-Fi: it runs by itself.
    t.run(network(NetworkKind::Wifi, false)).await;
    let j = wait_job_state(&t, JobState::Done).await;
    assert_eq!(j.waiting, None);
    wait_offline(&t, "t0", OfflineState::Downloaded).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn downloads_run_on_a_metered_connection_without_wifi_only() {
    let t = started("metered-allowed").await;
    t.run(setting("downloads.wifiOnly", "false")).await;
    t.run(network(NetworkKind::Cellular, true)).await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    wait_job_state(&t, JobState::Done).await;
    // Switched back on mid-way: the next download waits again.
    t.run(setting("downloads.wifiOnly", "true")).await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t1".into() },
        transcode: false,
    })
    .await;
    t.run_for(1000.0).await;
    let waiting: Vec<_> = t
        .snapshot()
        .await
        .jobs
        .into_iter()
        .filter(|j| j.kind == JobKind::Download && j.waiting == Some(JobWait::Metered))
        .collect();
    assert_eq!(waiting.len(), 1, "{waiting:?}");
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn offline_mode_acts_as_offline_until_turned_off() {
    let t = started("offline-mode").await;
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run(setting("network.offlineMode", "true")).await;
    let s = t.snapshot().await;
    assert_eq!(
        s.network.as_ref().map(|n| n.kind),
        Some(NetworkKind::Offline)
    );
    // Nothing that needs the server runs: downloads wait.
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.run_for(2000.0).await;
    let j = download_job(&t).await;
    assert_eq!(j.waiting, Some(JobWait::Offline));
    assert_eq!(t.upstream.stream_requests("t0"), 0);
    // The device's network moving underneath changes nothing.
    t.run(network(NetworkKind::Cellular, false)).await;
    let s = t.snapshot().await;
    assert_eq!(
        s.network.as_ref().map(|n| n.kind),
        Some(NetworkKind::Offline)
    );
    // Off: the device's network is back, and the download runs.
    t.run(setting("network.offlineMode", "false")).await;
    let s = t.snapshot().await;
    assert_eq!(
        s.network.as_ref().map(|n| n.kind),
        Some(NetworkKind::Cellular)
    );
    wait_job_state(&t, JobState::Done).await;
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_network_reported_again_changes_nothing() {
    let t = started("same-network").await;
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run_for(500.0).await;
    t.events.clear();
    t.run(network(NetworkKind::Wifi, false)).await;
    t.run_for(500.0).await;
    assert!(
        !t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::NetworkChanged { .. })),
        "a repeated report is not a change"
    );
    t.core.shutdown().await;
}
