//! "Never panics on arbitrary input" property tests for the lyrics parsers
//! and cursor, plus regression tests for inputs the fuzz targets
//! (`fuzz/fuzz_targets/lyrics_*.rs`) found.

use proptest::prelude::*;

use super::*;
use crate::api::{LyricLine, LyricSyllable, LyricsTier};
use crate::connect::wire::arbitrary_json::{json, mutated};

const WORDS: &[&str] = &[
    "subsonic-response", "status", "ok", "lyricsList", "structuredLyrics", "kind", "main",
    "translation", "pronunciation", "lang", "und", "xxx", "offset", "synced", "line", "start",
    "end", "value", "agents", "id", "role", "bg", "voice", "group", "name", "cueLine", "index",
    "agentId", "cue", "byteStart", "byteEnd", "displayArtist", "displayTitle", " ", "a ", "é",
    "日本", "ti", "tle",
];

const ENHANCED: &str = r#"{"subsonic-response":{"status":"ok","lyricsList":{"structuredLyrics":[{"kind":"main","lang":"fr","synced":true,"offset":-250,
 "agents":[{"id":"v1","role":"main"},{"id":"bg","role":"bg"}],
 "line":[{"start":1000,"value":"Café naïve—déjà vu"},{"start":4000,"value":"日本語の歌"}],
 "cueLine":[{"index":0,"start":1000,"end":3000,"value":"Café naïve—déjà vu","agentId":"v1","cue":[
   {"start":1000,"end":1400,"byteStart":0,"byteEnd":4,"value":"Café"},{"start":1400,"end":1800,"byteStart":6,"byteEnd":11,"value":"naïve"},
   {"start":1800,"end":2400,"byteStart":15,"byteEnd":19,"value":"déjà"},{"start":2400,"end":3000,"byteStart":21,"byteEnd":22,"value":"vu"}]},
  {"index":0,"value":"(oui)","agentId":"bg","cue":[{"start":2000,"end":2500,"value":"(oui)"}]},
  {"index":1,"start":4000,"end":6000,"value":"日本語の歌","cue":[{"start":4000,"byteStart":0,"byteEnd":5,"value":"日本"},
   {"start":4500,"byteStart":7,"byteEnd":3,"value":"語の"},{"start":5000,"byteStart":12,"byteEnd":14,"value":"歌"}]}]},
 {"kind":"translation","lang":"en","synced":true,"line":[{"start":1000,"value":"Naive cafe"},{"start":4100,"value":"Japanese song"}]}]}}}"#;

const LRC: &str = "[ar:x]\n[offset:+250]\n[00:12.00]First\n[00:17.20][01:02.5]Twice\n[01:10.123]<01:10.123>Enhanced <01:11.00>words\n";

fn same(a: &CursorState, b: &CursorState) -> bool {
    a.line == b.line
        && a.active_lines == b.active_lines
        && a.line_ended == b.line_ended
        && a.syllable == b.syllable
        && a.line_progress.to_bits() == b.line_progress.to_bits()
        && a.syllable_progress.to_bits() == b.syllable_progress.to_bits()
        && a.effective_ms.to_bits() == b.effective_ms.to_bits()
}

/// In bounds, and the incremental cursor agrees with a fresh lookup.
fn check_cursor(l: &Lyrics, positions: &[f64]) {
    let mut c = LyricsCursor::new();
    for &p in positions {
        let fresh = locate(l, p);
        let inc = c.locate(l, p);
        assert!(same(&fresh, &inc), "at {p}: {fresh:?} vs {inc:?}");
        if let Some(i) = fresh.line {
            assert!(i < l.lines.len());
            if let Some(s) = fresh.syllable {
                assert!(s < l.lines[i].syllables.len());
            }
        }
        assert!(fresh.active_lines.iter().all(|&i| i < l.lines.len()));
        if !fresh.effective_ms.is_nan() {
            assert!((0.0..=1.0).contains(&fresh.line_progress));
            assert!((0.0..=1.0).contains(&fresh.syllable_progress));
        }
    }
}

fn boundaries(l: &Lyrics) -> Vec<f64> {
    let mut out = vec![f64::NEG_INFINITY, -1.0, 0.0, f64::INFINITY, f64::NAN];
    for line in &l.lines {
        for t in line.start_ms.into_iter().chain(line.end_ms) {
            out.extend([f64::from(t) - 1.0, f64::from(t), f64::from(t) + 1.0]);
        }
    }
    out
}

