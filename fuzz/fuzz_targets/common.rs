//! Invariants shared by the lyrics targets.
#![allow(dead_code)]

use hocket_core::api::{Lyrics, LyricsTier};
use hocket_core::lyrics::{locate, CursorState, LyricsCursor};

/// Structural invariants every adapted `Lyrics` keeps.
pub fn check_lyrics(l: &Lyrics) {
    for line in &l.lines {
        for s in &line.syllables {
            assert!(
                s.end_ms >= s.start_ms,
                "syllable ends before it starts: {s:?}"
            );
        }
        if let Some(last) = line.syllables.last() {
            assert!(!last.joined, "a line's last syllable is never joined");
        }
    }
    if l.tier == LyricsTier::Syllable {
        assert!(l.lines.iter().any(|x| x.syllables.len() >= 2));
    }
    if l.tier == LyricsTier::Unsynced {
        assert!(l.lines.iter().all(|x| x.start_ms.is_none()));
    }
}

/// Interesting positions for a document: every boundary ±1 plus extremes.
pub fn positions(l: &Lyrics) -> Vec<f64> {
    let mut out = vec![f64::NEG_INFINITY, -1.0, 0.0, 4.3e9, f64::INFINITY, f64::NAN];
    let mut edge = |t: u32| {
        let t = f64::from(t);
        out.extend([t - 1.0, t, t + 0.5, t + 1.0]);
    };
    for line in l.lines.iter().take(64) {
        line.start_ms.map(&mut edge);
        line.end_ms.map(&mut edge);
        for s in line.syllables.iter().take(16) {
            edge(s.start_ms);
            edge(s.end_ms);
        }
    }
    out
}

fn same(a: &CursorState, b: &CursorState) -> bool {
    a.line == b.line
        && a.active_lines == b.active_lines
        && a.line_ended == b.line_ended
        && a.syllable == b.syllable
        && a.line_progress.to_bits() == b.line_progress.to_bits()
        && a.syllable_progress.to_bits() == b.syllable_progress.to_bits()
        && a.effective_ms.to_bits() == b.effective_ms.to_bits()
}

/// The cursor stays in bounds, and the incremental cursor always agrees with
/// a fresh lookup whatever order positions arrive in (playback, seeks).
pub fn check_cursor(l: &Lyrics, positions: &[f64]) {
    let mut cursor = LyricsCursor::new();
    for &p in positions {
        let fresh = locate(l, p);
        let inc = cursor.locate(l, p);
        assert!(
            same(&fresh, &inc),
            "incremental cursor disagrees at {p}: fresh {fresh:?} vs cached {inc:?}"
        );
        match fresh.line {
            Some(i) => {
                assert!(i < l.lines.len());
                if let Some(s) = fresh.syllable {
                    assert!(s < l.lines[i].syllables.len());
                }
            }
            None => assert!(fresh.syllable.is_none()),
        }
        assert!(fresh.active_lines.iter().all(|&i| i < l.lines.len()));
        assert!(fresh.active_lines.windows(2).all(|w| w[0] < w[1]));
        if !fresh.effective_ms.is_nan() {
            assert!((0.0..=1.0).contains(&fresh.line_progress), "{fresh:?}");
            assert!((0.0..=1.0).contains(&fresh.syllable_progress), "{fresh:?}");
        }
    }
}

/// JSON equality that lets floats differ in the last couple of bits.
///
/// serde_json without its `float_roundtrip` feature (the workspace does not
/// enable it) parses some floats one ulp off, e.g. `1e42` or
/// `215492859907334.66`, so an exact decode → encode → decode comparison
/// would only rediscover that. Everything else must match exactly.
pub fn json_close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value;
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => match (x.as_f64(), y.as_f64()) {
            _ if x == y => true,
            (Some(p), Some(q)) if x.is_f64() || y.is_f64() => {
                p == q || (p - q).abs() <= p.abs().max(q.abs()) * 4.0 * f64::EPSILON
            }
            _ => false,
        },
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_close(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_close(v, w)))
        }
        _ => a == b,
    }
}

/// `a` and `b` serialise to [`json_close`] JSON.
pub fn same_modulo_float_parsing<T: serde::Serialize>(a: &T, b: &T) -> bool {
    json_close(
        &serde_json::to_value(a).expect("serialises"),
        &serde_json::to_value(b).expect("serialises"),
    )
}
