//! YouTube Music behind the [`Provider`] trait.
//!
//! Track ids are video ids, valid in the one global [`SOURCE`] whatever the
//! account (anonymous or signed in), so pins, cache entries and queue items
//! survive signing in. The account id is the channel id, or [`ANON`].

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use url::Url;

use super::client::{EditAction, InnerTube, SearchFilter};
use super::http::{HttpMethod, HttpRequest};
use super::parse::{
    self, collect_items, parse_album, parse_artist, parse_lyrics, parse_next, parse_playlist,
    parse_search, parse_shelves, Item, YtAlbum, YtArtist, YtPlaylist, YtSong,
};
use super::player::{ChosenStream, JsHost, StreamChooser, EXPIRY_MARGIN_MS};
use crate::api::{self, AccountKind, ArtistRef, EpochMs, ProviderCapabilities};
use crate::provider::{
    HomeShelf, LibraryPage, LibraryTarget, PlaylistContents, PlaylistEntry, Provider,
    ProviderError, ProviderFuture, ProviderResult, ProviderWrite, ResolvedStream, StreamRequest,
    WriteOutcome,
};
use crate::subsonic::transport::DownloadOutcome;
use crate::subsonic::types::{LyricLineBody, StructuredLyrics};
use crate::util::Clock;

/// The one source every YouTube Music track reference names.
pub const SOURCE: &str = "ytm";
/// The anonymous account's id.
pub const ANON: &str = "ytm:anon";
/// The largest range asked of googlevideo in one request (Phase 0: bounded
/// ranges ≤ 10 MiB avoid the throttle on open-ended requests).
pub const MAX_RANGE: u64 = 8 * 1024 * 1024;
/// The first range of a stream: small, so playback starts quickly from a cold edge.
pub const FIRST_RANGE: u64 = 1024 * 1024;
/// How long a live catalogue answer (search, album and artist pages) is reused.
pub const CATALOGUE_TTL_MS: f64 = 30.0 * 60_000.0;
/// Pages of one library list fetched at most per sync (100 entries each).
const MAX_PAGES: usize = 50;

/// What a whole-library walk found.
#[derive(Debug, Clone, Default)]
struct LibrarySnapshot {
    tracks: Vec<api::Track>,
    albums: Vec<api::Album>,
    artists: Vec<api::Artist>,
}

pub struct YtmProvider {
    account_id: String,
    name: RwLock<String>,
    it: Arc<InnerTube>,
    chooser: StreamChooser,
    js: RwLock<Option<Arc<dyn JsHost>>>,
    premium: AtomicBool,
    clock: Arc<dyn Clock>,
    /// Resolved streams by video id, reused until shortly before they expire.
    streams: Mutex<HashMap<String, ChosenStream>>,
    /// Raw catalogue answers by request, with when they were fetched.
    catalogue: Mutex<HashMap<String, (f64, Value)>>,
    snapshot: Mutex<Option<LibrarySnapshot>>,
}

impl YtmProvider {
    pub fn new(
        account_id: impl Into<String>,
        it: Arc<InnerTube>,
        clock: Arc<dyn Clock>,
    ) -> YtmProvider {
        YtmProvider {
            account_id: account_id.into(),
            name: RwLock::new("YouTube Music".into()),
            it,
            chooser: StreamChooser::new(clock.clone()),
            js: RwLock::new(None),
            premium: AtomicBool::new(false),
            clock,
            streams: Mutex::default(),
            catalogue: Mutex::default(),
            snapshot: Mutex::default(),
        }
    }

    pub fn innertube(&self) -> &Arc<InnerTube> {
        &self.it
    }

    pub fn signed_in(&self) -> bool {
        self.it.signed_in()
    }

    pub fn set_premium(&self, premium: bool) {
        self.premium.store(premium, Ordering::Relaxed);
    }

    pub fn premium(&self) -> bool {
        self.premium.load(Ordering::Relaxed)
    }

    pub fn set_name(&self, name: &str) {
        *self.name.write() = name.to_string();
    }

    pub fn set_js_host(&self, js: Option<Arc<dyn JsHost>>) {
        *self.js.write() = js;
    }

    pub fn chooser(&self) -> &StreamChooser {
        &self.chooser
    }

