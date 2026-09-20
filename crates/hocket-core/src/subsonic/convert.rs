//! Raw wire types → `api` entity types.

use crate::api::{self, OfflineState};

use super::types::{
    parse_iso_ms, AlbumId3, ArtistId3, Child, GenreBody, PlaylistBody, ReplayGainBody,
};

fn replay_gain(rg: &Option<ReplayGainBody>) -> Option<api::ReplayGain> {
    let rg = rg.as_ref()?;
    if rg.track_gain.is_none()
        && rg.album_gain.is_none()
        && rg.track_peak.is_none()
        && rg.album_peak.is_none()
    {
        return None;
    }
    Some(api::ReplayGain {
        track_gain_db: rg.track_gain,
        track_peak: rg.track_peak,
        album_gain_db: rg.album_gain,
        album_peak: rg.album_peak,
    })
}

/// Server-reported modification time, if any (Navidrome varies the field name).
pub fn child_changed_ms(c: &Child) -> Option<f64> {
    c.changed
        .as_deref()
        .or(c.updated.as_deref())
        .and_then(parse_iso_ms)
}

pub fn album_changed_ms(a: &AlbumId3) -> Option<f64> {
    a.changed
        .as_deref()
        .or(a.updated.as_deref())
        .and_then(parse_iso_ms)
}

/// A song as the mirror stores it. `offline`, local counts and compilation
/// flags are filled in by the database layer, not here.
pub fn track_from_child(server_id: &str, c: &Child) -> api::Track {
    let genre = c
        .genre
        .clone()
        .or_else(|| c.genres.first().map(|g| g.name.clone()))
        .filter(|g| !g.is_empty());
    let album_artist = c
        .display_album_artist
        .clone()
        .or_else(|| c.album_artists.first().map(|a| a.name.clone()))
        .filter(|s| !s.is_empty());
    let artist = c
        .artist
        .clone()
        .or_else(|| c.display_artist.clone())
        .filter(|s| !s.is_empty());
    let sonic = if c.bpm.is_some_and(|b| b > 0.0) || !c.moods.is_empty() {
        Some(api::SonicAttributes {
            bpm: c.bpm.filter(|b| *b > 0.0),
            key: None,
            energy: None,
            mood: c.moods.first().cloned(),
            danceability: None,
            valence: None,
        })
    } else {
        None
    };
    api::Track {
        id: c.id.clone(),
        server_id: server_id.to_string(),
        title: c.title.clone(),
        album_id: c.album_id.clone().filter(|s| !s.is_empty()),
        album: c.album.clone().filter(|s| !s.is_empty()),
        artist_id: c.artist_id.clone().filter(|s| !s.is_empty()),
        artist,
        album_artist,
        track_number: c.track.filter(|n| *n > 0),
        disc_number: c.disc_number.filter(|n| *n > 0),
        year: c.year.filter(|n| *n > 0),
        genre,
        duration_ms: c
            .duration
            .map(|d| (d * 1000.0).round().max(0.0) as u32)
            .unwrap_or(0),
        bit_rate: c.bit_rate.filter(|n| *n > 0),
        sample_rate: c.sampling_rate.filter(|n| *n > 0),
        bit_depth: c.bit_depth.filter(|n| *n > 0),
        channels: c.channel_count.filter(|n| *n > 0),
        suffix: c.suffix.clone().filter(|s| !s.is_empty()),
        content_type: c.content_type.clone().filter(|s| !s.is_empty()),
        size_bytes: c.size.filter(|s| *s > 0.0),
        path: c.path.clone().filter(|s| !s.is_empty()),
        cover_art: c.cover_art.clone().filter(|s| !s.is_empty()),
        rating: c.user_rating.unwrap_or(0).min(5),
        loved: c.starred.as_deref().is_some_and(|s| !s.is_empty()),
        play_count: c.play_count.unwrap_or(0),
        last_played: c.played.as_deref().and_then(parse_iso_ms),
        created: c.created.as_deref().and_then(parse_iso_ms),
        replay_gain: replay_gain(&c.replay_gain),
        sonic,
        offline: OfflineState::None,
        music_brainz_id: c.music_brainz_id.clone().filter(|s| !s.is_empty()),
        explicit: c.explicit_status.as_deref() == Some("explicit"),
        comment: c.comment.clone().filter(|s| !s.is_empty()),
    }
}

