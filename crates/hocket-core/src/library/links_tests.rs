//! Dedup fixtures: versions stay apart, true duplicates merge.

use super::*;

fn song(source: &str, id: &str, title: &str, artist: &str, album: &str, secs: f64) -> SongFacts {
    SongFacts {
        track: TrackRef::new(source, id),
        title: title.into(),
        artist: Some(artist.into()),
        album: Some(album.into()),
        duration_ms: (secs * 1000.0) as u32,
        explicit: false,
        isrc: None,
    }
}

fn prio() -> Vec<SourceId> {
    vec!["a".into(), "b".into(), "c".into()]
}

/// The groups as `[[source:id, …], …]`, representative first.
fn grouped(songs: &[SongFacts], manual: &[ManualLink]) -> Vec<Vec<String>> {
    group_songs(songs, manual, &prio())
        .into_iter()
        .map(|g| {
            g.members
                .iter()
                .map(|m| format!("{}:{}", m.track.source, m.track.id))
                .collect()
        })
        .collect()
}

fn merged(x: SongFacts, y: SongFacts) -> bool {
    !grouped(&[x, y], &[]).is_empty()
}

#[test]
fn true_duplicates_merge() {
    let x = song(
        "a",
        "1",
        "Bohemian Rhapsody",
        "Queen",
        "A Night at the Opera",
        354.0,
    );
    let y = song(
        "b",
        "9",
        "Bohemian Rhapsody",
        "Queen",
        "A Night at the Opera",
        355.5,
    );
    assert_eq!(
        grouped(&[y.clone(), x.clone()], &[]),
        vec![vec!["a:1", "b:9"]]
    );
}

#[test]
fn spelling_noise_is_folded() {
    let pairs = [
        (
            "Don’t Stop Me Now",
            "Queen",
            "Jazz",
            "Don't stop me now",
            "QUEEN",
            "Jazz",
        ),
        (
            "Café Del Mar",
            "Energy 52",
            "Café",
            "Cafe del Mar",
            "Energy 52",
            "Cafe",
        ),
        ("Song [Mono]", "A", "Album", "Song (Mono)", "A", "Album"),
        (
            "Rock & Roll",
            "Led Zeppelin",
            "IV",
            "Rock and Roll",
            "Led Zeppelin",
            "IV",
        ),
        ("Hey Jude", "The Beatles", "1", "Hey Jude", "Beatles", "1"),
        ("Song (feat. B)", "A", "Album", "Song", "A feat. B", "Album"),
        ("Song feat. B", "A", "Album", "Song", "A; B", "Album"),
        ("Song - Live", "A", "Album", "Song (Live)", "A", "Album"),
    ];
    for (t1, a1, al1, t2, a2, al2) in pairs {
        assert!(
            merged(
                song("a", "1", t1, a1, al1, 200.0),
                song("b", "2", t2, a2, al2, 200.0)
            ),
            "{t1} / {t2}"
        );
    }
}

#[test]
fn versions_stay_apart() {
    let pairs = [
        ("Song (Live)", "Song"),
        ("Song - Live at Wembley", "Song (Live)"),
        ("Song - 2011 Remaster", "Song"),
        ("Song (Remastered 2011)", "Song (Remastered 2009)"),
        ("Song (Clean)", "Song"),
        ("Song (Acoustic Version)", "Song"),
        ("Song (Radio Edit)", "Song"),
        ("Song (Remix)", "Song"),
        ("Song (Demo)", "Song"),
        ("Song Pt. 1", "Song Pt. 2"),
    ];
    for (t1, t2) in pairs {
        assert!(
            !merged(
                song("a", "1", t1, "A", "Album", 200.0),
                song("b", "2", t2, "A", "Album", 200.0)
            ),
            "{t1} / {t2}"
        );
    }
}

#[test]
fn explicit_and_clean_stay_apart() {
    let mut x = song("a", "1", "Song", "A", "Album", 200.0);
    let y = song("b", "2", "Song", "A", "Album", 200.0);
    x.explicit = true;
    assert!(!merged(x.clone(), y));
    let mut z = song("b", "3", "Song", "A", "Album", 200.0);
    z.explicit = true;
    assert!(merged(x, z));
}

#[test]
fn another_album_is_another_recording() {
    // The live album, the compilation, the deluxe edition: kept apart.
    for album in ["Live at Wembley", "Greatest Hits", "Album (Deluxe Edition)"] {
        assert!(!merged(
            song("a", "1", "Song", "A", "Album", 200.0),
            song("b", "2", "Song", "A", album, 200.0)
        ));
    }
}

