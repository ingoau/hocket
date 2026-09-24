//! Navidrome Smart Playlist (`.nsp`) export and import.
//!
//! Format (Navidrome `model/criteria`): a JSON object with `all` or `any`
//! (a list of rule objects, each `{ "<op>": { "<field>": value } }` or a
//! nested `all`/`any`), plus optional `name`, `comment`, `sort`, `order`,
//! `limit`. Operators: `is`, `isNot`, `gt`, `lt`, `contains`, `notContains`,
//! `startsWith`, `endsWith`, `inTheRange` (two-element array), `before`,
//! `after` (`YYYY-MM-DD`), `inTheLast`, `notInTheLast` (days), `inPlaylist`,
//! `notInPlaylist` (`{"id": …}`), `isMissing`, `isPresent`. Booleans are bare
//! JSON booleans. `sort` may be a comma list with `-` prefixes; `order`
//! (`asc`/`desc`) reverses all of them.

use serde_json::{json, Map, Value};

use crate::api::{Filter, FilterField, FilterNode, FilterOp, FilterRule, FilterValue, SortOrder};

use super::model::{
    field_by_nsp_name, field_info, is_server_expressible, validate_filter, FieldKind, FilterError,
    ServerCaps,
};
use super::model::{fields_used, Expressibility};

const EXPORT_COMMENT: &str = "Exported from Hocket";

/// Serialises a filter as an NSP document (pretty JSON). Fails when the
/// filter uses fields the server cannot evaluate; sonic fields are allowed
/// when `caps.sonic_attributes` is set.
pub fn to_nsp(filter: &Filter, caps: ServerCaps) -> Result<String, FilterError> {
    validate_filter(filter)?;
    let blocked: Vec<FilterField> = fields_used(&filter.root)
        .into_iter()
        .filter(|f| !is_server_expressible(*f, caps))
        .collect();
    if !blocked.is_empty() {
        return Err(FilterError::NotServerExpressible(blocked));
    }
    let mut doc = Map::new();
    doc.insert("name".into(), Value::String(filter.name.clone()));
    doc.insert("comment".into(), Value::String(EXPORT_COMMENT.into()));
    match &filter.root {
        FilterNode::Any(children) => {
            doc.insert(
                "any".into(),
                Value::Array(children.iter().map(node_to_json).collect()),
            );
        }
        FilterNode::All(children) => {
            doc.insert(
                "all".into(),
                Value::Array(children.iter().map(node_to_json).collect()),
            );
        }
        rule @ FilterNode::Rule(_) => {
            doc.insert("all".into(), Value::Array(vec![node_to_json(rule)]));
        }
    }
    if let Some(sort) = sort_name(filter.sort) {
        doc.insert("sort".into(), Value::String(sort.into()));
    }
    // Random has no direction; the default order still records one so a
    // document round-trips (Navidrome ignores `order` without `sort`).
    if filter.sort != SortOrder::Random && (filter.sort != SortOrder::Default || filter.descending)
    {
        doc.insert(
            "order".into(),
            Value::String(if filter.descending { "desc" } else { "asc" }.into()),
        );
    }
    if let Some(limit) = filter.limit {
        doc.insert("limit".into(), json!(limit));
    }
    serde_json::to_string_pretty(&Value::Object(doc)).map_err(|e| FilterError::Nsp(e.to_string()))
}

/// NSP sort name for a sort order; `None` for the default order.
pub fn sort_name(sort: SortOrder) -> Option<&'static str> {
    Some(match sort {
        SortOrder::Default => return None,
        SortOrder::Title => "title",
        SortOrder::Artist => "artist",
        SortOrder::Album => "album",
        SortOrder::Year => "year",
        SortOrder::DateAdded => "dateadded",
        SortOrder::Rating => "rating",
        SortOrder::PlayCount => "playcount",
        SortOrder::Duration => "duration",
        SortOrder::Random => "random",
        SortOrder::Bpm => "bpm",
        SortOrder::Energy => "energy",
    })
}

