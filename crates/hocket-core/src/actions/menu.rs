//! Item menus: the song, album, artist, playlist and saved-queue menus.
//!
//! One layout for every platform (design.md "Item menus"). Each row has a
//! [`MenuSection`] (its place in the order), a default tier (in the phone's
//! short list or under More) and a priority (which default rows fold first
//! when the short list is full). [`menu_layout`] applies, in turn: what the
//! target and screen offer, the user's `menu.overrides`, and the fold rule.
//! Desktop draws every row, a separator between sections; the phone folds.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::api::{
    ActionDescriptor, ActionTarget, MenuItem, MenuItemInfo, MenuLayout, MenuScreen, MenuSection,
    MenuTier,
};

use super::{ids_of, ActionRegistry, StateView, Surface};

/// Most rows in the phone's short list by default, counting the app's own extras.
pub const SHORT_LIST_MAX: usize = 6;
/// A menu with this many rows or fewer shows them all, with no More.
pub const NO_FOLD_MAX: usize = 7;

/// What performs a row.
enum Source {
    /// Registry actions, in order (both halves of a pair).
    Actions(&'static [&'static str]),
    /// The app: offered when the app lists it in `app_items`.
    App,
}

struct Def {
    id: &'static str,
    section: MenuSection,
    /// Default tier: the short list or under More.
    folded: bool,
    /// Which default short-list rows stay when it is full: higher stays.
    priority: u8,
    /// Only for one item, not a selection.
    single: bool,
    source: Source,
}

const fn def(
    id: &'static str,
    section: MenuSection,
    folded: bool,
    priority: u8,
    single: bool,
    source: Source,
) -> Def {
    Def {
        id,
        section,
        folded,
        priority,
        single,
        source,
    }
}

use MenuSection::{About, Danger, GoTo, Keep, Organise, Play, Rate, Tune};
use Source::{Actions as A, App};

/// Every row, in display order.
static DEFS: &[Def] = &[
    def("play", Play, false, 0, false, A(&["play"])),
    def("playShuffled", Play, false, 0, false, A(&["playShuffled"])),
    def("playNext", Play, false, 0, false, A(&["playNext"])),
    def("addToQueue", Play, false, 0, false, A(&["addToQueue"])),
    def("playLater", Play, false, 0, false, A(&["playLater"])),
    def(
        "rating",
        Rate,
        false,
        0,
        false,
        A(&["rate5", "rate4", "rate3", "rate2", "rate1", "rate0"]),
    ),
    def("love", Rate, false, 0, false, A(&["love", "unlove"])),
    def(
        "addToPlaylist",
        Organise,
        false,
        80,
        false,
        A(&["addToPlaylist"]),
    ),
    def("stash", Organise, false, 60, false, A(&["stash"])),
    def(
        "pinToLibrary",
        Organise,
        true,
        30,
        true,
        A(&["pinToLibrary", "unpinFromLibrary"]),
    ),
    def(
        "addToLibrary",
        Organise,
        true,
        25,
        false,
        A(&["addToLibrary", "removeFromLibrary"]),
    ),
    def(
        "restoreSavedQueue",
        Organise,
        false,
        90,
        false,
        A(&["restoreSavedQueue"]),
    ),
    def(
        "pinSavedQueue",
        Organise,
        false,
        70,
        false,
        A(&["pinSavedQueue", "unpinSavedQueue"]),
    ),
    def(
        "saveQueueAsPlaylist",
        Organise,
        false,
        65,
        false,
        A(&["saveQueueAsPlaylist"]),
    ),
    def("goToAlbum", GoTo, false, 95, true, A(&["goToAlbum"])),
    def("goToArtist", GoTo, false, 100, true, A(&["goToArtist"])),
    def(
        "download",
        Keep,
        false,
        20,
        false,
        A(&["download", "unpin"]),
    ),
    def("share", Keep, false, 50, false, App),
    def("preview", About, true, 15, true, A(&["preview"])),
    def("info", About, true, 12, true, App),
    def("genius", About, true, 10, true, App),
    def("lyricsTiming", About, true, 8, true, App),
    def("snooze", Tune, true, 5, false, A(&["snooze", "unsnooze"])),
    def(
        "removeFromQueue",
        Danger,
        false,
        0,
        false,
        A(&["removeFromQueue"]),
    ),
    def(
        "removeFromPlaylist",
        Danger,
        false,
        0,
        false,
        A(&["removeFromPlaylist"]),
    ),
    def(
        "deleteSavedQueue",
        Danger,
        false,
        0,
        false,
        A(&["deleteSavedQueue"]),
    ),
    def(
        "deletePlaylist",
        Danger,
        false,
        0,
        false,
        A(&["deletePlaylist"]),
    ),
];

