//! The replica the coordinator role keeps: a resume point, not a log.
//!
//! Stored per scope (`docs/design.md`, "Resume and recovery"):
//!
//! | Stored | Detail |
//! |---|---|
//! | Session | context, cursor, history, shuffle, insertions, `(position, takenAt, isPlaying)` as last received |
//! | Saved queues | the LWW set, per Navidrome user scope |
//! | Settings | account-synced keys, LWW |
//! | Leases | per-device last-seen and the transport lease |
//! | Scrobbles | a short dedupe log of `(trackId, startedAt)` |
//! | Not stored | play history, library metadata, anything re-derivable |
//!
//! It is a cache with an expiry: every device persists its own complete
//! session, and if the replica and a live device disagree, the live device
//! wins. [`ReplicaStore`] is the persistence seam: in-memory for tests and the
//! LAN role, JSON files for the hosted coordinator.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::api::{
    DeviceId, DeviceInfo, EpochMs, SavedQueue, SessionDocument, Setting, TrackId, TransportLease,
};
use crate::connect::wire::{
    merge_saved_queues, merge_settings, DeviceLease, LastStamp, ReplicaState, ScrobbleRecord,
};

/// Replicas idle for this long are dropped by [`ReplicaStore::purge_expired`].
pub const REPLICA_TTL_MS: f64 = 30.0 * 24.0 * 3_600_000.0;
/// Dedupe records older than this are forgotten. Last.fm rejects scrobbles
/// older than two weeks anyway; a day is plenty for handoffs and outbox flushes.
pub const SCROBBLE_LOG_TTL_MS: f64 = 24.0 * 3_600_000.0;
/// Hard cap on the dedupe log, oldest first.
pub const SCROBBLE_LOG_CAP: usize = 500;
/// Device leases not seen for this long are forgotten.
pub const DEVICE_TTL_MS: f64 = 7.0 * 24.0 * 3_600_000.0;
/// Cap on stored history (mirrors the session module's own cap).
pub const HISTORY_CAP: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum ReplicaError {
    #[error("replica io: {0}")]
    Io(#[from] std::io::Error),
    #[error("replica json: {0}")]
    Json(#[from] serde_json::Error),
}

/// Outcome of a scrobble claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrobbleClaim {
    /// Recorded now; the caller should submit.
    New,
    /// Already recorded by this same device (idempotent retry); submit is fine.
    Own,
    /// Already recorded by another device: a duplicate.
    Duplicate,
}

/// A fresh replica around a document.
pub fn new_replica(document: SessionDocument, now: EpochMs) -> ReplicaState {
    ReplicaState {
        document,
        saved_queues: vec![],
        settings: vec![],
        devices: vec![],
        transport_lease: TransportLease::default(),
        last_stamp: None,
        scrobbles: vec![],
        updated_at: now,
        extra: HashMap::new(),
    }
}

/// Operations on a replica. Every mutation bumps `updated_at`.
pub trait ReplicaExt {
    fn touch_device(&mut self, device: &DeviceInfo, now: EpochMs);
    fn forget_device(&mut self, device_id: &str);
    fn merge_saved_queues(&mut self, theirs: &[SavedQueue], now: EpochMs) -> bool;
    fn merge_settings(&mut self, theirs: &[Setting], now: EpochMs) -> bool;
    fn claim_scrobble(
        &mut self,
        track_id: &str,
        started_at: EpochMs,
        device_id: &str,
        now: EpochMs,
    ) -> ScrobbleClaim;
    fn set_document(&mut self, document: SessionDocument, now: EpochMs);
    fn set_stamp(&mut self, stamp: LastStamp, now: EpochMs);
    fn set_lease(&mut self, lease: TransportLease, now: EpochMs);
    fn expire(&mut self, now: EpochMs);
    fn is_expired(&self, now: EpochMs) -> bool;
    fn device_last_seen(&self, device_id: &str) -> Option<EpochMs>;
}

impl ReplicaExt for ReplicaState {
    fn touch_device(&mut self, device: &DeviceInfo, now: EpochMs) {
        match self.devices.iter_mut().find(|d| d.device.id == device.id) {
            Some(d) => {
                d.device = device.clone();
                d.device.is_self = false;
                d.last_seen = now;
            }
            None => {
                let mut dev = device.clone();
                dev.is_self = false;
                self.devices.push(DeviceLease {
                    device: dev,
                    last_seen: now,
                });
            }
        }
        self.updated_at = now;
    }

    fn forget_device(&mut self, device_id: &str) {
        self.devices.retain(|d| d.device.id != device_id);
    }

    fn merge_saved_queues(&mut self, theirs: &[SavedQueue], now: EpochMs) -> bool {
        let (merged, changed) = merge_saved_queues(&self.saved_queues, theirs);
        if changed {
            self.saved_queues = merged;
            self.updated_at = now;
        }
        changed
    }

    fn merge_settings(&mut self, theirs: &[Setting], now: EpochMs) -> bool {
        let (merged, changed) = merge_settings(&self.settings, theirs);
        if changed {
            self.settings = merged;
            self.updated_at = now;
        }
        changed
    }

    fn claim_scrobble(
        &mut self,
        track_id: &str,
        started_at: EpochMs,
        device_id: &str,
        now: EpochMs,
    ) -> ScrobbleClaim {
        if let Some(r) = self
            .scrobbles
            .iter()
            .find(|r| r.track_id == track_id && same_start(r.started_at, started_at))
        {
            return if r.device_id == device_id {
                ScrobbleClaim::Own
            } else {
                ScrobbleClaim::Duplicate
            };
        }
        self.scrobbles.push(ScrobbleRecord {
            track_id: track_id.to_string(),
            started_at,
            device_id: device_id.to_string(),
            recorded_at: now,
        });
        while self.scrobbles.len() > SCROBBLE_LOG_CAP {
            self.scrobbles.remove(0);
        }
        self.updated_at = now;
        ScrobbleClaim::New
    }

    fn set_document(&mut self, mut document: SessionDocument, now: EpochMs) {
        if document.history.len() > HISTORY_CAP {
            let drop = document.history.len() - HISTORY_CAP;
            document.history.drain(0..drop);
        }
        self.document = document;
        self.updated_at = now;
    }

    fn set_stamp(&mut self, stamp: LastStamp, now: EpochMs) {
        self.document.transport.position = stamp.position.clone();
        self.document.transport.played_ms = stamp.played_ms;
        self.last_stamp = Some(stamp);
        self.updated_at = now;
    }

    fn set_lease(&mut self, lease: TransportLease, now: EpochMs) {
        self.document.transport.lease = lease.clone();
        self.transport_lease = lease;
        self.updated_at = now;
    }

    fn expire(&mut self, now: EpochMs) {
        self.scrobbles
            .retain(|r| now - r.recorded_at < SCROBBLE_LOG_TTL_MS);
        self.devices.retain(|d| now - d.last_seen < DEVICE_TTL_MS);
    }

    fn is_expired(&self, now: EpochMs) -> bool {
        now - self.updated_at > REPLICA_TTL_MS
    }

    fn device_last_seen(&self, device_id: &str) -> Option<EpochMs> {
        self.devices
            .iter()
            .find(|d| d.device.id == device_id)
            .map(|d| d.last_seen)
    }
}

/// Scrobble identity tolerance: `startedAt` travels as a float through JSON
/// and a handoff; a second of slack keeps the pair recognisable.
fn same_start(a: EpochMs, b: EpochMs) -> bool {
    (a - b).abs() < 1000.0
}

/// A scrobble identity, for callers that want to reason about pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct ScrobblePair {
    pub track_id: TrackId,
    pub started_at: EpochMs,
    pub device_id: DeviceId,
}