fn sort_from_name(name: &str) -> Option<SortOrder> {
    Some(match name.to_ascii_lowercase().as_str() {
        "title" | "sorttitle" => SortOrder::Title,
        "artist" | "sortartist" | "albumartist" | "sortalbumartist" => SortOrder::Artist,
        "album" | "sortalbum" => SortOrder::Album,
        "year" | "date" | "releaseyear" | "originalyear" => SortOrder::Year,
        "dateadded" | "albumdateadded" => SortOrder::DateAdded,
        "rating" => SortOrder::Rating,
        "playcount" => SortOrder::PlayCount,
        "duration" => SortOrder::Duration,
        "random" => SortOrder::Random,
        "bpm" => SortOrder::Bpm,
        "energy" => SortOrder::Energy,
        _ => return None,
    })
}

fn node_to_json(node: &FilterNode) -> Value {
    match node {
        FilterNode::All(children) => {
            json!({ "all": children.iter().map(node_to_json).collect::<Vec<_>>() })
        }
        FilterNode::Any(children) => {
            json!({ "any": children.iter().map(node_to_json).collect::<Vec<_>>() })
        }
        FilterNode::Rule(rule) => rule_to_json(rule),
    }
}

fn num(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9.0e15 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn op_json(op: &str, field: &str, value: Value) -> Value {
    json!({ op: { field: value } })
}

fn rule_to_json(rule: &FilterRule) -> Value {
    let info = field_info(rule.field);
    let name = info.nsp_name;
    match info.kind {
        FieldKind::Bool => {
            let truthy = rule.op == FilterOp::IsTrue;
            if rule.field == FilterField::Lyrics {
                op_json(
                    if truthy { "isPresent" } else { "isMissing" },
                    name,
                    json!(true),
                )
            } else {
                op_json("is", name, json!(truthy))
            }
        }
        FieldKind::Playlist => {
            let op = if rule.op == FilterOp::IsNot {
                "notInPlaylist"
            } else {
                "inPlaylist"
            };
            match &rule.value {
                FilterValue::Text(id) => json!({ op: { "id": id } }),
                FilterValue::List(ids) => {
                    let items: Vec<Value> =
                        ids.iter().map(|id| json!({ op: { "id": id } })).collect();
                    json!({ if rule.op == FilterOp::IsNot { "all" } else { "any" }: items })
                }
                _ => Value::Null,
            }
        }
        FieldKind::Text => match (&rule.op, &rule.value) {
            (FilterOp::Is, FilterValue::List(items)) => {
                json!({ "any": items.iter().map(|i| op_json("is", name, json!(i))).collect::<Vec<_>>() })
            }
            (FilterOp::IsNot, FilterValue::List(items)) => {
                json!({ "all": items.iter().map(|i| op_json("isNot", name, json!(i))).collect::<Vec<_>>() })
            }
            (op, FilterValue::Text(t)) => op_json(text_op_name(*op), name, json!(t)),
            _ => Value::Null,
        },
        FieldKind::Number => match (&rule.op, &rule.value) {
            (FilterOp::InTheRange, FilterValue::Range { low, high }) => {
                op_json("inTheRange", name, json!([num(*low), num(*high)]))
            }
            (op, FilterValue::Number(n)) => op_json(text_op_name(*op), name, num(*n)),
            _ => Value::Null,
        },
        FieldKind::Date => match (&rule.op, &rule.value) {
            (FilterOp::InTheLast, FilterValue::Days(n)) => op_json("inTheLast", name, json!(n)),
            (FilterOp::NotInTheLast, FilterValue::Days(n)) => {
                op_json("notInTheLast", name, json!(n))
            }
            (FilterOp::Before, FilterValue::Date(d)) => op_json("before", name, json!(d)),
            (FilterOp::After, FilterValue::Date(d)) => op_json("after", name, json!(d)),
            (FilterOp::InTheRange, FilterValue::List(items)) => {
                op_json("inTheRange", name, json!(items))
            }
            _ => Value::Null,
        },
    }
}

fn text_op_name(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Is => "is",
        FilterOp::IsNot => "isNot",
        FilterOp::Contains => "contains",
        FilterOp::NotContains => "notContains",
        FilterOp::StartsWith => "startsWith",
        FilterOp::EndsWith => "endsWith",
        FilterOp::Gt => "gt",
        FilterOp::Lt => "lt",
        FilterOp::InTheRange => "inTheRange",
        FilterOp::Before => "before",
        FilterOp::After => "after",
        FilterOp::InTheLast => "inTheLast",
        FilterOp::NotInTheLast => "notInTheLast",
        FilterOp::IsTrue => "is",
        FilterOp::IsFalse => "isNot",
    }
}

