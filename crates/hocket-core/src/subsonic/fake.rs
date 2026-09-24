//! An in-memory Navidrome stand-in implementing [`SubsonicApi`]. Holds real
//! state (songs, ratings, stars, playlists, scrobbles) so sync, outbox,
//! downloads and the simulation harness can assert on server-side effects.
//! Failure injection: [`FakeServer::fail_next`] / [`FakeServer::set_offline`].

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use url::Url;

use crate::api::ServerCapabilities;

use super::client::{AlbumListType, Search3Page, StreamOptions};
use super::native::NativePlaylistUpdate;
use super::transport::DownloadOutcome;
use super::types::*;
use super::{
    ApiFuture, PlayQueueSave, PlaylistUpdate, StarTarget, SubsonicApi, SubsonicError,
    SubsonicResult,
};

#[derive(Debug, Clone, PartialEq)]
pub struct ScrobbleRecord {
    pub id: String,
    pub time_ms: Option<f64>,
    pub submission: bool,
}

#[derive(Default)]
pub struct FakeState {
    pub songs: BTreeMap<String, Child>,
    pub albums: BTreeMap<String, AlbumId3>,
    pub artists: BTreeMap<String, ArtistId3>,
    pub genres: Vec<GenreBody>,
    pub playlists: BTreeMap<String, PlaylistBody>,
    pub playlist_songs: BTreeMap<String, Vec<String>>,
    pub native_rules: BTreeMap<String, serde_json::Value>,
    pub lyrics: HashMap<String, Vec<StructuredLyrics>>,
    pub similar: HashMap<String, Vec<Child>>,
    pub sonic: HashMap<String, Vec<SonicMatch>>,
    pub scrobbles: Vec<ScrobbleRecord>,
    pub play_queue: Option<PlayQueue>,
    pub scan_status: ScanStatus,
    pub version: String,
    /// Bytes served for any media download, keyed by song id.
    pub media: HashMap<String, Vec<u8>>,
    pub calls: Vec<String>,
    queued_failures: VecDeque<SubsonicError>,
    offline: bool,
    next_id: u32,
}

/// The fake server. Cheap to clone (shared state).
#[derive(Clone)]
pub struct FakeServer {
    server_id: String,
    username: String,
    caps: Arc<Mutex<ServerCapabilities>>,
    pub state: Arc<Mutex<FakeState>>,
}

