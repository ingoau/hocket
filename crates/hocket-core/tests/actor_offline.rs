//! Offline launch: a server this device already authenticated with is
//! installed immediately from its persisted row (downloads play with no
//! network), verified in the background, and re-probed when the network
//! comes back. Plus: `AddServer` rejects URLs that are not plain http(s).

#![cfg(feature = "sim")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server_with_id, server_id_for, TestCore};
use hocket_core::sim::SimTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A Subsonic stub that answers every request with an `ok` ping while
/// `online`, and drops connections otherwise.
async fn stub_server() -> (String, Arc<AtomicBool>) {
    let online = Arc::new(AtomicBool::new(true));
    let flag = online.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
            if !flag.load(Ordering::SeqCst) {
                drop(sock);
                continue;
            }
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut read = 0;
                loop {
                    let Ok(n) = sock.read(&mut buf[read..]).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    read += n;
                    if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") || read == buf.len() {
                        break;
                    }
                }
                let body = r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.63.1","openSubsonic":true}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    (format!("http://{addr}/"), online)
}

fn network(kind: NetworkKind) -> Command {
    Command::SetNetworkState {
        state: NetworkState {
            kind,
            metered: false,
            network_id: Some("home".into()),
        },
    }
}

async fn wait_for_probe_result(t: &TestCore, want_reachable: bool) {
    for _ in 0..400 {
        t.core.settle().await;
        let servers = match t.query(Query::Servers).await {
            QueryResult::Servers(s) => s,
            other => panic!("{other:?}"),
        };
        let reached = servers.first().is_some_and(|s| s.reachable);
        let failed = t.events.all().iter().any(|e| {
            matches!(
                e,
                Event::Error {
                    kind: ErrorKind::Network,
                    ..
                }
            )
        });
        if (want_reachable && reached) || (!want_reachable && failed) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("probe did not finish (want reachable = {want_reachable})");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_server_installs_offline_plays_downloads_and_reprobes_online() {
    let (url, online) = stub_server().await;
    // First run: the server is authenticated and a track is pinned.
    let server_id = server_id_for(&url, "alice");
    let server = seeded_server_with_id(&server_id, 2, 100.0);
    server.set_media("t0", vec![0u8; 4096]);
    let clock = SimTime::new(1_700_000_000_000.0);
    let dir = tempfile::tempdir().unwrap();
    let t = TestCore::start_in_with(
        "first",
        server.clone(),
        None,
        1,
        clock.clone(),
        dir,
        true,
        &url,
        "secret",
    )
    .await;
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t.run(Command::Pin {
        target: PinTarget::Track { id: "t0".into() },
        transcode: false,
    })
    .await;
    t.wait_until(20_000.0, |t| {
        t.events.all().iter().any(|e| {
            matches!(e, Event::PinsChanged { pins } if pins.iter().any(|p| p.downloaded_count == 1))
        })
    })
    .await;
    t.core.shutdown().await;
    let TestCore { dir, .. } = t;

    // Second run, offline, the way a platform starts: the persisted row is
    // there but no API until the keystore replays `AddServer`.
    online.store(false, Ordering::SeqCst);
    let t = TestCore::start_in_with(
        "second", server, None, 1, clock, dir, false, &url, "secret",
    )
    .await;
    t.run(network(NetworkKind::Offline)).await;
    t.events.clear();
    t.run(Command::AddServer {
        url: url.clone(),
        username: "alice".into(),
        password: "secret".into(),
        name: None,
    })
    .await;
    // Installed at once, marked unreachable, announced.
    let servers = match t.query(Query::Servers).await {
        QueryResult::Servers(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].id, server_id);
    assert!(!servers[0].reachable);
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::ServersChanged { servers } if servers.len() == 1)));
    // The pinned download plays with no network at all.
    match t
        .query(Query::MediaSource {
            track_id: "t0".into(),
        })
        .await
    {
        QueryResult::Source(Some(s)) => assert!(s.url.starts_with("file://"), "{}", s.url),
        other => panic!("{other:?}"),
    }
    t.run(Command::PlayTracks {
        server_id: server_id.clone(),
        track_ids: vec!["t0".into()],
        start_index: 0,
        label: "Downloaded".into(),
        shuffle: false,
    })
    .await;
    t.run_for(500.0).await;
    assert!(t.backend.is_playing(), "the download plays offline");
    // The background probe fails (stub is down): an error, but the server
    // stays installed.
    wait_for_probe_result(&t, false).await;
    let servers = match t.query(Query::Servers).await {
        QueryResult::Servers(s) => s,
        other => panic!("{other:?}"),
    };
    assert_eq!(servers.len(), 1, "a failed probe keeps the server");
    assert!(!servers[0].reachable);
    assert!(t.backend.is_playing());

    // The network comes back: re-probe, now reachable.
    online.store(true, Ordering::SeqCst);
    t.events.clear();
    t.run(network(NetworkKind::Wifi)).await;
    wait_for_probe_result(&t, true).await;
    let servers = match t.query(Query::Servers).await {
        QueryResult::Servers(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(servers[0].reachable);
    assert!(servers[0].capabilities.open_subsonic);
    assert!(t
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::ServersChanged { servers } if servers[0].reachable)));
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_server_rejects_urls_that_are_not_plain_http() {
    let t = TestCore::start("urls", seeded_server_with_id("srv", 1, 100.0)).await;
    for url in [
        "mailto:alice@music.example",
        "data:text/plain,hello",
        "ftp://music.example/",
        "http://alice:pw@music.example/",
        "http:///rest",
        "not a url",
    ] {
        t.events.clear();
        t.run(Command::AddServer {
            url: url.into(),
            username: "alice".into(),
            password: "pw".into(),
            name: None,
        })
        .await;
        t.core.settle().await;
        assert!(
            t.events.all().iter().any(|e| matches!(
                e,
                Event::Toast { toast } if toast.message.starts_with("Invalid server URL")
            )),
            "{url}: {:?}",
            t.events.all()
        );
        assert!(
            !t.events
                .all()
                .iter()
                .any(|e| matches!(e, Event::ServersChanged { .. } | Event::Error { .. })),
            "{url}: nothing was probed or installed"
        );
    }
    // The preset server is untouched.
    match t.query(Query::Servers).await {
        QueryResult::Servers(s) => {
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].id, "srv");
        }
        other => panic!("{other:?}"),
    }
}
