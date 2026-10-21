//! A [`PlaybackBackend`] that logs every command the core gives the player
//! before passing it on, so a diagnostics bundle shows what the player was
//! told and when: a load (where its audio comes from, at what position,
//! playing or not), play, pause, stop, seek. Debug level, target
//! `hocket::player`, captured while developer mode is on.
//!
//! Volume is left out: a slider drag sends dozens. Nothing logged carries a
//! URL, only what kind of source it is, so no stream token or credential
//! reaches a log.

use std::sync::Arc;

use crate::api::{MediaSource, OutputDevice};
use crate::audio::backend::{BackendError, PlaybackBackend};
use crate::core::stream_reader::STREAM_URL_PREFIX;

const TARGET: &str = "hocket::player";

/// See the module docs.
pub struct LoggedBackend {
    inner: Arc<dyn PlaybackBackend>,
}

impl LoggedBackend {
    pub fn new(inner: Arc<dyn PlaybackBackend>) -> Self {
        Self { inner }
    }
}

/// Where a source's audio comes from, by its URL's form.
pub fn source_kind(url: &str) -> &'static str {
    if url.starts_with("file:") {
        "download"
    } else if url.starts_with(STREAM_URL_PREFIX) {
        "core stream"
    } else {
        "direct URL"
    }
}

/// `key (track) from kind`, for a log line.
pub fn describe_source(source: &MediaSource) -> String {
    let transcoded = if source.transcoded {
        ", transcoded"
    } else {
        ""
    };
    format!(
        "{} ({}) from {}{transcoded}",
        source.key,
        source.track.id,
        source_kind(&source.url)
    )
}

fn logged(what: impl FnOnce() -> String, r: Result<(), BackendError>) -> Result<(), BackendError> {
    match &r {
        Ok(()) => tracing::debug!(target: TARGET, "{}", what()),
        Err(e) => tracing::debug!(target: TARGET, "{} failed: {e}", what()),
    }
    r
}

impl PlaybackBackend for LoggedBackend {
    fn load(
        &self,
        source: MediaSource,
        next: Option<MediaSource>,
        position_ms: u32,
        play: bool,
    ) -> Result<(), BackendError> {
        let what = format!(
            "load {} at {position_ms} ms, {}; next {}",
            describe_source(&source),
            if play { "playing" } else { "paused" },
            next.as_ref().map(|n| n.key.as_str()).unwrap_or("none"),
        );
        logged(|| what, self.inner.load(source, next, position_ms, play))
    }

    fn set_next(&self, next: Option<MediaSource>) -> Result<(), BackendError> {
        let what = format!(
            "next {}",
            next.as_ref()
                .map(describe_source)
                .unwrap_or_else(|| "none".into())
        );
        logged(|| what, self.inner.set_next(next))
    }

    fn play(&self) -> Result<(), BackendError> {
        logged(|| "play".into(), self.inner.play())
    }

    fn pause(&self) -> Result<(), BackendError> {
        logged(|| "pause".into(), self.inner.pause())
    }

    fn stop(&self) -> Result<(), BackendError> {
        logged(|| "stop".into(), self.inner.stop())
    }

    fn seek(&self, position_ms: u32) -> Result<(), BackendError> {
        logged(
            || format!("seek to {position_ms} ms"),
            self.inner.seek(position_ms),
        )
    }

    fn set_volume(&self, volume: f64) -> Result<(), BackendError> {
        self.inner.set_volume(volume)
    }

    fn set_system_volume(&self, volume: f64, muted: bool) -> Result<(), BackendError> {
        self.inner.set_system_volume(volume, muted)
    }

    fn pre_buffer(&self, source: MediaSource, position_ms: u32) -> Result<(), BackendError> {
        let what = format!(
            "pre-buffer {} at {position_ms} ms",
            describe_source(&source)
        );
        logged(|| what, self.inner.pre_buffer(source, position_ms))
    }

    fn discard_pre_buffer(&self) -> Result<(), BackendError> {
        logged(
            || "discard pre-buffer".into(),
            self.inner.discard_pre_buffer(),
        )
    }

    fn set_gapless(&self, enabled: bool) -> Result<(), BackendError> {
        logged(
            || format!("gapless {}", if enabled { "on" } else { "off" }),
            self.inner.set_gapless(enabled),
        )
    }

    fn set_output_device(&self, id: Option<String>) -> Result<(), BackendError> {
        let what = format!("output device {}", id.as_deref().unwrap_or("default"));
        logged(|| what, self.inner.set_output_device(id))
    }

    fn output_devices(&self) -> Vec<OutputDevice> {
        self.inner.output_devices()
    }

    fn set_exclusive(&self, exclusive: bool) -> Result<(), BackendError> {
        logged(
            || format!("exclusive output {}", if exclusive { "on" } else { "off" }),
            self.inner.set_exclusive(exclusive),
        )
    }

    fn name(&self) -> &'static str {
        self.inner.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::TrackSummary;
    use parking_lot::Mutex;