impl FakeServer {
    pub fn new(server_id: &str, username: &str) -> Self {
        let caps = ServerCapabilities {
            server_version: Some("0.63.1 (fake)".into()),
            open_subsonic: true,
            extensions: vec![
                "transcodeOffset".into(),
                "formPost".into(),
                "songLyrics".into(),
                "sonicSimilarity".into(),
            ],
            transcode_offset: true,
            form_post: true,
            song_lyrics: true,
            sonic_similarity: true,
            api_key_authentication: false,
            transcoding_extension: false,
            native_api: true,
            meets_floor: true,
        };
        let state = FakeState {
            version: "0.63.1 (fake)".into(),
            ..Default::default()
        };
        FakeServer {
            server_id: server_id.into(),
            username: username.into(),
            caps: Arc::new(Mutex::new(caps)),
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn set_capabilities(&self, caps: ServerCapabilities) {
        *self.caps.lock() = caps;
    }

    /// Add a song (and its album/artist rows if absent).
    pub fn add_song(&self, c: Child) -> &Self {
        let mut st = self.state.lock();
        if let (Some(aid), Some(name)) = (&c.album_id, &c.album) {
            st.albums.entry(aid.clone()).or_insert_with(|| AlbumId3 {
                id: aid.clone(),
                name: name.clone(),
                artist: c.artist.clone(),
                artist_id: c.artist_id.clone(),
                created: c.created.clone(),
                year: c.year,
                genre: c.genre.clone(),
                ..Default::default()
            });
        }
        if let (Some(arid), Some(name)) = (&c.artist_id, &c.artist) {
            st.artists.entry(arid.clone()).or_insert_with(|| ArtistId3 {
                id: arid.clone(),
                name: name.clone(),
                ..Default::default()
            });
        }
        st.songs.insert(c.id.clone(), c);
        self
    }

    /// Convenience: a minimal song.
    pub fn song(id: &str, title: &str, album_id: &str, artist_id: &str, duration_s: f64) -> Child {
        Child {
            id: id.into(),
            title: title.into(),
            album: Some(format!("Album {album_id}")),
            album_id: Some(album_id.into()),
            artist: Some(format!("Artist {artist_id}")),
            artist_id: Some(artist_id.into()),
            duration: Some(duration_s),
            suffix: Some("flac".into()),
            content_type: Some("audio/flac".into()),
            size: Some(1000.0),
            created: Some("2024-01-01T00:00:00Z".into()),
            cover_art: Some(format!("al-{album_id}")),
            ..Default::default()
        }
    }

    pub fn add_playlist(
        &self,
        id: &str,
        name: &str,
        owner: &str,
        song_ids: &[&str],
        readonly: bool,
    ) -> &Self {
        let mut st = self.state.lock();
        st.playlists.insert(
            id.into(),
            PlaylistBody {
                id: id.into(),
                name: name.into(),
                owner: Some(owner.into()),
                song_count: song_ids.len() as u32,
                readonly: Some(readonly),
                changed: Some("2024-01-01T00:00:00Z".into()),
                created: Some("2024-01-01T00:00:00Z".into()),
                ..Default::default()
            },
        );
        st.playlist_songs
            .insert(id.into(), song_ids.iter().map(|s| s.to_string()).collect());
        self
    }

    pub fn set_media(&self, song_id: &str, bytes: Vec<u8>) {
        self.state.lock().media.insert(song_id.into(), bytes);
    }

    /// Serves a recorded `getLyricsBySongId` answer (the full
    /// `subsonic-response` envelope, e.g. a `fixtures/lyrics_*.json`) for a
    /// song. Like a real OpenSubsonic server asked with `enhanced=true`, the
    /// fake hands back every `agents`/`cueLine` in the document, so a
    /// syllable-tier fixture reaches the actor as syllable lyrics.
    pub fn set_lyrics_json(&self, song_id: &str, envelope: &str) -> SubsonicResult<()> {
        let entries = super::client::parse_envelope(envelope.as_bytes())?
            .lyrics_list
            .map(|l| l.structured_lyrics)
            .unwrap_or_default();
        self.state.lock().lyrics.insert(song_id.into(), entries);
        Ok(())
    }

    /// The next `n` calls fail with `err`.
    pub fn fail_next(&self, err: SubsonicError, n: usize) {
        let mut st = self.state.lock();
        for _ in 0..n {
            st.queued_failures.push_back(err.clone());
        }
    }

    pub fn set_offline(&self, offline: bool) {
        self.state.lock().offline = offline;
    }

    pub fn calls(&self) -> Vec<String> {
        self.state.lock().calls.clone()
    }

    pub fn calls_to(&self, endpoint: &str) -> usize {
        self.state
            .lock()
            .calls
            .iter()
            .filter(|c| c.as_str() == endpoint)
            .count()
    }

    pub fn rating_of(&self, id: &str) -> u32 {
        self.state
            .lock()
            .songs
            .get(id)
            .and_then(|s| s.user_rating)
            .unwrap_or(0)
    }

    pub fn starred(&self, id: &str) -> bool {
        self.state
            .lock()
            .songs
            .get(id)
            .and_then(|s| s.starred.clone())
            .is_some()
    }

    pub fn playlist_song_ids(&self, id: &str) -> Vec<String> {
        self.state
            .lock()
            .playlist_songs
            .get(id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn scrobbles(&self) -> Vec<ScrobbleRecord> {
        self.state.lock().scrobbles.clone()
    }

    fn gate(&self, endpoint: &str) -> SubsonicResult<()> {
        let mut st = self.state.lock();
        st.calls.push(endpoint.to_string());
        if st.offline {
            return Err(SubsonicError::Network("fake offline".into()));
        }
        if let Some(e) = st.queued_failures.pop_front() {
            return Err(e);
        }
        Ok(())
    }

    fn playlist_with_songs(st: &FakeState, id: &str) -> SubsonicResult<PlaylistWithSongs> {
        let mut p = st
            .playlists
            .get(id)
            .cloned()
            .ok_or_else(|| SubsonicError::NotFound(format!("playlist {id}")))?;
        let ids = st.playlist_songs.get(id).cloned().unwrap_or_default();
        p.song_count = ids.len() as u32;
        let entry: Vec<Child> = ids
            .iter()
            .filter_map(|s| st.songs.get(s).cloned())
            .collect();
        Ok(PlaylistWithSongs { playlist: p, entry })
    }

    fn bump_changed(st: &mut FakeState, id: &str) {
        if let Some(p) = st.playlists.get_mut(id) {
            p.changed = Some(format!("2024-01-01T00:00:{:02}Z", st.next_id % 60));
            st.next_id += 1;
        }
    }
}

fn url_for(server_id: &str, endpoint: &str, id: &str) -> Url {
    Url::parse(&format!("https://{server_id}.fake/rest/{endpoint}?id={id}")).expect("static url")
}

impl SubsonicApi for FakeServer {
    fn server_id(&self) -> &str {
        &self.server_id
    }
    fn username(&self) -> Option<String> {
        Some(self.username.clone())
    }
    fn capabilities(&self) -> ServerCapabilities {
        self.caps.lock().clone()
    }

    fn ping(&self) -> ApiFuture<'_, SubsonicResponse> {
        Box::pin(async move {
            self.gate("ping")?;
            Ok(SubsonicResponse {
                status: "ok".into(),
                version: "1.16.1".into(),
                server_type: Some("navidrome".into()),
                server_version: Some(self.state.lock().version.clone()),
                open_subsonic: true,
                ..Default::default()
            })
        })
    }

    fn open_subsonic_extensions(&self) -> ApiFuture<'_, Vec<OpenSubsonicExtension>> {
        Box::pin(async move {
            self.gate("getOpenSubsonicExtensions")?;
            Ok(self
                .caps
                .lock()
                .extensions
                .iter()
                .map(|n| OpenSubsonicExtension {
                    name: n.clone(),
                    versions: vec![1],
                })
                .collect())
        })
    }

    fn music_folders(&self) -> ApiFuture<'_, Vec<MusicFolder>> {
        Box::pin(async move {
            self.gate("getMusicFolders")?;
            Ok(vec![MusicFolder {
                id: "1".into(),
                name: Some("Music".into()),
            }])
        })
    }

    fn artists(&self) -> ApiFuture<'_, ArtistsIndex> {
        Box::pin(async move {
            self.gate("getArtists")?;
            let st = self.state.lock();
            let mut by_letter: BTreeMap<String, Vec<ArtistId3>> = BTreeMap::new();
            for a in st.artists.values() {
                let letter = a
                    .name
                    .chars()
                    .next()
                    .map(|c| c.to_ascii_uppercase().to_string())
                    .unwrap_or_else(|| "#".into());
                by_letter.entry(letter).or_default().push(a.clone());
            }
            Ok(ArtistsIndex {
                ignored_articles: None,
                index: by_letter
                    .into_iter()
                    .map(|(name, artist)| IndexEntry { name, artist })
                    .collect(),
            })
        })
    }