pub fn album_from_id3(server_id: &str, a: &AlbumId3) -> api::Album {
    let genre = a
        .genre
        .clone()
        .or_else(|| a.genres.first().map(|g| g.name.clone()))
        .filter(|g| !g.is_empty());
    api::Album {
        id: a.id.clone(),
        server_id: server_id.to_string(),
        name: a.name.clone(),
        artist_id: a.artist_id.clone().filter(|s| !s.is_empty()),
        artist: a
            .artist
            .clone()
            .or_else(|| a.display_artist.clone())
            .filter(|s| !s.is_empty()),
        year: a.year.filter(|n| *n > 0),
        genre,
        song_count: a.song_count.unwrap_or(0),
        duration_ms: a
            .duration
            .map(|d| (d * 1000.0).round().max(0.0) as u32)
            .unwrap_or(0),
        cover_art: a.cover_art.clone().filter(|s| !s.is_empty()),
        rating: a.user_rating.unwrap_or(0).min(5),
        loved: a.starred.as_deref().is_some_and(|s| !s.is_empty()),
        play_count: a.play_count.unwrap_or(0),
        created: a.created.as_deref().and_then(parse_iso_ms),
        last_played: a.played.as_deref().and_then(parse_iso_ms),
        is_compilation: a.is_compilation.unwrap_or(false),
        music_brainz_id: a.music_brainz_id.clone().filter(|s| !s.is_empty()),
        replay_gain: replay_gain(&a.replay_gain),
        offline: OfflineState::None,
    }
}

pub fn artist_from_id3(server_id: &str, a: &ArtistId3) -> api::Artist {
    api::Artist {
        id: a.id.clone(),
        server_id: server_id.to_string(),
        name: a.name.clone(),
        album_count: a.album_count.unwrap_or(0),
        song_count: 0,
        cover_art: a.cover_art.clone().filter(|s| !s.is_empty()),
        artist_image_url: a.artist_image_url.clone().filter(|s| !s.is_empty()),
        loved: a.starred.as_deref().is_some_and(|s| !s.is_empty()),
        music_brainz_id: a.music_brainz_id.clone().filter(|s| !s.is_empty()),
        biography: None,
    }
}

/// `username` is the authenticated user, for `is_mine`.
pub fn playlist_from_body(
    server_id: &str,
    username: Option<&str>,
    p: &PlaylistBody,
) -> api::Playlist {
    api::Playlist {
        id: p.id.clone(),
        server_id: server_id.to_string(),
        name: p.name.clone(),
        comment: p.comment.clone().filter(|s| !s.is_empty()),
        owner: p.owner.clone().filter(|s| !s.is_empty()),
        public: p.public,
        song_count: p.song_count,
        duration_ms: (p.duration * 1000.0).round().max(0.0) as u32,
        cover_art: p.cover_art.clone().filter(|s| !s.is_empty()),
        created: p.created.as_deref().and_then(parse_iso_ms),
        changed: p.changed.as_deref().and_then(parse_iso_ms),
        is_smart: p.readonly.unwrap_or(false),
        is_mine: match (username, p.owner.as_deref()) {
            (Some(u), Some(o)) => u == o,
            _ => false,
        },
        offline: OfflineState::None,
    }
}

pub fn genre_from_body(g: &GenreBody) -> api::Genre {
    api::Genre {
        name: g.value.clone(),
        song_count: g.song_count,
        album_count: g.album_count,
    }
}

pub fn summary_of(t: &api::Track) -> api::TrackSummary {
    api::TrackSummary {
        id: t.id.clone(),
        server_id: t.server_id.clone(),
        title: t.title.clone(),
        artist: t.artist.clone(),
        album: t.album.clone(),
        album_id: t.album_id.clone(),
        artist_id: t.artist_id.clone(),
        duration_ms: t.duration_ms,
        cover_art: t.cover_art.clone(),
        rating: t.rating,
        loved: t.loved,
        offline: t.offline,
    }
}
