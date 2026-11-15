//! The identity's library (see `crate::library`): the actor keeps the record
//! set, persists it under `library:{scope}`, hands every local change to the
//! engine (which syncs it) and applies it to the database: the dedup groups,
//! the Mixed playlists' rows and the Hocket library's stars. Remote copies
//! come back as `Output::LibraryRecordsMerged`.

use crate::api::{
    AccountKind, Event, OfflineState, QueueItem, SourceId, Track, TrackRef, TrackSource,
};
use crate::connect::engine::Input;
use crate::core::actor::Actor;
use crate::db::links::LinkKind;
use crate::library::{self, links, LibraryRecord};
use crate::session::QueueOp;

impl Actor {
    /// Load the identity's records. Called once its scope is known.
    pub(crate) fn load_library_records(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        self.library_records = match self
            .db
            .saved_state_get::<Vec<LibraryRecord>>(&library::state_key(&scope))
        {
            Ok(list) => list.unwrap_or_default(),
            Err(e) => {
                self.log(
                    "warn",
                    format!("library records: unreadable, starting empty: {e}"),
                );
                vec![]
            }
        };
        let now = self.now();
        library::prune(&mut self.library_records, now);
        if !self.library_records.is_empty() {
            self.engine_input(Input::LibraryRecordsChanged(self.library_records.clone()));
        }
        self.apply_library_records();
    }

    fn save_library_records(&mut self) {
        let Some(scope) = self.scope.clone() else {
            return;
        };
        let now = self.now();
        library::prune(&mut self.library_records, now);
        if let Err(e) = self.db.saved_state_set(
            &library::state_key(&scope),
            &self.library_records,
            self.clock.as_ref(),
        ) {
            self.log("warn", format!("library records: couldn't save: {e}"));
        }
    }

    /// Put records made here, then save, sync and apply them.
    pub(crate) fn put_library_records(&mut self, records: Vec<LibraryRecord>) {
        let now = self.now();
        for r in records {
            library::put(&mut self.library_records, r, now);
        }
        self.save_library_records();
        self.engine_input(Input::LibraryRecordsChanged(self.library_records.clone()));
        self.apply_library_records();
    }

    pub(crate) fn on_library_records_merged(&mut self, list: Vec<LibraryRecord>) {
        let (mut merged, _) = library::merge(&self.library_records, &list);
        library::prune(&mut merged, self.now());
        if merged != self.library_records {
            self.library_records = merged;
            self.save_library_records();
            self.apply_library_records();
        }
    }

    /// Bring the database in line with the records: groups, Mixed
    /// playlists, Hocket library stars.
    pub(crate) fn apply_library_records(&mut self) {
        self.relink();
        let playlists = library::playlists(&self.library_records);
        if let Err(e) = self.db.replace_mixed_playlists(&playlists) {
            self.log("warn", format!("mixed playlists: {e}"));
        }
        self.apply_saved_tracks();
        self.emit(Event::LibraryChanged {
            server_id: String::new(),
            tables: vec!["playlists".into(), "tracks".into()],
            ids: vec![],
        });
        self.emit_queue();
    }

    /// The account order, as the matcher ranks sources.
    pub(crate) fn source_priority(&self) -> Vec<SourceId> {
        self.accounts
            .iter()
            .map(|a| a.info.source.clone())
            .collect()
    }

    /// Rebuild the dedup groups from the mirror and the manual links. With
    /// one account and no manual links there is nothing to group.
    pub(crate) fn relink(&mut self) {
        let manual = library::manual_links(&self.library_records);
        let priority = self.source_priority();
        if priority.len() < 2 && manual.is_empty() {
            if self.db.has_links().unwrap_or(false) {
                let _ = self.db.replace_links(&[], &[], &[]);
            }
            self.cache.links_present = false;
            return;
        }
        let started = std::time::Instant::now();
        let songs = self.db.song_facts().unwrap_or_default();
        let groups = links::group_songs(&songs, &manual, &priority);
        let (albums, artists) = if priority.len() < 2 {
            (vec![], vec![])
        } else {
            (
                links::group_entities(
                    &self.db.entity_facts(LinkKind::Album).unwrap_or_default(),
                    &priority,
                ),
                links::group_entities(
                    &self.db.entity_facts(LinkKind::Artist).unwrap_or_default(),
                    &priority,
                ),
            )
        };
        if let Err(e) = self.db.replace_links(&groups, &albums, &artists) {
            self.log("warn", format!("dedup: {e}"));
            return;
        }
        self.cache.links_present = !groups.is_empty();
        self.cache.alts_checked.clear();
        tracing::debug!(
            target: "hocket_core",
            songs = songs.len(),
            groups = groups.len(),
            ms = started.elapsed().as_millis() as u64,
            "dedup"
        );
    }

