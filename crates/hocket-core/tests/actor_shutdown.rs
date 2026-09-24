//! Awaitable shutdown: `Core::shutdown()` resolves only after the actor has
//! flushed the session document and position, a second call is a no-op,
//! and `shutdown_blocking` works from a plain thread.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::audio::scripted::ScriptedCall;
use hocket_core::core::test_support::{seeded_server, TestCore};

fn album_args(server_id: &str, album: &str) -> PlayContextArgs {
    PlayContextArgs {
        context: QueueContext {
            server_id: server_id.into(),
            kind: ContextKind::Album { id: album.into() },
            label: String::new(),
            sort: SortOrder::Default,
            tracks: vec![],
        },
        start_index: Some(0),
        shuffle: false,
        save_outgoing: true,
    }
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_resolves_after_flush_and_restart_restores_state() {
    let t = synced_core("a").await;
    let sid = t.server_id.clone();
    t.run(Command::PlayContext {
        args: album_args(&sid, "al0"),
    })
    .await;
    t.run_for(500.0).await;
    t.run(Command::SeekTo {
        position_ms: 42_000,
    })
    .await;
    // Move the queue on without letting the document debounce (250 ms) or
    // the position heartbeat (30 s) run: only the shutdown flush can write.
    t.run(Command::Next).await;
    t.run(Command::SeekTo {
        position_ms: 17_000,
    })
    .await;
    assert_eq!(t.current_track_id().await.as_deref(), Some("t1"));

    // Restart on the same data dir: `restart` awaits `Core::shutdown()`.
    let t = t.restart("a").await;
    let q = t.queue().await;
    assert_eq!(
        q.current.as_ref().map(|c| c.track.id.as_str()),
        Some("t1"),
        "the session document written by the shutdown flush is restored"
    );
    assert!(!t.backend.is_playing(), "never auto-resumes on start");
    // The resume point is dormant until Play: the load then starts where
    // the shutdown flush left off.
    t.run(Command::Play).await;
    t.run_for(500.0).await;
    assert!(t.backend.is_playing());
    let load = t
        .backend
        .log()
        .into_iter()
        .find_map(|c| match c {
            ScriptedCall::Load { position_ms, .. } => Some(position_ms),
            _ => None,
        })
        .expect("play loads the restored item");
    assert_eq!(
        load, 17_000,
        "the position written by the shutdown flush is restored"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn second_shutdown_is_a_noop_and_dispatch_fails_after() {
    let t = TestCore::start("b", seeded_server(2, 100.0)).await;
    t.core.shutdown().await;
    // Already stopped: resolves immediately instead of hanging.
    tokio::time::timeout(std::time::Duration::from_secs(5), t.core.shutdown())
        .await
        .expect("second shutdown resolves");
    assert!(matches!(
        t.core.dispatch(Command::RequestSnapshot),
        Err(hocket_core::CoreError::ShutDown)
    ));
    assert!(t.core.query(Query::Snapshot).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_after_command_shutdown_resolves() {
    let t = TestCore::start("c", seeded_server(2, 100.0)).await;
    t.core.dispatch(Command::Shutdown).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), t.core.shutdown())
        .await
        .expect("shutdown after Command::Shutdown resolves");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_blocking_from_a_plain_thread() {
    let t = TestCore::start("d", seeded_server(2, 100.0)).await;
    let core = t.core.clone();
    let flushed = tokio::task::spawn_blocking(move || {
        core.shutdown_blocking(std::time::Duration::from_secs(5))
    })
    .await
    .unwrap();
    assert!(flushed);
    // And again: still true, still immediate.
    let core = t.core.clone();
    let again = tokio::task::spawn_blocking(move || {
        core.shutdown_blocking(std::time::Duration::from_secs(5))
    })
    .await
    .unwrap();
    assert!(again);
}

// -- owned-runtime teardown -------------------------------------------------
//
// `Core::new` owns its tokio runtime. Dropping a `Runtime` from inside any
// runtime context panics ("Cannot drop a runtime in a context where blocking
// is not allowed"), and from one of its own blocking threads it would wait on
// itself. The last `Core` handle can go away anywhere a platform happens to
// hold it: a plain thread, a task on the platform's own runtime, or a task on
// the core's runtime (the old `shutdown_blocking` kept a clone in exactly
// such a task). Every combination below must shut down cleanly, with the
// dispatched state on disk.

mod owned_runtime {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Once};
    use std::time::Duration;

    use hocket_core::api::*;
    use hocket_core::Core;

    const TIMEOUT: Duration = Duration::from_secs(20);

    /// Panics anywhere in the process (including inside tokio tasks, whose
    /// panics the runtime would otherwise swallow).
    static PANICS: AtomicUsize = AtomicUsize::new(0);

    fn count_panics() {
        static HOOK: Once = Once::new();
        HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                PANICS.fetch_add(1, Ordering::SeqCst);
                previous(info);
            }));
        });
    }

    fn config(dir: &std::path::Path) -> CoreConfig {
        CoreConfig {
            data_dir: dir.join("data").to_string_lossy().into_owned(),
            cache_dir: dir.join("cache").to_string_lossy().into_owned(),
            device_id: "dev-owned".into(),
            device_name: "Owned runtime".into(),
            platform: Platform::Linux,
            app_version: "test".into(),
            audio: AudioMode::None,
            coordinator_listen: None,
        }
    }

    fn volume(core: &Core) -> f64 {
        let q = core.query(Query::Snapshot);
        match core.runtime().block_on(q).expect("snapshot") {
            QueryResult::SnapshotResult(s) => s.transport.volume,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum ShutdownOn {
        /// `shutdown_blocking` on a plain thread (the JNI path).
        PlainThreadBlocking,
        /// `shutdown_blocking` inside a task on another runtime.
        OtherRuntimeBlocking,
        /// `shutdown().await` inside a task on another runtime (napi).
        OtherRuntimeAsync,
    }

    #[derive(Clone, Copy, Debug)]
    enum DropOn {
        PlainThread,
        OtherRuntimeTask,
        OwnRuntimeTask,
        OwnRuntimeBlocking,
    }

    fn shutdown(core: &Core, on: ShutdownOn, other: &tokio::runtime::Runtime) -> bool {
        match on {
            ShutdownOn::PlainThreadBlocking => {
                let c = core.clone();
                std::thread::spawn(move || c.shutdown_blocking(TIMEOUT))
                    .join()
                    .expect("shutdown thread")
            }
            ShutdownOn::OtherRuntimeBlocking => {
                let c = core.clone();
                other
                    .block_on(other.spawn(async move { c.shutdown_blocking(TIMEOUT) }))
                    .expect("shutdown task")
            }
            ShutdownOn::OtherRuntimeAsync => {
                let c = core.clone();
                other
                    .block_on(other.spawn(async move {
                        tokio::time::timeout(TIMEOUT, c.shutdown()).await.is_ok()
                    }))
                    .expect("shutdown task")
            }
        }
    }

    /// Drop `core` (the last handle) in `on`; waits until the drop returned.
    fn drop_last(core: Core, on: DropOn, other: &tokio::runtime::Runtime) {
        let (done_tx, done_rx) = mpsc::channel::<()>();
        match on {
            DropOn::PlainThread => {
                std::thread::spawn(move || {
                    drop(core);
                    let _ = done_tx.send(());
                });
            }
            DropOn::OtherRuntimeTask => {
                other.spawn(async move {
                    drop(core);
                    let _ = done_tx.send(());
                });
            }
            DropOn::OwnRuntimeTask => {
                let handle = core.runtime().clone();
                handle.spawn(async move {
                    drop(core);
                    let _ = done_tx.send(());
                });
            }
            DropOn::OwnRuntimeBlocking => {
                let handle = core.runtime().clone();
                handle.spawn_blocking(move || {
                    drop(core);
                    let _ = done_tx.send(());
                });
            }
        }
        done_rx
            .recv_timeout(TIMEOUT)
            .unwrap_or_else(|_| panic!("dropping the last handle ({on:?}) never returned"));
    }

    #[test]
    fn last_handle_dropped_anywhere_after_shutdown_from_anywhere() {
        count_panics();
        let other = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let shutdowns = [
            ShutdownOn::PlainThreadBlocking,
            ShutdownOn::OtherRuntimeBlocking,
            ShutdownOn::OtherRuntimeAsync,
        ];
        let drops = [
            DropOn::PlainThread,
            DropOn::OtherRuntimeTask,
            DropOn::OwnRuntimeTask,
            DropOn::OwnRuntimeBlocking,
        ];
        let mut round = 0u32;
        for _ in 0..3 {
            for &s in &shutdowns {
                for &d in &drops {
                    round += 1;
                    let dir = tempfile::tempdir().unwrap();
                    let want = f64::from(round % 90 + 5) / 100.0;
                    let core = Core::new(config(dir.path())).expect("core");
                    for i in 0..20 {
                        core.dispatch(Command::SetVolume {
                            volume: f64::from(i) / 100.0,
                        })
                        .unwrap();
                    }
                    core.dispatch(Command::SetVolume { volume: want }).unwrap();
                    assert!(shutdown(&core, s, &other), "{s:?}: flush timed out");
                    // A second handle held elsewhere goes first, so `core`
                    // really is the last one.
                    let spare = core.clone();
                    std::thread::spawn(move || drop(spare)).join().unwrap();
                    drop_last(core, d, &other);
                    assert_eq!(PANICS.load(Ordering::SeqCst), 0, "{s:?} / {d:?}");

                    // The work dispatched before shutdown reached the disk.
                    let again = Core::new(config(dir.path())).expect("reopen");
                    assert_eq!(volume(&again), want, "{s:?} / {d:?}");
                    assert!(again.shutdown_blocking(TIMEOUT));
                    drop(again);
                }
            }
        }
        drop(other);
        assert_eq!(PANICS.load(Ordering::SeqCst), 0);
    }

    /// No shutdown at all: the last handle still goes away cleanly from
    /// inside the core's own runtime or another one.
    #[test]
    fn last_handle_dropped_in_a_runtime_without_shutdown() {
        count_panics();
        let other = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for d in [
            DropOn::OwnRuntimeTask,
            DropOn::OwnRuntimeBlocking,
            DropOn::OtherRuntimeTask,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let core = Core::new(config(dir.path())).expect("core");
            core.dispatch(Command::SetVolume { volume: 0.3 }).unwrap();
            if matches!(d, DropOn::OtherRuntimeTask) {
                let (tx, rx) = mpsc::channel();
                other.block_on(async move {
                    tokio::spawn(async move {
                        drop(core);
                        let _ = tx.send(());
                    })
                    .await
                    .unwrap();
                });
                rx.recv_timeout(TIMEOUT).expect("dropped");
            } else {
                drop_last(core, d, &other);
            }
            assert_eq!(PANICS.load(Ordering::SeqCst), 0, "{d:?}");
        }
    }

    /// `shutdown_blocking` followed at once by dropping the caller's handle:
    /// the task it spawned on the core's runtime must not be left holding
    /// the last one.
    #[test]
    fn shutdown_blocking_then_immediate_drop() {
        count_panics();
        for _ in 0..30 {
            let dir = tempfile::tempdir().unwrap();
            let core = Core::new(config(dir.path())).expect("core");
            core.dispatch(Command::SetVolume { volume: 0.4 }).unwrap();
            let t = std::thread::spawn(move || {
                let ok = core.shutdown_blocking(TIMEOUT);
                drop(core);
                ok
            });
            assert!(t.join().expect("no panic"));
            // Give a straggling task on the (now shut down) runtime time to
            // run its drop.
            std::thread::sleep(Duration::from_millis(5));
            assert_eq!(PANICS.load(Ordering::SeqCst), 0);
        }
    }
}
