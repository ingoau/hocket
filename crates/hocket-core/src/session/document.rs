//! Helpers over [`api::SessionDocument`]: construction, load/save that
//! round-trips unknown fields, revision bumping and structural validation.
//!
//! Loading parses the JSON into a map first, deserialises the known shape from
//! it, and stashes every key this version doesn't know in `extra` (as raw JSON
//! text). Saving serialises the known shape and merges `extra` back in, so a
//! device on this schema version never strips state a newer device wrote.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};

use crate::api::{self, EpochMs, QueueSource, SessionDocument, SessionId, TransportState};
use crate::session::saved::is_id_referenced;
use crate::session::shuffle::is_bijection;

/// Schema version of the session document this build writes.
pub const SESSION_SCHEMA_VERSION: u32 = api::API_SCHEMA_VERSION;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum DocumentError {
    #[error("invalid json: {0}")]
    Json(String),
    #[error("session document must be a JSON object")]
    NotAnObject,
    #[error("schema version {0} is older than the minimum supported {1}")]
    TooOld(u32, u32),
    #[error("invalid document: {0}")]
    Invalid(String),
}

/// Oldest schema this build can still read.
pub const SESSION_SCHEMA_MIN: u32 = 1;

/// A fresh, empty document for a scope (`serverId:username`).
pub fn new_document(
    scope: impl Into<String>,
    session_id: SessionId,
    now: EpochMs,
) -> SessionDocument {
    SessionDocument {
        schema_version: SESSION_SCHEMA_VERSION,
        session_id,
        scope: scope.into(),
        revision: 0,
        updated_at: now,
        context: None,
        mode: Default::default(),
        cursor: 0,
        current: None,
        history: vec![],
        insertions: vec![],
        shuffle: None,
        repeat: Default::default(),
        autoplay: false,
        transport: TransportState::default(),
        saved_queues: vec![],
        extra: HashMap::new(),
    }
}

/// Keys this version of the document type serialises. Computed from the type
/// itself so it can't drift from the struct.
fn known_keys() -> HashSet<String> {
    let probe = new_document("probe", "probe".into(), 0.0);
    match serde_json::to_value(&probe) {
        Ok(Value::Object(m)) => {
            let mut keys: HashSet<String> = m.keys().cloned().collect();
            keys.insert("extra".into());
            keys
        }
        _ => HashSet::new(),
    }
}

/// Parse a document, preserving unknown top-level fields in `extra`.
pub fn load(json: &str) -> Result<SessionDocument, DocumentError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| DocumentError::Json(e.to_string()))?;
    load_value(value)
}

/// Same as [`load`] for an already-parsed value.
pub fn load_value(value: Value) -> Result<SessionDocument, DocumentError> {
    let Value::Object(map) = value else {
        return Err(DocumentError::NotAnObject);
    };
    let mut doc: SessionDocument = serde_json::from_value(Value::Object(map.clone()))
        .map_err(|e| DocumentError::Json(e.to_string()))?;
    if doc.schema_version < SESSION_SCHEMA_MIN {
        return Err(DocumentError::TooOld(
            doc.schema_version,
            SESSION_SCHEMA_MIN,
        ));
    }
    let known = known_keys();
    // Anything the wire carried under `extra` (written by a peer that already
    // did this dance) stays, and unknown top-level keys are added beside it.
    for (k, v) in map {
        if !known.contains(&k) {
            doc.extra.insert(k, v.to_string());
        }
    }
    Ok(doc)
}

/// Serialise a document, merging `extra` back into the top level. Known keys
/// always win over a stale `extra` entry of the same name.
pub fn save(doc: &SessionDocument) -> Result<String, DocumentError> {
    let value = save_value(doc)?;
    serde_json::to_string(&value).map_err(|e| DocumentError::Json(e.to_string()))
}

