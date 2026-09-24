//! The curated action set and the default order per surface.
//!
//! Adding an action: one entry in [`ALL`], optionally a place in the default
//! orders. Icons are Material Symbols names.

use crate::api::{ActionTarget, Command, MediaSessionAction, RepeatMode, SleepTimer};

use super::{
    ids_of, pin_targets, play_commands, queue_commands, rating_targets, single_id, Resolver,
    StateView, Surface, SEEK_STEP_MS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Playback,
    Queue,
    Library,
    Edit,
    View,
    Session,
    System,
    Navigation,
}

impl Category {
    pub fn as_str(&self) -> &'static str {
        match self {
            Category::Playback => "playback",
            Category::Queue => "queue",
            Category::Library => "library",
            Category::Edit => "edit",
            Category::View => "view",
            Category::Session => "session",
            Category::System => "system",
            Category::Navigation => "navigation",
        }
    }
}

/// Kinds of [`ActionTarget`]; `Global` is `ActionTarget::None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Global,
    Tracks,
    Albums,
    Artists,
    Playlists,
    QueueItems,
    SavedQueue,
}

impl TargetKind {
    pub fn of(target: &ActionTarget) -> TargetKind {
        match target {
            ActionTarget::None => TargetKind::Global,
            ActionTarget::Tracks { .. } => TargetKind::Tracks,
            ActionTarget::Albums { .. } => TargetKind::Albums,
            ActionTarget::Artists { .. } => TargetKind::Artists,
            ActionTarget::Playlists { .. } => TargetKind::Playlists,
            ActionTarget::QueueItems { .. } => TargetKind::QueueItems,
            ActionTarget::SavedQueue { .. } => TargetKind::SavedQueue,
        }
    }
}

pub type EnabledFn = fn(&ActionTarget, &StateView) -> bool;
pub type CommandsFn = fn(&ActionTarget, &StateView, &dyn Resolver) -> Vec<Command>;

/// One user-visible action.
pub struct ActionDef {
    pub id: &'static str,
    pub label: &'static str,
    /// Material Symbols icon name.
    pub icon: &'static str,
    pub category: Category,
    /// Target kinds it accepts. `Global` means it needs no target.
    pub targets: &'static [TargetKind],
    pub undoable: bool,
    /// Confirm-instead: a dialog, no undo entry.
    pub destructive: bool,
    /// Portable chord (`Mod` = primary modifier).
    pub default_shortcut: Option<&'static str>,
    /// Media-session button this action provides.
    pub media_session: Option<MediaSessionAction>,
    /// No core command: the platform layer performs it by id (navigation,
    /// panels, pickers, clipboard).
    pub ui_handled: bool,
    pub enabled: EnabledFn,
    pub commands: CommandsFn,
}

// -- predicates ---------------------------------------------------------------

fn always(_: &ActionTarget, _: &StateView) -> bool {
    true
}

/// A non-empty selection, or the current track for a global target.
fn has_items(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::None => s.has_current,
        ActionTarget::SavedQueue { .. } => true,
        _ => !ids_of(t).is_empty(),
    }
}

fn has_selection(t: &ActionTarget, _: &StateView) -> bool {
    !ids_of(t).is_empty()
}

fn has_current(_: &ActionTarget, s: &StateView) -> bool {
    s.has_current
}

fn has_session(_: &ActionTarget, s: &StateView) -> bool {
    s.has_session
}

fn can_undo(_: &ActionTarget, s: &StateView) -> bool {
    s.can_undo && !s.text_field_focused
}

fn can_redo(_: &ActionTarget, s: &StateView) -> bool {
    s.can_redo && !s.text_field_focused
}

fn go_to_album(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::None => s.current_album_id.is_some(),
        _ => single_id(t).is_some(),
    }
}

fn go_to_artist(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::None => s.current_artist_id.is_some(),
        _ => single_id(t).is_some(),
    }
}

fn love_enabled(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::None => s.has_current && !s.current_loved,
        _ => has_selection(t, s),
    }
}

fn unlove_enabled(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::None => s.has_current && s.current_loved,
        _ => has_selection(t, s),
    }
}

