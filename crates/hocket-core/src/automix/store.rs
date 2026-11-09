//! The local store of analysis records: the best [`AnalysisRecord`] known per
//! (server id, track id), in the client database (`automix_records`,
//! migration 0009). The planner reads it through [`best_for`]; platforms add
//! records with `Command::StoreAnalysisRecord` and, in a later phase, records
//! fetched from the coordinator land here too.

use rusqlite::OptionalExtension;

use crate::api::{AnalysisProducer, AnalysisRecord, SourceFingerprint, Track};
use crate::db::{Db, DbResult};

use super::record::{compact, fingerprint_matches, is_usable, preference};

/// The fingerprint a record for `track` must match: the original file as the
/// library describes it.
pub fn fingerprint_of(track: &Track) -> SourceFingerprint {
    SourceFingerprint {
        duration_ms: track.duration_ms,
        size_bytes: track.size_bytes,
        suffix: track.suffix.as_ref().map(|s| s.to_ascii_lowercase()),
        transcoded: false,
    }
}

fn producer_name(p: AnalysisProducer) -> &'static str {
    match p {
        AnalysisProducer::Worker => "worker",
        AnalysisProducer::Desktop => "desktop",
        AnalysisProducer::Device => "device",
    }
}

/// The stored record for a track, usable or not.
pub fn get(db: &Db, server_id: &str, track_id: &str) -> DbResult<Option<AnalysisRecord>> {
    db.with_conn(|c| {
        let json: Option<String> = c
            .query_row(
                "SELECT json FROM automix_records WHERE server_id = ?1 AND track_id = ?2",
                [server_id, track_id],
                |r| r.get(0),
            )
            .optional()?;
        match json {
            // A row this build can't read is as good as none (the next put replaces it).
            Some(j) => Ok(serde_json::from_str(&j).ok()),
            None => Ok(None),
        }
    })
}

/// Whether `new` should replace `old`: always when `old` describes another
/// file or is of another analysis version than `new`; otherwise when `new` is
/// at least as good ([`preference`]).
fn replaces(new: &AnalysisRecord, old: &AnalysisRecord) -> bool {
    if !fingerprint_matches(&old.source, &new.source) || old.version != new.version {
        return true;
    }
    preference(new, old) != std::cmp::Ordering::Less
}

/// Keep `record` for the track if it is the best known. Returns whether it
/// was stored.
pub fn put(
    db: &Db,
    server_id: &str,
    track_id: &str,
    record: AnalysisRecord,
    now: f64,
) -> DbResult<bool> {
    let record = compact(record);
    if let Some(old) = get(db, server_id, track_id)? {
        if !replaces(&record, &old) {
            return Ok(false);
        }
    }
    let json = serde_json::to_string(&record)?;
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO automix_records(server_id, track_id, version, produced_by, json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(server_id, track_id) DO UPDATE SET
               version = excluded.version, produced_by = excluded.produced_by,
               json = excluded.json, updated_at = excluded.updated_at",
            rusqlite::params![
                server_id,
                track_id,
                record.version,
                producer_name(record.produced_by),
                json,
                now
            ],
        )?;
        Ok(true)
    })
}

/// The stored record for `track` when the planner may use it (this analysis
/// version, fingerprint matching the track).
pub fn best_for(db: &Db, track: &Track) -> DbResult<Option<AnalysisRecord>> {
    let fp = fingerprint_of(track);
    Ok(get(db, &track.server_id, &track.id)?.filter(|r| is_usable(r, &fp)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automix::ANALYSIS_VERSION;

    fn track(id: &str, duration_ms: u32) -> Track {
        Track {
            id: id.into(),
            server_id: "srv".into(),
            duration_ms,
            suffix: Some("FLAC".into()),
            size_bytes: Some(1234.0),
            ..Default::default()
        }
    }

    fn rec(producer: AnalysisProducer, beats: bool, duration_ms: u32) -> AnalysisRecord {
        AnalysisRecord {
            version: ANALYSIS_VERSION,
            analyser: "test".into(),
            produced_by: producer,
            source: SourceFingerprint {
                duration_ms,
                size_bytes: Some(1234.0),
                suffix: Some("flac".into()),
                transcoded: false,
            },
            duration_ms,
            audible_end_ms: duration_ms,
            beats_ms: if beats { vec![500, 1000] } else { vec![] },
            downbeats_ms: if beats { vec![500] } else { vec![] },
            ..Default::default()
        }
    }

    #[test]
    fn keeps_the_best_record() {
        let db = Db::open_in_memory().unwrap();
        let t = track("t1", 200_000);
        assert_eq!(best_for(&db, &t).unwrap(), None);
        assert!(put(&db, "srv", "t1", rec(AnalysisProducer::Device, false, 200_000), 1.0).unwrap());
        assert_eq!(
            best_for(&db, &t).unwrap().unwrap().produced_by,
            AnalysisProducer::Device
        );
        // Beats beat a loudness-only record...
        assert!(put(&db, "srv", "t1", rec(AnalysisProducer::Desktop, true, 200_000), 2.0).unwrap());
        // ... and a loudness-only one never replaces them.
        assert!(!put(&db, "srv", "t1", rec(AnalysisProducer::Worker, false, 200_000), 3.0).unwrap());
        let best = get(&db, "srv", "t1").unwrap().unwrap();
        assert_eq!(best.produced_by, AnalysisProducer::Desktop);
        assert_eq!(best.beats_ms, vec![500, 1000]);
        // Other servers and tracks are separate.
        assert_eq!(get(&db, "other", "t1").unwrap(), None);
        assert_eq!(get(&db, "srv", "t2").unwrap(), None);
    }

    #[test]
    fn a_record_for_another_file_is_replaced_and_never_used() {
        let db = Db::open_in_memory().unwrap();
        assert!(put(&db, "srv", "t1", rec(AnalysisProducer::Worker, true, 100_000), 1.0).unwrap());
        // The file changed (another duration): the stored record no longer applies.
        let t = track("t1", 200_000);
        assert_eq!(best_for(&db, &t).unwrap(), None);
        assert!(put(&db, "srv", "t1", rec(AnalysisProducer::Device, false, 200_000), 2.0).unwrap());
        assert!(best_for(&db, &t).unwrap().is_some());
        // Another analysis version is stored but not usable.
        let mut old = rec(AnalysisProducer::Worker, true, 200_000);
        old.version = ANALYSIS_VERSION + 1;
        assert!(put(&db, "srv", "t1", old, 3.0).unwrap());
        assert_eq!(best_for(&db, &t).unwrap(), None);
        assert!(get(&db, "srv", "t1").unwrap().is_some());
    }
}
