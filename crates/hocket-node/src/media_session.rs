//! Desktop `MediaSessionAdapter`: the playwire addon.
//!
//! The Electron main process feeds it `Event::MediaSession` states (as JSON)
//! and position ticks; playwire publishes them to MPRIS / SMTC / Now Playing
//! and calls back with user commands, which arrive here as JSON documents the
//! main process can dispatch straight into the core:
//!
//! ```json
//! { "kind": "command", "command": { "type": "mediaSessionCommand", "data": { "action": "next" } } }
//! { "kind": "raise" }
//! { "kind": "quit" }
//! { "kind": "openUri", "uri": "hocket://..." }
//! ```
//!
//! Platform notes (docs/design.md "OS media session"):
//! - Position is published on every tick; playwire diffs against its last
//!   snapshot, and MPRIS `Position` is not change-signalling anyway.
//! - Artwork must be a `file://` URL resolved through the image cache; a bare
//!   path is converted here.
//! - Windows ties SMTC to an HWND: pass the persistent hidden anchor window's
//!   handle in [`MediaSessionOptions::hwnd`], never the closable main window.
//! - `desktop_entry` must match the `.desktop` file name electron-builder
//!   produces (`hocket`), or MPRIS widgets show no icon.

use napi::bindgen_prelude::*;
use napi_derive::napi;
use serde::Serialize;

use hocket_core::api::{Command, MediaSessionAction, MediaSessionState};

use crate::JsonCallback;

/// Construction options. Only `name` is required.
#[napi(object)]
#[derive(Debug, Clone, Default)]
pub struct MediaSessionOptions {
    /// Human-readable player identity ("Hocket").
    pub name: String,
    /// Linux: `.desktop` file basename without extension.
    pub desktop_entry: Option<String>,
    /// Windows: HWND of a persistent hidden window (SMTC anchor).
    pub hwnd: Option<f64>,
    /// MPRIS track id prefix; defaults to `/app/hocket/track`.
    pub track_id_prefix: Option<String>,
}

/// Message delivered to the JS callback.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Outgoing {
    Command { command: Command },
    Raise,
    Quit,
    OpenUri { uri: String },
}

/// Whether the addon was built with the `media-session` feature.
#[napi]
pub fn media_session_available() -> bool {
    cfg!(feature = "media-session")
}

/// OS media session handle. One per process, owned by the main process.
#[napi]
pub struct MediaSession {
    inner: Option<backend::Inner>,
    last: MediaSessionState,
}

#[napi]
impl MediaSession {
    /// Attach to the OS media service. Fails with a descriptive error when the
    /// service is unavailable (no session bus, feature disabled, ...); the caller
    /// should log it and carry on without OS integration.
    #[napi(constructor)]
    pub fn new(options: MediaSessionOptions, callback: JsonCallback) -> Result<Self> {
        let inner = backend::attach(&options, callback)?;
        Ok(Self { inner: Some(inner), last: MediaSessionState::default() })
    }

    /// Publish a full `MediaSessionState` (JSON). Idempotent; playwire diffs.
    #[napi]
    pub fn set_state(&mut self, state_json: String) -> Result<()> {
        let state: MediaSessionState =
            serde_json::from_str(&state_json).map_err(|e| Error::from_reason(format!("bad MediaSessionState: {e}")))?;
        self.last = state;
        self.publish(None)
    }

    /// Republish the last state with an explicit position (one call per tick
    /// while playing). Cheap: no allocation beyond the playwire snapshot.
    #[napi]
    pub fn set_position(&mut self, position_ms: u32) -> Result<()> {
        self.last.position.position_ms = position_ms;
        self.publish(Some(position_ms))
    }

    /// Release the OS session (close-to-quit). Safe to call twice.
    #[napi]
    pub fn detach(&mut self) {
        if let Some(inner) = self.inner.as_mut() {
            inner.detach();
        }
        self.inner = None;
    }

    #[napi]
    pub fn is_attached(&self) -> bool {
        self.inner.is_some()
    }

    fn publish(&mut self, position_override: Option<u32>) -> Result<()> {
        let Some(inner) = self.inner.as_mut() else { return Ok(()) };
        inner.publish(&self.last, position_override)
    }
}