    /// Every copy of a song, this one included, representative first.
    pub(crate) fn copies_of(&self, track: &TrackRef) -> Vec<TrackRef> {
        let members = self.db.group_members(track).unwrap_or_default();
        if members.is_empty() {
            return vec![track.clone()];
        }
        members.into_iter().map(|m| m.track).collect()
    }

    /// The copy of a queue item's song to play here: a pinned or cached
    /// copy first, then the highest-priority account that has one, else
    /// the item's own reference (which may be unavailable here).
    pub(crate) fn track_for_item(&self, item: &QueueItem) -> Option<Track> {
        self.best_copy(&item.track, &item.alts)
    }

    /// [`Actor::track_for_item`] for a song and the other copies known for it.
    pub(crate) fn best_copy(&self, track: &TrackRef, alts: &[TrackRef]) -> Option<Track> {
        if !self.cache.links_present && alts.is_empty() {
            return self.track_or_bare(track);
        }
        let mut candidates: Vec<TrackRef> = vec![];
        for r in std::iter::once(track).chain(alts.iter()) {
            for c in self.copies_of(r) {
                if !candidates.contains(&c) {
                    candidates.push(c);
                }
            }
        }
        if candidates.len() < 2 {
            return self.track_or_bare(track);
        }
        let priority = self.source_priority();
        let rank = |t: &Track| {
            priority
                .iter()
                .position(|s| s == &t.server_id)
                .unwrap_or(priority.len())
        };
        let mut playable: Vec<Track> = self
            .db
            .tracks_by_refs(&candidates)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| {
                self.provider_for(&t.server_id).is_some() || t.offline != OfflineState::None
            })
            .collect();
        playable.sort_by_key(|t| {
            (
                !matches!(t.offline, OfflineState::Downloaded | OfflineState::Cached),
                rank(t),
            )
        });
        playable
            .into_iter()
            .next()
            .or_else(|| self.track_or_bare(track))
    }

    /// Note other copies of queue items' songs on the document, so a device
    /// without an item's account can still play it. Each key is looked at
    /// once per set of groups.
    pub(crate) fn offer_alternates(&mut self) {
        if !self.cache.links_present {
            return;
        }
        let Some(doc) = self.doc() else {
            return;
        };
        let items: Vec<QueueItem> = doc
            .history
            .iter()
            .chain(doc.current.iter())
            .chain(doc.upcoming.iter())
            .filter(|i| !self.cache.alts_checked.contains(&i.key))
            .cloned()
            .collect();
        let mut alts = vec![];
        for item in items {
            self.cache.alts_checked.insert(item.key.clone());
            let mut copies: Vec<TrackRef> = self
                .copies_of(&item.track)
                .into_iter()
                .filter(|c| c != &item.track)
                .collect();
            for a in &item.alts {
                if !copies.contains(a) && a != &item.track {
                    copies.push(a.clone());
                }
            }
            if copies != item.alts && !copies.is_empty() {
                alts.push((item.key.clone(), copies));
            }
        }
        if !alts.is_empty() {
            self.local_reduced_op(QueueOp::SetAlternates { alts }, |_| true);
        }
    }

    /// Star or add songs in the Hocket library (`None` leaves that flag).
    pub(crate) fn save_to_hocket_library(
        &mut self,
        tracks: Vec<TrackRef>,
        starred: Option<bool>,
        added: Option<bool>,
    ) {
        let now = self.now();
        let mut records = vec![];
        for t in tracks {
            let mut entry = library::saved_entry(&self.library_records, &t).unwrap_or_else(|| {
                library::SavedTrack {
                    track: t.clone(),
                    ..Default::default()
                }
            });
            if let Some(s) = starred {
                entry.starred = s;
            }
            if let Some(a) = added {
                entry.added = a;
            }
            if entry.snapshot.is_none() {
                entry.snapshot = self.db.track_ref(&t).ok().flatten();
            }
            if let Some(s) = starred {
                let _ = self.db.set_loved_ref(&t, s);
            }
            records.push(LibraryRecord::saved(entry, now));
        }
        self.put_library_records(records);
    }

    /// `Command::SetInLibrary`: adds for accounts that keep no library of
    /// their own go to the Hocket library. Navidrome's library is the
    /// server's files, so there is nothing to add there.
    pub(crate) fn set_in_library(&mut self, tracks: Vec<TrackRef>, added: bool) {
        let hocket: Vec<TrackRef> = tracks
            .into_iter()
            .filter(|t| {
                self.accounts.for_source(&t.source).is_some_and(|a| {
                    !a.info.features.library_edit && a.info.kind != AccountKind::Navidrome
                })
            })
            .collect();
        if hocket.is_empty() {
            return;
        }
        let n = hocket.len();
        self.save_to_hocket_library(hocket, None, Some(added));
        let what = if n == 1 {
            "1 song".to_string()
        } else {
            format!("{n} songs")
        };
        self.toast(
            &if added {
                format!("Added {what} to your library")
            } else {
                format!("Removed {what} from your library")
            },
            None,
        );
    }

    /// "Link to…": `track` and `to` are the same song.
    pub(crate) fn link_tracks(&mut self, track: TrackRef, to: TrackRef) {
        if track == to {
            return;
        }
        let now = self.now();
        self.put_library_records(vec![LibraryRecord::link(track, to, true, now)]);
        self.toast("Linked: one song, two sources", None);
    }

    /// "Not the same song": `track` leaves its group, and the matcher won't
    /// put it back with any of the others.
    pub(crate) fn unlink_track(&mut self, track: TrackRef) {
        let others: Vec<TrackRef> = self
            .copies_of(&track)
            .into_iter()
            .filter(|c| c != &track)
            .collect();
        if others.is_empty() {
            return;
        }
        let now = self.now();
        let records = others
            .into_iter()
            .map(|o| LibraryRecord::link(track.clone(), o, false, now))
            .collect();
        self.put_library_records(records);
        self.toast("Unlinked", None);
    }

    /// Every copy of a song, as the track details list its sources.
    pub(crate) fn track_sources(&self, track: &TrackRef) -> Vec<TrackSource> {
        let members = self.db.group_members(track).unwrap_or_default();
        let members: Vec<(TrackRef, bool)> = if members.is_empty() {
            vec![(track.clone(), false)]
        } else {
            members.into_iter().map(|m| (m.track, m.manual)).collect()
        };
        let preferred = self
            .best_copy(track, &[])
            .map(|t| TrackRef::new(t.server_id, t.id));
        members
            .into_iter()
            .map(|(r, manual)| {
                let account = self.accounts.for_source(&r.source);
                TrackSource {
                    track: self.track_or_bare(&r).unwrap_or_default(),
                    account_id: account
                        .map(|a| a.info.id.clone())
                        .unwrap_or_else(|| r.source.clone()),
                    account_name: account.map(|a| a.info.name.clone()).unwrap_or_default(),
                    available: account.is_some(),
                    manual,
                    preferred: preferred.as_ref() == Some(&r),
                }
            })
            .collect()
    }

    /// Stars kept in the Hocket library show on the mirror's rows.
    fn apply_saved_tracks(&mut self) {
        for s in library::saved(&self.library_records) {
            if !s.starred {
                continue;
            }
            if self.db.track_ref(&s.track).ok().flatten().is_none() {
                // Known only from the record: list it from its snapshot, when
                // this device has the account.
                let Some(mut t) = s.snapshot.clone() else {
                    continue;
                };
                if self.accounts.for_source(&s.track.source).is_none() {
                    continue;
                }
                t.id = s.track.id.clone();
                t.server_id = s.track.source.clone();
                t.loved = true;
                let _ = self.db.upsert_tracks(&[t], &[None], 0);
                continue;
            }
            let _ = self.db.set_loved_ref(&s.track, true);
        }
    }
}
