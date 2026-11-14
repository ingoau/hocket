//! A song preview beside the queue.
//!
//! The preview player is a second output. On this device, whatever is
//! playing is paused first and given back at the same position when the
//! preview stops. A Connect peer is not touched: the preview is local.
//! The session never reaches the queue, the scrobbler, play history or the
//! saved resume position.

use crate::api::*;
use crate::core::actor::Actor;
use crate::core::state::{PreviewHold, PreviewSession};

/// Where a preview starts: past the intro.
pub(crate) const PREVIEW_START: f64 = 0.30;

/// Position a preview opens at. Unknown durations start at the top and are
/// moved once the player reports a duration.
pub(crate) fn preview_start_ms(duration_ms: Ms) -> Ms {
    if duration_ms == 0 {
        0
    } else {
        (f64::from(duration_ms) * PREVIEW_START) as Ms
    }
}

/// Transport commands that take the output back from a preview.
pub(crate) fn command_ends_preview(cmd: &Command) -> bool {
    matches!(
        cmd,
        Command::Play
            | Command::Pause
            | Command::TogglePlay
            | Command::Stop
            | Command::Next
            | Command::Previous
            | Command::PreviousTrack
            | Command::SeekTo { .. }
            | Command::SeekBy { .. }
            | Command::PlayContext { .. }
            | Command::PlayTracks { .. }
            | Command::JumpToQueueItem { .. }
            | Command::RestoreSavedQueue { .. }
            | Command::ResumeHere
    )
}

impl Actor {
    pub(crate) fn preview_start(&mut self, track_id: TrackId) {
        if self
            .preview
            .as_ref()
            .is_some_and(|p| p.track_id == track_id)
        {
            self.preview_toggle();
            return;
        }
        let Some(track) = self.track_or_bare(&self.track_ref_of(&track_id)) else {
            return;
        };
        let duration_ms = track.duration_ms;
        let position_ms = preview_start_ms(duration_ms);
        let key = format!("preview:{track_id}");
        let Some(source) = self.media_source_for(&key, &track) else {
            self.log("debug", format!("preview: no source for {track_id}"));
            return;
        };
        // A second preview keeps the hold from the first: main stays paused
        // until every preview has stopped.
        let hold = match self.preview.as_ref().and_then(|p| p.hold.clone()) {
            Some(hold) => Some(hold),
            None => self.hold_for_preview(),
        };
        let title = track.title.clone();
        // Installed before the load: a backend that reports inline must find
        // the session already there.
        self.preview = Some(PreviewSession {
            track_id: track_id.clone(),
            title,
            duration_ms,
            position_ms,
            playing: true,
            hold,
            positioned: duration_ms > 0,
        });
        if let Err(e) = self.backend.preview_load(source, position_ms) {
            self.log("debug", format!("preview load: {e}"));
            self.toast("Couldn't preview this song", None);
            let hold = self.preview.take().and_then(|p| p.hold);
            self.emit_preview();
            self.restore_hold(hold);
            return;
        }
        self.emit_preview();
    }

    pub(crate) fn preview_toggle(&mut self) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let playing = !preview.playing;
        preview.playing = playing;
        let r = if playing {
            self.backend.preview_play()
        } else {
            self.backend.preview_pause()
        };
        if let Err(e) = r {
            self.log("debug", format!("preview toggle: {e}"));
        }
        self.emit_preview();
    }

    pub(crate) fn preview_seek(&mut self, position_ms: Ms) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let cap = preview.duration_ms;
        let position_ms = if cap > 0 {
            position_ms.min(cap)
        } else {
            position_ms
        };
        preview.position_ms = position_ms;
        preview.positioned = true;
        if let Err(e) = self.backend.preview_seek(position_ms) {
            self.log("debug", format!("preview seek: {e}"));
        }
        self.emit_preview();
    }

    /// Stop the preview. `restore` gives this device's playback back when
    /// the preview had paused it.
    pub(crate) fn stop_preview(&mut self, restore: bool) {
        let Some(session) = self.preview.take() else {
            return;
        };
        if let Err(e) = self.backend.preview_stop() {
            self.log("debug", format!("preview stop: {e}"));
        }
        self.emit_preview();
        if restore {
            self.restore_hold(session.hold);
        }
    }

    /// Main playback was stopped on purpose during the preview: closing the
    /// preview must leave it stopped.
    pub(crate) fn drop_preview_hold(&mut self) {
        if let Some(hold) = self.preview.as_mut().and_then(|p| p.hold.as_mut()) {
            hold.was_playing = false;
        }
    }

    pub(crate) fn on_preview_ready(&mut self, duration_ms: Option<Ms>) {
        let Some(duration_ms) = duration_ms.filter(|d| *d > 0) else {
            return;
        };
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let seek = !preview.positioned;
        if preview.duration_ms == 0 {
            preview.duration_ms = duration_ms;
        }
        if seek {
            let position_ms = preview_start_ms(preview.duration_ms);
            preview.position_ms = position_ms;
            preview.positioned = true;
            if let Err(e) = self.backend.preview_seek(position_ms) {
                self.log("debug", format!("preview seek: {e}"));
            }
        }
        self.emit_preview();
    }

    pub(crate) fn on_preview_progress(&mut self, position_ms: Ms, playing: bool) {
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let cap = preview.duration_ms;
        preview.position_ms = if cap > 0 {
            position_ms.min(cap)
        } else {
            position_ms
        };
        preview.playing = playing;
        if cap > 0 && preview.position_ms >= cap {
            self.stop_preview(true);
            return;
        }
        self.emit_preview();
    }

    pub(crate) fn emit_preview(&mut self) {
        let now = self.now();
        let preview = self.preview.as_ref().map(|p| PreviewState {
            track_id: p.track_id.clone(),
            title: p.title.clone(),
            duration_ms: p.duration_ms,
            position_ms: p.position_ms,
            playing: p.playing,
            taken_at: now,
        });
        self.emit(Event::PreviewChanged { preview });
    }

    /// Pause this device, without writing the resume position, so the
    /// preview can be heard alone. A peer that owns playback is not paused.
    fn hold_for_preview(&mut self) -> Option<PreviewHold> {
        if !self.owns_transport() {
            return None;
        }
        // `want_playing` covers a load or a skip that has not become audible
        // yet. Buffering and a pending load are also set when the user has
        // paused, so they must not count or the preview would start the track.
        let was_playing =
            self.playback.playing || self.playback.focus_suspended || self.playback.want_playing;
        if was_playing {
            self.playback.want_playing = false;
            if self.playback.playing || self.playback.focus_suspended {
                self.set_playing_inner(false, false);
            } else if let Err(e) = self.backend.pause() {
                // Not audible yet (still loading). Pause the backend so it
                // does not start under the preview.
                self.log("debug", format!("preview pause: {e}"));
            }
        }
        Some(PreviewHold { was_playing })
    }

    fn restore_hold(&mut self, hold: Option<PreviewHold>) {
        let Some(hold) = hold else {
            return;
        };
        if hold.was_playing && self.owns_transport() {
            self.playback.want_playing = true;
            if self.playback.loaded {
                self.set_playing(true);
            } else if let Err(e) = self.backend.play() {
                self.log("debug", format!("preview resume: {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_opens_thirty_percent_in() {
        assert_eq!(preview_start_ms(0), 0);
        assert_eq!(preview_start_ms(100_000), 30_000);
        assert_eq!(preview_start_ms(10), 3);
    }
}
