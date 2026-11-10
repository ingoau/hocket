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
    head_vocals: VocalCurve,      // voice probability of the first ~36 s (empty: not analysed)
    tail_vocals: VocalCurve,      // voice probability of the last ~36 s
    bpm: Option<f32>,             // None when no beat analysis or not trustworthy
    beats_ms: Vec<u32>,           // empty for loudness-only records
    downbeats_ms: Vec<u32>,       // subset of beats_ms
    beat_confidence: f32,         // 0..1; planner gates on it
    created_at: f64,              // epoch ms
}
struct SourceFingerprint { duration_ms: u32, size_bytes: Option<f64>, suffix: Option<String>, transcoded: bool }
struct LoudnessCurve { start_ms: u32, hop_ms: u32, lufs: Vec<f32> }
struct VocalCurve { start_ms: u32, hop_ms: u32, voice: Vec<f32> }
```

`size_bytes` is `f64` like `Track::size_bytes` (no 64-bit integers cross the
FFI). Curves are K-weighted (BS.1770), 3 s windows centred on points 100 ms
apart, floored at −70 LUFS. A loudness-only record's `integrated_lufs` is
measured over its head and tail windows together (an approximation of the
whole track); `automix::loudness::loudness_record` builds one from PCM.
Voice curves are singing-voice probabilities (sung or rapped, 0..1) every
100 ms, held over the detector's 3 s patches (see *Analysis*); only full
records have them.

- A record is usable for a track when `version` matches and its fingerprint
  matches the track's (duration within 1 s; size and suffix when both known).
- Preference when two records exist: higher `version`, then records with
  beats over loudness-only, then Worker over Desktop over Device, then newer.
- Loudness-only records (`beats_ms` empty) are valid and drive the smart plain
  fade.
- `automix::record::compact` tidies a record before storing or sending it
  (sorted beats, downbeats ⊂ beats, curve values to 0.1 LU, voice
  probabilities to 0.01).

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
  - Only a plan already handed to the backend (the preloaded follow-up's
    `mix_in`) is committed. Right after a handover onto a short track the
    plan into the track after is a full one from the source's `mix_out_ms`.
  - The edit wins over the pair rules: a new follow-up that `should_mix`
    refuses for the pair (album sequence, gapless, genre, release) still gets
    the late-edit fade, since A may already be clipped there.
  - A context rule (mode Off, repeat-one, sleep end-of-track, Cast, exclusive
    output) drops the plan even after lock-in, and no plan is sent. A backend
    that clipped A at `mix_out_ms` plays A out unclipped when the follow-up
    arrives without `mix_in` (Android un-clips itself).
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
     ≥ 4 LU below their integrated loudness and 16 bars of A last ≤ 32 s;
     phase-3 tuning: at 90 BPM 16 bars were 43 s plus the ramp), tail start on a downbeat that
     leaves the overlap ending at or before A's audible end, B entering on its
     first downbeat at or after `audible_start_ms`. `TempoBlend` ramps the
     ratio over the 4 bars before the overlap (the tail starts 4 bars
     earlier), the ratio being A's bar over B's bar across the overlap.
     Bass swap (low shelf 100 Hz to −26 dB) over the bar before the overlap's
     midpoint downbeat; handover at that midpoint. A high shelf (5 kHz, to
     −12 dB) cuts A from the handover to the end.
   - otherwise a beat-aligned `Fade` of 4 bars of A (2 if gap > 12 %, 1 if
     > 20 %), B's first downbeat on A's. Two phase-3 tunings from the
     corpus stats: here the gap is counted modulo an octave (half/double
     time lines up on every beat of the slower track), and bars are added
     until the fade lasts ≥ 4 s (a 1-bar fade at 128 BPM is a near-cut
     under 2 s; on random corpus pairs ~60 % of beat-aligned fades were
     1 bar before).
   - With `fixed_tail_start_ms`, the overlap (or the ramp) starts at the first
     downbeat at or after it; A plays unaltered up to there.
   - **Voices** (both records have voice curves; otherwise all of the above
     unchanged): one voice at a time. A point is voiced at probability
     ≥ 0.5; over a candidate's overlap (output time; A through the tempo
     ramp, B from `incoming_from_ms`) the voices are apart when A's last
     voiced point is no more than 0.5 s after B's first. Blends, in order:
     (a) the usual 8/16 bars when apart, the handover moved, if the midpoint
     isn't between the voices, to B's downbeat between them nearest the
     midpoint (so B's voice comes in on or after it; bass swap over the bar
     before it, never before B enters; high cut from it); (b) else the first
     of 4 and 2 bars that keeps them apart (same handover rule); (c) else
     the fewest bars lasting ≥ 4 s, with the bass swap from B's entry over
     half the way to the handover and A's high shelf moved down to 2 kHz
     (−12 dB) from B's entry to the handover. Beat-aligned fades drop bars
     (down to the 4 s minimum) until the voices are apart, else take the
     shortest.
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

## Analysis (analysis/)

- **Tracker**: Beat This! (CPJKU, ISMIR 2024) through its Rust port, crate
  `beat-this` **=1.1.0** (github.com/danigb/beat-this-rs, rev `1ae768e`),
  `default-features = false` (no Symphonia 0.6, no CLI), inference on
  `rten` **=0.24.0** (pure Rust: no ONNX Runtime, no system libraries). The
  published crate ships no models; `mel_spectrogram.onnx` (270 KB) and the
  small checkpoint `beat_this_small.onnx` (10.6 MB) are copied from that
  revision into `analysis/models/` and embedded with `include_bytes!`, so
  analysis works offline. `analysis/NOTICE` carries both MIT notices and
  the files' SHA-256. Our own `rten` model wrapper (`model.rs`) loads them
  from memory and takes a thread count (beat-this's loads only from paths).
- **API**: `Analyser::new(threads)` (0: rten's shared pool; background work
  passes 1) then `analyse_file(path, producer)` or `analyse_pcm(pcm,
  channels, rate, source, producer, created_at)`; `analysis::analyse_file`
  is the one-shot form. Both stream: decode chunks are downmixed into the
  tracker's stream while a whole-track `LoudnessMeter` runs and 60 s of
  head and tail PCM are kept for `loudness::loudness_record`. So
  `integrated_lufs` of a full record is the whole track's. `source` is the
  file's (decoded duration, size, extension, `transcoded: false`); callers
  replace it with the library's fingerprint when they analysed a
  transcode. `analyser` is `"beat-this-small/1.1.0+musicnn-mtt"`.
- **Voice detector** (`vocals.rs`): musicnn's MagnaTagATune model
  (`MTT_musicnn`, Pons & Serra, MTG-UPF; ISC licence, code and checkpoint),
  converted to ONNX by `scripts/musicnn-to-onnx.py` (3.2 MB, embedded, run
  by the same `rten`; `analysis/NOTICE` has provenance and SHA-256). The
  front end is ours, a port of the reference's librosa calls (16 kHz mono via
  `audio::resample`, 512-point periodic Hann every 256 samples, centred with
  zero padding, power, 96 Slaney mel bands, `log10(10000·x + 1)`), checked
  against librosa and TensorFlow to 1e-3 by `parity_with_reference`
  (ignored; needs the script's reference files). It runs on non-overlapping
  3 s patches (187 frames) of the first and last 35.9 s (12 patches each:
  blends are at most 32 s); a patch's voice probability is the strongest of
  the vocal tags (vocal, vocals, singing, voice, male/female …, choir,
  opera) over that plus the strongest no-vocal tag (`v / (v + n)`), held
  over the patch's 3 s in the 100 ms `VocalCurve`. Candidates considered:
  YAMNet (Apache-2.0) and EfficientAT `mn10_as` (MIT) read AudioSet's
  Singing / Rapping / Speech classes, but on the corpus's vocal and
  instrumental versions of the same songs they told them apart only on
  10 s windows (YAMNet not at all); Essentia's voice/instrumental models are
  CC BY-NC-SA; the dedicated SVD research models publish no weights or no
  licence; Demucs-class separation is too slow on the worker.
- **Grid** (`grid.rs`). The tracker's "minimal" output (peaks on 50 fps
  frames) is tidied before it goes in the record:
  1. each peak is refined to sub-frame time by a parabola through the
     logits (20 ms steps are a 1 % bar-length error, 150 ms over 8 bars);
  2. a beat splitting one interval in two is dropped, gaps of 2–4 local
     intervals are filled evenly; longer gaps start a new segment;
  3. downbeats are regularised: the metre is 3 or 4, whichever more raw
     downbeat spacings (in beats) are multiples of; each beat's bar phase is
     the vote of the raw downbeats within ±8 bars of the same segment; the
     record's downbeats are the beats on that phase.
  `bpm` is 60 000 / the median 8-beat span ÷ 8. `beat_confidence` =
  steadiness × agreement × (0.5 + 0.5 × coverage): the share of beat
  intervals within 8 % of the median of their ±8 neighbours, the share of
  the tracker's raw downbeats the regular grid keeps, and the beats' share
  of the audible span. `apply_grid(record, raw)` rebuilds a record's grid
  from the refined tracker output (`RawBeats`), which the mixlab caches.
- **Cost** (mixlab release build, one inference thread, i7-8650U with the
  host heavily loaded by other jobs, so read as upper bounds): ~0.11 s of
  CPU per second of audio (a 3:54 FLAC: 27 s CPU, 35 s wall; 1:01: 9 s;
  8:22: 54 s). Peak RSS ~470–485 MB whatever the length: rten's working
  buffers for the 30 s chunks the model runs on, plus the mel spectrogram
  (~1.5 MB/min). More threads (`Analyser::new(0)`) did not help on the
  loaded host. Binary: ~15 MB more in whatever links a caller (10.8 MB of
  models in `.rodata`, ~4 MB of rten/rustfft code); the desktop addon
  grew only 42 KB so far because nothing in hocket-node calls analysis yet.
- **Corpus results** (100 tracks, `stats --pairs 500 --seed 1`, AutoMix):
  94 records with confidence ≥ 0.5 (median 0.82); A's tail grid passes the
  planner's gate on 74, B's head on 62. Fade 91.8 % (overlap median 4.9 s,
  p90 9.1 s), TempoBlend 8.0 % (median 19.5 s, max 32.8 s, stretch median
  4.7 %, max 7.8 %), Blend 0.2 %. Beat-aligned fades by A's bars: 1: 20,
  2: 162, 3: 87, 4: 54, more: 22. Before the grid tidying in `grid.rs`
  only 9 of 38 tails passed the gate; before the 4 s fade minimum most
  beat-aligned fades were a single bar. 80 rendered pairs: no non-finite
  samples, peak ≤ −0.00 dBFS after the limiter, exact lengths.
- **Voice detector cost** (same build, one thread, five 2:38–5:12 tracks,
  old and new analyser run alternately): +1.3–1.5 s per track, fixed
  whatever the length (24 patches at ~45 ms, plus resampling and the mel
  front end): +15–17 % on a 3-minute track, +8 % on a 5-minute one. The
  network's cost is linear in the audio it sees, so the regions are the
  planner's longest overlap, not the loudness curves' minute.
- **Voices on the corpus** (same 500 pairs): of the 40 beat-matched blends,
  A's and B's voices overlapped in 31 without voice curves and in 14 with
  them (the rest kept apart; the 14 are voices all the way through, now
  short shaped blends); median blend overlap 19.2 s → 7.1 s, longest
  32.8 s → 25.9 s. Beat-aligned fades changed in 20 pairs. Smart and rescue
  fades (3–12 s, no grid) don't read voices: their voices overlap in 199 of
  460 pairs either way.

## Platforms

- **Desktop** (`audio/native/engine.rs`, `audio/native/mix.rs`):
  - Once current and follow-up are both loaded, the follow-up carries
    `mix_in` and A is within 60 s of `tail_start_ms`, a side thread opens A
    again (its own decode handle, from `tail_start_ms` − 200 ms of preroll),
    runs it through a clone of A's DSP chain and a fresh resampler to the
    device rate and channels, drops the preroll and calls `render_tail`.
    When the tail is ready, B's decoder is moved to `incoming_from_ms`.
  - The switch is sample-exact: when A's converted output reaches the
    output frame of `tail_start_ms`, live A stops; its next 10 ms are
    crossfaded into the tail's first frames (filter state and sub-sample
    offsets never click). B becomes the current source with its own DSP
    chain (a clone of A's with B's format and gain) and resampler, shaped
    by `IncomingShaper` from `incoming_at_ms`. The sum goes through the
    chain's `SoftLimiter` (−1 dB knee), then volume, into the ring.
  - `TransitionStarted` and the handover reports are sent when those
    output frames are heard (ring and pending output accounted for), with
    B's handover position from the plan. Position/Playing/Paused reports
    name A until the handover.
  - Fallback: if the tail isn't ready when A reaches the switch point (or
    rendering failed, or A is already past it), the plan is dropped for
    this follow-up, logged, and A plays to its end with B following from
    its start gaplessly; playback never waits for a tail.
  - Seek before the handover puts A back as current and B back as the
    follow-up (the tail is kept, so a seek before the tail start mixes
    again); after it, the tail is dropped and B seeks. `complete_transition`
    and an output reopen (device change, exclusive rate switch, stream
    error) hand over at once and play B alone from its heard position. A
    `set_next` during the overlap is held until the handover; a `set_next`
    with the same source keeps the rendered tail. Exclusive output never
    mixes. If B's decoder starves during the overlap the mix waits for it.
  - Analysis on a miss (`core/handlers/analysis.rs`, feature
    `automix-analysis`, not in the coordinator's core): with the mode on
    AutoMix, every 2 s the actor picks the first of current / next /
    next-next with no usable record (or only a phone's loudness-only one)
    whose whole file is on disk (download, else a complete stream-cache
    entry), and hands it to one worker thread holding one `Analyser::new(1)`.
    The record's `source` is replaced by the library fingerprint
    (`transcoded` set for transcoded files) and stored through
    `store_analysis_record`, which replans the follow-up. One at a time,
    each track tried once per session. `on_analysis_produced` uploads the
    record (see *Sharing records*).
- **Android** (`ExoBackend.kt`, `MixProcessor.kt`, `PcmDecoder.kt`,
  `LoudnessRecorder.kt`; Rust side `crates/hocket-android/src/dsp.rs`):
  - **DSP**: a Media3 `BaseAudioProcessor` in the main player's
    `DefaultAudioSink` (PCM path only: 16-bit or float in, the same out; no
    float output, no offload) runs hocket-core's `DspChain` per buffer
    through **direct JNI** (`NativeDsp`: opaque handles, in place on a direct
    `ByteBuffer` of floats). UniFFI is used only for the one-shot calls
    (tail render, loudness record, record usability), with PCM as LE `f32`
    bytes. The chain applies the source's `gain_db` (boost included), the
    in-app EQ and normalisation from `AudioSettings` (live, from
    `AudioSettingsChanged`), and the limiter; `player.volume` is only the
    master/sleep-fade volume.
  - **Item boundaries**: at a gapless join the sink drains the processors
    (`queueEndOfStream`) and flushes them with the new stream's
    `StreamMetadata` (timeline + period → the item's media id, and its
    start position); a seek flushes without an end of stream first. So the
    first frame after such a flush is the item's first frame, sample-exact,
    and the track gain snaps there. No frame counting against durations.
    Right after, the sink drains and flushes again for the same item (the
    renderer's `handleDiscontinuity` at every stream change forces a timing
    resync); a contiguous flush of the item already playing is the same
    stream going on and must not end a mix.
    When B is at another sample rate the sink can't join gaplessly: it
    drains A, flushes once still naming A (the new configuration takes over
    after that flush), then flushes for B with no end of stream in between.
    So every flush between an end of stream and the next frame belongs to
    the join; without that, every mix between a 44.1 kHz and a 48 kHz track
    was dropped and A cut off at its clip point.
  - **Clipping**: an item with `mix_out_ms` gets
    `ClippingConfiguration(endPositionMs)` (on `load` only when its `next`
    has `mix_in`); the processor also drops frames past the clip end
    (ExoPlayer clips by whole decoder buffers). Only a clipped item arms a
    transition (unclipped, A's end would play twice). With nothing to do (no
    mix, no clip in the buffer, a `DspChain::is_bypassed` chain) a buffer
    passes through untouched: no float conversion, no JNI.
  - **Tail**: when the next source carries `mix_in`, the transition is armed
    (same pair and plan keep a rendered tail; the newest `set_next` wins
    before the tail starts). Once A is within 60 s of `tail_start_ms` one
    background thread decodes A from `tail_start_ms` (200 ms preroll
    dropped by timestamp, `tail_source_ms` long) with
    `MediaExtractor`/`MediaCodec` — `hocket-stream://` through the core's
    stream reader as a `MediaDataSource`, `file:` from disk, else HTTP with
    the source's headers — probes B's rate and channels from its container,
    and calls `automix_prepare_tail`: a `DspChain` like A's (A's gain, the
    EQ), channel map, **rubato** resampling to B's rate (the desktop
    resampler, moved to `audio::resample` behind the feature `resample`,
    which `hocket-android` enables; pure Rust), then `render_tail`.
  - **Mixing**: at B's first frame after a contiguous join, if the armed
    pair matches and the tail is at B's format, the processor outputs the
    tail's first `incoming_at_ms` at once, then B (its first
    `incoming_from_ms` dropped, through the chain and `IncomingShaper`) plus
    the rest of the tail, through the limiter. The output thus runs
    `incoming_at_ms − incoming_from_ms` ahead of B's input, and ExoPlayer's
    position for B is the output time since the tail start.
  - **Reports**: `TransitionStarted` when ExoPlayer moves on to B (the tail is
    heard); the handover (`Ended(A)`, `TransitionedToNext(B)`,
    `Position(B, p)`) when B's ExoPlayer position reaches `handover_ms`
    (checked every 40 ms). Until then reports name A at `tail_start_ms` +
    the tail played (at the tail's average speed) and A stays in the
    playlist; after it B's position is ExoPlayer's + `incoming_from_ms −
    incoming_at_ms`, until the next seek or the clock correction below.
  - **Clock correction**: ExoPlayer's clock counts the sink's output, so
    after a mix it runs `incoming_at_ms − incoming_from_ms` ahead of B. Left
    alone, the next item would inherit that: ExoPlayer moves on by the lead
    too early (or late), and the renderer, whose position never goes back,
    holds the next item's position at the lead until real time catches up
    (seen as a 48 kHz B reported 14 s in at its start). So once the tail has
    sounded (B at `tail_len_ms`), a lead of 100 ms or more is taken out of
    the clock by `MixClockSink` (a `ForwardingAudioSink`), signalled as a
    position discontinuity so the renderer accepts it. The correction lasts
    until the sink re-bases its own clock at the next item's first frame
    (the raw clock steps by the lead it carried) or a flush. The backend
    keeps adding the offset until ExoPlayer's position shows the step, so the
    reported position stays continuous.
  - Seek before the handover goes back to A (B stays next, the tail stays
    armed unless the target is past the tail start); after it B seeks.
    `complete_transition` hands over and seeks B to its position, which
    drops the tail queued in the sink. A `set_next` during the overlap is
    held until the handover. Pause/play pause both (one output).
  - **Fallback (unclip)**: ExoPlayer can't change a clip in place, so a
    clipped current item is replaced by an unclipped copy at the same
    position (a short re-buffer from the cache or file, instead of losing
    A's end): when the core drops its transition (a `set_next` without
    `mix_in` — mode Off, sleep at end of track — or with no next, gapless
    off), once a transition into it has stopped sounding; and 2 s before
    the clip point when no tail is ready (render late or failed), so A plays
    to its end and B joins gaplessly. These waits (and the tail render's
    60 s lead) sleep until the time left at the playback speed, or until
    play state, a seek or the item changes; nothing runs while paused. A
    tail at another format than B turned out to have → B starts unmixed at
    A's clip point, logged (A's audio after `mix_out_ms` is then lost).
  - **Error recovery** during or after a mixed transition hands over and
    seeks B to its real position before re-preparing (the queued tail is
    gone and ExoPlayer's position is offset).
  - **Loudness-only records**: with the mode on AutoMix, on each window
    change the first of current / next / next-next (from the queue) with no
    usable record (`Query::TrackAnalysis`, version and duration checked in
    Rust) whose file is on the device (a `file:` URL or a core stream the
    core has whole; nothing is fetched for analysis) gets its first and
    last 60 s decoded with `MediaCodec` on one lowest-priority thread,
    `automix_loudness_record` (producer Device, fingerprint: the library
    duration, size and suffix unknown) and `Command::StoreAnalysisRecord`.
    One at a time, each track tried once per process; a window change during
    a pass runs (the newest) after it.
  - **Limitations**: the switch from live A to the tail is not crossfaded
    (desktop crossfades 10 ms), and MediaCodec doesn't trim encoder delay,
    so the tail can start a few tens of ms off A's clip point; B's format is
    taken from its container (an HE-AAC stream whose decoder doubles the
    rate falls back to a cut).
  - **Settings**: Audio → Transitions (Off / Crossfade / AutoMix, crossfade
    length while Crossfade is picked), in settings search.
- **Coordinator**: see *Sharing records* and *Coordinator worker*.

## Sharing records

Wire (`connect/wire.rs`, no protocol bump: all optional, and peers ignore
message types they don't know):

- `Welcome.analysis_store: bool` (`#[serde(default)]`): the hosted
  coordinator keeps records (`RoomConfig::analysis_store`). LAN rooms never
  set it, and the engine only believes it from the `Coordinator` tier.
- `AnalysisFetch { track_ids }` → coordinator (≤ `ANALYSIS_FETCH_MAX` = 64
  ids; more are dropped). Answered with `AnalysisRecords { records:
  [AnalysisEntry { track_id, record }] }`, in one or more frames of at
  most `ANALYSIS_BATCH_MAX` = 16 records and half the frame cap (4 MB) of
  JSON each, always at least one (possibly empty) frame.
  Only records of the coordinator's `ANALYSIS_VERSION`; misses are left out.
  With the worker on, the ids that missed (no record, or one without a
  beat grid that isn't the worker's) on the server it analyses go onto its
  priority queue (see *Coordinator worker*); the answer doesn't wait.
- `AnalysisUpload { records }` → coordinator (≤ 16; more are dropped). No
  answer.
- The engine: `Input::FetchAnalysis` / `Input::UploadAnalysis` go upstream
  only while the store is advertised; `Output::AnalysisStore(bool)` on
  every change (false on any upstream loss); `Output::AnalysisRecords`.

Coordinator (`hocket-coordinator/src/analysis.rs`):

- `<data-dir>/automix.sqlite` (in memory without `--data-dir`; off with
  `--no-analysis-store`), table `records(server, track_id, version,
  has_beats, produced_by, json, updated_at)`, primary key (server, track
  id). `server` is `wire::server_key` of the verified server: the
  credential's `server_url` (or `--verify-url`; with `--no-verify`, the
  Hello's), trimmed, without a trailing `/`, scheme and host lowercased
  (the path keeps its case) — the same normalisation as `scope_key`. Every verified user of a server reads and
  writes that server's records.
- Analysis messages are the coordinator's own business: never passed to
  the room, only from a socket whose device is an admitted member, at most
  60 per minute per socket (the rest are dropped). Store work runs on the
  blocking pool.
- Uploads are made honest (`analysis::sanitize_upload`: a claimed `Worker`
  becomes `Desktop`, only the coordinator's worker writes `Worker`;
  `created_at` is clamped to the coordinator's now), then validated
  (`analysis::validate`): version 1..=known, track id 1..=256 bytes,
  duration 1 ms..6 h, ≤ 40 000 beats, downbeats ≤ beats, ≤ 2 000 points per
  curve with a non-zero hop, positions within the duration (+1 s), finite
  numbers, short text fields, ≤ 256 KB of JSON. Then `compact`.
- A record replaces the stored one when it is strictly preferred
  (`preference`: version, beats, Worker > Desktop > Device, newer), or when
  it describes another file (fingerprint mismatch) and is at least as good
  on version, beats and producer, or is the worker's own. So a client with
  a stale fingerprint can't wipe the worker's beat grid. An older version
  never replaces a newer one.
- Quota: at most `--analysis-records-per-server` (400 000) records and
  `--analysis-mb-per-server` (2048 MiB of JSON) per server, and
  `--analysis-quota-mb` (8192 MiB) in all. Past a cap the least recently
  updated records are dropped, down to 90% of it.

Client (`core/handlers/analysis_share.rs`):

- With the store advertised for the current server and the mode on
  AutoMix, every 2 s (and when the store appears or an answer arrives) the
  actor asks for the current track and the next four that need a record
  (none usable, or only a phone's loudness-only one): one `AnalysisFetch`
  at a time, given up after 15 s. Each track asked is not asked again for
  30 minutes, answered or not (an unanswered fetch's tracks are freed).
  A miss on the coordinator worker's server gets that track analysed next,
  so the re-ask half an hour later usually finds it.
- Answers go through `store_analysis_record` (local store + replan when
  near the current track). Fetched records are never uploaded.
- Records this device produced — desktop analysis (`on_analysis_produced`)
  and anything a platform stores with `Command::StoreAnalysisRecord`
  (Android's loudness-only records) — are queued (≤ 64, newest per track)
  and uploaded in batches of 16 whenever the store is advertised; records
  of another server are dropped at that point.
- Offline nothing changes: planning reads only the local store.

## Coordinator worker

`hocket-coordinator/src/worker.rs`, off unless `--analysis-server` is set:

| Flag | Default | |
|---|---|---|
| `--analysis-server URL` | off | The Navidrome URL the worker fetches from. Records are keyed by `--verify-url` when set (as clients' are), otherwise by this URL, which must then be the one **your clients use**. A warning is logged when the two name different hosts. |
| `--analysis-user NAME` | — | A dedicated non-admin Navidrome user. |
| `--analysis-password-file PATH` | — | Its password (trailing newline trimmed); or env `HOCKET_ANALYSIS_PASSWORD`. Never logged. |
| `--analysis-nice N` | 10 | Nice value of the worker thread (0–19, Linux; 0 leaves it). The inference pool inherits it. |
| `--analysis-pause-secs S` | 2 | Pause between tracks. |
| `--analysis-rescan-hours H` | 6 | Walk the library again this long after a pass ends. |
| `--analysis-max-file-mb M` | 500 | Skip files larger than this (checked on the song's size and again after download). |

- One thread with its own single-threaded Tokio runtime, one
  `Analyser::new(1)`. A pass pages through `search3("")` (500 per page),
  skips tracks with a usable record that has beats or is the worker's own,
  tracks over 90 minutes (the analyser holds the whole track's spectrogram)
  and files over `--analysis-max-file-mb`, and for each of the rest: `download` (the original) into
  `<data-dir>/analysis-tmp/` (emptied at start and stop), `analyse_file(..,
  Worker)`, `source` = the song's fingerprint, `compact`, store, delete the
  file. A file that can't be decoded or analysed is logged at warn and
  remembered (by track and fingerprint) for the life of the process; the
  pass carries on. A failed download (or store write) is not remembered:
  the track is tried again next pass, and three in a row end the pass. A
  failed library walk or pass is retried after 10 minutes.
- **Priority queue**: `answer_analysis_fetch` pushes the ids an
  `AnalysisFetch` missed (no record, or one without a beat grid that isn't
  the worker's) onto `PriorityQueue`, but only when the socket's server key
  is the worker's (`--verify-url` if set, else `--analysis-server`). Only
  ids are taken (non-empty, ≤ 256 bytes), deduplicated, at most 256 queued
  (the oldest dropped: clients are untrusted), and ids the worker failed on
  or won't analyse in this process (bad file, too long or large, unknown to
  the server) are not queued again. The push is a short lock and a
  `Notify`, never a wait on the worker. The worker drains the queue oldest
  first before each library track and wakes for it while it waits between
  passes: each id's metadata comes from `getSong`, the usual checks apply
  (covered meanwhile, too long, too large, failed before), and the pause
  follows each track it analyses. A lookup or download the server fails
  (network, auth) leaves the rest for the next drain. The library walk
  skips what was analysed meanwhile.
- Cost ≈ 30 s of one core and ~480 MB peak RSS per track: a first pass
  over 10 000 tracks takes about three days (measured on an 8-core x86
  host against a remote Navidrome: ~150 tracks an hour, downloads included). `/health` shows `analysis`:
  `{state, passes, libraryTracks, remaining, analysed, failed, perHour,
  priorityQueue}` (`priorityQueue`: ids waiting in the priority queue);
  a progress line is logged at info every 5 minutes and at the end of each
  pass.
- Shutdown cancels between tracks; a track being analysed is not waited
  for beyond 3 s.

Deployment (compose, next to the existing coordinator service):

```yaml
services:
  hocket-coordinator:
    image: hocket-coordinator
    command:
      - --listen=0.0.0.0:7373
      - --data-dir=/data
      - --analysis-server=https://music.example.net
      - --analysis-user=hocket-analysis
      - --analysis-password-file=/run/secrets/hocket_analysis_password
    volumes:
      - hocket-data:/data
    secrets:
      - hocket_analysis_password
    # one analysis at a time: ~1 core and ~0.5 GB on top of the relay
    mem_limit: 1g
secrets:
  hocket_analysis_password:
    file: ./analysis-password.txt
volumes:
  hocket-data:
```

Create the Navidrome user first (Settings → Users, not an admin; it needs
access to every library the clients see).

## Harness

`hocket-mixlab` (its own workspace crate, `crates/hocket-mixlab`, so clap
and friends stay out of hocket-core; depends on hocket-core with
`automix-analysis`). Run with `cargo run -p hocket-mixlab --release -- …`:

- `analyse <files..> [--out dir] [--timing] [--cache]`: full records as
  JSON (`--cache`: fill the record cache next to each file instead).
- `pair <a> <b> [--mode crossfade|automix] [--crossfade s] [--out dir]`:
  analyse both (cached as `<file>.automix.json`: the record plus the
  tracker's raw output, so grid changes need no re-analysis), plan, and
  render 30 s of A before the overlap through 30 s of B after it to a
  32-bit float WAV with `render_tail` + `IncomingShaper` + the chain's
  `SoftLimiter` (B linearly resampled to A's rate if they differ), plus the
  plan JSON. Every render is checked: no non-finite samples, peak below
  0 dBFS, tail and total lengths exact.
- `batch <pairs.txt> [--corpus dir] [--out dir] [--no-render]
  [--reference dir]`: `pair` for each `<idA> <idB> [label]` line;
  `--reference src/automix/corpus` rewrites the committed records and
  reference plans.
- `stats <dir> [--pairs N] [--seed S] [--check]`: plan N random pairs of
  the directory's tracks and print the share of each kind, overlap /
  cut-from-A / skipped-B-intro / stretch quantiles and overlaps in bars;
  `--check` renders every pair in memory and checks it. Pairs with voice
  curves also get a voices line: how many plans put A's and B's voices on
  top of each other without the curves and with them, how many changed, and
  the median overlap before and after.
- `vocals <files..> [--tags]`: the voice detector alone on each file's
  first and last minute: its time and a digit (0–9) per second of voice
  probability.
- `--no-vocals` (any command): plan without the records' voice curves, so
  `pair` renders the same pair as before voice detection.

Corpus: `scripts/automix-corpus.sh [count]` downloads original files
(Subsonic `download`) from the test Navidrome server
(`~/.config/hocket/test-server.env`) into the gitignored
`automix-corpus/`: up to 3 tracks per genre, two runs of consecutive album
tracks, the tracks in the pair list, random songs to fill, and a
`tracks.json` of their metadata. Committed under
`crates/hocket-core/src/automix/corpus/`: `pairs.txt` (track ids and the
case each pair covers), `records/<id>.json` (no audio) and
`plans/<a>_<b>.json`. `tests/automix_corpus.rs` re-plans every pair from
the records and compares with the reference plans; after an intended
planner change, `HOCKET_BLESS=1 cargo test -p hocket-core --test
automix_corpus` rewrites them.