/// Parses an NSP document into a filter. `id` is assigned fresh; the name
/// comes from the document's `name`, falling back to `fallback_name`.
/// Rules outside our vocabulary (custom tags, album/artist-level fields,
/// `isMissing` on text fields, playlist references by path) are rejected
/// with [`FilterError::Nsp`] naming the offending key.
pub fn from_nsp(document: &str, fallback_name: &str) -> Result<Filter, FilterError> {
    let value: Value = serde_json::from_str(document)
        .map_err(|e| FilterError::Nsp(format!("invalid JSON: {e}")))?;
    let obj = value
        .as_object()
        .ok_or_else(|| FilterError::Nsp("top level must be an object".into()))?;
    let lower: Map<String, Value> = obj
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();

    let root = match (lower.get("all"), lower.get("any")) {
        (Some(_), Some(_)) => {
            return Err(FilterError::Nsp("both 'all' and 'any' at top level".into()))
        }
        (Some(all), None) => FilterNode::All(parse_children(all)?),
        (None, Some(any)) => FilterNode::Any(parse_children(any)?),
        (None, None) => return Err(FilterError::Nsp("missing 'all' or 'any'".into())),
    };

    let name = lower
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_name)
        .to_string();

    let (mut sort, mut descending) = (SortOrder::Default, false);
    if let Some(s) = lower.get("sort").and_then(Value::as_str) {
        // "-year,-rating,title": the first key is what we can represent.
        if let Some(first) = s.split(',').map(str::trim).find(|p| !p.is_empty()) {
            let (neg, key) = match first.strip_prefix('-') {
                Some(k) => (true, k),
                None => (false, first.strip_prefix('+').unwrap_or(first)),
            };
            if let Some(so) = sort_from_name(key) {
                sort = so;
                descending = neg;
            }
        }
    }
    if let Some(order) = lower.get("order").and_then(Value::as_str) {
        if order.eq_ignore_ascii_case("desc") {
            descending = !descending;
        }
    }
    // Random has no direction (and export writes none), so `-random` or
    // `order: desc` must not leave a flag that silently vanishes on re-export.
    if sort == SortOrder::Random {
        descending = false;
    }
    let limit = lower
        .get("limit")
        .and_then(Value::as_u64)
        .filter(|l| *l > 0)
        .map(|l| l.min(u32::MAX as u64) as u32);

    let filter = Filter {
        id: crate::util::new_id(),
        name,
        root,
        sort,
        descending,
        limit,
    };
    validate_filter(&filter)?;
    Ok(filter)
}

fn parse_children(value: &Value) -> Result<Vec<FilterNode>, FilterError> {
    let items = value
        .as_array()
        .ok_or_else(|| FilterError::Nsp("'all'/'any' must be an array".into()))?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| FilterError::Nsp("rule must be an object".into()))?;
        for (k, v) in obj {
            out.push(parse_expression(&k.to_ascii_lowercase(), v)?);
        }
    }
    Ok(out)
}