/// Persistence seam for replicas, keyed by scope.
pub trait ReplicaStore: Send + Sync {
    fn load(&self, scope: &str) -> Result<Option<ReplicaState>, ReplicaError>;
    fn save(&self, scope: &str, replica: &ReplicaState) -> Result<(), ReplicaError>;
    fn delete(&self, scope: &str) -> Result<(), ReplicaError>;
    fn scopes(&self) -> Result<Vec<String>, ReplicaError>;
    /// Drop replicas idle past [`REPLICA_TTL_MS`]. Returns how many were dropped.
    fn purge_expired(&self, now: EpochMs) -> Result<usize, ReplicaError> {
        let mut dropped = 0;
        for scope in self.scopes()? {
            if let Some(r) = self.load(&scope)? {
                if r.is_expired(now) {
                    self.delete(&scope)?;
                    dropped += 1;
                }
            }
        }
        Ok(dropped)
    }
}

/// In-memory store: tests, the LAN role, the simulation.
#[derive(Debug, Default)]
pub struct MemoryReplicaStore {
    inner: parking_lot::Mutex<HashMap<String, ReplicaState>>,
}

impl MemoryReplicaStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ReplicaStore for MemoryReplicaStore {
    fn load(&self, scope: &str) -> Result<Option<ReplicaState>, ReplicaError> {
        Ok(self.inner.lock().get(scope).cloned())
    }

    fn save(&self, scope: &str, replica: &ReplicaState) -> Result<(), ReplicaError> {
        self.inner.lock().insert(scope.to_string(), replica.clone());
        Ok(())
    }

    fn delete(&self, scope: &str) -> Result<(), ReplicaError> {
        self.inner.lock().remove(scope);
        Ok(())
    }

    fn scopes(&self) -> Result<Vec<String>, ReplicaError> {
        Ok(self.inner.lock().keys().cloned().collect())
    }
}

