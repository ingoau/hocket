//! Deterministic harness for driving full cores: a virtual clock, the
//! scripted backend, a fake Subsonic server, a temp database and, for
//! several cores, an in-memory Connect network. Feature `sim` (always in
//! tests).

use std::sync::Arc;

use parking_lot::Mutex;

use crate::api::*;
use crate::audio::backend::PlaybackBackend;
use crate::audio::scripted::ScriptedBackend;
use crate::lyrics::LyricsHttp;
use crate::sim::SimTime;
use crate::subsonic::fake::FakeServer;
use crate::subsonic::SubsonicApi;
use crate::util::Clock;

use super::io::memory::MemoryNet;
use super::io::ConnectIo;
use super::{BackendChoice, Core, CoreError, Deps, EventSink, LateSink, PollFn, PresetServer};

/// A backend built by the test, with its sink wired into the core later.
pub struct TestBackend {
    backend: Arc<dyn PlaybackBackend>,
    sink: LateSink,
    poll: Option<PollFn>,
}

impl TestBackend {
    /// A [`ScriptedBackend`] on the given clock, polled on every tick.
    pub fn scripted(clock: Arc<dyn Clock>) -> (TestBackend, Arc<ScriptedBackend>) {
        let sink = LateSink::new();
        let backend = Arc::new(ScriptedBackend::new(clock, sink.sink()));
        let poll_backend = backend.clone();
        let poll: PollFn = Arc::new(move || poll_backend.poll());
        (
            TestBackend {
                backend: backend.clone(),
                sink,
                poll: Some(poll),
            },
            backend,
        )
    }

    /// Any backend; `sink` must be the one it was built with.
    pub fn custom(backend: Arc<dyn PlaybackBackend>, sink: LateSink) -> TestBackend {
        TestBackend {
            backend,
            sink,
            poll: None,
        }
    }
}

/// Everything a test can inject.
pub struct TestOptions {
    pub backend: TestBackend,
    pub api: Arc<dyn SubsonicApi>,
    /// Server metadata (the platform would pass these with `AddServer`).
    pub server_url: String,
    pub password: String,
    /// In-memory Connect network shared between cores; `None` = no network.
    pub net: Option<Arc<MemoryNet>>,
    pub lyrics_http: Option<Arc<dyn LyricsHttp>>,
    /// Session entropy seed (queue keys, shuffle seeds).
    pub seed: u64,
}

impl Core {
    /// A core driven entirely from the test: `clock` is read for every
    /// timestamp, `backend` reports on `Core::tick`, `api` stands in for the
    /// server. No wall-clock ticker runs; call [`Core::tick`] after advancing
    /// the clock and [`Core::settle`] to wait for the actor and its tasks.
    pub fn new_for_test(
        config: CoreConfig,
        clock: Arc<dyn Clock>,
        backend: TestBackend,
        api: Arc<dyn SubsonicApi>,
    ) -> Result<Core, CoreError> {
        Self::new_for_test_with(
            config,
            clock,
            TestOptions {
                backend,
                api,
                server_url: "https://music.example/".into(),
                password: "secret".into(),
                net: None,
                lyrics_http: None,
                seed: 1,
            },
        )
    }

    pub fn new_for_test_with(
        config: CoreConfig,
        clock: Arc<dyn Clock>,
        opts: TestOptions,
    ) -> Result<Core, CoreError> {
        let io: Arc<dyn ConnectIo> = match &opts.net {
            Some(net) => net.io(&config.device_id, true),
            None => Arc::new(NoNet),
        };
        let username = opts.api.username().unwrap_or_else(|| "user".into());
        let deps = Deps {
            clock,
            backend: BackendChoice::Provided {
                backend: opts.backend.backend,
                sink: opts.backend.sink,
                poll: opts.backend.poll,
            },
            io,
            storage: Arc::new(crate::downloads::UnknownStorage),
            lyrics_http: opts.lyrics_http,
            server: Some(PresetServer {
                api: opts.api,
                url: opts.server_url,
                username,
                password: opts.password,
            }),
            seed: Some(opts.seed),
            manual_tick: true,
        };
        let handle = tokio::runtime::Handle::current();
        Core::with_runtime(config, handle, None, deps)
    }
}

/// I/O that never connects anywhere (single-core tests).
struct NoNet;

impl ConnectIo for NoNet {
    fn connect(
        &self,
        _peer: crate::connect::PeerId,
        _candidates: Vec<String>,
    ) -> futures::future::BoxFuture<
        'static,
        Result<crate::connect::transport::Connection, crate::connect::transport::TransportError>,
    > {
        Box::pin(async { Err(crate::connect::transport::TransportError::NoCandidate) })
    }
    fn listen(
        &self,
        _ids: Arc<crate::connect::transport::PeerIds>,
    ) -> futures::future::BoxFuture<
        'static,
        Result<Box<dyn super::io::Listener>, crate::connect::transport::TransportError>,
    > {
        Box::pin(async {
            Err(crate::connect::transport::TransportError::Bind {
                addr: "none".into(),
                error: "no network in this test".into(),
            })
        })
    }
    fn discovery(&self) -> Box<dyn crate::connect::discovery::Discovery> {
        Box::new(crate::connect::discovery::NoDiscovery)
    }
    fn verify(
        &self,
        _credential: crate::connect::wire::Credential,
    ) -> futures::future::BoxFuture<'static, bool> {
        Box::pin(async { true })
    }
}

