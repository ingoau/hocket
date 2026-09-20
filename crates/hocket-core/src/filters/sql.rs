//! SQL generation against the mirror's `tracks` table.
//!
//! Output is a [`WhereClause`] (parameterised, injection-safe: user text only
//! ever travels as a bound parameter, never spliced into SQL) plus `ORDER BY`
//! and `LIMIT` fragments. The actor owns the connection and runs the query.
//!
//! Column names are the schema contract in `db/schema.sql` (snake_case
//! `api::Track` fields, flattened `rg_*` / sonic columns, `has_lyrics`,
//! `local_play_count`, `local_last_played`, `is_compilation`, `changed`) and
//! `playlist_tracks(server_id, playlist_id, position, track_id)`.

use rusqlite::types::Value;

use crate::api::{Filter, FilterNode, FilterOp, FilterRule, FilterValue, SortOrder};

use super::dates::{CivilDate, MS_PER_DAY};
use super::model::{field_info, validate_node, FieldKind, FilterError};

/// A parameterised SQL predicate. `sql` contains `?` placeholders in the
/// order of `params`. Safe to embed as `WHERE {sql}` or `AND ({sql})`.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    pub sql: String,
    pub params: Vec<Value>,
}

impl WhereClause {
    /// A predicate that matches everything.
    pub fn always() -> WhereClause {
        WhereClause {
            sql: "1=1".into(),
            params: vec![],
        }
    }
}

/// Generates the predicate for a node tree. Validates first; malformed
/// trees are refused rather than silently matching nothing.
pub fn where_clause(node: &FilterNode, now_ms: f64) -> Result<WhereClause, FilterError> {
    validate_node(node)?;
    let mut params = Vec::new();
    let sql = node_sql(node, now_ms, &mut params);
    Ok(WhereClause { sql, params })
}

fn node_sql(node: &FilterNode, now_ms: f64, params: &mut Vec<Value>) -> String {
    match node {
        FilterNode::Rule(rule) => rule_sql(rule, now_ms, params),
        FilterNode::All(children) => {
            if children.is_empty() {
                "1=1".into()
            } else {
                let parts: Vec<String> = children
                    .iter()
                    .map(|c| node_sql(c, now_ms, params))
                    .collect();
                format!("({})", parts.join(" AND "))
            }
        }
        FilterNode::Any(children) => {
            if children.is_empty() {
                "1=0".into()
            } else {
                let parts: Vec<String> = children
                    .iter()
                    .map(|c| node_sql(c, now_ms, params))
                    .collect();
                format!("({})", parts.join(" OR "))
            }
        }
    }
}

/// Escapes `%`, `_` and `\` for use with `LIKE ? ESCAPE '\'`.
fn like_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn rule_sql(rule: &FilterRule, now_ms: f64, params: &mut Vec<Value>) -> String {
    let info = field_info(rule.field);
    let col = info.column;
    match info.kind {
        FieldKind::Text => text_sql(col, rule, params),
        FieldKind::Number => number_sql(col, rule, params),
        FieldKind::Date => date_sql(col, rule, now_ms, params),
        FieldKind::Bool => match rule.op {
            FilterOp::IsTrue => format!("COALESCE({col}, 0) = 1"),
            _ => format!("COALESCE({col}, 0) = 0"),
        },
        FieldKind::Playlist => playlist_sql(rule, params),
    }
}

fn text_sql(col: &str, rule: &FilterRule, params: &mut Vec<Value>) -> String {
    let push_text = |params: &mut Vec<Value>, t: &str| params.push(Value::Text(t.to_string()));
    match (&rule.op, &rule.value) {
        (FilterOp::Is, FilterValue::Text(t)) => {
            push_text(params, t);
            format!("{col} = ? COLLATE NOCASE")
        }
        (FilterOp::IsNot, FilterValue::Text(t)) => {
            push_text(params, t);
            format!("{col} <> ? COLLATE NOCASE")
        }
        (FilterOp::Is, FilterValue::List(items)) => {
            let marks = vec!["?"; items.len()].join(", ");
            items.iter().for_each(|i| push_text(params, i));
            format!("{col} COLLATE NOCASE IN ({marks})")
        }
        (FilterOp::IsNot, FilterValue::List(items)) => {
            let marks = vec!["?"; items.len()].join(", ");
            items.iter().for_each(|i| push_text(params, i));
            format!("{col} COLLATE NOCASE NOT IN ({marks})")
        }
        (FilterOp::Contains, FilterValue::Text(t)) => {
            push_text(params, &format!("%{}%", like_escape(t)));
            format!("{col} LIKE ? ESCAPE '\\'")
        }
        (FilterOp::NotContains, FilterValue::Text(t)) => {
            push_text(params, &format!("%{}%", like_escape(t)));
            format!("{col} NOT LIKE ? ESCAPE '\\'")
        }
        (FilterOp::StartsWith, FilterValue::Text(t)) => {
            push_text(params, &format!("{}%", like_escape(t)));
            format!("{col} LIKE ? ESCAPE '\\'")
        }
        (FilterOp::EndsWith, FilterValue::Text(t)) => {
            push_text(params, &format!("%{}", like_escape(t)));
            format!("{col} LIKE ? ESCAPE '\\'")
        }
        // Unreachable after validation; never match rather than match all.
        _ => "1=0".into(),
    }
}

