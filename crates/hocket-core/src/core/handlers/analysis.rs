//! AutoMix analysis on a miss (`docs/automix.md` → *Platforms*, desktop):
//! with the mode on AutoMix, the current track and the next two that lack a
//! full record are analysed, one at a time, on a single background thread
//! once their whole file is on disk (a download or a complete stream-cache
//! entry; prefetch already caches the next two). The record goes to the
//! local store, which replans the follow-up.
//!
//! Only builds with the feature `automix-analysis` analyse (the desktop
//! addon); elsewhere the tick is a no-op. The coordinator's core never runs
//! it (its own worker is `hocket_coordinator::analysis`).

use std::collections::HashSet;

use crate::api::*;
use crate::core::actor::Actor;

/// How often the actor looks for something to analyse.
const CHECK_INTERVAL_MS: f64 = 2_000.0;

#[derive(Default)]
pub(crate) struct AnalysisState {
    /// Jobs to the worker thread, started on first use.
    #[cfg(feature = "automix-analysis")]
    worker: Option<std::sync::mpsc::Sender<worker::Job>>,
    /// (server, track) being analysed; one at a time.
    busy: Option<(String, TrackId)>,
    /// Tracks tried this session (successful or not): never twice.
    tried: HashSet<(String, TrackId)>,
    last_check_ms: f64,
}

/// What the worker sends back.
pub(crate) type AnalysisResult = Result<Box<AnalysisRecord>, String>;

impl Actor {
    /// From `on_tick`: start analysing the next track that needs it.
    pub(crate) fn analysis_tick(&mut self, now: f64) {
        if now - self.analysis.last_check_ms < CHECK_INTERVAL_MS {
            return;
        }
        self.analysis.last_check_ms = now;
        self.analysis_share_tick(now);
        self.start_next_analysis();
    }

    /// Whether `track` has no record good enough for AutoMix: none usable,
    /// or only a phone's loudness-only one.
    pub(crate) fn needs_analysis(&self, track: &Track) -> bool {
        match self.analysis_record_for(track) {
            None => true,
            Some(r) => r.beats_ms.is_empty() && r.produced_by == AnalysisProducer::Device,
        }
    }

