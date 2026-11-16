//! A live smoke test of YouTube Music, run by hand only (it goes to the
//! network): anonymous search, stream resolve through the client chain, the
//! first ranged chunk of itag 140, then the whole stream in bounded ranges,
//! decoded on the desktop's native path (Symphonia, with the MP4 edit list's
//! priming trimmed).
//!
//! ```sh
//! cargo test -p hocket-core --features decode --test ytm_live -- --ignored --nocapture
//! ```

#![cfg(feature = "decode")]

use std::sync::Arc;
use std::time::Instant;

use hocket_core::provider::{Provider, StreamRequest};
use hocket_core::util::WallClock;
use hocket_core::ytm::client::InnerTube;
use hocket_core::ytm::http::{HttpMethod, HttpRequest, ReqwestYtmHttp, YtmHttp};
use hocket_core::ytm::{YtmProvider, ANON};

#[tokio::test]
#[ignore = "live network"]
async fn anonymous_search_resolve_download_and_decode() {
    let http: Arc<dyn YtmHttp> = Arc::new(ReqwestYtmHttp::new().unwrap());
    let it = Arc::new(InnerTube::new(http.clone(), Arc::new(WallClock), None));
    let p = YtmProvider::new(ANON, it, Arc::new(WallClock));

    let t0 = Instant::now();
    let found = p.search("daft punk get lucky", 5).await.expect("search");
    let track = found.tracks.first().expect("a song").clone();
    println!(
        "search: {} songs, {} albums, {} artists in {:?}; first: {} by {:?} ({} ms)",
        found.tracks.len(),
        found.albums.len(),
        found.artists.len(),
        t0.elapsed(),
        track.title,
        track.artist,
        track.duration_ms
    );

    let t1 = Instant::now();
    let s = p
        .resolve_stream(&track.id, &StreamRequest::default())
        .await
        .expect("resolve");
    println!(
        "resolve: itag {} via {:?}, {} bytes, gain {:?} dB, in {:?}",
        s.variant,
        p.chooser().last_good(),
        s.content_length.unwrap_or(0),
        s.gain_db,
        t1.elapsed()
    );
    assert_eq!(s.variant, "140");

    let t2 = Instant::now();
    let chunk = 1024 * 1024;
    let r = http
        .send(HttpRequest {
            method: HttpMethod::Get,
            url: s.url.to_string(),
            headers: vec![
                ("User-Agent".into(), s.headers["User-Agent"].clone()),
                ("Range".into(), format!("bytes=0-{}", chunk - 1)),
            ],
            body: None,
        })
        .await
        .expect("first chunk");
    println!(
        "first chunk: HTTP {} {} bytes ({:?}) in {:?}",
        r.status,
        r.body.len(),
        r.content_range,
        t2.elapsed()
    );
    assert_eq!(r.status, 206);
    assert_eq!(r.body.len(), chunk);

    // The whole stream as a pin keeps it (bounded ranges, re-resolving if a URL is refused):
    // Symphonia won't open a cut-off fragmented MP4, and playback reads the whole length anyway.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("whole.m4a");
    let t3 = Instant::now();
    let out = p.download_to_file(&s, &path).await.expect("download");
    println!("download: {} bytes in {:?}", out.bytes, t3.elapsed());
    assert_eq!(Some(out.bytes), s.content_length);
    let opened = hocket_core::audio::decoder::open_file(&path).expect("open");
    println!(
        "decode: {:?}, priming trim {} frames, {:?} frames in all",
        opened.spec, opened.trim.skip_start, opened.duration_frames
    );
    assert_eq!(opened.trim.skip_start, 1600, "the edit list's priming");
    drop(opened);
    let mut frames = 0u64;
    let mut peak = 0f32;
    let spec = hocket_core::audio::decoder::decode_file(&path, |spec, pcm| {
        frames += (pcm.len() / spec.channels) as u64;
        peak = pcm.iter().fold(peak, |m, x| m.max(x.abs()));
    })
    .expect("decode");
    let secs = frames as f64 / f64::from(spec.sample_rate);
    println!("decoded {secs:.1} s, peak {peak:.2}");
    assert!(secs > 5.0, "a few seconds decoded");
    // The edit list's duration: the priming and the tail padding are trimmed.
    let expect = track.duration_ms as f64 / 1000.0;
    assert!((secs - expect).abs() < 1.5, "{secs} s vs {expect} s");
    assert!(peak > 0.05, "not silence");
}
