//! The concrete [`Client`]: URL building, per-request auth, retry/backoff,
//! rate limiting and JSON envelope handling.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use tokio::sync::Semaphore;
use url::Url;

use crate::api::ServerCapabilities;

use super::auth::{auth_params, AuthMode, Credential};
use super::native::{NativePlaylistUpdate, NativeSession};
use super::transport::{DownloadOutcome, HttpRequest, HttpResponse, HttpTransport};
use super::types::*;
use super::{
    ApiFuture, PlayQueueSave, PlaylistUpdate, StarTarget, SubsonicApi, SubsonicError,
    SubsonicResult,
};

/// How failed requests are retried. Only transient failures (network,
/// timeout, HTTP 5xx/429) retry; auth and protocol errors surface at once.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts including the first.
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 4,
            initial_backoff: Duration::from_millis(500),
            max_backoff: Duration::from_secs(8),
        }
    }
}

impl RetryPolicy {
    /// No retries, no waiting (tests).
    pub fn none() -> Self {
        RetryPolicy {
            max_attempts: 1,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
        }
    }

    pub fn backoff_for(&self, attempt: u32) -> Duration {
        let mult = 2u32.saturating_pow(attempt.saturating_sub(1));
        self.initial_backoff
            .saturating_mul(mult)
            .min(self.max_backoff)
    }
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub server_id: String,
    /// Server root, e.g. `https://music.example.org/` (path prefixes allowed).
    pub base_url: Url,
    pub auth: AuthMode,
    pub retry: RetryPolicy,
    /// Maximum in-flight requests to this server.
    pub max_concurrent: usize,
}

impl ClientConfig {
    pub fn new(server_id: impl Into<String>, base_url: Url, auth: AuthMode) -> Self {
        ClientConfig {
            server_id: server_id.into(),
            base_url,
            auth,
            retry: RetryPolicy::default(),
            max_concurrent: 4,
        }
    }
}

/// `getAlbumList2` types.
#[derive(Debug, Clone, PartialEq)]
pub enum AlbumListType {
    Random,
    Newest,
    Highest,
    Frequent,
    Recent,
    AlphabeticalByName,
    AlphabeticalByArtist,
    Starred,
    ByYear { from_year: u32, to_year: u32 },
    ByGenre(String),
}

/// Paging for `search3`. Navidrome caps each count at 500 per request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Search3Page {
    pub artist_count: u32,
    pub artist_offset: u32,
    pub album_count: u32,
    pub album_offset: u32,
    pub song_count: u32,
    pub song_offset: u32,
}

impl Search3Page {
    pub fn songs(count: u32, offset: u32) -> Self {
        Search3Page {
            artist_count: 0,
            artist_offset: 0,
            album_count: 0,
            album_offset: 0,
            song_count: count,
            song_offset: offset,
        }
    }
    pub fn albums(count: u32, offset: u32) -> Self {
        Search3Page {
            artist_count: 0,
            artist_offset: 0,
            album_count: count,
            album_offset: offset,
            song_count: 0,
            song_offset: 0,
        }
    }
    pub fn artists(count: u32, offset: u32) -> Self {
        Search3Page {
            artist_count: count,
            artist_offset: offset,
            album_count: 0,
            album_offset: 0,
            song_count: 0,
            song_offset: 0,
        }
    }
    pub fn all(count: u32) -> Self {
        Search3Page {
            artist_count: count,
            artist_offset: 0,
            album_count: count,
            album_offset: 0,
            song_count: count,
            song_offset: 0,
        }
    }
}

/// `stream` parameters. `format = None` and `max_bit_rate = None` request the
/// original ("raw") file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamOptions {
    pub format: Option<String>,
    pub max_bit_rate: Option<u32>,
    /// Seconds into the track (`transcodeOffset` extension).
    pub time_offset_s: Option<u32>,
    pub estimate_content_length: bool,
}

/// The Subsonic client for one server.
pub struct Client {
    config: ClientConfig,
    auth: RwLock<AuthMode>,
    transport: Arc<dyn HttpTransport>,
    limiter: Arc<Semaphore>,
    caps: RwLock<ServerCapabilities>,
    pub(super) native: NativeSession,
}

impl Client {
    /// Build a client after validating the base URL (see
    /// [`super::validate_base_url`]). Prefer this over [`Client::new`] for
    /// user-supplied URLs.
    pub fn try_new(
        config: ClientConfig,
        transport: Arc<dyn HttpTransport>,
    ) -> SubsonicResult<Self> {
        super::validate_base_url(&config.base_url)?;
        Ok(Self::new(config, transport))
    }