/// Same as [`save`] but returns the merged value.
pub fn save_value(doc: &SessionDocument) -> Result<Value, DocumentError> {
    let value = serde_json::to_value(doc).map_err(|e| DocumentError::Json(e.to_string()))?;
    let Value::Object(mut map) = value else {
        return Err(DocumentError::NotAnObject);
    };
    map.remove("extra");
    // Known keys first, then the preserved unknown ones in a stable order.
    let mut merged: Map<String, Value> = map;
    let mut extra: Vec<(&String, &String)> = doc.extra.iter().collect();
    extra.sort();
    for (k, raw) in extra {
        if merged.contains_key(k) {
            continue;
        }
        let v = serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.clone()));
        merged.insert(k.clone(), v);
    }
    Ok(Value::Object(merged))
}

/// Advance the revision and stamp the time. Every accepted mutation does this
/// exactly once.
pub fn bump_revision(doc: &mut SessionDocument, now: EpochMs) {
    doc.revision = doc.revision.saturating_add(1);
    doc.updated_at = now;
}

/// True when two documents are equal apart from `revision` and `updated_at`.
pub fn same_state(a: &SessionDocument, b: &SessionDocument) -> bool {
    let mut b2 = b.clone();
    b2.revision = a.revision;
    b2.updated_at = a.updated_at;
    *a == b2
}

/// Structural validation. A document that fails here came from a buggy peer
/// or a corrupted store; the reducer still never panics on it, but the actor
/// should refuse to adopt it.
pub fn validate(doc: &SessionDocument) -> Result<(), DocumentError> {
    let n = doc.context.as_ref().map(|c| c.tracks.len()).unwrap_or(0);
    // An ID-referenced context restored from a snapshot has no tracks until
    // the actor resolves them; its cursor and indices are checked afterwards.
    let unresolved = doc
        .context
        .as_ref()
        .map(|c| c.tracks.is_empty() && is_id_referenced(&c.kind))
        .unwrap_or(false);
    if unresolved {
        return validate_rest(doc);
    }
    if doc.context.is_none() {
        if doc.cursor != 0 {
            return Err(DocumentError::Invalid("cursor without context".into()));
        }
        if matches!(
            doc.current.as_ref().map(|c| &c.source),
            Some(QueueSource::Context { .. })
        ) {
            return Err(DocumentError::Invalid(
                "context-sourced current without context".into(),
            ));
        }
    } else if doc.cursor as usize > n {
        // The cursor is the boundary before the next context item, so it may
        // sit just past the last one.
        return Err(DocumentError::Invalid(format!(
            "cursor {} out of range for {} tracks",
            doc.cursor, n
        )));
    }
    for item in doc
        .history
        .iter()
        .chain(doc.current.iter())
        .chain(doc.insertions.iter())
    {
        if let QueueSource::Context { index } = item.source {
            if index as usize >= n {
                return Err(DocumentError::Invalid(format!(
                    "context index {index} out of range for {n} tracks"
                )));
            }
        }
    }
    if let Some(s) = &doc.shuffle {
        if let Some(order) = &s.order {
            if order.len() != n || !is_bijection(order) {
                return Err(DocumentError::Invalid(
                    "shuffle order is not a bijection over the context".into(),
                ));
            }
        }
        if let Some(a) = s.anchor {
            if a as usize >= n {
                return Err(DocumentError::Invalid("shuffle anchor out of range".into()));
            }
        }
    }
    validate_rest(doc)
}

