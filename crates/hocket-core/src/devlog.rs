//! Log capture for developer mode and diagnostics bundles.
//!
//! [`install`] sets the process's `tracing` subscriber: a reloadable filter, the usual stderr
//! output, and a capture layer that keeps the last hour of formatted lines in memory. Every
//! captured line is also appended to a small rotating file (`hocket.log`, `hocket.log.1`) by a
//! background thread, and the next [`install`] seeds the buffer from it, so a crash or a restart
//! doesn't lose what led up to it. Nothing here touches the network.
//!
//! The platform layers add their own lines (Kotlin, Electron main and renderer) with [`append`]
//! and read the window back with [`recent`]. [`set_detailed`] switches between the base filter and
//! debug for everything ours; the actor calls it from the `developer.mode` setting.
//!
//! Everything is process-global (one subscriber per process). Before [`install`], or when another
//! subscriber already owns the process (tests), the functions are harmless no-ops.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

/// How far back [`recent`] reaches by default, and how long lines are kept.
pub const WINDOW_MS: u64 = 60 * 60 * 1000;
/// Memory bound for the buffer, whatever the window says (debug logging during a big sync is
/// chatty).
const RING_MAX_BYTES: usize = 32 << 20;
/// The live file is rotated to `hocket.log.1` past this size.
const FILE_MAX_BYTES: u64 = 8 << 20;
const FILE_NAME: &str = "hocket.log";
const OLD_FILE_NAME: &str = "hocket.log.1";
/// The filter while developer mode is on: debug for us, info for the chatty dependencies.
const DETAILED_FILTER: &str = "debug,hyper=info,hyper_util=info,reqwest=info,rustls=info,h2=info,\
tungstenite=info,tokio_tungstenite=info,mdns_sd=info,symphonia=info,symphonia_core=info,\
symphonia_bundle_mp3=info,symphonia_format_isomp4=info,cpal=info,zbus=info,polling=info,async_io=info";

static CAPTURE: OnceLock<Capture> = OnceLock::new();
static FILTER: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();

/// Install the process's subscriber (once; later calls only return). `base_filter` is an
/// `EnvFilter` directive such as `info`; `log_dir`, when given, holds the rotating file; `ansi`
/// colours the stderr output.
pub fn install(base_filter: &str, log_dir: Option<&Path>, ansi: bool) {
    if CAPTURE.get().is_some() {
        return;
    }
    let base = if base_filter.trim().is_empty() {
        "info".to_string()
    } else {
        base_filter.to_string()
    };
    let now = now_ms();
    let mut capture = Capture::new(base.clone());
    if let Some(dir) = log_dir {
        capture.seed(&read_previous(dir), now);
        capture.file = spawn_writer(dir.to_path_buf());
    }
    if CAPTURE.set(capture).is_err() {
        return;
    }
    let filter = EnvFilter::try_new(&base).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter, handle) = reload::Layer::new(filter);
    let installed = tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(ansi).with_writer(std::io::stderr))
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_thread_names(true)
                .with_writer(MakeCapture),
        )
        .try_init()
        .is_ok();
    if installed {
        let _ = FILTER.set(handle);
        install_panic_hook();
    }
    marker(&format!(
        "log capture started (pid {}, core {})",
        std::process::id(),
        env!("CARGO_PKG_VERSION")
    ));
}

/// Debug logging for everything ours (developer mode) or back to the base filter. Idempotent.
pub fn set_detailed(on: bool) {
    let Some(c) = CAPTURE.get() else { return };
    if c.detailed.swap(on, Ordering::SeqCst) == on {
        return;
    }
    if let Some(h) = FILTER.get() {
        let directive = if on { DETAILED_FILTER } else { c.base.as_str() };
        if let Ok(f) = EnvFilter::try_new(directive) {
            let _ = h.reload(f);
        }
    }
    marker(if on {
        "developer mode on: detailed logging"
    } else {
        "developer mode off: normal logging"
    });
}

/// Whether [`set_detailed`] last turned detailed logging on.
pub fn is_detailed() -> bool {
    CAPTURE
        .get()
        .is_some_and(|c| c.detailed.load(Ordering::SeqCst))
}