    /// A catalogue answer, from the TTL cache or the network.
    async fn cached(
        &self,
        key: String,
        fetch: impl std::future::Future<Output = ProviderResult<Value>>,
    ) -> ProviderResult<Value> {
        let now = self.clock.now_ms();
        if let Some((at, v)) = self.catalogue.lock().get(&key) {
            if now - at < CATALOGUE_TTL_MS {
                return Ok(v.clone());
            }
        }
        let v = fetch.await?;
        let mut c = self.catalogue.lock();
        if c.len() > 256 {
            c.retain(|_, (at, _)| now - *at < CATALOGUE_TTL_MS);
        }
        c.insert(key, (now, v.clone()));
        Ok(v)
    }

    async fn browse_cached(&self, id: &str) -> ProviderResult<Value> {
        self.cached(format!("browse:{id}"), self.it.browse(id, None))
            .await
    }

    /// A stream for `id`: the cached one while it is fresh, else the chain.
    async fn stream(&self, id: &str) -> ProviderResult<ChosenStream> {
        let now = self.clock.now_ms();
        if let Some(s) = self.streams.lock().get(id) {
            if s.expires_at - EXPIRY_MARGIN_MS > now {
                return Ok(s.clone());
            }
        }
        let js = self.js.read().clone();
        let s = self
            .chooser
            .choose(
                &self.it,
                js.as_ref(),
                self.premium() && self.signed_in(),
                id,
            )
            .await?;
        self.streams.lock().insert(id.to_string(), s.clone());
        Ok(s)
    }

    /// Every item of a list page, following its continuations.
    async fn browse_all(&self, browse_id: &str) -> ProviderResult<Vec<Item>> {
        let first = self.it.browse(browse_id, None).await?;
        let mut items = collect_items(first.get("contents").unwrap_or(&first));
        let mut next = parse::continuation(&first);
        let mut pages = 1;
        while let Some(token) = next.take() {
            if pages >= MAX_PAGES {
                break;
            }
            let v = self.it.continuation(&token).await?;
            items.extend(collect_items(&v));
            next = parse::continuation(&v);
            pages += 1;
        }
        Ok(items)
    }

    async fn playlist_live(&self, id: &str) -> ProviderResult<PlaylistContents> {
        let browse = if id.starts_with("VL") {
            id.to_string()
        } else {
            format!("VL{id}")
        };
        let first = self.it.browse(&browse, None).await?;
        let mut page = parse_playlist(&first, id);
        let mut pages = 1;
        while let Some(token) = page.continuation.take() {
            if pages >= MAX_PAGES {
                break;
            }
            let v = self.it.continuation(&token).await?;
            let more = parse_playlist(&v, id);
            page.entries.extend(more.entries);
            page.continuation = more.continuation;
            pages += 1;
        }
        let mut playlist = playlist_of(&page.playlist, true);
        let entries: Vec<PlaylistEntry> = page
            .entries
            .iter()
            .map(|s| PlaylistEntry {
                track: track_of(s),
                entry_id: s.set_video_id.clone(),
                visible: s.is_song(),
            })
            .collect();
        playlist.song_count = entries.iter().filter(|e| e.visible).count() as u32;
        playlist.duration_ms = entries
            .iter()
            .filter(|e| e.visible)
            .map(|e| e.track.duration_ms)
            .sum();
        Ok(PlaylistContents { playlist, entries })
    }

    async fn library_snapshot(&self) -> ProviderResult<LibrarySnapshot> {
        let mut snap = LibrarySnapshot::default();
        let mut seen = std::collections::HashSet::new();
        // Liked songs are the stars; library songs and uploads are the rest.
        for (browse, loved) in [
            ("VLLM", true),
            ("FEmusic_liked_videos", false),
            ("FEmusic_library_privately_owned_tracks", false),
        ] {
            let items = match self.browse_all(browse).await {
                Ok(i) => i,
                // An empty library list answers without the list.
                Err(ProviderError::NotFound(_)) => continue,
                Err(e) => return Err(e),
            };
            for s in parse::songs(&items).iter().filter(|s| s.is_song()) {
                if seen.insert(s.video_id.clone()) {
                    let mut t = track_of(s);
                    t.loved = loved;
                    snap.tracks.push(t);
                } else if loved {
                    if let Some(t) = snap.tracks.iter_mut().find(|t| t.id == s.video_id) {
                        t.loved = true;
                    }
                }
            }
        }
        for item in self
            .browse_all("FEmusic_liked_albums")
            .await
            .unwrap_or_default()
        {
            if let Item::Album(a) = item {
                snap.albums.push(album_of(&a));
            }
        }
        for item in self
            .browse_all("FEmusic_library_corpus_track_artists")
            .await
            .unwrap_or_default()
        {
            if let Item::Artist(a) = item {
                snap.artists.push(artist_of(&a));
            }
        }
        Ok(snap)
    }

