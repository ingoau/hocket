//! `Query` → `QueryResult`, answered from real state. Answers that need the
//! network (artwork fetch, related tracks) are produced by a spawned task
//! that owns the reply channel; the loop never waits.

use tokio::sync::oneshot;

use crate::actions::Surface;
use crate::api::*;
use crate::core::actor::Actor;
use crate::core::handlers::library::DbAutoplaySource;
use crate::core::{ActorMsg, Internal};
use crate::db::queries::{AlbumQuery, TrackQuery, WhereClause};
use crate::stats::StatsOptions;

impl Actor {
    pub(crate) fn answer_query(&mut self, query: Query, reply: oneshot::Sender<QueryResult>) {
        let answer = match query {
            Query::Snapshot => QueryResult::SnapshotResult(self.snapshot()),
            Query::Servers => QueryResult::Servers(self.server_infos()),
            Query::Tracks {
                server_id,
                filter,
                sort,
                descending,
                page,
            } => {
                let filter = self.where_for(filter.as_ref());
                let q = TrackQuery {
                    server_id,
                    filter,
                    sort,
                    descending,
                    page: Some(page.clone()),
                };
                QueryResult::Tracks(self.db.track_page(&q).unwrap_or(TrackPage {
                    items: vec![],
                    offset: page.offset,
                    total: 0,
                }))
            }
            Query::TrackCount { server_id, filter } => {
                let filter = self.where_for(filter.as_ref());
                QueryResult::Count(self.db.track_count(&server_id, &filter).unwrap_or(0))
            }
            Query::Track { id } => QueryResult::TrackDetail(self.db.track(&id).ok().flatten()),
            Query::TracksByIds { ids } => {
                QueryResult::TrackList(self.db.tracks_by_ids(&ids).unwrap_or_default())
            }
            Query::Albums {
                server_id,
                artist_id,
                genre,
                sort,
                descending,
                page,
            } => {
                let q = AlbumQuery {
                    server_id,
                    artist_id,
                    genre,
                    sort,
                    descending,
                    page: Some(page.clone()),
                };
                QueryResult::Albums(self.db.album_page(&q).unwrap_or(AlbumPage {
                    items: vec![],
                    offset: page.offset,
                    total: 0,
                }))
            }
            Query::AlbumCount {
                server_id,
                artist_id,
                genre,
            } => {
                let q = AlbumQuery {
                    server_id,
                    artist_id,
                    genre,
                    ..Default::default()
                };
                QueryResult::Count(self.db.album_count(&q).unwrap_or(0))
            }
            Query::Album { id } => QueryResult::AlbumDetail(self.db.album(&id).ok().flatten()),
            Query::AlbumTracks { id } => {
                QueryResult::TrackList(self.db.album_tracks(&id).unwrap_or_default())
            }
            Query::Artists {
                server_id,
                page,
                sort,
                descending,
            } => QueryResult::Artists(
                self.db
                    .artist_page_sorted(&server_id, page.clone(), sort, descending)
                    .unwrap_or(ArtistPage {
                        items: vec![],
                        offset: page.offset,
                        total: 0,
                    }),
            ),
            Query::Artist { id } => QueryResult::ArtistDetail(
                self.db
                    .artist_or_credited(&id, self.server_id().as_deref())
                    .ok()
                    .flatten(),
            ),
            Query::ArtistAppearsOn { id } => {
                let items = self
                    .db
                    .artist_appears_on(&id, self.server_id().as_deref())
                    .unwrap_or_default();
                QueryResult::Albums(AlbumPage {
                    total: items.len() as u32,
                    items,
                    offset: 0,
                })
            }
            Query::ArtistTopSongs { id, count } => {
                self.artist_top_songs(id, count, reply);
                return;
            }
            Query::ArtistInfo { id } => {
                self.external_info(InfoOf::Artist, id, reply);
                return;
            }
            Query::AlbumInfo { id } => {
                self.external_info(InfoOf::Album, id, reply);
                return;
            }
            Query::Genres { server_id } => {
                QueryResult::Genres(self.db.genres(&server_id).unwrap_or_default())
            }
            Query::Playlists { server_id } => {
                QueryResult::Playlists(self.db.playlists(&server_id).unwrap_or_default())
            }
            Query::Playlist { id } => {
                QueryResult::PlaylistDetail(self.db.playlist(&id).ok().flatten())
            }
            Query::PlaylistTracks { id, page } => {
                QueryResult::Tracks(self.db.playlist_track_page(&id, page.clone()).unwrap_or(
                    TrackPage {
                        items: vec![],
                        offset: page.offset,
                        total: 0,
                    },
                ))
            }
            Query::PlaylistMembership {
                server_id,
                track_ids,
            } => QueryResult::Membership(
                self.db
                    .playlist_membership(&server_id, &track_ids)
                    .unwrap_or_default(),
            ),
            Query::Search {
                server_id,
                query,
                limit,
                include_server,
                request_id,
            } => QueryResult::Search(self.search(
                server_id,
                query,
                limit,
                include_server,
                request_id,
            )),
            Query::Queue => QueryResult::Queue(self.queue_view()),
            Query::SavedQueues => QueryResult::SavedQueues(
                self.doc()
                    .map(|d| d.saved_queues.clone())
                    .unwrap_or_default(),
            ),
            Query::Lyrics { track_id } => {
                let cached = self.cached_lyrics(&track_id);
                if cached.is_none() {
                    self.fetch_lyrics(track_id, false);
                }
                QueryResult::LyricsResult(cached)
            }
            Query::Related { track_id, count } => {
                self.related(track_id, count, reply);
                return;
            }
            Query::Stats { period_days } => {
                let now = self.now();
                let since = now - f64::from(period_days.max(1)) * 86_400_000.0;
                let rows = self.play_rows(since);
                let opts = StatsOptions {
                    period_days,
                    ..Default::default()
                };
                QueryResult::Stats(crate::stats::listening_stats(&rows, &opts, now))
            }
            Query::RecentlyPlayed { limit } => {
                QueryResult::History(self.db.recently_played(limit).unwrap_or_default())
            }
            Query::Jobs => QueryResult::Jobs(self.jobs.jobs().unwrap_or_default()),
            Query::Problems => QueryResult::Problems(self.jobs.problems().unwrap_or_default()),
            Query::Pins => QueryResult::Pins(self.pins()),
            Query::Storage => QueryResult::Storage(self.storage_summary()),
            Query::Filters => QueryResult::Filters(self.filters()),
            Query::FilterPreview { filter } => QueryResult::Preview(self.filter_preview(&filter)),
            Query::Settings => QueryResult::Settings(self.settings.to_api()),
            Query::Setting { key } => QueryResult::SettingDetail(self.settings.setting(&key)),
            Query::AudioSettings => QueryResult::Audio(self.audio.clone()),
            Query::OutputDevices => QueryResult::OutputDevices(self.output_devices.clone()),
            Query::Connection => QueryResult::Connection(self.connection_state()),
            Query::Devices => QueryResult::Devices(
                self.engine
                    .as_ref()
                    .map(|e| e.devices())
                    .unwrap_or_default(),
            ),
            Query::UndoState => QueryResult::Undo(self.undo.state()),
            Query::Actions { surface, target } => {
                let state = self.state_view();
                let list = match Surface::parse(&surface) {
                    Some(s) => self.registry.actions_for(s, &target, &state),
                    None => vec![],
                };
                QueryResult::Actions(list)
            }
            Query::Shortcuts => QueryResult::Shortcuts(self.registry.shortcuts()),
            Query::Artwork { id, size } => {
                self.artwork_query(id, size, reply);
                return;
            }
            Query::MediaSource { track_id } => {
                let source = self
                    .track_or_bare(&track_id)
                    .and_then(|t| self.media_source_for("query", &t));
                QueryResult::Source(source)
            }
            Query::Diagnostics => QueryResult::Text(self.diagnostics()),
            Query::RoomDiagnostics => {
                self.room_diagnostics_query(reply);
                return;
            }
            Query::ConfigDocument { include_secrets } => {
                QueryResult::Text(self.config_document(include_secrets))
            }
            Query::CreateShare { ids } => {
                self.create_share_query(ids, reply);
                return;
            }
            Query::ExportAudio { track_id } => {
                self.export_audio_query(track_id, reply);
                return;
            }
        };
        let _ = reply.send(answer);
    }

