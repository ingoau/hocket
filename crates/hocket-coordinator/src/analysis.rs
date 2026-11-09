//! The coordinator's AutoMix analysis store (`docs/automix.md` → *Sharing
//! records*): the preferred [`AnalysisRecord`] per (Navidrome server, track
//! id) in `<data-dir>/automix.sqlite` (in memory without a data dir). Any
//! client verified for a server reads and writes that server's records,
//! whoever its user; the opt-in worker ([`crate::worker`]) writes them too.
//!
//! A record replaces the stored one only when it describes another file
//! (the track was replaced) or is preferred ([`preference`]: version, beats,
//! producer, age). Uploads are validated first ([`validate`]).

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

/// What became of one stored or uploaded record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutOutcome {
    Stored,
    /// The stored one is preferred.
    Kept,
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
    Ok(())
}

/// Whether `new` should replace `old` here: when it describes another file
/// or is preferred. Unlike a device's local store, an older analysis version
/// never replaces a newer one (both came from clients of various ages).
pub fn replaces(new: &AnalysisRecord, old: &AnalysisRecord) -> bool {
    !fingerprint_matches(&old.source, &new.source)
        || preference(new, old) == std::cmp::Ordering::Greater
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

pub struct AnalysisStore {
    conn: Mutex<Connection>,
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
             ) WITHOUT ROWID;",
        )?;
        Ok(AnalysisStore {
            conn: Mutex::new(conn),
        })
    }

    /// The stored record, whatever its version.
    pub fn get(&self, server: &str, track_id: &str) -> anyhow::Result<Option<AnalysisRecord>> {
        let json: Option<String> = self
            .conn
            .lock()
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
        let conn = self.conn.lock();
        let mut stmt = conn.prepare_cached(
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
        let conn = self.conn.lock();
        let old: Option<String> = conn
            .query_row(
                "SELECT json FROM records WHERE server = ?1 AND track_id = ?2",
                [server, track_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old.and_then(|j| serde_json::from_str::<AnalysisRecord>(&j).ok()) {
            if !replaces(&record, &old) {
                return Ok(PutOutcome::Kept);
            }
        }
        conn.execute(
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
                serde_json::to_string(&record)?,
                now_ms as i64,
            ],
        )?;
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
        Ok(self.get(server, track_id)?.is_some_and(|r| {
            is_usable(&r, fingerprint)
                && (has_beats(&r) || r.produced_by == AnalysisProducer::Worker)
        }))
    }

    pub fn counts(&self, server: Option<&str>) -> anyhow::Result<StoreCounts> {
        let conn = self.conn.lock();
        let (records, with_beats): (i64, i64) = conn.query_row(
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
        // the file changed: any record of the new file replaces the old one
        let mut other = rec(AnalysisProducer::Device, false, 0.0);
        other.source = fp(2000.0);
        assert_eq!(put(other), PutOutcome::Stored);
        assert!(!s.covers("srv", "t1", &fp(1000.0)).unwrap());
        assert!(
            !s.covers("srv", "t1", &fp(2000.0)).unwrap(),
            "loudness-only"
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