fn parse_expression(op: &str, value: &Value) -> Result<FilterNode, FilterError> {
    match op {
        "all" => return Ok(FilterNode::All(parse_children(value)?)),
        "any" => return Ok(FilterNode::Any(parse_children(value)?)),
        "inplaylist" | "notinplaylist" => {
            let id = value.get("id").and_then(Value::as_str).ok_or_else(|| {
                FilterError::Nsp(format!(
                    "'{op}' needs an 'id' (playlist paths are not supported)"
                ))
            })?;
            return Ok(FilterNode::Rule(FilterRule {
                field: FilterField::InPlaylist,
                op: if op == "inplaylist" {
                    FilterOp::Is
                } else {
                    FilterOp::IsNot
                },
                value: FilterValue::Text(id.to_string()),
            }));
        }
        _ => {}
    }
    let obj = value
        .as_object()
        .ok_or_else(|| FilterError::Nsp(format!("'{op}' must map a field to a value")))?;
    let (field_name, raw) = obj
        .iter()
        .next()
        .ok_or_else(|| FilterError::Nsp(format!("'{op}' has no field")))?;
    if obj.len() != 1 {
        return Err(FilterError::Nsp(format!(
            "'{op}' must name exactly one field"
        )));
    }
    let info = field_by_nsp_name(field_name)
        .ok_or_else(|| FilterError::Nsp(format!("unsupported field '{field_name}'")))?;
    if info.expressibility == Expressibility::Local && info.field != FilterField::InPlaylist {
        return Err(FilterError::Nsp(format!(
            "unsupported field '{field_name}'"
        )));
    }
    let field = info.field;
    let unsupported = || FilterError::Nsp(format!("'{op}' is not supported for '{field_name}'"));
    let rule = match info.kind {
        FieldKind::Bool => {
            let truthy = coerce_bool(raw)
                .ok_or_else(|| FilterError::Nsp(format!("'{field_name}' needs a boolean")))?;
            let is_true = match op {
                "is" => truthy,
                "isnot" => !truthy,
                "ispresent" if field == FilterField::Lyrics => truthy,
                "ismissing" if field == FilterField::Lyrics => !truthy,
                _ => return Err(unsupported()),
            };
            FilterRule {
                field,
                op: if is_true {
                    FilterOp::IsTrue
                } else {
                    FilterOp::IsFalse
                },
                value: FilterValue::Bool(is_true),
            }
        }
        FieldKind::Text => {
            let text = raw
                .as_str()
                .map(str::to_string)
                .or_else(|| raw.as_f64().map(|n| num(n).to_string()));
            let text =
                text.ok_or_else(|| FilterError::Nsp(format!("'{field_name}' needs a string")))?;
            let fop = match op {
                "is" => FilterOp::Is,
                "isnot" => FilterOp::IsNot,
                "contains" => FilterOp::Contains,
                "notcontains" => FilterOp::NotContains,
                "startswith" => FilterOp::StartsWith,
                "endswith" => FilterOp::EndsWith,
                _ => return Err(unsupported()),
            };
            FilterRule {
                field,
                op: fop,
                value: FilterValue::Text(text),
            }
        }
        FieldKind::Number => {
            if op == "intherange" {
                let arr = raw.as_array().filter(|a| a.len() == 2).ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' range needs two numbers"))
                })?;
                let low = coerce_number(&arr[0]).ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' range needs two numbers"))
                })?;
                let high = coerce_number(&arr[1]).ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' range needs two numbers"))
                })?;
                FilterRule {
                    field,
                    op: FilterOp::InTheRange,
                    value: FilterValue::Range { low, high },
                }
            } else {
                let n = coerce_number(raw)
                    .ok_or_else(|| FilterError::Nsp(format!("'{field_name}' needs a number")))?;
                let fop = match op {
                    "is" => FilterOp::Is,
                    "isnot" => FilterOp::IsNot,
                    "gt" => FilterOp::Gt,
                    "lt" => FilterOp::Lt,
                    _ => return Err(unsupported()),
                };
                FilterRule {
                    field,
                    op: fop,
                    value: FilterValue::Number(n),
                }
            }
        }
        FieldKind::Date => match op {
            "inthelast" | "notinthelast" => {
                let days = coerce_number(raw)
                    .filter(|d| *d >= 1.0)
                    .ok_or_else(|| FilterError::Nsp(format!("'{field_name}' needs a day count")))?;
                FilterRule {
                    field,
                    op: if op == "inthelast" {
                        FilterOp::InTheLast
                    } else {
                        FilterOp::NotInTheLast
                    },
                    value: FilterValue::Days(days.min(f64::from(u32::MAX)) as u32),
                }
            }
            "before" | "after" => {
                let d = raw.as_str().ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' needs a YYYY-MM-DD date"))
                })?;
                FilterRule {
                    field,
                    op: if op == "before" {
                        FilterOp::Before
                    } else {
                        FilterOp::After
                    },
                    value: FilterValue::Date(d.to_string()),
                }
            }
            "intherange" => {
                let arr = raw.as_array().filter(|a| a.len() == 2).ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' range needs two dates"))
                })?;
                let dates: Option<Vec<String>> =
                    arr.iter().map(|v| v.as_str().map(str::to_string)).collect();
                let dates = dates.ok_or_else(|| {
                    FilterError::Nsp(format!("'{field_name}' range needs two dates"))
                })?;
                FilterRule {
                    field,
                    op: FilterOp::InTheRange,
                    value: FilterValue::List(dates),
                }
            }
            _ => return Err(unsupported()),
        },
        FieldKind::Playlist => return Err(unsupported()),
    };
    Ok(FilterNode::Rule(rule))
}