    fn where_for(&self, node: Option<&FilterNode>) -> Option<WhereClause> {
        let node = node?;
        match crate::filters::where_clause(node, self.now()) {
            Ok(w) => Some(WhereClause::new(w.sql, w.params)),
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "filter rejected");
                Some(WhereClause::new("0=1", vec![]))
            }
        }
    }

    fn create_share_query(&mut self, ids: Vec<String>, reply: oneshot::Sender<QueryResult>) {
        let ids: Vec<String> = ids.into_iter().filter(|id| !id.is_empty()).collect();
        let Some(api) = self.api().filter(|_| !ids.is_empty()) else {
            let _ = reply.send(QueryResult::Share(None));
            return;
        };
        self.spawn(async move {
            let url = match api.create_share(&ids).await {
                Ok(url) if !url.is_empty() => Some(url),
                Ok(_) => None,
                Err(e) => {
                    tracing::debug!(error = %e, "createShare failed");
                    None
                }
            };
            let _ = reply.send(QueryResult::Share(url.map(|url| CreatedShare { url })));
        });
    }

    fn export_audio_query(&mut self, track_id: String, reply: oneshot::Sender<QueryResult>) {
        let Some(track) = self.db.track(&track_id).ok().flatten() else {
            let _ = reply.send(QueryResult::AudioFile(None));
            return;
        };
        let file_name = export_file_name(&track);
        let content_type = track
            .content_type
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "application/octet-stream".into());
        // A transcoded pin is a different file (opus for a flac). Share the original only.
        if let Ok(Some(path)) = self.downloads.downloaded_path(&track.server_id, &track.id) {
            if pin_matches_original(&path, track.suffix.as_deref()) {
                let _ = reply.send(QueryResult::AudioFile(Some(ExportedAudio {
                    path: path.to_string_lossy().into_owned(),
                    file_name,
                    content_type,
                })));
                return;
            }
        }
        let Some(api) = self.api() else {
            let _ = reply.send(QueryResult::AudioFile(None));
            return;
        };
        let mut dest = std::path::PathBuf::from(&self.cfg.cache_dir);
        dest.push("share");
        let ext = track
            .suffix
            .as_deref()
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("bin");
        dest.push(format!("{}.{}", export_cache_stem(&track.id), ext));
        if share_cache_fresh(&dest, track.size_bytes) {
            let _ = reply.send(QueryResult::AudioFile(Some(ExportedAudio {
                path: dest.to_string_lossy().into_owned(),
                file_name,
                content_type,
            })));
            return;
        }
        let id = track.id.clone();
        self.spawn(async move {
            let path = async {
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await.ok()?;
                }
                api.download_to_file(api.download_url(&id), &dest)
                    .await
                    .ok()?;
                Some(dest.to_string_lossy().into_owned())
            }
            .await;
            let _ = reply.send(QueryResult::AudioFile(path.map(|path| ExportedAudio {
                path,
                file_name,
                content_type,
            })));
        });
    }

    fn artwork_query(&mut self, id: String, size: u32, reply: oneshot::Sender<QueryResult>) {
        let Some(sid) = self.server_id() else {
            let _ = reply.send(QueryResult::Path(None));
            return;
        };
        if let Ok(Some(p)) = self.caches.artwork_cached(&sid, &id, size) {
            let _ = reply.send(QueryResult::Path(Some(p.to_string_lossy().into_owned())));
            return;
        }
        let Some(api) = self.api() else {
            let _ = reply.send(QueryResult::Path(None));
            return;
        };
        let caches = self.caches.clone();
        self.spawn(async move {
            // A failed fetch answers "no file" like a missing image; the
            // shell asks again (a later attempt, a network change).
            let path = match caches.artwork_path(api.as_ref(), &id, size).await {
                Ok(p) => p.map(|p| p.to_string_lossy().into_owned()),
                Err(e) => {
                    tracing::debug!(target: "hocket_core", id = %id, size, error = %e, "artwork fetch failed");
                    None
                }
            };
            let _ = reply.send(QueryResult::Path(path));
        });
    }

    fn related(&mut self, track_id: String, count: u32, reply: oneshot::Sender<QueryResult>) {
        let Some(api) = self.api() else {
            let _ = reply.send(QueryResult::Related(vec![]));
            return;
        };
        let track = self.summary_or_bare(&track_id);
        let source = DbAutoplaySource {
            api,
            db: self.db.clone(),
            server_id: self.server_id().unwrap_or_default(),
            now: self.now(),
            filters: self.filters(),
        };
        self.spawn(async move {
            let list = crate::autoplay::related(&source, &track, count).await;
            let _ = reply.send(QueryResult::Related(list));
        });
    }

    /// Top songs are served from the cache whenever it holds an answer, so
    /// the artist page renders at once; a stale answer is refreshed in the
    /// background and a `LibraryChanged` (table `artistTopSongs`) tells UIs
    /// to refetch when the list changed. Only a first visit waits for the
    /// server. Offline with nothing cached: the artist's most played tracks
    /// from the mirror.
    fn artist_top_songs(&mut self, id: String, count: u32, reply: oneshot::Sender<QueryResult>) {
        let Some(artist) = self
            .db
            .artist_or_credited(&id, self.server_id().as_deref())
            .ok()
            .flatten()
        else {
            let _ = reply.send(QueryResult::TrackList(vec![]));
            return;
        };
        let sid = self.server_id().unwrap_or_else(|| artist.server_id.clone());
        let api = self.api();
        let cached = match self.caches.top_songs_get(&sid, &id, count) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "top songs cache");
                None
            }
        };
        if let Some(cached) = cached {
            let tracks = with_mirror_state(&self.db, cached.tracks.clone());
            let _ = reply.send(QueryResult::TrackList(tracks));
            if let Some(api) = api {
                if cached.is_stale(self.now()) && self.top_songs_refreshing.insert(id.clone()) {
                    let caches = self.caches.clone();
                    let tx = self.tx.clone();
                    let requested = cached.requested.max(count);
                    self.spawn(async move {
                        let changed = match api.top_songs(&artist.name, requested).await {
                            Ok(children) => {
                                let tracks = tracks_of(&sid, &children);
                                caches
                                    .top_songs_put(&sid, &id, requested, &tracks)
                                    .unwrap_or(false)
                            }
                            Err(e) => {
                                tracing::debug!(target: "hocket_core", error = %e, "top songs refresh");
                                false
                            }
                        };
                        let _ = tx.send(ActorMsg::Internal(Internal::TopSongsRefreshed {
                            server_id: sid,
                            artist_id: id,
                            changed,
                        }));
                    });
                }
            }
            return;
        }
        let Some(api) = api else {
            let _ = reply.send(QueryResult::TrackList(most_played(&self.db, &id, count)));
            return;
        };
        let db = self.db.clone();
        let caches = self.caches.clone();
        self.spawn(async move {
            let list = match api.top_songs(&artist.name, count).await {
                Ok(children) => {
                    let tracks = tracks_of(&sid, &children);
                    if let Err(e) = caches.top_songs_put(&sid, &id, count, &tracks) {
                        tracing::debug!(target: "hocket_core", error = %e, "top songs cache");
                    }
                    with_mirror_state(&db, tracks)
                }
                // Not cached, so the next visit asks again.
                Err(_) => most_played(&db, &id, count),
            };
            let _ = reply.send(QueryResult::TrackList(list));
        });
    }

    /// [`Query::ArtistInfo`] / [`Query::AlbumInfo`]: a cached answer when one is fresh,
    /// otherwise the server, falling back to an older cached answer when the server can't be
    /// reached. An empty answer is cached too (most servers without a Last.fm key), for a day.
    fn external_info(&mut self, of: InfoOf, id: String, reply: oneshot::Sender<QueryResult>) {
        let owner = match of {
            InfoOf::Artist => self
                .db
                .artist_or_credited(&id, None)
                .ok()
                .flatten()
                .map(|a| a.server_id),
            InfoOf::Album => self.db.album(&id).ok().flatten().map(|a| a.server_id),
        };
        let Some(sid) = self.server_id().or(owner) else {
            let _ = reply.send(QueryResult::ExternalInfo(None));
            return;
        };
        let key = match of {
            InfoOf::Artist => format!("artistInfo:{id}"),
            InfoOf::Album => format!("albumInfo:{id}"),
        };
        // One read: the entry is kept long past its freshness (see [`CachedExternalInfo`]) so an
        // older answer is still there to fall back on offline.
        let cached = self
            .caches
            .meta_get_stale::<CachedExternalInfo>(&sid, &key)
            .ok()
            .flatten();
        if let Some((entry, fetched_at)) = &cached {
            if fetched_at + entry.fresh_ms > self.now() {
                let _ = reply.send(QueryResult::ExternalInfo(entry.info.clone()));
                return;
            }
        }
        let stale = cached.and_then(|(entry, _)| entry.info);
        let Some(api) = self.api() else {
            let _ = reply.send(QueryResult::ExternalInfo(stale));
            return;
        };
        let db = self.db.clone();
        let caches = self.caches.clone();
        self.spawn(async move {
            let fetched = match of {
                InfoOf::Artist => api
                    .artist_info2(&id, 10, false)
                    .await
                    .map(|a| artist_info(&db, &a)),
                InfoOf::Album => api.album_info2(&id).await.map(|a| album_info(&a)),
            };
            let answer = match fetched {
                Ok(info) => {
                    let info = (!info.is_empty()).then_some(info);
                    if let (InfoOf::Artist, Some(text)) =
                        (of, info.as_ref().and_then(|i| i.text.as_deref()))
                    {
                        let _ = db.set_artist_biography(&id, Some(text));
                    }
                    let fresh_ms = if info.is_some() {
                        EXTERNAL_INFO_TTL_MS
                    } else {
                        crate::cache::DEFAULT_META_TTL_MS
                    };
                    let entry = CachedExternalInfo {
                        info: info.clone(),
                        fresh_ms,
                    };
                    if let Err(e) = caches.meta_put(&sid, &key, &entry, Some(EXTERNAL_INFO_KEEP_MS))
                    {
                        tracing::debug!(target: "hocket_core", error = %e, "external info cache");
                    }
                    info
                }
                Err(e) => {
                    tracing::debug!(target: "hocket_core", error = %e, "external info");
                    stale
                }
            };
            let _ = reply.send(QueryResult::ExternalInfo(answer));
        });
    }

    pub(crate) fn on_top_songs_refreshed(
        &mut self,
        server_id: ServerId,
        artist_id: ArtistId,
        changed: bool,
    ) {
        self.top_songs_refreshing.remove(&artist_id);
        if changed {
            self.emit(Event::LibraryChanged {
                server_id,
                tables: vec![TOP_SONGS_TABLE.into()],
                ids: vec![artist_id],
            });
        }
    }

    pub(crate) fn diagnostics(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Hocket core {} (api schema {})\n",
            env!("CARGO_PKG_VERSION"),
            API_SCHEMA_VERSION
        ));
        out.push_str(&format!(
            "device: {} ({}) platform {:?} app {}\n",
            self.cfg.device_name, self.cfg.device_id, self.cfg.platform, self.cfg.app_version
        ));
        out.push_str(&format!(
            "audio: {:?} backend={} gapless={} device={:?}\n",
            self.cfg.audio,
            self.backend.name(),
            self.audio.gapless,
            self.audio.output_device
        ));
        out.push_str(&format!(
            "developer mode: {} (detailed logging {})\n",
            self.settings
                .get_bool(crate::settings::keys::DEVELOPER_MODE),
            crate::devlog::is_detailed()
        ));
        out.push_str(&format!(
            "data: {} cache: {}\n",
            self.cfg.data_dir, self.cfg.cache_dir
        ));
        out.push_str(&format!(
            "db: schema {} (from {}) backup={:?} mirror_rebuilt={}\n",
            self.open_report.to_version,
            self.open_report.from_version,
            self.open_report.backup_path,
            self.open_report.mirror_rebuilt
        ));
        for s in self.server_infos() {
            out.push_str(&format!(
                "server: {} {} user={} reachable={} version={:?} extensions={:?} floor={}\n",
                s.name,
                s.url,
                s.username,
                s.reachable,
                s.capabilities.server_version,
                s.capabilities.extensions,
                s.capabilities.meets_floor
            ));
        }
        let c = self.connection_state();
        out.push_str(&format!(
            "connect: tier={:?} connected={} peers={} offset={:.0}ms rtt={:?} error={:?}\n",
            c.tier, c.connected, c.peer_count, c.clock_offset_ms, c.round_trip_ms, c.error
        ));
        if let Some(e) = &self.engine {
            out.push_str(&format!(
                "session: rev={} owns={} detached={} lease={:?}\n",
                e.document().revision,
                e.owns_transport(),
                e.is_detached(),
                e.lease()
            ));
        }
        out.push_str(&format!(
            "playback: key={:?} playing={} pos={} loaded={}\n",
            self.playback.doc_key,
            self.playback.playing,
            self.playback.position_now(self.now()),
            self.playback.loaded
        ));
        out.push_str(&format!(
            "jobs: {} problems: {} outbox pending: {}\n",
            self.jobs.jobs().map(|j| j.len()).unwrap_or(0),
            self.jobs.problems().map(|p| p.len()).unwrap_or(0),
            self.outbox.pending_count().unwrap_or(0)
        ));
        if let Some(p) = &self.sync_progress {
            out.push_str(&format!(
                "sync: phase={} {}/{:?} ready={:?} finished={}\n",
                p.phase, p.done, p.total, p.ready_tables, p.finished
            ));
        }
        out.push_str("recent log:\n");
        for l in &self.log_ring {
            out.push_str("  ");
            out.push_str(l);
            out.push('\n');
        }
        out
    }
}

