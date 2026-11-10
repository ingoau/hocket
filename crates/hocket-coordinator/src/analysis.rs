//! The coordinator's AutoMix analysis store (`docs/automix.md` → *Sharing
//! records*): the preferred [`AnalysisRecord`] per (Navidrome server, track
//! id) in `<data-dir>/automix.sqlite` (in memory without a data dir). Any
//! client verified for a server reads and writes that server's records,
//! whoever its user; the opt-in worker ([`crate::worker`]) writes them too.
//!
//! A record replaces the stored one only when it is preferred
//! ([`preference`]: version, beats, producer, age) or describes another file
//! and is at least as good ([`replaces`]). Uploads are tidied
//! ([`sanitize_upload`]) and validated ([`validate`]) first. The store keeps
//! to its [`StoreLimits`] by dropping the least recently updated records.

use std::collections::HashMap;
use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension};

use hocket_core::api::{AnalysisProducer, AnalysisRecord, SourceFingerprint, TrackId};
use hocket_core::automix::record::{
    compact, fingerprint_matches, has_beats, is_usable, preference, ANALYSIS_VERSION,
};
use hocket_core::connect::wire::AnalysisEntry;

/// The store's file name in `--data-dir`.
pub const STORE_FILE: &str = "automix.sqlite";

/// Upload limits. A five-minute track has ~600 beats and 600 points per
/// curve; these leave room for a three-hour mix at 200 bpm and refuse junk.
pub const MAX_BEATS: usize = 40_000;
pub const MAX_CURVE_POINTS: usize = 2_000;
pub const MAX_DURATION_MS: u32 = 6 * 3_600_000;
pub const MAX_TRACK_ID_LEN: usize = 256;
pub const MAX_TEXT_LEN: usize = 64;
/// A record's JSON may be at most this long. A typical one is ~10 KB and a
/// three-hour mix ~350 KB; this keeps a full `AnalysisRecords` batch
/// ([`hocket_core::connect::wire::ANALYSIS_BATCH_MAX`] records) far under
/// the frame cap.
pub const MAX_RECORD_BYTES: usize = 256 * 1024;

/// What became of one stored or uploaded record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Stored,
    /// The stored one is preferred.
    Kept,
}

/// Whether a record (for the right file) leaves the worker nothing to do:
/// it has a beat grid or is the worker's own.
pub fn complete(r: &AnalysisRecord) -> bool {
    has_beats(r) || r.produced_by == AnalysisProducer::Worker
}

/// Make a client's upload honest before it is validated: only this
/// coordinator's own worker produces `Worker` records (an upload claiming it
/// counts as `Desktop`), and nothing is created after `now_ms` (a record
/// "from the future" would otherwise win every tie forever).
pub fn sanitize_upload(r: &mut AnalysisRecord, now_ms: u64) {
    if r.produced_by == AnalysisProducer::Worker {
        r.produced_by = AnalysisProducer::Desktop;
    }
    let now = now_ms as f64;
    if r.created_at > now {
        r.created_at = now;
    }
}

/// Why an upload was refused.
pub fn validate(track_id: &str, r: &AnalysisRecord) -> Result<(), String> {
    if track_id.is_empty() || track_id.len() > MAX_TRACK_ID_LEN {
        return Err("bad track id".into());
    }
    if r.version == 0 || r.version > ANALYSIS_VERSION {
        return Err(format!(
            "version {} (this coordinator knows {ANALYSIS_VERSION})",
            r.version
        ));
    }
    if r.duration_ms == 0 || r.duration_ms > MAX_DURATION_MS {
        return Err("bad duration".into());
    }
    if r.analyser.len() > MAX_TEXT_LEN
        || r.source
            .suffix
            .as_ref()
            .is_some_and(|s| s.len() > MAX_TEXT_LEN)
    {
        return Err("text field too long".into());
    }
    if r.beats_ms.len() > MAX_BEATS || r.downbeats_ms.len() > r.beats_ms.len() {
        return Err("too many beats".into());
    }
    for c in [&r.head, &r.tail] {
        if c.lufs.len() > MAX_CURVE_POINTS || (c.hop_ms == 0 && !c.lufs.is_empty()) {
            return Err("bad loudness curve".into());
        }
    }
    let limit = r.duration_ms.saturating_add(1_000);
    if r.beats_ms.iter().chain(&r.downbeats_ms).any(|b| *b > limit)
        || r.audible_start_ms > limit
        || r.audible_end_ms > limit
    {
        return Err("position past the end".into());
    }
    if r.bpm
        .is_some_and(|b| !b.is_finite() || !(0.0..=1_000.0).contains(&b))
        || r.integrated_lufs.is_some_and(|v| !v.is_finite())
        || !r.beat_confidence.is_finite()
        || !r.created_at.is_finite()
        || r.source
            .size_bytes
            .is_some_and(|s| !s.is_finite() || s < 0.0)
    {
        return Err("bad number".into());
    }
    match serde_json::to_vec(r) {
        Ok(json) if json.len() <= MAX_RECORD_BYTES => Ok(()),
        Ok(_) => Err("record too large".into()),
        Err(_) => Err("unencodable record".into()),
    }
}

