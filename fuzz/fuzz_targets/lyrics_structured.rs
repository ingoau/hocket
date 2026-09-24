//! OpenSubsonic structured / enhanced lyrics → `api::Lyrics`, both the
//! direct `lyrics::raw` parse and the actor's path (subsonic `types.rs`
//! envelope → JSON → `lyrics::raw`), which must agree. Seeds carry
//! `byteStart`/`byteEnd` offsets the fuzzer bends out of range, reverses and
//! points into the middle of multi-byte characters.
#![no_main]

mod common;

use hocket_core::api::LyricsSource;
use hocket_core::lyrics::{
    adapt_entry, adapt_list, adapt_response, apply_offset, from_cache_json, raw, set_user_offset,
    to_cache_json, LyricsListResponse,
};
use hocket_core::subsonic::client::parse_envelope;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let parsed = LyricsListResponse::parse(text);
    if let Ok(resp) = &parsed {
        for e in resp.entries() {
            common::check_lyrics(&adapt_entry("t", e, LyricsSource::Server));
        }
        if let Some(l) = adapt_response("t", resp) {
            common::check_lyrics(&l);
            let mut pos = common::positions(&l);
            common::check_cursor(&l, &pos);
            pos.reverse();
            common::check_cursor(&l, &pos);

            let json = to_cache_json(Some(&l));
            assert_eq!(
                from_cache_json(&json).expect("cache json reads back"),
                Some(l.clone())
            );

            for off in [i32::MIN, -1500, 1500, i32::MAX] {
                let mut o = l.clone();
                set_user_offset(&mut o, off);
                common::check_cursor(&o, &pos);
                let baked = apply_offset(&o);
                assert_eq!(baked.offset_ms, 0);
                common::check_lyrics(&baked);
            }
        }
    }

    // The actor's path: the subsonic client's envelope types, re-read as
    // `lyrics::raw` through JSON (core/handlers/library.rs).
    if let Ok(r) = parse_envelope(data) {
        let entries = r
            .lyrics_list
            .map(|l| l.structured_lyrics)
            .unwrap_or_default();
        let v = serde_json::to_value(&entries).expect("subsonic lyrics serialise");
        let raw: Vec<raw::StructuredLyrics> =
            serde_json::from_value(v).expect("subsonic lyrics convert to lyrics::raw");
        let via_actor = adapt_list("t", &raw, LyricsSource::Server);
        if let Ok(resp) = &parsed {
            assert_eq!(
                via_actor,
                adapt_response("t", resp),
                "the actor's conversion and the direct parse disagree"
            );
        }
    }
});
