//! The `links` table (dedup groups) and the Mixed playlists' rows.

use rusqlite::{params, OptionalExtension};

use super::{Db, DbResult};
use crate::api::{SourceId, TrackRef};
use crate::library::links::{EntityFacts, EntityGroup, Group, SongFacts};
use crate::library::{MixedPlaylist, HOCKET_SOURCE};

/// What a dedup group is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Track,
    Album,
    Artist,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Track => "track",
            LinkKind::Album => "album",
            LinkKind::Artist => "artist",
        }
    }
}

/// One member of a song's group, as `Db::group_members` reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedCopy {
    pub track: TrackRef,
    pub manual: bool,
}

/// SQL that is true for a row the unified library leaves out: a member of a
/// group that isn't its representative. `{alias}` names the row's table.
pub fn hidden_copy(kind: LinkKind, alias: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM links l WHERE l.kind = '{}' AND l.source = {alias}.server_id AND l.id = {alias}.id AND l.rep = 0)",
        kind.as_str()
    )
}

impl Db {
    /// Every song in the mirror, as the matcher reads it.
    pub fn song_facts(&self) -> DbResult<Vec<SongFacts>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT server_id, id, title, artist, album, duration_ms, explicit, isrc FROM tracks",
            )?;
            let rows = st.query_map([], |r| {
                Ok(SongFacts {
                    track: TrackRef::new(r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                    title: r.get(2)?,
                    artist: r.get(3)?,
                    album: r.get(4)?,
                    duration_ms: r.get::<_, i64>(5)?.max(0) as u32,
                    explicit: r.get::<_, i64>(6)? != 0,
                    isrc: r.get(7)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Every album (`Album`) or artist (`Artist`) in the mirror.
    pub fn entity_facts(&self, kind: LinkKind) -> DbResult<Vec<EntityFacts>> {
        let sql = match kind {
            LinkKind::Album => "SELECT server_id, id, name, artist, year FROM albums",
            LinkKind::Artist => "SELECT server_id, id, name, NULL, NULL FROM artists",
            LinkKind::Track => return Ok(vec![]),
        };
        self.with_conn(|c| {
            let mut st = c.prepare_cached(sql)?;
            let rows = st.query_map([], |r| {
                Ok(EntityFacts {
                    source: r.get(0)?,
                    id: r.get(1)?,
                    name: r.get(2)?,
                    artist: r.get(3)?,
                    year: r.get::<_, Option<i64>>(4)?.map(|y| y.max(0) as u32),
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Replace every group with the matcher's answer.
    pub fn replace_links(
        &self,
        songs: &[Group],
        albums: &[EntityGroup],
        artists: &[EntityGroup],
    ) -> DbResult<()> {
        self.with_tx(|tx| {
            tx.execute("DELETE FROM links", [])?;
            let mut st = tx.prepare_cached(
                "INSERT OR REPLACE INTO links(kind, source, id, group_id, rep, manual) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for g in songs {
                for (i, m) in g.members.iter().enumerate() {
                    st.execute(params![
                        LinkKind::Track.as_str(),
                        m.track.source,
                        m.track.id,
                        g.id,
                        (i == 0) as i64,
                        m.manual as i64
                    ])?;
                }
            }
            for (kind, groups) in [(LinkKind::Album, albums), (LinkKind::Artist, artists)] {
                for g in groups {
                    for (i, (source, id)) in g.members.iter().enumerate() {
                        st.execute(params![kind.as_str(), source, id, g.id, (i == 0) as i64, 0])?;
                    }
                }
            }
            Ok(())
        })
    }

    pub fn has_links(&self) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT 1 FROM links LIMIT 1", [], |_| Ok(()))
                .optional()?
                .is_some())
        })
    }

    /// The copies of a song: every member of its group, representative
    /// first. Empty when the song is in no group.
    pub fn group_members(&self, track: &TrackRef) -> DbResult<Vec<LinkedCopy>> {
        self.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT l.source, l.id, l.manual FROM links l
                 WHERE l.kind = 'track' AND l.group_id = (SELECT group_id FROM links WHERE kind = 'track' AND source = ?1 AND id = ?2)
                 ORDER BY l.rep DESC, l.source, l.id",
            )?;
            let rows = st.query_map([&track.source, &track.id], |r| {
                Ok(LinkedCopy {
                    track: TrackRef::new(r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                    manual: r.get::<_, i64>(2)? != 0,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// The members of an album's or artist's group (`(source, id)`),
    /// representative first; just the one when it is in no group.
    pub fn entity_members(
        &self,
        kind: LinkKind,
        source: &str,
        id: &str,
    ) -> DbResult<Vec<(SourceId, String)>> {
        let mut out: Vec<(SourceId, String)> = self.with_conn(|c| {
            let mut st = c.prepare_cached(
                "SELECT l.source, l.id FROM links l
                 WHERE l.kind = ?1 AND l.group_id = (SELECT group_id FROM links WHERE kind = ?1 AND source = ?2 AND id = ?3)
                 ORDER BY l.rep DESC, l.source, l.id",
            )?;
            let rows = st.query_map(params![kind.as_str(), source, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })?;
        if out.is_empty() {
            out.push((source.to_string(), id.to_string()));
        }
        Ok(out)
    }

    /// Star or unstar one copy of a song.
    pub fn set_loved_ref(&self, track: &TrackRef, loved: bool) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE tracks SET loved = ?3 WHERE server_id = ?1 AND id = ?2",
                params![track.source, track.id, loved as i64],
            )? > 0)
        })
    }

    /// Rate one copy of a song.
    pub fn set_rating_ref(&self, track: &TrackRef, rating: u32) -> DbResult<bool> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE tracks SET rating = ?3 WHERE server_id = ?1 AND id = ?2",
                params![track.source, track.id, rating.min(5)],
            )? > 0)
        })
    }

    /// Replace the Mixed playlists' rows with `playlists`.
    pub fn replace_mixed_playlists(&self, playlists: &[MixedPlaylist]) -> DbResult<()> {
        self.with_tx(|tx| {
            tx.execute("DELETE FROM playlist_tracks WHERE server_id = ?1", [HOCKET_SOURCE])?;
            tx.execute("DELETE FROM playlists WHERE server_id = ?1", [HOCKET_SOURCE])?;
            let mut pl = tx.prepare_cached(
                "INSERT INTO playlists(id, server_id, name, comment, owner, public, song_count, duration_ms, cover_art, created, changed, is_smart, is_mine, tracks_synced)
                 VALUES (?1, ?2, ?3, ?4, NULL, 0, ?5,
                   (SELECT COALESCE(SUM(t.duration_ms), 0) FROM json_each(?6) j JOIN tracks t ON t.server_id = json_extract(j.value, '$.source') AND t.id = json_extract(j.value, '$.id')),
                   (SELECT t.cover_art FROM json_each(?6) j JOIN tracks t ON t.server_id = json_extract(j.value, '$.source') AND t.id = json_extract(j.value, '$.id') WHERE t.cover_art IS NOT NULL ORDER BY j.key LIMIT 1),
                   ?7, ?7, 0, 1, 1)",
            )?;
            let mut tr = tx.prepare_cached(
                "INSERT INTO playlist_tracks(server_id, playlist_id, position, track_id, track_source) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for p in playlists {
                let refs = serde_json::to_string(&p.tracks)?;
                pl.execute(params![
                    p.id,
                    HOCKET_SOURCE,
                    p.name,
                    p.comment,
                    p.tracks.len() as i64,
                    refs,
                    p.created
                ])?;
                for (i, t) in p.tracks.iter().enumerate() {
                    tr.execute(params![HOCKET_SOURCE, p.id, i as i64, t.id, t.source])?;
                }
            }
            Ok(())
        })
    }
}