fn in_editable_playlist(t: &ActionTarget, s: &StateView) -> bool {
    matches!(t, ActionTarget::Tracks { .. })
        && has_selection(t, s)
        && s.viewing_playlist.is_some()
        && !s.viewing_playlist_is_smart
}

fn remove_enabled(t: &ActionTarget, s: &StateView) -> bool {
    match t {
        ActionTarget::QueueItems { .. } => has_selection(t, s),
        ActionTarget::Tracks { .. } => in_editable_playlist(t, s),
        _ => false,
    }
}

fn not_in_text_field(_: &ActionTarget, s: &StateView) -> bool {
    !s.text_field_focused
}

fn transport_enabled(_: &ActionTarget, s: &StateView) -> bool {
    s.has_current && !s.text_field_focused
}

// -- handlers ---------------------------------------------------------------

fn none(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![]
}

fn play(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    play_commands(t, s, r, false)
}

fn play_shuffled(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    play_commands(t, s, r, true)
}

fn play_next(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    queue_commands(t, s, r, true)
}

fn play_later(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    queue_commands(t, s, r, false)
}

macro_rules! rate_fn {
    ($name:ident, $n:literal) => {
        fn $name(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
            vec![Command::SetRating {
                targets: rating_targets(t, s, r),
                rating: $n,
            }]
        }
    };
}
rate_fn!(rate0, 0);
rate_fn!(rate1, 1);
rate_fn!(rate2, 2);
rate_fn!(rate3, 3);
rate_fn!(rate4, 4);
rate_fn!(rate5, 5);

fn set_loved(t: &ActionTarget, s: &StateView, r: &dyn Resolver, loved: bool) -> Vec<Command> {
    match t {
        ActionTarget::Artists { ids } => ids
            .iter()
            .map(|id| Command::SetArtistLoved {
                artist_id: id.clone(),
                loved,
            })
            .collect(),
        _ => {
            let targets = rating_targets(t, s, r);
            if targets.is_empty() {
                vec![]
            } else {
                vec![Command::SetLoved { targets, loved }]
            }
        }
    }
}

fn love(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    set_loved(t, s, r, true)
}

fn unlove(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    set_loved(t, s, r, false)
}

fn download(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    pin_targets(t, s, r)
        .into_iter()
        .map(|target| Command::Pin {
            target,
            transcode: false,
        })
        .collect()
}

fn unpin(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    pin_targets(t, s, r)
        .into_iter()
        .map(|target| Command::Unpin { target })
        .collect()
}

fn remove_from_queue(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    match t {
        ActionTarget::QueueItems { keys } if !keys.is_empty() => {
            vec![Command::RemoveQueueItems { keys: keys.clone() }]
        }
        _ => vec![],
    }
}

fn remove_from_playlist(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    match &s.viewing_playlist {
        Some(playlist_id) if !s.selection_indices.is_empty() => {
            vec![Command::PlaylistRemove {
                playlist_id: playlist_id.clone(),
                indices: s.selection_indices.clone(),
            }]
        }
        _ => vec![],
    }
}

fn remove(t: &ActionTarget, s: &StateView, r: &dyn Resolver) -> Vec<Command> {
    match t {
        ActionTarget::QueueItems { .. } => remove_from_queue(t, s, r),
        ActionTarget::Tracks { .. } => remove_from_playlist(t, s, r),
        _ => vec![],
    }
}

fn toggle_shuffle(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetShuffle {
        enabled: !s.shuffle,
    }]
}

fn cycle_repeat(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    let mode = match s.repeat {
        RepeatMode::Off => RepeatMode::All,
        RepeatMode::All => RepeatMode::One,
        RepeatMode::One => RepeatMode::Off,
    };
    vec![Command::SetRepeat { mode }]
}

fn toggle_autoplay(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetAutoplay {
        enabled: !s.autoplay,
    }]
}

fn saved_queue_cmd(t: &ActionTarget, f: fn(String) -> Command) -> Vec<Command> {
    match t {
        ActionTarget::SavedQueue { id } => vec![f(id.clone())],
        _ => vec![],
    }
}

fn restore_saved_queue(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    saved_queue_cmd(t, |id| Command::RestoreSavedQueue { id })
}

