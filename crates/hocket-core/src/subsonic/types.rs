//! Raw serde models of Subsonic 1.16.1 / OpenSubsonic / Navidrome JSON.
//!
//! These mirror the wire shape (camelCase, everything optional) so a Navidrome
//! response round-trips without loss. Conversions to the UI-facing `api`
//! types live in [`super::convert`]. Field names follow Navidrome's
//! `server/subsonic/responses/responses.go`.

use serde::{Deserialize, Serialize};

/// `{"subsonic-response": {...}}`
#[derive(Debug, Clone, Deserialize)]
pub struct Envelope {
    #[serde(rename = "subsonic-response")]
    pub subsonic_response: SubsonicResponse,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubsonicResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: String,
    #[serde(default, rename = "type")]
    pub server_type: Option<String>,
    #[serde(default)]
    pub server_version: Option<String>,
    #[serde(default)]
    pub open_subsonic: bool,
    #[serde(default)]
    pub error: Option<ErrorBody>,

    #[serde(default)]
    pub open_subsonic_extensions: Option<Vec<OpenSubsonicExtension>>,
    #[serde(default)]
    pub music_folders: Option<MusicFolders>,
    #[serde(default)]
    pub artists: Option<ArtistsIndex>,
    #[serde(default)]
    pub artist: Option<ArtistWithAlbums>,
    #[serde(default)]
    pub album: Option<AlbumWithSongs>,
    #[serde(default)]
    pub album_list2: Option<AlbumList2>,
    #[serde(default)]
    pub song: Option<Child>,
    #[serde(default)]
    pub random_songs: Option<Songs>,
    #[serde(default)]
    pub songs_by_genre: Option<Songs>,
    #[serde(default)]
    pub genres: Option<Genres>,
    #[serde(default)]
    pub starred2: Option<Starred2>,
    #[serde(default)]
    pub playlists: Option<Playlists>,
    #[serde(default)]
    pub playlist: Option<PlaylistWithSongs>,
    #[serde(default)]
    pub search_result3: Option<SearchResult3>,
    #[serde(default)]
    pub lyrics_list: Option<LyricsList>,
    #[serde(default)]
    pub similar_songs2: Option<Songs>,
    #[serde(default)]
    pub top_songs: Option<Songs>,
    #[serde(default)]
    pub artist_info2: Option<ArtistInfo2>,
    /// `sonicSimilarity` extension (Navidrome ≥ 0.62): a flat array of matches.
    #[serde(default)]
    pub sonic_match: Option<Vec<SonicMatch>>,
    /// Defensive: some drafts wrapped the matches in an object.
    #[serde(default)]
    pub sonic_similar_tracks: Option<SonicMatchList>,
    #[serde(default)]
    pub scan_status: Option<ScanStatus>,
    #[serde(default)]
    pub play_queue: Option<PlayQueue>,
    #[serde(default)]
    pub now_playing: Option<NowPlaying>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorBody {
    #[serde(default)]
    pub code: u32,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub help_url: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OpenSubsonicExtension {
    pub name: String,
    #[serde(default)]
    pub versions: Vec<u32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicFolders {
    #[serde(default)]
    pub music_folder: Vec<MusicFolder>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicFolder {
    #[serde(default, deserialize_with = "de_string_or_number")]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistsIndex {
    #[serde(default)]
    pub ignored_articles: Option<String>,
    #[serde(default)]
    pub index: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: Vec<ArtistId3>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtistId3 {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub album_count: Option<u32>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default)]
    pub user_rating: Option<u32>,
    #[serde(default)]
    pub artist_image_url: Option<String>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    #[serde(default)]
    pub sort_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistWithAlbums {
    #[serde(flatten)]
    pub artist: ArtistId3,
    #[serde(default)]
    pub album: Vec<AlbumId3>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplayGainBody {
    #[serde(default)]
    pub track_gain: Option<f64>,
    #[serde(default)]
    pub album_gain: Option<f64>,
    #[serde(default)]
    pub track_peak: Option<f64>,
    #[serde(default)]
    pub album_peak: Option<f64>,
    #[serde(default)]
    pub base_gain: Option<f64>,
    #[serde(default)]
    pub fallback_gain: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemGenre {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ItemArtist {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlbumId3 {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<u32>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub play_count: Option<u32>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    // OpenSubsonic
    #[serde(default)]
    pub played: Option<String>,
    #[serde(default)]
    pub user_rating: Option<u32>,
    #[serde(default)]
    pub genres: Vec<ItemGenre>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    #[serde(default)]
    pub is_compilation: Option<bool>,
    #[serde(default)]
    pub sort_name: Option<String>,
    #[serde(default)]
    pub artists: Vec<ItemArtist>,
    #[serde(default)]
    pub display_artist: Option<String>,
    #[serde(default)]
    pub explicit_status: Option<String>,
    #[serde(default)]
    pub moods: Vec<String>,
    #[serde(default)]
    pub replay_gain: Option<ReplayGainBody>,
    /// Navidrome exposes the modification time as `changed`/`updated` depending on version.
    #[serde(default)]
    pub changed: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumWithSongs {
    #[serde(flatten)]
    pub album: AlbumId3,
    #[serde(default)]
    pub song: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumList2 {
    #[serde(default)]
    pub album: Vec<AlbumId3>,
}

/// A song (`Child` in Subsonic terms). Directories/videos are never requested.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Child {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub is_dir: bool,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub track: Option<u32>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub size: Option<f64>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub transcoded_content_type: Option<String>,
    #[serde(default)]
    pub transcoded_suffix: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub bit_rate: Option<u32>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub play_count: Option<u32>,
    #[serde(default)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub starred: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default, rename = "type")]
    pub media_type_legacy: Option<String>,
    #[serde(default)]
    pub user_rating: Option<u32>,
    #[serde(default)]
    pub average_rating: Option<f64>,
    #[serde(default)]
    pub bookmark_position: Option<f64>,
    // OpenSubsonic
    #[serde(default)]
    pub played: Option<String>,
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub sort_name: Option<String>,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    #[serde(default)]
    pub isrc: Vec<String>,
    #[serde(default)]
    pub genres: Vec<ItemGenre>,
    #[serde(default)]
    pub replay_gain: Option<ReplayGainBody>,
    #[serde(default)]
    pub channel_count: Option<u32>,
    #[serde(default)]
    pub sampling_rate: Option<u32>,
    #[serde(default)]
    pub bit_depth: Option<u32>,
    #[serde(default)]
    pub moods: Vec<String>,
    #[serde(default)]
    pub artists: Vec<ItemArtist>,
    #[serde(default)]
    pub display_artist: Option<String>,
    #[serde(default)]
    pub album_artists: Vec<ItemArtist>,
    #[serde(default)]
    pub display_album_artist: Option<String>,
    #[serde(default)]
    pub explicit_status: Option<String>,
    /// Navidrome-specific modification timestamp when present.
    #[serde(default)]
    pub changed: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Songs {
    #[serde(default)]
    pub song: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Genres {
    #[serde(default)]
    pub genre: Vec<GenreBody>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GenreBody {
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub album_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Starred2 {
    #[serde(default)]
    pub artist: Vec<ArtistId3>,
    #[serde(default)]
    pub album: Vec<AlbumId3>,
    #[serde(default)]
    pub song: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlists {
    #[serde(default)]
    pub playlist: Vec<PlaylistBody>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistBody {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub changed: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    /// OpenSubsonic: smart playlists are reported read-only.
    #[serde(default)]
    pub readonly: Option<bool>,
    #[serde(default)]
    pub valid_until: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistWithSongs {
    #[serde(flatten)]
    pub playlist: PlaylistBody,
    #[serde(default)]
    pub entry: Vec<Child>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult3 {
    #[serde(default)]
    pub artist: Vec<ArtistId3>,
    #[serde(default)]
    pub album: Vec<AlbumId3>,
    #[serde(default)]
    pub song: Vec<Child>,
}

// -- Lyrics (OpenSubsonic songLyrics v1 + v2 structured) --------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsList {
    #[serde(default)]
    pub structured_lyrics: Vec<StructuredLyrics>,
}

/// One lyrics document for a song. v2 adds `kind`, `agents` and `cueLine`
/// (word/syllable timing); the `lyrics` module adapts this to `api::Lyrics`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StructuredLyrics {
    #[serde(default)]
    pub display_artist: Option<String>,
    #[serde(default)]
    pub display_title: Option<String>,
    /// ISO 639 or `und`.
    #[serde(default)]
    pub lang: String,
    #[serde(default)]
    pub synced: bool,
    /// Milliseconds; positive means lyrics appear sooner.
    #[serde(default)]
    pub offset: Option<i64>,
    /// `main` (default), `translation`, `pronunciation`.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub line: Vec<LyricLineBody>,
    #[serde(default)]
    pub agents: Vec<LyricAgent>,
    #[serde(default)]
    pub cue_line: Vec<CueLine>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricLineBody {
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub value: String,
}

/// Reusable per-track attribution for cue lines (voices in a duet, chorus).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricAgent {
    #[serde(default)]
    pub id: String,
    /// `main`, `voice`, `bg`, `group`.
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub name: Option<String>,
}

/// Word/syllable-level timing for `line[index]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CueLine {
    #[serde(default)]
    pub index: u32,
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub cue: Vec<LyricCue>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LyricCue {
    #[serde(default)]
    pub start: Option<i64>,
    #[serde(default)]
    pub end: Option<i64>,
    #[serde(default)]
    pub value: String,
    /// 0-based, inclusive byte offsets into the parent cue line's `value`
    /// (Navidrome: `"I"` at the start of a line is `0..=0`).
    #[serde(default)]
    pub byte_start: Option<u32>,
    #[serde(default)]
    pub byte_end: Option<u32>,
}

// -- Artist info / similarity ----------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo2 {
    #[serde(default)]
    pub biography: Option<String>,
    #[serde(default)]
    pub music_brainz_id: Option<String>,
    #[serde(default)]
    pub last_fm_url: Option<String>,
    #[serde(default)]
    pub small_image_url: Option<String>,
    #[serde(default)]
    pub medium_image_url: Option<String>,
    #[serde(default)]
    pub large_image_url: Option<String>,
    #[serde(default)]
    pub similar_artist: Vec<ArtistId3>,
}

/// `sonicSimilarity` match: a song and a normalised similarity in 0..1
/// (`-1` when the plugin can't say).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SonicMatch {
    #[serde(default, alias = "song")]
    pub entry: Child,
    #[serde(default)]
    pub similarity: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SonicMatchList {
    #[serde(default)]
    pub sonic_match: Vec<SonicMatch>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    #[serde(default)]
    pub scanning: bool,
    #[serde(default)]
    pub count: Option<f64>,
    #[serde(default)]
    pub folder_count: Option<f64>,
    #[serde(default)]
    pub last_scan: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub scan_type: Option<String>,
    #[serde(default)]
    pub elapsed_time: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayQueue {
    #[serde(default)]
    pub entry: Vec<Child>,
    #[serde(default, deserialize_with = "de_opt_string_or_number")]
    pub current: Option<String>,
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub changed: Option<String>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    #[serde(default)]
    pub entry: Vec<NowPlayingEntry>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlayingEntry {
    #[serde(flatten)]
    pub song: Child,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub minutes_ago: Option<u32>,
    #[serde(default)]
    pub player_id: Option<u32>,
    #[serde(default)]
    pub player_name: Option<String>,
}

// -- Navidrome native API ---------------------------------------------------

/// `POST /auth/login` response.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLogin {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub is_admin: bool,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub subsonic_salt: Option<String>,
    #[serde(default)]
    pub subsonic_token: Option<String>,
}

/// `GET /api/playlist/:id` (subset). `rules` is Navidrome's smart-playlist
/// criteria object, passed through as raw JSON for the filters module.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaylist {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub size: f64,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub owner_name: Option<String>,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sync: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub rules: Option<serde_json::Value>,
    #[serde(default)]
    pub evaluated_at: Option<String>,
}

// -- helpers ----------------------------------------------------------------

fn de_string_or_number<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn de_opt_string_or_number<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<String>, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(match v {
        None | Some(serde_json::Value::Null) => None,
        Some(serde_json::Value::String(s)) => Some(s),
        Some(serde_json::Value::Number(n)) => Some(n.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

/// Parse an ISO-8601 / RFC 3339 timestamp (`2024-05-01T12:34:56.789Z`,
/// with optional fractional seconds and `Z` or `±HH:MM` offset) to epoch
/// milliseconds. Returns `None` for anything unparsable, including
/// Go's zero time (`0001-01-01T00:00:00Z`).
pub fn parse_iso_ms(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let b = s.as_bytes();
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse::<i64>().ok() };
    let year = num(0, 4)?;
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    if year < 1900 {
        return None;
    }
    let (mut hour, mut minute, mut second, mut frac_ms, mut offset_min) =
        (0i64, 0i64, 0i64, 0i64, 0i64);
    if s.len() > 10 {
        if !matches!(b[10], b'T' | b't' | b' ') {
            return None;
        }
        hour = num(11, 13)?;
        minute = num(14, 16)?;
        second = num(17, 19)?;
        let mut i = 19;
        if b.get(i) == Some(&b'.') {
            let start = i + 1;
            let mut end = start;
            while end < b.len() && b[end].is_ascii_digit() {
                end += 1;
            }
            let digits = &s[start..end];
            let padded: String = digits
                .chars()
                .chain(std::iter::repeat('0'))
                .take(3)
                .collect();
            frac_ms = padded.parse().ok()?;
            i = end;
        }
        match b.get(i) {
            None | Some(b'Z') | Some(b'z') => {}
            Some(b'+') | Some(b'-') => {
                let sign = if b[i] == b'+' { 1 } else { -1 };
                let oh = num(i + 1, i + 3)?;
                let om = if b.get(i + 3) == Some(&b':') {
                    num(i + 4, i + 6)?
                } else {
                    num(i + 3, i + 5).unwrap_or(0)
                };
                offset_min = sign * (oh * 60 + om);
            }
            _ => return None,
        }
    }
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_min * 60;
    Some((secs * 1000 + frac_ms) as f64)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parsing() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(
            parse_iso_ms("2024-05-01T12:34:56.789Z"),
            Some(1_714_566_896_789.0)
        );
        assert_eq!(
            parse_iso_ms("2024-05-01T12:34:56.789123456Z"),
            Some(1_714_566_896_789.0)
        );
        assert_eq!(
            parse_iso_ms("2024-05-01T14:34:56+02:00"),
            Some(1_714_566_896_000.0)
        );
        assert_eq!(parse_iso_ms("2024-05-01"), Some(1_714_521_600_000.0));
        assert_eq!(parse_iso_ms("0001-01-01T00:00:00Z"), None);
        assert_eq!(parse_iso_ms("garbage"), None);
        assert_eq!(parse_iso_ms(""), None);
    }
}
