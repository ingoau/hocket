//! Mixed playlists (`docs/providers.md`, Phase 3): playlists Hocket keeps in
//! the identity's library, whose songs may come from any account; and the
//! cross-account rules for provider playlists (another account's song goes
//! in as the playlist account's copy, or the playlist is offered "Convert to
//! Mixed").

use crate::api::{Command, PlaylistId, TrackRef};
use crate::core::actor::Actor;
use crate::library::{self, LibraryRecord, MixedPlaylist, HOCKET_SOURCE};

fn songs(n: usize) -> String {
    if n == 1 {
        "1 song".into()
    } else {
        format!("{n} songs")
    }
}

impl Actor {
    pub(crate) fn is_mixed_playlist(&self, playlist_id: &str) -> bool {
        library::playlist(&self.library_records, playlist_id).is_some()
    }

    /// `tracks` as `source`'s copies. `Err` lists the songs `source` has no
    /// copy of.
    pub(crate) fn copies_on(
        &self,
        source: &str,
        tracks: &[TrackRef],
    ) -> Result<Vec<TrackRef>, Vec<TrackRef>> {
        let mut out = vec![];
        let mut missing = vec![];
        for t in tracks {
            if t.source == source {
                out.push(t.clone());
                continue;
            }
            match self.copies_of(t).into_iter().find(|c| c.source == source) {
                Some(c) => out.push(c),
                None => missing.push(t.clone()),
            }
        }
        if missing.is_empty() {
            Ok(out)
        } else {
            Err(missing)
        }
    }

    fn account_label(&self, source: &str) -> String {
        self.accounts
            .for_source(source)
            .map(|a| a.info.name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "this account".into())
    }

    pub(crate) fn create_mixed_playlist(&mut self, name: String, tracks: Vec<TrackRef>) -> String {
        let now = self.now();
        let id = format!("mix-{}", uuid::Uuid::new_v4().simple());
        let p = MixedPlaylist {
            id: id.clone(),
            name,
            comment: None,
            tracks,
            created: now,
        };
        self.put_library_records(vec![LibraryRecord::playlist(p, now)]);
        id
    }

    /// Change a Mixed playlist in place.
    fn edit_mixed(&mut self, playlist_id: &str, f: impl FnOnce(&mut MixedPlaylist)) {
        let Some(mut p) = library::playlist(&self.library_records, playlist_id) else {
            return;
        };
        f(&mut p);
        let now = self.now();
        self.put_library_records(vec![LibraryRecord::playlist(p, now)]);
    }

    pub(crate) fn mixed_playlist_add(
        &mut self,
        playlist_id: &str,
        tracks: Vec<TrackRef>,
        at: Option<u32>,
    ) {
        self.edit_mixed(playlist_id, |p| {
            let at = at
                .map(|a| a as usize)
                .unwrap_or(p.tracks.len())
                .min(p.tracks.len());
            for (i, t) in tracks.into_iter().enumerate() {
                p.tracks.insert(at + i, t);
            }
        });
    }

    pub(crate) fn mixed_playlist_remove(&mut self, playlist_id: &str, mut indices: Vec<u32>) {
        indices.sort_unstable();
        indices.dedup();
        self.edit_mixed(playlist_id, |p| {
            for i in indices.into_iter().rev() {
                if (i as usize) < p.tracks.len() {
                    p.tracks.remove(i as usize);
                }
            }
        });
    }

    pub(crate) fn mixed_playlist_move(&mut self, playlist_id: &str, from: u32, to: u32) {
        self.edit_mixed(playlist_id, |p| {
            let (from, to) = (from as usize, to as usize);
            if from < p.tracks.len() {
                let t = p.tracks.remove(from);
                p.tracks.insert(to.min(p.tracks.len()), t);
            }
        });
    }

    pub(crate) fn mixed_playlist_rename(
        &mut self,
        playlist_id: &str,
        name: String,
        comment: Option<String>,
    ) {
        self.edit_mixed(playlist_id, |p| {
            p.name = name;
            if comment.is_some() {
                p.comment = comment;
            }
        });
    }

