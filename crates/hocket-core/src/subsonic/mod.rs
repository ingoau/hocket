//! Subsonic 1.16.1 / OpenSubsonic / Navidrome-native client. Owner: core-server.
//!
//! Entry points:
//! - [`Client`] — one per server. Built from a [`ClientConfig`] (base URL,
//!   [`AuthMode`], retry policy) and an [`HttpTransport`] (reqwest in
//!   production, [`FakeTransport`] in tests). Implements [`SubsonicApi`].
//! - [`SubsonicApi`] — the trait every consumer (sync, outbox, downloads,
//!   autoplay, lyrics) programs against, so the harness can substitute
//!   [`fake::FakeServer`].
//! - [`Client::probe`] — the capability probe: `ping`,
//!   `getOpenSubsonicExtensions`, and a native-API login/keepalive; returns
//!   [`api::ServerCapabilities`] and switches the client to `apiKey` auth when
//!   the server advertises it.
//! - [`Credential`] — secret bytes that zeroise on drop. The platform keystore
//!   hands the password in; the core never persists it.
//! - URL builders ([`SubsonicApi::stream_url`], [`SubsonicApi::cover_art_url`],
//!   [`SubsonicApi::download_url`]) mint a fresh token+salt per call.
//!
//! Auth is token+salt (`t`/`s`) or OpenSubsonic `apiKey`; plaintext `p=` and
//! `jwt=` are not representable. Errors are typed ([`SubsonicError`]) and map
//! onto [`api::ErrorKind`] via [`SubsonicError::kind`].

pub mod auth;
pub mod client;
pub mod convert;
pub mod fake;
pub mod native;
pub mod probe;
pub mod transport;
pub mod types;

#[cfg(test)]
mod tests;

use std::path::Path;

use futures::future::BoxFuture;
use url::Url;

use crate::api::{self, ErrorKind};

pub use auth::{AuthMode, Credential};
pub use client::{AlbumListType, Client, ClientConfig, RetryPolicy, Search3Page, StreamOptions};
pub use native::{NativePlaylistUpdate, NativeSession};
pub use transport::{FakeReply, FakeTransport, HttpTransport, ReqwestTransport};
pub use types::*;

/// Typed client error. `kind()` maps it onto the seam's [`ErrorKind`].
#[derive(Debug, Clone, thiserror::Error, PartialEq)]
pub enum SubsonicError {
    /// Subsonic error 40/41/42/43/44 or HTTP 401/403.
    #[error("authentication failed: {0}")]
    Auth(String),
    /// Transport failure or timeout (retried before surfacing).
    #[error("network: {0}")]
    Network(String),
    /// Subsonic error 70 or HTTP 404 (also: extension endpoint absent).
    #[error("not found: {0}")]
    NotFound(String),
    /// Subsonic error 50 (not authorised for the operation) or 60.
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// Any other `status: failed` (code, message) or HTTP 5xx after retries.
    #[error("server error {code}: {message}")]
    Server { code: u32, message: String },
    /// Response wasn't the JSON we expect.
    #[error("protocol: {0}")]
    Protocol(String),
    /// The server does not advertise the capability the call needs.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Local I/O (downloads).
    #[error("io: {0}")]
    Io(String),
}

impl SubsonicError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            SubsonicError::Auth(_) => ErrorKind::Auth,
            SubsonicError::Network(_) => ErrorKind::Network,
            SubsonicError::NotFound(_)
            | SubsonicError::Forbidden(_)
            | SubsonicError::Server { .. } => ErrorKind::Server,
            SubsonicError::Protocol(_) | SubsonicError::Unsupported(_) => ErrorKind::Protocol,
            SubsonicError::Io(_) => ErrorKind::Storage,
        }
    }

    /// Whether a retry could plausibly succeed without user intervention.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            SubsonicError::Network(_) | SubsonicError::Server { .. }
        )
    }

    pub(crate) fn from_code(code: u32, message: String) -> Self {
        match code {
            40..=44 => SubsonicError::Auth(message),
            50 | 60 => SubsonicError::Forbidden(message),
            70 => SubsonicError::NotFound(message),
            _ => SubsonicError::Server { code, message },
        }
    }
}

impl From<transport::TransportError> for SubsonicError {
    fn from(e: transport::TransportError) -> Self {
        match e {
            transport::TransportError::Network(m) => SubsonicError::Network(m),
            transport::TransportError::Timeout => SubsonicError::Network("timeout".into()),
            transport::TransportError::Io(e) => SubsonicError::Io(e.to_string()),
        }
    }
}

pub type SubsonicResult<T> = Result<T, SubsonicError>;
pub type ApiFuture<'a, T> = BoxFuture<'a, SubsonicResult<T>>;

/// Which entities `star`/`unstar`/`setRating` target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StarTarget {
    Song(String),
    Album(String),
    Artist(String),
}

/// Arguments for `updatePlaylist`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistUpdate {
    pub name: Option<String>,
    pub comment: Option<String>,
    pub public: Option<bool>,
    pub song_ids_to_add: Vec<String>,
    pub song_indices_to_remove: Vec<u32>,
}

/// Arguments for `savePlayQueue`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayQueueSave {
    pub song_ids: Vec<String>,
    pub current: Option<String>,
    pub position_ms: Option<u32>,
}

