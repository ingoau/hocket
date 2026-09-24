//! Structured lyrics → [`api::Lyrics`].
//!
//! Tiers degrade honestly: a line gets syllables only when the server gave
//! every cue on that line a start time; otherwise it is a plain timed line.
//! Nothing is ever interpolated across a line's duration.

use crate::api::{
    LyricLine, LyricSyllable, Lyrics, LyricsAgent, LyricsSource, LyricsTier, Ms, TrackId,
};

use super::raw::{RawCue, RawCueLine, StructuredLyrics};

/// Server-supplied `offset` follows the LRC `[offset:]` convention that
/// Navidrome passes through from sidecars: a positive value means the lyrics
/// are *early* relative to the audio, so timestamps move earlier (t − offset).
/// If a server is ever found to use the opposite sign, flip this constant.
const SERVER_OFFSET_SIGN: i64 = -1;

/// Picks the entry to render: the first synced `main`-kind entry, then any
/// `main`, then the first entry at all.
pub fn pick_main(entries: &[StructuredLyrics]) -> Option<&StructuredLyrics> {
    entries
        .iter()
        .find(|e| e.effective_kind() == "main" && e.synced && !e.line.is_empty())
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.effective_kind() == "main" && !e.line.is_empty())
        })
        .or_else(|| entries.iter().find(|e| !e.line.is_empty()))
}

/// Adapts a whole `lyricsList` for a track. `None` when there is nothing to
/// show (no entries, or only empty ones).
pub fn adapt_list(
    track_id: &str,
    entries: &[StructuredLyrics],
    source: LyricsSource,
) -> Option<Lyrics> {
    let main = pick_main(entries)?;
    let (mut lyrics, origin) = adapt_entry_mapped(track_id, main, source);
    // A translation layer is attached line-by-line only when it clearly lines
    // up with the main layer: same line count and (for synced) same starts.
    if let Some(tr) = entries
        .iter()
        .find(|e| e.effective_kind() == "translation" && !std::ptr::eq(*e, main))
    {
        attach_translation(&mut lyrics, &origin, main, tr);
    }
    Some(lyrics)
}

/// Adapts one entry.
pub fn adapt_entry(track_id: &str, entry: &StructuredLyrics, source: LyricsSource) -> Lyrics {
    adapt_entry_mapped(track_id, entry, source).0
}

/// [`adapt_entry`] plus, for every output line, the index of the `line[]`
/// entry it came from (`None` for a sub-voice line that only exists as an
/// extra cue line, e.g. Navidrome's `__nd_bg__|<agent>` background vocals).
fn adapt_entry_mapped(
    track_id: &str,
    entry: &StructuredLyrics,
    source: LyricsSource,
) -> (Lyrics, Vec<Option<usize>>) {
    let shift = entry.offset.unwrap_or(0) * SERVER_OFFSET_SIGN;
    let agents = build_agents(entry);
    let bg_ids: Vec<&str> = entry
        .agents
        .iter()
        .filter(|a| a.role.eq_ignore_ascii_case("bg"))
        .map(|a| a.id.as_str())
        .collect();
    let is_bg = |cl: &RawCueLine| cue_line_agent(cl).is_some_and(|a| bg_ids.contains(&a));

    let synced = entry.synced && entry.line.iter().any(|l| l.start.is_some());
    let mut lines: Vec<LyricLine> = Vec::with_capacity(entry.line.len());
    let mut origin: Vec<Option<usize>> = Vec::with_capacity(entry.line.len());
    for (i, raw) in entry.line.iter().enumerate() {
        // Several cue lines can share one `line[]` index: the main voice's
        // words plus one per extra voice singing over it. The first non-
        // background one is the line itself; the rest are sub-voice lines.
        let mut cue_lines = if synced {
            cue_lines_for(entry, i)
        } else {
            Vec::new()
        };
        let main_pos = cue_lines.iter().position(|c| !is_bg(c)).unwrap_or(0);
        let cue_line = (!cue_lines.is_empty()).then(|| cue_lines.remove(main_pos));
        let start_ms = if synced {
            raw.start
                .or(cue_line.and_then(|c| c.start))
                .map(|s| to_ms(s + shift))
        } else {
            None
        };
        let mut end_ms = cue_line.and_then(|c| c.end).map(|e| to_ms(e + shift));
        let (text, syllables) = match cue_line {
            Some(cl) => {
                let text = if cl.value.is_empty() {
                    raw.value.clone()
                } else {
                    cl.value.clone()
                };
                (text, syllables_from(cl, shift))
            }
            None => (raw.value.clone(), Vec::new()),
        };
        if end_ms.is_none() {
            if let Some(last) = syllables.last() {
                end_ms = Some(last.end_ms);
            }
        }
        let agent = cue_line.and_then(|c| cue_line_agent(c).map(str::to_string));
        let background = agent.as_deref().is_some_and(|a| bg_ids.contains(&a));
        lines.push(LyricLine {
            start_ms,
            end_ms,
            text,
            syllables,
            agent,
            background,
            translation: None,
        });
        origin.push(Some(i));
        for cl in cue_lines {
            if let Some(line) = sub_voice_line(cl, shift, &bg_ids) {
                lines.push(line);
                origin.push(None);
            }
        }
    }

    // A line's end defaults to the next line's start when the server gave
    // none (sub-voice lines are not "next": they overlap their line).
    if synced {
        for i in 0..lines.len() {
            if lines[i].end_ms.is_none() && origin[i].is_some() {
                let next_start = (i + 1..lines.len())
                    .filter(|&j| origin[j].is_some())
                    .find_map(|j| lines[j].start_ms);
                if let (Some(s), Some(n)) = (lines[i].start_ms, next_start) {
                    if n >= s {
                        lines[i].end_ms = Some(n);
                    }
                }
            }
        }
    }

    let tier = if !synced {
        LyricsTier::Unsynced
    } else if lines.iter().any(|l| l.syllables.len() >= 2) {
        LyricsTier::Syllable
    } else {
        LyricsTier::Line
    };

    let lyrics = Lyrics {
        track_id: TrackId::from(track_id),
        tier,
        lang: Some(entry.lang.clone()).filter(|l| !l.is_empty() && l != "xxx" && l != "und"),
        display_artist: entry.display_artist.clone(),
        display_title: entry.display_title.clone(),
        agents,
        lines,
        source,
        offset_ms: 0,
    };
    (lyrics, origin)
}