/// Both lyrics paths (direct `raw` parse, and the actor's subsonic types →
/// JSON → `raw`) must not panic and must agree.
fn check_structured(text: &str) {
    let parsed = LyricsListResponse::parse(text);
    if let Ok(r) = &parsed {
        if let Some(l) = adapt_response("t", r) {
            for s in l.lines.iter().flat_map(|x| &x.syllables) {
                assert!(s.end_ms >= s.start_ms);
            }
            let mut pos = boundaries(&l);
            check_cursor(&l, &pos);
            pos.reverse();
            check_cursor(&l, &pos);
            let _ = apply_offset(&l);
        }
    }
    if let Ok(env) = crate::subsonic::client::parse_envelope(text.as_bytes()) {
        let entries = env.lyrics_list.map(|l| l.structured_lyrics).unwrap_or_default();
        let raw: Vec<StructuredLyrics> =
            serde_json::from_value(serde_json::to_value(&entries).unwrap()).unwrap();
        if let Ok(r) = &parsed {
            assert_eq!(
                adapt_list("t", &raw, LyricsSource::Server),
                adapt_response("t", r)
            );
        }
    }
}

fn check_lrc(body: &str) {
    let doc = parse_lrc(body);
    assert!(doc.lines.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
    if let Some(l) = lrc_to_lyrics("t", &doc, LyricsSource::External) {
        check_cursor(&l, &boundaries(&l));
    }
    let _ = from_plain_text("t", body, LyricsSource::External);
}

fn arb_lyrics() -> impl Strategy<Value = Lyrics> {
    let line = (
        prop::option::of(0u32..20_000),
        prop::option::of(0u32..20_000),
        prop::collection::vec((0u32..20_000, 0u32..20_000), 0..4),
    )
        .prop_map(|(start_ms, end_ms, syl)| LyricLine {
            start_ms,
            end_ms,
            text: String::new(),
            syllables: syl
                .into_iter()
                .map(|(a, b)| LyricSyllable {
                    text: "x".into(),
                    start_ms: a.min(b),
                    end_ms: a.max(b),
                    joined: false,
                })
                .collect(),
            agent: None,
            background: false,
            translation: None,
        });
    (
        prop::collection::vec(line, 0..12),
        prop::sample::select(vec![LyricsTier::Unsynced, LyricsTier::Line, LyricsTier::Syllable]),
        any::<i32>(),
    )
        .prop_map(|(lines, tier, offset_ms)| Lyrics {
            track_id: "t".into(),
            tier,
            lang: None,
            display_artist: None,
            display_title: None,
            agents: vec![],
            lines,
            source: LyricsSource::Server,
            offset_ms,
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn lrc_arbitrary(s in "\\PC{0,200}") {
        check_lrc(&s);
    }

    #[test]
    fn lrc_shaped(lines in prop::collection::vec(
        "(\\[[0-9]{0,22}[:.][0-9]{0,4}([.:][0-9]{0,25})?\\]|\\[offset:[+-]?[0-9]{0,20}\\]|<[0-9:.]{0,12}>|[a-z ]{0,6}){0,5}",
        0..8,
    )) {
        check_lrc(&lines.join("\n"));
    }

    #[test]
    fn lrc_mutated(s in mutated(LRC)) {
        check_lrc(&s);
    }

    #[test]
    fn structured_arbitrary(v in json(WORDS)) {
        check_structured(&serde_json::json!({ "subsonic-response": { "status": "ok", "lyricsList": v } }).to_string());
        check_structured(&v.to_string());
    }

    #[test]
    fn structured_entry_arbitrary(v in json(WORDS)) {
        let doc = serde_json::json!({ "subsonic-response": { "status": "ok",
            "lyricsList": { "structuredLyrics": [v] } } });
        check_structured(&doc.to_string());
    }

    #[test]
    fn structured_mutated(s in mutated(ENHANCED)) {
        check_structured(&s);
    }

    #[test]
    fn cursor_arbitrary(l in arb_lyrics(), positions in prop::collection::vec(any::<f64>(), 0..16)) {
        check_cursor(&l, &positions);
        let mut all = positions.clone();
        all.extend(boundaries(&l));
        check_cursor(&l, &all);
        check_cursor(&apply_offset(&l), &all);
    }
}

// -- regressions found by the fuzz targets ------------------------------------

#[test]
fn fuzz_regression_lrc_huge_minutes_saturate() {
    // `lyrics_lrc`: `min * 60_000` overflowed (panic in debug builds).
    let doc = parse_lrc("[666666666666666:0]x");
    assert_eq!(doc.lines[0].start_ms, u32::MAX);
    let doc = parse_lrc("[18446744073709551615:59.999]x");
    assert_eq!(doc.lines[0].start_ms, u32::MAX);
}

#[test]
fn fuzz_regression_lrc_long_fraction() {
    // `10u64.pow(digits - 3)` overflowed for 23+ fraction digits.
    let doc = parse_lrc("[00:01.00000000000000000000001]x\n[00:02.5]y\n[00:03.123456]z");
    let starts: Vec<_> = doc.lines.iter().map(|l| l.start_ms).collect();
    assert_eq!(starts, vec![1000, 2500, 3123]);
}

#[test]
fn fuzz_regression_lrc_extreme_offset_tag() {
    // `-doc.offset_ms` overflowed for i64::MIN; `start + shift` for huge ones.
    for tag in ["[offset:-9223372036854775808]", "[offset:9223372036854775807]"] {
        let doc = parse_lrc(&format!("{tag}\n[00:01.00]a\n[00:02.00]b"));
        let l = lrc_to_lyrics("t", &doc, LyricsSource::External).unwrap();
        assert_eq!(l.lines.len(), 2);
    }
}

#[test]
fn fuzz_regression_cursor_unsorted_starts() {
    // `lyrics_structured`: lines starting at 8 then 0. The incremental
    // cursor stuck to the cached line 1 (start 0) at 8 while a fresh
    // lookup picked line 0 (the latest start ≤ position).
    let text = r#"{"subsonic-response":{"lyricsList":{"structuredLyrics":[{"synced":true,"line":[{"start":8},{"start":0,"":""}]}]}}}"#;
    let l = adapt_response("t", &LyricsListResponse::parse(text).unwrap()).unwrap();
    let mut c = LyricsCursor::new();
    assert_eq!(c.locate(&l, 0.0).line, Some(1));
    assert_eq!(c.locate(&l, 8.0).line, Some(0));
    assert_eq!(c.locate(&l, 8.0), locate(&l, 8.0));
    check_structured(text);
}

#[test]
fn fuzz_regression_agent_without_id() {
    // `lyrics_structured`: an agent with no `id` failed the direct parse
    // (so no lyrics) while the actor's path defaulted it and showed them.
    let text = r#"{"subsonic-response":{"status":"ok","lyricsList":{"structuredLyrics":[{"agents":[{}]},{"line":[{}]}]}}}"#;
    let direct = LyricsListResponse::parse(text).unwrap();
    assert_eq!(direct.entries().len(), 2);
    assert!(adapt_response("t", &direct).is_some());
    check_structured(text);
}

#[test]
fn fuzz_regression_extreme_server_offsets_and_times() {
    // `offset * -1`, `start + shift` and the translation alignment check
    // `(a - b).abs()` all overflowed on extreme server values.
    let text = r#"{"lyricsList":{"structuredLyrics":[
      {"kind":"main","synced":true,"offset":-9223372036854775808,
       "line":[{"start":9223372036854775807,"value":"a"},{"start":-9223372036854775808,"value":"b"}],
       "cueLine":[{"index":0,"start":9223372036854775807,"end":9223372036854775807,"value":"a","cue":[
          {"start":9223372036854775807,"end":-9223372036854775808,"value":"a","byteStart":9223372036854775807,"byteEnd":9223372036854775807},
          {"start":0,"value":"b","byteStart":9223372036854775807}]}]},
      {"kind":"translation","synced":true,"line":[{"start":-1,"value":"x"},{"start":9223372036854775807,"value":"y"}]}]}}"#;
    let l = adapt_response("t", &LyricsListResponse::parse(text).unwrap()).unwrap();
    assert_eq!(l.lines.len(), 2);
    for (off, _) in [(i64::MAX, 0), (i64::MIN, 0)] {
        let t = text.replace("-9223372036854775808,\n", &format!("{off},\n"));
        check_structured(&t);
    }
    check_structured(text);
}
