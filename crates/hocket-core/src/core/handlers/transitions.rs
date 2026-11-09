//! AutoMix in the actor (`docs/automix.md` → *Core behaviour*): the setting,
//! the record store, and the plans attached to the sources handed to the
//! backend. Playback (`playback.rs`) calls in here when it builds a source.

use crate::api::*;
use crate::automix::{
    late_edit_fade, plan_transition, should_mix, store, MixContext, MixTrack, PlanInput,
    PlanTrack, LOCK_IN_MS,
};
use crate::core::actor::Actor;
use crate::settings::keys;

/// What planning needs to know about one track.
pub(crate) struct MixFacts {
    pub track: Track,
    pub release_types: Vec<String>,
    pub record: Option<AnalysisRecord>,
}

impl Actor {
    // -- the setting -------------------------------------------------------------

    pub(crate) fn transition_settings(&self) -> TransitionSettings {
        self.settings
            .get_typed(keys::PLAYBACK_TRANSITIONS)
            .unwrap_or_default()
    }

    pub(crate) fn set_transition_settings(&mut self, settings: TransitionSettings) {
        match serde_json::to_value(&settings) {
            Ok(v) => self.set_setting_value(keys::PLAYBACK_TRANSITIONS, v),
            Err(e) => self.error(
                ErrorKind::Internal,
                "transition settings",
                Some(e.to_string()),
            ),
        }
    }

    // -- records -----------------------------------------------------------------

