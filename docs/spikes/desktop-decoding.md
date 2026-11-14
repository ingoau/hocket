# Spike: decoding itag 140 on desktop (Symphonia 0.5.5)

*14 Nov 2026. File: `4D7u5KF7SP8` itag 140, 5,983,881 bytes, downloaded in full (see [streams.md](streams.md)).*

The test opened the file the same way `audio/decoder.rs::open_media_source` does: `.m4a` hint, `enable_gapless` both off and on. It then:
1. walked the MP4 boxes by hand
2. decoded the whole file
3. did `SeekMode::Accurate` seeks to 0, 1, 30, 95.5, 200 and 367.7 s, each on a fresh reader

## File layout

- `ftyp`, then `moov` (with `mvex`/`trex`), then `sidx` (38 refs), then 38 `moof`/`mdat` fragments. This is fragmented MP4 (DASH).
- **No `iTunSMPB`.** The only `ilst` tag is the encoder string "Google".
- **An edit list is present:** `elst` v0, one entry: `segment_duration 16300536`, **`media_time 1600`**.
- `mdhd`/`mvhd`: timescale 44100, duration 16300536 (369.627 s). `approxDurationMs` from the player response is 369626, which matches.
- The final packet's duration is 56 frames, so the media ends at 16302136 = 1600 + 16300536.

So YouTube's AAC-LC has **1600 frames of priming**: not the 1024 or 2112 that `padding.rs` describes for iTunes, and not a multiple of the 1024-frame AAC packet. It also has **968 frames of tail padding** (16303104 decoded minus 16302136).

## Symphonia results

- **Decoding works:** 15921 packets give 16,303,104 frames (369.685 s) in 0.36 s.
- **Seeking works.** Every accurate seek returns the packet boundary at or below the target, for example 30 s gives `actual_ts` 1321984 for 1323000 required, in <0.6 ms. The decoder already uses `actual_ts` (`decoder.rs:287-316`).
- `enable_gapless` changes nothing. Symphonia 0.5.5's isomp4 demuxer **parses `edts`/`elst` but never uses them**. `codec_params.delay` and `padding` are `None`, and no packet has `trim_start`/`trim_end`.
- `codec_params.n_frames` is 16300536, which leaves out priming and padding, but 16303104 frames come out. The repo's `duration_frames` is therefore 2568 frames (58 ms) short of what plays.
- `codec_params.channels` is `None`. The repo falls back to 2, which is fine for these stereo files.

## How the repo would treat it today

`decoder.rs:171-191` finds no `iTunSMPB`, and `params.delay` is `None`, so `Trim::default()` applies and **nothing is trimmed**. Every YTM track would play:
- 1600 frames (36.3 ms) of encoder priming at the start
- 968 frames (22 ms) of padding at the end

That's an audible gap or click on gapless album sides. **Gapless fails as is.**

The fix is small (Phase 4):
1. Parse the `moov/trak/edts/elst` single-entry case in `padding.rs` (pure and unit-testable, like `parse_itunsmpb`).
2. Map it to `Trim { skip_start: media_time, skip_end: decoded_total − (media_time + segment_duration) }`. The tail is known from the last packet's `dur`, or as total packets × 1024 − end.
3. Apply it before falling back to iTunSMPB.

## Media3 comparison (from source, not run)

- Source: `androidx/media` 1.11.1, the version the app pins, `FragmentedMp4Extractor.java:1336-1439`.
- For one edit where `segment_duration + media_time ≥ track duration` (true here), Media3 sets `edtsOffset = media_time` and subtracts it from every sample's presentation time.
- So **Media3's timeline 0 is media sample 1600**, and Symphonia's timeline 0 is media sample 0.
- **AutoMix correction for itag 140: +1600 frames (36.28 ms at 44.1 kHz).** In other words, Symphonia time = Media3 time + 1600/44100 s, until the desktop trims by `elst`. After the fix, both timelines start at sample 1600 and the correction becomes 0.
- Record the correction per `(itag, elst media_time)`, not as a constant. 141 (and anything else) has to be measured, because 141 wasn't available here.
- Unknown: does Media3 drop exactly 1600 frames, or whole buffers with negative timestamps? Those would be frames 0 and 1, so 2048. That changes what you hear, not the timeline. Check on the device (see [manual.md](manual.md)).
- The fMP4 path doesn't apply tail padding, so Media3 probably plays the 968 padding frames too.

## Result

| Check | Result |
|---|---|
| Decode fMP4 itag 140 | **Pass** |
| Seek | **Pass** |
| Gapless without iTunSMPB | **Fail** today. Fixed by an `elst` parser (≈0.5 day). |
| Priming matches Media3 | Not today (off by 1600 frames). Known and correctable. |