/// An extra cue line at an already-taken index: a voice singing over the
/// line (background vocals, a second voice in a duet). It starts when its
/// first timed word does, since its own `start` usually just mirrors the
/// line it belongs to. `None` when there is nothing to show.
fn sub_voice_line(cl: &RawCueLine, shift: i64, bg_ids: &[&str]) -> Option<LyricLine> {
    if cl.value.trim().is_empty() {
        return None;
    }
    let syllables = syllables_from(cl, shift);
    let start = cl
        .cue
        .iter()
        .find_map(|c| c.start)
        .or(cl.start)
        .map(|s| to_ms(s + shift));
    let end = cl
        .end
        .map(|e| to_ms(e + shift))
        .or_else(|| syllables.last().map(|s| s.end_ms));
    let agent = cue_line_agent(cl).map(str::to_string);
    let background = agent.as_deref().is_some_and(|a| bg_ids.contains(&a));
    Some(LyricLine {
        start_ms: start,
        end_ms: end,
        text: cl.value.clone(),
        syllables,
        agent,
        background,
        translation: None,
    })
}

fn cue_line_agent(cl: &RawCueLine) -> Option<&str> {
    cl.agent_id
        .as_deref()
        .or_else(|| cl.cue.iter().find_map(|q| q.agent_id.as_deref()))
}

/// Every cue line for `line[line_index]`, in document order. Cue lines
/// without an index (older Navidrome builds) pair up positionally.
fn cue_lines_for(entry: &StructuredLyrics, line_index: usize) -> Vec<&RawCueLine> {
    if entry.cue_line.iter().all(|c| c.index.is_none()) {
        return entry.cue_line.get(line_index).into_iter().collect();
    }
    entry
        .cue_line
        .iter()
        .filter(|c| c.index == Some(line_index as i64))
        .collect()
}

fn to_ms(v: i64) -> Ms {
    v.clamp(0, i64::from(u32::MAX)) as Ms
}

/// Syllables for a cue line. Whitespace-only cues are not syllables; they
/// (and trailing whitespace inside a cue) mark word boundaries. Returns
/// empty when any cue lacks a start, so the line renders at line tier.
///
/// Navidrome trims each cue's `value` and encodes the word boundaries in
/// `byteStart`/`byteEnd` (0-based, inclusive offsets into the line's
/// `value`): `"I"` is `0..=0`, the following `"lost"` starts at 2 because
/// byte 1 is the space, while a syllable split like `"ti"` `0..=1` +
/// `"tle"` `2..=4` is contiguous. So two cues are joined when nothing but
/// non-space bytes sits between them in the line; cues without offsets
/// fall back to their own whitespace.
fn syllables_from(cl: &RawCueLine, shift: i64) -> Vec<LyricSyllable> {
    if cl.cue.is_empty() || cl.cue.iter().any(|c| c.start.is_none()) {
        return Vec::new();
    }
    let mut out: Vec<LyricSyllable> = Vec::with_capacity(cl.cue.len());
    let cues = &cl.cue;
    for (i, cue) in cues.iter().enumerate() {
        if cue.value.trim().is_empty() {
            // Word boundary: the previous syllable is not joined.
            if let Some(prev) = out.last_mut() {
                prev.joined = false;
            }
            continue;
        }
        let start = cue.start.unwrap_or(0) + shift;
        let end = cue
            .end
            .map(|e| e + shift)
            .or_else(|| {
                cues[i + 1..]
                    .iter()
                    .find_map(|n| n.start)
                    .map(|s| s + shift)
            })
            .or_else(|| cl.end.map(|e| e + shift))
            .unwrap_or(start);
        let mut text = cue.value.trim_end().to_string();
        let trailing_space = cue.value.ends_with(char::is_whitespace);
        let next = cues[i + 1..].iter().find(|n| !n.value.is_empty());
        let next_is_text = next.is_some_and(|n| {
            !n.value.trim().is_empty() && !n.value.starts_with(char::is_whitespace)
        });
        let mut joined = !trailing_space && next_is_text;
        if let (true, Some(gap)) = (joined, next.and_then(|n| byte_gap(&cl.value, cue, n))) {
            if gap.chars().any(char::is_whitespace) {
                joined = false;
            } else {
                // Punctuation between two joined cues ("well-known")
                // belongs to the word; keep it visible.
                text.push_str(gap);
            }
        }
        out.push(LyricSyllable {
            text,
            start_ms: to_ms(start),
            end_ms: to_ms(end.max(start)),
            joined,
        });
    }
    if let Some(last) = out.last_mut() {
        last.joined = false;
    }
    out
}

