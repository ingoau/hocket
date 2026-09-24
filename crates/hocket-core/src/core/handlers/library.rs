//! Library: mutations through the outbox (undo tier 2), downloads, filters,
//! lyrics, autoplay's data source and search.

use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;

use crate::api::*;
use crate::autoplay::{AutoplayError, AutoplaySource, ScoredTrack};
use crate::core::actor::Actor;
use crate::core::state::PendingCas;
use crate::core::{ActorMsg, Internal};
use crate::db::Db;
use crate::filters::{self, ServerCaps};
use crate::lyrics::{ExternalLyricsProvider, LyricsRequest};
use crate::outbox::{CancelOutcome, CasTarget, LoveTarget, Mutation, Prior};
use crate::subsonic::convert::{album_from_id3, artist_from_id3, summary_of, track_from_child};
use crate::subsonic::SubsonicApi;
use crate::undo::{
    CasMutation, CasOutcome, CasResult, RemoteItem, RemoteTarget, RemoteUndo, RemoteValue,
};

impl Actor {
    // -- ratings and loves -----------------------------------------------------------

    fn enqueue(&mut self, mutation: Mutation, prior: Option<Prior>) -> Option<String> {
        let server_id = self.server_id()?;
        let key = mutation.target_key();
        match self.outbox.enqueue(&server_id, mutation, prior) {
            Ok(id) => {
                self.outbox_entries.insert(key, id.clone());
                Some(id)
            }
            Err(e) => {
                self.error(ErrorKind::Storage, "queue change", Some(e.to_string()));
                None
            }
        }
    }

    fn after_library_mutation(&mut self, tables: Vec<&str>, ids: Vec<String>) {
        if let Some(sid) = self.server_id() {
            self.emit(Event::LibraryChanged {
                server_id: sid,
                tables: tables.into_iter().map(String::from).collect(),
                ids,
            });
        }
        self.schedule_flush();
        self.emit_queue();
    }

    fn push_remote_undo(&mut self, kind: &str, label: String, items: Vec<RemoteItem>) {
        if items.is_empty() {
            return;
        }
        let now = self.now();
        self.undo.push(
            Box::new(RemoteUndo::new(kind, label, items)),
            self.selection.clone(),
            now,
        );
        self.emit(Event::UndoChanged {
            state: self.undo.state(),
        });
    }

    pub(crate) fn set_rating(&mut self, targets: Vec<RatingTarget>, rating: u32) {
        let rating = rating.min(5);
        let mut items = vec![];
        let mut ids = vec![];
        let mut love_targets = vec![];
        let threshold =
            self.settings
                .get_i64(crate::settings::keys::RATINGS_LOVE_BRIDGE_THRESHOLD) as u32;
        let bridge = self
            .settings
            .get_bool(crate::settings::keys::RATINGS_LOVE_BRIDGE_ENABLED);
        for t in targets {
            let (prior, target, loved_now) = match &t {
                RatingTarget::Track { id } => {
                    let tr = self.db.track(id).ok().flatten();
                    (
                        tr.as_ref().map(|t| t.rating).unwrap_or(0),
                        RemoteTarget::Track { id: id.clone() },
                        tr.map(|t| t.loved).unwrap_or(false),
                    )
                }
                RatingTarget::Album { id } => {
                    let al = self.db.album(id).ok().flatten();
                    (
                        al.as_ref().map(|a| a.rating).unwrap_or(0),
                        RemoteTarget::Album { id: id.clone() },
                        al.map(|a| a.loved).unwrap_or(false),
                    )
                }
            };
            ids.push(match &t {
                RatingTarget::Track { id } | RatingTarget::Album { id } => id.clone(),
            });
            self.enqueue(
                Mutation::SetRating {
                    target: t.clone(),
                    rating,
                },
                Some(Prior::Rating(prior)),
            );
            items.push(RemoteItem {
                target,
                prior: RemoteValue::Rating(prior),
                set: RemoteValue::Rating(rating),
            });
            if let Some(love) = crate::stats::bridge_love(rating, threshold, bridge) {
                if love != loved_now {
                    love_targets.push(t);
                }
            }
        }
        let n = items.len();
        self.push_remote_undo("rate", rate_label(rating, n), items);
        self.after_library_mutation(vec!["tracks", "albums"], ids);
        if !love_targets.is_empty() {
            // Rating → love bridge: one-way, only above the threshold.
            self.set_loved(love_targets, true);
        }
    }

    pub(crate) fn set_loved(&mut self, targets: Vec<RatingTarget>, loved: bool) {
        let mut items = vec![];
        let mut ids = vec![];
        for t in targets {
            let (prior, target) = match &t {
                RatingTarget::Track { id } => (
                    self.db
                        .track(id)
                        .ok()
                        .flatten()
                        .map(|t| t.loved)
                        .unwrap_or(false),
                    RemoteTarget::Track { id: id.clone() },
                ),
                RatingTarget::Album { id } => (
                    self.db
                        .album(id)
                        .ok()
                        .flatten()
                        .map(|a| a.loved)
                        .unwrap_or(false),
                    RemoteTarget::Album { id: id.clone() },
                ),
            };
            ids.push(match &t {
                RatingTarget::Track { id } | RatingTarget::Album { id } => id.clone(),
            });
            self.enqueue(
                Mutation::SetLoved {
                    target: LoveTarget::from(t),
                    loved,
                },
                Some(Prior::Loved(prior)),
            );
            items.push(RemoteItem {
                target,
                prior: RemoteValue::Loved(prior),
                set: RemoteValue::Loved(loved),
            });
        }
        let n = items.len();
        let label = if loved { "Love" } else { "Unlove" };
        self.push_remote_undo(
            "love",
            if n == 1 {
                label.to_string()
            } else {
                format!("{label} {n} items")
            },
            items,
        );
        self.after_library_mutation(vec!["tracks", "albums"], ids);
    }