fn number_sql(col: &str, rule: &FilterRule, params: &mut Vec<Value>) -> String {
    match (&rule.op, &rule.value) {
        (FilterOp::Is, FilterValue::Number(n)) => {
            params.push(Value::Real(*n));
            format!("{col} = ?")
        }
        (FilterOp::IsNot, FilterValue::Number(n)) => {
            params.push(Value::Real(*n));
            format!("{col} <> ?")
        }
        (FilterOp::Gt, FilterValue::Number(n)) => {
            params.push(Value::Real(*n));
            format!("{col} > ?")
        }
        (FilterOp::Lt, FilterValue::Number(n)) => {
            params.push(Value::Real(*n));
            format!("{col} < ?")
        }
        (FilterOp::InTheRange, FilterValue::Range { low, high }) => {
            params.push(Value::Real(*low));
            params.push(Value::Real(*high));
            format!("{col} BETWEEN ? AND ?")
        }
        _ => "1=0".into(),
    }
}

fn date_sql(col: &str, rule: &FilterRule, now_ms: f64, params: &mut Vec<Value>) -> String {
    match (&rule.op, &rule.value) {
        (FilterOp::InTheLast, FilterValue::Days(n)) => {
            params.push(Value::Real(now_ms - f64::from(*n) * MS_PER_DAY));
            format!("{col} >= ?")
        }
        (FilterOp::NotInTheLast, FilterValue::Days(n)) => {
            params.push(Value::Real(now_ms - f64::from(*n) * MS_PER_DAY));
            format!("({col} IS NULL OR {col} < ?)")
        }
        (FilterOp::Before, FilterValue::Date(d)) => match CivilDate::parse(d) {
            Some(d) => {
                params.push(Value::Real(d.start_ms()));
                format!("{col} < ?")
            }
            None => "1=0".into(),
        },
        (FilterOp::After, FilterValue::Date(d)) => match CivilDate::parse(d) {
            Some(d) => {
                params.push(Value::Real(d.end_ms_exclusive()));
                format!("{col} >= ?")
            }
            None => "1=0".into(),
        },
        (FilterOp::InTheRange, FilterValue::List(items)) if items.len() == 2 => {
            match (CivilDate::parse(&items[0]), CivilDate::parse(&items[1])) {
                (Some(from), Some(to)) => {
                    params.push(Value::Real(from.start_ms()));
                    params.push(Value::Real(to.end_ms_exclusive()));
                    format!("({col} >= ? AND {col} < ?)")
                }
                _ => "1=0".into(),
            }
        }
        _ => "1=0".into(),
    }
}

fn playlist_sql(rule: &FilterRule, params: &mut Vec<Value>) -> String {
    let ids: Vec<&str> = match &rule.value {
        FilterValue::Text(id) => vec![id.as_str()],
        FilterValue::List(items) => items.iter().map(String::as_str).collect(),
        _ => return "1=0".into(),
    };
    let marks = vec!["?"; ids.len()].join(", ");
    ids.iter()
        .for_each(|i| params.push(Value::Text((*i).to_string())));
    let exists = format!(
        "EXISTS (SELECT 1 FROM playlist_tracks pt WHERE pt.server_id = tracks.server_id \
         AND pt.track_id = tracks.id AND pt.playlist_id IN ({marks}))"
    );
    match rule.op {
        FilterOp::IsNot => format!("NOT {exists}"),
        _ => exists,
    }
}