fn coerce_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "1" | "t" | "yes" => Some(true),
            "false" | "0" | "f" | "no" => Some(false),
            _ => None,
        },
        Value::Number(n) => match n.as_f64() {
            Some(1.0) => Some(true),
            Some(0.0) => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn coerce_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod from_nsp_never_panics {
    use super::*;
    use crate::connect::wire::arbitrary_json::{json, mutated, same_modulo_float_parsing};
    use proptest::prelude::*;

    const WORDS: &[&str] = &[
        "all",
        "any",
        "name",
        "comment",
        "sort",
        "order",
        "limit",
        "asc",
        "desc",
        "is",
        "isNot",
        "gt",
        "lt",
        "contains",
        "notContains",
        "startsWith",
        "endsWith",
        "inTheRange",
        "before",
        "after",
        "inTheLast",
        "notInTheLast",
        "inPlaylist",
        "notInPlaylist",
        "isMissing",
        "isPresent",
        "id",
        "title",
        "artist",
        "album",
        "genre",
        "year",
        "rating",
        "playcount",
        "loved",
        "lyrics",
        "compilation",
        "dateadded",
        "lastplayed",
        "bpm",
        "duration",
        "random",
        "-random",
        "-year,title",
        "2024-02-29",
        "NaN",
        "inf",
        "-1e400",
        "true",
        "1",
    ];

    const DOC: &str = r#"{"name":"Nineties","all":[{"inTheRange":{"year":[1990,1999]}},{"contains":{"genre":"rock"}},{"gt":{"rating":3}},{"inTheLast":{"lastPlayed":30}},{"is":{"loved":true}},{"any":[{"startsWith":{"title":"A"}},{"before":{"dateAdded":"2020-01-01"}}]}],"sort":"-year,title","order":"desc","limit":50}"#;

    /// Never panics; an imported filter is valid and (when the server can
    /// evaluate it) exports and re-imports to the same filter.
    fn check(text: &str) {
        let Ok(f) = from_nsp(text, "fallback") else {
            return;
        };
        validate_filter(&f).unwrap();
        let caps = ServerCaps {
            sonic_attributes: true,
            native_api: true,
        };
        match to_nsp(&f, caps) {
            Ok(out) => {
                let mut back = from_nsp(&out, "other").unwrap();
                back.id = f.id.clone();
                assert!(same_modulo_float_parsing(&back, &f), "{out}");
            }
            Err(FilterError::NotServerExpressible(_)) => {}
            Err(e) => panic!("imported filter does not export: {e:?}"),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

        #[test]
        fn arbitrary_text(s in "\\PC{0,128}") {
            check(&s);
        }

        #[test]
        fn arbitrary_documents(v in json(WORDS)) {
            check(&v.to_string());
        }

        #[test]
        fn arbitrary_rules(
            top in prop::sample::select(&["all", "any"][..]),
            op in prop::sample::select(WORDS),
            field in prop::sample::select(WORDS),
            value in json(WORDS),
            sort in prop::sample::select(WORDS),
            order in prop::sample::select(WORDS),
        ) {
            let doc = serde_json::json!({ top: [{ op: { field: value } }], "sort": sort, "order": order });
            check(&doc.to_string());
        }

        #[test]
        fn mutated_documents(s in mutated(DOC)) {
            check(&s);
        }
    }

    #[test]
    fn fuzz_regression_random_sort_has_no_direction() {
        for doc in [
            r#"{"all":[{"is":{"loved":true}}],"sort":"-random"}"#,
            r#"{"all":[{"is":{"loved":true}}],"sort":"random","order":"desc"}"#,
        ] {
            let f = from_nsp(doc, "x").unwrap();
            assert_eq!(f.sort, SortOrder::Random);
            assert!(!f.descending, "a direction export would drop");
            check(doc);
        }
    }
}