    pub(crate) fn set_artist_loved(&mut self, artist_id: ArtistId, loved: bool) {
        let prior = self
            .db
            .artist(&artist_id)
            .ok()
            .flatten()
            .map(|a| a.loved)
            .unwrap_or(false);
        self.enqueue(
            Mutation::SetLoved {
                target: LoveTarget::Artist {
                    id: artist_id.clone(),
                },
                loved,
            },
            Some(Prior::Loved(prior)),
        );
        self.push_remote_undo(
            "loveArtist",
            if loved {
                "Love artist"
            } else {
                "Unlove artist"
            }
            .to_string(),
            vec![RemoteItem {
                target: RemoteTarget::Artist {
                    id: artist_id.clone(),
                },
                prior: RemoteValue::Loved(prior),
                set: RemoteValue::Loved(loved),
            }],
        );
        self.after_library_mutation(vec!["artists"], vec![artist_id]);
    }

    // -- playlists ---------------------------------------------------------------------

    pub(crate) fn create_playlist(
        &mut self,
        server_id: ServerId,
        name: String,
        track_ids: Vec<TrackId>,
    ) {
        if self.server_id().as_deref() != Some(server_id.as_str()) {
            self.toast("Unknown server", None);
            return;
        }
        if name.trim().is_empty() {
            self.toast("Give the playlist a name", None);
            return;
        }
        self.enqueue(Mutation::PlaylistCreate { name, track_ids }, None);
        self.after_library_mutation(vec!["playlists"], vec![]);
    }

    pub(crate) fn delete_playlist(&mut self, playlist_id: PlaylistId) {
        // Confirm-instead: no undo entry (the platform asks first).
        if !matches!(self.selection, ActionTarget::None) {
            let sel = std::mem::replace(&mut self.selection, ActionTarget::None);
            self.undo.remember_cleared_selection(sel);
            self.toast(
                "Selection cleared",
                Some(("Restore".into(), Command::RestoreSelection)),
            );
        }
        self.enqueue(
            Mutation::PlaylistDelete {
                playlist_id: playlist_id.clone(),
            },
            None,
        );
        self.after_library_mutation(vec!["playlists"], vec![playlist_id]);
    }

    pub(crate) fn rename_playlist(
        &mut self,
        playlist_id: PlaylistId,
        name: String,
        comment: Option<String>,
        public: Option<bool>,
    ) {
        let prior = self.db.playlist(&playlist_id).ok().flatten();
        self.enqueue(
            Mutation::PlaylistRename {
                playlist_id: playlist_id.clone(),
                name: Some(name.clone()),
                comment: comment.clone(),
                public,
            },
            None,
        );
        if let Some(p) = prior {
            self.push_remote_undo(
                "renamePlaylist",
                "Rename playlist".into(),
                vec![RemoteItem {
                    target: RemoteTarget::Playlist {
                        id: playlist_id.clone(),
                    },
                    prior: RemoteValue::Meta {
                        name: p.name,
                        comment: p.comment,
                        public: Some(p.public),
                    },
                    set: RemoteValue::Meta {
                        name,
                        comment,
                        public,
                    },
                }],
            );
        }
        self.after_library_mutation(vec!["playlists"], vec![playlist_id]);
    }

    fn playlist_order(&self, playlist_id: &str) -> Vec<String> {
        self.db.playlist_track_ids(playlist_id).unwrap_or_default()
    }

    fn playlist_is_smart(&self, playlist_id: &str) -> bool {
        self.db
            .playlist(playlist_id)
            .ok()
            .flatten()
            .map(|p| p.is_smart)
            .unwrap_or(false)
    }

    pub(crate) fn playlist_add(
        &mut self,
        playlist_id: PlaylistId,
        track_ids: Vec<TrackId>,
        at_index: Option<u32>,
    ) {
        if self.playlist_is_smart(&playlist_id) {
            self.toast("Smart playlists are read-only", None);
            return;
        }
        let order = self.playlist_order(&playlist_id);
        self.enqueue(
            Mutation::PlaylistAdd {
                playlist_id: playlist_id.clone(),
                track_ids: track_ids.clone(),
                at_index,
            },
            Some(Prior::PlaylistOrder(order)),
        );
        let items = track_ids
            .iter()
            .enumerate()
            .map(|(i, t)| RemoteItem {
                target: RemoteTarget::PlaylistTrack {
                    playlist_id: playlist_id.clone(),
                    track_id: t.clone(),
                },
                prior: RemoteValue::Member {
                    present: false,
                    index: None,
                },
                set: RemoteValue::Member {
                    present: true,
                    index: at_index.map(|a| a + i as u32),
                },
            })
            .collect();
        self.push_remote_undo(
            "playlistAdd",
            format!("Add to playlist ({})", track_ids.len()),
            items,
        );
        self.after_library_mutation(vec!["playlists", "playlist_tracks"], vec![playlist_id]);
    }

