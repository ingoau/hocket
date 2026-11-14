# Spike: rustypipe on Android (build size)

*14 Nov 2026. NDK 27.2.12479018, API 26, Rust 1.100, release profile matching the workspace (`lto = "thin"`, `codegen-units = 1`, `opt-level = 3`, `strip = true`).*

## Method

Two minimal `cdylib`s, cross-compiled with the same CC/AR/linker environment as `scripts/build-android-core.sh`:
- **base**: tokio, reqwest 0.12 (rustls, json, gzip, brotli), serde and serde_json. These are things `hocket-core` already links.
- **with rustypipe**: base plus `rustypipe = "=0.11.4"` (rustls-webpki-roots, `userdata`). It calls search, player, details, lyrics, playlist, album, artist, radio, the saved/liked/history readers and cookie auth, so none of that is stripped as dead code.

## The build blocker, and the workaround

`rquickjs-sys` 0.9.0 ships pregenerated bindings only for desktop and wasm targets. Building for `*-linux-android` fails with:

```
couldn't find file `…/rquickjs-sys-0.9.0/src/bindings/x86_64-linux-android.rs`
rquickjs probably doesn't ship bindings for platform `x86_64-linux-android`. try the `bindgen` feature instead.
```

This is upstream rustypipe issue #39. Two ways round it:
- Turn on the `bindgen` feature. That needs a host libclang, which this host doesn't have. The NDK only has a musl build.
- **What the spike did:** `[patch.crates-io] rquickjs-sys` with a local copy whose `bindings/{x86_64,aarch64}-linux-android.rs` are copies of the matching `*-unknown-linux-gnu` files. Both are LP64 Linux ABIs. It builds cleanly in 1m43s for both ABIs, including QuickJS's C code through the NDK clang.

Doing this in production needs a decision:
- vendor a patched `rquickjs-sys`
- generate the bindings once with bindgen and commit them
- or drop QuickJS altogether. The visionOS stream path needs no JS today (see [streams.md](streams.md)).

## Sizes (stripped `.so`)

| ABI | base | with rustypipe | delta | delta, gzip -9 |
|---|---|---|---|---|
| aarch64-linux-android | 3,040,592 | 8,445,728 | **+5.41 MB** | +2.24 MB |
| x86_64-linux-android | 3,387,920 | 9,520,720 | **+6.13 MB** | +2.41 MB |

QuickJS plus rustypipe's parsers and models make up most of the delta. Against the real `libhocket_android.so` the delta will be a bit smaller, since regex, time and similar crates overlap.

**Result: pass** (< 8 MB), but only with the `rquickjs-sys` workaround.

## First-resolve latency

This couldn't be measured on the emulator here (the host has none). On the host, cold, the visionOS path takes 0.70–0.93 s to the first audio bytes, and rustypipe `player()` takes 0.22–1.2 s (see [streams.md](streams.md)). An on-device measurement is in [manual.md](manual.md).