/// Whether `new` should replace `old` here: when it is preferred, or when
/// it describes another file and is at least as good (version, beats,
/// producer; age aside) or is the worker's own. So a client with a stale
/// fingerprint can't wipe the worker's beat grid (and the worker would not
/// put it back only to see it wiped again), while the worker, which
/// fingerprints from the server's current metadata, always gets its record
/// for a replaced file in. Unlike a device's local store, an older analysis
/// version never replaces a newer one (both came from clients of various
/// ages).
pub fn replaces(new: &AnalysisRecord, old: &AnalysisRecord) -> bool {
    use std::cmp::Ordering;
    if preference(new, old) == Ordering::Greater {
        return true;
    }
    if fingerprint_matches(&old.source, &new.source) {
        return false;
    }
    new.produced_by == AnalysisProducer::Worker
        || new
            .version
            .cmp(&old.version)
            .then(has_beats(new).cmp(&has_beats(old)))
            .then(producer_rank(new.produced_by).cmp(&producer_rank(old.produced_by)))
            != Ordering::Less
}

/// As [`preference`] ranks producers.
fn producer_rank(p: AnalysisProducer) -> u8 {
    match p {
        AnalysisProducer::Worker => 2,
        AnalysisProducer::Desktop => 1,
        AnalysisProducer::Device => 0,
    }
}

fn producer_name(p: AnalysisProducer) -> &'static str {
    match p {
        AnalysisProducer::Worker => "worker",
        AnalysisProducer::Desktop => "desktop",
        AnalysisProducer::Device => "device",
    }
}

/// Counts for `/health` and the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct StoreCounts {
    pub records: u64,
    pub with_beats: u64,
}

/// How much the store keeps. Sizes are the records' JSON, a little under
/// what they take on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreLimits {
    pub records_per_server: u64,
    pub bytes_per_server: u64,
    pub bytes_total: u64,
}

impl StoreLimits {
    pub const UNLIMITED: StoreLimits = StoreLimits {
        records_per_server: u64::MAX,
        bytes_per_server: u64::MAX,
        bytes_total: u64::MAX,
    };
}

#[derive(Debug, Clone, Copy, Default)]
struct Usage {
    rows: u64,
    bytes: u64,
}

impl Usage {
    fn add(&mut self, rows: i64, bytes: i64) {
        self.rows = self.rows.saturating_add_signed(rows);
        self.bytes = self.bytes.saturating_add_signed(bytes);
    }
}

struct Inner {
    conn: Connection,
    limits: StoreLimits,
    /// Rows and JSON bytes per server, kept in step with the table.
    usage: HashMap<String, Usage>,
    total: Usage,
}

impl Inner {
    fn account(&mut self, server: &str, rows: i64, bytes: i64) {
        self.usage
            .entry(server.to_string())
            .or_default()
            .add(rows, bytes);
        self.total.add(rows, bytes);
    }

