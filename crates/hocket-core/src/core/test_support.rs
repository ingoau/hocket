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
use super::stream_reader::{StreamUpstream, UpstreamRequest, UpstreamResponse};
use super::io::ConnectIo;
use super::{BackendChoice, Core, CoreError, Deps, EventSink, LateSink, PollFn, PresetServer};

pub use super::handlers::servers::server_id_for;

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
    /// `None` starts with no server (the platform adds one with `AddServer`).
    pub api: Option<Arc<dyn SubsonicApi>>,
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
                api: Some(api),
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
        Self::new_for_test_upstream(config, clock, opts, Arc::new(FakeUpstream::new()))
    }

    /// [`Core::new_for_test_with`] with the server side of the stream
    /// reader supplied by the test.
    pub fn new_for_test_upstream(
        config: CoreConfig,
        clock: Arc<dyn Clock>,
        opts: TestOptions,
        upstream: Arc<dyn StreamUpstream>,
    ) -> Result<Core, CoreError> {
        let io: Arc<dyn ConnectIo> = match &opts.net {
            Some(net) => net.io(&config.device_id, true),
            None => Arc::new(NoNet),
        };
        let server = opts.api.map(|api| {
            let username = api.username().unwrap_or_else(|| "user".into());
            PresetServer {
                api,
                url: opts.server_url,
                username,
                password: opts.password,
            }
        });
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
            server,
            seed: Some(opts.seed),
            manual_tick: true,
            stream_upstream: Some(upstream),
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
    /// Every source the backend was handed (`load`, its `next`, `set_next`,
    /// `pre_buffer`), in order.
    pub sources: Arc<Mutex<Vec<MediaSource>>>,
    /// What the stream reader fetches from "the server".
    pub upstream: Arc<FakeUpstream>,
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
        Self::start_in(name, server, net, seed, clock, dir).await
    }

    /// A core on an existing data directory (restart scenarios): the
    /// database, persisted session and position are reused.
    pub async fn start_in(
        name: &str,
        server: FakeServer,
        net: Option<Arc<MemoryNet>>,
        seed: u64,
        clock: Arc<SimTime>,
        dir: tempfile::TempDir,
    ) -> TestCore {
        Self::start_in_with(
            name,
            server,
            net,
            seed,
            clock,
            dir,
            true,
            "https://music.example/",
            "secret",
        )
        .await
    }

    /// Like [`TestCore::start_in`]; `attach_api == false` starts the core the
    /// way a platform does, with the persisted server row but no API until
    /// `Command::AddServer` supplies credentials. `server_url` is what the
    /// preset server (and `AddServer`) are keyed on; `password` is the
    /// account password the LAN key is derived from.
    #[allow(clippy::too_many_arguments)]
    pub async fn start_in_with(
        name: &str,
        server: FakeServer,
        net: Option<Arc<MemoryNet>>,
        seed: u64,
        clock: Arc<SimTime>,
        dir: tempfile::TempDir,
        attach_api: bool,
        server_url: &str,
        password: &str,
    ) -> TestCore {
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
        let (mut backend, scripted) = TestBackend::scripted(clock.clone());
        let sources: Arc<Mutex<Vec<MediaSource>>> = Arc::default();
        backend.backend = Arc::new(RecordingBackend {
            inner: backend.backend.clone(),
            sources: sources.clone(),
        });
        let upstream = Arc::new(FakeUpstream::new());
        let server_id = server.server_id().to_string();
        let core = Core::new_for_test_upstream(
            config,
            clock.clone(),
            TestOptions {
                backend,
                api: attach_api.then(|| Arc::new(server.clone()) as Arc<dyn SubsonicApi>),
                server_url: server_url.into(),
                password: password.into(),
                net,
                lyrics_http: None,
                seed,
            },
            upstream.clone(),
        )
        .expect("core");
        let events = Arc::new(EventLog::default());
        core.add_sink(events.clone());
        core.dispatch(Command::Start).expect("start");
        core.settle().await;
        TestCore {
            core,
            backend: scripted,
            sources,
            upstream,
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

    /// Shut the core down (awaiting the flush) and start a fresh one on the
    /// same data directory, clock and fake server.
    pub async fn restart(self, name: &str) -> TestCore {
        self.core.shutdown().await;
        let TestCore {
            clock, server, dir, ..
        } = self;
        Self::start_in(name, server, None, 1, clock, dir).await
    }

    /// Dispatch and wait for the actor to go quiet.
    pub async fn run(&self, cmd: Command) {
        self.dispatch(cmd);
        self.core.settle().await;
    }

    pub async fn query(&self, q: Query) -> QueryResult {
        self.core.query(q).await.expect("query")
    }

    /// The data directory (for tests that inspect or tamper with the
    /// database between a shutdown and a restart).
    pub fn data_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("data")
    }

    /// Open the core's database directly (between a shutdown and a restart,
    /// or to inspect rows while it runs).
    pub fn open_db(&self) -> crate::db::Db {
        let data = self.data_dir();
        crate::db::Db::open(&data.join("hocket.sqlite"), &data.join("backups")).expect("db")
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
    seeded_server_with_id("srv", n, duration_s)
}

/// [`seeded_server`] under a chosen server id (e.g. the id `AddServer`
/// derives for a URL and username).
pub fn seeded_server_with_id(server_id: &str, n: usize, duration_s: f64) -> FakeServer {
    let s = FakeServer::new(server_id, "alice");
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

/// Passes every call to `inner`, recording the sources it hands over.
struct RecordingBackend {
    inner: Arc<dyn PlaybackBackend>,
    sources: Arc<Mutex<Vec<MediaSource>>>,
}

impl PlaybackBackend for RecordingBackend {
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), crate::audio::backend::BackendError> {
        {
            let mut s = self.sources.lock();
            s.push(source.clone());
            s.extend(next.clone());
        }
        self.inner.load(source, next, position_ms, play)
    }
    fn set_next(
        &self,
        next: Option<MediaSource>,
    ) -> Result<(), crate::audio::backend::BackendError> {
        self.sources.lock().extend(next.clone());
        self.inner.set_next(next)
    }
    fn play(&self) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.play()
    }
    fn pause(&self) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.pause()
    }
    fn stop(&self) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.stop()
    }
    fn seek(&self, position_ms: u32) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.seek(position_ms)
    }
    fn set_volume(&self, volume: f64) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.set_volume(volume)
    }
    fn pre_buffer(
        &self,
        source: MediaSource,
        position_ms: u32,
    ) -> Result<(), crate::audio::backend::BackendError> {
        self.sources.lock().push(source.clone());
        self.inner.pre_buffer(source, position_ms)
    }
    fn discard_pre_buffer(&self) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.discard_pre_buffer()
    }
    fn set_gapless(&self, enabled: bool) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.set_gapless(enabled)
    }
    fn set_output_device(
        &self,
        id: Option<String>,
    ) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.set_output_device(id)
    }
    fn output_devices(&self) -> Vec<OutputDevice> {
        self.inner.output_devices()
    }
    fn set_exclusive(&self, exclusive: bool) -> Result<(), crate::audio::backend::BackendError> {
        self.inner.set_exclusive(exclusive)
    }
    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

