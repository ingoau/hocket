//! The `MediaSessionAdapter` seam and the state derivation behind it.
//!
//! The OS media session (Media3 on Android, the playwire addon on desktop:
//! MPRIS, SMTC, MPNowPlayingInfoCenter) is fed by whichever device currently
//! owns transport — and also by devices that don't, so a laptop's media keys
//! can drive the phone that is playing. The adapter is one-way out
//! ([`MediaSessionAdapter::set_state`]); commands come back in as
//! [`crate::api::Command::MediaSessionCommand`] which the actor maps with
//! [`command_for`].
//!
//! [`derive_media_session_state`] is pure: it takes the queue view, the
//! transport, the clock and the customised action list and produces the
//! [`MediaSessionState`] the platform publishes. Position is a stamp
//! `(position, taken_at, rate, is_playing)`, never a timer — the platform
//! extrapolates, exactly like a Connect peer would.
//!
//! Platform specifics for the desktop addon are in `PLATFORM_NOTES.md` next
//! to this file.

use crate::api::{
    Command, MediaSessionAction, MediaSessionMetadata, MediaSessionState, PositionStamp, QueueView,
    RatingTarget, RepeatMode, TransportState,
};

/// Platform side of the seam: publish state, nothing else. Media3 and the
/// playwire addon implement this; the actor calls it on every relevant
/// change (track, play/pause, seek, shuffle/repeat, volume, position tick).
pub trait MediaSessionAdapter: Send + Sync {
    fn set_state(&self, state: MediaSessionState);
}

impl<F: Fn(MediaSessionState) + Send + Sync> MediaSessionAdapter for F {
    fn set_state(&self, state: MediaSessionState) {
        self(state)
    }
}

/// An adapter that drops everything (coordinator, tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct NullMediaSession;

impl MediaSessionAdapter for NullMediaSession {
    fn set_state(&self, _state: MediaSessionState) {}
}

/// The default button set when the user hasn't customised the
/// `mediaSession` surface: transport plus seek, shuffle, repeat and love,
/// which every platform can show (MPRIS as properties, Media3 as custom
/// commands, SMTC as the standard buttons).
pub const DEFAULT_ACTIONS: [MediaSessionAction; 9] = [
    MediaSessionAction::Play,
    MediaSessionAction::Pause,
    MediaSessionAction::Previous,
    MediaSessionAction::Next,
    MediaSessionAction::Seek,
    MediaSessionAction::Stop,
    MediaSessionAction::Shuffle,
    MediaSessionAction::Repeat,
    MediaSessionAction::Love,
];

/// Actions that are always available while something is loaded, regardless
/// of customisation, because the OS assumes them (headset play/pause, stop).
const MANDATORY_ACTIONS: [MediaSessionAction; 3] = [
    MediaSessionAction::Play,
    MediaSessionAction::Pause,
    MediaSessionAction::Stop,
];

/// Extrapolate a stamp to `now`, clamped to `duration_ms` when known.
pub fn extrapolate_position(stamp: &PositionStamp, now_ms: f64, duration_ms: Option<u32>) -> u32 {
    let mut pos = f64::from(stamp.position_ms);
    if stamp.is_playing && stamp.rate > 0.0 && now_ms > stamp.taken_at {
        pos += (now_ms - stamp.taken_at) * stamp.rate;
    }
    let pos = pos.max(0.0);
    let pos = match duration_ms {
        Some(d) if d > 0 => pos.min(f64::from(d)),
        _ => pos,
    };
    pos.min(f64::from(u32::MAX)) as u32
}