    /// Drop the least recently updated records until `server` and the whole
    /// store are within the limits again, down to 90% of them so this does
    /// not run on every write. `keep` (the record just written) stays.
    fn enforce_limits(&mut self, server: &str, keep: &str) -> anyhow::Result<u64> {
        let l = self.limits;
        let below = |cap: u64| cap - cap / 10;
        let mut evicted = 0;
        let u = self.usage.get(server).copied().unwrap_or_default();
        if u.rows > l.records_per_server || u.bytes > l.bytes_per_server {
            let (rows, bytes) = (below(l.records_per_server), below(l.bytes_per_server));
            evicted += self.evict(Some(server), (server, keep), |s: &Inner| {
                let u = s.usage.get(server).copied().unwrap_or_default();
                u.rows <= rows && u.bytes <= bytes
            })?;
        }
        if self.total.bytes > l.bytes_total {
            let bytes = below(l.bytes_total);
            evicted += self.evict(None, (server, keep), |s: &Inner| s.total.bytes <= bytes)?;
        }
        Ok(evicted)
    }

    fn evict(
        &mut self,
        server: Option<&str>,
        keep: (&str, &str),
        done: impl Fn(&Inner) -> bool,
    ) -> anyhow::Result<u64> {
        let mut evicted = 0;
        while !done(self) {
            let oldest: Vec<(String, String, i64)> = {
                let mut stmt = self.conn.prepare_cached(
                    "SELECT server, track_id, length(json) FROM records
                     WHERE ?1 IS NULL OR server = ?1 ORDER BY updated_at LIMIT 256",
                )?;
                let rows = stmt.query_map([server], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect::<Result<_, _>>()?
            };
            let before = evicted;
            for (srv, track, bytes) in oldest {
                if srv == keep.0 && track == keep.1 {
                    continue;
                }
                self.conn.execute(
                    "DELETE FROM records WHERE server = ?1 AND track_id = ?2",
                    [&srv, &track],
                )?;
                self.account(&srv, -1, -bytes);
                evicted += 1;
                if done(self) {
                    break;
                }
            }
            if evicted == before {
                break; // only `keep` is left
            }
        }
        Ok(evicted)
    }
}

pub struct AnalysisStore {
    conn: Mutex<Inner>,
}

impl AnalysisStore {
    /// Open (or create) `<dir>/automix.sqlite`.
    pub fn open(dir: &Path) -> anyhow::Result<AnalysisStore> {
        std::fs::create_dir_all(dir)?;
        let conn = Connection::open(dir.join(STORE_FILE))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Self::init(conn)
    }

    /// A store that lives as long as the process (no `--data-dir`).
    pub fn in_memory() -> anyhow::Result<AnalysisStore> {
        Self::init(Connection::open_in_memory()?)
    }

    /// Keep to `limits` from now on (dropping records at the next write when
    /// already over them).
    pub fn with_limits(self, limits: StoreLimits) -> AnalysisStore {
        self.conn.lock().limits = limits;
        self
    }

    fn init(conn: Connection) -> anyhow::Result<AnalysisStore> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS records (
               server TEXT NOT NULL,
               track_id TEXT NOT NULL,
               version INTEGER NOT NULL,
               has_beats INTEGER NOT NULL,
               produced_by TEXT NOT NULL,
               json TEXT NOT NULL,
               updated_at INTEGER NOT NULL,
               PRIMARY KEY (server, track_id)
             ) WITHOUT ROWID;
             CREATE INDEX IF NOT EXISTS records_by_age ON records(updated_at);
             CREATE INDEX IF NOT EXISTS records_by_server_age ON records(server, updated_at);",
        )?;
        let mut usage: HashMap<String, Usage> = HashMap::new();
        let mut total = Usage::default();
        {
            let mut stmt = conn.prepare(
                "SELECT server, COUNT(*), COALESCE(SUM(length(json)), 0) FROM records GROUP BY server",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?;
            for row in rows {
                let (server, n, bytes) = row?;
                usage.entry(server).or_default().add(n, bytes);
                total.add(n, bytes);
            }
        }
        Ok(AnalysisStore {
            conn: Mutex::new(Inner {
                conn,
                limits: StoreLimits::UNLIMITED,
                usage,
                total,
            }),
        })
    }

