//! Native playback: Symphonia decoding, the DSP chain, rubato sample-rate
//! conversion and one continuous cpal output stream. Feature `native-audio`.
//!
//! ```text
//!  decode thread (current) ─┐            engine thread                cpal callback
//!  decode thread (next)    ─┼─ PcmQueue ─► mixer/transition ─► DSP ─► resample ─► SpscRing ─► device
//!  decode thread (prebuf)  ─┘  (per source)   (sample-accurate)   chain   (rubato)   (lock-free)
//! ```
//!
//! One output stream for the life of the backend (reopened only for device
//! changes or an exclusive-rate switch) is what makes gapless the natural
//! behaviour: the engine simply keeps feeding the ring from the next
//! source's queue the moment the current one ends, and the encoder padding
//! has already been trimmed by the decoder.

pub mod decoder;
pub mod engine;
pub mod http;
pub mod output;
pub mod resample;
pub mod ring;

pub use engine::{NativeBackend, NativeConfig};
pub use http::{MemoryFetcher, RangeFetcher, ReqwestFetcher};
pub use output::{list_output_devices, OutputConfig};
