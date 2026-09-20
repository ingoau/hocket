//! Action registry: every user-visible action as a first-class object.
//! Owner: core-session.
//!
//! Menus, context menus, keyboard shortcuts, the command palette, media
//! session buttons, remote actions and undo labels are all generated from
//! here (design.md "The action registry").
//!
//! # Entry points for the actor
//!
//! - [`ActionRegistry::new(platform)`](ActionRegistry::new) — one per core.
//! - [`ActionRegistry::actions_for(surface, target, state)`] answers
//!   `Query::Actions`: the descriptors for a [`Surface`], filtered by
//!   applicability to the [`ActionTarget`] and ordered by the user's
//!   customisation (or the default order). `enabled` reflects the [`StateView`].
//! - [`ActionRegistry::commands(id, target, state, resolver)`] turns
//!   `Command::RunAction` into the commands to dispatch. Actions flagged
//!   [`ActionDef::ui_handled`] (navigation, panels, pickers, clipboard) return
//!   no commands: the platform layer performs them by id after the core has
//!   validated applicability.
//! - [`ActionRegistry::set_order`] handles `Command::SetActionOrder`;
//!   [`ActionRegistry::set_shortcut`] handles `Command::SetShortcut` with
//!   conflict detection; [`ActionRegistry::shortcuts`] answers `Query::Shortcuts`;
//!   [`ActionRegistry::action_for_chord`] resolves a keypress.
//! - [`ActionRegistry::media_session_actions(state)`] is the ordered button
//!   set for the OS media session.
//! - [`ActionRegistry::to_json`] / [`ActionRegistry::from_json`] persist the
//!   customisation (the settings subsystem stores the string).
//! - [`ActionRegistry::label(id)`] is the undo label for an action.
//!
//! Chords are normalised to `Ctrl+Alt+Shift+Meta+Key` order with the primary
//! modifier written as `Cmd` on macOS and `Ctrl` elsewhere; the persisted form
//! writes it as `Mod` so a config moves between platforms.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::api::{
    ActionDescriptor, ActionTarget, Command, ContextKind, MediaSessionAction, PinTarget, Platform,
    PlayContextArgs, PlaylistId, QueueContext, QueueKey, RatingTarget, RepeatMode, ServerId,
    Shortcut, SortOrder, TrackId,
};

pub mod chord;
mod defs;

pub use chord::{normalise, to_portable, ChordError};
pub use defs::{ActionDef, Category, TargetKind};

/// Seek step for the arrow keys.
pub const SEEK_STEP_MS: i32 = 10_000;

/// Where actions are shown. Each has a default choose-and-order list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Surface {
    ContextMenu,
    Sidebar,
    MediaSession,
    Palette,
    NowPlaying,
}

impl Surface {
    pub const ALL: [Surface; 5] = [
        Surface::ContextMenu,
        Surface::Sidebar,
        Surface::MediaSession,
        Surface::Palette,
        Surface::NowPlaying,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Surface::ContextMenu => "contextMenu",
            Surface::Sidebar => "sidebar",
            Surface::MediaSession => "mediaSession",
            Surface::Palette => "palette",
            Surface::NowPlaying => "nowPlaying",
        }
    }

    pub fn parse(s: &str) -> Option<Surface> {
        Surface::ALL.iter().copied().find(|x| x.as_str() == s)
    }

    /// Surfaces where global (target-less) actions show regardless of the
    /// current target. The context menu is strictly about its target.
    fn is_global(&self) -> bool {
        !matches!(self, Surface::ContextMenu)
    }
}

/// The minimal state the applicability predicates need. The actor fills it
/// from its subsystems; everything defaults to "nothing loaded".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateView {
    /// The active server (the registry always has exactly one for now).
    pub server_id: Option<ServerId>,
    /// A context is loaded.
    pub has_session: bool,
    pub has_current: bool,
    pub is_playing: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub autoplay: bool,
    pub current_track_id: Option<TrackId>,
    pub current_album_id: Option<String>,
    pub current_artist_id: Option<String>,
    pub current_loved: bool,
    pub current_rating: u32,
    /// The playlist the selection is being viewed in, if any, and whether it
    /// is smart (read-only).
    pub viewing_playlist: Option<PlaylistId>,
    pub viewing_playlist_is_smart: bool,
    /// Row indices of the selection within the viewed list (playlist removal).
    pub selection_indices: Vec<u32>,
    pub queue_panel_open: bool,
    pub fullscreen: bool,
    pub mini_player: bool,
    pub sleep_timer_active: bool,
    /// Devices that could take over playback.
    pub peer_count: u32,
    pub has_resume_offer: bool,
    pub has_cleared_selection: bool,
    /// Ctrl+Z belongs to the field.
    pub text_field_focused: bool,
    pub saved_queue_count: u32,
}

