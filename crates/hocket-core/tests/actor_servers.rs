//! Adding a server: the probe decides. Bad credentials or an unreachable
//! host leave no trace (no server row, no job, no problem).

#![cfg(feature = "sim")]

use std::sync::Arc;

use hocket_core::api::*;
use hocket_core::core::test_support::{EventLog, TestBackend, TestOptions};
use hocket_core::sim::SimTime;
use hocket_core::Core;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A one-endpoint Subsonic stub answering every request with `body`.
async fn stub_server(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                break;
            };
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
    format!("http://{addr}/")
}

struct Bare {
    core: Core,
    events: Arc<EventLog>,
    _dir: tempfile::TempDir,
}

async fn bare_core(name: &str) -> Bare {
    let clock = SimTime::new(1_700_000_000_000.0);
    let dir = tempfile::tempdir().unwrap();
    let config = CoreConfig {
        data_dir: dir.path().join("d").to_string_lossy().into_owned(),
        cache_dir: dir.path().join("c").to_string_lossy().into_owned(),
        device_id: format!("dev-{name}"),
        device_name: name.into(),
        platform: Platform::Linux,
        app_version: "test".into(),
        audio: AudioMode::None,
        coordinator_listen: None,
    };
    let (backend, _scripted) = TestBackend::scripted(clock.clone());
    let core = Core::new_for_test_with(
        config,
        clock,
        TestOptions {
            backend,
            api: None,
            server_url: String::new(),
            password: String::new(),
            net: None,
            lyrics_http: None,
            seed: 3,
        },
    )
    .unwrap();
    let events = Arc::new(EventLog::default());
    core.add_sink(events.clone());
    core.dispatch(Command::Start).unwrap();
    core.settle().await;
    Bare {
        core,
        events,
        _dir: dir,
    }
}

async fn wait_for_error(b: &Bare) -> ErrorKind {
    for _ in 0..600 {
        b.core.settle().await;
        if let Some(k) = b.events.all().iter().find_map(|e| match e {
            Event::Error { kind, .. } => Some(*kind),
            _ => None,
        }) {
            return k;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("no Event::Error");
}

async fn assert_nothing_added(b: &Bare) {
    match b.core.query(Query::Servers).await.unwrap() {
        QueryResult::Servers(s) => assert!(s.is_empty(), "no server row: {s:?}"),
        other => panic!("{other:?}"),
    }
    match b.core.query(Query::Jobs).await.unwrap() {
        QueryResult::Jobs(j) => assert!(j.is_empty(), "no job: {j:?}"),
        other => panic!("{other:?}"),
    }
    match b.core.query(Query::Problems).await.unwrap() {
        QueryResult::Problems(p) => assert!(p.is_empty(), "no problem: {p:?}"),
        other => panic!("{other:?}"),
    }
    let events = b.events.all();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::ServersChanged { servers } if !servers.is_empty())),
        "no ServersChanged with a server"
    );
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::SyncProgress { .. })));
    assert!(events.iter().any(|e| matches!(e, Event::Toast { .. })));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_server_with_wrong_password_adds_nothing() {
    let url = stub_server(
        r#"{"subsonic-response":{"status":"failed","version":"1.16.1","type":"navidrome","serverVersion":"0.63.1","error":{"code":40,"message":"Wrong username or password"}}}"#,
    )
    .await;
    let b = bare_core("auth").await;
    b.core
        .dispatch(Command::AddServer {
            url,
            username: "alice".into(),
            password: "nope".into(),
            name: None,
        })
        .unwrap();
    assert_eq!(wait_for_error(&b).await, ErrorKind::Auth);
    assert_nothing_added(&b).await;
    let snap = match b.core.query(Query::Snapshot).await.unwrap() {
        QueryResult::SnapshotResult(s) => s,
        other => panic!("{other:?}"),
    };
    assert!(snap.session.is_none(), "no session was opened");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_server_that_is_unreachable_adds_nothing() {
    let b = bare_core("net").await;
    b.core
        .dispatch(Command::AddServer {
            url: "http://127.0.0.1:9/".into(),
            username: "alice".into(),
            password: "pw".into(),
            name: Some("Nowhere".into()),
        })
        .unwrap();
    assert_eq!(wait_for_error(&b).await, ErrorKind::Network);
    assert_nothing_added(&b).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn add_server_that_answers_the_probe_is_persisted_and_synced() {
    // An old server: the ping succeeds, so it is added with a floor warning,
    // and the sync starts (the stub answers every endpoint with a bare ok).
    let url = stub_server(
        r#"{"subsonic-response":{"status":"ok","version":"1.16.1","type":"navidrome","serverVersion":"0.62.0","openSubsonic":false}}"#,
    )
    .await;
    let b = bare_core("old").await;
    b.core
        .dispatch(Command::AddServer {
            url: url.clone(),
            username: "alice".into(),
            password: "pw".into(),
            name: Some("Old".into()),
        })
        .unwrap();
    for _ in 0..600 {
        b.core.settle().await;
        if b.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::ServersChanged { servers } if !servers.is_empty()))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    match b.core.query(Query::Servers).await.unwrap() {
        QueryResult::Servers(s) => {
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].name, "Old");
            assert_eq!(s[0].url, url);
            assert!(!s[0].capabilities.meets_floor);
            assert!(s[0].reachable);
        }
        other => panic!("{other:?}"),
    }
    assert!(b
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Log { message, .. } if message.contains("floor"))));
    match b.core.query(Query::Jobs).await.unwrap() {
        QueryResult::Jobs(j) => assert!(j.iter().any(|j| j.kind == JobKind::LibrarySync)),
        other => panic!("{other:?}"),
    }
    assert!(!b
        .events
        .all()
        .iter()
        .any(|e| matches!(e, Event::Error { .. })));
    b.core.dispatch(Command::Shutdown).unwrap();
}