/// The `LibraryChanged.tables` entry for a refreshed top songs list.
pub(crate) const TOP_SONGS_TABLE: &str = "artistTopSongs";

#[derive(Clone, Copy)]
enum InfoOf {
    Artist,
    Album,
}

/// A week: biographies and album notes rarely change.
const EXTERNAL_INFO_TTL_MS: f64 = 7.0 * 24.0 * 3600.0 * 1000.0;
/// How long an artist or album info entry stays stored (for offline) after it stops being fresh.
const EXTERNAL_INFO_KEEP_MS: f64 = 180.0 * 24.0 * 3600.0 * 1000.0;

/// An artist or album info answer as cached: the answer (None = the server had nothing) and how
/// long after fetching it counts as fresh. Stored with a much longer expiry than that, so a stale
/// answer survives housekeeping to stand in when the server can't be reached.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedExternalInfo {
    info: Option<ExternalInfo>,
    fresh_ms: f64,
}

fn artist_info(db: &crate::db::Db, a: &crate::subsonic::types::ArtistInfo2) -> ExternalInfo {
    ExternalInfo {
        text: a.biography.as_deref().and_then(plain_text),
        music_brainz_id: non_empty(&a.music_brainz_id),
        last_fm_url: non_empty(&a.last_fm_url),
        image_url: non_empty(&a.large_image_url)
            .or_else(|| non_empty(&a.medium_image_url))
            .or_else(|| non_empty(&a.small_image_url)),
        similar_artists: a
            .similar_artist
            .iter()
            .filter_map(|s| db.artist(&s.id).ok().flatten())
            .collect(),
    }
}

