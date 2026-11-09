# AutoMix implementation spec

`docs/design.md` → *AutoMix* records the decisions. This file is the contract
the pieces are built to: where code lives, the types that cross module and FFI
boundaries, and what each backend must do. Change it when the contract
changes.

## Vocabulary

- **Outgoing** (A) and **incoming** (B): the track that is ending and the one
  that follows it.
- **Tail**: the end of A from `tail_start_ms` on. Backends stop playing A
  normally at `tail_start_ms` and play a **rendered tail** instead: A's audio
  from that point, time-stretched, EQ'd and faded according to the plan.
- **Overlap**: the stretch of output time where the tail and B both sound.
- **Handover**: one instant inside the overlap where B takes over as "now
  playing".

All times in a plan are milliseconds. Positions in A or B (`*_from_ms`,
`tail_start_ms`) are source positions; offsets from the tail start
(`incoming_at_ms`, `handover_ms`, `tail_len_ms`) are output time.

## Module layout (hocket-core)

```
src/automix/
  mod.rs        re-exports; ANALYSIS_VERSION; TransitionMode helpers
  record.rs     AnalysisRecord and friends (serde, versioned)
  rules.rs      no-mix rules: should_mix(a, b, ctx) -> MixDecision
  plan.rs       the planner: plan_transition(..) -> TransitionPlan
  render.rs     pure DSP: render_tail(..), IncomingShaper, fade curves, shelving EQ automation
  stretch.rs    time-stretch wrapper used by render_tail
  loudness.rs   loudness envelope + silence edges from PCM (pure, no decoding)
  analysis/     feature "automix-analysis": decode + Beat This! + loudness -> AnalysisRecord
    mod.rs      Analyser (analyse_file / analyse_pcm), streaming accumulator
    grid.rs     bpm and beat_confidence from the tracker's beats/downbeats
    model.rs    the embedded ONNX models run by rten
    models/     mel_spectrogram.onnx + beat_this_small.onnx (see NOTICE)
    NOTICE      provenance and MIT notices of Beat This! and beat-this-rs
  corpus/       pairs.txt, records/, plans/: the planner's snapshot test data
  store.rs      local SQLite table of records (core db migrations)
```

- Everything outside `analysis/` is pure Rust over PCM buffers and plain data,
  compiles on every target (Android included) and has no I/O.
- `analysis/` needs decoding (Symphonia) and the beat model. It sits behind a
  cargo feature `automix-analysis` (implies the feature `decode`: Symphonia
  and `audio::decoder`, no cpal/ALSA; `native-audio` implies `decode` too),
  enabled by `hocket-node` and `hocket-coordinator`, never by
  `hocket-android`.
- Decoding is per platform: Symphonia on desktop and the coordinator,
  MediaCodec on Android. Analysis functions take PCM.

## Types (api.rs, typeshare)

### Setting

```rust
enum TransitionMode { Off, Crossfade, AutoMix }      // default Off
struct TransitionSettings { mode: TransitionMode, crossfade_seconds: f64 } // 1..=12, default 6
```

Synced across devices through the existing synced-settings mechanism, so the
same queue sounds the same whichever device plays it.

### AnalysisRecord

One per track, JSON, a few KB.

```rust
struct AnalysisRecord {
    version: u32,                 // ANALYSIS_VERSION; bump on any algorithm change
    analyser: String,             // e.g. "beat-this-small/0.x", "loudness"
    produced_by: AnalysisProducer,// Worker | Desktop | Device
    source: SourceFingerprint,    // what was analysed
    duration_ms: u32,
    integrated_lufs: Option<f32>,
    audible_start_ms: u32,        // first sample above -60 dBFS (trim leading silence)
    audible_end_ms: u32,          // last sample above -60 dBFS
    head: LoudnessCurve,          // short-term loudness of the first ~60 s
    tail: LoudnessCurve,          // short-term loudness of the last ~60 s
    bpm: Option<f32>,             // None when no beat analysis or not trustworthy
    beats_ms: Vec<u32>,           // empty for loudness-only records
    downbeats_ms: Vec<u32>,       // subset of beats_ms
    beat_confidence: f32,         // 0..1; planner gates on it
    created_at: f64,              // epoch ms
}
struct SourceFingerprint { duration_ms: u32, size_bytes: Option<f64>, suffix: Option<String>, transcoded: bool }
struct LoudnessCurve { start_ms: u32, hop_ms: u32, lufs: Vec<f32> }
```

