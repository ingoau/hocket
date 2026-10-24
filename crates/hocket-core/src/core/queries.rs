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
            Query::Artist { id } => QueryResult::ArtistDetail(self.db.artist(&id).ok().flatten()),
            Query::ArtistTopSongs { id, count } => {
                self.artist_top_songs(id, count, reply);
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
        if let Ok(Some(path)) = self.downloads.downloaded_path(&track.server_id, &track.id) {
            let _ = reply.send(QueryResult::AudioFile(Some(ExportedAudio {
                path: path.to_string_lossy().into_owned(),
                file_name,
                content_type,
            })));
            return;
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
        dest.push(format!("{}.{}", export_safe_id(&track.id), ext));
        if dest.is_file() && std::fs::metadata(&dest).is_ok_and(|m| m.len() > 0) {
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
            let path = caches
                .artwork_path(api.as_ref(), &id, size)
                .await
                .ok()
                .flatten()
                .map(|p| p.to_string_lossy().into_owned());
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
        let Some(artist) = self.db.artist(&id).ok().flatten() else {
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
        .take(80)
        .collect();
    if safe.is_empty() {
        "track".into()
    } else {
        safe
    }
}

/// The artist's most played tracks from the mirror (offline answer).
#[cfg(test)]
mod tests {
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
    }
}

fn most_played(db: &crate::db::Db, artist_id: &str, count: u32) -> Vec<Track> {
    let mut tracks = db.artist_tracks(artist_id).unwrap_or_default();
    tracks.sort_by_key(|t| std::cmp::Reverse(t.play_count));
    tracks.truncate(count as usize);
    tracks
}
