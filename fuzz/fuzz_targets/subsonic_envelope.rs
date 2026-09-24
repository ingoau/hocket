//! Subsonic response envelope → `types.rs` structs → `convert.rs` api types,
//! plus the Navidrome native API bodies and the ISO timestamp parser.
#![no_main]

use hocket_core::subsonic::client::parse_envelope;
use hocket_core::subsonic::convert::{
    album_changed_ms, album_from_id3, artist_from_id3, child_changed_ms, genre_from_body,
    playlist_from_body, summary_of, track_from_child,
};
use hocket_core::subsonic::types::{parse_iso_ms, AlbumId3, ArtistId3, Child, NativeLogin, NativePlaylist};
use libfuzzer_sys::fuzz_target;

fn song(c: &Child) {
    let t = track_from_child("s", c);
    assert!(t.rating <= 5);
    let _ = summary_of(&t);
    let _ = child_changed_ms(c);
}

fn album(a: &AlbumId3) {
    let al = album_from_id3("s", a);
    assert!(al.rating <= 5);
    let _ = album_changed_ms(a);
}

fn artist(a: &ArtistId3) {
    let _ = artist_from_id3("s", a);
}

fn iso(s: &str) {
    if let Some(ms) = parse_iso_ms(s) {
        assert!(ms.is_finite(), "{s:?} -> {ms}");
    }
}

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        iso(s);
    }
    let _ = serde_json::from_slice::<NativeLogin>(data);
    let _ = serde_json::from_slice::<NativePlaylist>(data);

    let Ok(r) = parse_envelope(data) else {
        return;
    };
    if let Some(idx) = &r.artists {
        idx.index.iter().flat_map(|i| &i.artist).for_each(artist);
    }
    if let Some(a) = &r.artist {
        a.album.iter().for_each(album);
    }
    if let Some(a) = &r.album {
        a.song.iter().for_each(song);
    }
    if let Some(l) = &r.album_list2 {
        l.album.iter().for_each(album);
    }
    if let Some(c) = &r.song {
        song(c);
    }
    for songs in [&r.random_songs, &r.songs_by_genre, &r.similar_songs2, &r.top_songs]
        .into_iter()
        .flatten()
    {
        songs.song.iter().for_each(song);
    }
    if let Some(g) = &r.genres {
        g.genre.iter().for_each(|g| {
            let _ = genre_from_body(g);
        });
    }
    if let Some(s) = &r.starred2 {
        s.artist.iter().for_each(artist);
        s.album.iter().for_each(album);
        s.song.iter().for_each(song);
    }
    if let Some(p) = &r.playlists {
        for body in &p.playlist {
            let pl = playlist_from_body("s", Some("me"), body);
            let _ = playlist_from_body("s", None, body);
            let _ = pl.is_mine;
        }
    }
    if let Some(p) = &r.playlist {
        p.entry.iter().for_each(song);
    }
    if let Some(s) = &r.search_result3 {
        s.artist.iter().for_each(artist);
        s.album.iter().for_each(album);
        s.song.iter().for_each(song);
    }
    if let Some(i) = &r.artist_info2 {
        i.similar_artist.iter().for_each(artist);
    }
    if let Some(q) = &r.play_queue {
        q.entry.iter().for_each(song);
    }
    if let Some(s) = &r.scan_status {
        if let Some(t) = &s.last_scan {
            iso(t);
        }
    }
});