/// Records every event for assertions.
#[derive(Default)]
pub struct EventLog {
    events: Mutex<Vec<Event>>,
}

impl EventSink for EventLog {
    fn on_event(&self, event: Event) {
        self.events.lock().push(event);
    }
}

impl EventLog {
    pub fn all(&self) -> Vec<Event> {
        self.events.lock().clone()
    }
    pub fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock())
    }
    pub fn clear(&self) {
        self.events.lock().clear();
    }
}

/// A full core with everything a test needs to drive and observe it.
pub struct TestCore {
    pub core: Core,
    pub backend: Arc<ScriptedBackend>,
    pub clock: Arc<SimTime>,
    pub server: FakeServer,
    pub events: Arc<EventLog>,
    pub device_id: String,
    pub server_id: String,
    pub dir: tempfile::TempDir,
}

impl TestCore {
    /// One core on its own (no Connect network), started.
    pub async fn start(name: &str, server: FakeServer) -> TestCore {
        Self::start_with(name, server, None, 1).await
    }

    pub async fn start_with(
        name: &str,
        server: FakeServer,
        net: Option<Arc<MemoryNet>>,
        seed: u64,
    ) -> TestCore {
        let clock = SimTime::new(1_700_000_000_000.0);
        Self::start_on(name, server, net, seed, clock).await
    }

    /// Several cores share one clock (and network).
    pub async fn start_on(
        name: &str,
        server: FakeServer,
        net: Option<Arc<MemoryNet>>,
        seed: u64,
        clock: Arc<SimTime>,
    ) -> TestCore {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = CoreConfig {
            data_dir: dir.path().join("data").to_string_lossy().into_owned(),
            cache_dir: dir.path().join("cache").to_string_lossy().into_owned(),
            device_id: format!("dev-{name}"),
            device_name: format!("Device {name}"),
            platform: Platform::Linux,
            app_version: "test".into(),
            audio: AudioMode::None,
            coordinator_listen: None,
        };
        let (backend, scripted) = TestBackend::scripted(clock.clone());
        let server_id = server.server_id().to_string();
        let core = Core::new_for_test_with(
            config,
            clock.clone(),
            TestOptions {
                backend,
                api: Arc::new(server.clone()),
                server_url: "https://music.example/".into(),
                password: "secret".into(),
                net,
                lyrics_http: None,
                seed,
            },
        )
        .expect("core");
        let events = Arc::new(EventLog::default());
        core.add_sink(events.clone());
        core.dispatch(Command::Start).expect("start");
        core.settle().await;
        TestCore {
            core,
            backend: scripted,
            clock,
            server,
            events,
            device_id: format!("dev-{name}"),
            server_id,
            dir,
        }
    }

    pub fn dispatch(&self, cmd: Command) {
        self.core.dispatch(cmd).expect("dispatch");
    }

    /// Dispatch and wait for the actor to go quiet.
    pub async fn run(&self, cmd: Command) {
        self.dispatch(cmd);
        self.core.settle().await;
    }

    pub async fn query(&self, q: Query) -> QueryResult {
        self.core.query(q).await.expect("query")
    }

    /// Advance virtual time in 250 ms steps, ticking and settling each step.
    pub async fn run_for(&self, ms: f64) {
        let steps = (ms / 250.0).ceil().max(1.0) as u32;
        for _ in 0..steps {
            self.clock.advance(250.0);
            self.core.tick().expect("tick");
            self.core.settle().await;
        }
    }

    /// Advance several cores on a shared clock together.
    pub async fn run_all_for(cores: &[&TestCore], ms: f64) {
        let Some(first) = cores.first() else { return };
        let steps = (ms / 250.0).ceil().max(1.0) as u32;
        for _ in 0..steps {
            first.clock.advance(250.0);
            for c in cores {
                c.core.tick().expect("tick");
            }
            for _ in 0..3 {
                for c in cores {
                    c.core.settle().await;
                }
            }
        }
    }

    pub async fn snapshot(&self) -> Snapshot {
        match self.query(Query::Snapshot).await {
            QueryResult::SnapshotResult(s) => s,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub async fn queue(&self) -> QueueView {
        match self.query(Query::Queue).await {
            QueryResult::Queue(q) => q,
            other => panic!("unexpected {other:?}"),
        }
    }

    pub async fn transport(&self) -> TransportState {
        self.snapshot().await.transport
    }

    pub async fn current_track_id(&self) -> Option<String> {
        self.queue().await.current.map(|e| e.track.id)
    }

    /// Wait until `f` holds, ticking virtual time (fails after `max_ms`).
    pub async fn wait_until(&self, max_ms: f64, mut f: impl FnMut(&TestCore) -> bool) {
        let mut elapsed = 0.0;
        while !f(self) {
            if elapsed >= max_ms {
                panic!("condition not met within {max_ms} ms");
            }
            self.run_for(250.0).await;
            elapsed += 250.0;
        }
    }
}

/// A fake server with `n` songs of `duration_s` seconds each, in two
/// albums by one artist.
pub fn seeded_server(n: usize, duration_s: f64) -> FakeServer {
    let s = FakeServer::new("srv", "alice");
    for i in 0..n {
        let mut c = FakeServer::song(
            &format!("t{i}"),
            &format!("Track {i}"),
            &format!("al{}", i / 4),
            "ar0",
            duration_s,
        );
        c.track = Some(i as u32 + 1);
        s.add_song(c);
    }
    s
}