fn find(id: &str) -> Option<&'static Def> {
    DEFS.iter().find(|d| d.id == id)
}

/// The tiers a row may take. The play strip isn't customisable; Rating, Love
/// and removing are shown or hidden, never folded.
fn choices(d: &Def) -> &'static [MenuTier] {
    match d.section {
        Play => &[],
        Rate | Danger => &[MenuTier::Shown, MenuTier::Hidden],
        _ => &[MenuTier::Shown, MenuTier::More, MenuTier::Hidden],
    }
}

fn default_tier(d: &Def) -> MenuTier {
    if d.folded {
        MenuTier::More
    } else {
        MenuTier::Shown
    }
}

/// Every registry action an item menu can show, in menu order: the
/// context-menu surface's default order.
pub fn registry_ids() -> Vec<&'static str> {
    DEFS.iter()
        .flat_map(|d| match d.source {
            Source::Actions(ids) => ids.to_vec(),
            Source::App => vec![],
        })
        .collect()
}

/// The `menu.overrides` setting: `{ id: "shown" | "more" | "hidden" }`.
pub fn overrides_valid(v: &Value) -> Result<(), String> {
    let map = v.as_object().ok_or("expected an object")?;
    for (id, tier) in map {
        let d = find(id).ok_or_else(|| format!("unknown menu item {id}"))?;
        let tier: MenuTier = serde_json::from_value(tier.clone())
            .map_err(|_| format!("{id}: expected shown, more or hidden"))?;
        if !choices(d).contains(&tier) {
            return Err(format!("{id} can't be {}", tier_str(tier)));
        }
    }
    Ok(())
}

fn tier_str(t: MenuTier) -> &'static str {
    match t {
        MenuTier::Shown => "shown",
        MenuTier::More => "more",
        MenuTier::Hidden => "hidden",
    }
}

/// Parses `menu.overrides`, skipping anything [`overrides_valid`] would refuse
/// (a newer device's rows).
pub fn parse_overrides(v: &Value) -> BTreeMap<String, MenuTier> {
    v.as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, t)| {
            let d = find(id)?;
            let t: MenuTier = serde_json::from_value(t.clone()).ok()?;
            choices(d).contains(&t).then(|| (id.clone(), t))
        })
        .collect()
}

/// `menu.overrides` with `id` set to `tier` (its default removes the entry).
pub fn with_override(current: &Value, id: &str, tier: MenuTier) -> Result<Value, String> {
    let d = find(id).ok_or_else(|| format!("unknown menu item {id}"))?;
    if !choices(d).contains(&tier) {
        return Err(format!("{id} can't be {}", tier_str(tier)));
    }
    let mut map = current.as_object().cloned().unwrap_or_default();
    if tier == default_tier(d) {
        map.remove(id);
    } else {
        map.insert(id.into(), Value::String(tier_str(tier).into()));
    }
    Ok(Value::Object(map))
}

/// The settings editor's list: every customisable row in menu order.
pub fn items_info(overrides: &BTreeMap<String, MenuTier>) -> Vec<MenuItemInfo> {
    DEFS.iter()
        .filter(|d| !choices(d).is_empty())
        .map(|d| MenuItemInfo {
            id: d.id.into(),
            section: d.section,
            default_tier: default_tier(d),
            tier: overrides.get(d.id).copied().unwrap_or(default_tier(d)),
            choices: choices(d).to_vec(),
            app_handled: matches!(d.source, Source::App),
        })
        .collect()
}

