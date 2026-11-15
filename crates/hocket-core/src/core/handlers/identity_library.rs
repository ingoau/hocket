//! The identity's library (see `crate::library`): the actor keeps the record
//! set, persists it under `library:{scope}`, hands every local change to the
//! engine (which syncs it) and applies it to the database: the dedup groups,
//! the Mixed playlists' rows and the Hocket library's stars. Remote copies
//! come back as `Output::LibraryRecordsMerged`.

use crate::api::{Event, SourceId, TrackRef};
use crate::connect::engine::Input;
use crate::core::actor::Actor;
use crate::db::links::LinkKind;
use crate::library::{self, links, LibraryRecord};

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