    pub(crate) fn playlist_remove(&mut self, playlist_id: PlaylistId, indices: Vec<u32>) {
        if self.playlist_is_smart(&playlist_id) {
            self.toast("Smart playlists are read-only", None);
            return;
        }
        let order = self.playlist_order(&playlist_id);
        let track_ids: Vec<String> = indices
            .iter()
            .filter_map(|i| order.get(*i as usize).cloned())
            .collect();
        if track_ids.is_empty() {
            return;
        }
        self.enqueue(
            Mutation::PlaylistRemove {
                playlist_id: playlist_id.clone(),
                track_ids: track_ids.clone(),
                indices: indices.clone(),
            },
            Some(Prior::PlaylistOrder(order)),
        );
        let items = track_ids
            .iter()
            .zip(indices.iter())
            .map(|(t, i)| RemoteItem {
                target: RemoteTarget::PlaylistTrack {
                    playlist_id: playlist_id.clone(),
                    track_id: t.clone(),
                },
                prior: RemoteValue::Member {
                    present: true,
                    index: Some(*i),
                },
                set: RemoteValue::Member {
                    present: false,
                    index: None,
                },
            })
            .collect();
        self.push_remote_undo(
            "playlistRemove",
            format!("Remove from playlist ({})", track_ids.len()),
            items,
        );
        self.after_library_mutation(vec!["playlists", "playlist_tracks"], vec![playlist_id]);
    }

    pub(crate) fn playlist_move(
        &mut self,
        playlist_id: PlaylistId,
        from_index: u32,
        to_index: u32,
    ) {
        if self.playlist_is_smart(&playlist_id) {
            self.toast("Smart playlists are read-only", None);
            return;
        }
        let order = self.playlist_order(&playlist_id);
        let Some(track_id) = order.get(from_index as usize).cloned() else {
            return;
        };
        self.enqueue(
            Mutation::PlaylistMove {
                playlist_id: playlist_id.clone(),
                track_id: track_id.clone(),
                from_index,
                to_index,
            },
            Some(Prior::PlaylistOrder(order)),
        );
        self.push_remote_undo(
            "playlistMove",
            "Move in playlist".into(),
            vec![RemoteItem {
                target: RemoteTarget::PlaylistTrack {
                    playlist_id: playlist_id.clone(),
                    track_id,
                },
                prior: RemoteValue::Position(from_index),
                set: RemoteValue::Position(to_index),
            }],
        );
        self.after_library_mutation(vec!["playlists", "playlist_tracks"], vec![playlist_id]);
    }

    // -- undo tier 2: compare-and-swap ---------------------------------------------------

    pub(crate) fn run_cas(
        &mut self,
        entry_id: &str,
        label: &str,
        mutations: Vec<CasMutation>,
        redo: bool,
    ) {
        let total = mutations.len();
        self.pending_cas.insert(
            entry_id.to_string(),
            PendingCas {
                outcome: CasOutcome::new(total),
                label: label.to_string(),
                redo,
            },
        );
        if total == 0 {
            self.finish_cas(entry_id);
            return;
        }
        let api = self.api();
        for m in mutations {
            // Still in the outbox: cancel is the exact inverse.
            let key = cas_target_key(&m);
            if let Some(id) = key
                .as_ref()
                .and_then(|k| self.outbox_entries.get(k).cloned())
            {
                match self.outbox.cancel_if_unsent(&id) {
                    Ok(CancelOutcome::Cancelled) => {
                        self.outbox_entries.remove(key.as_ref().unwrap());
                        self.record_cas(entry_id, CasResult::Applied);
                        continue;
                    }
                    Ok(_) => {}
                    Err(e) => self.log("warn", format!("cancel outbox entry: {e}")),
                }
            }
            match (cas_target(&m), &m.expect, cas_new(&m), api.clone()) {
                (Some(target), RemoteValue::Rating(exp), Some(Prior::Rating(new)), Some(api)) => {
                    self.spawn_cas(
                        entry_id,
                        api,
                        target,
                        Prior::Rating(*exp),
                        Prior::Rating(new),
                    );
                }
                (Some(target), RemoteValue::Loved(exp), Some(Prior::Loved(new)), Some(api)) => {
                    self.spawn_cas(entry_id, api, target, Prior::Loved(*exp), Prior::Loved(new));
                }
                _ => {
                    // Playlist inverses run as plain commands (the outbox
                    // rebases them on the server's current order).
                    self.handle_command(m.command.clone());
                    self.record_cas(entry_id, CasResult::Applied);
                }
            }
        }
        self.finish_cas(entry_id);
    }

    fn spawn_cas(
        &mut self,
        entry_id: &str,
        api: Arc<dyn SubsonicApi>,
        target: CasTarget,
        expected: Prior,
        new: Prior,
    ) {
        let outbox = self.outbox.clone();
        let tx = self.tx.clone();
        let entry_id = entry_id.to_string();
        let db = self.db.clone();
        let local_target = target.clone();
        let local_new = new.clone();
        self.spawn(async move {
            let result = match outbox
                .execute_cas(api.as_ref(), target, expected, new)
                .await
            {
                Ok(crate::outbox::CasOutcome::Applied) => {
                    apply_cas_locally(&db, &local_target, &local_new);
                    CasResult::Applied
                }
                Ok(crate::outbox::CasOutcome::Skipped { .. }) => CasResult::Skipped,
                Err(e) => {
                    tracing::warn!(target: "hocket_core", error = %e, "cas failed");
                    CasResult::Failed
                }
            };
            let _ = tx.send(ActorMsg::Internal(Internal::CasResult { entry_id, result }));
        });
    }

    fn record_cas(&mut self, entry_id: &str, result: CasResult) {
        if let Some(p) = self.pending_cas.get_mut(entry_id) {
            p.outcome.record(result);
        }
    }

    pub(crate) fn on_cas_result(&mut self, entry_id: String, result: CasResult) {
        self.record_cas(&entry_id, result);
        self.finish_cas(&entry_id);
        if let Some(sid) = self.server_id() {
            self.emit(Event::LibraryChanged {
                server_id: sid,
                tables: vec!["tracks".into(), "albums".into(), "artists".into()],
                ids: vec![],
            });
        }
        self.emit_queue();
    }