/// `ORDER BY …` for a sort order. Always ends with a deterministic
/// tie-break (`title`, `id`) so paging is stable. `descending` flips the
/// primary key only; tie-breakers stay ascending.
pub fn order_by(sort: SortOrder, descending: bool) -> String {
    let dir = if descending { "DESC" } else { "ASC" };
    let primary: Vec<String> = match sort {
        SortOrder::Default | SortOrder::Title => vec![format!("tracks.title COLLATE NOCASE {dir}")],
        SortOrder::Artist => vec![
            format!("tracks.artist COLLATE NOCASE {dir}"),
            "tracks.album COLLATE NOCASE ASC".into(),
            "tracks.disc_number ASC".into(),
            "tracks.track_number ASC".into(),
        ],
        SortOrder::Album => vec![
            format!("tracks.album COLLATE NOCASE {dir}"),
            "tracks.disc_number ASC".into(),
            "tracks.track_number ASC".into(),
        ],
        SortOrder::Year => vec![format!("tracks.year {dir}")],
        SortOrder::DateAdded => vec![format!("tracks.created {dir}")],
        SortOrder::Rating => vec![format!("tracks.rating {dir}")],
        SortOrder::PlayCount => vec![format!("tracks.play_count {dir}")],
        SortOrder::Duration => vec![format!("tracks.duration_ms {dir}")],
        SortOrder::Bpm => vec![format!("tracks.bpm {dir}")],
        SortOrder::Energy => vec![format!("tracks.energy {dir}")],
        SortOrder::Random => return "ORDER BY RANDOM()".into(),
    };
    let mut keys = primary;
    if !matches!(sort, SortOrder::Default | SortOrder::Title) {
        keys.push("tracks.title COLLATE NOCASE ASC".into());
    }
    keys.push("tracks.id ASC".into());
    format!("ORDER BY {}", keys.join(", "))
}

/// `LIMIT n` (or `LIMIT n OFFSET m`), or empty when unbounded.
pub fn limit_clause(limit: Option<u32>, offset: Option<u32>) -> String {
    match (limit, offset) {
        (Some(l), Some(o)) if o > 0 => format!("LIMIT {l} OFFSET {o}"),
        (Some(l), _) => format!("LIMIT {l}"),
        (None, Some(o)) if o > 0 => format!("LIMIT -1 OFFSET {o}"),
        (None, _) => String::new(),
    }
}

/// A complete `SELECT` ready to run: `SELECT {columns} FROM tracks WHERE
/// server_id = ? AND ({filter}) {order} {limit}`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectQuery {
    pub sql: String,
    pub params: Vec<Value>,
}

/// Builds the full query for a saved filter scoped to one server. `columns`
/// is a trusted, caller-supplied projection such as `"tracks.id"` or
/// `"tracks.*"` — never user input.
pub fn select_for_filter(
    filter: &Filter,
    server_id: &str,
    columns: &str,
    now_ms: f64,
) -> Result<SelectQuery, FilterError> {
    select_for_node(
        Some(&filter.root),
        filter.sort,
        filter.descending,
        filter.limit,
        None,
        server_id,
        columns,
        now_ms,
    )
}

/// Builds a query for an optional node tree (a bare list query when `None`).
#[allow(clippy::too_many_arguments)]
pub fn select_for_node(
    node: Option<&FilterNode>,
    sort: SortOrder,
    descending: bool,
    limit: Option<u32>,
    offset: Option<u32>,
    server_id: &str,
    columns: &str,
    now_ms: f64,
) -> Result<SelectQuery, FilterError> {
    let clause = match node {
        Some(n) => where_clause(n, now_ms)?,
        None => WhereClause::always(),
    };
    let mut params = vec![Value::Text(server_id.to_string())];
    params.extend(clause.params);
    let mut sql = format!(
        "SELECT {columns} FROM tracks WHERE tracks.server_id = ? AND ({}) {}",
        clause.sql,
        order_by(sort, descending)
    );
    let lim = limit_clause(limit, offset);
    if !lim.is_empty() {
        sql.push(' ');
        sql.push_str(&lim);
    }
    Ok(SelectQuery { sql, params })
}

/// `SELECT COUNT(*)` for the same predicate (previews).
pub fn count_for_node(
    node: Option<&FilterNode>,
    server_id: &str,
    now_ms: f64,
) -> Result<SelectQuery, FilterError> {
    let clause = match node {
        Some(n) => where_clause(n, now_ms)?,
        None => WhereClause::always(),
    };
    let mut params = vec![Value::Text(server_id.to_string())];
    params.extend(clause.params);
    Ok(SelectQuery {
        sql: format!(
            "SELECT COUNT(*) FROM tracks WHERE tracks.server_id = ? AND ({})",
            clause.sql
        ),
        params,
    })
}