fn album_info(a: &crate::subsonic::types::AlbumInfo) -> ExternalInfo {
    ExternalInfo {
        text: a.notes.as_deref().and_then(plain_text),
        music_brainz_id: non_empty(&a.music_brainz_id),
        last_fm_url: non_empty(&a.last_fm_url),
        image_url: non_empty(&a.large_image_url)
            .or_else(|| non_empty(&a.medium_image_url))
            .or_else(|| non_empty(&a.small_image_url)),
        similar_artists: vec![],
    }
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Last.fm text as the server passes it on: HTML with a closing
/// `<a href="...">Read more on Last.fm</a>`. Drops that link, strips the other tags and decodes
/// the common entities; `None` when nothing is left.
fn plain_text(html: &str) -> Option<String> {
    let cut = html
        .rfind("<a ")
        .filter(|&i| html[i..].contains("Last.fm"))
        .unwrap_or(html.len());
    let mut text = String::with_capacity(cut);
    let mut tag: Option<String> = None;
    for ch in html[..cut].chars() {
        match (&mut tag, ch) {
            (None, '<') => tag = Some(String::new()),
            (Some(name), '>') => {
                // A line break or the end of a block keeps the words apart.
                let n = name
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(n.trim_end_matches('/'), "br" | "p" | "div" | "li") {
                    text.push('\n');
                }
                tag = None;
            }
            (Some(name), c) => name.push(c),
            (None, c) => text.push(c),
        }
    }
    let out = decode_entities(&text);
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_string())
}