/// The checks that hold even for an unresolved context: unique keys, unique
/// saved-queue ids.
fn validate_rest(doc: &SessionDocument) -> Result<(), DocumentError> {
    let mut keys = HashSet::new();
    for item in doc
        .history
        .iter()
        .chain(doc.current.iter())
        .chain(doc.insertions.iter())
    {
        if !keys.insert(item.key.as_str()) {
            return Err(DocumentError::Invalid(format!(
                "duplicate queue key {}",
                item.key
            )));
        }
    }
    let mut ids = HashSet::new();
    for q in &doc.saved_queues {
        if !ids.insert(q.id.as_str()) {
            return Err(DocumentError::Invalid(format!(
                "duplicate saved queue id {}",
                q.id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContextKind, QueueContext, QueueItem, ShuffleState, SortOrder};

    fn doc_with_context(n: usize) -> SessionDocument {
        let mut d = new_document("s:u", "sess".into(), 1.0);
        d.context = Some(QueueContext {
            server_id: "s".into(),
            kind: ContextKind::AdHoc { label: "x".into() },
            label: "x".into(),
            sort: SortOrder::Default,
            tracks: (0..n).map(|i| format!("t{i}")).collect(),
        });
        d.current = Some(QueueItem {
            key: "k0".into(),
            track_id: "t0".into(),
            source: QueueSource::Context { index: 0 },
            unavailable: false,
        });
        d
    }

    #[test]
    fn newer_document_survives_load_modify_save() {
        // A "future" device wrote fields we have never heard of, at the top
        // level, with nested structure.
        let mut v = serde_json::to_value(doc_with_context(3)).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.insert("schemaVersion".into(), Value::from(7));
        obj.insert("crossfadeMs".into(), Value::from(1500));
        obj.insert(
            "futureBlock".into(),
            serde_json::json!({"a": [1, 2, {"b": null}], "c": "d"}),
        );
        let json = serde_json::to_string(&v).unwrap();

        let mut doc = load(&json).expect("loads");
        assert_eq!(doc.schema_version, 7);
        assert_eq!(doc.extra.len(), 2);
        assert_eq!(doc.extra["crossfadeMs"], "1500");

        // Modify something we do understand.
        doc.cursor = 2;
        bump_revision(&mut doc, 99.0);
        let out = save(&doc).unwrap();
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["cursor"], 2);
        assert_eq!(back["revision"], 1);
        assert_eq!(back["crossfadeMs"], 1500);
        assert_eq!(
            back["futureBlock"],
            serde_json::json!({"a": [1, 2, {"b": null}], "c": "d"})
        );
        // `extra` itself is not written as a nested blob.
        assert!(back.get("extra").is_none());

        // And it survives a second round trip unchanged.
        let again = load(&out).unwrap();
        assert_eq!(again.extra, doc.extra);
        assert_eq!(save(&again).unwrap(), out);
    }

    #[test]
    fn known_keys_win_over_extra() {
        let mut doc = doc_with_context(1);
        doc.extra.insert("cursor".into(), "42".into());
        let v = save_value(&doc).unwrap();
        assert_eq!(v["cursor"], 0);
    }

    #[test]
    fn non_json_extra_is_kept_as_string() {
        let mut doc = doc_with_context(1);
        doc.extra.insert("odd".into(), "not json".into());
        let v = save_value(&doc).unwrap();
        assert_eq!(v["odd"], "not json");
    }

    #[test]
    fn load_errors() {
        assert!(matches!(load("[]"), Err(DocumentError::NotAnObject)));
        assert!(matches!(load("{"), Err(DocumentError::Json(_))));
        let mut v = serde_json::to_value(doc_with_context(1)).unwrap();
        v["schemaVersion"] = Value::from(0);
        assert!(matches!(load_value(v), Err(DocumentError::TooOld(0, 1))));
    }

    #[test]
    fn validation() {
        assert!(validate(&doc_with_context(3)).is_ok());
        let mut d = doc_with_context(3);
        d.cursor = 3;
        assert!(validate(&d).is_ok(), "cursor may sit just past the end");
        d.cursor = 4;
        assert!(validate(&d).is_err());
        let mut d = doc_with_context(3);
        d.history.push(d.current.clone().unwrap());
        assert!(validate(&d).unwrap_err().to_string().contains("duplicate"));
        let mut d = doc_with_context(3);
        d.shuffle = Some(ShuffleState {
            seed: 1,
            anchor: Some(5),
            order: None,
        });
        assert!(validate(&d).is_err());
        d.shuffle = Some(ShuffleState {
            seed: 1,
            anchor: None,
            order: Some(vec![0, 0, 1]),
        });
        assert!(validate(&d).is_err());
        let mut d = new_document("a", "b".into(), 0.0);
        d.cursor = 1;
        assert!(validate(&d).is_err());
    }

    #[test]
    fn same_state_ignores_revision() {
        let a = doc_with_context(2);
        let mut b = a.clone();
        bump_revision(&mut b, 5.0);
        assert!(same_state(&a, &b));
        b.cursor = 1;
        assert!(!same_state(&a, &b));
    }
}