/// Add a platform line (`tag` names its source, e.g. `kotlin/PlaybackService` or `renderer`).
/// `debug` and `trace` lines are dropped unless detailed logging is on. They go to the buffer and
/// the file only: the platform already printed them to its own console.
pub fn append(level: &str, tag: &str, message: &str) {
    let Some(c) = CAPTURE.get() else { return };
    let level = Level::parse(level);
    if level.is_verbose() && !c.detailed.load(Ordering::SeqCst) {
        return;
    }
    let now = now_ms();
    c.push(format_line(now, level.as_str(), tag, message), now);
}

/// The captured lines of the last `window_ms` (oldest first), as text.
pub fn recent(window_ms: u64) -> String {
    CAPTURE
        .get()
        .map(|c| c.recent(window_ms, now_ms()))
        .unwrap_or_default()
}

/// Wait (up to `timeout`) until every line captured so far is on disk. For crash paths: a
/// platform's uncaught-exception handler calls it before letting the process die.
pub fn flush(timeout: Duration) {
    let Some(tx) = CAPTURE.get().and_then(|c| c.file.as_ref()) else {
        return;
    };
    let (done_tx, done_rx) = mpsc::channel();
    if tx.send(FileMsg::Flush(done_tx)).is_ok() {
        let _ = done_rx.recv_timeout(timeout);
    }
}

fn marker(message: &str) {
    if let Some(c) = CAPTURE.get() {
        let now = now_ms();
        c.push(format_line(now, "INFO", "hocket", message), now);
    }
}

/// Panics go to the capture (and to disk) before the previous hook runs, so a crash on a worker
/// thread is in the next bundle.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(c) = CAPTURE.get() {
            let now = now_ms();
            let thread = std::thread::current();
            let message = format!(
                "panic on thread '{}': {info}",
                thread.name().unwrap_or("<unnamed>")
            );
            c.push(format_line(now, "ERROR", "panic", &message), now);
            flush(Duration::from_millis(500));
        }
        previous(info);
    }));
}

// -- the buffer ----------------------------------------------------------------------------------

struct Capture {
    ring: Mutex<Ring>,
    file: Option<mpsc::Sender<FileMsg>>,
    base: String,
    detailed: AtomicBool,
}

#[derive(Default)]
struct Ring {
    /// `(captured at, line)`, oldest first; each line ends in `\n`.
    lines: VecDeque<(u64, String)>,
    bytes: usize,
}

impl Capture {
    fn new(base: String) -> Self {
        Capture {
            ring: Mutex::new(Ring::default()),
            file: None,
            base,
            detailed: AtomicBool::new(false),
        }
    }

    fn push(&self, mut line: String, now: u64) {
        if !line.ends_with('\n') {
            line.push('\n');
        }
        if let Some(tx) = &self.file {
            let _ = tx.send(FileMsg::Line(line.clone()));
        }
        let mut ring = self.ring.lock();
        ring.bytes += line.len();
        ring.lines.push_back((now, line));
        ring.trim(now);
    }

    /// Lines from a previous run's file, keeping those inside the window.
    fn seed(&self, text: &str, now: u64) {
        let mut guard = self.ring.lock();
        let ring = &mut *guard;
        for raw in text.split_inclusive('\n') {
            match parse_timestamp(raw) {
                Some(ts) => {
                    ring.bytes += raw.len();
                    ring.lines.push_back((ts, raw.to_string()));
                }
                // A continuation line (a stack trace) joins the entry before it.
                None => {
                    if let Some((_, prev)) = ring.lines.back_mut() {
                        prev.push_str(raw);
                        ring.bytes += raw.len();
                    }
                }
            }
        }
        if let Some((_, last)) = ring.lines.back_mut().filter(|(_, l)| !l.ends_with('\n')) {
            last.push('\n');
            ring.bytes += 1;
        }
        ring.trim(now);
    }

    fn recent(&self, window_ms: u64, now: u64) -> String {
        let since = now.saturating_sub(window_ms);
        let ring = self.ring.lock();
        let mut out = String::new();
        for (_, line) in ring.lines.iter().filter(|(ts, _)| *ts >= since) {
            out.push_str(line);
        }
        out
    }
}