/// One JSON file per scope under a directory, written atomically
/// (temp file + rename). The file name is a hash of the scope, so a scope
/// containing a URL never touches the file system's naming rules.
#[derive(Debug, Clone)]
pub struct FileReplicaStore {
    dir: PathBuf,
}

/// On-disk envelope with a schema version for forward-only migrations.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct StoredReplica {
    schema_version: u32,
    scope: String,
    replica: ReplicaState,
}

const STORE_SCHEMA_VERSION: u32 = 1;

impl FileReplicaStore {
    /// Creates the directory if missing.
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self, ReplicaError> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(FileReplicaStore { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, scope: &str) -> PathBuf {
        use md5::{Digest, Md5};
        let digest = Md5::digest(scope.as_bytes());
        self.dir
            .join(format!("{}.replica.json", hex::encode(digest)))
    }

    fn read_file(path: &Path) -> Result<Option<StoredReplica>, ReplicaError> {
        match std::fs::read(path) {
            Ok(bytes) => {
                let stored: StoredReplica = serde_json::from_slice(&bytes)?;
                Ok(Some(stored))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

impl ReplicaStore for FileReplicaStore {
    fn load(&self, scope: &str) -> Result<Option<ReplicaState>, ReplicaError> {
        match Self::read_file(&self.path_for(scope))? {
            Some(stored) if stored.schema_version <= STORE_SCHEMA_VERSION => {
                Ok(Some(stored.replica))
            }
            Some(stored) => {
                tracing::warn!(
                    scope,
                    version = stored.schema_version,
                    "replica file from a newer schema; ignoring"
                );
                Ok(None)
            }
            None => Ok(None),
        }
    }

    fn save(&self, scope: &str, replica: &ReplicaState) -> Result<(), ReplicaError> {
        use std::io::Write;
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = self.path_for(scope);
        // A unique temp name per writer, so two saves of one scope never
        // share (and truncate) each other's file.
        let tmp = path.with_extension(format!(
            "json.{}.{}.tmp",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let stored = StoredReplica {
            schema_version: STORE_SCHEMA_VERSION,
            scope: scope.to_string(),
            replica: replica.clone(),
        };
        let bytes = serde_json::to_vec(&stored)?;
        let result = (|| -> std::io::Result<()> {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(&bytes)?;
            // Content before name: a rename that survives a power cut must
            // point at a file that survived it too.
            f.sync_all()?;
            std::fs::rename(&tmp, &path)?;
            #[cfg(unix)]
            {
                std::fs::File::open(&self.dir)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result?;
        Ok(())
    }

    fn delete(&self, scope: &str) -> Result<(), ReplicaError> {
        match std::fs::remove_file(self.path_for(scope)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn scopes(&self) -> Result<Vec<String>, ReplicaError> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if !path.to_string_lossy().ends_with(".replica.json") {
                continue;
            }
            match Self::read_file(&path) {
                Ok(Some(stored)) => out.push(stored.scope),
                Ok(None) => {}
                Err(e) => tracing::warn!(?path, error = %e, "unreadable replica file skipped"),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Platform, QueueMode, RepeatMode, SettingScope, TransportState};

    fn doc() -> SessionDocument {
        SessionDocument {
            schema_version: 1,
            session_id: "s".into(),
            scope: "scope".into(),
            revision: 1,
            updated_at: 0.0,
            context: None,
            mode: QueueMode::Apple,
            cursor: 0,
            current: None,
            history: vec![],
            insertions: vec![],
            shuffle: None,
            repeat: RepeatMode::Off,
            autoplay: false,
            transport: TransportState::default(),
            saved_queues: vec![],
            extra: HashMap::new(),
        }
    }

    fn dev(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.into(),
            name: id.into(),
            platform: Platform::Android,
            app_version: "1".into(),
            playing: false,
            ready: false,
            last_seen: 0.0,
            is_self: true,
        }
    }

    #[test]
    fn scrobble_claims_dedupe_across_devices_but_not_self() {
        let mut r = new_replica(doc(), 0.0);
        assert_eq!(r.claim_scrobble("t", 1000.0, "a", 1.0), ScrobbleClaim::New);
        assert_eq!(r.claim_scrobble("t", 1000.4, "a", 2.0), ScrobbleClaim::Own);
        assert_eq!(
            r.claim_scrobble("t", 1000.0, "b", 3.0),
            ScrobbleClaim::Duplicate
        );
        assert_eq!(r.claim_scrobble("t", 5000.0, "b", 4.0), ScrobbleClaim::New);
        assert_eq!(r.scrobbles.len(), 2);
    }

    #[test]
    fn scrobble_log_is_capped_and_expires() {
        let mut r = new_replica(doc(), 0.0);
        for i in 0..(SCROBBLE_LOG_CAP + 10) {
            r.claim_scrobble("t", i as f64 * 10_000.0, "a", i as f64);
        }
        assert_eq!(r.scrobbles.len(), SCROBBLE_LOG_CAP);
        // kept: recorded_at 10..=509; expiring at TTL+100 drops recorded_at <= 100
        r.expire(SCROBBLE_LOG_TTL_MS + 100.0);
        assert_eq!(r.scrobbles.len(), SCROBBLE_LOG_CAP - 91);
    }

    #[test]
    fn devices_touch_and_expire() {
        let mut r = new_replica(doc(), 0.0);
        r.touch_device(&dev("a"), 10.0);
        r.touch_device(&dev("a"), 20.0);
        r.touch_device(&dev("b"), 30.0);
        assert_eq!(r.devices.len(), 2);
        assert_eq!(r.device_last_seen("a"), Some(20.0));
        assert!(!r.devices[0].device.is_self);
        r.expire(20.0 + DEVICE_TTL_MS);
        assert_eq!(r.devices.len(), 1);
        r.forget_device("b");
        assert!(r.devices.is_empty());
    }

    #[test]
    fn history_is_capped_when_stored() {
        let mut r = new_replica(doc(), 0.0);
        let mut d = doc();
        for i in 0..300 {
            d.history.push(crate::api::QueueItem {
                key: format!("k{i}"),
                track_id: format!("t{i}"),
                source: crate::api::QueueSource::Inserted,
                unavailable: false,
            });
        }
        r.set_document(d, 5.0);
        assert_eq!(r.document.history.len(), HISTORY_CAP);
        assert_eq!(r.document.history[0].key, "k100");
    }

    #[test]
    fn lww_merges_bump_updated_at_only_on_change() {
        let mut r = new_replica(doc(), 0.0);
        let s = Setting {
            key: "k".into(),
            value: "1".into(),
            scope: SettingScope::AccountSynced,
            updated_at: 5.0,
        };
        assert!(r.merge_settings(std::slice::from_ref(&s), 1.0));
        assert_eq!(r.updated_at, 1.0);
        assert!(!r.merge_settings(&[s], 2.0));
        assert_eq!(r.updated_at, 1.0);
    }

    #[test]
    fn replica_expiry() {
        let r = new_replica(doc(), 0.0);
        assert!(!r.is_expired(REPLICA_TTL_MS));
        assert!(r.is_expired(REPLICA_TTL_MS + 1.0));
    }

    #[test]
    fn memory_store_round_trip_and_purge() {
        let store = MemoryReplicaStore::new();
        store.save("a", &new_replica(doc(), 0.0)).unwrap();
        store.save("b", &new_replica(doc(), 1_000_000.0)).unwrap();
        assert_eq!(store.load("a").unwrap().unwrap().updated_at, 0.0);
        let mut scopes = store.scopes().unwrap();
        scopes.sort();
        assert_eq!(scopes, vec!["a", "b"]);
        assert_eq!(store.purge_expired(REPLICA_TTL_MS + 10.0).unwrap(), 1);
        assert!(store.load("a").unwrap().is_none());
        assert!(store.load("b").unwrap().is_some());
        store.delete("b").unwrap();
        assert!(store.scopes().unwrap().is_empty());
    }

    #[test]
    fn file_store_round_trip_atomic_and_purge() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileReplicaStore::new(dir.path().join("replicas")).unwrap();
        assert!(store.load("https://x|u").unwrap().is_none());
        let mut r = new_replica(doc(), 100.0);
        r.extra.insert("future".into(), serde_json::json!(1));
        store.save("https://x|u", &r).unwrap();
        let back = store.load("https://x|u").unwrap().unwrap();
        assert_eq!(back, r);
        assert_eq!(store.scopes().unwrap(), vec!["https://x|u".to_string()]);
        // no temp files left behind
        let leftovers: Vec<_> = std::fs::read_dir(store.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        // garbage file is skipped, not fatal
        std::fs::write(store.dir().join("junk.replica.json"), b"not json").unwrap();
        assert_eq!(store.scopes().unwrap().len(), 1);
        assert_eq!(
            store.purge_expired(100.0 + REPLICA_TTL_MS + 1.0).unwrap(),
            1
        );
        assert!(store.load("https://x|u").unwrap().is_none());
        store.delete("https://x|u").unwrap(); // idempotent
    }

    #[test]
    fn file_store_ignores_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileReplicaStore::new(dir.path()).unwrap();
        let stored = StoredReplica {
            schema_version: 99,
            scope: "s".into(),
            replica: new_replica(doc(), 0.0),
        };
        std::fs::write(store.path_for("s"), serde_json::to_vec(&stored).unwrap()).unwrap();
        assert!(store.load("s").unwrap().is_none());
    }
}