/// HTML entities in one pass (so "&amp;lt;" stays "&lt;"): the named ones a biography uses, and
/// numeric ones. Anything else is left as written.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest
            .char_indices()
            .take_while(|&(i, _)| i < 12)
            .find(|&(_, c)| c == ';')
            .map(|(i, _)| i)
        else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn tracks_of(server_id: &str, children: &[crate::subsonic::types::Child]) -> Vec<Track> {
    children
        .iter()
        .map(|c| crate::subsonic::convert::track_from_child(server_id, c))
        .collect()
}

/// Cached tracks with the mirror's current copy where it has one (ratings,
/// loves, play counts and offline state move on; the cached copy does not).
fn with_mirror_state(db: &crate::db::Db, tracks: Vec<Track>) -> Vec<Track> {
    let ids: Vec<String> = tracks.iter().map(|t| t.id.clone()).collect();
    let mut mirror: std::collections::HashMap<String, Track> = db
        .tracks_by_ids(&ids)
        .unwrap_or_default()
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect();
    tracks
        .into_iter()
        .map(|t| mirror.remove(&t.id).unwrap_or(t))
        .collect()
}

/// "Artist - Title.flac", with characters filesystems refuse turned into underscores.
fn export_file_name(track: &Track) -> String {
    let stem = match track.artist.as_deref().filter(|s| !s.is_empty()) {
        Some(artist) => format!("{artist} - {}", track.title),
        None => track.title.clone(),
    };
    let stem: String = stem
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(120)
        .collect();
    let stem = stem.trim().trim_matches('.').to_string();
    let stem = if stem.is_empty() {
        "track".to_string()
    } else {
        stem
    };
    match track
        .suffix
        .as_deref()
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        Some(suffix) => format!("{stem}.{suffix}"),
        None => stem,
    }
}