impl Ring {
    fn trim(&mut self, now: u64) {
        let since = now.saturating_sub(WINDOW_MS);
        while let Some((ts, line)) = self.lines.front() {
            if *ts >= since && self.bytes <= RING_MAX_BYTES {
                break;
            }
            self.bytes -= line.len();
            self.lines.pop_front();
        }
    }
}

/// The capture layer's writer: `fmt` formats one event and writes it whole, so each writer is
/// one line, pushed when it drops.
struct MakeCapture;

struct LineWriter(Vec<u8>);

impl<'a> fmt::MakeWriter<'a> for MakeCapture {
    type Writer = LineWriter;
    fn make_writer(&'a self) -> LineWriter {
        LineWriter(Vec::with_capacity(256))
    }
}

impl Write for LineWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for LineWriter {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        if let Some(c) = CAPTURE.get() {
            c.push(String::from_utf8_lossy(&self.0).into_owned(), now_ms());
        }
    }
}

// -- the file ------------------------------------------------------------------------------------

enum FileMsg {
    Line(String),
    Flush(mpsc::Sender<()>),
}

fn read_previous(dir: &Path) -> String {
    let mut text = String::new();
    for name in [OLD_FILE_NAME, FILE_NAME] {
        if let Ok(f) = File::open(dir.join(name)) {
            for line in BufReader::new(f).lines().map_while(Result::ok) {
                text.push_str(&line);
                text.push('\n');
            }
        }
    }
    text
}

/// The writer thread: appends lines, flushes whenever the queue runs dry, rotates past
/// [`FILE_MAX_BYTES`]. I/O errors are swallowed (logging them would loop back here).
fn spawn_writer(dir: PathBuf) -> Option<mpsc::Sender<FileMsg>> {
    fs::create_dir_all(&dir).ok()?;
    let path = dir.join(FILE_NAME);
    let open = |p: &Path| OpenOptions::new().create(true).append(true).open(p).ok();
    let file = open(&path)?;
    let mut size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut out = BufWriter::new(file);
    let (tx, rx) = mpsc::channel::<FileMsg>();
    std::thread::Builder::new()
        .name("hocket-log".into())
        .spawn(move || {
            let mut pending_flush: Vec<mpsc::Sender<()>> = Vec::new();
            while let Ok(first) = rx.recv() {
                let mut next = Some(first);
                while let Some(msg) = next.take() {
                    match msg {
                        FileMsg::Line(line) => {
                            let _ = out.write_all(line.as_bytes());
                            size += line.len() as u64;
                        }
                        FileMsg::Flush(done) => pending_flush.push(done),
                    }
                    if size > FILE_MAX_BYTES {
                        let _ = out.flush();
                        let _ = fs::rename(&path, dir.join(OLD_FILE_NAME));
                        if let Some(f) = open(&path) {
                            out = BufWriter::new(f);
                        }
                        size = 0;
                    }
                    next = rx.try_recv().ok();
                }
                let _ = out.flush();
                for done in pending_flush.drain(..) {
                    let _ = done.send(());
                }
            }
        })
        .ok()?;
    Some(tx)
}

// -- formatting ----------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Debug)]
enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    fn parse(s: &str) -> Level {
        match s.trim().to_ascii_lowercase().as_str() {
            "trace" | "verbose" | "v" => Level::Trace,
            "debug" | "d" => Level::Debug,
            "warn" | "warning" | "w" => Level::Warn,
            "error" | "e" | "assert" | "fatal" => Level::Error,
            _ => Level::Info,
        }
    }
    fn is_verbose(self) -> bool {
        matches!(self, Level::Trace | Level::Debug)
    }
    fn as_str(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A line in the same shape as `tracing_subscriber::fmt`'s (`<rfc3339> <LEVEL> <target>: <msg>`).
fn format_line(ms: u64, level: &str, tag: &str, message: &str) -> String {
    format!("{} {level:>5} {tag}: {message}\n", format_timestamp(ms))
}

/// `2026-10-20T03:04:05.678Z` (UTC).
pub fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60,
        ms % 1000
    )
}

