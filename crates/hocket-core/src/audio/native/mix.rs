//! AutoMix in the native engine (`docs/automix.md` → *Platforms*): the
//! rendered outgoing tail, the running transition's state, and the side job
//! that decodes and renders a tail ahead of time.
//!
//! ```text
//!  tail job (thread): A from tail_start − preroll ─► A's DSP (a clone) ─► resample ─► render_tail
//!  engine, at A's tail start:  rendered tail ─┐
//!                              live A (10 ms) ─┼─► sum ─► limiter ─► volume ─► ring
//!  B ─► B's DSP ─► resample ─► IncomingShaper ─┘
//! ```

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::api::{MediaSource, QueueKey, TransitionPlan};
use crate::audio::dsp::DspChain;
use crate::audio::native::http::RangeFetcher;
use crate::audio::native::resample::{remap_channels, Resampler};
use crate::automix::{render_tail, IncomingShaper};

/// Decode this much of A before the tail start so the tail's filters and
/// resampler are settled when its first frame plays.
pub const TAIL_PREROLL_MS: u32 = 200;
/// Start rendering a tail once A is this close to its tail start.
pub const TAIL_LEAD_MS: u32 = 60_000;
/// Give up on a tail whose decode takes longer than this.
pub const TAIL_DECODE_TIMEOUT: Duration = Duration::from_secs(60);
/// Live A and the rendered tail are crossfaded over this long at the
/// switch so filter state and sub-sample offsets never click.
pub const SWITCH_XFADE_MS: u32 = 10;

/// A tail rendered for the transition from the current source into the
/// next one, at the output format it was rendered for.
pub struct RenderedTail {
    pub plan: TransitionPlan,
    /// `next_generation` of the follow-up it was planned into.
    pub next_generation: u64,
    pub rate: u32,
    pub channels: usize,
    pub pcm: Vec<f32>,
}

impl RenderedTail {
    pub fn frames(&self) -> usize {
        self.pcm.len() / self.channels.max(1)
    }
}

/// Where the tail for the pending transition stands.
pub enum TailState {
    None,
    Rendering {
        next_generation: u64,
        plan: TransitionPlan,
        rate: u32,
        channels: usize,
    },
    Ready(RenderedTail),
}

/// A transition in progress: from A's tail start until the tail and B's
/// shaping have both run out.
pub struct Transition {
    pub from: QueueKey,
    pub to: QueueKey,
    pub tail: RenderedTail,
    /// Output frames mixed since the tail start.
    pub frame: usize,
    pub incoming_at: usize,
    pub handover: usize,
    /// Live A past the switch point, crossfaded into the tail's first frames.
    pub live: Vec<f32>,
    /// A until the handover (seek cancels back to it), with its DSP chain.
    pub outgoing: Option<(super::engine::Source, DspChain)>,
    pub shaper: IncomingShaper,
    /// B converted to the output format and shaped, not yet mixed.
    pub incoming: Vec<f32>,
    /// B's decoder has run out (its resampler flushed).
    pub incoming_done: bool,
    pub started_reported: bool,
    pub handed_over: bool,
    /// A `set_next` that arrived during the overlap: applied at the handover.
    pub held_next: Option<Option<MediaSource>>,
}

impl Transition {
    /// Output frames of this transition the listener has heard, given how
    /// many are still queued towards the device (negative: the tail start
    /// is still in the ring).
    pub fn heard(&self, queued_frames: usize) -> i64 {
        self.frame as i64 - queued_frames as i64
    }
}

/// Everything the tail job needs; all `Send`.
pub struct TailJob {
    pub source: MediaSource,
    pub plan: TransitionPlan,
    /// A clone of A's chain (settings, ReplayGain, AGC state).
    pub dsp: DspChain,
    pub gapless: bool,
    pub out_rate: u32,
    pub out_channels: usize,
    pub fetcher: Arc<dyn RangeFetcher>,
    pub runtime: tokio::runtime::Handle,
}

/// Decode A from `tail_start_ms` (plus preroll) on its own handle, run it
/// through A's DSP and a resampler to the output rate, and render the tail.
pub fn render_tail_job(job: TailJob) -> Result<Vec<f32>, String> {
    #[cfg(test)]
    if job.source.key.starts_with("slow-tail") {
        thread::sleep(Duration::from_secs(3));
    }
    let plan = &job.plan;
    let start_ms = plan.tail_start_ms.saturating_sub(TAIL_PREROLL_MS);
    let handle = super::engine::load_source(
        &job.source,
        start_ms,
        job.gapless,
        4,
        job.fetcher,
        job.runtime,
        "tail",
    )
    .map_err(|e| e.to_string())?;
    let spec = handle.spec;
    let rate = u64::from(spec.sample_rate);
    let preroll = u64::from(plan.tail_start_ms) * rate / 1000 - u64::from(start_ms) * rate / 1000;
    // A little more than the tail consumes, for the resampler's delay.
    let want = preroll + (u64::from(plan.tail_source_ms) + 100) * rate / 1000;
    let mut pcm: Vec<f32> = Vec::with_capacity(want as usize * spec.channels);
    let deadline = Instant::now() + TAIL_DECODE_TIMEOUT;
    let mut got = 0u64;
    while got < want {
        let (n, _) = handle.queue.pop((want - got).min(8192) as usize, &mut pcm);
        got += n as u64;
        if n > 0 {
            continue;
        }
        if let Some(e) = handle.queue.error() {
            return Err(e.to_string());
        }
        if handle.queue.is_eof() {
            break;
        }
        if Instant::now() > deadline {
            return Err("decoding the tail timed out".into());
        }
        thread::sleep(Duration::from_millis(2));
    }
    drop(handle);

    let mut dsp = job.dsp;
    dsp.set_format(f64::from(spec.sample_rate), spec.channels);
    dsp.reset();
    dsp.process(&mut pcm);
    let out_ch = job.out_channels.max(1);
    let mut mapped = Vec::with_capacity(pcm.len() / spec.channels.max(1) * out_ch);
    remap_channels(&pcm, spec.channels, out_ch, &mut mapped);
    drop(pcm);
    let out = if spec.sample_rate != job.out_rate {
        let mut r =
            Resampler::new(spec.sample_rate, job.out_rate, out_ch).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(
            (mapped.len() as f64 * f64::from(job.out_rate) / f64::from(spec.sample_rate)) as usize
                + 4096,
        );
        r.process(&mapped, &mut out).map_err(|e| e.to_string())?;
        r.flush(&mut out).map_err(|e| e.to_string())?;
        out
    } else {
        mapped
    };
    let skip = (preroll as f64 * f64::from(job.out_rate) / rate as f64).round() as usize * out_ch;
    let from = skip.min(out.len());
    Ok(render_tail(plan, &out[from..], out_ch, job.out_rate))
}
