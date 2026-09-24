//! Position → active line / syllable, for renderers.
//!
//! Both platforms either query the core for this each frame or reimplement
//! it; the rules live here so they agree. Position is the *extrapolated*
//! session position (see the Connect design), and the per-track user offset
//! stored in [`Lyrics::offset_ms`] is applied here.

use crate::api::{Lyrics, LyricsTier};

/// What is active at a moment.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CursorState {
    /// Primary active line: the latest-starting line whose start ≤ position.
    /// `None` before the first line, or for unsynced lyrics.
    pub line: Option<usize>,
    /// Every line whose `[start, end)` contains the position — several for
    /// overlapping duet/background lines. Always contains `line` when that
    /// line has not ended.
    pub active_lines: Vec<usize>,
    /// True when the primary line has a known end and the position is past it
    /// (the gap before the next line).
    pub line_ended: bool,
    /// 0..1 progress through the primary line, when its end is known.
    pub line_progress: f32,
    /// Active syllable within the primary line (syllable tier only).
    pub syllable: Option<usize>,
    /// 0..1 progress through the active syllable; 1.0 once it has ended.
    pub syllable_progress: f32,
    /// Position after the user offset, ms, as used for all of the above.
    pub effective_ms: f64,
}

/// Locates the cursor for a position. Pure; see [`LyricsCursor`] for the
/// incremental version.
pub fn locate(lyrics: &Lyrics, position_ms: f64) -> CursorState {
    let mut c = LyricsCursor::default();
    c.locate(lyrics, position_ms)
}

/// Incremental cursor: remembers the last line so per-frame lookups on a
/// long track stay O(1) while playing forward, with a binary-search fallback
/// after seeks.
#[derive(Debug, Clone, Default)]
pub struct LyricsCursor {
    last_line: Option<usize>,
}

impl LyricsCursor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget the cached line (call on track change).
    pub fn reset(&mut self) {
        self.last_line = None;
    }

    pub fn locate(&mut self, lyrics: &Lyrics, position_ms: f64) -> CursorState {
        // Positive user offset = lyrics should appear later, so the audio
        // position is compared as if it were earlier.
        let effective = position_ms - f64::from(lyrics.offset_ms);
        let mut state = CursorState {
            effective_ms: effective,
            ..Default::default()
        };
        if lyrics.tier == LyricsTier::Unsynced || lyrics.lines.is_empty() {
            self.last_line = None;
            return state;
        }
        let starts: Vec<Option<f64>> = lyrics
            .lines
            .iter()
            .map(|l| l.start_ms.map(f64::from))
            .collect();

        // Primary line: latest start ≤ effective. Try the cached line and its
        // successor first, then fall back to a scan. The shortcut only holds
        // when the timed starts are in order; servers do not promise that
        // (overlapping duet and background lines), and a cursor that answers
        // differently depending on where it was last frame would flicker.
        let sorted = starts
            .iter()
            .flatten()
            .zip(starts.iter().flatten().skip(1))
            .all(|(a, b)| a <= b);
        let mut line = None;
        if let (Some(last), true) = (self.last_line, sorted) {
            let ok_here = starts
                .get(last)
                .copied()
                .flatten()
                .is_some_and(|s| s <= effective);
            let next = (last + 1..starts.len()).find(|&i| starts[i].is_some());
            let next_started = next.is_some_and(|n| starts[n].is_some_and(|s| s <= effective));
            if ok_here && !next_started {
                line = Some(last);
            }
        }
        if line.is_none() {
            let mut best: Option<(usize, f64)> = None;
            for (i, s) in starts.iter().enumerate() {
                if let Some(s) = *s {
                    if s <= effective && best.is_none_or(|(_, bs)| s >= bs) {
                        best = Some((i, s));
                    }
                }
            }
            line = best.map(|(i, _)| i);
        }
        self.last_line = line;
        state.line = line;

        // All lines containing the position (duets, background).
        for (i, l) in lyrics.lines.iter().enumerate() {
            let Some(s) = l.start_ms.map(f64::from) else {
                continue;
            };
            if s > effective {
                continue;
            }
            let end = l
                .end_ms
                .map(f64::from)
                .or_else(|| starts[i + 1..].iter().find_map(|x| *x));
            let inside = match end {
                Some(e) => effective < e || (Some(i) == line && e <= s),
                None => Some(i) == line,
            };
            if inside {
                state.active_lines.push(i);
            }
        }

        let Some(li) = line else { return state };
        let l = &lyrics.lines[li];
        if let (Some(s), Some(e)) = (l.start_ms, l.end_ms) {
            let (s, e) = (f64::from(s), f64::from(e));
            if e > s {
                state.line_progress = ((effective - s) / (e - s)).clamp(0.0, 1.0) as f32;
                state.line_ended = effective >= e;
            } else {
                state.line_progress = 1.0;
                state.line_ended = effective > e;
            }
        }
        if state.line_ended && !state.active_lines.contains(&li) {
            // Keep the primary line visible-but-ended; renderers use `line_ended`.
        }

        if lyrics.tier == LyricsTier::Syllable && !l.syllables.is_empty() {
            let mut idx = None;
            for (i, syl) in l.syllables.iter().enumerate() {
                if f64::from(syl.start_ms) <= effective {
                    idx = Some(i);
                }
            }
            state.syllable = idx;
            if let Some(i) = idx {
                let syl = &l.syllables[i];
                let (s, e) = (f64::from(syl.start_ms), f64::from(syl.end_ms));
                state.syllable_progress = if e > s {
                    ((effective - s) / (e - s)).clamp(0.0, 1.0) as f32
                } else {
                    1.0
                };
            }
        }
        state
    }
}