    pub fn new(config: ClientConfig, transport: Arc<dyn HttpTransport>) -> Self {
        let limiter = Arc::new(Semaphore::new(config.max_concurrent.max(1)));
        Client {
            auth: RwLock::new(config.auth.clone()),
            native: NativeSession::default(),
            config,
            transport,
            limiter,
            caps: RwLock::new(ServerCapabilities::default()),
        }
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    pub fn base_url(&self) -> &Url {
        &self.config.base_url
    }

    /// The underlying transport (shared with the image cache).
    pub fn transport(&self) -> &Arc<dyn HttpTransport> {
        &self.transport
    }

    pub fn auth_mode(&self) -> AuthMode {
        self.auth.read().clone()
    }

    /// Switch to `apiKey` auth (after the probe reports support and the
    /// platform obtained a key). The password can then be dropped.
    pub fn set_api_key(&self, api_key: Credential) {
        *self.auth.write() = AuthMode::ApiKey { api_key };
    }

    pub fn set_capabilities(&self, caps: ServerCapabilities) {
        *self.caps.write() = caps;
    }

    /// `base/rest/<endpoint>` with auth + protocol params and `params`.
    /// Fails (never panics) when the base URL cannot take a path.
    pub fn rest_url(&self, endpoint: &str, params: &[(&str, String)]) -> SubsonicResult<Url> {
        let mut url = self.config.base_url.clone();
        {
            let mut segs = url
                .path_segments_mut()
                .map_err(|_| SubsonicError::Protocol("server url cannot take a path".into()))?;
            segs.pop_if_empty();
            segs.push("rest");
            segs.push(endpoint);
        }
        {
            let auth = self.auth.read();
            let mut q = url.query_pairs_mut();
            for (k, v) in auth_params(&auth) {
                q.append_pair(&k, &v);
            }
            for (k, v) in params {
                q.append_pair(k, v);
            }
        }
        Ok(url)
    }

    /// `rest_url` for the infallible URL-builder trait methods: a base URL
    /// that cannot take a path was rejected at `add_server` time, so this is
    /// unreachable in practice; rather than panic, hand back a URL that will
    /// fail the request.
    fn rest_url_or_base(&self, endpoint: &str, params: &[(&str, String)]) -> Url {
        self.rest_url(endpoint, params).unwrap_or_else(|e| {
            tracing::error!(error = %e, endpoint, "cannot build rest url");
            self.config.base_url.clone()
        })
    }

    /// Execute one endpoint with retries; returns the inner response on `status: ok`.
    pub async fn call(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> SubsonicResult<SubsonicResponse> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            // Fresh salt on every attempt.
            let url = self.rest_url(endpoint, params)?;
            let result = self.execute_raw(HttpRequest::get(url)).await;
            match result {
                Ok(resp) => match classify(resp) {
                    Ok(body) => return parse_envelope(&body),
                    Err(e) if e.is_transient() && attempt < self.config.retry.max_attempts => {
                        tracing::debug!(endpoint, attempt, error = %e, "retrying");
                        tokio::time::sleep(self.config.retry.backoff_for(attempt)).await;
                    }
                    Err(e) => return Err(e),
                },
                Err(e) => {
                    let e: SubsonicError = e.into();
                    if e.is_transient() && attempt < self.config.retry.max_attempts {
                        tracing::debug!(endpoint, attempt, error = %e, "retrying");
                        tokio::time::sleep(self.config.retry.backoff_for(attempt)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }

    /// Raw request through the concurrency limiter (also used by the native API).
    pub(super) async fn execute_raw(
        &self,
        request: HttpRequest,
    ) -> Result<HttpResponse, super::transport::TransportError> {
        let _permit = self
            .limiter
            .acquire()
            .await
            .map_err(|_| super::transport::TransportError::Network("closed".into()))?;
        self.transport.execute(request).await
    }

    fn params_for_stars(targets: &[StarTarget]) -> Vec<(&'static str, String)> {
        targets
            .iter()
            .map(|t| match t {
                StarTarget::Song(id) => ("id", id.clone()),
                StarTarget::Album(id) => ("albumId", id.clone()),
                StarTarget::Artist(id) => ("artistId", id.clone()),
            })
            .collect()
    }
}

/// A 2xx media download that is not media: a JSON/text body (a Subsonic
/// error envelope, an HTML login page) or nothing at all. Returns the typed
/// error to surface; the caller deletes the file.
async fn media_download_error(out: &DownloadOutcome, dest: &Path) -> Option<SubsonicError> {
    let ct = out
        .content_type
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let textual = ct.starts_with("application/json") || ct.starts_with("text/");
    if !textual && out.bytes > 0 {
        return None;
    }
    if out.bytes == 0 {
        return Some(SubsonicError::Protocol("empty media response".into()));
    }
    // Small bodies only: an error envelope is a few hundred bytes.
    let body = if out.bytes <= 64 * 1024 {
        tokio::fs::read(dest).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    Some(match parse_envelope(&body) {
        Err(SubsonicError::Protocol(_)) | Ok(_) => {
            SubsonicError::Protocol(format!("media response was {ct} instead of audio/image"))
        }
        Err(e) => e,
    })
}

/// HTTP-level status → error, or the body for the JSON layer.
fn classify(resp: HttpResponse) -> SubsonicResult<bytes::Bytes> {
    match resp.status {
        200..=299 => Ok(resp.body),
        401 | 403 => Err(SubsonicError::Auth(format!("http {}", resp.status))),
        404 => Err(SubsonicError::NotFound("http 404".into())),
        429 | 500..=599 => Err(SubsonicError::Server {
            code: resp.status as u32,
            message: format!("http {}", resp.status),
        }),
        s => Err(SubsonicError::Protocol(format!(
            "unexpected http status {s}"
        ))),
    }
}

/// Parse the envelope and turn `status: failed` into a typed error.
pub fn parse_envelope(body: &[u8]) -> SubsonicResult<SubsonicResponse> {
    let env: Envelope = serde_json::from_slice(body).map_err(|e| {
        let snippet: String = String::from_utf8_lossy(&body[..body.len().min(120)]).into_owned();
        SubsonicError::Protocol(format!("bad json ({e}): {snippet}"))
    })?;
    let r = env.subsonic_response;
    if r.status != "ok" {
        let err = r.error.unwrap_or_default();
        return Err(SubsonicError::from_code(err.code, err.message));
    }
    Ok(r)
}

fn missing(what: &str) -> SubsonicError {
    SubsonicError::Protocol(format!("response missing `{what}`"))
}

impl SubsonicApi for Client {
    fn server_id(&self) -> &str {
        &self.config.server_id
    }

    fn username(&self) -> Option<String> {
        self.auth
            .read()
            .username()
            .map(String::from)
            .or_else(|| self.native.username())
    }

    fn capabilities(&self) -> ServerCapabilities {
        self.caps.read().clone()
    }

    fn ping(&self) -> ApiFuture<'_, SubsonicResponse> {
        Box::pin(async move { self.call("ping", &[]).await })
    }

    fn open_subsonic_extensions(&self) -> ApiFuture<'_, Vec<OpenSubsonicExtension>> {
        Box::pin(async move {
            let r = self.call("getOpenSubsonicExtensions", &[]).await?;
            Ok(r.open_subsonic_extensions.unwrap_or_default())
        })
    }

    fn music_folders(&self) -> ApiFuture<'_, Vec<MusicFolder>> {
        Box::pin(async move {
            let r = self.call("getMusicFolders", &[]).await?;
            Ok(r.music_folders.map(|m| m.music_folder).unwrap_or_default())
        })
    }

    fn artists(&self) -> ApiFuture<'_, ArtistsIndex> {
        Box::pin(async move {
            self.call("getArtists", &[])
                .await?
                .artists
                .ok_or_else(|| missing("artists"))
        })
    }

    fn artist(&self, id: &str) -> ApiFuture<'_, ArtistWithAlbums> {
        let id = id.to_string();
        Box::pin(async move {
            self.call("getArtist", &[("id", id)])
                .await?
                .artist
                .ok_or_else(|| missing("artist"))
        })
    }

    fn album(&self, id: &str) -> ApiFuture<'_, AlbumWithSongs> {
        let id = id.to_string();
        Box::pin(async move {
            self.call("getAlbum", &[("id", id)])
                .await?
                .album
                .ok_or_else(|| missing("album"))
        })
    }

