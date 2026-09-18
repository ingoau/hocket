//! Field metadata, rule validation and server-expressibility.
//!
//! Every [`FilterField`] has a *kind* (text, number, date, boolean, playlist)
//! which decides the operators and value shapes it accepts, and an
//! *expressibility* tag: `Server` (always representable as an NSP rule),
//! `Sonic` (representable only when the server exposes sonic attributes) or
//! `Local` (only the local mirror knows the answer).
//!
//! Value conventions (also the NSP conventions):
//! - `Duration` is in **seconds**; `BitRate` in kbps; `Rating` 0–5.
//! - Date fields take `FilterValue::Date("YYYY-MM-DD")` for `Before`/`After`,
//!   `FilterValue::Days(n)` for `InTheLast`/`NotInTheLast`, and
//!   `FilterValue::List([from, to])` (two `YYYY-MM-DD` strings, inclusive) for `InTheRange`.
//! - Boolean fields use `IsTrue`/`IsFalse`; the value is ignored (NSP `is: {loved: true}`).
//! - Text fields accept `Text` for every operator and `List` for `Is`/`IsNot`
//!   ("is any of" / "is none of").
//! - `InPlaylist` takes `Is`/`IsNot` with a playlist id (`Text`) or ids (`List`).
//! - `Lyrics` is a boolean locally ("has lyrics"): the mirror only knows presence.

use crate::api::{Filter, FilterCapability, FilterField, FilterNode, FilterOp, FilterRule, FilterValue};

use super::dates::CivilDate;

/// The value shape a field works with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Date,
    Bool,
    /// Playlist membership; value is a playlist id.
    Playlist,
}

/// Whether a field can be pushed to Navidrome as an NSP rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expressibility {
    /// Always representable server-side.
    Server,
    /// Representable only when the server exposes sonic attributes.
    Sonic,
    /// Only the local mirror can answer.
    Local,
}

/// What the server can do, as far as filters care. Derived from
/// `api::ServerCapabilities` by the actor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerCaps {
    /// The server exposes sonic attributes (bpm/key/energy/mood) as queryable tags.
    pub sonic_attributes: bool,
    /// The Navidrome native API is reachable (smart playlists can be created remotely).
    pub native_api: bool,
}

/// Static metadata for one field.
#[derive(Debug, Clone, Copy)]
pub struct FieldInfo {
    pub field: FilterField,
    pub kind: FieldKind,
    pub expressibility: Expressibility,
    /// NSP field name (lower-case, as Navidrome's criteria package expects).
    pub nsp_name: &'static str,
    /// Mirror column or SQL expression yielding the comparable value.
    pub(crate) column: &'static str,
}

