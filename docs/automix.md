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
  key.rs        Camelot codes and the Harmony of two keys (pure)
  order.rs      mixability score and pick for AutoMix's order (pure)
  render.rs     pure DSP: render_tail(..), IncomingShaper, fade curves, shelving EQ automation
  stretch.rs    time-stretch wrapper used by render_tail
  loudness.rs   loudness envelope + silence edges from PCM (pure, no decoding)
  analysis/     feature "automix-analysis": decode + Beat This! + loudness -> AnalysisRecord
    mod.rs      Analyser (analyse_file / analyse_pcm), streaming accumulator
    grid.rs     bpm and beat_confidence from the tracker's beats/downbeats
    key.rs      chroma key estimator (streaming, track + head + tail)
    vocals.rs   musicnn singing-voice detector
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
    key: Option<MusicalKey>,      // the whole track's key (None: loudness-only)
    head_key: Option<MusicalKey>, // the first ~60 s
    tail_key: Option<MusicalKey>, // the last ~60 s
    bpm: Option<f32>,             // None when no beat analysis or not trustworthy
    beats_ms: Vec<u32>,           // empty for loudness-only records
    downbeats_ms: Vec<u32>,       // subset of beats_ms
    beat_confidence: f32,         // 0..1; planner gates on it
    created_at: f64,              // epoch ms
}
struct SourceFingerprint { duration_ms: u32, size_bytes: Option<f64>, suffix: Option<String>, transcoded: bool }
struct LoudnessCurve { start_ms: u32, hop_ms: u32, lufs: Vec<f32> }
struct VocalCurve { start_ms: u32, hop_ms: u32, voice: Vec<f32> }
struct MusicalKey { tonic: u32 /* 0 = C .. 11 = B */, minor: bool, strength: f32 /* profile correlation */ }
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
  probabilities and key strengths to 0.01, keys with a tonic out of range
  dropped).
- Version 3 added the keys; version-2 records are ignored, so the worker
  re-analyses the library after a coordinator update.

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
    the late-edit fade, since A may already be clipped there. The same track
    again is the exception: it drops the plan like a context rule.
  - A context rule (mode Off, repeat-one, sleep end-of-track, Cast, exclusive
    output) drops the plan even after lock-in, and no plan is sent. A backend
    that clipped A at `mix_out_ms` plays A out unclipped when the follow-up
    arrives without `mix_in` (Android un-clips itself).
- **Next before handover** → `backend.complete_transition()`; the core then
  treats B as current. **Next after handover** is an ordinary skip.
