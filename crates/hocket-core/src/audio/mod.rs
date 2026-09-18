//! Audio: the `PlaybackBackend` seam, the DSP chain, the native
//! Symphonia + cpal backend, the external (Android / ExoPlayer) bridge, the
//! sleep timer and the platform decode-capability lists.
//!
//! See `docs/design.md` §"Audio path". The actor owns exactly one
//! [`PlaybackBackend`] chosen from [`crate::api::AudioMode`]:
//!
//! | mode       | backend                                   |
//! |------------|-------------------------------------------|
//! | `Native`   | [`native::NativeBackend`] (feature `native-audio`) |
//! | `External` | [`external::ExternalBackend`]             |
//! | `None`     | [`backend::NullBackend`]                  |
//!
//! Every backend reports back with [`crate::api::BackendReport`] through a
//! [`backend::ReportSink`], so the actor has one code path regardless of where
//! the audio is actually produced. [`scripted::ScriptedBackend`] plays virtual
//! time from an injected [`crate::util::Clock`] for the actor's tests and the
//! simulation harness.

pub mod backend;
pub mod dsp;
pub mod external;
pub mod formats;
pub mod padding;
pub mod scripted;
pub mod sleep;

#[cfg(feature = "native-audio")]
pub mod native;

pub use backend::{BackendError, NullBackend, PlaybackBackend, ReportSink};
pub use external::ExternalBackend;
pub use scripted::ScriptedBackend;
