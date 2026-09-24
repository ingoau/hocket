//! Minimal LRC parsing, only for external sources (LRCLIB returns plain
//! LRC). Sidecars on the server are parsed by Navidrome, never here.
//!
//! Handles `[mm:ss.xx]` / `[mm:ss.xxx]` / `[mm:ss]` timestamps, several
//! timestamps per line, the `[offset:±ms]` tag, and ignores other id tags.
//! Inline word timestamps (`<mm:ss.xx>`, enhanced LRC) are stripped: we do
//! not build a syllable tier from an external plain-text source.

use crate::api::{LyricLine, Lyrics, LyricsSource, LyricsTier, Ms, TrackId};

/// One parsed LRC line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LrcLine {
    pub start_ms: Ms,
    pub text: String,
}

/// Parsed LRC body: timed lines sorted by start, plus the `[offset:]` tag.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LrcDocument {
    pub lines: Vec<LrcLine>,
    pub offset_ms: i64,
}

fn parse_timestamp(s: &str) -> Option<Ms> {
    // mm:ss(.xx|.xxx)? — minutes may exceed 59.
    let (min, rest) = s.split_once(':')?;
    let min: u64 = min.trim().parse().ok()?;
    let (sec, frac) = match rest.split_once(['.', ':']) {
        Some((sec, frac)) => (sec, Some(frac)),
        None => (rest, None),
    };
    let sec: u64 = sec.trim().parse().ok()?;
    if sec >= 60 {
        return None;
    }
    let frac_ms: u64 = match frac {
        None => 0,
        Some(f) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            // Milliseconds are the first three digits, right-padded: `.5` is
            // 500, `.05` is 50, `.123456` is 123. Never parse the whole run
            // of digits (it can be arbitrarily long).
            f.bytes()
                .chain(std::iter::repeat(b'0'))
                .take(3)
                .fold(0, |acc, b| acc * 10 + u64::from(b - b'0'))
        }
    };
    // Minutes are unbounded in the format; saturate rather than overflow.
    let total = min
        .saturating_mul(60_000)
        .saturating_add(sec * 1000)
        .saturating_add(frac_ms);
    Some(total.min(u64::from(u32::MAX)) as Ms)
}

/// Parses an LRC body. Lines without any timestamp are ignored; an LRC with
/// no timed lines at all yields an empty document.
pub fn parse_lrc(body: &str) -> LrcDocument {
    let mut doc = LrcDocument::default();
    for raw in body.lines() {
        let raw = raw.trim_end_matches('\r');
        let mut rest = raw.trim_start();
        let mut stamps: Vec<Ms> = Vec::new();
        while let Some(after) = rest.strip_prefix('[') {
            let Some(close) = after.find(']') else { break };
            let tag = &after[..close];
            rest = &after[close + 1..];
            if let Some(ts) = parse_timestamp(tag) {
                stamps.push(ts);
            } else if let Some((k, v)) = tag.split_once(':') {
                if k.trim().eq_ignore_ascii_case("offset") {
                    if let Ok(o) = v.trim().trim_start_matches('+').parse::<i64>() {
                        doc.offset_ms = o;
                    }
                }
            }
            rest = rest.trim_start();
        }
        if stamps.is_empty() {
            continue;
        }
        let text = strip_inline_timestamps(rest).trim().to_string();
        for s in stamps {
            doc.lines.push(LrcLine {
                start_ms: s,
                text: text.clone(),
            });
        }
    }
    doc.lines.sort_by_key(|l| l.start_ms);
    doc
}

fn strip_inline_timestamps(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        let (before, after) = rest.split_at(open);
        out.push_str(before);
        match after[1..].find('>') {
            Some(close) if parse_timestamp(&after[1..1 + close]).is_some() => {
                rest = &after[close + 2..];
            }
            _ => {
                out.push('<');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Converts a parsed LRC to line-tier lyrics. The `[offset:]` tag uses the
/// LRC convention (positive = lyrics early → shift timestamps earlier).
pub fn lrc_to_lyrics(track_id: &str, doc: &LrcDocument, source: LyricsSource) -> Option<Lyrics> {
    if doc.lines.is_empty() {
        return None;
    }
    // The tag is any i64 the file says; saturate so `[offset:-9223372036854775808]`
    // cannot overflow.
    let shift = doc.offset_ms.saturating_neg();
    let mut lines: Vec<LyricLine> = doc
        .lines
        .iter()
        .map(|l| LyricLine {
            start_ms: Some(
                i64::from(l.start_ms)
                    .saturating_add(shift)
                    .clamp(0, i64::from(u32::MAX)) as Ms,
            ),
            end_ms: None,
            text: l.text.clone(),
            syllables: vec![],
            agent: None,
            background: false,
            translation: None,
        })
        .collect();
    for i in 0..lines.len().saturating_sub(1) {
        lines[i].end_ms = lines[i + 1].start_ms;
    }
    Some(Lyrics {
        track_id: TrackId::from(track_id),
        tier: LyricsTier::Line,
        lang: None,
        display_artist: None,
        display_title: None,
        agents: vec![],
        lines,
        source,
        offset_ms: 0,
    })
}