/// Build the state the platform publishes.
///
/// - `queue` supplies the current entry (metadata), shuffle and repeat.
/// - `transport` supplies the stamp and volume. The stamp is re-based to
///   `now_ms` so the published `taken_at` is fresh and the platform's own
///   extrapolation starts from a current value.
/// - `artwork_path` is a local `file://` path the actor resolved through the
///   image cache (never a remote URL, see the platform notes).
/// - `owns_transport` says whether this device is the one playing.
/// - `actions` is the customised action list for the `mediaSession`
///   surface; `None` uses [`DEFAULT_ACTIONS`]. Play/Pause/Stop are always
///   present while something is loaded; nothing is offered when the queue
///   is empty.
pub fn derive_media_session_state(
    queue: &QueueView,
    transport: &TransportState,
    now_ms: f64,
    artwork_path: Option<String>,
    owns_transport: bool,
    actions: Option<Vec<MediaSessionAction>>,
) -> MediaSessionState {
    let metadata = queue.current.as_ref().map(|entry| {
        let t = &entry.track;
        MediaSessionMetadata {
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_ms: t.duration_ms,
            artwork_path: artwork_path.clone(),
            track_id: Some(t.id.clone()),
            loved: t.loved,
            rating: t.rating.min(5),
        }
    });
    let duration = metadata.as_ref().map(|m| m.duration_ms);
    let is_playing = metadata.is_some() && transport.position.is_playing;
    let position = PositionStamp {
        position_ms: if metadata.is_some() {
            extrapolate_position(&transport.position, now_ms, duration)
        } else {
            0
        },
        taken_at: now_ms,
        rate: if is_playing {
            if transport.position.rate > 0.0 {
                transport.position.rate
            } else {
                1.0
            }
        } else {
            0.0
        },
        is_playing,
    };
    let actions = if metadata.is_none() {
        Vec::new()
    } else {
        let mut list = actions.unwrap_or_else(|| DEFAULT_ACTIONS.to_vec());
        for m in MANDATORY_ACTIONS {
            if !list.contains(&m) {
                list.push(m);
            }
        }
        dedup_in_order(list)
    };
    MediaSessionState {
        metadata,
        is_playing,
        position,
        shuffle: queue.shuffle,
        repeat: queue.repeat,
        volume: if transport.volume.is_finite() {
            transport.volume.clamp(0.0, 1.0)
        } else {
            1.0
        },
        actions,
        owns_transport,
    }
}

fn dedup_in_order(list: Vec<MediaSessionAction>) -> Vec<MediaSessionAction> {
    let mut out: Vec<MediaSessionAction> = Vec::with_capacity(list.len());
    for a in list {
        if !out.contains(&a) {
            out.push(a);
        }
    }
    out
}

