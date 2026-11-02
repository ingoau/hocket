//! Match a remembered phrase against lyrics already cached on the device.
//!
//! Folding is the same for the index and the query: case is folded, diacritics
//! are stripped, and punctuation is not part of a word (an apostrophe is
//! dropped, so "don't" and "dont" meet; other punctuation is a word break, so
//! "well-known" and "well known" meet). Lines are one word stream, so a phrase
//! can start on one line and finish on the next. The last word may be a prefix,
//! the same way library search works as you type.

use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::api::{Lyrics, Ms};

/// What a phrase hit looks like in the original lyric text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LyricSpan {
    pub line: String,
    pub match_start: u32,
    pub match_end: u32,
    /// Timestamp of the line the phrase starts on, when that line is synced.
    pub start_ms: Option<Ms>,
}

/// Folded words plus the original lines, ready to store in `lyrics_search`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedLyrics {
    pub body: String,
    pub lines_json: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct StoredLine {
    text: String,
    start_ms: Option<Ms>,
}

struct Word {
    line: usize,
    /// Inclusive char index in that line.
    start: usize,
    /// Exclusive char index in that line.
    end: usize,
    folded: String,
}

/// Audio position for a synced line. Positive `offset_ms` shows lyrics later,
/// so playback has to be that much further in for the line to be current.
pub fn seek_position(start_ms: Ms, offset_ms: i32) -> Ms {
    (i64::from(start_ms) + i64::from(offset_ms)).clamp(0, i64::from(u32::MAX)) as Ms
}

/// `None` only when the line list cannot be stored. A lyric with no words
/// still gets a row (`body` empty) so opening the database does not parse it again.
pub fn index_document(lyrics: &Lyrics) -> Option<IndexedLyrics> {
    let lines = stored_lines(lyrics);
    let words = words_of(&lines);
    let body = words
        .iter()
        .map(|w| w.folded.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let lines_json = serde_json::to_string(&lines).ok()?;
    Some(IndexedLyrics { body, lines_json })
}

/// FTS5 phrase, prefix on the last word. `None` when the query has no words.
pub fn lyrics_match_expr(query: &str) -> Option<String> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return None;
    }
    let inner = terms
        .iter()
        .map(|t| t.replace('"', "\"\""))
        .collect::<Vec<_>>()
        .join(" ");
    Some(format!("\"{inner}\"*"))
}

/// Find `query` in the stored line JSON written by [`index_document`].
pub fn find_in_stored(lines_json: &str, query: &str) -> Option<LyricSpan> {
    let lines: Vec<StoredLine> = serde_json::from_str(lines_json).ok()?;
    find_in_lines(&lines, query)
}

fn stored_lines(lyrics: &Lyrics) -> Vec<StoredLine> {
    lyrics
        .lines
        .iter()
        .map(|l| StoredLine {
            text: l.text.clone(),
            start_ms: l.start_ms,
        })
        .collect()
}

fn find_in_lines(lines: &[StoredLine], query: &str) -> Option<LyricSpan> {
    let terms = query_terms(query);
    if terms.is_empty() {
        return None;
    }
    let words = words_of(lines);
    let (from, to) = find_words(&words, &terms)?;
    let first = &words[from];
    let last = &words[to];
    let mut display = String::new();
    let mut match_start = 0u32;
    let mut match_end = 0u32;
    for (li, line) in lines
        .iter()
        .enumerate()
        .take(last.line + 1)
        .skip(first.line)
    {
        if li > first.line {
            display.push('\n');
        }
        let base = display.chars().count();
        display.push_str(&line.text);
        if li == first.line {
            match_start = (base + first.start) as u32;
        }
        if li == last.line {
            match_end = (base + last.end) as u32;
        }
    }
    Some(LyricSpan {
        line: display,
        match_start,
        match_end,
        start_ms: lines.get(first.line).and_then(|l| l.start_ms),
    })
}

fn find_words(words: &[Word], terms: &[String]) -> Option<(usize, usize)> {
    if terms.is_empty() || words.len() < terms.len() {
        return None;
    }
    let last = terms.len() - 1;
    for i in 0..=words.len() - terms.len() {
        let matched = terms.iter().enumerate().all(|(k, term)| {
            let word = &words[i + k].folded;
            if k == last {
                word.starts_with(term.as_str())
            } else {
                word == term
            }
        });
        if matched {
            return Some((i, i + last));
        }
    }
    None
}

fn query_terms(query: &str) -> Vec<String> {
    words_of(&[StoredLine {
        text: query.to_string(),
        start_ms: None,
    }])
    .into_iter()
    .map(|w| w.folded)
    .collect()
}

fn words_of(lines: &[StoredLine]) -> Vec<Word> {
    let mut words = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.text.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            // A combining mark is part of the letter it follows, including when
            // the lyric was stored decomposed (NFD), as macOS tags often are.
            if is_apostrophe(chars[i]) || is_combining_mark(chars[i]) || !chars[i].is_alphanumeric()
            {
                i += 1;
                continue;
            }
            let start = i;
            let mut raw = String::new();
            while i < chars.len()
                && (chars[i].is_alphanumeric()
                    || is_apostrophe(chars[i])
                    || is_combining_mark(chars[i]))
            {
                if !is_apostrophe(chars[i]) {
                    raw.push(chars[i]);
                }
                i += 1;
            }
            let folded = fold(&raw);
            if !folded.is_empty() {
                words.push(Word {
                    line: li,
                    start,
                    end: i,
                    folded,
                });
            }
        }
    }
    words
}