    /// The record the planner may use for `track`. Only the local store for
    /// now; records fetched from the coordinator will be written to the store
    /// first, so this stays the one place planning reads them from.
    pub(crate) fn analysis_record_for(&self, track: &Track) -> Option<AnalysisRecord> {
        match store::best_for(&self.db, track) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(target: "hocket::automix", error = %e, "read analysis record");
                None
            }
        }
    }

    /// `Command::StoreAnalysisRecord`: keep it if it is the best known, and
    /// replan the follow-up when it is about a track around the current one.
    pub(crate) fn store_analysis_record(&mut self, track_id: TrackId, record: AnalysisRecord) {
        let Some(server_id) = self.server_id() else {
            return;
        };
        let now = self.now();
        match store::put(&self.db, &server_id, &track_id, record, now) {
            Ok(true) => {
                tracing::debug!(target: "hocket::automix", track = %track_id, "analysis record stored");
                let near = self.playback.track_id() == Some(track_id.as_str())
                    || self
                        .upcoming_playable(2)
                        .iter()
                        .any(|i| i.track_id == track_id);
                if near {
                    self.replan_next();
                }
            }
            Ok(false) => {}
            Err(e) => self.error(ErrorKind::Storage, "analysis record", Some(e.to_string())),
        }
    }

    pub(crate) fn stored_analysis_record(&self, track_id: &str) -> Option<AnalysisRecord> {
        let server_id = self.server_id()?;
        store::get(&self.db, &server_id, track_id).ok().flatten()
    }

    // -- planning ----------------------------------------------------------------

    pub(crate) fn mix_facts(&self, track: &Track) -> MixFacts {
        let release_types = track
            .album_id
            .as_deref()
            .and_then(|id| self.db.album(id).ok().flatten())
            .map(|a| a.release_types)
            .unwrap_or_default();
        MixFacts {
            track: track.clone(),
            release_types,
            record: self.analysis_record_for(track),
        }
    }

    /// The rules' view of playback state.
    pub(crate) fn mix_context(&self) -> MixContext {
        MixContext {
            mode: self.transition_settings().mode,
            repeat_one: self.doc().is_some_and(|d| d.repeat == RepeatMode::One),
            sleep_end_of_track: self.sleep.state().is_some_and(|t| t.stop_at_end_of_track),
            cast_active: self.renderer.is_some(),
            exclusive_output: self.audio.exclusive,
        }
    }

    /// The plan from `a` into `b`, `None` when they are not mixed. `late`
    /// replaces a plan committed at `fixed` (lock-in) with a short fade from
    /// the same tail start.
    pub(crate) fn plan_between(
        &self,
        a: &MixFacts,
        b: &MixFacts,
        fixed: Option<Ms>,
        late: bool,
    ) -> Option<TransitionPlan> {
        let settings = self.transition_settings();
        if !settings.mode.is_on() || a.track.duration_ms == 0 || b.track.duration_ms == 0 {
            return None;
        }
        let ma = MixTrack::from_track(&a.track)
            .with_release_types(&a.release_types)
            .with_record(a.record.as_ref());
        let mb = MixTrack::from_track(&b.track)
            .with_release_types(&b.release_types)
            .with_record(b.record.as_ref());
        let decision = should_mix(&ma, &mb, &self.mix_context());
        if !decision.is_mix() {
            tracing::debug!(target: "hocket::automix", from = %a.track.id, to = %b.track.id, ?decision, "not mixed");
            return None;
        }
        let pa = PlanTrack {
            duration_ms: a.track.duration_ms,
            record: a.record.as_ref(),
        };
        let pb = PlanTrack {
            duration_ms: b.track.duration_ms,
            record: b.record.as_ref(),
        };
        match (late, fixed) {
            (true, Some(tail_start)) => Some(late_edit_fade(tail_start, pa, pb, &settings)),
            _ => plan_transition(&PlanInput {
                settings: &settings,
                a: pa,
                b: pb,
                fixed_tail_start_ms: fixed,
            }),
        }
    }

    /// `mix_out_ms` for a source of `track` followed by `following`.
    pub(crate) fn mix_out_for(&self, track: &Track, following: Option<&Track>) -> Option<Ms> {
        if !self.transition_settings().mode.is_on() {
            return None;
        }
        let following = following?;
        self.plan_between(
            &self.mix_facts(track),
            &self.mix_facts(following),
            None,
            false,
        )
        .map(|p| p.tail_start_ms)
    }

    /// The tail start the plan into the next track is committed to, once the
    /// current position is within [`LOCK_IN_MS`] of it: the committed plan's,
    /// else the loaded source's `mix_out_ms`.
    pub(crate) fn locked_tail_start(&self) -> Option<Ms> {
        let tail_start = self
            .playback
            .next
            .as_ref()
            .and_then(|n| n.mix_in.as_ref())
            .map(|p| p.tail_start_ms)
            .or(self.playback.mix_out_ms)?;
        let pos = self.playback.position_now(self.now());
        (pos.saturating_add(LOCK_IN_MS) >= tail_start).then_some(tail_start)
    }

    /// Attach the AutoMix fields to `source`, the follow-up of the loaded
    /// track: `mix_in` from the current track and its own `mix_out_ms` from
    /// what would follow it. `locked` is a committed tail start
    /// ([`Self::locked_tail_start`]): the plan is then a late-edit fade from it.
    pub(crate) fn with_mix(
        &self,
        mut source: MediaSource,
        next: &Track,
        following: Option<&Track>,
        locked: Option<Ms>,
    ) -> MediaSource {
        source.mix_in = None;
        source.mix_out_ms = None;
        if !self.transition_settings().mode.is_on() {
            return source;
        }
        let next_facts = self.mix_facts(next);
        if let Some(current) = self.playback.track.as_ref().filter(|_| self.playback.loaded) {
            let current = self.mix_facts(current);
            source.mix_in = match locked {
                Some(tail_start) => {
                    self.plan_between(&current, &next_facts, Some(tail_start), true)
                }
                None => self.plan_between(&current, &next_facts, self.playback.mix_out_ms, false),
            };
        }
        source.mix_out_ms = following.and_then(|f| {
            self.plan_between(&next_facts, &self.mix_facts(f), None, false)
                .map(|p| p.tail_start_ms)
        });
        source
    }

    /// The plan into the follow-up must be worked out again (the setting
    /// changed, a record arrived, the context of the rules changed). After
    /// lock-in an unchanged follow-up keeps its plan.
    pub(crate) fn replan_next(&mut self) {
        if !self.playback.loaded {
            return;
        }
        let mixed = self
            .playback
            .next
            .as_ref()
            .is_some_and(|n| n.mix_in.is_some() || n.mix_out_ms.is_some());
        if !self.transition_settings().mode.is_on() && !mixed {
            // Nothing planned and nothing to plan: Off stays exactly as it was.
            return;
        }
        self.refresh_next_with(true);
    }
}
