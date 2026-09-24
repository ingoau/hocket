//! The lyrics cursor over arbitrary (unsorted, overlapping, zero-length)
//! documents and arbitrary positions, incremental vs fresh.
#![no_main]

mod common;

use arbitrary::Arbitrary;
use hocket_core::api::{LyricLine, LyricSyllable, Lyrics, LyricsSource, LyricsTier};
use hocket_core::lyrics::apply_offset;
use libfuzzer_sys::fuzz_target;

#[derive(Arbitrary, Debug)]
struct Line {
    start: Option<u32>,
    end: Option<u32>,
    syllables: Vec<(u32, u32)>,
}

#[derive(Arbitrary, Debug)]
struct Input {
    tier: u8,
    offset_ms: i32,
    lines: Vec<Line>,
    positions: Vec<f64>,
    add_boundaries: bool,
}

fuzz_target!(|input: Input| {
    let tier = match input.tier % 3 {
        0 => LyricsTier::Unsynced,
        1 => LyricsTier::Line,
        _ => LyricsTier::Syllable,
    };
    let lines: Vec<LyricLine> = input
        .lines
        .iter()
        .take(256)
        .map(|l| LyricLine {
            start_ms: l.start,
            end_ms: l.end,
            text: String::new(),
            syllables: l
                .syllables
                .iter()
                .take(64)
                .map(|&(a, b)| LyricSyllable {
                    text: "x".into(),
                    start_ms: a.min(b),
                    end_ms: a.max(b),
                    joined: false,
                })
                .collect(),
            agent: None,
            background: false,
            translation: None,
        })
        .collect();
    let lyrics = Lyrics {
        track_id: "t".into(),
        tier,
        lang: None,
        display_artist: None,
        display_title: None,
        agents: vec![],
        lines,
        source: LyricsSource::Server,
        offset_ms: input.offset_ms,
    };
    let mut positions: Vec<f64> = input.positions.iter().take(512).copied().collect();
    if input.add_boundaries {
        positions.extend(common::positions(&lyrics));
    }
    common::check_cursor(&lyrics, &positions);
    let baked = apply_offset(&lyrics);
    assert_eq!(baked.offset_ms, 0);
    common::check_cursor(&baked, &positions);
});