#[test]
fn durations_must_be_within_two_seconds() {
    let x = song("a", "1", "Song", "A", "Album", 200.0);
    assert!(merged(
        x.clone(),
        song("b", "2", "Song", "A", "Album", 202.0)
    ));
    assert!(!merged(x, song("b", "2", "Song", "A", "Album", 202.5)));
}

#[test]
fn isrc_decides_when_both_have_one() {
    let mut x = song("a", "1", "Song", "A", "Album", 200.0);
    let mut y = song("b", "2", "Song", "A", "Album", 200.0);
    x.isrc = Some("GBAYE0601498".into());
    assert!(merged(x.clone(), y.clone()), "one side without an ISRC");
    y.isrc = Some("gbaye0601498".into());
    assert!(merged(x.clone(), y.clone()));
    y.isrc = Some("GBAYE0601499".into());
    assert!(!merged(x, y), "different recordings");
}

#[test]
fn one_account_s_duplicates_are_left_alone() {
    let x = song("a", "1", "Song", "A", "Album", 200.0);
    let y = song("a", "2", "Song", "A", "Album", 201.0);
    assert!(!merged(x.clone(), y.clone()));
    // With a copy elsewhere, the closer one joins it; the other stays out.
    let z = song("b", "3", "Song", "A", "Album", 201.5);
    assert_eq!(grouped(&[x, y, z], &[]), vec![vec!["a:2", "b:3"]]);
}

#[test]
fn three_accounts_make_one_group_led_by_the_first() {
    let songs = [
        song("c", "x", "Song", "A", "Album", 200.0),
        song("b", "y", "Song", "A", "Album", 200.5),
        song("a", "z", "Song", "A", "Album", 201.0),
    ];
    assert_eq!(grouped(&songs, &[]), vec![vec!["a:z", "b:y", "c:x"]]);
    // Priority picks the representative; the group's id doesn't move.
    let g1 = group_songs(&songs, &[], &prio());
    let g2 = group_songs(&songs, &[], &["c".into(), "a".into()]);
    assert_eq!(g1[0].id, g2[0].id);
    assert_eq!(g2[0].representative(), &TrackRef::new("c", "x"));
}

#[test]
fn not_the_same_overrides_the_matcher() {
    let x = song("a", "1", "Song", "A", "Album", 200.0);
    let y = song("b", "2", "Song", "A", "Album", 200.0);
    let z = song("c", "3", "Song", "A", "Album", 200.0);
    let apart = ManualLink {
        a: TrackRef::new("b", "2"),
        b: TrackRef::new("a", "1"),
        same: false,
    };
    // y can't join x's group; z joins one of them.
    let g = grouped(&[x, y, z], &[apart]);
    assert_eq!(g, vec![vec!["a:1", "c:3"]]);
}

#[test]
fn a_manual_link_joins_what_the_matcher_would_not() {
    let x = song("a", "1", "Song (Live)", "A", "Album", 200.0);
    let y = song("b", "2", "Song", "A", "Other", 260.0);
    let link = ManualLink {
        a: TrackRef::new("a", "1"),
        b: TrackRef::new("b", "2"),
        same: true,
    };
    let g = group_songs(&[x, y], &[link], &prio());
    assert_eq!(g.len(), 1);
    assert!(g[0].members.iter().all(|m| m.manual));
}

#[test]
fn a_link_may_name_a_song_this_device_has_no_account_for() {
    let x = song("b", "2", "Song", "A", "Album", 200.0);
    let link = ManualLink {
        a: TrackRef::new("elsewhere", "9"),
        b: TrackRef::new("b", "2"),
        same: true,
    };
    assert_eq!(grouped(&[x], &[link]), vec![vec!["b:2", "elsewhere:9"]]);
}

#[test]
fn albums_and_artists_group_by_name() {
    let e =
        |source: &str, id: &str, name: &str, artist: Option<&str>, year: Option<u32>| EntityFacts {
            source: source.into(),
            id: id.into(),
            name: name.into(),
            artist: artist.map(Into::into),
            year,
        };
    let groups = group_entities(
        &[
            e("b", "al2", "Jazz", Some("Queen"), Some(1978)),
            e("a", "al1", "jazz", Some("The Queen"), None),
            e("a", "al3", "Jazz", Some("Queen"), Some(1978)),
            e("c", "al4", "Jazz", Some("Queen"), Some(2011)),
            e("c", "al5", "Innuendo", Some("Queen"), None),
        ],
        &prio(),
    );
    let members: Vec<Vec<(SourceId, String)>> = groups.into_iter().map(|g| g.members).collect();
    // One copy per source, and years that differ stay apart (al4); al3 is
    // a's second "Jazz", left out since al1 came first.
    assert_eq!(
        members,
        vec![vec![("a".into(), "al1".into()), ("b".into(), "al2".into())]]
    );
}