/// The bytes of `line` between the end of `cue` and the start of `next`,
/// from their byte offsets. `None` when either lacks offsets or they do not
/// describe `line` (not on a char boundary, out of range, or the cue text is
/// not where the offset says), in which case the caller falls back to the
/// cues' own whitespace.
fn byte_gap<'a>(line: &'a str, cue: &RawCue, next: &RawCue) -> Option<&'a str> {
    let bs = usize::try_from(cue.byte_start?).ok()?;
    let be = usize::try_from(cue.byte_end?).ok()?;
    let nbs = usize::try_from(next.byte_start?).ok()?;
    let value = cue.value.trim_end();
    // The cue's own text pins its end; if the offset points elsewhere, the
    // inclusive `byteEnd` does.
    let end_excl = match line.get(bs..bs + value.len()) {
        Some(at) if at == value => bs + value.len(),
        _ => be.checked_add(1)?,
    };
    if nbs < end_excl {
        return None;
    }
    line.get(end_excl..nbs)
}

/// Side assignment: agents in order of appearance with `main` first; the
/// first gets the left side, the second the right, further ones alternate.
/// Background (`bg`) agents share the side of the agent before them.
fn build_agents(entry: &StructuredLyrics) -> Vec<LyricsAgent> {
    let mut ordered: Vec<&super::raw::RawAgent> = entry.agents.iter().collect();
    ordered.sort_by_key(|a| {
        if a.role.eq_ignore_ascii_case("main") {
            0
        } else {
            1
        }
    });
    let mut out = Vec::with_capacity(ordered.len());
    let mut voice_index = 0u32;
    let mut last_side = 0u32;
    for a in ordered {
        let side = if a.role.eq_ignore_ascii_case("bg") {
            last_side
        } else {
            let s = voice_index % 2;
            voice_index += 1;
            last_side = s;
            s
        };
        out.push(LyricsAgent {
            id: a.id.clone(),
            name: a.name.clone(),
            side,
        });
    }
    out
}

/// Attaches `translation.line[k]` to the output line that came from
/// `main.line[k]` (sub-voice lines get none).
fn attach_translation(
    lyrics: &mut Lyrics,
    origin: &[Option<usize>],
    main: &StructuredLyrics,
    translation: &StructuredLyrics,
) {
    if translation.line.len() != main.line.len() {
        return;
    }
    if main.synced && translation.synced {
        let aligned =
            main.line
                .iter()
                .zip(&translation.line)
                .all(|(m, t)| match (m.start, t.start) {
                    (Some(a), Some(b)) => (a - b).abs() <= 500,
                    (None, None) => true,
                    _ => false,
                });
        if !aligned {
            return;
        }
    }
    for (line, from) in lyrics.lines.iter_mut().zip(origin) {
        let Some(k) = *from else { continue };
        if let Some(t) = translation.line.get(k) {
            if !t.value.trim().is_empty() {
                line.translation = Some(t.value.clone());
            }
        }
    }
}

/// Plain text lines (unsynced) → [`Lyrics`], for external providers that
/// only have a plain body.
pub fn from_plain_text(track_id: &str, text: &str, source: LyricsSource) -> Option<Lyrics> {
    let lines: Vec<LyricLine> = text
        .lines()
        .map(|l| LyricLine {
            start_ms: None,
            end_ms: None,
            text: l.trim_end().to_string(),
            syllables: vec![],
            agent: None,
            background: false,
            translation: None,
        })
        .collect();
    if lines.iter().all(|l| l.text.is_empty()) {
        return None;
    }
    Some(Lyrics {
        track_id: track_id.into(),
        tier: LyricsTier::Unsynced,
        lang: None,
        display_artist: None,
        display_title: None,
        agents: vec![],
        lines,
        source,
        offset_ms: 0,
    })
}