    fn album_list2(
        &self,
        kind: AlbumListType,
        size: u32,
        offset: u32,
    ) -> ApiFuture<'_, Vec<AlbumId3>> {
        Box::pin(async move {
            let mut params: Vec<(&str, String)> =
                vec![("size", size.to_string()), ("offset", offset.to_string())];
            let ty = match &kind {
                AlbumListType::Random => "random",
                AlbumListType::Newest => "newest",
                AlbumListType::Highest => "highest",
                AlbumListType::Frequent => "frequent",
                AlbumListType::Recent => "recent",
                AlbumListType::AlphabeticalByName => "alphabeticalByName",
                AlbumListType::AlphabeticalByArtist => "alphabeticalByArtist",
                AlbumListType::Starred => "starred",
                AlbumListType::ByYear { from_year, to_year } => {
                    params.push(("fromYear", from_year.to_string()));
                    params.push(("toYear", to_year.to_string()));
                    "byYear"
                }
                AlbumListType::ByGenre(g) => {
                    params.push(("genre", g.clone()));
                    "byGenre"
                }
            };
            params.push(("type", ty.to_string()));
            let r = self.call("getAlbumList2", &params).await?;
            Ok(r.album_list2.map(|a| a.album).unwrap_or_default())
        })
    }

    fn song(&self, id: &str) -> ApiFuture<'_, Child> {
        let id = id.to_string();
        Box::pin(async move {
            self.call("getSong", &[("id", id)])
                .await?
                .song
                .ok_or_else(|| missing("song"))
        })
    }

    fn random_songs(
        &self,
        size: u32,
        genre: Option<&str>,
        from_year: Option<u32>,
        to_year: Option<u32>,
    ) -> ApiFuture<'_, Vec<Child>> {
        let genre = genre.map(String::from);
        Box::pin(async move {
            let mut params = vec![("size", size.to_string())];
            if let Some(g) = genre {
                params.push(("genre", g));
            }
            if let Some(y) = from_year {
                params.push(("fromYear", y.to_string()));
            }
            if let Some(y) = to_year {
                params.push(("toYear", y.to_string()));
            }
            let r = self.call("getRandomSongs", &params).await?;
            Ok(r.random_songs.map(|s| s.song).unwrap_or_default())
        })
    }

    fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> ApiFuture<'_, Vec<Child>> {
        let genre = genre.to_string();
        Box::pin(async move {
            let params = [
                ("genre", genre),
                ("count", count.to_string()),
                ("offset", offset.to_string()),
            ];
            let r = self.call("getSongsByGenre", &params).await?;
            Ok(r.songs_by_genre.map(|s| s.song).unwrap_or_default())
        })
    }

    fn genres(&self) -> ApiFuture<'_, Vec<GenreBody>> {
        Box::pin(async move {
            Ok(self
                .call("getGenres", &[])
                .await?
                .genres
                .map(|g| g.genre)
                .unwrap_or_default())
        })
    }

    fn starred2(&self) -> ApiFuture<'_, Starred2> {
        Box::pin(async move {
            Ok(self
                .call("getStarred2", &[])
                .await?
                .starred2
                .unwrap_or_default())
        })
    }

    fn playlists(&self) -> ApiFuture<'_, Vec<PlaylistBody>> {
        Box::pin(async move {
            Ok(self
                .call("getPlaylists", &[])
                .await?
                .playlists
                .map(|p| p.playlist)
                .unwrap_or_default())
        })
    }

    fn playlist(&self, id: &str) -> ApiFuture<'_, PlaylistWithSongs> {
        let id = id.to_string();
        Box::pin(async move {
            self.call("getPlaylist", &[("id", id)])
                .await?
                .playlist
                .ok_or_else(|| missing("playlist"))
        })
    }

    fn create_playlist(&self, name: &str, song_ids: &[String]) -> ApiFuture<'_, PlaylistWithSongs> {
        let mut params: Vec<(&str, String)> = vec![("name", name.to_string())];
        params.extend(song_ids.iter().map(|s| ("songId", s.clone())));
        Box::pin(async move {
            self.call("createPlaylist", &params)
                .await?
                .playlist
                .ok_or_else(|| missing("playlist"))
        })
    }

    fn replace_playlist(
        &self,
        playlist_id: &str,
        song_ids: &[String],
    ) -> ApiFuture<'_, PlaylistWithSongs> {
        let mut params: Vec<(&str, String)> = vec![("playlistId", playlist_id.to_string())];
        params.extend(song_ids.iter().map(|s| ("songId", s.clone())));
        Box::pin(async move {
            self.call("createPlaylist", &params)
                .await?
                .playlist
                .ok_or_else(|| missing("playlist"))
        })
    }

    fn update_playlist(&self, id: &str, update: PlaylistUpdate) -> ApiFuture<'_, ()> {
        let mut params: Vec<(&str, String)> = vec![("playlistId", id.to_string())];
        if let Some(n) = update.name {
            params.push(("name", n));
        }
        if let Some(c) = update.comment {
            params.push(("comment", c));
        }
        if let Some(p) = update.public {
            params.push(("public", p.to_string()));
        }
        params.extend(
            update
                .song_ids_to_add
                .into_iter()
                .map(|s| ("songIdToAdd", s)),
        );
        params.extend(
            update
                .song_indices_to_remove
                .into_iter()
                .map(|i| ("songIndexToRemove", i.to_string())),
        );
        Box::pin(async move { self.call("updatePlaylist", &params).await.map(|_| ()) })
    }

    fn delete_playlist(&self, id: &str) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move { self.call("deletePlaylist", &[("id", id)]).await.map(|_| ()) })
    }

    fn search3(&self, query: &str, page: Search3Page) -> ApiFuture<'_, SearchResult3> {
        let query = query.to_string();
        Box::pin(async move {
            let params = [
                ("query", query),
                ("artistCount", page.artist_count.to_string()),
                ("artistOffset", page.artist_offset.to_string()),
                ("albumCount", page.album_count.to_string()),
                ("albumOffset", page.album_offset.to_string()),
                ("songCount", page.song_count.to_string()),
                ("songOffset", page.song_offset.to_string()),
            ];
            Ok(self
                .call("search3", &params)
                .await?
                .search_result3
                .unwrap_or_default())
        })
    }

    fn lyrics_by_song_id(&self, id: &str) -> ApiFuture<'_, Vec<StructuredLyrics>> {
        let id = id.to_string();
        Box::pin(async move {
            let r = self.call("getLyricsBySongId", &[("id", id)]).await?;
            Ok(r.lyrics_list
                .map(|l| l.structured_lyrics)
                .unwrap_or_default())
        })
    }

    fn similar_songs2(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<Child>> {
        let id = id.to_string();
        Box::pin(async move {
            let r = self
                .call(
                    "getSimilarSongs2",
                    &[("id", id), ("count", count.to_string())],
                )
                .await?;
            Ok(r.similar_songs2.map(|s| s.song).unwrap_or_default())
        })
    }

    fn top_songs(&self, artist: &str, count: u32) -> ApiFuture<'_, Vec<Child>> {
        let artist = artist.to_string();
        Box::pin(async move {
            let r = self
                .call(
                    "getTopSongs",
                    &[("artist", artist), ("count", count.to_string())],
                )
                .await?;
            Ok(r.top_songs.map(|s| s.song).unwrap_or_default())
        })
    }

    fn artist_info2(
        &self,
        id: &str,
        count: u32,
        include_not_present: bool,
    ) -> ApiFuture<'_, ArtistInfo2> {
        let id = id.to_string();
        Box::pin(async move {
            let params = [
                ("id", id),
                ("count", count.to_string()),
                ("includeNotPresent", include_not_present.to_string()),
            ];
            Ok(self
                .call("getArtistInfo2", &params)
                .await?
                .artist_info2
                .unwrap_or_default())
        })
    }

    fn sonic_similar_tracks(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<SonicMatch>> {
        let id = id.to_string();
        Box::pin(async move {
            if !self.caps.read().sonic_similarity {
                return Err(SubsonicError::Unsupported("sonicSimilarity".into()));
            }
            let r = self
                .call(
                    "getSonicSimilarTracks",
                    &[("id", id), ("count", count.to_string())],
                )
                .await?;
            Ok(r.sonic_match
                .or_else(|| r.sonic_similar_tracks.map(|s| s.sonic_match))
                .unwrap_or_default())
        })
    }

    fn star(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()> {
        let params = Self::params_for_stars(targets);
        Box::pin(async move { self.call("star", &params).await.map(|_| ()) })
    }

    fn unstar(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()> {
        let params = Self::params_for_stars(targets);
        Box::pin(async move { self.call("unstar", &params).await.map(|_| ()) })
    }

    fn set_rating(&self, id: &str, rating: u32) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.call(
                "setRating",
                &[("id", id), ("rating", rating.min(5).to_string())],
            )
            .await
            .map(|_| ())
        })
    }

    fn scrobble(&self, id: &str, time_ms: Option<f64>, submission: bool) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            let mut params = vec![("id", id), ("submission", submission.to_string())];
            if let Some(t) = time_ms {
                params.push(("time", format!("{}", t.round() as i64)));
            }
            self.call("scrobble", &params).await.map(|_| ())
        })
    }

    fn scan_status(&self) -> ApiFuture<'_, ScanStatus> {
        Box::pin(async move {
            Ok(self
                .call("getScanStatus", &[])
                .await?
                .scan_status
                .unwrap_or_default())
        })
    }

    fn save_play_queue(&self, save: PlayQueueSave) -> ApiFuture<'_, ()> {
        Box::pin(async move {
            let mut params: Vec<(&str, String)> =
                save.song_ids.into_iter().map(|s| ("id", s)).collect();
            if let Some(c) = save.current {
                params.push(("current", c));
            }
            if let Some(p) = save.position_ms {
                params.push(("position", p.to_string()));
            }
            self.call("savePlayQueue", &params).await.map(|_| ())
        })
    }

    fn play_queue(&self) -> ApiFuture<'_, Option<PlayQueue>> {
        Box::pin(async move { Ok(self.call("getPlayQueue", &[]).await?.play_queue) })
    }

    fn now_playing(&self) -> ApiFuture<'_, Vec<NowPlayingEntry>> {
        Box::pin(async move {
            Ok(self
                .call("getNowPlaying", &[])
                .await?
                .now_playing
                .map(|n| n.entry)
                .unwrap_or_default())
        })
    }

    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Url {
        let mut params = vec![("id", id.to_string())];
        if let Some(s) = size {
            params.push(("size", s.to_string()));
        }
        self.rest_url_or_base("getCoverArt", &params)
    }

    fn stream_url(&self, id: &str, options: &StreamOptions) -> Url {
        let mut params = vec![("id", id.to_string())];
        match &options.format {
            Some(f) => params.push(("format", f.clone())),
            None if options.max_bit_rate.is_none() => params.push(("format", "raw".to_string())),
            None => {}
        }
        if let Some(b) = options.max_bit_rate {
            params.push(("maxBitRate", b.to_string()));
        }
        if let Some(t) = options.time_offset_s {
            if self.caps.read().transcode_offset {
                params.push(("timeOffset", t.to_string()));
            }
        }
        if options.estimate_content_length {
            params.push(("estimateContentLength", "true".to_string()));
        }
        self.rest_url_or_base("stream", &params)
    }

    fn download_url(&self, id: &str) -> Url {
        self.rest_url_or_base("download", &[("id", id.to_string())])
    }

    fn download_to_file(&self, url: Url, dest: &Path) -> ApiFuture<'_, DownloadOutcome> {
        let dest = dest.to_path_buf();
        Box::pin(async move {
            let _permit = self
                .limiter
                .acquire()
                .await
                .map_err(|_| SubsonicError::Network("closed".into()))?;
            let out = self.transport.download(url, &dest, None).await?;
            match out.status {
                200..=299 => {
                    // Subsonic reports protocol errors (bad token, unknown
                    // id) as HTTP 200 + JSON; never keep that as media.
                    if let Some(e) = media_download_error(&out, &dest).await {
                        let _ = tokio::fs::remove_file(&dest).await;
                        return Err(e);
                    }
                    Ok(out)
                }
                401 | 403 => Err(SubsonicError::Auth(format!("http {}", out.status))),
                404 => Err(SubsonicError::NotFound("http 404".into())),
                s => Err(SubsonicError::Server {
                    code: s as u32,
                    message: format!("http {s}"),
                }),
            }
        })
    }

    fn native_playlist(&self, id: &str) -> ApiFuture<'_, NativePlaylist> {
        let id = id.to_string();
        Box::pin(async move { super::native::get_playlist(self, &id).await })
    }

    fn native_update_playlist(
        &self,
        id: &str,
        update: NativePlaylistUpdate,
    ) -> ApiFuture<'_, NativePlaylist> {
        let id = id.to_string();
        Box::pin(async move { super::native::update_playlist(self, &id, &update).await })
    }

    fn native_create_playlist(&self, update: NativePlaylistUpdate) -> ApiFuture<'_, String> {
        Box::pin(async move { super::native::create_playlist(self, &update).await })
    }
}