/// Translate a playwire event into the outgoing JSON document, using the
/// last published state to resolve toggles (play/pause, relative seek).
#[cfg(feature = "media-session")]
fn translate(event: &playwire::Event, snapshot: &Snapshot) -> Option<Outgoing> {
    use playwire::Event as E;
    let cmd = |action: MediaSessionAction, value: Option<f64>| Some(Outgoing::Command { command: Command::MediaSessionCommand { action, value } });
    match event {
        E::Play => cmd(MediaSessionAction::Play, None),
        E::Pause => cmd(MediaSessionAction::Pause, None),
        E::PlayPause => cmd(if snapshot.playing { MediaSessionAction::Pause } else { MediaSessionAction::Play }, None),
        E::Stop => cmd(MediaSessionAction::Stop, None),
        E::Next => cmd(MediaSessionAction::Next, None),
        E::Previous => cmd(MediaSessionAction::Previous, None),
        E::SeekTo(d) => cmd(MediaSessionAction::Seek, Some(d.as_secs_f64() * 1000.0)),
        E::SeekBy(secs) => {
            let target = (snapshot.position_ms as f64 + secs * 1000.0).max(0.0);
            cmd(MediaSessionAction::Seek, Some(target))
        }
        E::SetVolume(v) => Some(Outgoing::Command { command: Command::SetVolume { volume: v.clamp(0.0, 1.0) } }),
        E::SetShuffle(on) => cmd(MediaSessionAction::Shuffle, Some(if *on { 1.0 } else { 0.0 })),
        E::SetRepeat(r) => cmd(
            MediaSessionAction::Repeat,
            Some(match r {
                playwire::Repeat::Off => 0.0,
                playwire::Repeat::All => 1.0,
                playwire::Repeat::One => 2.0,
            }),
        ),
        E::OpenUri(uri) => Some(Outgoing::OpenUri { uri: uri.clone() }),
        E::Raise => Some(Outgoing::Raise),
        E::Quit => Some(Outgoing::Quit),
        _ => None,
    }
}

/// What the callback thread needs to know about the last published state.
#[derive(Debug, Clone, Copy, Default)]
struct Snapshot {
    playing: bool,
    position_ms: u32,
}

/// Turn a cache path into a `file://` URL; pass URLs through untouched.
fn artwork_url(path: &str) -> String {
    if path.is_empty() || path.contains("://") {
        return path.to_string();
    }
    let p = std::path::Path::new(path);
    if p.is_absolute() {
        let mut s = String::from("file://");
        if !path.starts_with('/') {
            s.push('/');
        }
        s.push_str(&path.replace('\\', "/"));
        s
    } else {
        path.to_string()
    }
}

#[cfg(feature = "media-session")]
mod backend {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use napi::bindgen_prelude::*;
    use napi::threadsafe_function::ThreadsafeFunctionCallMode;

    use super::{artwork_url, translate, MediaSessionOptions, Snapshot};
    use hocket_core::api::{MediaSessionAction, MediaSessionState, RepeatMode};

    pub(super) struct Inner {
        controls: playwire::MediaControls,
        snapshot: Arc<Mutex<Snapshot>>,
    }

    pub(super) fn attach(options: &MediaSessionOptions, callback: crate::JsonCallback) -> Result<Inner> {
        let mut config = playwire::PlayerConfig::new(options.name.clone())
            .track_id_prefix(options.track_id_prefix.clone().unwrap_or_else(|| "/app/hocket/track".to_string()))
            .supported_uri_schemes(vec!["hocket".to_string(), "http".to_string(), "https".to_string()]);
        if let Some(entry) = &options.desktop_entry {
            config = config.desktop_entry(entry.clone());
        }
        if let Some(hwnd) = options.hwnd {
            if hwnd.is_finite() && hwnd >= 0.0 {
                config = config.hwnd(hwnd as u64);
            }
        }
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let snap = Arc::clone(&snapshot);
        let controls = playwire::MediaControls::new(config, move |event| {
            let current = snap.lock().map(|s| *s).unwrap_or_default();
            if let Some(out) = translate(&event, &current) {
                match serde_json::to_string(&out) {
                    Ok(json) => {
                        callback.call(json, ThreadsafeFunctionCallMode::NonBlocking);
                    }
                    Err(e) => tracing::error!(target: "hocket_node::media_session", "serialise failed: {e}"),
                }
            }
        })
        .map_err(|e| Error::from_reason(format!("media session unavailable: {e}")))?;
        Ok(Inner { controls, snapshot })
    }