    /// The stored record, whatever its version.
    pub fn get(&self, server: &str, track_id: &str) -> anyhow::Result<Option<AnalysisRecord>> {
        let json: Option<String> = self
            .conn
            .lock()
            .conn
            .query_row(
                "SELECT json FROM records WHERE server = ?1 AND track_id = ?2",
                [server, track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    /// The records of this analysis version among `track_ids`; the rest are
    /// left out.
    pub fn get_many(
        &self,
        server: &str,
        track_ids: &[TrackId],
    ) -> anyhow::Result<Vec<AnalysisEntry>> {
        let inner = self.conn.lock();
        let mut stmt = inner.conn.prepare_cached(
            "SELECT json FROM records WHERE server = ?1 AND track_id = ?2 AND version = ?3",
        )?;
        let mut out = vec![];
        for id in track_ids {
            let json: Option<String> = stmt
                .query_row(rusqlite::params![server, id, ANALYSIS_VERSION], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(record) = json.and_then(|j| serde_json::from_str(&j).ok()) {
                out.push(AnalysisEntry {
                    track_id: id.clone(),
                    record,
                });
            }
        }
        Ok(out)
    }

    /// Keep `record` if it is preferred over the stored one. The caller has
    /// validated it.
    pub fn put(
        &self,
        server: &str,
        track_id: &str,
        record: AnalysisRecord,
        now_ms: u64,
    ) -> anyhow::Result<PutOutcome> {
        let record = compact(record);
        let mut inner = self.conn.lock();
        let old: Option<String> = inner
            .conn
            .query_row(
                "SELECT json FROM records WHERE server = ?1 AND track_id = ?2",
                [server, track_id],
                |r| r.get(0),
            )
            .optional()?;
        let old_bytes = old.as_ref().map(|j| j.len() as i64);
        if let Some(old) = old.and_then(|j| serde_json::from_str::<AnalysisRecord>(&j).ok()) {
            if !replaces(&record, &old) {
                return Ok(PutOutcome::Kept);
            }
        }
        let json = serde_json::to_string(&record)?;
        let new_bytes = json.len() as i64;
        inner.conn.execute(
            "INSERT INTO records(server, track_id, version, has_beats, produced_by, json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(server, track_id) DO UPDATE SET
               version = excluded.version, has_beats = excluded.has_beats,
               produced_by = excluded.produced_by, json = excluded.json,
               updated_at = excluded.updated_at",
            rusqlite::params![
                server,
                track_id,
                record.version,
                has_beats(&record),
                producer_name(record.produced_by),
                json,
                now_ms as i64,
            ],
        )?;
        match old_bytes {
            Some(old) => inner.account(server, 0, new_bytes - old),
            None => inner.account(server, 1, new_bytes),
        }
        let evicted = inner.enforce_limits(server, track_id)?;
        if evicted > 0 {
            tracing::info!(
                evicted,
                "analysis store over its limits: dropped the oldest records"
            );
        }
        Ok(PutOutcome::Stored)
    }

    /// Whether the worker can leave this track alone: a usable record for
    /// the file `fingerprint` describes has a beat grid, or is the worker's
    /// own (a track without beats is not analysed again every pass).
    pub fn covers(
        &self,
        server: &str,
        track_id: &str,
        fingerprint: &SourceFingerprint,
    ) -> anyhow::Result<bool> {
        Ok(self
            .get(server, track_id)?
            .is_some_and(|r| is_usable(&r, fingerprint) && complete(&r)))
    }

    pub fn counts(&self, server: Option<&str>) -> anyhow::Result<StoreCounts> {
        let inner = self.conn.lock();
        let (records, with_beats): (i64, i64) = inner.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(has_beats), 0) FROM records
             WHERE version = ?1 AND (?2 IS NULL OR server = ?2)",
            rusqlite::params![ANALYSIS_VERSION, server],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok(StoreCounts {
            records: records as u64,
            with_beats: with_beats as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocket_core::api::LoudnessCurve;

    fn fp(size: f64) -> SourceFingerprint {
        SourceFingerprint {
            duration_ms: 200_000,
            size_bytes: Some(size),
            suffix: Some("flac".into()),
            transcoded: false,
        }
    }

    fn rec(producer: AnalysisProducer, beats: bool, created_at: f64) -> AnalysisRecord {
        AnalysisRecord {
            version: ANALYSIS_VERSION,
            analyser: "test".into(),
            produced_by: producer,
            source: fp(1000.0),
            duration_ms: 200_000,
            integrated_lufs: Some(-9.0),
            audible_start_ms: 0,
            audible_end_ms: 199_000,
            head: LoudnessCurve {
                start_ms: 0,
                hop_ms: 100,
                lufs: vec![-10.0; 600],
            },
            tail: LoudnessCurve {
                start_ms: 140_000,
                hop_ms: 100,
                lufs: vec![-10.0; 600],
            },
            head_vocals: Default::default(),
            tail_vocals: Default::default(),
            bpm: beats.then_some(120.0),
            beats_ms: if beats {
                (0..400).map(|i| i * 500).collect()
            } else {
                vec![]
            },
            downbeats_ms: if beats {
                (0..100).map(|i| i * 2000).collect()
            } else {
                vec![]
            },
            beat_confidence: if beats { 0.9 } else { 0.0 },
            created_at,
        }
    }

    #[test]
    fn preferred_records_replace_and_worse_ones_are_kept_out() {
        let s = AnalysisStore::in_memory().unwrap();
        let put = |r: AnalysisRecord| s.put("srv", "t1", r, 1).unwrap();
        assert_eq!(
            put(rec(AnalysisProducer::Device, false, 1.0)),
            PutOutcome::Stored
        );
        // beats beat loudness-only
        assert_eq!(
            put(rec(AnalysisProducer::Desktop, true, 2.0)),
            PutOutcome::Stored
        );
        // a phone's loudness-only record never displaces a beat grid
        assert_eq!(
            put(rec(AnalysisProducer::Device, false, 9.0)),
            PutOutcome::Kept
        );
        // Worker over Desktop
        assert_eq!(
            put(rec(AnalysisProducer::Worker, true, 1.0)),
            PutOutcome::Stored
        );
        assert_eq!(
            put(rec(AnalysisProducer::Desktop, true, 9.0)),
            PutOutcome::Kept
        );
        // the same record again changes nothing
        assert_eq!(
            put(rec(AnalysisProducer::Worker, true, 1.0)),
            PutOutcome::Kept
        );
        // newer of two equals wins
        assert_eq!(
            put(rec(AnalysisProducer::Worker, true, 5.0)),
            PutOutcome::Stored
        );
        assert_eq!(s.get("srv", "t1").unwrap().unwrap().created_at, 5.0);
        // the file changed: a worse record of the new file (or a stale
        // client's) doesn't wipe the worker's beat grid…
        let mut other = rec(AnalysisProducer::Device, false, 9.0);
        other.source = fp(2000.0);
        assert_eq!(put(other), PutOutcome::Kept);
        let mut stale_desktop = rec(AnalysisProducer::Desktop, true, 9.0);
        stale_desktop.source = fp(2000.0);
        assert_eq!(put(stale_desktop), PutOutcome::Kept);
        // …but the worker's own record of it always gets in
        let mut other = rec(AnalysisProducer::Worker, false, 0.0);
        other.source = fp(2000.0);
        assert_eq!(put(other), PutOutcome::Stored);
        assert!(!s.covers("srv", "t1", &fp(1000.0)).unwrap());
        assert!(
            s.covers("srv", "t1", &fp(2000.0)).unwrap(),
            "the worker's own"
        );
        assert_eq!(
            s.counts(None).unwrap(),
            StoreCounts {
                records: 1,
                with_beats: 0
            }
        );
    }

    #[test]
    fn another_file_replaces_when_at_least_as_good() {
        let s = AnalysisStore::in_memory().unwrap();
        let put = |r: AnalysisRecord| s.put("srv", "t1", r, 1).unwrap();
        put(rec(AnalysisProducer::Desktop, true, 5.0));
        let mut newer_file = rec(AnalysisProducer::Desktop, true, 1.0);
        newer_file.source = fp(2000.0);
        assert_eq!(put(newer_file), PutOutcome::Stored, "same rank, older");
        let mut phone = rec(AnalysisProducer::Device, true, 9.0);
        phone.source = fp(3000.0);
        assert_eq!(put(phone), PutOutcome::Kept, "lower rank");
        let mut loud = rec(AnalysisProducer::Desktop, false, 9.0);
        loud.source = fp(3000.0);
        assert_eq!(put(loud), PutOutcome::Kept, "no beats");
    }

    #[test]
    fn uploads_are_made_honest() {
        let mut r = rec(AnalysisProducer::Worker, true, 1e300);
        sanitize_upload(&mut r, 1_000);
        assert_eq!(r.produced_by, AnalysisProducer::Desktop);
        assert_eq!(r.created_at, 1_000.0);
        // a forged "worker" record no longer locks out the real worker
        let s = AnalysisStore::in_memory().unwrap();
        s.put("srv", "t1", r, 1).unwrap();
        assert!(s.covers("srv", "t1", &fp(1000.0)).unwrap(), "has beats");
        assert_eq!(
            s.put("srv", "t1", rec(AnalysisProducer::Worker, true, 1.0), 2)
                .unwrap(),
            PutOutcome::Stored
        );
        let mut device = rec(AnalysisProducer::Device, false, 5.0);
        sanitize_upload(&mut device, 1_000);
        assert_eq!(device.produced_by, AnalysisProducer::Device);
        assert_eq!(device.created_at, 5.0);
    }

    #[test]
    fn oversized_records_are_refused() {
        let mut r = rec(AnalysisProducer::Desktop, true, 1.0);
        r.duration_ms = MAX_DURATION_MS;
        r.beats_ms = (0..MAX_BEATS as u32).map(|i| i * 500).collect();
        r.head.lufs = vec![-12.345; MAX_CURVE_POINTS];
        r.tail.lufs = vec![-12.345; MAX_CURVE_POINTS];
        assert_eq!(validate("t1", &r), Err("record too large".into()));
        // a long but ordinary record is fine
        r.beats_ms.truncate(20_000);
        assert!(validate("t1", &r).is_ok());
    }

    #[test]
    fn the_oldest_records_go_when_over_the_limits() {
        let one = serde_json::to_string(&compact(rec(AnalysisProducer::Device, false, 1.0)))
            .unwrap()
            .len() as u64;
        let s = AnalysisStore::in_memory()
            .unwrap()
            .with_limits(StoreLimits {
                records_per_server: 10,
                bytes_per_server: u64::MAX,
                bytes_total: one * 15,
            });
        for i in 0..10 {
            s.put(
                "a",
                &format!("t{i}"),
                rec(AnalysisProducer::Device, false, 1.0),
                i,
            )
            .unwrap();
        }
        assert_eq!(s.counts(Some("a")).unwrap().records, 10);
        // the 11th drops the oldest down to 90%
        s.put("a", "t10", rec(AnalysisProducer::Device, false, 1.0), 10)
            .unwrap();
        assert_eq!(s.counts(Some("a")).unwrap().records, 9);
        assert!(s.get("a", "t0").unwrap().is_none());
        assert!(s.get("a", "t1").unwrap().is_none());
        assert!(s.get("a", "t10").unwrap().is_some());
        // the global cap counts every server; b's own record survives
        for i in 0..7 {
            s.put(
                "b",
                &format!("t{i}"),
                rec(AnalysisProducer::Device, false, 1.0),
                100 + i,
            )
            .unwrap();
        }
        let (a, b) = (
            s.counts(Some("a")).unwrap().records,
            s.counts(Some("b")).unwrap().records,
        );
        assert!(a + b <= 15, "{a} + {b}");
        assert!(a < 9, "a's older records went first");
        assert!(s.get("b", "t6").unwrap().is_some());
        assert_eq!((a, b), (6, 7));
    }

    #[test]
    fn usage_is_counted_again_on_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = AnalysisStore::open(dir.path()).unwrap();
            for i in 0..5 {
                s.put(
                    "a",
                    &format!("t{i}"),
                    rec(AnalysisProducer::Device, false, 1.0),
                    i,
                )
                .unwrap();
            }
        }
        let s = AnalysisStore::open(dir.path())
            .unwrap()
            .with_limits(StoreLimits {
                records_per_server: 5,
                ..StoreLimits::UNLIMITED
            });
        s.put("a", "t5", rec(AnalysisProducer::Device, false, 1.0), 5)
            .unwrap();
        assert_eq!(s.counts(Some("a")).unwrap().records, 5);
        assert!(s.get("a", "t0").unwrap().is_none());
    }

    #[test]
    fn servers_are_kept_apart() {
        let s = AnalysisStore::in_memory().unwrap();
        s.put("a", "t1", rec(AnalysisProducer::Worker, true, 1.0), 1)
            .unwrap();
        s.put("b", "t2", rec(AnalysisProducer::Device, false, 1.0), 1)
            .unwrap();
        let ids = vec!["t1".to_string(), "t2".to_string(), "t3".to_string()];
        let a = s.get_many("a", &ids).unwrap();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].track_id, "t1");
        let b = s.get_many("b", &ids).unwrap();
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].track_id, "t2");
        assert!(s.get_many("c", &ids).unwrap().is_empty());
        assert!(s.covers("a", "t1", &fp(1000.0)).unwrap());
        assert!(!s.covers("b", "t1", &fp(1000.0)).unwrap());
        // the worker's own record covers its track even without beats
        s.put("b", "t3", rec(AnalysisProducer::Worker, false, 1.0), 1)
            .unwrap();
        assert!(s.covers("b", "t3", &fp(1000.0)).unwrap());
        assert_eq!(s.counts(Some("a")).unwrap().with_beats, 1);
        assert_eq!(s.counts(None).unwrap().records, 3);
    }