`size_bytes` is `f64` like `Track::size_bytes` (no 64-bit integers cross the
FFI). Curves are K-weighted (BS.1770), 3 s windows centred on points 100 ms
apart, floored at −70 LUFS. A loudness-only record's `integrated_lufs` is
measured over its head and tail windows together (an approximation of the
whole track); `automix::loudness::loudness_record` builds one from PCM.

- A record is usable for a track when `version` matches and its fingerprint
  matches the track's (duration within 1 s; size and suffix when both known).
- Preference when two records exist: higher `version`, then records with
  beats over loudness-only, then Worker over Desktop over Device, then newer.
- Loudness-only records (`beats_ms` empty) are valid and drive the smart plain
  fade.
- `automix::record::compact` tidies a record before storing or sending it
  (sorted beats, downbeats ⊂ beats, curve values to 0.1 LU).

### TransitionPlan

Computed by the core, carried to the backend on the incoming source.

```rust
struct TransitionPlan {
    kind: TransitionKind,         // Fade | Blend | TempoBlend
    tail_start_ms: u32,           // A position where normal playback stops and the rendered tail begins
    tail_source_ms: u32,          // how much of A (from tail_start) the tail consumes
    tail_len_ms: u32,             // rendered tail length in output time
    incoming_at_ms: u32,          // output offset from tail start where B's first sample plays
    incoming_from_ms: u32,        // B position it starts from (leading-silence/intro trim)
    handover_ms: u32,             // output offset from tail start of the handover
    out_curve: FadeCurve,         // outgoing gain over the overlap
    in_curve: FadeCurve,          // incoming gain over the overlap
    tempo: Option<TempoRamp>,     // outgoing stretch: ratio 1.0 -> ratio_end over ramp_ms from tail start
    bass_swap: Option<ShelfMove>, // low shelf (~100 Hz) automation, see render.rs
    high_cut: Option<ShelfMove>,  // high shelf automation on the outgoing tail
    incoming_gain_db: f32,        // extra gain on B at the overlap start, easing to 0 dB by its end; usually 0
}
enum TransitionKind { Fade, Blend, TempoBlend }
enum FadeCurve { EqualPower, Linear }          // out side runs 1 -> 0, in side 0 -> 1
struct TempoRamp { ratio_end: f32, ramp_ms: u32 } // ratio = speed (source ms per output ms)
struct ShelfMove { freq_hz: f32, from_db: f32, to_db: f32, start_ms: u32, len_ms: u32 }
```

- The overlap is `incoming_at_ms..tail_len_ms` (output time); both fade
  curves run over it.
- `ShelfMove` times are output time from the tail start; the gain moves
  linearly in dB. The incoming side of `bass_swap` runs it mirrored (B's low
  shelf sits at `to_db` until the move, then rises to `from_db`).
- `tail_source_ms` is the exact integral of the tempo ramp over
  `tail_len_ms` (`automix::stretch::source_ms`), so planner and renderer agree.

`MediaSource` gains:

- `mix_in: Option<TransitionPlan>` — on the **incoming** source given to
  `set_next`/`load`: how to enter this source from the current one. `None`
  means the existing gapless/cut behaviour.