/// Resolves what handlers need from the mirror. The actor implements it;
/// [`NoResolver`] answers nothing (tests, pure descriptors).
pub trait Resolver {
    fn track_for_key(&self, key: &QueueKey) -> Option<TrackId>;
    fn context_tracks(&self, server_id: &str, kind: &ContextKind) -> Vec<TrackId>;
    fn context_label(&self, server_id: &str, kind: &ContextKind) -> String;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoResolver;

impl Resolver for NoResolver {
    fn track_for_key(&self, _key: &QueueKey) -> Option<TrackId> {
        None
    }
    fn context_tracks(&self, _server_id: &str, _kind: &ContextKind) -> Vec<TrackId> {
        vec![]
    }
    fn context_label(&self, _server_id: &str, kind: &ContextKind) -> String {
        match kind {
            ContextKind::Album { id }
            | ContextKind::Artist { id }
            | ContextKind::Playlist { id } => id.clone(),
            ContextKind::Genre { name } => name.clone(),
            ContextKind::Filter { filter } => filter.name.clone(),
            ContextKind::AdHoc { label } => label.clone(),
            ContextKind::Autoplay => "Autoplay".into(),
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ActionError {
    #[error("unknown action {0}")]
    UnknownAction(String),
    #[error("action {0} does not apply to this target")]
    NotApplicable(String),
    #[error("action {0} is disabled in the current state")]
    Disabled(String),
    #[error("unknown surface {0}")]
    UnknownSurface(String),
    #[error("{0}")]
    Chord(#[from] ChordError),
    #[error("shortcut {chord} is already bound to {other_action_id}")]
    Conflict {
        chord: String,
        other_action_id: String,
    },
}

/// Persisted customisation: surface orders and shortcut overrides (portable chords).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionCustomisation {
    #[serde(default = "customisation_version")]
    pub version: u32,
    /// Surface → chosen action ids in order.
    #[serde(default)]
    pub orders: BTreeMap<String, Vec<String>>,
    /// Action id → portable chord, or `None` for "unbound".
    #[serde(default)]
    pub shortcuts: BTreeMap<String, Option<String>>,
}

fn customisation_version() -> u32 {
    1
}

pub struct ActionRegistry {
    platform: Platform,
    orders: BTreeMap<Surface, Vec<String>>,
    /// Concrete (platform) chords; `None` = explicitly unbound.
    overrides: BTreeMap<String, Option<String>>,
}

impl ActionRegistry {
    pub fn new(platform: Platform) -> ActionRegistry {
        ActionRegistry {
            platform,
            orders: BTreeMap::new(),
            overrides: BTreeMap::new(),
        }
    }

    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Every definition, in registry order.
    pub fn definitions(&self) -> &'static [ActionDef] {
        defs::all()
    }

    pub fn get(&self, id: &str) -> Option<&'static ActionDef> {
        defs::all().iter().find(|d| d.id == id)
    }

    /// The undo label for an action.
    pub fn label(&self, id: &str) -> Option<&'static str> {
        self.get(id).map(|d| d.label)
    }

    // -- descriptors -------------------------------------------------------

    fn describe(
        &self,
        def: &ActionDef,
        target: &ActionTarget,
        state: &StateView,
    ) -> ActionDescriptor {
        ActionDescriptor {
            id: def.id.into(),
            label: def.label.into(),
            icon: def.icon.into(),
            category: def.category.as_str().into(),
            enabled: (def.enabled)(target, state),
            default_shortcut: self.shortcut_for(def.id),
            undoable: def.undoable,
            destructive: def.destructive,
        }
    }

    /// One descriptor regardless of surface.
    pub fn descriptor(
        &self,
        id: &str,
        target: &ActionTarget,
        state: &StateView,
    ) -> Option<ActionDescriptor> {
        self.get(id).map(|d| self.describe(d, target, state))
    }

    /// Whether the action accepts this target at all (on a global surface a
    /// global action always does).
    fn applies(def: &ActionDef, target: &ActionTarget, surface: Option<Surface>) -> bool {
        let kind = TargetKind::of(target);
        if def.targets.contains(&kind) {
            return true;
        }
        if def.targets.contains(&TargetKind::Global) {
            return kind == TargetKind::Global || surface.map(|s| s.is_global()).unwrap_or(false);
        }
        false
    }