    #[test]
    fn records_of_another_version_are_not_served() {
        let s = AnalysisStore::in_memory().unwrap();
        let mut old = rec(AnalysisProducer::Worker, true, 1.0);
        old.version = ANALYSIS_VERSION - 1;
        if old.version == 0 {
            // Nothing older exists yet: store it raw to stand in for one.
            s.conn
                .lock()
                .conn
                .execute(
                    "INSERT INTO records VALUES ('srv', 't1', 0, 1, 'worker', ?1, 0)",
                    [serde_json::to_string(&old).unwrap()],
                )
                .unwrap();
        } else {
            s.put("srv", "t1", old, 1).unwrap();
        }
        assert!(s.get_many("srv", &["t1".into()]).unwrap().is_empty());
        // the current version replaces it
        assert_eq!(
            s.put("srv", "t1", rec(AnalysisProducer::Device, false, 0.0), 1)
                .unwrap(),
            PutOutcome::Stored
        );
        assert_eq!(s.get_many("srv", &["t1".into()]).unwrap().len(), 1);
    }

    #[test]
    fn validation_refuses_junk() {
        let good = rec(AnalysisProducer::Desktop, true, 1.0);
        assert!(validate("t1", &good).is_ok());
        assert!(validate("t1", &rec(AnalysisProducer::Device, false, 1.0)).is_ok());
        assert!(validate("", &good).is_err());
        assert!(validate(&"x".repeat(300), &good).is_err());
        let bad: Vec<fn(&mut AnalysisRecord)> = vec![
            |r| r.version = ANALYSIS_VERSION + 1,
            |r| r.version = 0,
            |r| r.duration_ms = 0,
            |r| r.duration_ms = MAX_DURATION_MS + 1,
            |r| r.beats_ms = vec![1; MAX_BEATS + 1],
            |r| r.downbeats_ms = vec![1; r.beats_ms.len() + 1],
            |r| r.head.lufs = vec![-10.0; MAX_CURVE_POINTS + 1],
            |r| r.tail.hop_ms = 0,
            |r| r.beats_ms.push(r.duration_ms + 5_000),
            |r| r.bpm = Some(f32::NAN),
            |r| r.integrated_lufs = Some(f32::INFINITY),
            |r| r.analyser = "a".repeat(100),
            |r| r.created_at = f64::NAN,
        ];
        for (i, f) in bad.into_iter().enumerate() {
            let mut r = good.clone();
            f(&mut r);
            assert!(validate("t1", &r).is_err(), "case {i}");
        }
    }

    #[test]
    fn the_file_survives_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let s = AnalysisStore::open(dir.path()).unwrap();
            s.put("srv", "t1", rec(AnalysisProducer::Worker, true, 1.0), 1)
                .unwrap();
        }
        let s = AnalysisStore::open(dir.path()).unwrap();
        assert!(s.get("srv", "t1").unwrap().is_some());
        assert!(dir.path().join(STORE_FILE).exists());
    }
}
