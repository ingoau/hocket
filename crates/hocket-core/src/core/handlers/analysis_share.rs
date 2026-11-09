//! AutoMix records shared through the hosted coordinator (`docs/automix.md`
//! → *Sharing records*). When the coordinator we are attached to says it
//! keeps records (`Welcome.analysis_store`), the tracks around the current
//! one that lack a full record are fetched in one batch at a time, and the
//! answers land in the local store like any other record (which replans the
//! follow-up). Records this device produced (desktop analysis, a platform's
//! loudness-only record) are uploaded; while there is no such coordinator
//! they wait in a short in-memory queue. Offline nothing changes: planning
//! only ever reads the local store.

use std::collections::HashMap;

use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::{AnalysisEntry, ANALYSIS_BATCH_MAX, ANALYSIS_FETCH_MAX};
use crate::core::actor::Actor;

/// Tracks after the current one that are fetched ahead.
const FETCH_AHEAD: usize = 4;
/// An unanswered fetch is given up after this long (its tracks may be asked again).
const FETCH_TIMEOUT_MS: f64 = 15_000.0;
/// A track asked for is not asked again for this long, answered or not.
const ASKED_TTL_MS: f64 = 30.0 * 60_000.0;
/// Records waiting for a coordinator, at most.
const UPLOAD_QUEUE_MAX: usize = 64;

#[derive(Default)]
pub(crate) struct ShareState {
    /// The server whose coordinator said it keeps records; `None` otherwise.
    available_for: Option<String>,
    /// The one outstanding fetch: when it went out and what it asked for.
    in_flight: Option<(f64, Vec<TrackId>)>,
    /// (server, track) → asked until (epoch ms).
    asked: HashMap<(String, TrackId), f64>,
    /// (server, record) waiting to be uploaded, oldest first.
    uploads: Vec<(String, AnalysisEntry)>,
}

impl Actor {
    /// Whether the attached coordinator keeps records for the current server.
    pub(crate) fn analysis_store_available(&self) -> bool {
        self.engine.is_some()
            && self.share.available_for.is_some()
            && self.share.available_for == self.server_id()
    }

    /// `Output::AnalysisStore`.
    pub(crate) fn on_analysis_store(&mut self, available: bool) {
        tracing::debug!(target: "hocket::automix", available, "coordinator analysis store");
        self.share.available_for = if available { self.server_id() } else { None };
        if !available {
            self.forget_in_flight_fetch();
            return;
        }
        self.flush_analysis_uploads();
        self.fetch_analysis_records(self.now());
    }

    /// From the analysis tick.
    pub(crate) fn analysis_share_tick(&mut self, now: f64) {
        if let Some((sent, _)) = &self.share.in_flight {
            if now - sent > FETCH_TIMEOUT_MS {
                self.forget_in_flight_fetch();
            }
        }
        if self.share.asked.len() > 4096 {
            self.share.asked.retain(|_, until| *until > now);
        }
        self.fetch_analysis_records(now);
    }

    /// The tracks of an unanswered fetch may be asked for again.
    fn forget_in_flight_fetch(&mut self) {
        if let (Some((_, ids)), Some(server)) = (self.share.in_flight.take(), self.server_id()) {
            for id in ids {
                self.share.asked.remove(&(server.clone(), id));
            }
        }
    }

    /// Ask the coordinator for the records the window lacks, when it keeps
    /// them, the mode is AutoMix and no fetch is outstanding.
    fn fetch_analysis_records(&mut self, now: f64) {
        if !self.analysis_store_available()
            || self.share.in_flight.is_some()
            || self.transition_settings().mode != TransitionMode::AutoMix
        {
            return;
        }
        let Some(server_id) = self.server_id() else {
            return;
        };
        let mut tracks: Vec<Track> = self.playback.track.iter().cloned().collect();
        for item in self.upcoming_playable(FETCH_AHEAD) {
            if let Some(t) = self.track_or_bare(&item.track_id) {
                tracks.push(t);
            }
        }
        let mut ids: Vec<TrackId> = vec![];
        for t in tracks {
            let key = (server_id.clone(), t.id.clone());
            if t.server_id != server_id
                || ids.contains(&t.id)
                || self.share.asked.get(&key).is_some_and(|until| *until > now)
                || !self.needs_analysis(&t)
            {
                continue;
            }
            ids.push(t.id);
        }
        ids.truncate(ANALYSIS_FETCH_MAX);
        if ids.is_empty() {
            return;
        }
        for id in &ids {
            self.share
                .asked
                .insert((server_id.clone(), id.clone()), now + ASKED_TTL_MS);
        }
        tracing::debug!(target: "hocket::automix", count = ids.len(), "fetching analysis records");
        self.share.in_flight = Some((now, ids.clone()));
        self.engine_input(Input::FetchAnalysis { track_ids: ids });
    }

    /// `Output::AnalysisRecords`: the coordinator's answer.
    pub(crate) fn on_analysis_records(&mut self, records: Vec<AnalysisEntry>) {
        self.share.in_flight = None;
        tracing::debug!(target: "hocket::automix", count = records.len(), "analysis records fetched");
        for e in records {
            self.store_analysis_record(e.track_id, e.record);
        }
        self.fetch_analysis_records(self.now());
    }

    /// A record this device produced: upload it now or once a coordinator
    /// that keeps records is reachable.
    pub(crate) fn share_analysis_record(
        &mut self,
        server_id: &str,
        track_id: &TrackId,
        record: &AnalysisRecord,
    ) {
        self.share
            .uploads
            .retain(|(s, e)| !(s == server_id && &e.track_id == track_id));
        if self.share.uploads.len() >= UPLOAD_QUEUE_MAX {
            self.share.uploads.remove(0);
        }
        self.share.uploads.push((
            server_id.to_string(),
            AnalysisEntry {
                track_id: track_id.clone(),
                record: record.clone(),
            },
        ));
        self.flush_analysis_uploads();
    }

    fn flush_analysis_uploads(&mut self) {
        if !self.analysis_store_available() || self.share.uploads.is_empty() {
            return;
        }
        let Some(server_id) = self.server_id() else {
            return;
        };
        // Records of another server have nowhere to go: this room is this server's.
        let records: Vec<AnalysisEntry> = std::mem::take(&mut self.share.uploads)
            .into_iter()
            .filter(|(s, _)| *s == server_id)
            .map(|(_, e)| e)
            .collect();
        tracing::debug!(target: "hocket::automix", count = records.len(), "uploading analysis records");
        for chunk in records.chunks(ANALYSIS_BATCH_MAX) {
            self.engine_input(Input::UploadAnalysis {
                records: chunk.to_vec(),
            });
        }
    }
}
