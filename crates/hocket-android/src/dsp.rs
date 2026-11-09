//! AutoMix and the DSP chain for ExoPlayer (`docs/automix.md` → Platforms → Android).
//!
//! Two surfaces:
//!
//! - **Per buffer, JNI** (`app.hocket.playback.NativeDsp`): the Kotlin `AudioProcessor` calls these
//!   every few milliseconds, so they work in place on a direct `ByteBuffer` of native-order `f32`
//!   samples through an opaque handle. No copy, no JSON, no allocation on the audio path. UniFFI
//!   would serialise every buffer through a `RustBuffer` twice.
//!   - [`Chain`]: hocket-core's [`DspChain`] (track gain incl. boost, EQ, normalisation, limiter).
//!   - [`IncomingShaper`]: B's side of a transition.
//!   - `limit`: the chain's soft limiter, run on the sum of the tail and B.
//! - **One shot, UniFFI** (`lib.rs`): [`prepare_tail`] (DSP chain like A's, channel map, resample to
//!   B's rate, `render_tail`) and [`loudness_record`], on a background thread.
//!
//! Everything here except the `extern "system"` shims is plain Rust, unit-tested on the host.

use std::panic::{catch_unwind, AssertUnwindSafe};

use hocket_core::api::{
    AnalysisProducer, AnalysisRecord, AudioSettings, SourceFingerprint, TransitionPlan,
};
use hocket_core::audio::dsp::{DspChain, DspSettings, SoftLimiter};
use hocket_core::audio::resample;
use hocket_core::automix::loudness::{self, HeadTail};
use hocket_core::automix::{is_usable, render_tail, IncomingShaper};

/// The chain for one stream: hocket-core's [`DspChain`] with the format the processor was
/// configured with.
pub struct Chain {
    chain: DspChain,
}

impl Chain {
    pub fn new(rate: u32, channels: usize) -> Self {
        Self {
            chain: DspChain::new(
                f64::from(rate.max(1)),
                channels.max(1),
                DspSettings::default(),
            ),
        }
    }

    pub fn set_format(&mut self, rate: u32, channels: usize) {
        self.chain
            .set_format(f64::from(rate.max(1)), channels.max(1));
    }

    /// The EQ and normalisation of `audio`; ReplayGain is the explicit track gain.
    pub fn set_audio_settings(&mut self, audio: &AudioSettings) {
        self.chain.apply_audio_settings(audio);
    }

    /// A new track starts at the next sample: snap to its gain.
    pub fn set_track_gain(&mut self, gain_db: f64) {
        self.chain.begin_track_gain_db(gain_db);
    }

    pub fn reset(&mut self) {
        self.chain.reset();
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        self.chain.process(samples);
    }

    pub fn track_gain_db(&self) -> f64 {
        self.chain.track_gain_db()
    }
}

/// The limiter run after summing the tail and B (the chain's own, −1 dB knee).
pub fn limit(samples: &mut [f32]) {
    SoftLimiter::default().process(samples);
}

pub fn parse_plan(json: &str) -> Result<TransitionPlan, String> {
    serde_json::from_str(json).map_err(|e| format!("plan: {e}"))
}

pub fn parse_audio_settings(json: &str) -> Result<AudioSettings, String> {
    serde_json::from_str(json).map_err(|e| format!("audio settings: {e}"))
}

/// What [`prepare_tail`] renders from.
pub struct TailInput<'a> {
    /// A from `tail_start_ms` (no preroll), interleaved, at `in_rate`, straight from the decoder.
    pub pcm: &'a [f32],
    pub in_channels: usize,
    pub in_rate: u32,
    /// B's output format: the tail is mixed into B's frames.
    pub out_channels: usize,
    pub out_rate: u32,
    /// A's chain settings (EQ, normalisation) and A's track gain.
    pub audio: Option<&'a AudioSettings>,
    pub gain_db: f64,
}

/// The rendered tail: A's PCM through a chain configured like A's, mapped to B's channels,
/// resampled to B's rate, then `render_tail`. Exactly `tail_len_ms` at `out_rate`.
pub fn prepare_tail(plan: &TransitionPlan, input: TailInput<'_>) -> Result<Vec<f32>, String> {
    let in_ch = input.in_channels.max(1);
    let mut pcm = input.pcm[..input.pcm.len() / in_ch * in_ch].to_vec();
    let mut chain = Chain::new(input.in_rate, in_ch);
    if let Some(a) = input.audio {
        chain.set_audio_settings(a);
    }
    chain.set_track_gain(input.gain_db);
    chain.process(&mut pcm);
    let out_ch = input.out_channels.max(1);
    let converted = resample::convert(&pcm, in_ch, input.in_rate, out_ch, input.out_rate)
        .map_err(|e| e.to_string())?;
    Ok(render_tail(plan, &converted, out_ch, input.out_rate))
}