    async fn history_ping(&self, id: &str) -> ProviderResult<()> {
        let v = self.it.player(super::clients::WEB_REMIX, id, None).await?;
        let info = parse::parse_player(&v);
        let Some(base) = info.playback_url else {
            return Err(ProviderError::Unsupported(
                "no playback tracking URL".into(),
            ));
        };
        let cpn: String = {
            use rand::Rng;
            const ALPHABET: &[u8] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
            let mut rng = rand::rng();
            (0..16)
                .map(|_| ALPHABET[rng.random_range(0..ALPHABET.len())] as char)
                .collect()
        };
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!(
            "{base}{sep}ver=2&c=WEB_REMIX&cver={}&cpn={cpn}&fmt=140&rtn=0&rt=0",
            super::clients::WEB_REMIX.version
        );
        self.it.ping(&url).await
    }

    /// Where an entry at visible index `i` sits among every entry.
    fn entry_at(c: &PlaylistContents, visible_index: usize) -> Option<&PlaylistEntry> {
        c.entries.iter().filter(|e| e.visible).nth(visible_index)
    }

    async fn playlist_write(&self, w: ProviderWrite) -> ProviderResult<WriteOutcome> {
        match w {
            ProviderWrite::CreatePlaylist { name, track_ids } => {
                let id = self.it.create_playlist(&name, &track_ids).await?;
                Ok(WriteOutcome::Created(id))
            }
            ProviderWrite::DeletePlaylist { id } => match self.it.delete_playlist(&id).await {
                Ok(()) | Err(ProviderError::NotFound(_)) => Ok(WriteOutcome::Done),
                Err(e) => Err(e),
            },
            ProviderWrite::RenamePlaylist { id, name } => {
                self.it
                    .edit_playlist(&id, &[EditAction::Rename { title: name }])
                    .await?;
                Ok(WriteOutcome::Done)
            }
            ProviderWrite::PlaylistAdd {
                id,
                track_ids,
                at_index,
            } => {
                let before = match at_index {
                    Some(i) => {
                        let c = self.playlist_live(&id).await?;
                        Self::entry_at(&c, i as usize).and_then(|e| e.entry_id.clone())
                    }
                    None => None,
                };
                let actions: Vec<EditAction> = track_ids
                    .iter()
                    .map(|v| EditAction::Add {
                        video_id: v.clone(),
                    })
                    .collect();
                let added = self.it.edit_playlist(&id, &actions).await?;
                // Adds append; an insert moves each new entry before the
                // entry that held the position (hidden videos skipped).
                if let Some(before) = before {
                    let moves: Vec<EditAction> = added
                        .iter()
                        .map(|(_, set)| EditAction::Move {
                            set_video_id: set.clone(),
                            before: Some(before.clone()),
                        })
                        .collect();
                    if !moves.is_empty() {
                        self.it.edit_playlist(&id, &moves).await?;
                    }
                }
                Ok(WriteOutcome::Done)
            }
            ProviderWrite::PlaylistRemove {
                id,
                track_ids,
                indices,
            } => {
                let c = self.playlist_live(&id).await?;
                let visible: Vec<&PlaylistEntry> = c.entries.iter().filter(|e| e.visible).collect();
                let mut taken: Vec<usize> = vec![];
                let mut actions = vec![];
                for (n, t) in track_ids.iter().enumerate() {
                    let hinted = indices.get(n).map(|i| *i as usize).filter(|i| {
                        visible.get(*i).is_some_and(|e| &e.track.id == t) && !taken.contains(i)
                    });
                    let found = hinted.or_else(|| {
                        visible
                            .iter()
                            .enumerate()
                            .find(|(i, e)| &e.track.id == t && !taken.contains(i))
                            .map(|(i, _)| i)
                    });
                    if let Some(i) = found {
                        taken.push(i);
                        if let Some(set) = visible[i].entry_id.clone() {
                            actions.push(EditAction::Remove {
                                video_id: t.clone(),
                                set_video_id: set,
                            });
                        }
                    }
                }
                if !actions.is_empty() {
                    self.it.edit_playlist(&id, &actions).await?;
                }
                Ok(WriteOutcome::Done)
            }
            ProviderWrite::PlaylistMove {
                id,
                track_id,
                from_index,
                to_index,
            } => {
                let c = self.playlist_live(&id).await?;
                let visible: Vec<&PlaylistEntry> = c.entries.iter().filter(|e| e.visible).collect();
                let from = if visible
                    .get(from_index as usize)
                    .is_some_and(|e| e.track.id == track_id)
                {
                    Some(from_index as usize)
                } else {
                    visible.iter().position(|e| e.track.id == track_id)
                };
                let Some(from) = from else {
                    return Ok(WriteOutcome::Done);
                };
                let Some(moving) = visible[from].entry_id.clone() else {
                    return Ok(WriteOutcome::Done);
                };
                // The order without the moving entry: it lands before
                // whatever then holds `to_index` (the end when nothing does).
                let rest: Vec<&&PlaylistEntry> = visible
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != from)
                    .map(|(_, e)| e)
                    .collect();
                let before = rest.get(to_index as usize).and_then(|e| e.entry_id.clone());
                // Moving to the end: before the hidden entry after the last
                // visible one, if any, keeps hidden entries where they were.
                let before = before.or_else(|| {
                    let last_visible = c.entries.iter().rposition(|e| {
                        e.visible && e.entry_id.as_deref() != Some(moving.as_str())
                    })?;
                    c.entries
                        .get(last_visible + 1..)?
                        .iter()
                        .find(|e| e.entry_id.as_deref() != Some(moving.as_str()))
                        .and_then(|e| e.entry_id.clone())
                });
                self.it
                    .edit_playlist(
                        &id,
                        &[EditAction::Move {
                            set_video_id: moving,
                            before,
                        }],
                    )
                    .await?;
                Ok(WriteOutcome::Done)
            }
            other => Err(ProviderError::Unsupported(format!("{other:?}"))),
        }
    }

    async fn library_write(&self, target: LibraryTarget, add: bool) -> ProviderResult<()> {
        match target {
            // A song goes into the library by liking it: the web app's
            // "Add to library" needs per-item menu tokens a reference lacks.
            LibraryTarget::Track { id } => self.it.like(&id, add).await,
            LibraryTarget::Album { id } => {
                let v = self.browse_cached(&id).await?;
                let playlist = parse_album(&v, &id)
                    .and_then(|(a, _)| a.playlist_id)
                    .ok_or_else(|| ProviderError::Protocol("album without a playlist".into()))?;
                self.it.save_playlist(&playlist, add).await
            }
            LibraryTarget::Playlist { id } => self.it.save_playlist(&id, add).await,
            LibraryTarget::Artist { id } => self.it.subscribe(&id, add).await,
        }
    }
}

