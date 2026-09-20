//! Wire types for OpenSubsonic `getLyricsBySongId` (song lyrics v1 and the
//! v2 "enhanced" shape Navidrome 0.63 emits with `enhanced=true`).
//!
//! Field names follow Navidrome's `server/subsonic/responses` structs:
//! `lyricsList.structuredLyrics[]` with `displayArtist`, `displayTitle`,
//! `kind` (`main` | `translation` | `pronunciation`), `lang`, `offset`,
//! `synced`, `line[] {start, value}`, `agents[] {id, role, name}` and
//! `cueLine[] {index, start, end, value, agentId, cue[] {start, end, value,
//! byteStart, byteEnd}}`. Everything optional is `Option`/`default` so a
//! v1 server or an older Navidrome still parses. The subsonic client
//! (core-server) can deserialise straight into [`LyricsListResponse`].

use serde::{Deserialize, Serialize};

/// The `subsonic-response` payload fields we care about.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsListResponse {
    #[serde(default)]
    pub lyrics_list: Option<LyricsList>,
}

impl LyricsListResponse {
    /// Parses either the full envelope (`{"subsonic-response": {...}}`) or
    /// the bare payload object.
    pub fn parse(json: &str) -> Result<LyricsListResponse, serde_json::Error> {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(rename = "subsonic-response")]
            inner: LyricsListResponse,
        }
        match serde_json::from_str::<Envelope>(json) {
            Ok(env) => Ok(env.inner),
            Err(_) => serde_json::from_str::<LyricsListResponse>(json),
        }
    }

    pub fn entries(&self) -> &[StructuredLyrics] {
        self.lyrics_list
            .as_ref()
            .map(|l| l.structured_lyrics.as_slice())
            .unwrap_or(&[])
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsList {
    #[serde(default)]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

/// One `structuredLyrics` entry: a self-contained lyric layer.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredLyrics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_artist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// `main`, `translation`, `pronunciation`; blank means main.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default)]
    pub lang: String,
    /// Milliseconds; same convention as the LRC `[offset:]` tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<i64>,
    #[serde(default)]
    pub synced: bool,
    #[serde(default)]
    pub line: Vec<RawLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<RawAgent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cue_line: Vec<RawCueLine>,
}

impl StructuredLyrics {
    /// `kind` with the blank-means-main rule applied.
    pub fn effective_kind(&self) -> &str {
        match self.kind.as_deref().map(str::trim) {
            Some("") | None => "main",
            Some(k) => k,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawAgent {
    pub id: String,
    /// `main`, `voice`, `bg`, `group`.
    #[serde(default)]
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Word/syllable timing for one line. `index` refers to `line[]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCueLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub cue: Vec<RawCue>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawCue {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<i64>,
    #[serde(default)]
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub byte_end: Option<i64>,
    /// Navidrome's model carries an agent per cue too; honoured as a fallback
    /// when the cue line itself has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
}