    fn artist(&self, id: &str) -> ApiFuture<'_, ArtistWithAlbums> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getArtist")?;
            let st = self.state.lock();
            let artist = st
                .artists
                .get(&id)
                .cloned()
                .ok_or_else(|| SubsonicError::NotFound(id.clone()))?;
            let album = st
                .albums
                .values()
                .filter(|a| a.artist_id.as_deref() == Some(&id))
                .cloned()
                .collect();
            Ok(ArtistWithAlbums { artist, album })
        })
    }

    fn album(&self, id: &str) -> ApiFuture<'_, AlbumWithSongs> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getAlbum")?;
            let st = self.state.lock();
            let album = st
                .albums
                .get(&id)
                .cloned()
                .ok_or_else(|| SubsonicError::NotFound(id.clone()))?;
            let song = st
                .songs
                .values()
                .filter(|s| s.album_id.as_deref() == Some(&id))
                .cloned()
                .collect();
            Ok(AlbumWithSongs { album, song })
        })
    }

    fn album_list2(
        &self,
        kind: AlbumListType,
        size: u32,
        offset: u32,
    ) -> ApiFuture<'_, Vec<AlbumId3>> {
        Box::pin(async move {
            self.gate("getAlbumList2")?;
            let st = self.state.lock();
            let mut all: Vec<AlbumId3> = st.albums.values().cloned().collect();
            match kind {
                AlbumListType::Newest => {
                    all.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)))
                }
                AlbumListType::AlphabeticalByName => all.sort_by(|a, b| a.name.cmp(&b.name)),
                AlbumListType::Starred => all.retain(|a| a.starred.is_some()),
                AlbumListType::ByGenre(g) => all.retain(|a| a.genre.as_deref() == Some(&g)),
                _ => {}
            }
            Ok(all
                .into_iter()
                .skip(offset as usize)
                .take(size as usize)
                .collect())
        })
    }

    fn song(&self, id: &str) -> ApiFuture<'_, Child> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getSong")?;
            self.state
                .lock()
                .songs
                .get(&id)
                .cloned()
                .ok_or_else(|| SubsonicError::NotFound(id.clone()))
        })
    }

    fn random_songs(
        &self,
        size: u32,
        genre: Option<&str>,
        _f: Option<u32>,
        _t: Option<u32>,
    ) -> ApiFuture<'_, Vec<Child>> {
        let genre = genre.map(String::from);
        Box::pin(async move {
            self.gate("getRandomSongs")?;
            let st = self.state.lock();
            Ok(st
                .songs
                .values()
                .filter(|s| {
                    genre
                        .as_deref()
                        .is_none_or(|g| s.genre.as_deref() == Some(g))
                })
                .take(size as usize)
                .cloned()
                .collect())
        })
    }

    fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> ApiFuture<'_, Vec<Child>> {
        let genre = genre.to_string();
        Box::pin(async move {
            self.gate("getSongsByGenre")?;
            let st = self.state.lock();
            Ok(st
                .songs
                .values()
                .filter(|s| s.genre.as_deref() == Some(&genre))
                .skip(offset as usize)
                .take(count as usize)
                .cloned()
                .collect())
        })
    }

    fn genres(&self) -> ApiFuture<'_, Vec<GenreBody>> {
        Box::pin(async move {
            self.gate("getGenres")?;
            let st = self.state.lock();
            if !st.genres.is_empty() {
                return Ok(st.genres.clone());
            }
            let mut counts: BTreeMap<String, u32> = BTreeMap::new();
            for s in st.songs.values() {
                if let Some(g) = &s.genre {
                    *counts.entry(g.clone()).or_default() += 1;
                }
            }
            Ok(counts
                .into_iter()
                .map(|(value, song_count)| GenreBody {
                    value,
                    song_count,
                    album_count: 0,
                })
                .collect())
        })
    }

    fn starred2(&self) -> ApiFuture<'_, Starred2> {
        Box::pin(async move {
            self.gate("getStarred2")?;
            let st = self.state.lock();
            Ok(Starred2 {
                artist: st
                    .artists
                    .values()
                    .filter(|a| a.starred.is_some())
                    .cloned()
                    .collect(),
                album: st
                    .albums
                    .values()
                    .filter(|a| a.starred.is_some())
                    .cloned()
                    .collect(),
                song: st
                    .songs
                    .values()
                    .filter(|s| s.starred.is_some())
                    .cloned()
                    .collect(),
            })
        })
    }

    fn playlists(&self) -> ApiFuture<'_, Vec<PlaylistBody>> {
        Box::pin(async move {
            self.gate("getPlaylists")?;
            let st = self.state.lock();
            Ok(st
                .playlists
                .values()
                .map(|p| {
                    let mut p = p.clone();
                    p.song_count = st
                        .playlist_songs
                        .get(&p.id)
                        .map(|v| v.len() as u32)
                        .unwrap_or(0);
                    p
                })
                .collect())
        })
    }

    fn playlist(&self, id: &str) -> ApiFuture<'_, PlaylistWithSongs> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getPlaylist")?;
            Self::playlist_with_songs(&self.state.lock(), &id)
        })
    }

    fn create_playlist(&self, name: &str, song_ids: &[String]) -> ApiFuture<'_, PlaylistWithSongs> {
        let name = name.to_string();
        let ids = song_ids.to_vec();
        Box::pin(async move {
            self.gate("createPlaylist")?;
            let mut st = self.state.lock();
            st.next_id += 1;
            let id = format!("pl-new-{}", st.next_id);
            st.playlists.insert(
                id.clone(),
                PlaylistBody {
                    id: id.clone(),
                    name,
                    owner: Some(self.username.clone()),
                    created: Some("2024-01-01T00:00:00Z".into()),
                    changed: Some("2024-01-01T00:00:00Z".into()),
                    ..Default::default()
                },
            );
            st.playlist_songs.insert(id.clone(), ids);
            Self::playlist_with_songs(&st, &id)
        })
    }

    fn replace_playlist(
        &self,
        playlist_id: &str,
        song_ids: &[String],
    ) -> ApiFuture<'_, PlaylistWithSongs> {
        let id = playlist_id.to_string();
        let ids = song_ids.to_vec();
        Box::pin(async move {
            self.gate("createPlaylist")?;
            let mut st = self.state.lock();
            if !st.playlists.contains_key(&id) {
                return Err(SubsonicError::NotFound(id));
            }
            st.playlist_songs.insert(id.clone(), ids);
            Self::bump_changed(&mut st, &id);
            Self::playlist_with_songs(&st, &id)
        })
    }

    fn update_playlist(&self, id: &str, update: PlaylistUpdate) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("updatePlaylist")?;
            let mut st = self.state.lock();
            if !st.playlists.contains_key(&id) {
                return Err(SubsonicError::NotFound(id));
            }
            if st.playlists.get(&id).and_then(|p| p.readonly) == Some(true)
                && (!update.song_ids_to_add.is_empty() || !update.song_indices_to_remove.is_empty())
            {
                return Err(SubsonicError::Forbidden(
                    "smart playlist is read-only".into(),
                ));
            }
            if let Some(p) = st.playlists.get_mut(&id) {
                if let Some(n) = update.name {
                    p.name = n;
                }
                if let Some(c) = update.comment {
                    p.comment = Some(c);
                }
                if let Some(pb) = update.public {
                    p.public = pb;
                }
            }
            let songs = st.playlist_songs.entry(id.clone()).or_default();
            // Subsonic semantics: removals are indices into the *current* list, applied before adds.
            let mut remove = update.song_indices_to_remove.clone();
            remove.sort_unstable_by(|a, b| b.cmp(a));
            remove.dedup();
            for i in remove {
                if (i as usize) < songs.len() {
                    songs.remove(i as usize);
                }
            }
            songs.extend(update.song_ids_to_add);
            Self::bump_changed(&mut st, &id);
            Ok(())
        })
    }

    fn delete_playlist(&self, id: &str) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("deletePlaylist")?;
            let mut st = self.state.lock();
            st.playlists
                .remove(&id)
                .ok_or_else(|| SubsonicError::NotFound(id.clone()))?;
            st.playlist_songs.remove(&id);
            Ok(())
        })
    }

    fn search3(&self, query: &str, page: Search3Page) -> ApiFuture<'_, SearchResult3> {
        let query = query.to_lowercase();
        Box::pin(async move {
            self.gate("search3")?;
            let st = self.state.lock();
            let m = |s: &str| query.is_empty() || s.to_lowercase().contains(&query);
            Ok(SearchResult3 {
                artist: st
                    .artists
                    .values()
                    .filter(|a| m(&a.name))
                    .skip(page.artist_offset as usize)
                    .take(page.artist_count as usize)
                    .cloned()
                    .collect(),
                album: st
                    .albums
                    .values()
                    .filter(|a| m(&a.name))
                    .skip(page.album_offset as usize)
                    .take(page.album_count as usize)
                    .cloned()
                    .collect(),
                song: st
                    .songs
                    .values()
                    .filter(|s| m(&s.title))
                    .skip(page.song_offset as usize)
                    .take(page.song_count as usize)
                    .cloned()
                    .collect(),
            })
        })
    }

    fn lyrics_by_song_id(&self, id: &str) -> ApiFuture<'_, Vec<StructuredLyrics>> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getLyricsBySongId")?;
            Ok(self
                .state
                .lock()
                .lyrics
                .get(&id)
                .cloned()
                .unwrap_or_default())
        })
    }

    fn similar_songs2(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<Child>> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getSimilarSongs2")?;
            Ok(self
                .state
                .lock()
                .similar
                .get(&id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(count as usize)
                .collect())
        })
    }

    fn top_songs(&self, artist: &str, count: u32) -> ApiFuture<'_, Vec<Child>> {
        let artist = artist.to_string();
        Box::pin(async move {
            self.gate("getTopSongs")?;
            let st = self.state.lock();
            Ok(st
                .songs
                .values()
                .filter(|s| s.artist.as_deref() == Some(&artist))
                .take(count as usize)
                .cloned()
                .collect())
        })
    }

    fn artist_info2(&self, id: &str, _count: u32, _inp: bool) -> ApiFuture<'_, ArtistInfo2> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("getArtistInfo2")?;
            let st = self.state.lock();
            Ok(ArtistInfo2 {
                biography: st.artists.get(&id).map(|a| format!("About {}", a.name)),
                similar_artist: st
                    .artists
                    .values()
                    .filter(|a| a.id != id)
                    .cloned()
                    .collect(),
                ..Default::default()
            })
        })
    }

    fn sonic_similar_tracks(&self, id: &str, count: u32) -> ApiFuture<'_, Vec<SonicMatch>> {
        let id = id.to_string();
        Box::pin(async move {
            if !self.caps.lock().sonic_similarity {
                return Err(SubsonicError::Unsupported("sonicSimilarity".into()));
            }
            self.gate("getSonicSimilarTracks")?;
            Ok(self
                .state
                .lock()
                .sonic
                .get(&id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .take(count as usize)
                .collect())
        })
    }

    fn star(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()> {
        let targets = targets.to_vec();
        Box::pin(async move {
            self.gate("star")?;
            let mut st = self.state.lock();
            for t in targets {
                match t {
                    StarTarget::Song(id) => {
                        if let Some(s) = st.songs.get_mut(&id) {
                            s.starred = Some("2024-01-01T00:00:00Z".into());
                        }
                    }
                    StarTarget::Album(id) => {
                        if let Some(a) = st.albums.get_mut(&id) {
                            a.starred = Some("2024-01-01T00:00:00Z".into());
                        }
                    }
                    StarTarget::Artist(id) => {
                        if let Some(a) = st.artists.get_mut(&id) {
                            a.starred = Some("2024-01-01T00:00:00Z".into());
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn unstar(&self, targets: &[StarTarget]) -> ApiFuture<'_, ()> {
        let targets = targets.to_vec();
        Box::pin(async move {
            self.gate("unstar")?;
            let mut st = self.state.lock();
            for t in targets {
                match t {
                    StarTarget::Song(id) => {
                        if let Some(s) = st.songs.get_mut(&id) {
                            s.starred = None;
                        }
                    }
                    StarTarget::Album(id) => {
                        if let Some(a) = st.albums.get_mut(&id) {
                            a.starred = None;
                        }
                    }
                    StarTarget::Artist(id) => {
                        if let Some(a) = st.artists.get_mut(&id) {
                            a.starred = None;
                        }
                    }
                }
            }
            Ok(())
        })
    }

    fn set_rating(&self, id: &str, rating: u32) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("setRating")?;
            let mut st = self.state.lock();
            if let Some(s) = st.songs.get_mut(&id) {
                s.user_rating = Some(rating);
                return Ok(());
            }
            if let Some(a) = st.albums.get_mut(&id) {
                a.user_rating = Some(rating);
                return Ok(());
            }
            Err(SubsonicError::NotFound(id))
        })
    }

    fn scrobble(&self, id: &str, time_ms: Option<f64>, submission: bool) -> ApiFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            self.gate("scrobble")?;
            let mut st = self.state.lock();
            if submission {
                if let Some(s) = st.songs.get_mut(&id) {
                    s.play_count = Some(s.play_count.unwrap_or(0) + 1);
                }
            }
            st.scrobbles.push(ScrobbleRecord {
                id,
                time_ms,
                submission,
            });
            Ok(())
        })
    }

    fn scan_status(&self) -> ApiFuture<'_, ScanStatus> {
        Box::pin(async move {
            self.gate("getScanStatus")?;
            let st = self.state.lock();
            let mut s = st.scan_status.clone();
            if s.count.is_none() {
                s.count = Some(st.songs.len() as f64);
            }
            Ok(s)
        })
    }

    fn save_play_queue(&self, save: PlayQueueSave) -> ApiFuture<'_, ()> {
        Box::pin(async move {
            self.gate("savePlayQueue")?;
            let mut st = self.state.lock();
            let entry = save
                .song_ids
                .iter()
                .filter_map(|s| st.songs.get(s).cloned())
                .collect();
            st.play_queue = Some(PlayQueue {
                entry,
                current: save.current,
                position: save.position_ms.map(f64::from),
                username: Some(self.username.clone()),
                ..Default::default()
            });
            Ok(())
        })
    }

    fn play_queue(&self) -> ApiFuture<'_, Option<PlayQueue>> {
        Box::pin(async move {
            self.gate("getPlayQueue")?;
            Ok(self.state.lock().play_queue.clone())
        })
    }

    fn now_playing(&self) -> ApiFuture<'_, Vec<NowPlayingEntry>> {
        Box::pin(async move {
            self.gate("getNowPlaying")?;
            Ok(vec![])
        })
    }

    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Url {
        let mut u = url_for(&self.server_id, "getCoverArt", id);
        if let Some(s) = size {
            u.query_pairs_mut().append_pair("size", &s.to_string());
        }
        u
    }

    fn stream_url(&self, id: &str, options: &StreamOptions) -> Url {
        let mut u = url_for(&self.server_id, "stream", id);
        if let Some(f) = &options.format {
            u.query_pairs_mut().append_pair("format", f);
        }
        if let Some(b) = options.max_bit_rate {
            u.query_pairs_mut()
                .append_pair("maxBitRate", &b.to_string());
        }
        u
    }

    fn download_url(&self, id: &str) -> Url {
        url_for(&self.server_id, "download", id)
    }

    fn download_to_file(&self, url: Url, dest: &Path) -> ApiFuture<'_, DownloadOutcome> {
        let dest = dest.to_path_buf();
        Box::pin(async move {
            self.gate("download")?;
            let id = url
                .query_pairs()
                .find(|(k, _)| k == "id")
                .map(|(_, v)| v.into_owned())
                .unwrap_or_default();
            let (bytes, content_type) = {
                let st = self.state.lock();
                let bytes = st
                    .media
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| format!("audio:{id}").into_bytes());
                let ct = st.songs.get(&id).and_then(|s| s.content_type.clone());
                (bytes, ct)
            };
            if let Some(parent) = dest.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| SubsonicError::Io(e.to_string()))?;
            }
            tokio::fs::write(&dest, &bytes)
                .await
                .map_err(|e| SubsonicError::Io(e.to_string()))?;
            Ok(DownloadOutcome {
                status: 200,
                content_type,
                bytes: bytes.len() as u64,
            })
        })
    }

    fn native_playlist(&self, id: &str) -> ApiFuture<'_, NativePlaylist> {
        let id = id.to_string();
        Box::pin(async move {
            if !self.caps.lock().native_api {
                return Err(SubsonicError::Unsupported("native api".into()));
            }
            self.gate("native:getPlaylist")?;
            let st = self.state.lock();
            let p = st
                .playlists
                .get(&id)
                .ok_or_else(|| SubsonicError::NotFound(id.clone()))?;
            Ok(NativePlaylist {
                id: p.id.clone(),
                name: p.name.clone(),
                comment: p.comment.clone().unwrap_or_default(),
                song_count: st
                    .playlist_songs
                    .get(&id)
                    .map(|v| v.len() as u32)
                    .unwrap_or(0),
                owner_name: p.owner.clone(),
                public: p.public,
                rules: st.native_rules.get(&id).cloned(),
                ..Default::default()
            })
        })
    }

    fn native_update_playlist(
        &self,
        id: &str,
        update: NativePlaylistUpdate,
    ) -> ApiFuture<'_, NativePlaylist> {
        let id = id.to_string();
        Box::pin(async move {
            if !self.caps.lock().native_api {
                return Err(SubsonicError::Unsupported("native api".into()));
            }
            self.gate("native:updatePlaylist")?;
            {
                let mut st = self.state.lock();
                let p = st
                    .playlists
                    .get_mut(&id)
                    .ok_or_else(|| SubsonicError::NotFound(id.clone()))?;
                if let Some(n) = update.name {
                    p.name = n;
                }
                if let Some(c) = update.comment {
                    p.comment = Some(c);
                }
                if let Some(pb) = update.public {
                    p.public = pb;
                }
                if let Some(r) = update.rules {
                    p.readonly = Some(true);
                    st.native_rules.insert(id.clone(), r);
                }
            }
            self.native_playlist(&id).await
        })
    }

    fn native_create_playlist(&self, update: NativePlaylistUpdate) -> ApiFuture<'_, String> {
        Box::pin(async move {
            if !self.caps.lock().native_api {
                return Err(SubsonicError::Unsupported("native api".into()));
            }
            self.gate("native:createPlaylist")?;
            let mut st = self.state.lock();
            st.next_id += 1;
            let id = format!("pl-smart-{}", st.next_id);
            st.playlists.insert(
                id.clone(),
                PlaylistBody {
                    id: id.clone(),
                    name: update.name.unwrap_or_default(),
                    comment: update.comment,
                    public: update.public.unwrap_or(false),
                    owner: Some(self.username.clone()),
                    readonly: Some(update.rules.is_some()),
                    ..Default::default()
                },
            );
            if let Some(r) = update.rules {
                st.native_rules.insert(id.clone(), r);
            }
            st.playlist_songs.insert(id.clone(), vec![]);
            Ok(id)
        })
    }
}