/// A loudness-only record from a track's decoded head and tail, fingerprinted with the
/// library's duration (the phone doesn't know the file's size or suffix; unknowns match).
#[allow(clippy::too_many_arguments)]
pub fn loudness_record(
    head: &[f32],
    tail: &[f32],
    tail_start_ms: u32,
    channels: usize,
    rate: u32,
    duration_ms: u32,
    transcoded: bool,
    created_at: f64,
) -> AnalysisRecord {
    loudness::loudness_record(
        HeadTail {
            head,
            tail,
            tail_start_ms,
            channels: channels.max(1),
            rate,
        },
        SourceFingerprint {
            duration_ms,
            size_bytes: None,
            suffix: None,
            transcoded,
        },
        AnalysisProducer::Device,
        created_at,
    )
}

/// Whether the planner can use `record` for a track of `duration_ms` (the phone's view: no size
/// or suffix known).
pub fn record_usable(record: &AnalysisRecord, duration_ms: u32) -> bool {
    is_usable(
        record,
        &SourceFingerprint {
            duration_ms,
            size_bytes: None,
            suffix: None,
            transcoded: false,
        },
    )
}

/// Little-endian `f32` bytes (what the UniFFI calls carry) to samples; a trailing partial sample
/// is dropped.
pub fn f32_from_le(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

pub fn f32_to_le(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

// -- JNI ------------------------------------------------------------------------------------------
//
// `app.hocket.playback.NativeDsp` (a Kotlin `object`, so the second argument is the instance).
// Handles are `Box` pointers; 0 is "none" and every call ignores it. A panic never crosses the
// boundary (it would abort the process): it is caught and the buffer left as it was.

mod jni_shims {
    use super::*;
    use jni::objects::{JByteBuffer, JObject, JString};
    use jni::sys::{jboolean, jdouble, jint, jlong, JNI_FALSE, JNI_TRUE};
    use jni::JNIEnv;

    fn guarded<T>(fallback: T, f: impl FnOnce() -> T) -> T {
        catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
    }

    /// The first `samples` floats of a direct buffer, when it holds them and is aligned.
    fn floats<'a>(env: &JNIEnv, buf: &JByteBuffer, samples: jint) -> Option<&'a mut [f32]> {
        let n = usize::try_from(samples).ok()?;
        let ptr = env.get_direct_buffer_address(buf).ok()?;
        let cap = env.get_direct_buffer_capacity(buf).ok()?;
        if ptr.is_null() || n.checked_mul(4)? > cap || !(ptr as usize).is_multiple_of(4) {
            return None;
        }
        // SAFETY: a direct buffer of at least `n` aligned floats, which the Kotlin caller owns
        // and doesn't touch for the duration of this call.
        Some(unsafe { std::slice::from_raw_parts_mut(ptr.cast::<f32>(), n) })
    }

    fn string(env: &mut JNIEnv, s: &JString) -> Option<String> {
        env.get_string(s).ok().map(Into::into)
    }

    fn chain<'a>(h: jlong) -> Option<&'a mut Chain> {
        // SAFETY: `h` came from `chainNew` and is not freed while Kotlin holds it.
        (h != 0).then(|| unsafe { &mut *(h as *mut Chain) })
    }

    fn shaper<'a>(h: jlong) -> Option<&'a mut IncomingShaper> {
        // SAFETY: as for `chain`, from `shaperNew`.
        (h != 0).then(|| unsafe { &mut *(h as *mut IncomingShaper) })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainNew(
        _env: JNIEnv,
        _this: JObject,
        rate: jint,
        channels: jint,
    ) -> jlong {
        guarded(0, || {
            Box::into_raw(Box::new(Chain::new(
                rate.max(1) as u32,
                channels.max(1) as usize,
            ))) as jlong
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainFree(
        _env: JNIEnv,
        _this: JObject,
        h: jlong,
    ) {
        if h != 0 {
            // SAFETY: from `chainNew`, freed once (Kotlin zeroes its copy).
            drop(unsafe { Box::from_raw(h as *mut Chain) });
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainSetFormat(
        _env: JNIEnv,
        _this: JObject,
        h: jlong,
        rate: jint,
        channels: jint,
    ) {
        guarded((), || {
            if let Some(c) = chain(h) {
                c.set_format(rate.max(1) as u32, channels.max(1) as usize);
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainSetAudioSettings(
        mut env: JNIEnv,
        _this: JObject,
        h: jlong,
        json: JString,
    ) -> jboolean {
        let Some(json) = string(&mut env, &json) else {
            return JNI_FALSE;
        };
        guarded(JNI_FALSE, || {
            match (chain(h), parse_audio_settings(&json)) {
                (Some(c), Ok(a)) => {
                    c.set_audio_settings(&a);
                    JNI_TRUE
                }
                _ => JNI_FALSE,
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainSetTrackGain(
        _env: JNIEnv,
        _this: JObject,
        h: jlong,
        gain_db: jdouble,
    ) {
        guarded((), || {
            if let Some(c) = chain(h) {
                c.set_track_gain(gain_db);
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainReset(
        _env: JNIEnv,
        _this: JObject,
        h: jlong,
    ) {
        guarded((), || {
            if let Some(c) = chain(h) {
                c.reset();
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_chainProcess(
        env: JNIEnv,
        _this: JObject,
        h: jlong,
        buf: JByteBuffer,
        samples: jint,
    ) {
        let Some(s) = floats(&env, &buf, samples) else {
            return;
        };
        guarded((), || {
            if let Some(c) = chain(h) {
                c.process(s);
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_shaperNew(
        mut env: JNIEnv,
        _this: JObject,
        plan_json: JString,
        channels: jint,
        rate: jint,
    ) -> jlong {
        let Some(json) = string(&mut env, &plan_json) else {
            return 0;
        };
        guarded(0, || match parse_plan(&json) {
            Ok(plan) => Box::into_raw(Box::new(IncomingShaper::new(
                &plan,
                channels.max(1) as usize,
                rate.max(1) as u32,
            ))) as jlong,
            Err(e) => {
                tracing::warn!(target: "hocket_android", error = %e, "AutoMix: shaper plan");
                0
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_shaperFree(
        _env: JNIEnv,
        _this: JObject,
        h: jlong,
    ) {
        if h != 0 {
            // SAFETY: from `shaperNew`, freed once.
            drop(unsafe { Box::from_raw(h as *mut IncomingShaper) });
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_shaperProcess(
        env: JNIEnv,
        _this: JObject,
        h: jlong,
        buf: JByteBuffer,
        samples: jint,
    ) {
        let Some(s) = floats(&env, &buf, samples) else {
            return;
        };
        guarded((), || {
            if let Some(sh) = shaper(h) {
                sh.process(s);
            }
        })
    }

    #[no_mangle]
    pub extern "system" fn Java_app_hocket_playback_NativeDsp_limit(
        env: JNIEnv,
        _this: JObject,
        buf: JByteBuffer,
        samples: jint,
    ) {
        let Some(s) = floats(&env, &buf, samples) else {
            return;
        };
        guarded((), || limit(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocket_core::api::{EqBand, EqSettings, FadeCurve, ReplayGainMode, TransitionKind};

    fn plan() -> TransitionPlan {
        TransitionPlan {
            kind: TransitionKind::Fade,
            tail_start_ms: 10_000,
            tail_source_ms: 1_000,
            tail_len_ms: 1_000,
            incoming_at_ms: 200,
            incoming_from_ms: 0,
            handover_ms: 600,
            out_curve: FadeCurve::Linear,
            in_curve: FadeCurve::Linear,
            tempo: None,
            bass_swap: None,
            high_cut: None,
            incoming_gain_db: 0.0,
        }
    }

    fn audio(eq_gain_db: f64) -> AudioSettings {
        AudioSettings {
            replay_gain: ReplayGainMode::Auto,
            replay_gain_preamp_db: 0.0,
            normalisation: false,
            eq: EqSettings {
                enabled: eq_gain_db != 0.0,
                preamp_db: 0.0,
                bands: vec![EqBand {
                    frequency_hz: 1_000.0,
                    gain_db: eq_gain_db,
                    q: 1.0,
                }],
                preset: None,
            },
            gapless: true,
            output_device: None,
            exclusive: false,
        }
    }

    fn dc(frames: usize, channels: usize, v: f32) -> Vec<f32> {
        vec![v; frames * channels]
    }

    #[test]
    fn chain_applies_positive_gain_and_limits() {
        let mut c = Chain::new(48_000, 2);
        c.set_track_gain(6.0);
        assert!((c.track_gain_db() - 6.0).abs() < 1e-9);
        let mut s = dc(256, 2, 0.25);
        c.process(&mut s);
        // +6 dB doubles a quiet signal…
        assert!((s[100] - 0.5).abs() < 0.01, "{}", s[100]);
        // …and the limiter keeps a loud one under full scale.
        let mut loud = dc(256, 2, 0.9);
        c.process(&mut loud);
        assert!(loud.iter().all(|x| x.abs() < 1.0));
        c.set_track_gain(-6.0);
        let mut q = dc(16, 2, 0.5);
        c.process(&mut q);
        assert!(
            (q[0] - 0.25).abs() < 0.01,
            "a new track snaps without a ramp"
        );
    }

    #[test]
    fn chain_takes_eq_from_audio_settings_json() {
        let json = serde_json::to_string(&audio(6.0)).unwrap();
        let a = parse_audio_settings(&json).unwrap();
        let peak_at = |freq: f64| {
            let mut c = Chain::new(48_000, 1);
            c.set_audio_settings(&a);
            let mut sine: Vec<f32> = (0..48_000)
                .map(|i| {
                    (2.0 * std::f64::consts::PI * freq * i as f64 / 48_000.0).sin() as f32 * 0.1
                })
                .collect();
            c.process(&mut sine);
            sine[24_000..].iter().fold(0f32, |m, s| m.max(s.abs()))
        };
        // +6 dB at 1 kHz, with the clip protection's −6 dB preamp: 1 kHz stays, 100 Hz drops.
        let (band, off) = (peak_at(1_000.0), peak_at(100.0));
        assert!(band > off * 1.6, "1 kHz {band}, 100 Hz {off}");
        assert!(parse_audio_settings("{").is_err());
    }

    #[test]
    fn tail_is_exact_length_at_bs_rate_and_channels() {
        let p = plan();
        let pcm = dc(44_100, 1, 0.5);
        let tail = prepare_tail(
            &p,
            TailInput {
                pcm: &pcm,
                in_channels: 1,
                in_rate: 44_100,
                out_channels: 2,
                out_rate: 48_000,
                audio: None,
                gain_db: -6.0,
            },
        )
        .unwrap();
        assert_eq!(tail.len(), 48_000 * 2);
        // Before the overlap the tail is A at A's gain (−6 dB of 0.5).
        assert!((tail[4_800 * 2] - 0.25).abs() < 0.01, "{}", tail[4_800 * 2]);
        // It has faded out by its end.
        assert!(tail[tail.len() - 1].abs() < 0.01);
        assert!(tail.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn plan_round_trips_through_json() {
        let json = serde_json::to_string(&plan()).unwrap();
        assert_eq!(parse_plan(&json).unwrap(), plan());
        // Kotlin leaves out null options (`explicitNulls = false`).
        let sparse = json.replace(",\"tempo\":null", "");
        assert_eq!(parse_plan(&sparse).unwrap(), plan());
        assert!(parse_plan("{}").is_err());
    }

    #[test]
    fn shaper_fades_b_in_over_the_overlap() {
        let mut sh = IncomingShaper::new(&plan(), 1, 1_000);
        let mut b = vec![1.0f32; 1_000];
        sh.process(&mut b);
        assert!(b[0].abs() < 0.01, "B enters silent");
        assert!(
            (b[400] - 0.5).abs() < 0.02,
            "halfway through the 800 ms overlap"
        );
        assert_eq!(b[900], 1.0, "untouched after the overlap");
    }

    #[test]
    fn loudness_record_is_device_loudness_only_and_usable() {
        let rate = 8_000;
        let head = dc(rate as usize * 10, 2, 0.3);
        let tail = dc(rate as usize * 10, 2, 0.3);
        let r = loudness_record(&head, &tail, 170_000, 2, rate, 180_000, true, 1.0);
        assert_eq!(r.produced_by, AnalysisProducer::Device);
        assert!(r.beats_ms.is_empty());
        assert!(r.source.transcoded);
        assert_eq!(r.source.duration_ms, 180_000);
        assert!(record_usable(&r, 180_500));
        assert!(!record_usable(&r, 185_000));
    }

    #[test]
    fn le_bytes_round_trip() {
        let s = vec![0.0, -1.0, 0.5, f32::MIN_POSITIVE];
        assert_eq!(f32_from_le(&f32_to_le(&s)), s);
        assert_eq!(f32_from_le(&[0, 0, 0]), Vec::<f32>::new());
    }

    #[test]
    fn limiter_bounds_a_hot_sum() {
        let mut s = vec![1.6f32, -1.8, 0.1];
        limit(&mut s);
        assert!(s[0] < 1.0 && s[1] > -1.0);
        assert!((s[2] - 0.1).abs() < 1e-6);
    }
}