- `mix_out_ms: Option<u32>` — on any source: the earliest position at which a
  transition out of it may begin, decided when the source is first handed to
  the backend (using the predicted next track). Backends that must commit
  early (Android's clipping) clip there. The final plan's `tail_start_ms`
  equals it whenever it is set.

### Backend reports and commands

- New report `TransitionStarted { from: QueueKey, to: QueueKey, tail_len_ms, incoming_at_ms, handover_ms }`
  when a backend begins playing a rendered tail.
- `Ended(A)` then `TransitionedToNext(B)` are reported **at the handover**,
  followed by `Position(B, p)` where `p` is B's true position (non-zero).
- New backend method `complete_transition()` (default no-op): drop the tail now
  and treat it as the handover if it hasn't happened. Used for Next pressed
  before the handover.
- `seek` during a transition cancels it: the backend drops whichever of A or B
  is not "now playing" and seeks the other.
- `pause`/`play` pause and resume both.
- `set_next` with a different source during the overlap but before the
  handover is ignored for this transition (it applies to the following one);
  before the tail starts, the newest `set_next` always wins.

## Core behaviour

- `refresh_next()` computes the plan for (current → next) whenever the next
  source is (re)built, from: settings, `should_mix`, the two tracks' records
  (local store, else fetched from the coordinator), and the current source's
  `mix_out_ms`.
- **Lock-in**: once current position ≥ `tail_start_ms − 30 s`, the plan is
  committed. A later change to what plays next produces a new plan of kind
  `Fade` with `tail_start_ms` kept (Android may already be clipped) and a short
  overlap (max(2 s, crossfade_seconds/2)).
- **Next before handover** → `backend.complete_transition()`; the core then
  treats B as current. **Next after handover** is an ordinary skip.
- `should_mix` returns false (→ `mix_in: None`) for: same album in track order
  (same album id, same disc and track n+1, or disc n+1 track 1); gapless tags
  or a gapless join; genres Classical / Opera / Orchestral, Audiobook /
  Podcast / Spoken word, Live (genre, or "live" release type / album title
  marker), Soundtrack / Score; repeat-one; sleep timer end-of-track; Cast
  renderer active; bit-perfect/exclusive output; mode Off.
  - Genres match whole words, case-insensitively, over the genre string
    (so "Hardcore" isn't "score"). Release types come from the album's
    OpenSubsonic `release_types` (`live`, `soundtrack`, `spokenword`,
    `audiobook`, `audio drama`, `interview`).
  - The library has no gapless tag (`iTunPGAP`) yet: `MixTrack::gapless` is
    `None` until it does. The gapless *join* is judged from both records: A
    audible to within 50 ms of its end and B from within 50 ms of its start,
    both at that edge within 10 LU of their integrated loudness.
- Scrobbling, played-ms, sleep timer, Connect stamps and media session all key
  off `TransitionedToNext` as today, so they switch at the handover.
- Connect handoff during a transition: the target starts B at B's position,
  unmixed.

## Planner (plan.rs)

Inputs: both records (optional, already checked usable), both tracks'
durations, mode, crossfade seconds, `fixed_tail_start_ms`. Mode Off plans
nothing. Every overlap is limited to half of each track's audible span.

1. **Crossfade mode**, or AutoMix with no usable records: `Fade`, overlap =
   crossfade seconds (AutoMix default 6 s), placed so the fade ends at A's
   `audible_end_ms` when known, else at its end; B from its
   `audible_start_ms` when known. Equal-power curves.
2. **AutoMix with a record for A but no beat grid on both** (loudness-only,
   or beats failing the gate below): smart `Fade`. Mix-out where A's tail
   loudness drops 6 LU below its sustained level (the median of the tail
   curve before the last 20 s) for good; overlap 4–10 s by how long A's fade
   is; never cut more than 20 s of audible A.
3. **AutoMix with beats on both** (confidence ≥ 0.5, ≥ 8 downbeats in A's
   last / B's first minute, steady grid: ≥ 85 % of bars within 4 % of the
   median bar; beats per bar is the mode of beats between downbeats, covering
   ≥ 80 % of bars):
   - tempo gap ≤ 8 % and both 4/4 → `Blend` when the drift over the overlap
     would be ≤ 25 ms unstretched, else `TempoBlend` (deviation: the spec said
     `Blend` at gap ≤ 1 %, but 1 % over 8 bars at 120 BPM is 160 ms of
     flamming): overlap 8 bars (16 when both A's last and B's first 30 s sit
     ≥ 4 LU below their integrated loudness), tail start on a downbeat that
     leaves the overlap ending at or before A's audible end, B entering on its
     first downbeat at or after `audible_start_ms`. `TempoBlend` ramps the
     ratio over the 4 bars before the overlap (the tail starts 4 bars
     earlier), the ratio being A's bar over B's bar across the overlap.
     Bass swap (low shelf 100 Hz to −26 dB) over the bar before the overlap's
     midpoint downbeat; handover at that midpoint. A high shelf (5 kHz, to
     −12 dB) cuts A from the handover to the end.
   - otherwise a beat-aligned `Fade` of 4 bars of A (2 if gap > 12 %, 1 if
     > 20 %), B's first downbeat on A's.
   - With `fixed_tail_start_ms`, the overlap (or the ramp) starts at the first
     downbeat at or after it; A plays unaltered up to there.
4. Rescue: if no candidate fits (A too short, overlap would cut > 20 s of
   audible A), plain 6 s `Fade`.

`late_edit_fade` builds the lock-in replacement plan: `Fade` from the
committed `tail_start_ms`, overlap max(2 s, crossfade / 2).

The planner is deterministic: same inputs → same plan on every device.

## Renderer (render.rs)

- `render_tail(plan, pcm, channels, rate) -> Vec<f32>`: `pcm` is A from
  `tail_start_ms` (at least `tail_source_ms` long), interleaved f32 at `rate`,
  already through the device's DSP chain except volume. Applies the tempo
  ramp, high-shelf/low-shelf automation and `out_curve`, and returns
  `tail_len_ms` of audio at `rate`.
- `IncomingShaper::new(plan, channels, rate)`, `process(&mut [f32])`: applied
  to B's frames from its first played sample: `in_curve`, `incoming_gain_db`
  and the incoming side of the bass swap (B's low shelf rises as A's falls).
- Mixing is a sum followed by the existing limiter.
- Filter coefficients move every 32 frames of the absolute timeline and gains
  are per frame, so `IncomingShaper` output doesn't depend on block size.
- Time-stretch (`stretch.rs`) is a pure-Rust WSOLA (40 ms Hann windows, 50 %
  overlap, ±8 ms similarity search). `signalsmith-stretch` was tried and
  rejected: its wrapper runs `bindgen` at build time, which needs libclang on
  every build host (absent on the dev host; would be needed in CI and the
  Android cross build) and links a C++ STL into the Android cdylib. For
  ±8 % over a few bars WSOLA is good enough; revisit if the mixlab says
  otherwise.
- Rates: the tail is rendered at the output rate the backend will mix at
  (desktop: device rate after resampling; Android: B's sample rate).

## Platforms

- **Desktop** (`audio/native/engine.rs`): when the current source reaches
  `tail_start_ms` with a plan pending, decode A's tail on the side (a second
  decode handle seeked to `tail_start_ms`) ahead of time, render it after
  DSP + resampling, then at `tail_start_ms` switch output to tail + shaped B.
  Each source keeps its own DSP chain and resampler.
- **Android** (`ExoBackend.kt`): A's `MediaItem` carries
  `ClippingConfiguration(endPositionMs = mix_out_ms)`. A Kotlin
  `AudioProcessor` wraps the Rust DSP chain (EQ, ReplayGain incl. boost,
  limiter) via JNI and, at B's first frame, sums the pre-rendered tail with
  `IncomingShaper`-processed B. The tail is decoded with
  `MediaExtractor`/`MediaCodec` and rendered by `render_tail` through the FFI.
  Android also computes loudness-only records from decoded heads/tails and
  hands them to the core.
- **Coordinator**: `AnalysisFetch`/`AnalysisRecords`/`AnalysisUpload` wire
  messages; SQLite store at `<data>/automix.sqlite` keyed by (normalised
  server URL, track id); optional worker (`--analysis-user`, password from
  env/file) that walks the library and analyses originals at low priority.

## Harness

`hocket-mixlab` (bin in hocket-core behind `automix-analysis`): analyse files,
plan a pair, render the transition (last 30 s of A blended into the first
30 s of B) to WAV plus the plan as JSON. Corpus: tracks fetched from the test
Navidrome server into a gitignored `automix-corpus/`; only the pair list and
reference plans are committed. A stats mode replays random pairs and prints
the distribution of transition kinds and overlap lengths.
