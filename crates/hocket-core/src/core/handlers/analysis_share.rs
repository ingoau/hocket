//! AutoMix records shared through the hosted coordinator (`docs/automix.md`
//! → *Sharing records*). When the coordinator we are attached to says it
//! keeps records (`Welcome.analysis_store`), the tracks around the current
//! one that lack a full record are fetched in one batch at a time, and the
//! answers land in the local store like any other record (which replans the
//! follow-up). Records this device produced (desktop analysis, a platform's
//! loudness-only record) are uploaded; while there is no such coordinator
//! they wait in a short in-memory queue. Offline nothing changes: planning
//! only ever reads the local store.
//!
//! Records are keyed by a provider-aware fingerprint ([`AnalysisKey`]): the
//! provider instance and the file, never the account or the identity.

use std::collections::HashMap;

use crate::api::*;
use crate::connect::engine::Input;
use crate::connect::wire::{
    AnalysisEntry, AnalysisKey, AnalysisWant, ANALYSIS_BATCH_MAX, ANALYSIS_FETCH_MAX,
};
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
    /// The coordinator keeps records.
    available: bool,
    /// The fetch waiting for an answer: when it went out and what it asked.
    in_flight: Option<(f64, Vec<TrackRef>)>,
    /// Tracks asked for, until when they are not asked again.
    asked: HashMap<TrackRef, f64>,
    uploads: Vec<AnalysisEntry>,
}

impl Actor {
    pub(crate) fn analysis_store_available(&self) -> bool {
        self.engine.is_some() && self.share.available
    }

    pub(crate) fn on_analysis_store(&mut self, available: bool) {
        tracing::debug!(target: "hocket::automix", available, "coordinator analysis store");
        self.share.available = available;
        if !available {
            self.forget_in_flight_fetch();
            return;
        }
        self.flush_analysis_uploads();
        self.fetch_analysis_records(self.now());
    }

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

    fn forget_in_flight_fetch(&mut self) {
        if let Some((_, refs)) = self.share.in_flight.take() {
            for r in refs {
                self.share.asked.remove(&r);
            }
        }
    }

    /// The provider instance a track's source belongs to (`None` for a
    /// source no account here serves).
    fn analysis_provider_url(&self, source: &str) -> Option<String> {
        self.accounts
            .for_source(source)
            .filter(|a| a.info.kind == AccountKind::Navidrome)
            .map(|a| a.info.url.clone())
    }

    fn fetch_analysis_records(&mut self, now: f64) {
        if !self.analysis_store_available()
            || self.share.in_flight.is_some()
            || self.transition_settings().mode != TransitionMode::AutoMix
        {
            return;
        }
        let mut tracks: Vec<Track> = self.playback.track.iter().cloned().collect();
        for item in self.upcoming_playable(FETCH_AHEAD) {
            if let Some(t) = self.track_for_item(&item) {
                tracks.push(t);
            }
        }
        let mut wants: Vec<AnalysisWant> = vec![];
        for t in tracks {
            let track_id = TrackRef::new(t.server_id.clone(), t.id.clone());
            if wants.iter().any(|w| w.track_id == track_id)
                || self
                    .share
                    .asked
                    .get(&track_id)
                    .is_some_and(|until| *until > now)
                || !self.needs_analysis(&t)
            {
                continue;
            }
            let Some(url) = self.analysis_provider_url(&t.server_id) else {
                continue;
            };
            let key = AnalysisKey::navidrome(&url, &t.id, t.size_bytes, t.suffix.as_deref());
            wants.push(AnalysisWant { track_id, key });
        }
        wants.truncate(ANALYSIS_FETCH_MAX);
        if wants.is_empty() {
            return;
        }
        for w in &wants {
            self.share
                .asked
                .insert(w.track_id.clone(), now + ASKED_TTL_MS);
        }
        tracing::debug!(target: "hocket::automix", count = wants.len(), "fetching analysis records");
        self.share.in_flight = Some((now, wants.iter().map(|w| w.track_id.clone()).collect()));
        self.engine_input(Input::FetchAnalysis { wants });
    }

    pub(crate) fn on_analysis_records(&mut self, records: Vec<AnalysisEntry>) {
        self.share.in_flight = None;
        tracing::debug!(target: "hocket::automix", count = records.len(), "analysis records fetched");
        for e in records {
            self.store_analysis_record(e.track_id, e.record);
        }
        self.fetch_analysis_records(self.now());
    }

    /// Queue a record this device produced for the coordinator.
    pub(crate) fn share_analysis_record(
        &mut self,
        server_id: &str,
        track_id: &TrackId,
        record: &AnalysisRecord,
    ) {
        let Some(url) = self.analysis_provider_url(server_id) else {
            return;
        };
        let track_ref = TrackRef::new(server_id, track_id.clone());
        let key = AnalysisKey::navidrome(
            &url,
            track_id,
            record.source.size_bytes,
            record.source.suffix.as_deref(),
        );
        self.share.uploads.retain(|e| e.track_id != track_ref);
        if self.share.uploads.len() >= UPLOAD_QUEUE_MAX {
            self.share.uploads.remove(0);
        }
        self.share.uploads.push(AnalysisEntry {
            track_id: track_ref,
            key,
            record: record.clone(),
        });
        self.flush_analysis_uploads();
    }

    fn flush_analysis_uploads(&mut self) {
        if !self.analysis_store_available() || self.share.uploads.is_empty() {
            return;
        }
        let records: Vec<AnalysisEntry> = std::mem::take(&mut self.share.uploads);
        tracing::debug!(target: "hocket::automix", count = records.len(), "uploading analysis records");
        for chunk in records.chunks(ANALYSIS_BATCH_MAX) {
            self.engine_input(Input::UploadAnalysis {
                records: chunk.to_vec(),
            });
        }
    }
}