/// The epoch milliseconds of a line's leading `YYYY-MM-DDTHH:MM:SS[.fff…]Z` timestamp.
fn parse_timestamp(line: &str) -> Option<u64> {
    let b = line.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| line.get(r)?.parse::<u64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut ms = 0;
    if b[19] == b'.' {
        let digits: String = line[20..]
            .chars()
            .take_while(char::is_ascii_digit)
            .take(3)
            .collect();
        ms = format!("{digits:0<3}").parse::<u64>().ok()?;
    }
    let days = days_from_civil(y as i64, mo as u32, d as u32);
    if days < 0 {
        return None;
    }
    Some(((days as u64 * 86_400) + h * 3600 + mi * 60 + s) * 1000 + ms)
}

// Howard Hinnant's civil-date algorithms.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_792_000_000_000; // 2026-10-15

    #[test]
    fn timestamps_round_trip() {
        for ms in [0, 951_782_400_123, T0, T0 + 86_399_999] {
            let s = format_timestamp(ms);
            assert_eq!(parse_timestamp(&s), Some(ms), "{s}");
        }
        assert_eq!(
            format_timestamp(951_782_400_123),
            "2000-02-29T00:00:00.123Z"
        );
        // tracing_subscriber::fmt's own shape: microseconds.
        assert_eq!(
            parse_timestamp("2000-02-29T00:00:00.123456Z  INFO x: y"),
            Some(951_782_400_123)
        );
        assert_eq!(parse_timestamp("    at Foo.bar(Foo.kt:3)"), None);
        assert_eq!(parse_timestamp("2000-13-29T00:00:00Z"), None);
    }

    #[test]
    fn ring_keeps_the_window_and_the_byte_bound() {
        let c = Capture::new("info".into());
        c.push("old".into(), T0);
        c.push("new".into(), T0 + WINDOW_MS);
        assert_eq!(c.recent(WINDOW_MS, T0 + WINDOW_MS), "old\nnew\n");
        c.push("newer".into(), T0 + WINDOW_MS + 1);
        assert_eq!(c.recent(WINDOW_MS, T0 + WINDOW_MS + 1), "new\nnewer\n");
        assert_eq!(c.recent(0, T0 + WINDOW_MS + 1), "newer\n");

        let big = "x".repeat(RING_MAX_BYTES / 2);
        for i in 0..3 {
            c.push(big.clone(), T0 + WINDOW_MS + 2 + i);
        }
        let ring = c.ring.lock();
        assert!(ring.bytes <= RING_MAX_BYTES);
        assert_eq!(
            ring.bytes,
            ring.lines.iter().map(|(_, l)| l.len()).sum::<usize>()
        );
    }

    #[test]
    fn seeding_keeps_recent_lines_and_joins_continuations() {
        let c = Capture::new("info".into());
        let old = format_line(T0, "INFO", "a", "too old");
        let kept = format_line(T0 + WINDOW_MS, "ERROR", "kotlin", "boom");
        let text = format!("{old}{kept}    at Foo.bar(Foo.kt:3)\nno newline at the end");
        c.seed(&text, T0 + WINDOW_MS + 10);
        let out = c.recent(WINDOW_MS, T0 + WINDOW_MS + 10);
        assert!(!out.contains("too old"));
        assert!(out.starts_with(&kept));
        assert!(out.ends_with("    at Foo.bar(Foo.kt:3)\nno newline at the end\n"));
        assert_eq!(c.ring.lock().lines.len(), 1);
    }

    #[test]
    fn levels_parse_loosely() {
        assert_eq!(Level::parse("W"), Level::Warn);
        assert_eq!(Level::parse("verbose"), Level::Trace);
        assert_eq!(Level::parse("Error"), Level::Error);
        assert_eq!(Level::parse("whatever"), Level::Info);
        assert!(Level::parse("debug").is_verbose());
        assert_eq!(
            format_line(951_782_400_123, "INFO", "renderer", "hi"),
            "2000-02-29T00:00:00.123Z  INFO renderer: hi\n"
        );
    }

    #[test]
    fn writer_appends_flushes_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let tx = spawn_writer(dir.path().to_path_buf()).unwrap();
        tx.send(FileMsg::Line(format_line(T0, "INFO", "t", "one")))
            .unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        tx.send(FileMsg::Flush(done_tx)).unwrap();
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let text = read_previous(dir.path());
        assert!(text.ends_with("t: one\n"), "{text}");
        let c = Capture::new("info".into());
        c.seed(&text, T0 + 1);
        assert!(c.recent(WINDOW_MS, T0 + 1).contains("t: one"));
    }
}