/// Every field, in a stable order (the builder's picker order).
pub const FIELDS: &[FieldInfo] = &[
    f(FilterField::Title, FieldKind::Text, Expressibility::Server, "title", "tracks.title"),
    f(FilterField::Album, FieldKind::Text, Expressibility::Server, "album", "tracks.album"),
    f(FilterField::Artist, FieldKind::Text, Expressibility::Server, "artist", "tracks.artist"),
    f(FilterField::AlbumArtist, FieldKind::Text, Expressibility::Server, "albumartist", "tracks.album_artist"),
    f(FilterField::Genre, FieldKind::Text, Expressibility::Server, "genre", "tracks.genre"),
    f(FilterField::Year, FieldKind::Number, Expressibility::Server, "year", "tracks.year"),
    f(FilterField::DateAdded, FieldKind::Date, Expressibility::Server, "dateadded", "tracks.created"),
    f(FilterField::DateModified, FieldKind::Date, Expressibility::Server, "datemodified", "tracks.changed"),
    f(FilterField::LastPlayed, FieldKind::Date, Expressibility::Server, "lastplayed", "tracks.last_played"),
    f(FilterField::PlayCount, FieldKind::Number, Expressibility::Server, "playcount", "tracks.play_count"),
    f(FilterField::Rating, FieldKind::Number, Expressibility::Server, "rating", "tracks.rating"),
    f(FilterField::Loved, FieldKind::Bool, Expressibility::Server, "loved", "tracks.loved"),
    f(FilterField::Duration, FieldKind::Number, Expressibility::Server, "duration", "(tracks.duration_ms / 1000.0)"),
    f(FilterField::BitRate, FieldKind::Number, Expressibility::Server, "bitrate", "tracks.bit_rate"),
    f(FilterField::FilePath, FieldKind::Text, Expressibility::Server, "filepath", "tracks.path"),
    f(FilterField::FileType, FieldKind::Text, Expressibility::Server, "filetype", "tracks.suffix"),
    f(FilterField::Comment, FieldKind::Text, Expressibility::Server, "comment", "tracks.comment"),
    f(FilterField::Lyrics, FieldKind::Bool, Expressibility::Server, "lyrics", "tracks.has_lyrics"),
    f(FilterField::HasCoverArt, FieldKind::Bool, Expressibility::Server, "hascoverart", "(tracks.cover_art IS NOT NULL)"),
    f(FilterField::Compilation, FieldKind::Bool, Expressibility::Server, "compilation", "tracks.is_compilation"),
    f(FilterField::DiscNumber, FieldKind::Number, Expressibility::Server, "discnumber", "tracks.disc_number"),
    f(FilterField::TrackNumber, FieldKind::Number, Expressibility::Server, "tracknumber", "tracks.track_number"),
    f(FilterField::Bpm, FieldKind::Number, Expressibility::Sonic, "bpm", "tracks.bpm"),
    f(FilterField::Key, FieldKind::Text, Expressibility::Sonic, "key", "tracks.key"),
    f(FilterField::Energy, FieldKind::Number, Expressibility::Sonic, "energy", "tracks.energy"),
    f(FilterField::Mood, FieldKind::Text, Expressibility::Sonic, "mood", "tracks.mood"),
    f(FilterField::Downloaded, FieldKind::Bool, Expressibility::Local, "downloaded", "(tracks.offline = 2)"),
    f(FilterField::Cached, FieldKind::Bool, Expressibility::Local, "cached", "(tracks.offline = 1)"),
    f(FilterField::LocalPlayCount, FieldKind::Number, Expressibility::Local, "localplaycount", "tracks.local_play_count"),
    f(FilterField::LocalLastPlayed, FieldKind::Date, Expressibility::Local, "locallastplayed", "tracks.local_last_played"),
    f(FilterField::InPlaylist, FieldKind::Playlist, Expressibility::Local, "inplaylist", ""),
];

const fn f(
    field: FilterField,
    kind: FieldKind,
    expressibility: Expressibility,
    nsp_name: &'static str,
    column: &'static str,
) -> FieldInfo {
    FieldInfo { field, kind, expressibility, nsp_name, column }
}

/// Metadata for a field. Every variant is in [`FIELDS`].
pub fn field_info(field: FilterField) -> &'static FieldInfo {
    FIELDS.iter().find(|i| i.field == field).expect("every FilterField has metadata")
}

/// Looks a field up by its NSP name (case-insensitive).
pub fn field_by_nsp_name(name: &str) -> Option<&'static FieldInfo> {
    let lower = name.to_ascii_lowercase();
    FIELDS.iter().find(|i| i.nsp_name == lower)
}

/// Operators valid for a field kind, in builder order.
pub fn ops_for_kind(kind: FieldKind) -> &'static [FilterOp] {
    use FilterOp::*;
    match kind {
        FieldKind::Text => &[Is, IsNot, Contains, NotContains, StartsWith, EndsWith],
        FieldKind::Number => &[Is, IsNot, Gt, Lt, InTheRange],
        FieldKind::Date => &[InTheLast, NotInTheLast, Before, After, InTheRange],
        FieldKind::Bool => &[IsTrue, IsFalse],
        FieldKind::Playlist => &[Is, IsNot],
    }
}

/// Operators valid for a field.
pub fn ops_for_field(field: FilterField) -> &'static [FilterOp] {
    ops_for_kind(field_info(field).kind)
}

/// Whether a field is expressible server-side given the server's capabilities.
pub fn is_server_expressible(field: FilterField, caps: ServerCaps) -> bool {
    match field_info(field).expressibility {
        Expressibility::Server => true,
        Expressibility::Sonic => caps.sonic_attributes,
        Expressibility::Local => false,
    }
}

