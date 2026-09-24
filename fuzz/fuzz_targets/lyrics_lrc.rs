//! External LRC bodies (LRCLIB) → `LrcDocument` → line-tier `Lyrics`, plus
//! the plain-text fallback, and the cursor over the result.
#![no_main]

use hocket_core::api::{LyricsSource, LyricsTier};
use hocket_core::lyrics::{apply_offset, from_plain_text, locate, lrc_to_lyrics, parse_lrc};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let body = String::from_utf8_lossy(data);
    let doc = parse_lrc(&body);
    assert!(
        doc.lines.windows(2).all(|w| w[0].start_ms <= w[1].start_ms),
        "lines sorted by start"
    );
    if let Some(l) = lrc_to_lyrics("t", &doc, LyricsSource::External) {
        assert_eq!(l.tier, LyricsTier::Line);
        assert_eq!(l.lines.len(), doc.lines.len());
        for w in l.lines.windows(2) {
            assert!(w[0].start_ms <= w[1].start_ms, "offset keeps order");
            assert_eq!(w[0].end_ms, w[1].start_ms);
        }
        let _ = apply_offset(&l);
        for p in [
            f64::NEG_INFINITY,
            -1.0,
            0.0,
            1e3,
            6e4,
            4.3e9,
            f64::INFINITY,
            f64::NAN,
        ] {
            let c = locate(&l, p);
            assert!(c.line.is_none_or(|i| i < l.lines.len()));
        }
        for line in &l.lines {
            let c = locate(&l, f64::from(line.start_ms.unwrap_or(0)));
            assert!(c.line.is_some());
        }
    }
    let _ = from_plain_text("t", &body, LyricsSource::External);
});