fn pin_saved_queue(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    saved_queue_cmd(t, |id| Command::PinSavedQueue { id, pinned: true })
}

fn unpin_saved_queue(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    saved_queue_cmd(t, |id| Command::PinSavedQueue { id, pinned: false })
}

fn delete_saved_queue(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    saved_queue_cmd(t, |id| Command::DeleteSavedQueue { id })
}

fn delete_playlist(t: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    match t {
        ActionTarget::Playlists { ids } => ids
            .iter()
            .map(|id| Command::DeletePlaylist {
                playlist_id: id.clone(),
            })
            .collect(),
        _ => vec![],
    }
}

macro_rules! cmd_fn {
    ($name:ident, $cmd:expr) => {
        fn $name(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
            vec![$cmd]
        }
    };
}
cmd_fn!(cmd_pause, Command::Pause);
cmd_fn!(cmd_toggle_play, Command::TogglePlay);
cmd_fn!(cmd_stop, Command::Stop);
cmd_fn!(cmd_next, Command::Next);
cmd_fn!(cmd_previous, Command::Previous);
cmd_fn!(cmd_clear_queue, Command::ClearQueue);
cmd_fn!(cmd_clear_insertions, Command::ClearInsertions);
cmd_fn!(cmd_undo, Command::Undo);
cmd_fn!(cmd_redo, Command::Redo);
cmd_fn!(cmd_restore_selection, Command::RestoreSelection);
cmd_fn!(cmd_open_handoff, Command::OpenHandoffPicker);
cmd_fn!(cmd_resume_here, Command::ResumeHere);
cmd_fn!(cmd_dismiss_resume, Command::DismissResumeOffer);

fn seek_forward(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SeekBy {
        delta_ms: SEEK_STEP_MS,
    }]
}

fn seek_backward(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SeekBy {
        delta_ms: -SEEK_STEP_MS,
    }]
}

/// Volume step for the keyboard / remote volume actions.
pub const VOLUME_STEP: f64 = 0.05;

fn volume_up(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetVolume {
        volume: (s.volume + VOLUME_STEP).clamp(0.0, 1.0),
    }]
}

fn volume_down(_: &ActionTarget, s: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetVolume {
        volume: (s.volume - VOLUME_STEP).clamp(0.0, 1.0),
    }]
}

fn reshuffle(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    // Turning shuffle off and on again reshuffles with a new seed.
    vec![
        Command::SetShuffle { enabled: false },
        Command::SetShuffle { enabled: true },
    ]
}

fn cancel_sleep_timer(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetSleepTimer { timer: None }]
}

fn stop_at_end_of_track(_: &ActionTarget, _: &StateView, _: &dyn Resolver) -> Vec<Command> {
    vec![Command::SetSleepTimer {
        timer: Some(SleepTimer {
            ends_at: None,
            stop_at_end_of_track: true,
        }),
    }]
}

// -- the table --------------------------------------------------------------

const GLOBAL: &[TargetKind] = &[TargetKind::Global];
const PLAYABLE: &[TargetKind] = &[
    TargetKind::Global,
    TargetKind::Tracks,
    TargetKind::Albums,
    TargetKind::Artists,
    TargetKind::Playlists,
    TargetKind::QueueItems,
    TargetKind::SavedQueue,
];
const QUEUEABLE: &[TargetKind] = &[
    TargetKind::Tracks,
    TargetKind::Albums,
    TargetKind::Artists,
    TargetKind::Playlists,
    TargetKind::QueueItems,
];
const RATEABLE: &[TargetKind] = &[
    TargetKind::Global,
    TargetKind::Tracks,
    TargetKind::Albums,
    TargetKind::QueueItems,
];
const LOVEABLE: &[TargetKind] = &[
    TargetKind::Global,
    TargetKind::Tracks,
    TargetKind::Albums,
    TargetKind::Artists,
    TargetKind::QueueItems,
];
const PINNABLE: &[TargetKind] = &[
    TargetKind::Global,
    TargetKind::Tracks,
    TargetKind::Albums,
    TargetKind::Playlists,
    TargetKind::QueueItems,
];
const TRACKISH: &[TargetKind] = &[
    TargetKind::Global,
    TargetKind::Tracks,
    TargetKind::QueueItems,
];
const SAVED: &[TargetKind] = &[TargetKind::SavedQueue];

