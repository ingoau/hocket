//! In-memory evaluation of a filter tree against an [`api::Track`].
//!
//! Used for live previews, for filtering lists already in memory (the queue,
//! search results) and for the static-playlist helper. Mirrors the SQL in
//! [`super::sql`] exactly; the two are cross-checked in tests. Case folding
//! is ASCII-only on purpose: that is what SQLite's `NOCASE` and `LIKE` do.

use crate::api::{
    FilterField, FilterNode, FilterOp, FilterRule, FilterValue, OfflineState, SortOrder, Track,
    TrackId,
};

use super::dates::{CivilDate, MS_PER_DAY};
use super::model::{field_info, FieldKind};

/// Local knowledge about a track that `api::Track` does not carry: the
/// mirror's extra columns and playlist membership. All optional; the
/// defaults make the corresponding rules evaluate as "unknown → false".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalFacts {
    /// `tracks.has_lyrics`.
    pub has_lyrics: bool,
    /// `tracks.is_compilation`.
    pub is_compilation: bool,
    /// `tracks.local_play_count`.
    pub local_play_count: u32,
    /// `tracks.local_last_played`, epoch ms.
    pub local_last_played: Option<f64>,
    /// `tracks.changed` (server modification time), epoch ms.
    pub changed: Option<f64>,
    /// Playlists (ids) the track belongs to.
    pub playlist_ids: Vec<String>,
}

/// Evaluates `node` against a track with no extra local facts.
pub fn matches(node: &FilterNode, track: &Track, now_ms: f64) -> bool {
    matches_with(node, track, &LocalFacts::default(), now_ms)
}

/// Evaluates `node` against a track plus the mirror's local facts.
pub fn matches_with(node: &FilterNode, track: &Track, facts: &LocalFacts, now_ms: f64) -> bool {
    match node {
        FilterNode::Rule(rule) => rule_matches(rule, track, facts, now_ms),
        // An empty `all` is vacuously true, an empty `any` is false — same as SQL.
        FilterNode::All(children) => children
            .iter()
            .all(|c| matches_with(c, track, facts, now_ms)),
        FilterNode::Any(children) => children
            .iter()
            .any(|c| matches_with(c, track, facts, now_ms)),
    }
}

/// The comparable value of one field for a track.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    Text(Option<String>),
    Number(Option<f64>),
    /// Epoch ms.
    Date(Option<f64>),
    Bool(bool),
    Playlists(Vec<String>),
}

/// Extracts a field's value from a track (plus facts).
pub fn field_value(field: FilterField, track: &Track, facts: &LocalFacts) -> FieldValue {
    use FieldValue as V;
    match field {
        FilterField::Title => V::Text(Some(track.title.clone())),
        FilterField::Album => V::Text(track.album.clone()),
        FilterField::Artist => V::Text(track.artist.clone()),
        FilterField::AlbumArtist => V::Text(track.album_artist.clone()),
        FilterField::Genre => V::Text(track.genre.clone()),
        FilterField::Year => V::Number(track.year.map(f64::from)),
        FilterField::DateAdded => V::Date(track.created),
        FilterField::DateModified => V::Date(facts.changed),
        FilterField::LastPlayed => V::Date(track.last_played),
        FilterField::PlayCount => V::Number(Some(f64::from(track.play_count))),
        FilterField::Rating => V::Number(Some(f64::from(track.rating))),
        FilterField::Loved => V::Bool(track.loved),
        FilterField::Duration => V::Number(Some(f64::from(track.duration_ms) / 1000.0)),
        FilterField::BitRate => V::Number(track.bit_rate.map(f64::from)),
        FilterField::FilePath => V::Text(track.path.clone()),
        FilterField::FileType => V::Text(track.suffix.clone()),
        FilterField::Comment => V::Text(track.comment.clone()),
        FilterField::Lyrics => V::Bool(facts.has_lyrics),
        FilterField::HasCoverArt => V::Bool(track.cover_art.is_some()),
        FilterField::Compilation => V::Bool(facts.is_compilation),
        FilterField::DiscNumber => V::Number(track.disc_number.map(f64::from)),
        FilterField::TrackNumber => V::Number(track.track_number.map(f64::from)),
        FilterField::Bpm => V::Number(track.sonic.as_ref().and_then(|s| s.bpm)),
        FilterField::Key => V::Text(track.sonic.as_ref().and_then(|s| s.key.clone())),
        FilterField::Energy => V::Number(track.sonic.as_ref().and_then(|s| s.energy)),
        FilterField::Mood => V::Text(track.sonic.as_ref().and_then(|s| s.mood.clone())),
        FilterField::Downloaded => V::Bool(track.offline == OfflineState::Downloaded),
        FilterField::Cached => V::Bool(track.offline == OfflineState::Cached),
        FilterField::AvailableOffline => V::Bool(matches!(
            track.offline,
            OfflineState::Cached | OfflineState::Downloaded
        )),
        FilterField::LocalPlayCount => V::Number(Some(f64::from(facts.local_play_count))),
        FilterField::LocalLastPlayed => V::Date(facts.local_last_played),
        FilterField::InPlaylist => V::Playlists(facts.playlist_ids.clone()),
    }
}