// ---------------------------------------------------------------------------
// conversions
// ---------------------------------------------------------------------------

pub fn track_of(s: &YtSong) -> api::Track {
    let artists: Vec<ArtistRef> = s
        .artists
        .iter()
        .map(|a| ArtistRef {
            id: a.id.clone(),
            name: a.name.clone(),
        })
        .collect();
    let artist = (!artists.is_empty()).then(|| {
        artists
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    });
    api::Track {
        id: s.video_id.clone(),
        server_id: SOURCE.into(),
        title: s.title.clone(),
        album_id: s.album.as_ref().and_then(|a| a.id.clone()),
        album: s.album.as_ref().map(|a| a.name.clone()),
        artist_id: artists.first().and_then(|a| a.id.clone()),
        artist,
        album_artist: artists.first().map(|a| a.name.clone()),
        year: s.year,
        duration_ms: s.duration_s.unwrap_or(0) as _,
        suffix: Some("m4a".into()),
        content_type: Some("audio/mp4".into()),
        cover_art: s.thumbnail.clone(),
        explicit: s.explicit,
        artists,
        ..Default::default()
    }
}

pub fn album_of(a: &YtAlbum) -> api::Album {
    api::Album {
        id: a.browse_id.clone(),
        server_id: SOURCE.into(),
        name: a.title.clone(),
        artist_id: a.artists.first().and_then(|x| x.id.clone()),
        artist: (!a.artists.is_empty()).then(|| {
            a.artists
                .iter()
                .map(|x| x.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }),
        year: a.year,
        cover_art: a.thumbnail.clone(),
        release_types: a.kind.iter().map(|k| k.to_lowercase()).collect(),
        ..Default::default()
    }
}

pub fn artist_of(a: &YtArtist) -> api::Artist {
    api::Artist {
        id: a.browse_id.clone(),
        server_id: SOURCE.into(),
        name: a.name.clone(),
        cover_art: a.thumbnail.clone(),
        artist_image_url: a.thumbnail.clone(),
        ..Default::default()
    }
}

pub fn playlist_of(p: &YtPlaylist, mine: bool) -> api::Playlist {
    api::Playlist {
        id: p.id.clone(),
        server_id: SOURCE.into(),
        name: p.title.clone(),
        owner: p.author.clone(),
        song_count: p.count.unwrap_or(0),
        cover_art: p.thumbnail.clone(),
        is_mine: mine,
        ..Default::default()
    }
}

/// googleusercontent thumbnails take their size in the URL (`=w544-h544`).
pub fn sized_thumbnail(url: &str, size: Option<u32>) -> String {
    let Some(size) = size else {
        return url.to_string();
    };
    let googleish = url.contains("googleusercontent.com") || url.contains("ggpht.com");
    if !googleish {
        return url.to_string();
    }
    let base = match url.rfind('=') {
        Some(i) if url[i..].starts_with("=w") || url[i..].starts_with("=s") => &url[..i],
        _ => url,
    };
    format!("{base}=w{size}-h{size}-l90-rj")
}

fn to_lyrics(text: &str) -> StructuredLyrics {
    StructuredLyrics {
        lang: "und".into(),
        synced: false,
        line: text
            .lines()
            .map(|l| LyricLineBody {
                start: None,
                value: l.to_string(),
            })
            .collect(),
        ..Default::default()
    }
}

fn ytm_unsupported(what: &str) -> ProviderError {
    ProviderError::Unsupported(format!("{what} needs a signed-in YouTube Music account"))
}

impl Provider for YtmProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn kind(&self) -> AccountKind {
        AccountKind::YoutubeMusic
    }

    fn source(&self) -> &str {
        SOURCE
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let signed_in = self.signed_in();
        ProviderCapabilities {
            library_sync: signed_in,
            search: true,
            browse: true,
            stream: true,
            // Pins keep the stream (there is no original file).
            download: false,
            transcode: false,
            cover_art: true,
            star: signed_in,
            rate: false,
            playlists: signed_in,
            smart_playlists: false,
            report_play: signed_in,
            lyrics: true,
            radio: true,
            home_shelves: true,
            share: true,
            library_edit: signed_in,
        }
    }

    fn library_page(&self, offset: u32, size: u32) -> ProviderFuture<'_, LibraryPage> {
        Box::pin(async move {
            if !self.signed_in() {
                return Err(ytm_unsupported("Library sync"));
            }
            if offset == 0 || self.snapshot.lock().is_none() {
                let snap = self.library_snapshot().await?;
                *self.snapshot.lock() = Some(snap);
            }
            let guard = self.snapshot.lock();
            let snap = guard.as_ref().expect("set above");
            let from = (offset as usize).min(snap.tracks.len());
            let to = (from + size.max(1) as usize).min(snap.tracks.len());
            let first = offset == 0;
            Ok(LibraryPage {
                tracks: snap.tracks[from..to].to_vec(),
                albums: if first { snap.albums.clone() } else { vec![] },
                artists: if first { snap.artists.clone() } else { vec![] },
                next_offset: (to < snap.tracks.len()).then_some(to as u32),
            })
        })
    }

    fn search(&self, query: &str, limit: u32) -> ProviderFuture<'_, LibraryPage> {
        let query = query.to_string();
        Box::pin(async move {
            let (songs, albums, artists) = futures::join!(
                self.cached(
                    format!("search:songs:{query}"),
                    self.it.search(&query, Some(SearchFilter::Songs))
                ),
                self.cached(
                    format!("search:albums:{query}"),
                    self.it.search(&query, Some(SearchFilter::Albums))
                ),
                self.cached(
                    format!("search:artists:{query}"),
                    self.it.search(&query, Some(SearchFilter::Artists))
                ),
            );
            let songs = songs?;
            let limit = limit.max(1) as usize;
            let r = parse_search(&songs);
            Ok(LibraryPage {
                tracks: r
                    .songs
                    .iter()
                    .filter(|s| s.is_song())
                    .take(limit)
                    .map(track_of)
                    .collect(),
                albums: albums
                    .map(|v| parse_search(&v).albums)
                    .unwrap_or_default()
                    .iter()
                    .take(limit)
                    .map(album_of)
                    .collect(),
                artists: artists
                    .map(|v| parse_search(&v).artists)
                    .unwrap_or_default()
                    .iter()
                    .take(limit)
                    .map(artist_of)
                    .collect(),
                next_offset: None,
            })
        })
    }

    fn album(&self, id: &str) -> ProviderFuture<'_, (api::Album, Vec<api::Track>)> {
        let id = id.to_string();
        Box::pin(async move {
            let v = self.browse_cached(&id).await?;
            let (a, tracks) = parse_album(&v, &id)
                .ok_or_else(|| ProviderError::NotFound(format!("album {id}")))?;
            let mut album = album_of(&a);
            let tracks: Vec<api::Track> = tracks
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let mut t = track_of(s);
                    t.track_number = Some(i as u32 + 1);
                    t
                })
                .collect();
            album.song_count = tracks.len() as u32;
            album.duration_ms = tracks.iter().map(|t| t.duration_ms).sum();
            Ok((album, tracks))
        })
    }

    fn artist(&self, id: &str) -> ProviderFuture<'_, (api::Artist, Vec<api::Album>)> {
        let id = id.to_string();
        Box::pin(async move {
            let v = self.browse_cached(&id).await?;
            let (a, shelves) = parse_artist(&v, &id);
            let mut artist = artist_of(&a);
            let albums: Vec<api::Album> = shelves
                .iter()
                .flat_map(|s| s.items.iter())
                .filter_map(|i| match i {
                    Item::Album(al) => {
                        let mut al = al.clone();
                        if al.artists.is_empty() {
                            al.artists = vec![parse::Named {
                                id: Some(id.clone()),
                                name: a.name.clone(),
                            }];
                        }
                        Some(album_of(&al))
                    }
                    _ => None,
                })
                .collect();
            artist.album_count = albums.len() as u32;
            Ok((artist, albums))
        })
    }

    fn track(&self, id: &str) -> ProviderFuture<'_, api::Track> {
        let id = id.to_string();
        Box::pin(async move {
            let v = self
                .cached(format!("next:{id}"), self.it.next(&id, None))
                .await?;
            let (queue, _) = parse_next(&v);
            queue
                .iter()
                .find(|s| s.video_id == id)
                .map(track_of)
                .ok_or_else(|| ProviderError::NotFound(format!("track {id}")))
        })
    }

    fn resolve_stream(&self, id: &str, _req: &StreamRequest) -> ProviderFuture<'_, ResolvedStream> {
        let id = id.to_string();
        Box::pin(async move {
            let s = self.stream(&id).await?;
            let url = Url::parse(&s.url).map_err(|e| ProviderError::Protocol(e.to_string()))?;
            Ok(ResolvedStream {
                url,
                headers: [("User-Agent".to_string(), s.user_agent.to_string())].into(),
                expires_at: Some(s.expires_at as EpochMs),
                container: Some("m4a".into()),
                codec: Some("aac".into()),
                // YouTube turns a track `loudnessDb` above its reference down by that much.
                gain_db: s.loudness_db.map(|l| -l),
                range_capable: true,
                transcoded: false,
                max_range: Some(MAX_RANGE),
                content_length: s.content_length,
                variant: s.itag.to_string(),
            })
        })
    }

    fn stream_failed(&self, id: &str, status: u16) {
        let gone = self.streams.lock().remove(id);
        if let (Some(s), 403) = (gone, status) {
            // That client's URLs aren't served to us right now.
            tracing::info!(target: "hocket::ytm", client = ?s.client, "googlevideo refused a URL; trying the next client");
            self.chooser.mark_broken(s.client);
        }
    }

    fn download_url(&self, _id: &str) -> Option<Url> {
        None
    }

    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Option<Url> {
        if !id.starts_with("https://") {
            return None;
        }
        Url::parse(&sized_thumbnail(id, size)).ok()
    }

    fn download_to_file<'a>(
        &'a self,
        stream: &ResolvedStream,
        dest: &'a Path,
    ) -> ProviderFuture<'a, DownloadOutcome> {
        let stream = stream.clone();
        Box::pin(async move { self.download_chunks(stream, dest).await })
    }

    fn write(&self, write: ProviderWrite) -> ProviderFuture<'_, WriteOutcome> {
        Box::pin(async move {
            if !self.signed_in() {
                return Err(ytm_unsupported("Library edits"));
            }
            match write {
                ProviderWrite::Star(targets) | ProviderWrite::Unstar(targets)
                    if targets.is_empty() =>
                {
                    Ok(WriteOutcome::Done)
                }
                ProviderWrite::Star(targets) => {
                    for t in targets {
                        match t {
                            api::RatingTarget::Track { id, .. } => self.it.like(&id, true).await?,
                            api::RatingTarget::Album { id } => {
                                self.library_write(LibraryTarget::Album { id }, true)
                                    .await?
                            }
                        }
                    }
                    Ok(WriteOutcome::Done)
                }
                ProviderWrite::Unstar(targets) => {
                    for t in targets {
                        match t {
                            api::RatingTarget::Track { id, .. } => self.it.like(&id, false).await?,
                            api::RatingTarget::Album { id } => {
                                self.library_write(LibraryTarget::Album { id }, false)
                                    .await?
                            }
                        }
                    }
                    Ok(WriteOutcome::Done)
                }
                ProviderWrite::Rate { .. } => Err(ProviderError::Unsupported(
                    "YouTube Music has no ratings".into(),
                )),
                ProviderWrite::LibraryAdd(t) => {
                    self.library_write(t, true).await?;
                    Ok(WriteOutcome::Done)
                }
                ProviderWrite::LibraryRemove(t) => {
                    self.library_write(t, false).await?;
                    Ok(WriteOutcome::Done)
                }
                other => self.playlist_write(other).await,
            }
        })
    }

    fn report_play(
        &self,
        id: &str,
        _played_at: Option<EpochMs>,
        submission: bool,
    ) -> ProviderFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move {
            if !self.signed_in() {
                return Err(ytm_unsupported("History"));
            }
            // YouTube Music has no "now playing"; history is the finished play.
            if !submission {
                return Ok(());
            }
            self.history_ping(&id).await
        })
    }

    fn lyrics(&self, id: &str) -> ProviderFuture<'_, Vec<StructuredLyrics>> {
        let id = id.to_string();
        Box::pin(async move {
            let v = self
                .cached(format!("next:{id}"), self.it.next(&id, None))
                .await?;
            let (_, browse) = parse_next(&v);
            let Some(browse) = browse else {
                return Ok(vec![]);
            };
            let l = self.browse_cached(&browse).await?;
            Ok(parse_lyrics(&l)
                .map(|(text, _)| vec![to_lyrics(&text)])
                .unwrap_or_default())
        })
    }

    fn radio(&self, seed: &str, count: u32) -> ProviderFuture<'_, Vec<api::Track>> {
        let seed = seed.to_string();
        Box::pin(async move {
            let v = self.it.next(&seed, Some(&format!("RDAMVM{seed}"))).await?;
            let (queue, _) = parse_next(&v);
            Ok(queue
                .iter()
                .filter(|s| s.video_id != seed && s.is_song())
                .take(count as usize)
                .map(track_of)
                .collect())
        })
    }

    fn home_shelves(&self) -> ProviderFuture<'_, Vec<HomeShelf>> {
        Box::pin(async move {
            let v = self
                .cached("home".into(), self.it.browse("FEmusic_home", None))
                .await?;
            Ok(parse_shelves(&v)
                .into_iter()
                .filter_map(|s| {
                    let mut shelf = HomeShelf {
                        id: shelf_id(&s.title),
                        title: s.title.clone(),
                        ..Default::default()
                    };
                    for i in &s.items {
                        match i {
                            Item::Song(x) if x.is_song() => shelf.tracks.push(track_of(x)),
                            Item::Album(a) => shelf.albums.push(album_of(a)),
                            Item::Playlist(p) => shelf.playlists.push(playlist_of(p, false)),
                            _ => {}
                        }
                    }
                    let empty = shelf.tracks.is_empty()
                        && shelf.albums.is_empty()
                        && shelf.playlists.is_empty();
                    (!empty && !shelf.title.is_empty()).then_some(shelf)
                })
                .collect())
        })
    }

    fn playlists(&self) -> ProviderFuture<'_, Vec<api::Playlist>> {
        Box::pin(async move {
            if !self.signed_in() {
                return Err(ytm_unsupported("Playlists"));
            }
            let me = self.name.read().clone();
            Ok(self
                .browse_all("FEmusic_liked_playlists")
                .await?
                .into_iter()
                .filter_map(|i| match i {
                    // Liked Music is the stars, not a playlist; episodes aren't songs.
                    Item::Playlist(p) if p.id != "LM" && p.id != "SE" => {
                        let mine = p.author.as_deref().is_none_or(|a| a == me);
                        Some(playlist_of(&p, mine))
                    }
                    _ => None,
                })
                .collect())
        })
    }

    fn playlist(&self, id: &str) -> ProviderFuture<'_, PlaylistContents> {
        let id = id.to_string();
        Box::pin(async move { self.playlist_live(&id).await })
    }
}

