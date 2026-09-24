use std::collections::HashSet;

use super::*;
use crate::api::{ContextKind, PinTarget, RatingTarget};

struct FakeResolver;

impl Resolver for FakeResolver {
    fn track_for_key(&self, key: &QueueKey) -> Option<TrackId> {
        key.strip_prefix("key-").map(|t| format!("track-{t}"))
    }
    fn context_tracks(&self, _server_id: &str, kind: &ContextKind) -> Vec<TrackId> {
        match kind {
            ContextKind::Album { id } => vec![format!("{id}-1"), format!("{id}-2")],
            ContextKind::Playlist { id } => vec![format!("{id}-a")],
            _ => vec![],
        }
    }
    fn context_label(&self, _server_id: &str, kind: &ContextKind) -> String {
        NoResolver.context_label("", kind)
    }
}

fn state() -> StateView {
    StateView {
        server_id: Some("srv".into()),
        has_session: true,
        has_current: true,
        current_track_id: Some("cur".into()),
        ..Default::default()
    }
}

fn tracks(ids: &[&str]) -> ActionTarget {
    ActionTarget::Tracks {
        ids: ids.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn registry_is_well_formed() {
    let r = ActionRegistry::new(Platform::Linux);
    let mut ids = HashSet::new();
    for d in r.definitions() {
        assert!(ids.insert(d.id), "duplicate action id {}", d.id);
        assert!(!d.label.is_empty() && !d.icon.is_empty());
        assert!(!d.targets.is_empty(), "{} has no targets", d.id);
        if let Some(c) = d.default_shortcut {
            normalise(c, Platform::Linux).unwrap_or_else(|e| panic!("{}: {e}", d.id));
        }
    }
    for required in [
        "play",
        "playNext",
        "playLater",
        "addToPlaylist",
        "removeFromPlaylist",
        "removeFromQueue",
        "rate0",
        "rate1",
        "rate2",
        "rate3",
        "rate4",
        "rate5",
        "love",
        "unlove",
        "download",
        "unpin",
        "goToAlbum",
        "goToArtist",
        "shuffle",
        "repeat",
        "autoplay",
        "clearQueue",
        "saveQueueAsPlaylist",
        "undo",
        "redo",
        "selectAll",
        "openCommandPalette",
        "toggleQueuePanel",
        "toggleFullscreen",
        "toggleMiniPlayer",
        "sleepTimer",
        "handoff",
        "resumeHere",
        "copyDiagnostics",
    ] {
        assert!(r.get(required).is_some(), "missing {required}");
    }
    // Every default order references real actions, without duplicates.
    for s in Surface::ALL {
        let order = r.default_order(s);
        let set: HashSet<&String> = order.iter().collect();
        assert_eq!(set.len(), order.len(), "{s:?} has duplicates");
        for id in &order {
            assert!(r.get(id).is_some(), "{s:?} lists unknown {id}");
        }
        assert_eq!(Surface::parse(s.as_str()), Some(s));
    }
    assert_eq!(Surface::parse("nope"), None);
    // Default chords never collide.
    let mut chords = HashSet::new();
    for d in r.definitions() {
        if let Some(c) = r.default_shortcut(d.id) {
            assert!(chords.insert(c.clone()), "default chord {c} used twice");
        }
    }
}

#[test]
fn design_keyboard_table() {
    let r = ActionRegistry::new(Platform::Linux);
    let expect = [
        ("Ctrl+K", "openCommandPalette"),
        ("Ctrl+F", "findInList"),
        ("Space", "togglePlay"),
        ("ArrowLeft", "seekBackward"),
        ("ArrowRight", "seekForward"),
        ("Shift+ArrowLeft", "previous"),
        ("Shift+ArrowRight", "next"),
        ("Q", "toggleQueuePanel"),
        ("F", "toggleFullscreen"),
        ("M", "toggleMiniPlayer"),
        ("0", "rate0"),
        ("5", "rate5"),
        ("Ctrl+Z", "undo"),
        ("Ctrl+Shift+Z", "redo"),
        ("Ctrl+Y", "redoAlt"),
        ("Ctrl+A", "selectAll"),
        ("Delete", "remove"),
    ];
    for (chord, id) in expect {
        assert_eq!(r.action_for_chord(chord), Some(id), "{chord}");
    }
    let mac = ActionRegistry::new(Platform::MacOs);
    assert_eq!(mac.action_for_chord("Cmd+K"), Some("openCommandPalette"));
    assert_eq!(mac.action_for_chord("cmd+shift+z"), Some("redo"));
    assert_eq!(mac.shortcut_for("redo").as_deref(), Some("Shift+Cmd+Z"));
    assert_eq!(
        mac.action_for_chord("Ctrl+K"),
        None,
        "Control is not Command on macOS"
    );
    assert_eq!(mac.shortcut_for("undo").as_deref(), Some("Cmd+Z"));
}

#[test]
fn shortcut_rebinding_and_conflicts() {
    let mut r = ActionRegistry::new(Platform::Windows);
    assert_eq!(
        r.set_shortcut("nope", Some("Ctrl+J")),
        Err(ActionError::UnknownAction("nope".into()))
    );
    assert!(matches!(
        r.set_shortcut("shuffle", Some("ctrl+shift")),
        Err(ActionError::Chord(ChordError::KeyCount))
    ));
    assert_eq!(
        r.set_shortcut("shuffle", Some("ctrl+k")),
        Err(ActionError::Conflict {
            chord: "Ctrl+K".into(),
            other_action_id: "openCommandPalette".into()
        })
    );
    r.set_shortcut("shuffle", Some("ctrl+shift+s")).unwrap();
    assert_eq!(r.shortcut_for("shuffle").as_deref(), Some("Ctrl+Shift+S"));
    assert_eq!(r.action_for_chord("Ctrl+Shift+S"), Some("shuffle"));
    // Rebinding an action to its own chord is fine; unbinding frees the chord.
    r.set_shortcut("shuffle", Some("Ctrl+Shift+S")).unwrap();
    r.set_shortcut("openCommandPalette", None).unwrap();
    assert_eq!(r.action_for_chord("Ctrl+K"), None);
    r.set_shortcut("shuffle", Some("Ctrl+K")).unwrap();
    assert_eq!(r.action_for_chord("Ctrl+K"), Some("shuffle"));
    // Setting the default explicitly clears the override.
    r.set_shortcut("shuffle", None).unwrap();
    r.set_shortcut("openCommandPalette", Some("Ctrl+K"))
        .unwrap();
    assert!(!r
        .customisation()
        .shortcuts
        .contains_key("openCommandPalette"));
    r.reset_shortcut("shuffle");
    assert_eq!(r.shortcut_for("shuffle"), None);

    let list = r.shortcuts();
    let undo = list.iter().find(|s| s.action_id == "undo").unwrap();
    assert_eq!(undo.shortcut.as_deref(), Some("Ctrl+Z"));
    assert_eq!(undo.default_shortcut.as_deref(), Some("Ctrl+Z"));
    assert!(
        !list.iter().any(|s| s.action_id == "shuffle"),
        "unbound actions without a default are not listed"
    );
}

#[test]
fn customisation_round_trips_across_platforms() {
    let mut linux = ActionRegistry::new(Platform::Linux);
    linux.set_shortcut("shuffle", Some("Mod+S")).unwrap();
    linux.set_shortcut("undo", None).unwrap();
    linux.set_order(
        Surface::MediaSession,
        vec![
            "togglePlay".into(),
            "next".into(),
            "bogus".into(),
            "next".into(),
        ],
    );
    linux.set_order(Surface::Sidebar, linux.default_order(Surface::Sidebar));
    let json = linux.to_json();
    let c: ActionCustomisation = serde_json::from_str(&json).unwrap();
    assert_eq!(c.shortcuts["shuffle"].as_deref(), Some("Mod+S"));
    assert_eq!(c.shortcuts["undo"], None);
    assert_eq!(c.orders["mediaSession"], vec!["togglePlay", "next"]);
    assert!(
        !c.orders.contains_key("sidebar"),
        "the default order is not stored"
    );

    let mut mac = ActionRegistry::new(Platform::MacOs);
    mac.from_json(&json).unwrap();
    assert_eq!(mac.shortcut_for("shuffle").as_deref(), Some("Cmd+S"));
    assert_eq!(mac.shortcut_for("undo"), None);
    assert_eq!(mac.order(Surface::MediaSession), vec!["togglePlay", "next"]);
    assert_eq!(mac.to_json(), json);
    // A conflicting stored chord is dropped, not fatal.
    let mut broken = ActionRegistry::new(Platform::Linux);
    broken
        .from_json(r#"{"shortcuts":{"shuffle":"Mod+K","repeat":"garbage+"}}"#)
        .unwrap();
    assert_eq!(broken.shortcut_for("shuffle"), None);
    assert!(broken.from_json("not json").is_err());
}

#[test]
fn surfaces_filter_and_order() {
    let mut r = ActionRegistry::new(Platform::Linux);
    let s = state();
    let menu = r.actions_for(Surface::ContextMenu, &tracks(&["a", "b"]), &s);
    let ids: Vec<&str> = menu.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        &ids[..4],
        &["play", "playShuffled", "playNext", "playLater"]
    );
    assert!(ids.contains(&"rate3") && ids.contains(&"download") && ids.contains(&"addToPlaylist"));
    assert!(!ids.contains(&"removeFromQueue"), "not for tracks");
    assert!(
        !ids.contains(&"undo"),
        "global actions stay out of a track's context menu"
    );
    let rfp = menu.iter().find(|d| d.id == "removeFromPlaylist").unwrap();
    assert!(!rfp.enabled, "not viewing a playlist");
    let go = menu.iter().find(|d| d.id == "goToAlbum").unwrap();
    assert!(!go.enabled, "two tracks selected");
    assert!(menu.iter().all(|d| d.enabled
        || matches!(
            d.id.as_str(),
            "removeFromPlaylist" | "goToAlbum" | "goToArtist"
        )));

    let queue_menu = r.actions_for(
        Surface::ContextMenu,
        &ActionTarget::QueueItems {
            keys: vec!["key-1".into()],
        },
        &s,
    );
    assert!(queue_menu
        .iter()
        .any(|d| d.id == "removeFromQueue" && d.enabled));

    let saved_menu = r.actions_for(
        Surface::ContextMenu,
        &ActionTarget::SavedQueue { id: "q".into() },
        &s,
    );
    let saved_ids: Vec<&str> = saved_menu.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(
        saved_ids,
        vec![
            "play",
            "playShuffled",
            "restoreSavedQueue",
            "pinSavedQueue",
            "unpinSavedQueue",
            "saveQueueAsPlaylist",
            "deleteSavedQueue"
        ]
    );
    assert!(
        saved_menu
            .iter()
            .find(|d| d.id == "deleteSavedQueue")
            .unwrap()
            .destructive
    );

    // The palette shows global actions alongside a selection, with state-driven enablement.
    let palette = r.actions_for(Surface::Palette, &tracks(&["a"]), &s);
    let undo = palette.iter().find(|d| d.id == "undo").unwrap();
    assert!(!undo.enabled);
    assert_eq!(undo.default_shortcut.as_deref(), Some("Ctrl+Z"));
    let s2 = StateView {
        can_undo: true,
        text_field_focused: true,
        ..state()
    };
    assert!(
        !r.descriptor("undo", &ActionTarget::None, &s2)
            .unwrap()
            .enabled,
        "Ctrl+Z belongs to the field"
    );
    let s3 = StateView {
        can_undo: true,
        ..state()
    };
    assert!(
        r.descriptor("undo", &ActionTarget::None, &s3)
            .unwrap()
            .enabled
    );
    assert!(r.all_for(&ActionTarget::None, &s).len() >= 40);

    // Choose-and-order.
    r.set_order(
        Surface::ContextMenu,
        vec!["playLater".into(), "play".into()],
    );
    let menu = r.actions_for(Surface::ContextMenu, &tracks(&["a"]), &s);
    assert_eq!(
        menu.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        vec!["playLater", "play"]
    );
    assert!(r.set_order_str("bogus", vec![]).is_err());
    r.set_order_str("contextMenu", r.default_order(Surface::ContextMenu))
        .unwrap();
    assert!(r.customisation().orders.is_empty());
    assert!(r.actions_for_str("nope", &ActionTarget::None, &s).is_err());
}

#[test]
fn handlers_map_to_commands() {
    let r = ActionRegistry::new(Platform::Linux);
    let s = state();
    let res = FakeResolver;
    let c = r.commands("play", &tracks(&["a", "b"]), &s, &res).unwrap();
    assert_eq!(
        c,
        vec![Command::PlayTracks {
            server_id: "srv".into(),
            track_ids: vec!["a".into(), "b".into()],
            start_index: 0,
            label: "Selection".into(),
            shuffle: false
        }]
    );
    let c = r
        .commands(
            "playShuffled",
            &ActionTarget::Albums {
                ids: vec!["al".into()],
            },
            &s,
            &res,
        )
        .unwrap();
    match &c[0] {
        Command::PlayContext { args } => {
            assert_eq!(args.context.kind, ContextKind::Album { id: "al".into() });
            assert_eq!(args.context.tracks, vec!["al-1", "al-2"]);
            assert!(args.shuffle);
        }
        other => panic!("{other:?}"),
    }
    let c = r
        .commands(
            "play",
            &ActionTarget::Albums {
                ids: vec!["x".into(), "y".into()],
            },
            &s,
            &res,
        )
        .unwrap();
    assert!(matches!(&c[0], Command::PlayTracks { track_ids, .. } if track_ids.len() == 4));
    assert_eq!(
        r.commands(
            "play",
            &ActionTarget::QueueItems {
                keys: vec!["key-9".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::JumpToQueueItem {
            key: "key-9".into()
        }]
    );
    assert_eq!(
        r.commands(
            "play",
            &ActionTarget::SavedQueue { id: "sq".into() },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::RestoreSavedQueue { id: "sq".into() }]
    );
    assert_eq!(
        r.commands("play", &ActionTarget::None, &s, &res).unwrap(),
        vec![Command::Play]
    );

    assert_eq!(
        r.commands(
            "playNext",
            &ActionTarget::QueueItems {
                keys: vec!["key-1".into(), "key-2".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::PlayNext {
            server_id: "srv".into(),
            track_ids: vec!["track-1".into(), "track-2".into()]
        }]
    );
    assert_eq!(
        r.commands(
            "playLater",
            &ActionTarget::Playlists {
                ids: vec!["pl".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::PlayLater {
            server_id: "srv".into(),
            track_ids: vec!["pl-a".into()]
        }]
    );
    assert_eq!(
        r.commands("rate4", &ActionTarget::None, &s, &res).unwrap(),
        vec![Command::SetRating {
            targets: vec![RatingTarget::Track { id: "cur".into() }],
            rating: 4
        }]
    );
    assert_eq!(
        r.commands(
            "rate0",
            &ActionTarget::Albums {
                ids: vec!["al".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::SetRating {
            targets: vec![RatingTarget::Album { id: "al".into() }],
            rating: 0
        }]
    );
    assert_eq!(
        r.commands(
            "love",
            &ActionTarget::Artists {
                ids: vec!["ar".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::SetArtistLoved {
            artist_id: "ar".into(),
            loved: true
        }]
    );
    assert_eq!(
        r.commands("unlove", &tracks(&["t"]), &s, &res).unwrap(),
        vec![Command::SetLoved {
            targets: vec![RatingTarget::Track { id: "t".into() }],
            loved: false
        }]
    );
    assert_eq!(
        r.commands(
            "download",
            &ActionTarget::Playlists {
                ids: vec!["pl".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::Pin {
            target: PinTarget::Playlist { id: "pl".into() },
            transcode: false
        }]
    );
    assert_eq!(
        r.commands("unpin", &tracks(&["t"]), &s, &res).unwrap(),
        vec![Command::Unpin {
            target: PinTarget::Track { id: "t".into() }
        }]
    );
    assert_eq!(
        r.commands(
            "removeFromQueue",
            &ActionTarget::QueueItems {
                keys: vec!["k".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::RemoveQueueItems {
            keys: vec!["k".into()]
        }]
    );
    assert_eq!(
        r.commands(
            "remove",
            &ActionTarget::QueueItems {
                keys: vec!["k".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::RemoveQueueItems {
            keys: vec!["k".into()]
        }]
    );
    let in_pl = StateView {
        viewing_playlist: Some("pl".into()),
        selection_indices: vec![3, 7],
        ..state()
    };
    assert_eq!(
        r.commands("remove", &tracks(&["t", "u"]), &in_pl, &res)
            .unwrap(),
        vec![Command::PlaylistRemove {
            playlist_id: "pl".into(),
            indices: vec![3, 7]
        }]
    );
    let smart = StateView {
        viewing_playlist_is_smart: true,
        ..in_pl.clone()
    };
    assert_eq!(
        r.commands("removeFromPlaylist", &tracks(&["t"]), &smart, &res),
        Err(ActionError::Disabled("removeFromPlaylist".into()))
    );

    assert_eq!(
        r.commands("shuffle", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![Command::SetShuffle { enabled: true }]
    );
    let on = StateView {
        shuffle: true,
        repeat: RepeatMode::All,
        autoplay: true,
        ..state()
    };
    assert_eq!(
        r.commands("shuffle", &ActionTarget::None, &on, &res)
            .unwrap(),
        vec![Command::SetShuffle { enabled: false }]
    );
    assert_eq!(
        r.commands("repeat", &ActionTarget::None, &on, &res)
            .unwrap(),
        vec![Command::SetRepeat {
            mode: RepeatMode::One
        }]
    );
    assert_eq!(
        r.commands("autoplay", &ActionTarget::None, &on, &res)
            .unwrap(),
        vec![Command::SetAutoplay { enabled: false }]
    );
    assert_eq!(
        r.commands("reshuffle", &ActionTarget::None, &on, &res)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        r.commands("clearQueue", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![Command::ClearQueue]
    );
    assert_eq!(
        r.commands("seekForward", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![Command::SeekBy {
            delta_ms: SEEK_STEP_MS
        }]
    );
    assert_eq!(
        r.commands("seekBackward", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![Command::SeekBy {
            delta_ms: -SEEK_STEP_MS
        }]
    );
    assert_eq!(
        r.commands("next", &ActionTarget::None, &s, &res).unwrap(),
        vec![Command::Next]
    );
    assert_eq!(
        r.commands("togglePlay", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![Command::TogglePlay]
    );
    assert_eq!(
        r.commands("stopAtEndOfTrack", &ActionTarget::None, &s, &res)
            .unwrap()
            .len(),
        1
    );
    let timer = StateView {
        sleep_timer_active: true,
        ..state()
    };
    assert_eq!(
        r.commands("cancelSleepTimer", &ActionTarget::None, &timer, &res)
            .unwrap(),
        vec![Command::SetSleepTimer { timer: None }]
    );
    assert_eq!(
        r.commands("cancelSleepTimer", &ActionTarget::None, &s, &res),
        Err(ActionError::Disabled("cancelSleepTimer".into()))
    );
    let peers = StateView {
        peer_count: 2,
        has_resume_offer: true,
        ..state()
    };
    assert_eq!(
        r.commands("handoff", &ActionTarget::None, &peers, &res)
            .unwrap(),
        vec![Command::OpenHandoffPicker]
    );
    assert_eq!(
        r.commands("resumeHere", &ActionTarget::None, &peers, &res)
            .unwrap(),
        vec![Command::ResumeHere]
    );
    assert_eq!(
        r.commands("handoff", &ActionTarget::None, &s, &res),
        Err(ActionError::Disabled("handoff".into()))
    );
    let undoable = StateView {
        can_undo: true,
        can_redo: true,
        has_cleared_selection: true,
        ..state()
    };
    assert_eq!(
        r.commands("undo", &ActionTarget::None, &undoable, &res)
            .unwrap(),
        vec![Command::Undo]
    );
    assert_eq!(
        r.commands("redo", &ActionTarget::None, &undoable, &res)
            .unwrap(),
        vec![Command::Redo]
    );
    assert_eq!(
        r.commands("restoreSelection", &ActionTarget::None, &undoable, &res)
            .unwrap(),
        vec![Command::RestoreSelection]
    );
    assert_eq!(
        r.commands(
            "deletePlaylist",
            &ActionTarget::Playlists {
                ids: vec!["p".into()]
            },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::DeletePlaylist {
            playlist_id: "p".into()
        }]
    );
    assert_eq!(
        r.commands(
            "pinSavedQueue",
            &ActionTarget::SavedQueue { id: "q".into() },
            &s,
            &res
        )
        .unwrap(),
        vec![Command::PinSavedQueue {
            id: "q".into(),
            pinned: true
        }]
    );
    // UI-handled actions validate but return nothing.
    assert_eq!(
        r.commands("goToAlbum", &tracks(&["t"]), &s, &res).unwrap(),
        vec![]
    );
    assert!(r.get("goToAlbum").unwrap().ui_handled);
    assert_eq!(
        r.commands("copyDiagnostics", &ActionTarget::None, &s, &res)
            .unwrap(),
        vec![]
    );
    // Errors.
    assert_eq!(
        r.commands("nope", &ActionTarget::None, &s, &res),
        Err(ActionError::UnknownAction("nope".into()))
    );
    assert_eq!(
        r.commands("removeFromQueue", &tracks(&["t"]), &s, &res),
        Err(ActionError::NotApplicable("removeFromQueue".into()))
    );
    assert_eq!(
        r.commands("play", &tracks(&[]), &s, &res),
        Err(ActionError::Disabled("play".into()))
    );
    assert_eq!(r.label("clearQueue"), Some("Clear queue"));
}

#[test]
fn media_session_buttons_follow_customisation() {
    let mut r = ActionRegistry::new(Platform::Android);
    let s = state();
    assert_eq!(
        r.media_session_actions(&s),
        vec![
            MediaSessionAction::Previous,
            MediaSessionAction::Play,
            MediaSessionAction::Next,
            MediaSessionAction::Shuffle,
            MediaSessionAction::Repeat,
            MediaSessionAction::Love,
            MediaSessionAction::Seek,
            MediaSessionAction::Stop,
        ]
    );
    let playing = StateView {
        is_playing: true,
        current_loved: true,
        ..state()
    };
    let a = r.media_session_actions(&playing);
    assert_eq!(a[1], MediaSessionAction::Pause);
    assert!(
        !a.contains(&MediaSessionAction::Love),
        "already loved: the love button is not offered"
    );
    r.set_order(
        Surface::MediaSession,
        vec!["next".into(), "togglePlay".into(), "rate".into()],
    );
    assert_eq!(
        &r.media_session_actions(&s)[..3],
        &[
            MediaSessionAction::Next,
            MediaSessionAction::Play,
            MediaSessionAction::Rate
        ]
    );
    assert_eq!(r.media_session_actions(&StateView::default()), vec![]);
}

/// The phone's Library place has its own action (it used to borrow
/// `navigateAlbums`), usable in a synced sidebar order without joining the
/// default one.
#[test]
fn navigate_library_is_a_navigation_action() {
    let mut r = ActionRegistry::new(Platform::Android);
    let d = r.get("navigateLibrary").expect("navigateLibrary");
    assert_eq!((d.label, d.icon), ("Library", "library_music"));
    assert!(d.ui_handled);
    assert_eq!(d.category, defs::Category::Navigation);
    assert!(!r
        .default_order(Surface::Sidebar)
        .contains(&"navigateLibrary".to_string()));
    let palette = r.actions_for(Surface::Palette, &ActionTarget::None, &state());
    assert!(palette
        .iter()
        .any(|a| a.id == "navigateLibrary" && a.enabled));

    let bar = ["navigateHome", "findInList", "navigateLibrary"].map(String::from);
    r.set_order(Surface::Sidebar, bar.to_vec());
    assert_eq!(r.order(Surface::Sidebar), bar);
    let sidebar = r.actions_for(Surface::Sidebar, &ActionTarget::None, &state());
    let ids: Vec<&str> = sidebar.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(ids, ["navigateHome", "findInList", "navigateLibrary"]);
}