    fn finish_cas(&mut self, entry_id: &str) {
        let done = self
            .pending_cas
            .get(entry_id)
            .is_some_and(|p| p.outcome.is_complete());
        if !done {
            return;
        }
        let Some(p) = self.pending_cas.remove(entry_id) else {
            return;
        };
        let note = p.outcome.note();
        self.undo.set_note(entry_id, note.clone());
        let verb = if p.redo { "Redid" } else { "Undid" };
        let message = match note {
            Some(n) => format!("{verb} {}: {n}", p.label),
            None => format!("{verb} {}", p.label),
        };
        let action = if p.redo {
            ("Undo".to_string(), Command::Undo)
        } else {
            ("Redo".to_string(), Command::Redo)
        };
        self.toast(message, Some(action));
        self.emit(Event::UndoChanged {
            state: self.undo.state(),
        });
    }

    // -- downloads -----------------------------------------------------------------------

    pub(crate) fn pins(&self) -> Vec<Pin> {
        self.server_id()
            .and_then(|sid| self.downloads.pins(&sid).ok())
            .unwrap_or_default()
    }

    pub(crate) fn storage_summary(&self) -> StorageSummary {
        let images = self.caches.images_bytes().unwrap_or(0.0);
        self.downloads.storage_summary(images).unwrap_or_default()
    }

    pub(crate) fn pin(&mut self, target: PinTarget, transcode: bool) {
        let Some(sid) = self.server_id() else {
            self.toast("Add a server first", None);
            return;
        };
        let transcode = transcode
            || self
                .settings
                .get_bool(crate::settings::keys::DOWNLOADS_TRANSCODE);
        match self.downloads.pin(&sid, &target, transcode) {
            Ok(Some(spec)) => {
                if let Err(e) = self.jobs.submit(spec) {
                    self.error(ErrorKind::Storage, "download job", Some(e.to_string()));
                }
            }
            Ok(None) => {}
            Err(e) => {
                self.toast(format!("Couldn't pin: {e}"), None);
                return;
            }
        }
        let pins = self.pins();
        self.emit(Event::PinsChanged { pins });
        let storage = self.storage_summary();
        if self.downloads.over_warn_threshold().unwrap_or(false) {
            self.toast("Downloads are past your storage warning threshold", None);
        }
        self.emit(Event::StorageChanged { storage });
    }

    pub(crate) fn unpin(&mut self, target: PinTarget) {
        let Some(sid) = self.server_id() else { return };
        if let Err(e) = self.downloads.unpin(&sid, &target) {
            self.toast(format!("Couldn't remove download: {e}"), None);
            return;
        }
        let pins = self.pins();
        self.emit(Event::PinsChanged { pins });
        let storage = self.storage_summary();
        self.emit(Event::StorageChanged { storage });
        self.emit(Event::LibraryChanged {
            server_id: sid,
            tables: vec!["tracks".into()],
            ids: vec![],
        });
    }

    pub(crate) fn clear_stream_cache(&mut self) {
        if let Err(e) = self.downloads.clear_stream_cache() {
            self.toast(format!("Couldn't clear the cache: {e}"), None);
        }
        let storage = self.storage_summary();
        self.emit(Event::StorageChanged { storage });
    }

    // -- filters --------------------------------------------------------------------------

    /// Saved filters (built-ins first, overridden by a saved one of the same id).
    pub(crate) fn filters(&self) -> Vec<Filter> {
        let saved: Vec<Filter> = self
            .db
            .with_conn(|c| {
                let mut st =
                    c.prepare_cached("SELECT json FROM filters ORDER BY created_at ASC")?;
                let rows = st.query_map([], |r| r.get::<_, String>(0))?;
                Ok(rows
                    .collect::<Result<Vec<_>, _>>()?
                    .iter()
                    .filter_map(|j| serde_json::from_str(j).ok())
                    .collect())
            })
            .unwrap_or_default();
        let mut out: Vec<Filter> = filters::default_filters()
            .into_iter()
            .filter(|d| !saved.iter().any(|s| s.id == d.id))
            .collect();
        out.extend(saved);
        out
    }

    pub(crate) fn save_filter(&mut self, mut filter: Filter) {
        if let Err(e) = filters::validate_filter(&filter) {
            self.toast(format!("Filter is invalid: {e}"), None);
            return;
        }
        if filter.id.is_empty() {
            filter.id = crate::util::new_id();
        }
        let now = self.now();
        let json = match serde_json::to_string(&filter) {
            Ok(j) => j,
            Err(e) => {
                self.error(ErrorKind::Internal, "filter", Some(e.to_string()));
                return;
            }
        };
        let r = self.db.with_conn(|c| {
            c.execute(
                "INSERT INTO filters(id, name, json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(id) DO UPDATE SET name = excluded.name, json = excluded.json, updated_at = excluded.updated_at",
                rusqlite::params![filter.id, filter.name, json, now],
            )?;
            Ok(())
        });
        if let Err(e) = r {
            self.error(ErrorKind::Storage, "save filter", Some(e.to_string()));
            return;
        }
        let filters = self.filters();
        self.emit(Event::FiltersChanged { filters });
    }

    pub(crate) fn delete_filter(&mut self, id: FilterId) {
        let r = self.db.with_conn(|c| {
            c.execute("DELETE FROM filters WHERE id = ?1", [&id])?;
            Ok(())
        });
        if let Err(e) = r {
            self.error(ErrorKind::Storage, "delete filter", Some(e.to_string()));
        }
        let filters = self.filters();
        self.emit(Event::FiltersChanged { filters });
    }

    pub(crate) fn server_caps(&self) -> ServerCaps {
        let caps = self
            .server
            .as_ref()
            .map(|s| s.info.capabilities.clone())
            .unwrap_or_default();
        ServerCaps {
            sonic_attributes: caps.sonic_similarity,
            native_api: caps.native_api,
        }
    }