fn fold(s: &str) -> String {
    // Lowercase first: İ lowercases to i plus a combining dot, which the
    // following strip then removes. Stripping first would leave that dot.
    let lower: String = s.chars().flat_map(|c| c.to_lowercase()).collect();
    lower.nfd().filter(|c| !is_combining_mark(*c)).collect()
}

fn is_apostrophe(c: char) -> bool {
    matches!(
        c,
        '\'' | '\u{2019}' | '\u{2018}' | '\u{02BC}' | '\u{FF07}' | '\u{00B4}' | '\u{2032}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{LyricLine, LyricsSource, LyricsTier};

    fn line(text: &str, start_ms: Option<u32>) -> LyricLine {
        LyricLine {
            start_ms,
            end_ms: None,
            text: text.to_string(),
            syllables: vec![],
            agent: None,
            background: false,
            translation: None,
        }
    }

    fn lyrics(lines: Vec<LyricLine>) -> Lyrics {
        Lyrics {
            track_id: "t".into(),
            tier: LyricsTier::Line,
            lang: None,
            display_artist: None,
            display_title: None,
            agents: vec![],
            lines,
            source: LyricsSource::Server,
            offset_ms: 0,
        }
    }

    fn hit(lyrics: &Lyrics, query: &str) -> LyricSpan {
        let doc = index_document(lyrics).expect("indexed");
        find_in_stored(&doc.lines_json, query).expect("hit")
    }

    #[test]
    fn phrase_ignores_case_punctuation_and_diacritics() {
        let lyrics = lyrics(vec![line("Don't you café, tonight?", Some(4_000))]);
        let span = hit(&lyrics, "dont you cafe");
        assert_eq!(span.line, "Don't you café, tonight?");
        assert_eq!(
            &span
                .line
                .chars()
                .skip(span.match_start as usize)
                .take((span.match_end - span.match_start) as usize)
                .collect::<String>(),
            "Don't you café"
        );
        assert_eq!(span.start_ms, Some(4_000));
        assert!(lyrics_match_expr("don't you café")
            .unwrap()
            .contains("dont you cafe"));
    }

    #[test]
    fn phrase_can_cross_a_line_break() {
        let lyrics = lyrics(vec![
            line("And the night,", Some(12_000)),
            line("was young.", Some(15_000)),
        ]);
        let span = hit(&lyrics, "NIGHT WAS");
        assert_eq!(span.line, "And the night,\nwas young.");
        let marked: String = span
            .line
            .chars()
            .skip(span.match_start as usize)
            .take((span.match_end - span.match_start) as usize)
            .collect();
        assert_eq!(marked, "night,\nwas");
        assert_eq!(span.start_ms, Some(12_000));
    }

    #[test]
    fn words_must_be_adjacent_and_only_the_last_may_be_a_prefix() {
        let lyrics = lyrics(vec![line("the night was young", Some(1))]);
        assert!(
            find_in_stored(&index_document(&lyrics).unwrap().lines_json, "was night").is_none()
        );
        assert!(find_in_stored(&index_document(&lyrics).unwrap().lines_json, "nig was").is_none());
        let span = hit(&lyrics, "night wa");
        let marked: String = span
            .line
            .chars()
            .skip(span.match_start as usize)
            .take((span.match_end - span.match_start) as usize)
            .collect();
        assert_eq!(marked, "night was");
    }

    #[test]
    fn unsynced_lines_have_no_seek_point_and_blank_lyrics_are_not_indexed() {
        let synced = lyrics(vec![line("hello there", None)]);
        assert_eq!(hit(&synced, "hello").start_ms, None);
        let empty = lyrics(vec![line("...", None), line("   ", None)]);
        assert!(index_document(&empty).unwrap().body.is_empty());
        assert!(lyrics_match_expr("...").is_none());
    }

    #[test]
    fn decomposed_lyrics_match_a_composed_query() {
        let stored = lyrics(vec![line(
            "the re\u{0301}sume\u{0301} is here",
            Some(2_000),
        )]);
        let span = hit(&stored, "résumé");
        let marked: String = span
            .line
            .chars()
            .skip(span.match_start as usize)
            .take((span.match_end - span.match_start) as usize)
            .collect();
        assert_eq!(marked, "re\u{0301}sume\u{0301}");
        assert_eq!(span.start_ms, Some(2_000));
        let city = lyrics(vec![line("from İstanbul tonight", None)]);
        assert!(find_in_stored(&index_document(&city).unwrap().lines_json, "istanbul").is_some());
    }

    #[test]
    fn hyphen_is_a_word_break() {
        let lyrics = lyrics(vec![line("a well-known tune", Some(8))]);
        let span = hit(&lyrics, "well known");
        let marked: String = span
            .line
            .chars()
            .skip(span.match_start as usize)
            .take((span.match_end - span.match_start) as usize)
            .collect();
        assert_eq!(marked, "well-known");
    }

    #[test]
    fn seek_position_applies_the_user_offset() {
        assert_eq!(seek_position(12_000, 250), 12_250);
        assert_eq!(seek_position(100, -250), 0);
    }
}