/// The Subsonic surface the rest of the core uses. Object-safe (boxed
/// futures) so it can be stored as `Arc<dyn SubsonicApi>` and faked.
pub trait SubsonicApi: Send + Sync + 'static {
    /// Server id this client is bound to (multi-server framework).
    fn server_id(&self) -> &str;
    /// Authenticated username, when known.
    fn username(&self) -> Option<String>;
    /// Last probed capabilities (empty default before a probe).
    fn capabilities(&self) -> api::ServerCapabilities;

    fn ping(&self) -> ApiFuture<'_, SubsonicResponse>;
    fn open_subsonic_extensions(&self) -> ApiFuture<'_, Vec<OpenSubsonicExtension>>;
    fn music_folders(&self) -> ApiFuture<'_, Vec<MusicFolder>>;
    fn artists(&self) -> ApiFuture<'_, ArtistsIndex>;
    fn artist(&self, id: &str) -> ApiFuture<'_, ArtistWithAlbums>;
    fn album(&self, id: &str) -> ApiFuture<'_, AlbumWithSongs>;
    fn album_list2(
        &self,
        kind: AlbumListType,
        size: u32,
        offset: u32,
    ) -> ApiFuture<'_, Vec<AlbumId3>>;
    fn song(&self, id: &str) -> ApiFuture<'_, Child>;
    fn random_songs(
        &self,
        size: u32,
        genre: Option<&str>,
        from_year: Option<u32>,
        to_year: Option<u32>,
    ) -> ApiFuture<'_, Vec<Child>>;
    fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> ApiFuture<'_, Vec<Child>>;
    fn genres(&self) -> ApiFuture<'_, Vec<GenreBody>>;
    fn starred2(&self) -> ApiFuture<'_, Starred2>;
    fn playlists(&self) -> ApiFuture<'_, Vec<PlaylistBody>>;
    fn playlist(&self, id: &str) -> ApiFuture<'_, PlaylistWithSongs>;
    /// `createPlaylist` with a name: a new playlist with the given songs.
    fn create_playlist(&self, name: &str, song_ids: &[String]) -> ApiFuture<'_, PlaylistWithSongs>;
    /// `createPlaylist` with `playlistId`: replaces the playlist's contents.
    fn replace_playlist(
        &self,
        playlist_id: &str,
        song_ids: &[String],
    ) -> ApiFuture<'_, PlaylistWithSongs>;
    fn update_playlist(&self, id: &str, update: PlaylistUpdate) -> ApiFuture<'_, ()>;
    fn delete_playlist(&self, id: &str) -> ApiFuture<'_, ()>;
    /// `search3`. An empty query with paging is the full-library sync path.
    fn search3(&self, query: &str, page: Search3Page) -> ApiFuture<'_, SearchResult3>;
    fn lyrics_by_song_id(&self, id: &str) -> ApiFuture<'_, Vec<StructuredLyrics>>;
    fn similar_songs2(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<Child>>;
    fn top_songs(&self, artist: &str, count: u32) -> ApiFuture<'_, Vec<Child>>;
    fn artist_info2(
        &self,
        id: &str,
        count: u32,
        include_not_present: bool,
    ) -> ApiFuture<'_, ArtistInfo2>;
    /// `sonicSimilarity` extension. `Unsupported` when not advertised.
    fn sonic_similar_tracks(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<SonicMatch>>;
    fn star(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()>;
    fn unstar(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()>;
    /// `setRating` on a song or album id (Subsonic ratings are 0–5; 0 clears).
    fn set_rating(&self, id: &str, rating: u32) -> ApiFuture<'_, ()>;
    /// `scrobble`. `time` is epoch ms; `submission=false` is "now playing".
    fn scrobble(&self, id: &str, time_ms: Option<f64>, submission: bool) -> ApiFuture<'_, ()>;
    fn scan_status(&self) -> ApiFuture<'_, ScanStatus>;
    fn save_play_queue(&self, save: PlayQueueSave) -> ApiFuture<'_, ()>;
    fn play_queue(&self) -> ApiFuture<'_, Option<PlayQueue>>;
    fn now_playing(&self) -> ApiFuture<'_, Vec<NowPlayingEntry>>;

    /// `getCoverArt` URL for an id at a fixed size.
    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Url;
    /// `stream` URL. Honours the transcoding options and the `transcodeOffset`
    /// capability for `time_offset`.
    fn stream_url(&self, id: &str, options: &StreamOptions) -> Url;
    /// `download` URL (original file, never transcoded).
    fn download_url(&self, id: &str) -> Url;
    /// Fetch a URL (cover art, stream, download) to a file. Returns bytes written.
    fn download_to_file(&self, url: Url, dest: &Path) -> ApiFuture<'_, transport::DownloadOutcome>;

    // -- Navidrome native API (behind `capabilities().native_api`) --------
    /// Read a playlist including smart-playlist `rules`.
    fn native_playlist(&self, id: &str) -> ApiFuture<'_, NativePlaylist>;
    /// Update playlist metadata and/or rules.
    fn native_update_playlist(
        &self,
        id: &str,
        update: NativePlaylistUpdate,
    ) -> ApiFuture<'_, NativePlaylist>;
    /// Create a (smart) playlist through the native API; returns its id.
    fn native_create_playlist(&self, update: NativePlaylistUpdate) -> ApiFuture<'_, String>;
}