/// How [`FakeUpstream`] answers `stream` for one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamBehaviour {
    /// The media, honouring `Range`.
    Serve,
    /// This many bytes, then nothing ever again (a stalled connection).
    StallAfter(usize),
    /// This many bytes, then a connection error.
    FailAfter(usize),
    /// HTTP 200 with a Subsonic JSON error envelope.
    ErrorEnvelope,
}

/// One `stream` request the fake saw.
#[derive(Debug, Clone)]
pub struct UpstreamCall {
    pub track_id: String,
    pub range: Option<String>,
    pub url: url::Url,
}

#[derive(Default)]
struct FakeUpstreamState {
    media: std::collections::HashMap<String, (bytes::Bytes, String)>,
    behaviour: std::collections::HashMap<String, UpstreamBehaviour>,
    calls: Vec<UpstreamCall>,
}

/// In-memory server side of the stream reader: serves registered media by
/// the `id` query parameter (or 64 KiB of deterministic bytes), honours
/// `Range`, counts requests, and can stall, fail or answer with an error
/// envelope per track.
#[derive(Default)]
pub struct FakeUpstream {
    state: Mutex<FakeUpstreamState>,
}

impl FakeUpstream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_media(&self, track_id: &str, bytes: Vec<u8>, content_type: &str) {
        self.state
            .lock()
            .media
            .insert(track_id.into(), (bytes.into(), content_type.into()));
    }

    pub fn set_behaviour(&self, track_id: &str, b: UpstreamBehaviour) {
        self.state.lock().behaviour.insert(track_id.into(), b);
    }

    /// The bytes served for a track.
    pub fn media(&self, track_id: &str) -> bytes::Bytes {
        self.entry(track_id).0
    }

    fn entry(&self, track_id: &str) -> (bytes::Bytes, String) {
        self.state
            .lock()
            .media
            .get(track_id)
            .cloned()
            .unwrap_or_else(|| {
                let seed = format!("audio:{track_id}:");
                let bytes: Vec<u8> = seed.bytes().cycle().take(64 * 1024).collect();
                (bytes.into(), "audio/mpeg".into())
            })
    }

    pub fn calls(&self) -> Vec<UpstreamCall> {
        self.state.lock().calls.clone()
    }

    /// `stream` requests for one track.
    pub fn stream_requests(&self, track_id: &str) -> usize {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|c| c.track_id == track_id)
            .count()
    }
}