/// What a menu needs besides the registry.
pub struct MenuRequest<'a> {
    pub target: &'a ActionTarget,
    pub screen: MenuScreen,
    pub app_items: &'a [String],
    pub extras: u32,
    pub overrides: &'a BTreeMap<String, MenuTier>,
}

/// Whether the screen leaves the row out: the player doesn't preview what is
/// playing, a page doesn't link to itself, and only an editable playlist
/// removes songs from itself.
fn screen_drops(id: &str, screen: MenuScreen) -> bool {
    match id {
        "preview" => screen == MenuScreen::NowPlaying,
        "goToAlbum" => screen == MenuScreen::AlbumPage,
        "goToArtist" => screen == MenuScreen::ArtistPage,
        "removeFromPlaylist" => screen != MenuScreen::Playlist,
        _ => false,
    }
}

/// The menu for a target, in display order.
pub fn menu_layout(registry: &ActionRegistry, req: &MenuRequest, state: &StateView) -> MenuLayout {
    let count = match req.target {
        ActionTarget::SavedQueue { .. } => 1,
        t => ids_of(t).len(),
    };
    let mut rows: Vec<(&Def, Vec<ActionDescriptor>, Option<MenuTier>)> = vec![];
    for d in DEFS {
        let tier = req.overrides.get(d.id).copied();
        if tier == Some(MenuTier::Hidden) || screen_drops(d.id, req.screen) {
            continue;
        }
        if d.single && count > 1 {
            continue;
        }
        let actions = match d.source {
            Source::App => {
                if !req.app_items.iter().any(|a| a == d.id) {
                    continue;
                }
                vec![]
            }
            Source::Actions(ids) => {
                let found: Vec<ActionDescriptor> = ids
                    .iter()
                    .filter_map(|id| registry.get(id))
                    .filter(|a| ActionRegistry::applies(a, req.target, Some(Surface::ContextMenu)))
                    .filter(|a| ActionRegistry::offered(a, req.target, state))
                    .map(|a| registry.describe(a, req.target, state))
                    .collect();
                if found.is_empty() {
                    continue;
                }
                found
            }
        };
        rows.push((d, actions, tier));
    }

    // The fold: only the list rows (not the strip, the stars, Love or removing).
    let foldable = |d: &Def| !matches!(d.section, Play | Rate | Danger);
    let extras = req.extras as usize;
    let listed = rows.iter().filter(|(d, ..)| foldable(d)).count();
    let mut folded = vec![false; rows.len()];
    if listed + extras > NO_FOLD_MAX {
        // Rows the user put in the menu always stay; the defaults fill what's
        // left of the short list by priority.
        let pinned = rows
            .iter()
            .filter(|(d, _, t)| foldable(d) && *t == Some(MenuTier::Shown))
            .count();
        let mut room = SHORT_LIST_MAX.saturating_sub(extras).saturating_sub(pinned);
        let mut by_priority: Vec<usize> = (0..rows.len())
            .filter(|&i| foldable(rows[i].0) && rows[i].2.is_none() && !rows[i].0.folded)
            .collect();
        by_priority.sort_by_key(|&i| std::cmp::Reverse(rows[i].0.priority));
        for (i, (d, _, t)) in rows.iter().enumerate() {
            if !foldable(d) {
                continue;
            }
            folded[i] = match t {
                Some(MenuTier::Shown) => false,
                Some(_) => true,
                None => d.folded,
            };
        }
        for i in by_priority {
            if room > 0 {
                room -= 1;
            } else {
                folded[i] = true;
            }
        }
        // A More holding one row is a tap for nothing.
        if folded.iter().filter(|f| **f).count() == 1 {
            folded.iter_mut().for_each(|f| *f = false);
        }
    }

    MenuLayout {
        items: rows
            .into_iter()
            .zip(folded)
            .map(|((d, actions, _), folded)| MenuItem {
                id: d.id.into(),
                section: d.section,
                folded,
                actions,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Platform, TrackRef};
    use serde_json::json;

    fn state() -> StateView {
        StateView {
            server_id: Some("srv".into()),
            has_session: true,
            has_current: true,
            current_track_id: Some("cur".into()),
            queue_section: true,
            ..Default::default()
        }
    }

    fn tracks(ids: &[&str]) -> ActionTarget {
        ActionTarget::Tracks {
            tracks: TrackRef::list("srv", ids.iter().copied()),
        }
    }

    fn apps(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    /// (shown list rows, folded rows), leaving out the strip, the stars, Love and removing.
    fn layout(
        target: &ActionTarget,
        screen: MenuScreen,
        app_items: &[&str],
        extras: u32,
        overrides: Value,
    ) -> (Vec<String>, Vec<String>, MenuLayout) {
        let r = ActionRegistry::new(Platform::Linux);
        let o = parse_overrides(&overrides);
        let app = apps(app_items);
        let req = MenuRequest {
            target,
            screen,
            app_items: &app,
            extras,
            overrides: &o,
        };
        let m = menu_layout(&r, &req, &state());
        let rows = |folded: bool| {
            m.items
                .iter()
                .filter(|i| !matches!(i.section, Play | Rate | Danger) && i.folded == folded)
                .map(|i| i.id.clone())
                .collect::<Vec<_>>()
        };
        (rows(false), rows(true), m)
    }

    const SONG_APPS: &[&str] = &["share", "info", "genius"];

    #[test]
    fn a_song_keeps_both_go_to_rows_in_the_short_list() {
        let (shown, more, m) = layout(&tracks(&["a"]), MenuScreen::List, SONG_APPS, 0, json!({}));
        assert_eq!(
            shown,
            [
                "addToPlaylist",
                "stash",
                "goToAlbum",
                "goToArtist",
                "download",
                "share"
            ]
        );
        assert_eq!(
            more,
            ["pinToLibrary", "preview", "info", "genius", "snooze"]
        );
        // One song has no Shuffle.
        let ids: Vec<&str> = m.items.iter().map(|i| i.id.as_str()).collect();
        assert!(ids.starts_with(&[
            "play",
            "playNext",
            "addToQueue",
            "playLater",
            "rating",
            "love"
        ]));
        assert!(!ids.contains(&"removeFromPlaylist"));
        let rating = m.items.iter().find(|i| i.id == "rating").unwrap();
        assert_eq!(rating.actions.len(), 6);
    }

    #[test]
    fn the_player_drops_preview_and_its_sleep_timer_takes_downloads_place() {
        let (shown, more, _) = layout(
            &tracks(&["a"]),
            MenuScreen::NowPlaying,
            &["share", "info", "genius", "lyricsTiming"],
            1,
            json!({}),
        );
        assert_eq!(
            shown,
            ["addToPlaylist", "stash", "goToAlbum", "goToArtist", "share"]
        );
        assert_eq!(
            more,
            [
                "pinToLibrary",
                "download",
                "info",
                "genius",
                "lyricsTiming",
                "snooze"
            ]
        );
    }

    #[test]
    fn pages_drop_the_row_that_goes_to_themselves() {
        let (shown, ..) = layout(
            &tracks(&["a"]),
            MenuScreen::AlbumPage,
            SONG_APPS,
            0,
            json!({}),
        );
        assert!(
            !shown.contains(&"goToAlbum".to_string()) && shown.contains(&"goToArtist".to_string())
        );
        let (shown, ..) = layout(
            &tracks(&["a"]),
            MenuScreen::ArtistPage,
            SONG_APPS,
            0,
            json!({}),
        );
        assert!(
            shown.contains(&"goToAlbum".to_string()) && !shown.contains(&"goToArtist".to_string())
        );
    }

    #[test]
    fn only_an_editable_playlist_removes_songs_and_removing_is_never_folded() {
        let (.., m) = layout(
            &tracks(&["a"]),
            MenuScreen::Playlist,
            SONG_APPS,
            0,
            json!({}),
        );
        let remove = m
            .items
            .iter()
            .find(|i| i.id == "removeFromPlaylist")
            .unwrap();
        assert_eq!(remove.section, Danger);
        assert!(!remove.folded);
        assert_eq!(m.items.last().unwrap().id, "removeFromPlaylist");
    }

    #[test]
    fn short_menus_have_no_more() {
        // Several songs: no single-item rows, few enough to show them all.
        let (shown, more, _) = layout(
            &tracks(&["a", "b"]),
            MenuScreen::List,
            &["share"],
            0,
            json!({}),
        );
        assert_eq!(
            shown,
            ["addToPlaylist", "stash", "download", "share", "snooze"]
        );
        assert!(more.is_empty());
        // An album: seven rows, all shown, Go to artist among them.
        let album = ActionTarget::Albums {
            ids: vec!["al".into()],
        };
        let (shown, more, _) = layout(&album, MenuScreen::List, &["share", "info"], 0, json!({}));
        assert_eq!(
            shown,
            [
                "addToPlaylist",
                "stash",
                "pinToLibrary",
                "goToArtist",
                "download",
                "share",
                "info"
            ]
        );
        assert!(more.is_empty());
        // With Start over on top it folds Pin and the info.
        let (shown, more, _) = layout(&album, MenuScreen::List, &["share", "info"], 1, json!({}));
        assert_eq!(
            shown,
            ["addToPlaylist", "stash", "goToArtist", "download", "share"]
        );
        assert_eq!(more, ["pinToLibrary", "info"]);
    }

    #[test]
    fn overrides_promote_fold_and_hide() {
        let (shown, more, m) = layout(
            &tracks(&["a"]),
            MenuScreen::List,
            SONG_APPS,
            0,
            json!({"genius": "shown", "stash": "more", "rating": "hidden", "share": "hidden"}),
        );
        // Genius is the user's, so it stays; the defaults fill the rest by priority.
        assert_eq!(
            shown,
            [
                "addToPlaylist",
                "goToAlbum",
                "goToArtist",
                "download",
                "genius"
            ]
        );
        assert_eq!(more, ["stash", "pinToLibrary", "preview", "info", "snooze"]);
        assert!(m.items.iter().all(|i| i.id != "rating" && i.id != "share"));
    }

    #[test]
    fn the_setting_takes_only_known_rows_and_their_tiers() {
        assert!(overrides_valid(&json!({"genius": "shown", "love": "hidden"})).is_ok());
        assert!(overrides_valid(&json!({"nope": "shown"})).is_err());
        assert!(overrides_valid(&json!({"rating": "more"})).is_err());
        assert!(overrides_valid(&json!({"play": "hidden"})).is_err());
        assert!(overrides_valid(&json!({"deletePlaylist": "more"})).is_err());
        assert!(overrides_valid(&json!([])).is_err());
        let v = with_override(&json!({}), "genius", MenuTier::Shown).unwrap();
        assert_eq!(v, json!({"genius": "shown"}));
        assert_eq!(
            with_override(&v, "genius", MenuTier::More).unwrap(),
            json!({})
        );
        assert!(with_override(&v, "rating", MenuTier::More).is_err());
        let info = items_info(&parse_overrides(&json!({"genius": "shown", "x": "more"})));
        assert!(info.iter().all(|i| i.id != "play"));
        let g = info.iter().find(|i| i.id == "genius").unwrap();
        assert_eq!(
            (g.default_tier, g.tier, g.app_handled),
            (MenuTier::More, MenuTier::Shown, true)
        );
    }

    #[test]
    fn saved_queues_get_their_own_rows() {
        let saved = ActionTarget::SavedQueue { id: "q".into() };
        let (shown, more, m) = layout(&saved, MenuScreen::List, &[], 0, json!({}));
        assert!(more.is_empty());
        assert!(shown.starts_with(&["restoreSavedQueue".to_string(), "pinSavedQueue".to_string()]));
        assert_eq!(m.items.last().unwrap().id, "deleteSavedQueue");
    }
}