fn export_safe_id(id: &str) -> String {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    if safe.is_empty() {
        "track".into()
    } else {
        safe
    }
}

/// Cache file stem. A hash of the whole id keeps `ab/cd` from landing on `ab_cd`.
fn export_cache_stem(id: &str) -> String {
    format!("{}-{:08x}", export_safe_id(id), fnv1a(id.as_bytes()))
}

fn fnv1a(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for b in bytes {
        hash ^= u32::from(*b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// A finished pin is the original when its extension is the library suffix.
fn pin_matches_original(path: &std::path::Path, suffix: Option<&str>) -> bool {
    let Some(want) =
        suffix.filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
    else {
        return false;
    };
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|got| want.eq_ignore_ascii_case(got))
}

/// Reuse a cached original only when it is non-empty and, if we know the size, that size.
fn share_cache_fresh(dest: &std::path::Path, size_bytes: Option<f64>) -> bool {
    let Ok(meta) = std::fs::metadata(dest) else {
        return false;
    };
    if !meta.is_file() || meta.len() == 0 {
        return false;
    }
    match size_bytes {
        Some(n) if n > 0.0 && n.is_finite() => meta.len() == n as u64,
        _ => true,
    }
}

/// The artist's most played tracks from the mirror (offline answer).
#[cfg(test)]
mod tests {
    #[test]
    fn plain_text_drops_tags_entities_and_the_last_fm_link() {
        assert_eq!(
            super::plain_text(
                "Low Lanterns are a <b>folk</b> trio &amp; friends. <a href=\"https://www.last.fm/music/x\">Read more on Last.fm</a>"
            )
            .as_deref(),
            Some("Low Lanterns are a folk trio & friends.")
        );
        assert_eq!(
            super::plain_text(" <a href=\"u\">Read more on Last.fm</a>"),
            None
        );
        // Line breaks and paragraphs keep words apart; entities decode once, numeric ones too.
        assert_eq!(
            super::plain_text(
                "One<br/>two</p><p>three &amp;lt;b&amp;gt; &#039;x&#x27; &unknown; a & b"
            )
            .as_deref(),
            Some("One\ntwo\n\nthree &lt;b&gt; 'x' &unknown; a & b")
        );
        // Multi-byte text right after an ampersand never splits a character.
        assert_eq!(
            super::plain_text("Café & crème brûlée ünïcödé").as_deref(),
            Some("Café & crème brûlée ünïcödé")
        );
    }

    use super::*;

    fn track(title: &str, artist: Option<&str>, suffix: Option<&str>) -> Track {
        Track {
            id: "id/1".into(),
            title: title.into(),
            artist: artist.map(str::to_string),
            suffix: suffix.map(str::to_string),
            ..Track::default()
        }
    }

    #[test]
    fn export_names_keep_the_artist_and_suffix_and_drop_path_characters() {
        assert_eq!(
            export_file_name(&track("Roygbiv", Some("Boards of Canada"), Some("flac"))),
            "Boards of Canada - Roygbiv.flac"
        );
        assert_eq!(
            export_file_name(&track("a/b:c", None, Some("mp3"))),
            "a_b_c.mp3"
        );
        assert_eq!(export_file_name(&track("..", None, Some("../x"))), "track");
        assert_eq!(export_safe_id("ab/cd"), "ab_cd");
        assert_ne!(export_cache_stem("ab/cd"), export_cache_stem("ab_cd"));
        assert_eq!(export_cache_stem("ab/cd"), export_cache_stem("ab/cd"));
        assert!(pin_matches_original(
            std::path::Path::new("t.flac"),
            Some("FLAC")
        ));
        assert!(!pin_matches_original(
            std::path::Path::new("t.opus"),
            Some("flac")
        ));
        assert!(!pin_matches_original(std::path::Path::new("t.flac"), None));
    }
}

fn most_played(db: &crate::db::Db, artist_id: &str, count: u32) -> Vec<Track> {
    let mut tracks = db.artist_tracks(artist_id).unwrap_or_default();
    tracks.sort_by_key(|t| std::cmp::Reverse(t.play_count));
    tracks.truncate(count as usize);
    tracks
}