- `should_mix` returns false (→ `mix_in: None`) for: the same track again
  (same server and track id: a duplicate queue entry); same album in track
  order (same album id, same disc and track n+1, or disc n+1 track 1);
  gapless tags or a gapless join; genres Classical / Opera / Orchestral,
  Audiobook / Podcast / Spoken word, Live (genre, "live" release type, album
  title marker, or track title marker), Soundtrack / Score; repeat-one; sleep timer end-of-track; Cast
  renderer active; bit-perfect/exclusive output; mode Off.
  - Genres match whole words, case-insensitively, over the genre string
    (so "Hardcore" isn't "score"). Release types come from the album's
    OpenSubsonic `release_types` (`live`, `soundtrack`, `spokenword`,
    `audiobook`, `audio drama`, `interview`).
  - Live markers: an album title counts with "(Live", "[Live", "- Live", a
    trailing "Live" or "Live at/in/from/on" (`has_live_marker`). A track
    title only with a bracketed or dashed one, "(Live…", "[Live…",
    "- Live…" (`has_track_live_marker`), so "The Way I Live" or "Live
    Forever" still mix. Both give `LiveRelease`.
  - The same track again is not mixed even after lock-in (like a context
    rule, not the edit-wins pair rules): the plan is dropped and the
    backend un-clips A and joins gaplessly, as for repeat-one.
  - The library has no gapless tag (`iTunPGAP`) yet: `MixTrack::gapless` is
    `None` until it does. The gapless *join* is judged from both records: A
    audible to within 50 ms of its end and B from within 50 ms of its start,
    both at that edge within 10 LU of their integrated loudness.
- Scrobbling, played-ms, sleep timer, Connect stamps and media session all key
  off `TransitionedToNext` as today, so they switch at the handover.
- Connect handoff during a transition: the target starts B at B's position,
  unmixed.
- **Outcome log** (`core/handlers/mix_log.rs`): one info line per planned
  transition on `hocket::player` ("automix outcome", also in the
  diagnostics' recent log) once its outcome is known, with the from/to queue
  keys and track ids, plan kind, `tail_len_ms` and `outcome`: `mixed`
  (started, handed over, and B not left within 20 s), `skipped` (the user
  left during the overlap, Next completing it included, or within 20 s
  after the handover: `after_handover_ms`), `not_started` (A ended or B took
  over without `TransitionStarted`), `replanned` (the follow-up or its plan
  was dropped before the tail), `failed` (B failed during the overlap),
  `stopped` (playback stopped or moved to another device during the
  overlap). A skip away from A before its tail is not logged (debug only).
  A replan of the same pair (a record arrived) is not an outcome.

## Order (order.rs, core/handlers/mix_order.rs)

With AutoMix on, when the order is Hocket's to choose, the device that owns
playback biases what plays toward pairs that blend. Decisions (issue #256):

- **Whose order**: entries that are not in the "Next in queue" group, not
  added by the user (`QueueItem.added`), not placed already
  (`QueueItem.settled`: dragged by the user, or put there by an earlier
  decision) and either autoplay's (radio and
  "continue playing" are autoplay providers: similar songs, sonic
  similarity, top songs, random) or a shuffled context's (`doc.shuffle`).
  An album, playlist or selection played in order, anything the user
  queued or added, and everything after the first entry that isn't ours
  are never moved. Repeat One and Repeat All turn it off, as do the context
  rules that stop mixing (sleep end-of-track, Cast, exclusive output).
- **What is chosen**: the follow-up of each of the first `ORDER_AHEAD` = 10
  upcoming entries, decided ahead as a chain (the next entry's follow-up,
  then that follow-up's, and so on), so the top of the queue is settled
  before the listener gets there: playing a shuffled queue orders its top
  at once, and in steady play each track decides one new pair, about ten
  entries down, where a move is out of the way. Each decision marks the
  follow-up `settled` in the document, also when it stays where it was, so
  a decided pair stays: Previous and Next again find the order they left,
  records that would now choose differently change nothing, and a new
  transport owner (another device, or this one after a restart) doesn't
  decide it again. A shuffle clears the marks; an edit that gives an entry
  a follow-up that isn't settled decides that pair again. The reducer
  refuses a promotion of, or past, an entry the user added or placed, a
  settled or an unavailable one, and a promotion isn't replayed after a
  conflict (the owner decides again on the new document), so neither
  undoes a drag. The next playable entry's follow-up is the one playback
  depends on: the current track's tail start was committed against the
  next track when it was preloaded (`mix_out_ms`), and the follow-up
  decides the next track's own tail start, so that pair isn't decided once
  the plan has locked in or a mix is running.
  After a skip nothing is decided until the track has played `SETTLE_MS` =
  15 s (or half the way to its tail's lock-in, when that is sooner), so a
  burst of skips moves nothing. The chain waits for a record on its way
  (the entry's or a candidate's: in a fetch in flight or about to go out,
  or being analysed here) rather than decide without it; an entry whose own
  record isn't coming keeps its follow-up undecided, in case it comes
  later. Checked on every document change, ownership gain, record
  arrival and AutoMix being turned on; a decision held back by something that passes (a skip just now,
  a load pending, a record on its way, offline) is asked again on position
  reports. Each decision is a
  synced `SessionOp::PromoteForMix { key, after }`: the entry moves to
  directly after `after` (or stays there, when it already is), past
  orderable entries only, keeping its ordinal, so turning shuffle off still
  restores the context's order, and is marked `settled`.
  The preloaded next source is then replanned against its new follow-up.
- **How much randomness**: an entry's candidates are the `ORDER_WINDOW` = 6
  entries after it in shuffle (or autoplay) order, which stays the source
  of randomness. Each is scored by `order::mix_score` from its plan with
  that entry: beat-matched blends 0.6–0.8 (longer better, stretch costs up
  to 0.1), beat-aligned fades 0.3, other fades 0.15, not mixed 0; voices
  apart +0.1 or shaped clash −0.35 (about a beat-aligned fade's worth); Camelot compatible +0.1, unknown +0.03,
  near 0, clash −0.15 (halved for fades). A candidate without a record
  counts 0.3 (a typical random pair), so unanalysed tracks are neither
  preferred nor pushed back. The entry shuffle put first keeps its place
  unless another scores at least 0.15 more (`PROMOTE_MARGIN`); an entry
  passed over 3 times (`MAX_PASSES`, per session in memory) plays next
  regardless, so a track that mixes with nothing is delayed, never
  starved. A pick moves at most 5 places.
- **Records ahead**: phones only read records, so the actor fetches the
  records of the next `ORDER_AHEAD + ORDER_WINDOW` = 16 entries (every
  entry the chain looks at) in batched `AnalysisFetch`es, nearest first; a
  planner run per candidate is microseconds, and records are a few KB each.

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
   - **Keys** (`key::harmony`, both records with a key of strength ≥ 0.7;
     A's tail key and B's head key when trusted, else the track's): on the
     Camelot wheel, *compatible* is the same key, ±1 (a fifth) or the
     relative major/minor; *near* is ±2 or a diagonal (±1 and the other
     mode); anything else *clashes*. Compatible or unknown keys plan as
     above. Near keys never take 16 bars. Clashing keys blend over 4 bars
     (2 if 4 don't fit) and take the shortest beat-aligned fade (whole bars
     of at least 4 s). The voice rules below then work within that.
   - **Voices** (both records have voice curves; otherwise all of the above
     unchanged): one voice at a time. A point is voiced at probability
     ≥ 0.5; over a candidate's overlap (output time; A through the tempo
     ramp, B from `incoming_from_ms`) the voices are apart when A's last
     voiced point is no more than 0.5 s after B's first. Blends, in order:
     (a) the usual 8/16 bars when apart, the handover moved, if the midpoint
     isn't between the voices, to B's downbeat between them nearest the
     midpoint (so B's voice comes in on or after it; bass swap over the bar
     before it, never before B enters; high cut from it). With no voice of
     A in the overlap the handover stays mid-overlap: B may sing over A's
     instrumental. (b) Else the first of 4 and 2 bars that keeps them apart
     (same handover rule). (b′) Without a fixed tail start, the bar
     counts of (a) and (b) placed 1, 2, … whole bars earlier in A, fewest bars back first,
     as long as at most 20 s of audible A is cut (A's voice curve must cover
     the overlap for any placement to count as apart: outside it "no
     voice" proves nothing): the handover
     then lands in an instrumental break before A's last vocal, usually its
     final chorus. (c) Else the fewest bars lasting ≥ 4 s, placed where
     both voices sound together least (each bar further back, up to 20 s,
     has to save 0.5 s of it), with the bass swap from B's entry
     over
     half the way to the handover and A's high shelf moved down to 2 kHz
     (−12 dB) from B's entry to the handover. Beat-aligned fades drop bars
     (down to the 4 s minimum) until the voices are apart, else take the
     shortest.
4. Rescue: if no candidate fits (A too short, overlap would cut > 20 s of
   audible A), plain 6 s `Fade`.

`late_edit_fade` builds the lock-in replacement plan: `Fade` from the
committed `tail_start_ms`, overlap max(2 s, crossfade / 2). `late_fade_at`
turns a plan into a `Fade` from a later tail start (Android, when a seek
leaves no time to render the planned tail; see Fallback below).

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
  transcode. `analyser` is `"beat-this-small/1.1.0+musicnn-mtt+kk-chroma"`.
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
  blends are at most 32 s, and an 8-bar blend moved back by the planner's
  20 s cut limit still fits at 120 BPM; a 60 s tail was tried, see *Voices
  on the corpus*); a patch's voice probability is the strongest of
  the vocal tags (vocal, vocals, singing, voice, male/female …, choir,
  opera) over that plus the strongest no-vocal tag (`v / (v + n)`), held
  over the patch's 3 s in the 100 ms `VocalCurve`. Candidates considered:
  YAMNet (Apache-2.0) and EfficientAT `mn10_as` (MIT) read AudioSet's
  Singing / Rapping / Speech classes, but on the corpus's vocal and
  instrumental versions of the same songs they told them apart only on
  10 s windows (YAMNet not at all); Essentia's voice/instrumental models are
  CC BY-NC-SA; the dedicated SVD research models publish no weights or no
  licence; Demucs-class separation is too slow on the worker.
- **Key** (`key.rs`): streaming over the same mono downmix the tracker
  gets, at the source rate. A 2^n-point Hann FFT (~0.37 s; 16 384 points at
  44.1/48 kHz) every half frame; frames below −60 dBFS RMS skipped. Each
  spectral peak between C2 and C7 (65–2093 Hz, at least −50 dB below the
  frame's strongest, frequency refined by a parabola through the log
  magnitudes) adds `ln(1 + 100·m/max)` to a 120-bin (10-cent) pitch
  histogram, and `0.9^(h−1)` of that to the bins of the notes it could be
  the 2nd–6th harmonic of. Each frame's histogram is normalised and summed
  into 2 s blocks. At the end the track's tuning is the weighted circular
  mean of the sub-semitone position over all blocks; the blocks of the
  whole track, the first 60 s and the last 60 s are folded onto 12 pitch
  classes around it (cos² weights) and correlated (Pearson) with the 24
  rotations of Krumhansl & Kessler's major and minor profiles. The best is
  the key, its correlation the `strength`. Cost: ~1 ms of CPU per second of
  audio (~0.25 s for a 4-minute track, ~1 % of a full analysis), a few
  hundred KB of block histograms for an hour of audio.
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
- **Keys, voices and order on the corpus** (version 3, 289 tracks: the 100
  above plus random library songs; `stats --pairs 5000 --seed 1`, `order
  --sessions 20`; numbers from November 2026, issues #256–#258):
  - Keys against Essentia's `KeyExtractor` (profiles bgate / edma, which
    agree with each other on 82 %): same key 63 % / 59 %, Camelot-compatible
    (same, a fifth, relative) 85 %. Strength ≥ 0.7 on 89 % of tracks, 89 %
    of those compatible with edma; 55 % of the weaker ones. Without the
    harmonic folding 46 % same, 76 % compatible, a quarter a fifth off. The
    last minute's key differs from the track's on 58 tracks, incompatibly on
    15 (5 %): modulations, the reason for head and tail keys.
  - Random pairs: compatible keys 16 %, near 14 %, clashing 55 %, unknown
    16 % (random keys would clash on 2/3). Blends between clashing keys are
    capped at 4 bars: tempo-blend overlap p90 16.0 s → 11.7 s.
  - Voices: of beat-matched blends with voices on both sides, `main`'s
    planner shaped 155 of 412 (38 %) as clashes; moving blends back into a
    break and placing the rest where voices overlap least: 128 of 441 (29 %),
    and 29 pairs more get a blend at all. In the shaped ones both voices
    sound together for a median 3.0 s (p90 5.9 s) of a 5 s overlap. A 60 s
    tail voice curve changed none of the 5000 plans; letting blends cut up to
    40 s of A as well saved only 15 more (118 → 103), so the curve stays at
    36 s and the cut at 20 s.
  - Order (shuffled sessions through all 289): beat-matched blends 8.4 % of
    transitions → 19.8 %, blends without a shaped voice clash 6.0 % →
    16.6 %, compatible keys 15.5 % → 20.3 %, clashing 55.5 % → 49.0 %. Tracks end up a median
    3 places from their shuffled position (a pick moves at most 5 forward;
    a track passed over can drift further back, the most seen was 5).

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
- **Android** (`ExoBackend.kt`, `MixProcessor.kt`, `OutputRate.kt`,
  `PcmDecoder.kt`, `LoudnessRecorder.kt`; Rust side
  `crates/hocket-android/src/dsp.rs`, `resampler.rs`):
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
    When B is at another channel count the sink can't keep its
    `AudioTrack`: it drains A, flushes once still naming A (the new
    configuration takes over after that flush), then flushes for B with no
    end of stream in between. So every flush between an end of stream and
    the next frame belongs to the join.
  - **Output rate** (`OutputRate`): the sink rebuilds its `AudioTrack`
    whenever its output format changes, and a rebuild plays the old track
    out to silence before the new one starts: a gap at every join between
    tracks at different rates, in the middle of A's audio when the join is
    a mix. So the processor keeps one output rate: the first stream's after
    a reset when it is 44.1 or 48 kHz, else the device's
    (`PROPERTY_OUTPUT_SAMPLE_RATE`). A library at one rate is never
    resampled. Other streams go through a `StreamResampler` (JNI; a
    Kaiser-windowed sinc with one row of taps per output phase, ~80 dB
    stopband), after the mixer, so the mixer's sample-exact item accounting
    is untouched. Output frame `j` is input frame `j × in / out` exactly,
    and only the kernel's lookahead (~1.5 ms) is held back.
    - A gapless join at the same format goes on through the same resampler:
      no seam inside continuous audio. What it held back comes out after
      the flush, which moves B's clock by ~1.5 ms. At the end of the
      playlist (`MixClockSink.playToEndOfStream` tells the processor) they
      come out at the end of stream.
    - When the next stream is at another format (the sink configures it
      while A plays), A's end of stream lets out A's last frames, with the
      armed tail's `after` (A from the tail start through A's chain, at A's
      format) as lookahead when the mix follows. B's resampler is primed
      with the tail's `lead` (A just before the tail start, at B's format;
      its frames line up with the tail's, both from one resampler run). So
      the switch from A to the tail is as seamless as at one rate.
    - The tail render decodes A from 60 ms before the tail start for that
      (and so the tail's own first frames have filter history).
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
    EQ), channel map, resampling to B's rate (the `StreamResampler`, primed
    with the 60 ms before the tail start), then `render_tail`; plus the
    `lead` and `after` above.
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
    armed unless the target is past the tail start, which takes a late fade,
    below); after it B seeks.
    `complete_transition` hands over and seeks B to its position, which
    drops the tail queued in the sink. A `set_next` during the overlap is
    held until the handover. Pause/play pause both (one output).
  - **Fallback (unclip)**: ExoPlayer can't change a clip in place, so a
    clipped current item is replaced by an unclipped copy at the same
    position (a short re-buffer from the cache or file, instead of losing
    A's end): when the core drops its transition (a `set_next` without
    `mix_in` — mode Off, sleep at end of track — or with no next, gapless
    off), once a transition into it has stopped sounding; and 2 s before
    the clip point when no tail is ready and no late fade fits (below), so A
    plays to its end and B joins gaplessly.
  - **Late fade**: when the planned tail can't be ready in time (a seek or a
    load to within 2 s of the tail start, or past it; or a render still
    running 2 s before it), A is rebuilt the same way but clipped 3.5 s after
    its position instead, and the plan becomes `late_fade_at` from there: a
    `Fade` of the planned overlap clamped to 2–4 s (never longer than the
    planned overlap, which fits B; less as A runs out, none under 1 s left), B from the plan's `incoming_from_ms`. Its tail is only a
    few seconds of A, so it renders in the 1.5 s before the 2 s margin. The
    reports carry the late plan's numbers (`TransitionStarted`, A's position
    from the new tail start), so the core follows. Once only: a late fade
    whose tail isn't ready either, or after a failed render, plays A out. A
    `set_next` with the planned plan is retargeted to the late clip; a seek
    to within 3.5 s of the planned tail start mixes late at once (no second
    re-buffer from the guard); a seek back to before that restores the
    planned clip and mix while A is still clipped (once A was unclipped to
    play out, it stays unclipped); a seek with a ready tail still ahead keeps
    it.
  - These waits (and the tail render's
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
- `AnalysisFetch { wants: [AnalysisWant { track_id, key }] }` → coordinator
  (≤ `ANALYSIS_FETCH_MAX` = 64; more are dropped). Answered with
  `AnalysisRecords { records: [AnalysisEntry { track_id, key, record }] }`,
  in one or more frames of at
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

- `<data-dir>/analysis.sqlite` (in memory without `--data-dir`; off with
  `--no-analysis-store`), table `records(server, track_id, version,
  has_beats, produced_by, json, updated_at)`, primary key (server, track
  id), holding a provider-aware fingerprint (`wire::AnalysisKey`): `server`
  is the provider (`navidrome:` plus `wire::server_key` of the account's
  URL: trimmed, without a trailing `/`, scheme and host lowercased) and
  `track_id` the file within it (`<track id>|<size>|<suffix>`). Clients
  send the key with every fetch and upload; every admitted identity using a
  server reads and writes that server's records. (Records keyed by
  `(server, track)` lived in `automix.sqlite`, which is no longer read.)
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
  actor asks for the current track and, of the next sixteen (as far as
  AutoMix's order looks, see *Order*), those that need a record (none
  usable, or only a phone's loudness-only one), nearest first: one
  `AnalysisFetch` of at most `ANALYSIS_BATCH_MAX` = 16 at a time, so its
  answer is one frame (an empty one, nothing found, answers it too), given
  up after 15 s. Each track asked is not asked again for
  30 minutes, answered or not (an unanswered fetch's tracks are freed).
  A miss on the coordinator worker's server gets that track analysed next,
  so the re-ask half an hour later usually finds it.
- Answers go through `store_analysis_record` (local store + replan when
  near the current track). Fetched records are never uploaded.
- A record of another `ANALYSIS_VERSION` is dropped before anything else
  (whatever file it is for) and warned about at warn level once per
  version per session, naming both versions, the producer and where it came
  from (`coordinator`, `analysis`, `platform`): a coordinator deployed from
  older code otherwise turns every mix into a plain fade with nothing above
  debug level. A record for another file is dropped at debug, per track.
  `curl /health` shows the coordinator's version under `analysis.version`.
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
| `--analysis-server URL` | off | The Navidrome URL the worker fetches from, with its own account. Records are keyed by this URL, which must be the one **your clients use**. |
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
  the worker's) onto `PriorityQueue`, but only when the key's provider is
  the worker's (`--analysis-server`). Only
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
  `{version, state, passes, libraryTracks, remaining, analysed, failed, perHour,
  priorityQueue}` (`priorityQueue`: ids waiting in the priority queue;
  `version`: the `ANALYSIS_VERSION` this build stores and serves, shown
  whenever the store is on, worker or not);
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