    /// The whole file of `track` on disk, and whether it is a transcode.
    pub(crate) fn complete_local_file(&self, track: &Track) -> Option<(std::path::PathBuf, bool)> {
        if let Ok(Some(path)) = self.downloads.downloaded_path(&track.server_id, &track.id) {
            let original = track
                .suffix
                .as_deref()
                .filter(|s| !s.is_empty())
                .is_some_and(|want| {
                    path.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|got| want.eq_ignore_ascii_case(got))
                });
            return Some((path, !original));
        }
        let profile = self.downloads.effective_profile(track.suffix.as_deref());
        match self
            .downloads
            .cache_lookup(&track.server_id, &track.id, profile.as_ref(), false)
        {
            Ok(Some(e)) => Some((e.path, !e.profile.is_empty())),
            _ => None,
        }
    }

    /// Current, next and next-next, in that order.
    fn analysis_candidates(&self) -> Vec<Track> {
        let mut out: Vec<Track> = self.playback.track.iter().cloned().collect();
        for item in self.upcoming_playable(2) {
            if let Some(t) = self.track_or_bare(&item.track_id) {
                if !out.iter().any(|o| o.id == t.id) {
                    out.push(t);
                }
            }
        }
        out
    }

    fn start_next_analysis(&mut self) {
        if !cfg!(feature = "automix-analysis")
            || self.cfg.coordinator_listen.is_some()
            || self.analysis.busy.is_some()
            || self.transition_settings().mode != TransitionMode::AutoMix
        {
            return;
        }
        let Some(server_id) = self.server_id() else {
            return;
        };
        for track in self.analysis_candidates() {
            let key = (server_id.clone(), track.id.clone());
            if track.server_id != server_id
                || self.analysis.tried.contains(&key)
                || !self.needs_analysis(&track)
            {
                continue;
            }
            let Some((path, transcoded)) = self.complete_local_file(&track) else {
                continue; // not on disk yet: tried again once it is
            };
            let mut fingerprint = crate::automix::store::fingerprint_of(&track);
            fingerprint.transcoded = transcoded;
            if self.send_analysis_job(key.clone(), path, fingerprint) {
                self.analysis.tried.insert(key.clone());
                self.analysis.busy = Some(key);
            }
            return;
        }
    }

    #[cfg(feature = "automix-analysis")]
    fn send_analysis_job(
        &mut self,
        key: (String, TrackId),
        path: std::path::PathBuf,
        fingerprint: SourceFingerprint,
    ) -> bool {
        let job = worker::Job {
            server_id: key.0,
            track_id: key.1,
            path,
            fingerprint,
        };
        if self.analysis.worker.is_none() {
            self.analysis.worker = worker::spawn(self.tx.clone());
        }
        match &self.analysis.worker {
            Some(w) => w.send(job).is_ok(),
            None => false,
        }
    }

    #[cfg(not(feature = "automix-analysis"))]
    fn send_analysis_job(
        &mut self,
        _key: (String, TrackId),
        _path: std::path::PathBuf,
        _fingerprint: SourceFingerprint,
    ) -> bool {
        false
    }

    /// `Internal::AnalysisDone`.
    pub(crate) fn on_analysis_done(
        &mut self,
        server_id: String,
        track_id: TrackId,
        result: AnalysisResult,
    ) {
        if self.analysis.busy.as_ref() == Some(&(server_id.clone(), track_id.clone())) {
            self.analysis.busy = None;
        }
        match result {
            Ok(record) if self.server_id().as_deref() == Some(server_id.as_str()) => {
                tracing::info!(target: "hocket::automix", track = %track_id, bpm = ?record.bpm, confidence = record.beat_confidence, "analysed");
                self.store_analysis_record(track_id.clone(), (*record).clone());
                self.on_analysis_produced(&server_id, &track_id, &record);
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(target: "hocket::automix", track = %track_id, error = %e, "analysis failed");
            }
        }
        self.start_next_analysis();
    }

    /// A record this device produced was stored: share it through the
    /// coordinator (`AnalysisUpload`).
    pub(crate) fn on_analysis_produced(
        &mut self,
        server_id: &str,
        track_id: &TrackId,
        record: &AnalysisRecord,
    ) {
        self.share_analysis_record(server_id, track_id, record);
    }
}

#[cfg(feature = "automix-analysis")]
mod worker {
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Sender};

    use tokio::sync::mpsc::UnboundedSender;

    use crate::api::{AnalysisProducer, SourceFingerprint, TrackId};
    use crate::automix::analysis::Analyser;
    use crate::core::{ActorMsg, Internal};

    pub(crate) struct Job {
        pub server_id: String,
        pub track_id: TrackId,
        pub path: PathBuf,
        /// The library's fingerprint of the track: replaces the file's, so a
        /// record made from a transcoded cache file matches the track.
        pub fingerprint: SourceFingerprint,
    }

    /// The worker thread: one `Analyser` with one inference thread (~0.1 s
    /// of CPU per second of audio, ~480 MB while it runs), jobs in order.
    pub(crate) fn spawn(actor: UnboundedSender<ActorMsg>) -> Option<Sender<Job>> {
        let (tx, rx) = mpsc::channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("hocket-analysis".into())
            .spawn(move || {
                let mut analyser: Option<Analyser> = None;
                for job in rx {
                    let result = analyse(&mut analyser, &job);
                    let msg = ActorMsg::Internal(Internal::AnalysisDone {
                        server_id: job.server_id,
                        track_id: job.track_id,
                        result,
                    });
                    if actor.send(msg).is_err() {
                        break;
                    }
                }
            });
        match spawned {
            Ok(_) => Some(tx),
            Err(e) => {
                tracing::warn!(target: "hocket::automix", error = %e, "analysis worker");
                None
            }
        }
    }

    fn analyse(analyser: &mut Option<Analyser>, job: &Job) -> super::AnalysisResult {
        if analyser.is_none() {
            *analyser = Some(Analyser::new(1).map_err(|e| e.to_string())?);
        }
        let a = analyser.as_mut().expect("just set");
        let mut record = a
            .analyse_file(&job.path, AnalysisProducer::Desktop)
            .map_err(|e| e.to_string())?;
        record.source = job.fingerprint.clone();
        Ok(Box::new(crate::automix::record::compact(record)))
    }
}