impl StreamUpstream for FakeUpstream {
    fn fetch(
        &self,
        request: UpstreamRequest,
    ) -> futures::future::BoxFuture<'static, Result<UpstreamResponse, String>> {
        use futures::StreamExt;
        let track_id = request
            .url
            .query_pairs()
            .find(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned())
            .unwrap_or_default();
        let (media, content_type) = self.entry(&track_id);
        let behaviour = {
            let mut st = self.state.lock();
            st.calls.push(UpstreamCall {
                track_id: track_id.clone(),
                range: request.range.clone(),
                url: request.url.clone(),
            });
            st.behaviour
                .get(&track_id)
                .copied()
                .unwrap_or(UpstreamBehaviour::Serve)
        };
        Box::pin(async move {
            if behaviour == UpstreamBehaviour::ErrorEnvelope {
                let body = br#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":70,"message":"not found"}}}"#;
                return Ok(UpstreamResponse {
                    status: 200,
                    content_type: Some("application/json".into()),
                    content_length: Some(body.len() as u64),
                    content_range: None,
                    body: futures::stream::iter([Ok(bytes::Bytes::from_static(body))]).boxed(),
                });
            }
            let len = media.len() as u64;
            let start = request
                .range
                .as_deref()
                .and_then(|r| r.strip_prefix("bytes="))
                .and_then(|r| r.split_once('-'))
                .and_then(|(a, _)| a.parse::<u64>().ok());
            let (status, from, content_range) = match start {
                Some(s) if s < len => (206, s, Some(format!("bytes {s}-{}/{len}", len - 1))),
                Some(_) => {
                    return Ok(UpstreamResponse {
                        status: 416,
                        content_type: None,
                        content_length: None,
                        content_range: Some(format!("bytes */{len}")),
                        body: futures::stream::empty().boxed(),
                    })
                }
                None => (200, 0, None),
            };
            let slice = media.slice(from as usize..);
            let content_length = Some(slice.len() as u64);
            let chunks: Vec<Result<bytes::Bytes, String>> = slice
                .chunks(16 * 1024)
                .map(|c| Ok(bytes::Bytes::copy_from_slice(c)))
                .collect();
            let body = match behaviour {
                UpstreamBehaviour::StallAfter(n) => {
                    let head: Vec<_> = split_at_bytes(chunks, n);
                    futures::stream::iter(head)
                        .chain(futures::stream::pending())
                        .boxed()
                }
                UpstreamBehaviour::FailAfter(n) => {
                    let head: Vec<_> = split_at_bytes(chunks, n);
                    futures::stream::iter(head)
                        .chain(futures::stream::iter([Err("connection reset".to_string())]))
                        .boxed()
                }
                _ => futures::stream::iter(chunks).boxed(),
            };
            Ok(UpstreamResponse {
                status,
                content_type: Some(content_type),
                content_length,
                content_range,
                body,
            })
        })
    }
}

/// The chunks covering the first `n` bytes (the last one truncated).
fn split_at_bytes(
    chunks: Vec<Result<bytes::Bytes, String>>,
    n: usize,
) -> Vec<Result<bytes::Bytes, String>> {
    let mut left = n;
    let mut out = vec![];
    for c in chunks.into_iter().flatten() {
        if left == 0 {
            break;
        }
        let take = c.len().min(left);
        out.push(Ok(c.slice(..take)));
        left -= take;
    }
    out
}