/// Evaluates one rule. Malformed rules (wrong value shape) never match;
/// validate first if you want to know.
pub fn rule_matches(rule: &FilterRule, track: &Track, facts: &LocalFacts, now_ms: f64) -> bool {
    let value = field_value(rule.field, track, facts);
    match value {
        FieldValue::Text(actual) => text_matches(rule, actual.as_deref()),
        FieldValue::Number(actual) => number_matches(rule, actual),
        FieldValue::Date(actual) => date_matches(rule, actual, now_ms),
        FieldValue::Bool(actual) => match rule.op {
            FilterOp::IsTrue => actual,
            FilterOp::IsFalse => !actual,
            _ => false,
        },
        FieldValue::Playlists(ids) => match (&rule.op, &rule.value) {
            (FilterOp::Is, FilterValue::Text(id)) => ids.contains(id),
            (FilterOp::IsNot, FilterValue::Text(id)) => !ids.contains(id),
            (FilterOp::Is, FilterValue::List(wanted)) => wanted.iter().any(|w| ids.contains(w)),
            (FilterOp::IsNot, FilterValue::List(wanted)) => !wanted.iter().any(|w| ids.contains(w)),
            _ => false,
        },
    }
}

fn text_matches(rule: &FilterRule, actual: Option<&str>) -> bool {
    let actual_lower = actual.map(|s| s.to_ascii_lowercase());
    let actual_lower = actual_lower.as_deref();
    match (&rule.op, &rule.value) {
        (FilterOp::Is, FilterValue::Text(t)) => {
            actual_lower == Some(t.to_ascii_lowercase().as_str())
        }
        // NULL <> 'x' is NULL in SQL (no match); we mirror that.
        (FilterOp::IsNot, FilterValue::Text(t)) => {
            actual_lower.is_some_and(|a| a != t.to_ascii_lowercase())
        }
        (FilterOp::Is, FilterValue::List(items)) => {
            actual_lower.is_some_and(|a| items.iter().any(|i| i.to_ascii_lowercase() == a))
        }
        (FilterOp::IsNot, FilterValue::List(items)) => {
            actual_lower.is_some_and(|a| !items.iter().any(|i| i.to_ascii_lowercase() == a))
        }
        (FilterOp::Contains, FilterValue::Text(t)) => {
            actual_lower.is_some_and(|a| a.contains(&t.to_ascii_lowercase()))
        }
        (FilterOp::NotContains, FilterValue::Text(t)) => {
            actual_lower.is_some_and(|a| !a.contains(&t.to_ascii_lowercase()))
        }
        (FilterOp::StartsWith, FilterValue::Text(t)) => {
            actual_lower.is_some_and(|a| a.starts_with(&t.to_ascii_lowercase()))
        }
        (FilterOp::EndsWith, FilterValue::Text(t)) => {
            actual_lower.is_some_and(|a| a.ends_with(&t.to_ascii_lowercase()))
        }
        _ => false,
    }
}

fn number_matches(rule: &FilterRule, actual: Option<f64>) -> bool {
    let Some(a) = actual else { return false };
    match (&rule.op, &rule.value) {
        (FilterOp::Is, FilterValue::Number(n)) => a == *n,
        (FilterOp::IsNot, FilterValue::Number(n)) => a != *n,
        (FilterOp::Gt, FilterValue::Number(n)) => a > *n,
        (FilterOp::Lt, FilterValue::Number(n)) => a < *n,
        (FilterOp::InTheRange, FilterValue::Range { low, high }) => a >= *low && a <= *high,
        _ => false,
    }
}