    pub(crate) fn mixed_playlist_delete(&mut self, playlist_id: &str) {
        let Some(p) = library::playlist(&self.library_records, playlist_id) else {
            return;
        };
        let now = self.now();
        let mut gone = LibraryRecord::playlist(p, now);
        gone.removed = true;
        self.put_library_records(vec![gone]);
    }

    /// A provider playlist can't hold songs its account lacks: offer to
    /// make it Mixed (dismissing the toast cancels).
    pub(crate) fn offer_convert_to_mixed(
        &mut self,
        playlist_id: PlaylistId,
        source: &str,
        missing: usize,
        add: Vec<TrackRef>,
    ) {
        let message = format!(
            "{} not on {}",
            if missing == 1 {
                "1 song is".to_string()
            } else {
                format!("{missing} songs are")
            },
            self.account_label(source)
        );
        self.toast(
            message,
            Some((
                "Convert to Mixed".into(),
                Command::ConvertPlaylistToMixed { playlist_id, add },
            )),
        );
    }

    /// `Command::ConvertPlaylistToMixed`: a Mixed playlist with the same
    /// name and songs (plus `add`); the provider's playlist is deleted.
    pub(crate) fn convert_playlist_to_mixed(
        &mut self,
        playlist_id: PlaylistId,
        add: Vec<TrackRef>,
    ) {
        if self.is_mixed_playlist(&playlist_id) {
            self.mixed_playlist_add(&playlist_id, add, None);
            return;
        }
        let Some(p) = self.db.playlist(&playlist_id).ok().flatten() else {
            return;
        };
        if p.is_smart {
            self.toast("Smart playlists are read-only", None);
            return;
        }
        // Its songs must be here first: converting deletes the provider's
        // playlist, so a list not yet fetched would lose them.
        if !self
            .db
            .playlist_tracks_synced(&playlist_id)
            .unwrap_or(false)
        {
            self.toast(
                format!(
                    "\u{201c}{}\u{201d} hasn't finished loading. Try again in a moment.",
                    p.name
                ),
                None,
            );
            return;
        }
        let mut tracks = self
            .db
            .playlist_track_refs(&playlist_id)
            .unwrap_or_default();
        tracks.extend(add);
        let n = tracks.len();
        self.create_mixed_playlist(p.name.clone(), tracks);
        self.delete_playlist(playlist_id);
        self.toast(
            format!("\u{201c}{}\u{201d} is Mixed now ({})", p.name, songs(n)),
            None,
        );
    }

    /// Where `CreatePlaylist` puts the playlist, and with which songs.
    /// `None` when it was handled here (Mixed, or refused with an offer).
    pub(crate) fn route_create_playlist(
        &mut self,
        account_id: &str,
        name: &str,
        tracks: Vec<TrackRef>,
    ) -> Option<(String, Vec<String>)> {
        if account_id == HOCKET_SOURCE {
            if name.trim().is_empty() {
                self.toast("Give the playlist a name", None);
                return None;
            }
            self.create_mixed_playlist(name.to_string(), tracks);
            self.toast(format!("Created \u{201c}{name}\u{201d}"), None);
            return None;
        }
        // No account named: the first song's, else the session's.
        let account = if account_id.is_empty() {
            tracks
                .first()
                .map(|t| t.source.clone())
                .or_else(|| self.scope_account())
                .unwrap_or_default()
        } else {
            account_id.to_string()
        };
        let source = self
            .accounts
            .get(&account)
            .map(|a| a.info.source.clone())
            .unwrap_or_else(|| account.clone());
        match self.copies_on(&source, &tracks) {
            Ok(copies) => Some((account, copies.into_iter().map(|t| t.id).collect())),
            Err(missing) => {
                let message = format!(
                    "{} not on {}",
                    if missing.len() == 1 {
                        "1 song is".to_string()
                    } else {
                        format!("{} songs are", missing.len())
                    },
                    self.account_label(&source)
                );
                self.toast(
                    message,
                    Some((
                        "Create as Mixed".into(),
                        Command::CreatePlaylist {
                            account_id: HOCKET_SOURCE.into(),
                            name: name.to_string(),
                            tracks,
                        },
                    )),
                );
                None
            }
        }
    }
}