    fn source(key: &str, url: &str) -> MediaSource {
        MediaSource {
            key: key.into(),
            track: TrackSummary {
                id: format!("t-{key}"),
                ..Default::default()
            },
            url: url.into(),
            headers: Default::default(),
            mime_type: None,
            gain_db: 0.0,
            transcoded: false,
        }
    }

    #[test]
    fn source_kinds_are_told_apart_by_url() {
        assert_eq!(source_kind("file:///data/music/a.flac"), "download");
        assert_eq!(source_kind("hocket-stream://abc123"), "core stream");
        assert_eq!(
            source_kind("https://music.example/rest/stream?id=1"),
            "direct URL"
        );
    }

    #[test]
    fn a_description_never_carries_the_url() {
        let s = source(
            "k1",
            "https://music.example/rest/stream?id=1&t=SECRET&s=SALT",
        );
        let d = describe_source(&s);
        assert_eq!(d, "k1 (t-k1) from direct URL");
        assert!(!d.contains("SECRET") && !d.contains("music.example"));
    }

    /// Records which methods reached it, with the arguments that matter.
    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl PlaybackBackend for Recorder {
        fn load(
            &self,
            s: MediaSource,
            n: Option<MediaSource>,
            p: u32,
            play: bool,
        ) -> Result<(), BackendError> {
            self.0
                .lock()
                .push(format!("load {} {:?} {p} {play}", s.key, n.map(|n| n.key)));
            Ok(())
        }
        fn set_next(&self, n: Option<MediaSource>) -> Result<(), BackendError> {
            self.0.lock().push(format!("next {:?}", n.map(|n| n.key)));
            Ok(())
        }
        fn play(&self) -> Result<(), BackendError> {
            self.0.lock().push("play".into());
            Ok(())
        }
        fn pause(&self) -> Result<(), BackendError> {
            self.0.lock().push("pause".into());
            Ok(())
        }
        fn stop(&self) -> Result<(), BackendError> {
            self.0.lock().push("stop".into());
            Ok(())
        }
        fn seek(&self, p: u32) -> Result<(), BackendError> {
            self.0.lock().push(format!("seek {p}"));
            Ok(())
        }
        fn set_volume(&self, v: f64) -> Result<(), BackendError> {
            self.0.lock().push(format!("volume {v}"));
            Ok(())
        }
        fn set_system_volume(&self, v: f64, m: bool) -> Result<(), BackendError> {
            self.0.lock().push(format!("system volume {v} {m}"));
            Ok(())
        }
        fn pre_buffer(&self, s: MediaSource, p: u32) -> Result<(), BackendError> {
            self.0.lock().push(format!("pre-buffer {} {p}", s.key));
            Ok(())
        }
        fn discard_pre_buffer(&self) -> Result<(), BackendError> {
            self.0.lock().push("discard".into());
            Ok(())
        }
        fn set_gapless(&self, e: bool) -> Result<(), BackendError> {
            self.0.lock().push(format!("gapless {e}"));
            Ok(())
        }
        fn set_output_device(&self, id: Option<String>) -> Result<(), BackendError> {
            self.0.lock().push(format!("output {id:?}"));
            Err(BackendError::Unsupported("routing"))
        }
        fn output_devices(&self) -> Vec<OutputDevice> {
            self.0.lock().push("devices".into());
            vec![]
        }
        fn set_exclusive(&self, e: bool) -> Result<(), BackendError> {
            self.0.lock().push(format!("exclusive {e}"));
            Ok(())
        }
        fn name(&self) -> &'static str {
            "recorder"
        }
    }

    /// Every call reaches the backend it wraps unchanged, the defaulted
    /// methods (system volume, exclusive) included, and so do its results.
    #[test]
    fn every_call_and_result_passes_through() {
        let inner = Arc::new(Recorder::default());
        let b = LoggedBackend::new(inner.clone());
        b.load(
            source("a", "file:///a"),
            Some(source("b", "hocket-stream://x")),
            1500,
            true,
        )
        .unwrap();
        b.set_next(None).unwrap();
        b.play().unwrap();
        b.pause().unwrap();
        b.seek(20).unwrap();
        b.set_volume(0.5).unwrap();
        b.set_system_volume(0.25, true).unwrap();
        b.pre_buffer(source("c", "file:///c"), 7).unwrap();
        b.discard_pre_buffer().unwrap();
        b.set_gapless(false).unwrap();
        assert!(matches!(
            b.set_output_device(Some("hdmi".into())),
            Err(BackendError::Unsupported("routing"))
        ));
        assert!(b.output_devices().is_empty());
        b.set_exclusive(true).unwrap();
        b.stop().unwrap();
        assert_eq!(b.name(), "recorder");
        assert_eq!(
            *inner.0.lock(),
            [
                "load a Some(\"b\") 1500 true",
                "next None",
                "play",
                "pause",
                "seek 20",
                "volume 0.5",
                "system volume 0.25 true",
                "pre-buffer c 7",
                "discard",
                "gapless false",
                "output Some(\"hdmi\")",
                "devices",
                "exclusive true",
                "stop",
            ]
        );
    }
}