impl ActionDef {
    const fn base(
        id: &'static str,
        label: &'static str,
        icon: &'static str,
        category: Category,
        targets: &'static [TargetKind],
        enabled: EnabledFn,
        commands: CommandsFn,
    ) -> ActionDef {
        ActionDef {
            id,
            label,
            icon,
            category,
            targets,
            undoable: false,
            destructive: false,
            default_shortcut: None,
            media_session: None,
            ui_handled: false,
            enabled,
            commands,
        }
    }
}

macro_rules! action {
    ($id:literal, $label:literal, $icon:literal, $cat:ident, $targets:expr, $enabled:expr, $commands:expr $(, $field:ident = $value:expr)* $(,)?) => {
        ActionDef { $($field: $value,)* ..ActionDef::base($id, $label, $icon, Category::$cat, $targets, $enabled, $commands) }
    };
}

fn ui_nav(_: &ActionTarget, _: &StateView) -> bool {
    true
}

static ALL: &[ActionDef] = &[
    // -- playback --
    action!(
        "play",
        "Play",
        "play_arrow",
        Playback,
        PLAYABLE,
        has_items,
        play,
        undoable = true
    ),
    action!(
        "playShuffled",
        "Shuffle play",
        "shuffle",
        Playback,
        PLAYABLE,
        has_items,
        play_shuffled,
        undoable = true
    ),
    action!(
        "togglePlay",
        "Play / pause",
        "play_pause",
        Playback,
        GLOBAL,
        transport_enabled,
        cmd_toggle_play,
        default_shortcut = Some("Space"),
        media_session = Some(MediaSessionAction::Play)
    ),
    action!(
        "pause",
        "Pause",
        "pause",
        Playback,
        GLOBAL,
        has_current,
        cmd_pause,
        media_session = Some(MediaSessionAction::Pause)
    ),
    action!(
        "stop",
        "Stop",
        "stop",
        Playback,
        GLOBAL,
        has_current,
        cmd_stop,
        media_session = Some(MediaSessionAction::Stop)
    ),
    action!(
        "next",
        "Next track",
        "skip_next",
        Playback,
        GLOBAL,
        transport_enabled,
        cmd_next,
        default_shortcut = Some("Shift+ArrowRight"),
        media_session = Some(MediaSessionAction::Next),
        undoable = true
    ),
    action!(
        "previous",
        "Previous track",
        "skip_previous",
        Playback,
        GLOBAL,
        transport_enabled,
        cmd_previous,
        default_shortcut = Some("Shift+ArrowLeft"),
        media_session = Some(MediaSessionAction::Previous),
        undoable = true
    ),
    action!(
        "seekForward",
        "Seek forward",
        "forward_10",
        Playback,
        GLOBAL,
        transport_enabled,
        seek_forward,
        default_shortcut = Some("ArrowRight")
    ),
    action!(
        "seekBackward",
        "Seek backward",
        "replay_10",
        Playback,
        GLOBAL,
        transport_enabled,
        seek_backward,
        default_shortcut = Some("ArrowLeft")
    ),
    action!(
        "volumeUp",
        "Volume up",
        "volume_up",
        Playback,
        GLOBAL,
        always,
        volume_up,
        default_shortcut = Some("Mod+ArrowUp")
    ),
    action!(
        "volumeDown",
        "Volume down",
        "volume_down",
        Playback,
        GLOBAL,
        always,
        volume_down,
        default_shortcut = Some("Mod+ArrowDown")
    ),
    // -- queue --
    action!(
        "playNext",
        "Play next",
        "playlist_play",
        Queue,
        QUEUEABLE,
        has_selection,
        play_next,
        undoable = true
    ),
    action!(
        "playLater",
        "Play later",
        "playlist_add",
        Queue,
        QUEUEABLE,
        has_selection,
        play_later,
        undoable = true
    ),
    action!(
        "removeFromQueue",
        "Remove from queue",
        "remove_from_queue",
        Queue,
        &[TargetKind::QueueItems],
        has_selection,
        remove_from_queue,
        undoable = true
    ),
    action!(
        "remove",
        "Remove",
        "delete",
        Queue,
        &[TargetKind::QueueItems, TargetKind::Tracks],
        remove_enabled,
        remove,
        default_shortcut = Some("Delete"),
        undoable = true
    ),
    action!(
        "shuffle",
        "Shuffle",
        "shuffle",
        Queue,
        GLOBAL,
        has_session,
        toggle_shuffle,
        media_session = Some(MediaSessionAction::Shuffle),
        undoable = true
    ),
    action!(
        "reshuffle",
        "Reshuffle",
        "shuffle_on",
        Queue,
        GLOBAL,
        |_, s| s.has_session && s.shuffle,
        reshuffle,
        undoable = true
    ),
    action!(
        "repeat",
        "Repeat",
        "repeat",
        Queue,
        GLOBAL,
        has_session,
        cycle_repeat,
        media_session = Some(MediaSessionAction::Repeat),
        undoable = true
    ),
    action!(
        "autoplay",
        "Autoplay",
        "all_inclusive",
        Queue,
        GLOBAL,
        always,
        toggle_autoplay,
        undoable = true
    ),
    action!(
        "clearQueue",
        "Clear queue",
        "clear_all",
        Queue,
        GLOBAL,
        has_session,
        cmd_clear_queue,
        undoable = true
    ),
    action!(
        "clearInsertions",
        "Clear playing next",
        "playlist_remove",
        Queue,
        GLOBAL,
        has_session,
        cmd_clear_insertions,
        undoable = true
    ),
    action!(
        "saveQueueAsPlaylist",
        "Save queue as playlist",
        "playlist_add_check",
        Queue,
        &[TargetKind::Global, TargetKind::SavedQueue],
        |t, s| matches!(t, ActionTarget::SavedQueue { .. }) || s.has_session,
        none,
        ui_handled = true
    ),
    action!(
        "restoreSavedQueue",
        "Restore queue",
        "history",
        Queue,
        SAVED,
        always,
        restore_saved_queue,
        undoable = true
    ),
    action!(
        "pinSavedQueue",
        "Pin queue",
        "push_pin",
        Queue,
        SAVED,
        always,
        pin_saved_queue
    ),
    action!(
        "unpinSavedQueue",
        "Unpin queue",
        "keep_off",
        Queue,
        SAVED,
        always,
        unpin_saved_queue
    ),
    action!(
        "deleteSavedQueue",
        "Delete saved queue",
        "delete_forever",
        Queue,
        SAVED,
        always,
        delete_saved_queue,
        destructive = true
    ),
    // -- library --
    action!(
        "addToPlaylist",
        "Add to playlist",
        "playlist_add",
        Library,
        &[
            TargetKind::Global,
            TargetKind::Tracks,
            TargetKind::Albums,
            TargetKind::QueueItems
        ],
        has_items,
        none,
        ui_handled = true
    ),
    action!(
        "removeFromPlaylist",
        "Remove from playlist",
        "playlist_remove",
        Library,
        &[TargetKind::Tracks],
        in_editable_playlist,
        remove_from_playlist,
        undoable = true
    ),
    action!(
        "rate",
        "Rate",
        "star_rate",
        Library,
        RATEABLE,
        has_items,
        none,
        ui_handled = true,
        media_session = Some(MediaSessionAction::Rate)
    ),
    action!(
        "rate0",
        "Clear rating",
        "star_outline",
        Library,
        RATEABLE,
        has_items,
        rate0,
        default_shortcut = Some("0"),
        undoable = true
    ),
    action!(
        "rate1",
        "Rate 1 star",
        "star",
        Library,
        RATEABLE,
        has_items,
        rate1,
        default_shortcut = Some("1"),
        undoable = true
    ),
    action!(
        "rate2",
        "Rate 2 stars",
        "star",
        Library,
        RATEABLE,
        has_items,
        rate2,
        default_shortcut = Some("2"),
        undoable = true
    ),
    action!(
        "rate3",
        "Rate 3 stars",
        "star",
        Library,
        RATEABLE,
        has_items,
        rate3,
        default_shortcut = Some("3"),
        undoable = true
    ),
    action!(
        "rate4",
        "Rate 4 stars",
        "star",
        Library,
        RATEABLE,
        has_items,
        rate4,
        default_shortcut = Some("4"),
        undoable = true
    ),
    action!(
        "rate5",
        "Rate 5 stars",
        "star",
        Library,
        RATEABLE,
        has_items,
        rate5,
        default_shortcut = Some("5"),
        undoable = true
    ),
    action!(
        "love",
        "Love",
        "favorite",
        Library,
        LOVEABLE,
        love_enabled,
        love,
        media_session = Some(MediaSessionAction::Love),
        undoable = true
    ),
    action!(
        "unlove",
        "Unlove",
        "heart_minus",
        Library,
        LOVEABLE,
        unlove_enabled,
        unlove,
        undoable = true
    ),
    action!("download", "Download", "download", Library, PINNABLE, has_items, download),
    action!(
        "unpin",
        "Remove download",
        "download_done",
        Library,
        PINNABLE,
        has_items,
        unpin,
        destructive = true
    ),
    action!(
        "goToAlbum",
        "Go to album",
        "album",
        Library,
        TRACKISH,
        go_to_album,
        none,
        ui_handled = true
    ),
    action!(
        "goToArtist",
        "Go to artist",
        "artist",
        Library,
        TRACKISH,
        go_to_artist,
        none,
        ui_handled = true
    ),
    action!(
        "deletePlaylist",
        "Delete playlist",
        "delete_forever",
        Library,
        &[TargetKind::Playlists],
        has_selection,
        delete_playlist,
        destructive = true
    ),
    // -- edit --
    action!(
        "undo",
        "Undo",
        "undo",
        Edit,
        GLOBAL,
        can_undo,
        cmd_undo,
        default_shortcut = Some("Mod+Z")
    ),
    action!(
        "redo",
        "Redo",
        "redo",
        Edit,
        GLOBAL,
        can_redo,
        cmd_redo,
        default_shortcut = Some("Mod+Shift+Z")
    ),
    action!(
        "redoAlt",
        "Redo",
        "redo",
        Edit,
        GLOBAL,
        can_redo,
        cmd_redo,
        default_shortcut = Some("Mod+Y")
    ),
    action!(
        "restoreSelection",
        "Restore selection",
        "select_all",
        Edit,
        GLOBAL,
        |_, s| s.has_cleared_selection,
        cmd_restore_selection
    ),
    action!(
        "selectAll",
        "Select all",
        "select_all",
        Edit,
        GLOBAL,
        not_in_text_field,
        none,
        default_shortcut = Some("Mod+A"),
        ui_handled = true
    ),
    // -- view --
    action!(
        "openCommandPalette",
        "Command palette",
        "keyboard_command_key",
        View,
        GLOBAL,
        always,
        none,
        default_shortcut = Some("Mod+K"),
        ui_handled = true
    ),
    action!(
        "findInList",
        "Find in list",
        "search",
        View,
        GLOBAL,
        always,
        none,
        default_shortcut = Some("Mod+F"),
        ui_handled = true
    ),
    action!(
        "toggleQueuePanel",
        "Queue panel",
        "queue_music",
        View,
        GLOBAL,
        not_in_text_field,
        none,
        default_shortcut = Some("Q"),
        ui_handled = true
    ),
    action!(
        "toggleFullscreen",
        "Fullscreen player",
        "fullscreen",
        View,
        GLOBAL,
        not_in_text_field,
        none,
        default_shortcut = Some("F"),
        ui_handled = true
    ),
    action!(
        "toggleMiniPlayer",
        "Mini player",
        "picture_in_picture_alt",
        View,
        GLOBAL,
        not_in_text_field,
        none,
        default_shortcut = Some("M"),
        ui_handled = true
    ),
    action!(
        "toggleLyrics",
        "Lyrics",
        "lyrics",
        View,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    // -- session --
    action!(
        "sleepTimer",
        "Sleep timer",
        "bedtime",
        Session,
        GLOBAL,
        always,
        none,
        ui_handled = true
    ),
    action!(
        "stopAtEndOfTrack",
        "Stop at end of track",
        "bedtime",
        Session,
        GLOBAL,
        has_current,
        stop_at_end_of_track
    ),
    action!(
        "cancelSleepTimer",
        "Cancel sleep timer",
        "bedtime_off",
        Session,
        GLOBAL,
        |_, s| s.sleep_timer_active,
        cancel_sleep_timer
    ),
    action!(
        "handoff",
        "Play on another device",
        "cast",
        Session,
        GLOBAL,
        |_, s| s.peer_count > 0,
        cmd_open_handoff
    ),
    action!(
        "resumeHere",
        "Resume here",
        "play_circle",
        Session,
        GLOBAL,
        |_, s| s.has_resume_offer,
        cmd_resume_here
    ),
    action!(
        "dismissResumeOffer",
        "Dismiss",
        "close",
        Session,
        GLOBAL,
        |_, s| s.has_resume_offer,
        cmd_dismiss_resume
    ),
    // -- system --
    action!(
        "copyDiagnostics",
        "Copy diagnostics",
        "bug_report",
        System,
        GLOBAL,
        always,
        none,
        ui_handled = true
    ),
    // -- navigation (sidebar) --
    action!(
        "navigateHome",
        "Home",
        "home",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    // The phone's Library place (albums, artists, songs… in one screen). Not
    // in the default sidebar; a platform without such a place shows its
    // nearest one.
    action!(
        "navigateLibrary",
        "Library",
        "library_music",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateTracks",
        "Songs",
        "music_note",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateAlbums",
        "Albums",
        "album",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateArtists",
        "Artists",
        "artist",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigatePlaylists",
        "Playlists",
        "queue_music",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateGenres",
        "Genres",
        "category",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateRecent",
        "Recent queues",
        "history",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateDownloads",
        "Downloads",
        "download",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateStats",
        "Listening stats",
        "insights",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateSettings",
        "Settings",
        "settings",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
    action!(
        "navigateFilters",
        "Filters",
        "filter_alt",
        Navigation,
        GLOBAL,
        ui_nav,
        none,
        ui_handled = true
    ),
];

pub fn all() -> &'static [ActionDef] {
    ALL
}

const CONTEXT_MENU: &[&str] = &[
    "play",
    "playShuffled",
    "playNext",
    "playLater",
    "addToPlaylist",
    "love",
    "unlove",
    "rate5",
    "rate4",
    "rate3",
    "rate2",
    "rate1",
    "rate0",
    "download",
    "unpin",
    "goToAlbum",
    "goToArtist",
    "removeFromQueue",
    "removeFromPlaylist",
    "restoreSavedQueue",
    "pinSavedQueue",
    "unpinSavedQueue",
    "saveQueueAsPlaylist",
    "deleteSavedQueue",
    "deletePlaylist",
];

const SIDEBAR: &[&str] = &[
    "navigateHome",
    "navigateTracks",
    "navigateAlbums",
    "navigateArtists",
    "navigatePlaylists",
    "navigateGenres",
    "navigateRecent",
    "navigateFilters",
    "navigateDownloads",
    "navigateStats",
    "navigateSettings",
];

const MEDIA_SESSION: &[&str] = &[
    "previous",
    "togglePlay",
    "next",
    "shuffle",
    "repeat",
    "love",
];

const NOW_PLAYING: &[&str] = &[
    "love",
    "rate",
    "shuffle",
    "repeat",
    "autoplay",
    "addToPlaylist",
    "goToAlbum",
    "goToArtist",
    "download",
    "sleepTimer",
    "handoff",
    "saveQueueAsPlaylist",
    "toggleLyrics",
    "toggleQueuePanel",
    "toggleFullscreen",
    "toggleMiniPlayer",
];

/// Default choose-and-order list per surface. The palette lists everything.
pub fn default_order(surface: Surface) -> Vec<&'static str> {
    match surface {
        Surface::ContextMenu => CONTEXT_MENU.to_vec(),
        Surface::Sidebar => SIDEBAR.to_vec(),
        Surface::MediaSession => MEDIA_SESSION.to_vec(),
        Surface::NowPlaying => NOW_PLAYING.to_vec(),
        Surface::Palette => ALL.iter().map(|d| d.id).collect(),
    }
}
