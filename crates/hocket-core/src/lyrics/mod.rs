//! Lyrics: OpenSubsonic v2 structured lyrics → the renderable
//! [`api::Lyrics`] model, cursor maths for renderers, opt-in external sources.
//!
//! Entry points for the actor:
//!
//! | Need | Call |
//! |---|---|
//! | Parse `getLyricsBySongId` (ask for `enhanced=true`) | [`raw::LyricsListResponse::parse`] or deserialise into it directly |
//! | Turn it into the model | [`adapt_response`] / [`adapt_list`] `→ Option<api::Lyrics>` |
//! | Apply the per-track user offset | [`set_user_offset`] (keeps timing, sets `offset_ms`) or [`apply_offset`] (bakes it in) |
//! | Where is the cursor at position *p*? | [`locate`] / [`LyricsCursor::locate`] `→ CursorState` |
//! | External lookup (behind the setting) | [`ExternalLyricsProvider`] with [`LrclibProvider`] + [`ReqwestLyricsHttp`] |
//! | Cache storage keys / JSON | [`cache_key`], [`to_cache_json`], [`from_cache_json`], [`offset_state_key`] |
//!
//! Tier rules: syllable timing only where the server supplied it, never
//! interpolated; a line without complete cue timing is a plain timed line
//! even inside a syllable-tier document.

pub mod adapt;
pub mod cursor;
pub mod external;
pub mod lrc;
pub mod raw;

#[cfg(test)]
mod lyrics_never_panics;
#[cfg(test)]
mod tests;

pub use adapt::{adapt_entry, adapt_list, from_plain_text, pick_main};
pub use cursor::{locate, CursorState, LyricsCursor};
pub use external::{
    ExternalLyricsProvider, LrclibProvider, LyricsError, LyricsHttp, LyricsRequest,
    ReqwestLyricsHttp,
};
pub use lrc::{lrc_to_lyrics, parse_lrc, LrcDocument, LrcLine};
pub use raw::{LyricsList, LyricsListResponse, StructuredLyrics};

use crate::api::{Lyrics, LyricsSource, Ms};

/// Adapts a parsed server response for a track.
pub fn adapt_response(track_id: &str, response: &LyricsListResponse) -> Option<Lyrics> {
    adapt_list(track_id, response.entries(), LyricsSource::Server)
}

/// Records the user's per-track offset without touching timing. Positive
/// means "show lyrics later". The cursor honours it.
pub fn set_user_offset(lyrics: &mut Lyrics, offset_ms: i32) {
    lyrics.offset_ms = offset_ms;
}

/// Returns a copy with `offset_ms` baked into every timestamp and reset to
/// zero — for renderers that consume timings directly.
pub fn apply_offset(lyrics: &Lyrics) -> Lyrics {
    let shift = i64::from(lyrics.offset_ms);
    let bump = |t: Ms| -> Ms { (i64::from(t) + shift).clamp(0, i64::from(u32::MAX)) as Ms };
    let mut out = lyrics.clone();
    for line in &mut out.lines {
        line.start_ms = line.start_ms.map(bump);
        line.end_ms = line.end_ms.map(bump);
        for s in &mut line.syllables {
            s.start_ms = bump(s.start_ms);
            s.end_ms = bump(s.end_ms);
        }
    }
    out.offset_ms = 0;
    out
}

/// `lyrics_cache` primary key parts: `(server_id, track_id, source)`.
pub fn cache_key(
    server_id: &str,
    track_id: &str,
    source: LyricsSource,
) -> (String, String, &'static str) {
    (
        server_id.to_string(),
        track_id.to_string(),
        source_name(source),
    )
}

/// Column value for `lyrics_cache.source`.
pub fn source_name(source: LyricsSource) -> &'static str {
    match source {
        LyricsSource::Server => "server",
        LyricsSource::External => "external",
        LyricsSource::Embedded => "embedded",
    }
}

/// `saved_state` key holding a track's user offset (`{"offsetMs": n}`).
pub fn offset_state_key(track_id: &str) -> String {
    format!("lyricsOffset:{track_id}")
}

/// Serialises for `lyrics_cache.json`. Empty string is the negative cache.
pub fn to_cache_json(lyrics: Option<&Lyrics>) -> String {
    lyrics
        .and_then(|l| serde_json::to_string(l).ok())
        .unwrap_or_default()
}

/// Reads back what [`to_cache_json`] wrote. `Ok(None)` for the negative cache.
pub fn from_cache_json(json: &str) -> Result<Option<Lyrics>, serde_json::Error> {
    if json.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(json).map(Some)
}