/// Maximum nesting depth and node count accepted by validation; keeps the
/// generated SQL bounded.
pub const MAX_DEPTH: usize = 8;
pub const MAX_NODES: usize = 256;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FilterError {
    #[error("operator {op:?} is not valid for field {field:?}")]
    InvalidOp { field: FilterField, op: FilterOp },
    #[error("value {value} is not valid for {field:?} {op:?}: {reason}")]
    InvalidValue { field: FilterField, op: FilterOp, value: String, reason: String },
    #[error("filter nests deeper than {MAX_DEPTH} levels")]
    TooDeep,
    #[error("filter has more than {MAX_NODES} rules")]
    TooLarge,
    #[error("filter name is empty")]
    EmptyName,
    #[error("filter uses fields that are not server-expressible: {0:?}")]
    NotServerExpressible(Vec<FilterField>),
    #[error("not a valid NSP document: {0}")]
    Nsp(String),
}

/// Validates a whole filter: name, structure, every rule.
pub fn validate_filter(filter: &Filter) -> Result<(), FilterError> {
    if filter.name.trim().is_empty() {
        return Err(FilterError::EmptyName);
    }
    validate_node(&filter.root)
}

/// Validates a node tree: bounded depth/size and every rule well-formed.
pub fn validate_node(node: &FilterNode) -> Result<(), FilterError> {
    let mut count = 0usize;
    walk(node, 0, &mut count)
}

fn walk(node: &FilterNode, depth: usize, count: &mut usize) -> Result<(), FilterError> {
    if depth > MAX_DEPTH {
        return Err(FilterError::TooDeep);
    }
    *count += 1;
    if *count > MAX_NODES {
        return Err(FilterError::TooLarge);
    }
    match node {
        FilterNode::Rule(rule) => validate_rule(rule),
        FilterNode::All(children) | FilterNode::Any(children) => {
            for c in children {
                walk(c, depth + 1, count)?;
            }
            Ok(())
        }
    }
}

/// Validates one rule: operator allowed for the field, value shape and range.
pub fn validate_rule(rule: &FilterRule) -> Result<(), FilterError> {
    let info = field_info(rule.field);
    if !ops_for_kind(info.kind).contains(&rule.op) {
        return Err(FilterError::InvalidOp { field: rule.field, op: rule.op });
    }
    let bad = |reason: &str| FilterError::InvalidValue {
        field: rule.field,
        op: rule.op,
        value: format!("{:?}", rule.value),
        reason: reason.to_string(),
    };
    match info.kind {
        FieldKind::Text | FieldKind::Playlist => match (&rule.op, &rule.value) {
            (_, FilterValue::Text(t)) => {
                if t.is_empty() {
                    Err(bad("text must not be empty"))
                } else {
                    Ok(())
                }
            }
            (FilterOp::Is | FilterOp::IsNot, FilterValue::List(items)) => {
                if items.is_empty() || items.iter().any(|s| s.is_empty()) {
                    Err(bad("list must contain at least one non-empty entry"))
                } else {
                    Ok(())
                }
            }
            _ => Err(bad("expected a text value")),
        },
        FieldKind::Number => match (&rule.op, &rule.value) {
            (FilterOp::InTheRange, FilterValue::Range { low, high }) => {
                if !low.is_finite() || !high.is_finite() {
                    Err(bad("range bounds must be finite"))
                } else if low > high {
                    Err(bad("range low must not exceed high"))
                } else {
                    check_number_bounds(rule.field, *low).and_then(|_| check_number_bounds(rule.field, *high)).map_err(bad)
                }
            }
            (FilterOp::InTheRange, _) => Err(bad("expected a range")),
            (_, FilterValue::Number(n)) => {
                if !n.is_finite() {
                    Err(bad("number must be finite"))
                } else {
                    check_number_bounds(rule.field, *n).map_err(bad)
                }
            }
            _ => Err(bad("expected a number")),
        },
        FieldKind::Date => match (&rule.op, &rule.value) {
            (FilterOp::Before | FilterOp::After, FilterValue::Date(d)) => {
                CivilDate::parse(d).map(|_| ()).ok_or_else(|| bad("expected YYYY-MM-DD"))
            }
            (FilterOp::InTheLast | FilterOp::NotInTheLast, FilterValue::Days(n)) => {
                if *n == 0 {
                    Err(bad("days must be at least 1"))
                } else {
                    Ok(())
                }
            }
            (FilterOp::InTheRange, FilterValue::List(items)) => {
                if items.len() != 2 {
                    return Err(bad("date range needs exactly two dates"));
                }
                let from = CivilDate::parse(&items[0]).ok_or_else(|| bad("expected YYYY-MM-DD"))?;
                let to = CivilDate::parse(&items[1]).ok_or_else(|| bad("expected YYYY-MM-DD"))?;
                if from > to {
                    Err(bad("range start must not be after its end"))
                } else {
                    Ok(())
                }
            }
            _ => Err(bad("date operators take a date, a day count or a two-date list")),
        },
        FieldKind::Bool => Ok(()),
    }
}