/// `"Quick picks"` → `quick-picks`.
pub fn shelf_id(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

impl YtmProvider {
    /// A whole stream in bounded ranges, re-resolving when a URL is refused
    /// or runs out partway (the bytes stay the same: Phase 0).
    async fn download_chunks(
        &self,
        stream: ResolvedStream,
        dest: &Path,
    ) -> ProviderResult<DownloadOutcome> {
        use tokio::io::AsyncWriteExt;
        let video = self
            .streams
            .lock()
            .iter()
            .find(|(_, s)| s.url == stream.url.as_str())
            .map(|(id, _)| id.clone());
        let mut url = stream.url.to_string();
        let mut ua = stream
            .headers
            .get("User-Agent")
            .cloned()
            .unwrap_or_default();
        let total = stream.content_length;
        let mut file = tokio::fs::File::create(dest)
            .await
            .map_err(|e| ProviderError::Io(e.to_string()))?;
        let mut at: u64 = 0;
        let mut retries = 0;
        let mut content_type = None;
        loop {
            if total.is_some_and(|t| at >= t) {
                break;
            }
            let want = if at == 0 { FIRST_RANGE } else { MAX_RANGE };
            let end = match total {
                Some(t) => (at + want).min(t) - 1,
                None => at + want - 1,
            };
            let r = self
                .it
                .http()
                .send(HttpRequest {
                    method: HttpMethod::Get,
                    url: url.clone(),
                    headers: vec![
                        ("User-Agent".into(), ua.clone()),
                        ("Range".into(), format!("bytes={at}-{end}")),
                    ],
                    body: None,
                })
                .await
                .map_err(ProviderError::Network)?;
            match r.status {
                200 | 206 => {
                    retries = 0;
                    content_type = content_type.or(r.content_type.clone());
                    if r.body.is_empty() {
                        break;
                    }
                    file.write_all(&r.body)
                        .await
                        .map_err(|e| ProviderError::Io(e.to_string()))?;
                    at += r.body.len() as u64;
                    if total.is_none() && (r.body.len() as u64) < end - at + 1 {
                        break;
                    }
                }
                416 => break,
                403 | 410 if retries < 2 => {
                    retries += 1;
                    let Some(id) = &video else {
                        return Err(ProviderError::Auth(format!("HTTP {}", r.status)));
                    };
                    self.stream_failed(id, r.status);
                    let s = self.stream(id).await?;
                    if s.content_length.is_some() && s.content_length != total {
                        return Err(ProviderError::Protocol(
                            "the stream changed while it downloaded".into(),
                        ));
                    }
                    url = s.url;
                    ua = s.user_agent.to_string();
                }
                s => {
                    return Ok(DownloadOutcome {
                        status: s,
                        content_type,
                        bytes: at,
                    })
                }
            }
        }
        file.flush()
            .await
            .map_err(|e| ProviderError::Io(e.to_string()))?;
        Ok(DownloadOutcome {
            status: 200,
            content_type: content_type.or(Some("audio/mp4".into())),
            bytes: at,
        })
    }
}