    pub(crate) fn filter_track_ids(&self, filter: &Filter, server_id: &str) -> Vec<TrackId> {
        let Ok(q) = filters::select_for_filter(filter, server_id, "tracks.id", self.now()) else {
            return vec![];
        };
        self.db
            .with_conn(|c| {
                let mut st = c.prepare(&q.sql)?;
                let rows = st.query_map(rusqlite::params_from_iter(q.params.iter()), |r| {
                    r.get::<_, String>(0)
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap_or_default()
    }

    pub(crate) fn filter_preview(&self, filter: &Filter) -> FilterPreview {
        let capability = filters::capability(filter, self.server_caps());
        let server_id = self.server_id().unwrap_or_default();
        let now = self.now();
        let count: u32 =
            filters::count_for_node(Some(&filter.root), &server_id, now)
                .ok()
                .and_then(|q| {
                    self.db
                        .with_conn(|c| {
                            Ok(c.query_row(
                                &q.sql,
                                rusqlite::params_from_iter(q.params.iter()),
                                |r| r.get::<_, i64>(0),
                            )?)
                        })
                        .ok()
                })
                .unwrap_or(0)
                .max(0) as u32;
        let sample = filters::select_for_node(
            Some(&filter.root),
            filter.sort,
            filter.descending,
            Some(10),
            None,
            &server_id,
            "tracks.id",
            now,
        )
        .ok()
        .and_then(|q| {
            self.db
                .with_conn(|c| {
                    let mut st = c.prepare(&q.sql)?;
                    let rows = st.query_map(rusqlite::params_from_iter(q.params.iter()), |r| {
                        r.get::<_, String>(0)
                    })?;
                    Ok(rows.collect::<Result<Vec<_>, _>>()?)
                })
                .ok()
        })
        .map(|ids| {
            let map = self.summaries(&ids);
            ids.iter().filter_map(|i| map.get(i).cloned()).collect()
        })
        .unwrap_or_default();
        FilterPreview {
            count,
            capability,
            sample,
        }
    }

    pub(crate) fn create_static_playlist(
        &mut self,
        server_id: ServerId,
        filter: Filter,
        name: String,
    ) {
        let ids = self.filter_track_ids(&filter, &server_id);
        if ids.is_empty() {
            self.toast("The filter matches nothing right now", None);
            return;
        }
        self.create_playlist(server_id, name, ids);
    }

    pub(crate) fn create_smart_playlist(
        &mut self,
        server_id: ServerId,
        filter: Filter,
        name: String,
    ) {
        let Some(api) = self.api() else {
            self.toast("Add a server first", None);
            return;
        };
        if !self.server_caps().native_api {
            self.toast(
                "This server doesn't expose the native API needed for smart playlists",
                None,
            );
            return;
        }
        let nsp = match filters::to_nsp(&filter, self.server_caps()) {
            Ok(n) => n,
            Err(e) => {
                self.toast(format!("Filter can't be a smart playlist: {e}"), None);
                return;
            }
        };
        let rules: serde_json::Value = match serde_json::from_str(&nsp) {
            Ok(v) => v,
            Err(e) => {
                self.error(ErrorKind::Internal, "nsp", Some(e.to_string()));
                return;
            }
        };
        let update = crate::subsonic::NativePlaylistUpdate {
            name: Some(name),
            comment: None,
            public: None,
            owner_id: None,
            rules: Some(rules),
            sync: None,
        };
        let tx = self.tx.clone();
        self.spawn(async move {
            match api.native_create_playlist(update).await {
                Ok(_) => {
                    let _ = tx.send(ActorMsg::Internal(Internal::PlaylistCreated { server_id }));
                }
                Err(e) => {
                    let _ = tx.send(ActorMsg::Internal(Internal::Toast {
                        message: format!("Couldn't create the smart playlist: {e}"),
                    }));
                }
            }
        });
    }

    pub(crate) fn export_nsp(&mut self, filter: Filter, path: Option<String>) {
        let document = match filters::to_nsp(&filter, self.server_caps()) {
            Ok(d) => d,
            Err(e) => {
                self.toast(format!("Filter can't be exported: {e}"), None);
                return;
            }
        };
        let filter_id = filter.id.clone();
        match path {
            Some(p) => {
                let tx = self.tx.clone();
                self.spawn(async move {
                    let error = tokio::fs::write(&p, document.as_bytes())
                        .await
                        .err()
                        .map(|e| e.to_string());
                    let _ = tx.send(ActorMsg::Internal(Internal::NspWritten {
                        filter_id,
                        document,
                        path: Some(p),
                        error,
                    }));
                });
            }
            None => self.emit(Event::NspExported {
                filter_id,
                document,
                path: None,
            }),
        }
    }

    // -- lyrics ----------------------------------------------------------------------------

    fn lyrics_offset(&self, track_id: &str) -> i32 {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Off {
            offset_ms: i32,
        }
        self.db
            .saved_state_get::<Off>(&crate::lyrics::offset_state_key(track_id))
            .ok()
            .flatten()
            .map(|o| o.offset_ms)
            .unwrap_or_else(|| {
                self.settings
                    .get_i64(crate::settings::keys::LYRICS_DEFAULT_OFFSET_MS) as i32
            })
    }

    /// Cached lyrics with the user's offset applied.
    pub(crate) fn cached_lyrics(&self, track_id: &str) -> Option<Lyrics> {
        let sid = self.server_id()?;
        let mut l = self.caches.lyrics_get_any(&sid, track_id).ok().flatten()?;
        crate::lyrics::set_user_offset(&mut l, self.lyrics_offset(track_id));
        Some(l)
    }

    pub(crate) fn set_lyrics_offset(&mut self, track_id: TrackId, offset_ms: i32) {
        let key = crate::lyrics::offset_state_key(&track_id);
        let v = serde_json::json!({ "offsetMs": offset_ms });
        if let Err(e) = self.db.saved_state_set(&key, &v, self.clock.as_ref()) {
            self.error(ErrorKind::Storage, "lyrics offset", Some(e.to_string()));
        }
        let lyrics = self.cached_lyrics(&track_id);
        self.emit(Event::LyricsChanged { track_id, lyrics });
    }

    /// Server first; external only when opted in; everything cached.
    pub(crate) fn fetch_lyrics(&mut self, track_id: TrackId, force: bool) {
        self.fetch_lyrics_with(track_id, force, true);
    }

    /// Warm the cache for a track without announcing it (the next item).
    pub(crate) fn prefetch_lyrics(&mut self, track_id: TrackId) {
        if self.battery_saver
            && self
                .settings
                .get_bool(crate::settings::keys::BATTERY_PAUSE_PREFETCH)
        {
            return;
        }
        let Some(sid) = self.server_id() else { return };
        let cached = self
            .caches
            .lyrics_get_any(&sid, &track_id)
            .ok()
            .flatten()
            .is_some()
            || matches!(
                self.caches
                    .lyrics_get(&sid, &track_id, LyricsSource::Server),
                Ok(Some(None))
            );
        if !cached {
            self.fetch_lyrics_with(track_id, false, false);
        }
    }

    /// Lyrics for whatever is now playing (cache → server → external),
    /// announced through `LyricsChanged`; the derived next item is prefetched.
    pub(crate) fn lyrics_for_now_playing(&mut self, queue: &QueueView) {
        let current = queue.current.as_ref().map(|e| e.track.id.clone());
        if current == self.lyrics_for {
            return;
        }
        self.lyrics_for = current.clone();
        let Some(id) = current else { return };
        self.fetch_lyrics(id, false);
        if let Some(next) = queue
            .playing_next
            .iter()
            .chain(queue.upcoming.iter())
            .find(|e| !e.item.unavailable)
        {
            let next_id = next.track.id.clone();
            self.prefetch_lyrics(next_id);
        }
    }

    fn fetch_lyrics_with(&mut self, track_id: TrackId, force: bool, announce: bool) {
        let Some(sid) = self.server_id() else { return };
        let Some(api) = self.api() else { return };
        if !force {
            if let Some(l) = self.cached_lyrics(&track_id) {
                if announce {
                    self.emit(Event::LyricsChanged {
                        track_id,
                        lyrics: Some(l),
                    });
                }
                return;
            }
        }
        let external_enabled = self
            .settings
            .get_bool(crate::settings::keys::LYRICS_EXTERNAL_ENABLED);
        let track = self.track_or_bare(&track_id);
        let caches = self.caches.clone();
        let tx = self.tx.clone();
        let http = self.lyrics_http.clone();
        let app_version = self.cfg.app_version.clone();
        let server_negative = matches!(
            caches.lyrics_get(&sid, &track_id, LyricsSource::Server),
            Ok(Some(None))
        ) && !force;
        self.spawn(async move {
            let mut found: Option<Lyrics> = None;
            if !server_negative {
                match api.lyrics_by_song_id(&track_id).await {
                    Ok(entries) => {
                        let raw: Vec<crate::lyrics::raw::StructuredLyrics> =
                            serde_json::to_value(&entries)
                                .ok()
                                .and_then(|v| serde_json::from_value(v).ok())
                                .unwrap_or_default();
                        match crate::lyrics::adapt_list(&track_id, &raw, LyricsSource::Server) {
                            Some(l) => {
                                let _ = caches.lyrics_put(&sid, &track_id, &l);
                                found = Some(l);
                            }
                            None => {
                                let _ = caches.lyrics_put_none(&sid, &track_id, LyricsSource::Server);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(target: "hocket_core", error = %e, "server lyrics");
                    }
                }
            }
            if found.is_none() && external_enabled {
                let cached_external = caches.lyrics_get(&sid, &track_id, LyricsSource::External);
                match cached_external {
                    Ok(Some(Some(l))) => found = Some(l),
                    Ok(Some(None)) if !force => {}
                    _ => {
                        if let Some(t) = &track {
                            let req = LyricsRequest {
                                track_id: track_id.clone(),
                                title: t.title.clone(),
                                artist: t.artist.clone(),
                                album: t.album.clone(),
                                duration_ms: Some(t.duration_ms).filter(|d| *d > 0),
                            };
                            let result = match http {
                                Some(h) => {
                                    let p = crate::lyrics::LrclibProvider::new(SharedHttp(h));
                                    p.fetch(&req).await
                                }
                                None => match crate::lyrics::ReqwestLyricsHttp::new(&app_version) {
                                    Ok(h) => crate::lyrics::LrclibProvider::new(h).fetch(&req).await,
                                    Err(e) => Err(e),
                                },
                            };
                            match result {
                                Ok(Some(l)) => {
                                    let _ = caches.lyrics_put(&sid, &track_id, &l);
                                    found = Some(l);
                                }
                                Ok(None) => {
                                    let _ = caches.lyrics_put_none(&sid, &track_id, LyricsSource::External);
                                }
                                Err(e) => {
                                    tracing::debug!(target: "hocket_core", error = %e, "external lyrics");
                                }
                            }
                        }
                    }
                }
            }
            let _ = tx.send(ActorMsg::Internal(Internal::LyricsFetched {
                server_id: sid,
                track_id,
                lyrics: found,
                announce,
            }));
        });
    }

    pub(crate) fn on_lyrics_fetched(
        &mut self,
        _server_id: ServerId,
        track_id: TrackId,
        lyrics: Option<Lyrics>,
        announce: bool,
    ) {
        if !announce {
            return;
        }
        let lyrics = lyrics.map(|mut l| {
            crate::lyrics::set_user_offset(&mut l, self.lyrics_offset(&track_id));
            l
        });
        self.emit(Event::LyricsChanged { track_id, lyrics });
    }

    // -- search ------------------------------------------------------------------------------

    /// Local batch now; the server batch follows as an event when wanted
    /// and the local results are thin.
    pub(crate) fn search(
        &mut self,
        server_id: ServerId,
        query: String,
        limit: u32,
        include_server: bool,
        request_id: String,
    ) -> SearchResults {
        let mut local = self
            .db
            .search(&server_id, &query, limit)
            .unwrap_or_else(|_| SearchResults {
                query: query.clone(),
                ..Default::default()
            });
        local.request_id = request_id.clone();
        local.from_server = false;
        self.emit(Event::SearchResults {
            results: local.clone(),
        });
        let thin = (local.tracks.len() as u32) < limit;
        let wanted = include_server
            && self
                .settings
                .get_bool(crate::settings::keys::SEARCH_INCLUDE_SERVER)
            && !query.trim().is_empty()
            && thin
            && !self
                .network
                .as_ref()
                .is_some_and(|n| n.kind == NetworkKind::Offline);
        if wanted {
            if let Some(api) = self.api() {
                let tx = self.tx.clone();
                let sid = server_id.clone();
                let username = self.server.as_ref().map(|s| s.info.username.clone());
                self.spawn(async move {
                    let page = crate::subsonic::Search3Page::all(limit.clamp(1, 100));
                    let Ok(r) = api.search3(&query, page).await else {
                        return;
                    };
                    let results = SearchResults {
                        request_id,
                        query,
                        tracks: r
                            .song
                            .iter()
                            .map(|c| summary_of(&track_from_child(&sid, c)))
                            .collect(),
                        albums: r.album.iter().map(|a| album_from_id3(&sid, a)).collect(),
                        artists: r.artist.iter().map(|a| artist_from_id3(&sid, a)).collect(),
                        playlists: vec![],
                        from_server: true,
                    };
                    let _ = username;
                    let _ = tx.send(ActorMsg::Internal(Internal::ServerSearch { results }));
                });
            }
        }
        local
    }

    // -- stats ---------------------------------------------------------------------------------

    pub(crate) fn play_rows(&self, since_ms: f64) -> Vec<crate::stats::PlayRow> {
        let rows: Vec<(String, f64, u32, bool, String)> = self
            .db
            .with_conn(|c| {
                let mut st = c.prepare_cached(
                    "SELECT track_id, played_at, played_ms, scrobbled, device_id FROM play_history WHERE played_at >= ?1 ORDER BY played_at DESC",
                )?;
                let rows = st.query_map([since_ms], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, f64>(1)?,
                        r.get::<_, i64>(2)?.max(0) as u32,
                        r.get::<_, i64>(3)? != 0,
                        r.get::<_, String>(4)?,
                    ))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap_or_default();
        let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
        let tracks: HashMap<String, Track> = self
            .db
            .tracks_by_ids(&ids)
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id.clone(), t))
            .collect();
        let album_ids: Vec<String> = tracks.values().filter_map(|t| t.album_id.clone()).collect();
        let artist_ids: Vec<String> = tracks
            .values()
            .filter_map(|t| t.artist_id.clone())
            .collect();
        let albums: HashMap<String, Album> = self
            .db
            .albums_by_ids(&album_ids)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        let artists: HashMap<String, Artist> = self
            .db
            .artists_by_ids(&artist_ids)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id.clone(), a))
            .collect();
        rows.into_iter()
            .map(|(track_id, played_at, played_ms, scrobbled, device_id)| {
                let track = tracks.get(&track_id);
                crate::stats::PlayRow {
                    track: track
                        .map(summary_of)
                        .unwrap_or_else(|| crate::core::handlers::session::bare_summary(&track_id)),
                    played_at,
                    played_ms,
                    scrobbled,
                    device_id,
                    album: track
                        .and_then(|t| t.album_id.as_ref())
                        .and_then(|id| albums.get(id).cloned()),
                    artist: track
                        .and_then(|t| t.artist_id.as_ref())
                        .and_then(|id| artists.get(id).cloned()),
                }
            })
            .collect()
    }
}

/// An `Arc<dyn LyricsHttp>` as a provider's HTTP.
struct SharedHttp(Arc<dyn crate::lyrics::LyricsHttp>);

impl crate::lyrics::LyricsHttp for SharedHttp {
    fn get(&self, url: &str) -> BoxFuture<'_, Result<String, crate::lyrics::LyricsError>> {
        self.0.get(url)
    }
}

fn rate_label(rating: u32, n: usize) -> String {
    let base = if rating == 0 {
        "Clear rating".to_string()
    } else {
        format!("Rate {rating} star{}", if rating == 1 { "" } else { "s" })
    };
    if n == 1 {
        base
    } else {
        format!("{base} ({n} items)")
    }
}

fn cas_target_key(m: &CasMutation) -> Option<String> {
    Some(match (&m.target, &m.expect) {
        (RemoteTarget::Track { id }, RemoteValue::Rating(_)) => format!("rating:track:{id}"),
        (RemoteTarget::Album { id }, RemoteValue::Rating(_)) => format!("rating:album:{id}"),
        (RemoteTarget::Track { id }, RemoteValue::Loved(_)) => format!("loved:track:{id}"),
        (RemoteTarget::Album { id }, RemoteValue::Loved(_)) => format!("loved:album:{id}"),
        (RemoteTarget::Artist { id }, RemoteValue::Loved(_)) => format!("loved:artist:{id}"),
        _ => return None,
    })
}

fn cas_target(m: &CasMutation) -> Option<CasTarget> {
    Some(match (&m.target, &m.expect) {
        (RemoteTarget::Track { id }, RemoteValue::Rating(_)) => {
            CasTarget::Rating(RatingTarget::Track { id: id.clone() })
        }
        (RemoteTarget::Album { id }, RemoteValue::Rating(_)) => {
            CasTarget::Rating(RatingTarget::Album { id: id.clone() })
        }
        (RemoteTarget::Track { id }, RemoteValue::Loved(_)) => {
            CasTarget::Loved(LoveTarget::Track { id: id.clone() })
        }
        (RemoteTarget::Album { id }, RemoteValue::Loved(_)) => {
            CasTarget::Loved(LoveTarget::Album { id: id.clone() })
        }
        (RemoteTarget::Artist { id }, RemoteValue::Loved(_)) => {
            CasTarget::Loved(LoveTarget::Artist { id: id.clone() })
        }
        _ => return None,
    })
}

fn cas_new(m: &CasMutation) -> Option<Prior> {
    match &m.command {
        Command::SetRating { rating, .. } => Some(Prior::Rating(*rating)),
        Command::SetLoved { loved, .. } => Some(Prior::Loved(*loved)),
        Command::SetArtistLoved { loved, .. } => Some(Prior::Loved(*loved)),
        _ => None,
    }
}

fn apply_cas_locally(db: &Db, target: &CasTarget, new: &Prior) {
    let _ = match (target, new) {
        (CasTarget::Rating(RatingTarget::Track { id }), Prior::Rating(r)) => {
            db.set_track_rating(id, *r)
        }
        (CasTarget::Rating(RatingTarget::Album { id }), Prior::Rating(r)) => {
            db.set_album_rating(id, *r)
        }
        (CasTarget::Loved(LoveTarget::Track { id }), Prior::Loved(l)) => db.set_track_loved(id, *l),
        (CasTarget::Loved(LoveTarget::Album { id }), Prior::Loved(l)) => db.set_album_loved(id, *l),
        (CasTarget::Loved(LoveTarget::Artist { id }), Prior::Loved(l)) => {
            db.set_artist_loved(id, *l)
        }
        _ => Ok(false),
    };
}

/// The autoplay chain's data source: the Subsonic client plus the mirror
/// for saved filters.
pub(crate) struct DbAutoplaySource {
    pub api: Arc<dyn SubsonicApi>,
    pub db: Db,
    pub server_id: String,
    pub now: f64,
    pub filters: Vec<Filter>,
}

fn net_err(e: crate::subsonic::SubsonicError) -> AutoplayError {
    match e {
        crate::subsonic::SubsonicError::Unsupported(_)
        | crate::subsonic::SubsonicError::NotFound(_) => AutoplayError::Unsupported,
        crate::subsonic::SubsonicError::Network(m) => AutoplayError::Network(m),
        other => AutoplayError::Server(other.to_string()),
    }
}

impl AutoplaySource for DbAutoplaySource {
    fn sonic_similar<'a>(
        &'a self,
        track_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<ScoredTrack>, AutoplayError>> {
        Box::pin(async move {
            if !self.api.capabilities().sonic_similarity {
                return Err(AutoplayError::Unsupported);
            }
            let list = self
                .api
                .sonic_similar_tracks(track_id, count)
                .await
                .map_err(net_err)?;
            Ok(list
                .into_iter()
                .map(|m| ScoredTrack {
                    track: summary_of(&track_from_child(&self.server_id, &m.entry)),
                    similarity: m.similarity,
                })
                .collect())
        })
    }

    fn similar_songs<'a>(
        &'a self,
        track_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
        Box::pin(async move {
            let list = self
                .api
                .similar_songs2(track_id, count)
                .await
                .map_err(net_err)?;
            Ok(list
                .iter()
                .map(|c| summary_of(&track_from_child(&self.server_id, c)))
                .collect())
        })
    }

    fn top_songs<'a>(
        &'a self,
        artist_name: &'a str,
        _artist_id: Option<&'a str>,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
        Box::pin(async move {
            let list = self
                .api
                .top_songs(artist_name, count)
                .await
                .map_err(net_err)?;
            Ok(list
                .iter()
                .map(|c| summary_of(&track_from_child(&self.server_id, c)))
                .collect())
        })
    }

