//! Filter model: an NSP superset evaluated locally, pushed server-side when
//! expressible, exported as `.nsp`.
//!
//! Entry points for the actor:
//!
//! | Need | Call |
//! |---|---|
//! | Validate a filter from the UI | [`validate_filter`] / [`validate_node`] |
//! | Which ops a field accepts (builder UI) | [`ops_for_field`], [`kind_of`] |
//! | Can it be pushed server-side? | [`capability`] `(&Filter, ServerCaps) -> api::FilterCapability` |
//! | Filter tracks already in memory | [`matches`] / [`matches_with`] |
//! | Run it against the mirror | [`select_for_filter`] / [`select_for_node`] / [`count_for_node`] → `SELECT … FROM tracks` with params; or [`where_clause`] + [`order_by`] + [`limit_clause`] to compose your own |
//! | Static playlist from the rows the SQL returned | [`static_playlist_ids`] |
//! | `.nsp` export / import | [`to_nsp`] / [`from_nsp`] |
//! | Built-in filters | [`default_filters`] |
//!
//! The schema contract is `db/schema.sql`: `tracks` columns are the
//! snake_case `api::Track` fields plus `has_lyrics`, `local_play_count`,
//! `local_last_played`, `is_compilation`, `changed`; playlist membership is
//! `playlist_tracks(server_id, playlist_id, position, track_id)`.
//!
//! Units: `Duration` rules are seconds, `BitRate` kbps, dates `YYYY-MM-DD`,
//! `InTheLast` days. See [`model`] for the full value conventions.

pub mod dates;
pub mod eval;
pub mod model;
pub mod nsp;
pub mod sql;

#[cfg(test)]
mod tests;

pub use eval::{
    field_value, kind_of, matches, matches_with, order_tracks, rule_matches, FieldValue, LocalFacts,
};
pub use model::{
    capability, field_by_nsp_name, field_info, fields_used, is_server_expressible, node_capability,
    ops_for_field, ops_for_kind, validate_filter, validate_node, validate_rule, Expressibility,
    FieldInfo, FieldKind, FilterError, ServerCaps, FIELDS, MAX_DEPTH, MAX_NODES,
};
pub use nsp::{from_nsp, sort_name, to_nsp};
pub use sql::{
    count_for_node, limit_clause, order_by, select_for_filter, select_for_node, where_clause,
    SelectQuery, WhereClause,
};

use crate::api::{
    Filter, FilterField, FilterNode, FilterOp, FilterRule, FilterValue, SortOrder, Track, TrackId,
};

/// Ids of the built-in filters, so the UI can recognise (and not delete) them.
pub const BUILTIN_PREFIX: &str = "builtin:";

/// Evaluates a filter against rows already fetched (each with its local
/// facts), then orders and limits them exactly as the SQL would, and returns
/// the track ids in playlist order. This is the "static playlist from filter"
/// step: the actor runs [`select_for_filter`] (or hands over any rows), calls
/// this, and writes the ids through `createPlaylist`.
///
/// `seed` only matters for `SortOrder::Random`.
pub fn static_playlist_ids<'a>(
    filter: &Filter,
    rows: impl IntoIterator<Item = (&'a Track, &'a LocalFacts)>,
    now_ms: f64,
    seed: u64,
) -> Vec<TrackId> {
    let matching: Vec<&Track> = rows
        .into_iter()
        .filter(|(t, f)| matches_with(&filter.root, t, f, now_ms))
        .map(|(t, _)| t)
        .collect();
    order_tracks(matching, filter.sort, filter.descending, filter.limit, seed)
}

fn rule(field: FilterField, op: FilterOp, value: FilterValue) -> FilterNode {
    FilterNode::Rule(FilterRule { field, op, value })
}

fn builtin(
    id: &str,
    name: &str,
    root: FilterNode,
    sort: SortOrder,
    descending: bool,
    limit: Option<u32>,
) -> Filter {
    Filter {
        id: format!("{BUILTIN_PREFIX}{id}"),
        name: name.to_string(),
        root,
        sort,
        descending,
        limit,
    }
}

/// A useful starting set. Names are ids for the strings table on the
/// platform side (`filter.builtin.<id>`); the `name` here is the English fallback.
pub fn default_filters() -> Vec<Filter> {
    vec![
        builtin(
            "recently-added",
            "Recently added",
            FilterNode::All(vec![rule(
                FilterField::DateAdded,
                FilterOp::InTheLast,
                FilterValue::Days(30),
            )]),
            SortOrder::DateAdded,
            true,
            Some(500),
        ),
        builtin(
            "never-played",
            "Never played",
            FilterNode::All(vec![
                rule(
                    FilterField::PlayCount,
                    FilterOp::Is,
                    FilterValue::Number(0.0),
                ),
                rule(
                    FilterField::LocalPlayCount,
                    FilterOp::Is,
                    FilterValue::Number(0.0),
                ),
            ]),
            SortOrder::Random,
            false,
            None,
        ),
        builtin(
            "top-rated",
            "Top rated",
            FilterNode::All(vec![rule(
                FilterField::Rating,
                FilterOp::Gt,
                FilterValue::Number(3.0),
            )]),
            SortOrder::Rating,
            true,
            None,
        ),
        builtin(
            "loved",
            "Loved",
            FilterNode::All(vec![rule(
                FilterField::Loved,
                FilterOp::IsTrue,
                FilterValue::Bool(true),
            )]),
            SortOrder::DateAdded,
            true,
            None,
        ),
        builtin(
            "downloaded",
            "Downloaded",
            FilterNode::All(vec![rule(
                FilterField::Downloaded,
                FilterOp::IsTrue,
                FilterValue::Bool(true),
            )]),
            SortOrder::Artist,
            false,
            None,
        ),
        builtin(
            "recently-played",
            "Recently played",
            FilterNode::Any(vec![
                rule(
                    FilterField::LastPlayed,
                    FilterOp::InTheLast,
                    FilterValue::Days(14),
                ),
                rule(
                    FilterField::LocalLastPlayed,
                    FilterOp::InTheLast,
                    FilterValue::Days(14),
                ),
            ]),
            SortOrder::PlayCount,
            true,
            Some(200),
        ),
    ]
}

/// True for ids produced by [`default_filters`].
pub fn is_builtin(id: &str) -> bool {
    id.starts_with(BUILTIN_PREFIX)
}