    impl Inner {
        pub(super) fn publish(&mut self, state: &MediaSessionState, position_override: Option<u32>) -> Result<()> {
            let position_ms = position_override.unwrap_or(state.position.position_ms);
            if let Ok(mut s) = self.snapshot.lock() {
                *s = Snapshot { playing: state.is_playing, position_ms };
            }
            let track = state.metadata.as_ref().map(|m| playwire::Track {
                id: m.track_id.clone().unwrap_or_else(|| "none".to_string()),
                title: m.title.clone(),
                artists: m.artist.iter().cloned().collect(),
                album: m.album.clone().unwrap_or_default(),
                artwork_url: m.artwork_path.as_deref().map(artwork_url).unwrap_or_default(),
                url: String::new(),
            });
            let duration = state.metadata.as_ref().map(|m| Duration::from_millis(u64::from(m.duration_ms)));
            let has = |a: MediaSessionAction| state.actions.contains(&a);
            let playback = playwire::PlaybackState {
                track,
                playing: state.is_playing,
                position: Duration::from_millis(u64::from(position_ms)),
                duration,
                volume: if state.volume.is_finite() { state.volume.clamp(0.0, 1.0) } else { 1.0 },
                repeat: match state.repeat {
                    RepeatMode::Off => playwire::Repeat::Off,
                    RepeatMode::All => playwire::Repeat::All,
                    RepeatMode::One => playwire::Repeat::One,
                },
                shuffle: state.shuffle,
                capabilities: playwire::Capabilities {
                    can_go_next: has(MediaSessionAction::Next),
                    can_go_previous: has(MediaSessionAction::Previous),
                    can_seek: has(MediaSessionAction::Seek),
                },
            };
            self.controls.set_state(&playback).map_err(|e| Error::from_reason(format!("media session publish: {e}")))
        }

        pub(super) fn detach(&mut self) {
            self.controls.detach();
        }
    }
}

#[cfg(not(feature = "media-session"))]
mod backend {
    use napi::bindgen_prelude::*;

    use super::MediaSessionOptions;
    use hocket_core::api::MediaSessionState;

    pub(super) struct Inner;

    pub(super) fn attach(_options: &MediaSessionOptions, _callback: crate::JsonCallback) -> Result<Inner> {
        Err(Error::from_reason("media session unavailable: addon built without the `media-session` feature"))
    }

    impl Inner {
        pub(super) fn publish(&mut self, _state: &MediaSessionState, _position: Option<u32>) -> Result<()> {
            Ok(())
        }
        pub(super) fn detach(&mut self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_paths_become_file_urls() {
        assert_eq!(artwork_url("/tmp/a.jpg"), "file:///tmp/a.jpg");
        assert_eq!(artwork_url("file:///tmp/a.jpg"), "file:///tmp/a.jpg");
        assert_eq!(artwork_url(""), "");
        assert_eq!(artwork_url("https://x/y.png"), "https://x/y.png");
    }

    #[cfg(feature = "media-session")]
    #[test]
    fn play_pause_resolves_from_last_state() {
        let playing = Snapshot { playing: true, position_ms: 10_000 };
        match translate(&playwire::Event::PlayPause, &playing) {
            Some(Outgoing::Command { command: Command::MediaSessionCommand { action, .. } }) => assert_eq!(action, MediaSessionAction::Pause),
            other => panic!("unexpected {other:?}"),
        }
        match translate(&playwire::Event::SeekBy(-15.0), &playing) {
            Some(Outgoing::Command { command: Command::MediaSessionCommand { action, value } }) => {
                assert_eq!(action, MediaSessionAction::Seek);
                assert_eq!(value, Some(0.0));
            }
            other => panic!("unexpected {other:?}"),
        }
        let json = serde_json::to_string(&translate(&playwire::Event::Next, &playing).unwrap()).unwrap();
        assert_eq!(json, r#"{"kind":"command","command":{"type":"mediaSessionCommand","data":{"action":"next","value":null}}}"#);
    }
}