    fn random_songs<'a>(
        &'a self,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
        Box::pin(async move {
            let list = self
                .api
                .random_songs(count, None, None, None)
                .await
                .map_err(net_err)?;
            Ok(list
                .iter()
                .map(|c| summary_of(&track_from_child(&self.server_id, c)))
                .collect())
        })
    }

    fn filter_tracks<'a>(
        &'a self,
        filter_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
        Box::pin(async move {
            let Some(filter) = self.filters.iter().find(|f| f.id == filter_id) else {
                return Err(AutoplayError::Other("unknown filter".into()));
            };
            let q = filters::select_for_node(
                Some(&filter.root),
                SortOrder::Random,
                false,
                Some(count),
                None,
                &self.server_id,
                "tracks.id",
                self.now,
            )
            .map_err(|e| AutoplayError::Other(e.to_string()))?;
            let ids: Vec<String> = self
                .db
                .with_conn(|c| {
                    let mut st = c.prepare(&q.sql)?;
                    let rows = st.query_map(rusqlite::params_from_iter(q.params.iter()), |r| {
                        r.get::<_, String>(0)
                    })?;
                    Ok(rows.collect::<Result<Vec<_>, _>>()?)
                })
                .map_err(|e| AutoplayError::Other(e.to_string()))?;
            self.db
                .summaries_by_ids(&ids)
                .map_err(|e| AutoplayError::Other(e.to_string()))
        })
    }
}
