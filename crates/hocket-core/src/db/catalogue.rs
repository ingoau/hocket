//! The live catalogue (`catalogue` table): songs, albums and artists a
//! provider answered live (search, album and artist pages, home, radio),
//! kept so they show without the network. Lookups of one track fall back to
//! it when the mirror lacks the track; list queries never read it.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::{Db, DbResult};
use crate::api::{Album, Artist, Track};

/// The most rows kept per kind; the oldest go first (pinned ones are kept
/// on top).
const KEEP: i64 = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogueAlbum {
    pub album: Album,
    pub tracks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CatalogueArtist {
    pub artist: Artist,
    pub albums: Vec<Album>,
}

/// Keep `kind`'s newest `keep` rows. Downloaded songs and albums stay on
/// top: the catalogue is all a pinned item a provider answered live is known
/// from.
fn trim(tx: &rusqlite::Connection, kind: &str, keep: i64) -> DbResult<()> {
    tx.execute(
        "DELETE FROM catalogue WHERE kind = ?1 AND rowid IN (
            SELECT rowid FROM catalogue c WHERE kind = ?1
              AND NOT EXISTS (SELECT 1 FROM pin_tracks p WHERE ?1 = 'track' AND p.server_id = c.server_id AND p.track_id = c.id)
              AND NOT EXISTS (SELECT 1 FROM pins p WHERE p.server_id = c.server_id AND p.target_kind = ?1 AND p.target_id = c.id)
            ORDER BY fetched_at DESC LIMIT -1 OFFSET ?2)",
        params![kind, keep],
    )?;
    Ok(())
}