fn check_number_bounds(field: FilterField, n: f64) -> Result<(), &'static str> {
    match field {
        FilterField::Rating if !(0.0..=5.0).contains(&n) => Err("rating is 0–5"),
        FilterField::Energy if !(0.0..=1.0).contains(&n) => Err("energy is 0–1"),
        FilterField::Year if !(0.0..=9999.0).contains(&n) => Err("year is 0–9999"),
        FilterField::PlayCount
        | FilterField::LocalPlayCount
        | FilterField::Duration
        | FilterField::BitRate
        | FilterField::DiscNumber
        | FilterField::TrackNumber
        | FilterField::Bpm
            if n < 0.0 =>
        {
            Err("must not be negative")
        }
        _ => Ok(()),
    }
}

/// Every distinct field referenced by a node tree, in first-seen order.
pub fn fields_used(node: &FilterNode) -> Vec<FilterField> {
    let mut out = Vec::new();
    collect_fields(node, &mut out);
    out
}

fn collect_fields(node: &FilterNode, out: &mut Vec<FilterField>) {
    match node {
        FilterNode::Rule(r) => {
            if !out.contains(&r.field) {
                out.push(r.field);
            }
        }
        FilterNode::All(c) | FilterNode::Any(c) => c.iter().for_each(|n| collect_fields(n, out)),
    }
}

/// Server-expressibility of a whole filter: which fields block pushing it
/// server-side. The builder shows this live.
pub fn capability(filter: &Filter, caps: ServerCaps) -> FilterCapability {
    node_capability(&filter.root, caps)
}