/// Date semantics, shared with the SQL generator:
/// - `InTheLast n`: value ≥ now − n days.
/// - `NotInTheLast n`: value < now − n days, or never (NULL) — "not played in
///   the last month" must include never-played tracks, as Navidrome does.
/// - `Before d`: value < start of `d`. `After d`: value ≥ start of the day after `d`.
/// - `InTheRange [a, b]`: start of `a` ≤ value < day after `b`.
fn date_matches(rule: &FilterRule, actual: Option<f64>, now_ms: f64) -> bool {
    match (&rule.op, &rule.value) {
        (FilterOp::InTheLast, FilterValue::Days(n)) => {
            actual.is_some_and(|a| a >= now_ms - f64::from(*n) * MS_PER_DAY)
        }
        (FilterOp::NotInTheLast, FilterValue::Days(n)) => {
            actual.is_none_or(|a| a < now_ms - f64::from(*n) * MS_PER_DAY)
        }
        (FilterOp::Before, FilterValue::Date(d)) => match (actual, CivilDate::parse(d)) {
            (Some(a), Some(d)) => a < d.start_ms(),
            _ => false,
        },
        (FilterOp::After, FilterValue::Date(d)) => match (actual, CivilDate::parse(d)) {
            (Some(a), Some(d)) => a >= d.end_ms_exclusive(),
            _ => false,
        },
        (FilterOp::InTheRange, FilterValue::List(items)) if items.len() == 2 => {
            match (
                actual,
                CivilDate::parse(&items[0]),
                CivilDate::parse(&items[1]),
            ) {
                (Some(a), Some(from), Some(to)) => {
                    a >= from.start_ms() && a < to.end_ms_exclusive()
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Sorts tracks in memory the way the SQL `ORDER BY` in [`super::sql`] would,
/// applies `limit`, and returns the ids. `seed` drives `SortOrder::Random`.
pub fn order_tracks<'a>(
    tracks: impl IntoIterator<Item = &'a Track>,
    sort: SortOrder,
    descending: bool,
    limit: Option<u32>,
    seed: u64,
) -> Vec<TrackId> {
    let mut items: Vec<&Track> = tracks.into_iter().collect();
    if sort == SortOrder::Random {
        use rand::seq::SliceRandom;
        use rand::SeedableRng;
        let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
        items.shuffle(&mut rng);
    } else {
        items.sort_by(|a, b| {
            let primary = sort_key_cmp(sort, a, b);
            let primary = if descending {
                primary.reverse()
            } else {
                primary
            };
            primary.then_with(|| tie_break(a, b))
        });
    }
    let mut ids: Vec<TrackId> = items.into_iter().map(|t| t.id.clone()).collect();
    if let Some(l) = limit {
        ids.truncate(l as usize);
    }
    ids
}

fn sort_key_cmp(sort: SortOrder, a: &Track, b: &Track) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let text = |x: Option<&str>, y: Option<&str>| -> Ordering {
        match (x, y) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase()),
        }
    };
    let num = |x: Option<f64>, y: Option<f64>| -> Ordering {
        match (x, y) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        }
    };
    match sort {
        SortOrder::Default | SortOrder::Title => text(Some(&a.title), Some(&b.title)),
        SortOrder::Artist => text(a.artist.as_deref(), b.artist.as_deref())
            .then_with(|| text(a.album.as_deref(), b.album.as_deref()))
            .then_with(|| num(a.disc_number.map(f64::from), b.disc_number.map(f64::from)))
            .then_with(|| num(a.track_number.map(f64::from), b.track_number.map(f64::from))),
        SortOrder::Album => text(a.album.as_deref(), b.album.as_deref())
            .then_with(|| num(a.disc_number.map(f64::from), b.disc_number.map(f64::from)))
            .then_with(|| num(a.track_number.map(f64::from), b.track_number.map(f64::from))),
        SortOrder::Year => num(a.year.map(f64::from), b.year.map(f64::from)),
        SortOrder::DateAdded => num(a.created, b.created),
        SortOrder::Rating => num(Some(f64::from(a.rating)), Some(f64::from(b.rating))),
        SortOrder::PlayCount => num(Some(f64::from(a.play_count)), Some(f64::from(b.play_count))),
        SortOrder::Duration => num(
            Some(f64::from(a.duration_ms)),
            Some(f64::from(b.duration_ms)),
        ),
        SortOrder::Bpm => num(
            a.sonic.as_ref().and_then(|s| s.bpm),
            b.sonic.as_ref().and_then(|s| s.bpm),
        ),
        SortOrder::Energy => num(
            a.sonic.as_ref().and_then(|s| s.energy),
            b.sonic.as_ref().and_then(|s| s.energy),
        ),
        SortOrder::Random => Ordering::Equal,
    }
}

/// Deterministic final tie-break, identical to the SQL: title, then id.
fn tie_break(a: &Track, b: &Track) -> std::cmp::Ordering {
    a.title
        .to_ascii_lowercase()
        .cmp(&b.title.to_ascii_lowercase())
        .then_with(|| a.id.cmp(&b.id))
}

/// The value-kind a field's rule compares; exposed for builders that want to
/// render an input of the right shape.
pub fn kind_of(field: FilterField) -> FieldKind {
    field_info(field).kind
}