impl Db {
    fn catalogue_put(
        &self,
        kind: &str,
        rows: &[(String, String, String)],
        now: f64,
    ) -> DbResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        self.with_tx(|tx| {
            let mut st = tx.prepare_cached(
                "INSERT INTO catalogue(kind, server_id, id, json, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, server_id, id) DO UPDATE SET json = excluded.json, fetched_at = excluded.fetched_at",
            )?;
            for (source, id, json) in rows {
                st.execute(params![kind, source, id, json, now])?;
            }
            trim(tx, kind, KEEP)?;
            Ok(())
        })
    }

    fn catalogue_get<T: for<'de> Deserialize<'de>>(
        &self,
        kind: &str,
        id: &str,
    ) -> DbResult<Option<T>> {
        let json: Option<String> = self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT json FROM catalogue WHERE kind = ?1 AND id = ?2 ORDER BY fetched_at DESC LIMIT 1",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()?)
        })?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub fn catalogue_put_tracks(&self, tracks: &[Track], now: f64) -> DbResult<()> {
        let rows: Vec<(String, String, String)> = tracks
            .iter()
            .filter_map(|t| {
                Some((
                    t.server_id.clone(),
                    t.id.clone(),
                    serde_json::to_string(t).ok()?,
                ))
            })
            .collect();
        self.catalogue_put("track", &rows, now)
    }

    pub fn catalogue_put_album(&self, album: &Album, tracks: &[Track], now: f64) -> DbResult<()> {
        self.catalogue_put_tracks(tracks, now)?;
        let a = CatalogueAlbum {
            album: album.clone(),
            tracks: tracks.iter().map(|t| t.id.clone()).collect(),
        };
        self.catalogue_put(
            "album",
            &[(
                album.server_id.clone(),
                album.id.clone(),
                serde_json::to_string(&a)?,
            )],
            now,
        )
    }

    /// Albums without their tracks (search results, an artist's list).
    pub fn catalogue_put_albums(&self, albums: &[Album], now: f64) -> DbResult<()> {
        let rows: Vec<(String, String, String)> = albums
            .iter()
            .filter(|a| {
                self.catalogue_album(&a.id)
                    .ok()
                    .flatten()
                    .is_none_or(|c| c.tracks.is_empty())
            })
            .filter_map(|a| {
                let c = CatalogueAlbum {
                    album: a.clone(),
                    tracks: vec![],
                };
                Some((
                    a.server_id.clone(),
                    a.id.clone(),
                    serde_json::to_string(&c).ok()?,
                ))
            })
            .collect();
        self.catalogue_put("album", &rows, now)
    }

    pub fn catalogue_put_artist(
        &self,
        artist: &Artist,
        albums: &[Album],
        now: f64,
    ) -> DbResult<()> {
        let a = CatalogueArtist {
            artist: artist.clone(),
            albums: albums.to_vec(),
        };
        self.catalogue_put(
            "artist",
            &[(
                artist.server_id.clone(),
                artist.id.clone(),
                serde_json::to_string(&a)?,
            )],
            now,
        )
    }

    pub fn catalogue_track(&self, id: &str) -> DbResult<Option<Track>> {
        self.catalogue_get("track", id)
    }

    pub fn catalogue_album(&self, id: &str) -> DbResult<Option<CatalogueAlbum>> {
        self.catalogue_get("album", id)
    }

    /// An album's tracks as the catalogue knows them (mirror rows first).
    pub fn catalogue_album_tracks(&self, id: &str) -> DbResult<Vec<Track>> {
        let Some(a) = self.catalogue_album(id)? else {
            return Ok(vec![]);
        };
        let mut out = vec![];
        for t in &a.tracks {
            let r = crate::api::TrackRef::new(a.album.server_id.clone(), t.clone());
            if let Some(t) = self.track_ref(&r)? {
                out.push(t);
            }
        }
        Ok(out)
    }

    pub fn catalogue_artist(&self, id: &str) -> DbResult<Option<CatalogueArtist>> {
        self.catalogue_get("artist", id)
    }

    /// A catalogue track named by reference.
    pub(crate) fn catalogue_track_ref(&self, source: &str, id: &str) -> DbResult<Option<Track>> {
        Ok(self.catalogue_track(id)?.filter(|t| t.server_id == source))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_tracks_answer_lookups_but_not_the_library() {
        let db = Db::open_in_memory().unwrap();
        let t = Track {
            id: "vid".into(),
            server_id: "ytm".into(),
            title: "Song".into(),
            ..Default::default()
        };
        let album = Album {
            id: "MPREb_x".into(),
            server_id: "ytm".into(),
            name: "Album".into(),
            ..Default::default()
        };
        db.catalogue_put_album(&album, std::slice::from_ref(&t), 1.0)
            .unwrap();
        let r = crate::api::TrackRef::new("ytm", "vid");
        assert_eq!(db.track_ref(&r).unwrap().unwrap().title, "Song");
        assert_eq!(
            db.tracks_by_refs(std::slice::from_ref(&r)).unwrap().len(),
            1
        );
        assert!(db
            .track_ref(&crate::api::TrackRef::new("other", "vid"))
            .unwrap()
            .is_none());
        assert_eq!(db.catalogue_album_tracks("MPREb_x").unwrap()[0].id, "vid");
        assert_eq!(
            db.track_count("ytm", &None).unwrap(),
            0,
            "not in the library"
        );
        // A bare album list doesn't forget the tracks a page brought.
        db.catalogue_put_albums(std::slice::from_ref(&album), 2.0)
            .unwrap();
        assert_eq!(
            db.catalogue_album("MPREb_x").unwrap().unwrap().tracks.len(),
            1
        );
    }

    #[test]
    fn the_trim_keeps_pinned_songs_and_albums() {
        let db = Db::open_in_memory().unwrap();
        let track = |id: &str| Track {
            id: id.into(),
            server_id: "ytm".into(),
            title: id.into(),
            ..Default::default()
        };
        let album = |id: &str| Album {
            id: id.into(),
            server_id: "ytm".into(),
            name: id.into(),
            ..Default::default()
        };
        db.catalogue_put_album(&album("old-al"), &[track("old-pinned")], 1.0)
            .unwrap();
        db.catalogue_put_tracks(&[track("old")], 1.0).unwrap();
        db.catalogue_put_albums(&[album("old-al2")], 1.0).unwrap();
        db.catalogue_put_tracks(&[track("new")], 2.0).unwrap();
        db.catalogue_put_albums(&[album("new-al")], 2.0).unwrap();
        db.with_tx(|tx| {
            tx.execute(
                "INSERT INTO pins(server_id, target_kind, target_id, label, created_at) VALUES ('ytm', 'album', 'old-al', 'A', 1)",
                [],
            )?;
            tx.execute(
                "INSERT INTO pin_tracks(server_id, target_kind, target_id, track_id, state) VALUES ('ytm', 'album', 'old-al', 'old-pinned', 'done')",
                [],
            )?;
            trim(tx, "track", 1)?;
            trim(tx, "album", 1)
        })
        .unwrap();
        assert!(db.catalogue_track("old-pinned").unwrap().is_some());
        assert!(db.catalogue_track("new").unwrap().is_some());
        assert!(db.catalogue_track("old").unwrap().is_none());
        assert!(db.catalogue_album("old-al").unwrap().is_some());
        assert!(db.catalogue_album("new-al").unwrap().is_some());
        assert!(db.catalogue_album("old-al2").unwrap().is_none());
    }
}