/// Map an incoming OS action to the [`Command`] the actor runs.
///
/// - `Seek` carries the target position in milliseconds in `value`.
/// - `Rate` carries 0–5 in `value` (Subsonic integer stars; anything else is
///   clamped and rounded).
/// - `Shuffle` with `value` `0`/`1` sets, without a value toggles.
/// - `Repeat` with `value` `0`/`1`/`2` sets Off/All/One, without cycles
///   Off → All → One → Off.
/// - `Love` toggles the current track's loved flag.
/// - Returns `None` when the action needs a current track and there is
///   none, or `Seek`/`Rate` arrive without a value.
pub fn command_for(
    action: MediaSessionAction,
    value: Option<f64>,
    current: &MediaSessionState,
) -> Option<Command> {
    let track_id = || current.metadata.as_ref().and_then(|m| m.track_id.clone());
    match action {
        MediaSessionAction::Play => Some(Command::Play),
        MediaSessionAction::Pause => Some(Command::Pause),
        MediaSessionAction::Stop => Some(Command::Stop),
        MediaSessionAction::Next => Some(Command::Next),
        MediaSessionAction::Previous => Some(Command::Previous),
        MediaSessionAction::Seek => {
            let v = value.filter(|v| v.is_finite())?;
            Some(Command::SeekTo {
                position_ms: v.max(0.0).min(f64::from(u32::MAX)) as u32,
            })
        }
        MediaSessionAction::Shuffle => {
            let enabled = match value {
                Some(v) if v.is_finite() => v != 0.0,
                _ => !current.shuffle,
            };
            Some(Command::SetShuffle { enabled })
        }
        MediaSessionAction::Repeat => {
            let mode = match value.filter(|v| v.is_finite()).map(|v| v.round() as i64) {
                Some(0) => RepeatMode::Off,
                Some(1) => RepeatMode::All,
                Some(2) => RepeatMode::One,
                _ => match current.repeat {
                    RepeatMode::Off => RepeatMode::All,
                    RepeatMode::All => RepeatMode::One,
                    RepeatMode::One => RepeatMode::Off,
                },
            };
            Some(Command::SetRepeat { mode })
        }
        MediaSessionAction::Love => {
            let id = track_id()?;
            let loved = match value {
                Some(v) if v.is_finite() => v != 0.0,
                _ => !current.metadata.as_ref().map(|m| m.loved).unwrap_or(false),
            };
            Some(Command::SetLoved {
                targets: vec![RatingTarget::Track { id }],
                loved,
            })
        }
        MediaSessionAction::Rate => {
            let id = track_id()?;
            let v = value.filter(|v| v.is_finite())?;
            Some(Command::SetRating {
                targets: vec![RatingTarget::Track { id }],
                rating: v.round().clamp(0.0, 5.0) as u32,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{QueueEntry, QueueItem, QueueSource, TrackSummary};

    fn queue(playing: bool) -> QueueView {
        let mut q = QueueView {
            shuffle: true,
            repeat: RepeatMode::All,
            ..Default::default()
        };
        if playing {
            q.current = Some(QueueEntry {
                item: QueueItem {
                    key: "k1".into(),
                    track_id: "t1".into(),
                    source: QueueSource::Inserted,
                    unavailable: false,
                },
                track: TrackSummary {
                    id: "t1".into(),
                    title: "Song".into(),
                    artist: Some("Band".into()),
                    album: Some("Album".into()),
                    duration_ms: 200_000,
                    loved: true,
                    rating: 7,
                    ..Default::default()
                },
            });
        }
        q
    }

    fn transport(is_playing: bool) -> TransportState {
        TransportState {
            position: PositionStamp {
                position_ms: 10_000,
                taken_at: 1_000.0,
                rate: 1.0,
                is_playing,
            },
            volume: 0.8,
            ..Default::default()
        }
    }

    #[test]
    fn derives_metadata_and_a_fresh_position_stamp() {
        let s = derive_media_session_state(
            &queue(true),
            &transport(true),
            6_000.0,
            Some("file:///art.jpg".into()),
            true,
            None,
        );
        let m = s.metadata.as_ref().unwrap();
        assert_eq!(m.title, "Song");
        assert_eq!(m.artwork_path.as_deref(), Some("file:///art.jpg"));
        assert_eq!(m.rating, 5, "ratings clamp to 5");
        assert!(m.loved);
        assert!(s.is_playing);
        assert_eq!(s.position.position_ms, 15_000, "extrapolated 5 s at rate 1");
        assert_eq!(s.position.taken_at, 6_000.0);
        assert_eq!(s.position.rate, 1.0);
        assert!(s.shuffle);
        assert_eq!(s.repeat, RepeatMode::All);
        assert_eq!(s.volume, 0.8);
        assert!(s.owns_transport);
        assert_eq!(s.actions, DEFAULT_ACTIONS.to_vec());
    }

    #[test]
    fn paused_stamp_does_not_advance_and_clamps_to_duration() {
        let s = derive_media_session_state(
            &queue(true),
            &transport(false),
            999_999.0,
            None,
            false,
            None,
        );
        assert!(!s.is_playing);
        assert_eq!(s.position.position_ms, 10_000);
        assert_eq!(s.position.rate, 0.0);
        let s = derive_media_session_state(
            &queue(true),
            &transport(true),
            10_000_000.0,
            None,
            false,
            None,
        );
        assert_eq!(s.position.position_ms, 200_000);
        assert_eq!(
            extrapolate_position(
                &PositionStamp {
                    position_ms: 5,
                    taken_at: 10.0,
                    rate: 1.0,
                    is_playing: true
                },
                5.0,
                None
            ),
            5
        );
    }

    #[test]
    fn empty_queue_publishes_nothing_actionable() {
        let s = derive_media_session_state(&queue(false), &transport(true), 0.0, None, true, None);
        assert!(s.metadata.is_none());
        assert!(!s.is_playing);
        assert!(s.actions.is_empty());
        assert_eq!(
            s,
            MediaSessionState {
                volume: 0.8,
                shuffle: true,
                repeat: RepeatMode::All,
                owns_transport: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn customised_actions_keep_order_and_add_mandatory_ones() {
        let custom = vec![
            MediaSessionAction::Next,
            MediaSessionAction::Love,
            MediaSessionAction::Next,
            MediaSessionAction::Rate,
        ];
        let s = derive_media_session_state(
            &queue(true),
            &transport(true),
            0.0,
            None,
            true,
            Some(custom),
        );
        assert_eq!(
            s.actions,
            vec![
                MediaSessionAction::Next,
                MediaSessionAction::Love,
                MediaSessionAction::Rate,
                MediaSessionAction::Play,
                MediaSessionAction::Pause,
                MediaSessionAction::Stop
            ]
        );
    }

    #[test]
    fn commands_for_actions() {
        let st = derive_media_session_state(&queue(true), &transport(true), 0.0, None, true, None);
        assert_eq!(
            command_for(MediaSessionAction::Play, None, &st),
            Some(Command::Play)
        );
        assert_eq!(
            command_for(MediaSessionAction::Pause, None, &st),
            Some(Command::Pause)
        );
        assert_eq!(
            command_for(MediaSessionAction::Stop, None, &st),
            Some(Command::Stop)
        );
        assert_eq!(
            command_for(MediaSessionAction::Next, None, &st),
            Some(Command::Next)
        );
        assert_eq!(
            command_for(MediaSessionAction::Previous, None, &st),
            Some(Command::Previous)
        );
        assert_eq!(
            command_for(MediaSessionAction::Seek, Some(42_000.4), &st),
            Some(Command::SeekTo {
                position_ms: 42_000
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Seek, Some(-5.0), &st),
            Some(Command::SeekTo { position_ms: 0 })
        );
        assert_eq!(command_for(MediaSessionAction::Seek, None, &st), None);
        assert_eq!(
            command_for(MediaSessionAction::Shuffle, None, &st),
            Some(Command::SetShuffle { enabled: false })
        );
        assert_eq!(
            command_for(MediaSessionAction::Shuffle, Some(1.0), &st),
            Some(Command::SetShuffle { enabled: true })
        );
        assert_eq!(
            command_for(MediaSessionAction::Repeat, None, &st),
            Some(Command::SetRepeat {
                mode: RepeatMode::One
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Repeat, Some(0.0), &st),
            Some(Command::SetRepeat {
                mode: RepeatMode::Off
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Repeat, Some(2.0), &st),
            Some(Command::SetRepeat {
                mode: RepeatMode::One
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Love, None, &st),
            Some(Command::SetLoved {
                targets: vec![RatingTarget::Track { id: "t1".into() }],
                loved: false
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Rate, Some(9.0), &st),
            Some(Command::SetRating {
                targets: vec![RatingTarget::Track { id: "t1".into() }],
                rating: 5
            })
        );
        assert_eq!(
            command_for(MediaSessionAction::Rate, Some(2.4), &st),
            Some(Command::SetRating {
                targets: vec![RatingTarget::Track { id: "t1".into() }],
                rating: 2
            })
        );
        assert_eq!(command_for(MediaSessionAction::Rate, None, &st), None);
        let empty = MediaSessionState::default();
        assert_eq!(command_for(MediaSessionAction::Love, None, &empty), None);
        assert_eq!(
            command_for(MediaSessionAction::Rate, Some(3.0), &empty),
            None
        );
        assert_eq!(
            command_for(MediaSessionAction::Repeat, None, &empty),
            Some(Command::SetRepeat {
                mode: RepeatMode::All
            })
        );
    }

    #[test]
    fn adapter_closures_work() {
        let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let adapter: Box<dyn MediaSessionAdapter> =
            Box::new(move |st: MediaSessionState| s2.lock().push(st));
        adapter.set_state(MediaSessionState::default());
        assert_eq!(seen.lock().len(), 1);
        NullMediaSession.set_state(MediaSessionState::default());
    }
}