/// Same as [`capability`] for a bare node tree.
pub fn node_capability(node: &FilterNode, caps: ServerCaps) -> FilterCapability {
    let local_only_fields: Vec<FilterField> =
        fields_used(node).into_iter().filter(|f| !is_server_expressible(*f, caps)).collect();
    FilterCapability { server_expressible: local_only_fields.is_empty(), local_only_fields }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(field: FilterField, op: FilterOp, value: FilterValue) -> FilterRule {
        FilterRule { field, op, value }
    }

    #[test]
    fn every_field_has_metadata_and_ops() {
        for info in FIELDS {
            assert_eq!(field_info(info.field).field, info.field);
            assert!(!ops_for_field(info.field).is_empty());
            assert_eq!(field_by_nsp_name(info.nsp_name).map(|i| i.field), Some(info.field));
        }
        assert_eq!(field_by_nsp_name("AlbumArtist").map(|i| i.field), Some(FilterField::AlbumArtist));
        assert!(field_by_nsp_name("nope").is_none());
    }

    #[test]
    fn op_validity() {
        assert!(validate_rule(&rule(FilterField::Title, FilterOp::Contains, FilterValue::Text("x".into()))).is_ok());
        assert_eq!(
            validate_rule(&rule(FilterField::Title, FilterOp::Gt, FilterValue::Number(1.0))),
            Err(FilterError::InvalidOp { field: FilterField::Title, op: FilterOp::Gt })
        );
        assert!(validate_rule(&rule(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true))).is_ok());
        assert!(validate_rule(&rule(FilterField::Loved, FilterOp::Is, FilterValue::Bool(true))).is_err());
        assert!(validate_rule(&rule(FilterField::Year, FilterOp::InTheRange, FilterValue::Range { low: 1980.0, high: 1989.0 })).is_ok());
        assert!(validate_rule(&rule(FilterField::Year, FilterOp::InTheRange, FilterValue::Range { low: 1990.0, high: 1989.0 })).is_err());
        assert!(validate_rule(&rule(FilterField::Rating, FilterOp::Gt, FilterValue::Number(6.0))).is_err());
        assert!(validate_rule(&rule(FilterField::Rating, FilterOp::Gt, FilterValue::Number(3.0))).is_ok());
        assert!(validate_rule(&rule(FilterField::Energy, FilterOp::Lt, FilterValue::Number(1.5))).is_err());
        assert!(validate_rule(&rule(FilterField::PlayCount, FilterOp::Gt, FilterValue::Number(-1.0))).is_err());
        assert!(validate_rule(&rule(FilterField::Title, FilterOp::Is, FilterValue::Text(String::new()))).is_err());
        assert!(validate_rule(&rule(FilterField::Title, FilterOp::Is, FilterValue::List(vec!["a".into(), "b".into()]))).is_ok());
        assert!(validate_rule(&rule(FilterField::Title, FilterOp::Contains, FilterValue::List(vec!["a".into()]))).is_err());
    }

    #[test]
    fn date_validity() {
        assert!(validate_rule(&rule(FilterField::LastPlayed, FilterOp::InTheLast, FilterValue::Days(30))).is_ok());
        assert!(validate_rule(&rule(FilterField::LastPlayed, FilterOp::InTheLast, FilterValue::Days(0))).is_err());
        assert!(validate_rule(&rule(FilterField::LastPlayed, FilterOp::InTheLast, FilterValue::Number(30.0))).is_err());
        assert!(validate_rule(&rule(FilterField::DateAdded, FilterOp::Before, FilterValue::Date("2024-01-01".into()))).is_ok());
        assert!(validate_rule(&rule(FilterField::DateAdded, FilterOp::Before, FilterValue::Date("2024-1-1x".into()))).is_err());
        assert!(validate_rule(&rule(
            FilterField::DateAdded,
            FilterOp::InTheRange,
            FilterValue::List(vec!["2024-01-01".into(), "2024-02-01".into()])
        ))
        .is_ok());
        assert!(validate_rule(&rule(
            FilterField::DateAdded,
            FilterOp::InTheRange,
            FilterValue::List(vec!["2024-03-01".into(), "2024-02-01".into()])
        ))
        .is_err());
        assert!(validate_rule(&rule(FilterField::DateAdded, FilterOp::Gt, FilterValue::Number(1.0))).is_err());
    }

    #[test]
    fn depth_and_size_limits() {
        let mut node = FilterNode::Rule(rule(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true)));
        for _ in 0..MAX_DEPTH {
            node = FilterNode::All(vec![node]);
        }
        assert!(validate_node(&node).is_ok());
        node = FilterNode::All(vec![node]);
        assert_eq!(validate_node(&node), Err(FilterError::TooDeep));

        let many: Vec<FilterNode> = (0..MAX_NODES)
            .map(|_| FilterNode::Rule(rule(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true))))
            .collect();
        assert_eq!(validate_node(&FilterNode::All(many)), Err(FilterError::TooLarge));
    }

    #[test]
    fn capability_reflects_caps() {
        let root = FilterNode::All(vec![
            FilterNode::Rule(rule(FilterField::Loved, FilterOp::IsTrue, FilterValue::Bool(true))),
            FilterNode::Rule(rule(FilterField::Bpm, FilterOp::Gt, FilterValue::Number(120.0))),
            FilterNode::Any(vec![FilterNode::Rule(rule(FilterField::Downloaded, FilterOp::IsTrue, FilterValue::Bool(true)))]),
        ]);
        let filter = Filter { id: "f".into(), name: "n".into(), root, sort: Default::default(), descending: false, limit: None };
        let c = capability(&filter, ServerCaps::default());
        assert!(!c.server_expressible);
        assert_eq!(c.local_only_fields, vec![FilterField::Bpm, FilterField::Downloaded]);
        let c = capability(&filter, ServerCaps { sonic_attributes: true, native_api: true });
        assert_eq!(c.local_only_fields, vec![FilterField::Downloaded]);
    }
}