    /// The chosen action ids for a surface (customised or default).
    pub fn order(&self, surface: Surface) -> Vec<String> {
        match self.orders.get(&surface) {
            Some(o) => o.clone(),
            None => defs::default_order(surface)
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    pub fn default_order(&self, surface: Surface) -> Vec<String> {
        defs::default_order(surface)
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Choose-and-order customisation. Unknown ids are dropped; passing the
    /// default order clears the customisation.
    pub fn set_order(&mut self, surface: Surface, ids: Vec<String>) {
        let mut seen = std::collections::HashSet::new();
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| self.get(id).is_some() && seen.insert(id.clone()))
            .collect();
        if ids == self.default_order(surface) {
            self.orders.remove(&surface);
        } else {
            self.orders.insert(surface, ids);
        }
    }

    pub fn set_order_str(&mut self, surface: &str, ids: Vec<String>) -> Result<(), ActionError> {
        let s =
            Surface::parse(surface).ok_or_else(|| ActionError::UnknownSurface(surface.into()))?;
        self.set_order(s, ids);
        Ok(())
    }

    /// Applicable actions for a surface, in the user's order.
    pub fn actions_for(
        &self,
        surface: Surface,
        target: &ActionTarget,
        state: &StateView,
    ) -> Vec<ActionDescriptor> {
        self.order(surface)
            .iter()
            .filter_map(|id| self.get(id))
            .filter(|d| Self::applies(d, target, Some(surface)))
            .map(|d| self.describe(d, target, state))
            .collect()
    }

    pub fn actions_for_str(
        &self,
        surface: &str,
        target: &ActionTarget,
        state: &StateView,
    ) -> Result<Vec<ActionDescriptor>, ActionError> {
        let s =
            Surface::parse(surface).ok_or_else(|| ActionError::UnknownSurface(surface.into()))?;
        Ok(self.actions_for(s, target, state))
    }

    /// Every action that applies to the target on a global surface (the
    /// palette's action half), enabled or not.
    pub fn all_for(&self, target: &ActionTarget, state: &StateView) -> Vec<ActionDescriptor> {
        defs::all()
            .iter()
            .filter(|d| Self::applies(d, target, Some(Surface::Palette)))
            .map(|d| self.describe(d, target, state))
            .collect()
    }

    // -- running -------------------------------------------------------------

    /// Validate applicability and produce the commands to dispatch. An empty
    /// list for a `ui_handled` action means "the platform does this".
    pub fn commands(
        &self,
        id: &str,
        target: &ActionTarget,
        state: &StateView,
        resolver: &dyn Resolver,
    ) -> Result<Vec<Command>, ActionError> {
        let def = self
            .get(id)
            .ok_or_else(|| ActionError::UnknownAction(id.into()))?;
        if !Self::applies(def, target, None) && !(def.targets.contains(&TargetKind::Global)) {
            return Err(ActionError::NotApplicable(id.into()));
        }
        if !(def.enabled)(target, state) {
            return Err(ActionError::Disabled(id.into()));
        }
        Ok((def.commands)(target, state, resolver))
    }

    /// Ordered media-session buttons from the `mediaSession` surface.
    pub fn media_session_actions(&self, state: &StateView) -> Vec<MediaSessionAction> {
        let mut out = vec![];
        for id in self.order(Surface::MediaSession) {
            let Some(def) = self.get(&id) else { continue };
            let Some(ms) = def.media_session else {
                continue;
            };
            if !(def.enabled)(&ActionTarget::None, state) {
                continue;
            }
            match ms {
                MediaSessionAction::Play | MediaSessionAction::Pause => {
                    let a = if state.is_playing {
                        MediaSessionAction::Pause
                    } else {
                        MediaSessionAction::Play
                    };
                    if !out.contains(&a) {
                        out.push(a);
                    }
                }
                other => {
                    if !out.contains(&other) {
                        out.push(other);
                    }
                }
            }
        }
        if state.has_current {
            for extra in [MediaSessionAction::Seek, MediaSessionAction::Stop] {
                if !out.contains(&extra) {
                    out.push(extra);
                }
            }
        }
        out
    }

    // -- shortcuts ---------------------------------------------------------

    /// The platform's rendering of an action's default chord.
    pub fn default_shortcut(&self, id: &str) -> Option<String> {
        self.get(id)
            .and_then(|d| d.default_shortcut)
            .and_then(|c| normalise(c, self.platform).ok())
    }

    /// The effective chord: override, else default.
    pub fn shortcut_for(&self, id: &str) -> Option<String> {
        match self.overrides.get(id) {
            Some(o) => o.clone(),
            None => self.default_shortcut(id),
        }
    }

    /// Which action a chord (any spelling) currently fires.
    pub fn action_for_chord(&self, chord: &str) -> Option<&'static str> {
        let chord = normalise(chord, self.platform).ok()?;
        defs::all()
            .iter()
            .find(|d| self.shortcut_for(d.id).as_deref() == Some(chord.as_str()))
            .map(|d| d.id)
    }

    /// Rebind (`Some(chord)`) or unbind (`None`). Rejects unknown actions,
    /// unparseable chords and chords another action already uses.
    pub fn set_shortcut(&mut self, id: &str, chord: Option<&str>) -> Result<(), ActionError> {
        if self.get(id).is_none() {
            return Err(ActionError::UnknownAction(id.into()));
        }
        let chord = match chord {
            Some(c) => Some(normalise(c, self.platform)?),
            None => None,
        };
        if let Some(c) = &chord {
            if let Some(other) = self.action_for_chord(c) {
                if other != id {
                    return Err(ActionError::Conflict {
                        chord: c.clone(),
                        other_action_id: other.into(),
                    });
                }
            }
        }
        if chord == self.default_shortcut(id) {
            self.overrides.remove(id);
        } else {
            self.overrides.insert(id.into(), chord);
        }
        Ok(())
    }

    /// Reset one action to its default chord.
    pub fn reset_shortcut(&mut self, id: &str) {
        self.overrides.remove(id);
    }

    /// Every action that has a default or an override, for `Query::Shortcuts`.
    pub fn shortcuts(&self) -> Vec<Shortcut> {
        defs::all()
            .iter()
            .filter(|d| d.default_shortcut.is_some() || self.overrides.contains_key(d.id))
            .map(|d| Shortcut {
                action_id: d.id.into(),
                shortcut: self.shortcut_for(d.id),
                default_shortcut: self.default_shortcut(d.id),
            })
            .collect()
    }

    // -- persistence ---------------------------------------------------------

    pub fn customisation(&self) -> ActionCustomisation {
        ActionCustomisation {
            version: 1,
            orders: self
                .orders
                .iter()
                .map(|(s, ids)| (s.as_str().to_string(), ids.clone()))
                .collect(),
            shortcuts: self
                .overrides
                .iter()
                .map(|(id, c)| {
                    (
                        id.clone(),
                        c.as_deref().map(|c| to_portable(c, self.platform)),
                    )
                })
                .collect(),
        }
    }

    /// Replace the customisation. Unknown ids and chords that fail to parse
    /// or conflict are dropped rather than failing the whole load.
    pub fn apply_customisation(&mut self, c: &ActionCustomisation) {
        self.orders.clear();
        self.overrides.clear();
        for (surface, ids) in &c.orders {
            if let Some(s) = Surface::parse(surface) {
                self.set_order(s, ids.clone());
            }
        }
        for (id, chord) in &c.shortcuts {
            let _ = self.set_shortcut(id, chord.as_deref());
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.customisation()).unwrap_or_else(|_| "{}".into())
    }

    pub fn from_json(&mut self, json: &str) -> Result<(), serde_json::Error> {
        let c: ActionCustomisation = serde_json::from_str(json)?;
        self.apply_customisation(&c);
        Ok(())
    }
}

// -- helpers shared with the definitions ------------------------------------

fn ids_of(target: &ActionTarget) -> &[String] {
    match target {
        ActionTarget::Tracks { ids }
        | ActionTarget::Albums { ids }
        | ActionTarget::Artists { ids }
        | ActionTarget::Playlists { ids } => ids,
        ActionTarget::QueueItems { keys } => keys,
        _ => &[],
    }
}

fn single_id(target: &ActionTarget) -> Option<&str> {
    let ids = ids_of(target);
    (ids.len() == 1).then(|| ids[0].as_str())
}

fn context_of(
    target: &ActionTarget,
    server_id: &str,
    resolver: &dyn Resolver,
) -> Option<QueueContext> {
    let kind = match target {
        ActionTarget::Albums { ids } if ids.len() == 1 => ContextKind::Album { id: ids[0].clone() },
        ActionTarget::Artists { ids } if ids.len() == 1 => {
            ContextKind::Artist { id: ids[0].clone() }
        }
        ActionTarget::Playlists { ids } if ids.len() == 1 => {
            ContextKind::Playlist { id: ids[0].clone() }
        }
        _ => return None,
    };
    Some(QueueContext {
        server_id: server_id.into(),
        label: resolver.context_label(server_id, &kind),
        tracks: resolver.context_tracks(server_id, &kind),
        kind,
        sort: SortOrder::Default,
    })
}

/// Track ids a target stands for (resolving queue keys and containers).
fn track_ids_of(target: &ActionTarget, state: &StateView, resolver: &dyn Resolver) -> Vec<TrackId> {
    let server = state.server_id.clone().unwrap_or_default();
    match target {
        ActionTarget::Tracks { ids } => ids.clone(),
        ActionTarget::QueueItems { keys } => keys
            .iter()
            .filter_map(|k| resolver.track_for_key(k))
            .collect(),
        ActionTarget::Albums { ids } => ids
            .iter()
            .flat_map(|id| resolver.context_tracks(&server, &ContextKind::Album { id: id.clone() }))
            .collect(),
        ActionTarget::Artists { ids } => ids
            .iter()
            .flat_map(|id| {
                resolver.context_tracks(&server, &ContextKind::Artist { id: id.clone() })
            })
            .collect(),
        ActionTarget::Playlists { ids } => ids
            .iter()
            .flat_map(|id| {
                resolver.context_tracks(&server, &ContextKind::Playlist { id: id.clone() })
            })
            .collect(),
        ActionTarget::None => state.current_track_id.iter().cloned().collect(),
        ActionTarget::SavedQueue { .. } => vec![],
    }
}

fn rating_targets(
    target: &ActionTarget,
    state: &StateView,
    resolver: &dyn Resolver,
) -> Vec<RatingTarget> {
    match target {
        ActionTarget::Albums { ids } => ids
            .iter()
            .map(|id| RatingTarget::Album { id: id.clone() })
            .collect(),
        _ => track_ids_of(target, state, resolver)
            .into_iter()
            .map(|id| RatingTarget::Track { id })
            .collect(),
    }
}

fn pin_targets(
    target: &ActionTarget,
    state: &StateView,
    resolver: &dyn Resolver,
) -> Vec<PinTarget> {
    match target {
        ActionTarget::Albums { ids } => ids
            .iter()
            .map(|id| PinTarget::Album { id: id.clone() })
            .collect(),
        ActionTarget::Playlists { ids } => ids
            .iter()
            .map(|id| PinTarget::Playlist { id: id.clone() })
            .collect(),
        _ => track_ids_of(target, state, resolver)
            .into_iter()
            .map(|id| PinTarget::Track { id })
            .collect(),
    }
}

fn play_commands(
    target: &ActionTarget,
    state: &StateView,
    resolver: &dyn Resolver,
    shuffle: bool,
) -> Vec<Command> {
    let server = state.server_id.clone().unwrap_or_default();
    match target {
        ActionTarget::None => vec![if shuffle {
            Command::SetShuffle { enabled: true }
        } else {
            Command::Play
        }],
        ActionTarget::QueueItems { keys } => keys
            .first()
            .map(|k| Command::JumpToQueueItem { key: k.clone() })
            .into_iter()
            .collect(),
        ActionTarget::SavedQueue { id } => vec![Command::RestoreSavedQueue { id: id.clone() }],
        _ => {
            if let Some(context) = context_of(target, &server, resolver) {
                return vec![Command::PlayContext {
                    args: PlayContextArgs {
                        context,
                        start_index: Some(0),
                        shuffle,
                        save_outgoing: true,
                    },
                }];
            }
            let track_ids = track_ids_of(target, state, resolver);
            if track_ids.is_empty() {
                return vec![];
            }
            vec![Command::PlayTracks {
                server_id: server,
                track_ids,
                start_index: 0,
                label: "Selection".into(),
                shuffle,
            }]
        }
    }
}

fn queue_commands(
    target: &ActionTarget,
    state: &StateView,
    resolver: &dyn Resolver,
    next: bool,
) -> Vec<Command> {
    let server_id = state.server_id.clone().unwrap_or_default();
    let track_ids = track_ids_of(target, state, resolver);
    if track_ids.is_empty() {
        return vec![];
    }
    vec![if next {
        Command::PlayNext {
            server_id,
            track_ids,
        }
    } else {
        Command::PlayLater {
            server_id,
            track_ids,
        }
    }]
}

#[cfg(test)]
mod tests;
