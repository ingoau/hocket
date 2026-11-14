//! Helpers over [`api::SessionDocument`]: construction, load/save that
//! round-trips unknown fields, revision bumping and structural validation.
//!
//! Loading parses the JSON into a map first, deserialises the known shape from
//! it, and stashes every key this version doesn't know in `extra` (as raw JSON
//! text). Saving serialises the known shape and merges `extra` back in, so a
//! device on this schema version never strips state a newer device wrote.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde_json::{Map, Value};

use crate::api::{EpochMs, SessionDocument, SessionId, TransportState};

/// Schema version of the session document this build writes. 2: the flat
/// queue list (`history` / `current` / `upcoming`, no modes, cursor or
/// shuffle permutation).
pub const SESSION_SCHEMA_VERSION: u32 = 2;

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

/// Oldest schema this build can still read. Older documents are not
/// migrated: loading one fails and the caller starts a fresh session.
pub const SESSION_SCHEMA_MIN: u32 = 2;

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
        history: vec![],
        current: None,
        upcoming: vec![],
        shuffle: false,
        repeat: Default::default(),
        autoplay: false,
        transport: TransportState::default(),
        saved_queues: vec![],
        extra: HashMap::new(),
    }
}

/// Keys this version of the document type serialises. Computed from the type
/// itself so it can't drift from the struct.
fn known_keys() -> &'static HashSet<String> {
    static KEYS: OnceLock<HashSet<String>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let probe = new_document("probe", "probe".into(), 0.0);
        match serde_json::to_value(&probe) {
            Ok(Value::Object(m)) => {
                let mut keys: HashSet<String> = m.keys().cloned().collect();
                keys.insert("extra".into());
                keys
            }
            _ => HashSet::new(),
        }
    })
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
    let known = known_keys();
    let extras: Vec<(String, String)> = map
        .iter()
        .filter(|(k, _)| !known.contains(*k))
        .map(|(k, v)| (k.clone(), v.to_string()))
        .collect();
    let mut doc: SessionDocument = serde_json::from_value(Value::Object(map))
        .map_err(|e| DocumentError::Json(e.to_string()))?;
    if doc.schema_version < SESSION_SCHEMA_MIN {
        return Err(DocumentError::TooOld(
            doc.schema_version,
            SESSION_SCHEMA_MIN,
        ));
    }
    // Anything the wire carried under `extra` (written by a peer that already
    // did this dance) stays, and unknown top-level keys are added beside it.
    for (k, v) in extras {
        doc.extra.insert(k, v);
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
/// Field by field, without copying either (every reducer op asks, and a
/// long context makes a copy cost as much as the op). The destructuring is
/// exhaustive, so a new field cannot be left out.
pub fn same_state(a: &SessionDocument, b: &SessionDocument) -> bool {
    let SessionDocument {
        schema_version,
        session_id,
        scope,
        revision: _,
        updated_at: _,
        context,
        history,
        current,
        upcoming,
        shuffle,
        repeat,
        autoplay,
        transport,
        saved_queues,
        extra,
    } = a;
    *schema_version == b.schema_version
        && *session_id == b.session_id
        && *scope == b.scope
        && *repeat == b.repeat
        && *autoplay == b.autoplay
        && *current == b.current
        && *shuffle == b.shuffle
        && *transport == b.transport
        && *upcoming == b.upcoming
        && *history == b.history
        && *saved_queues == b.saved_queues
        && *extra == b.extra
        && *context == b.context
}

/// Structural validation. A document that fails here came from a buggy peer
/// or a corrupted store; the reducer still never panics on it, but the actor
/// should refuse to adopt it.
pub fn validate(doc: &SessionDocument) -> Result<(), DocumentError> {
    if doc.current.is_none() && !doc.upcoming.is_empty() {
        return Err(DocumentError::Invalid(
            "entries after the pointer with nothing current".into(),
        ));
    }
    let mut keys = HashSet::new();
    for item in doc
        .history
        .iter()
        .chain(doc.current.iter())
        .chain(doc.upcoming.iter())
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
    use crate::api::{ContextKind, QueueContext, QueueItem, QueueSource, SortOrder};

    fn doc_with_context(n: usize) -> SessionDocument {
        let mut d = new_document("s:u", "sess".into(), 1.0);
        d.context = Some(QueueContext {
            source: "s".into(),
            kind: ContextKind::AdHoc { label: "x".into() },
            label: "x".into(),
            sort: SortOrder::Default,
            tracks: (0..n)
                .map(|i| format!("t{i}"))
                .map(crate::api::TrackRef::from)
                .collect(),
        });
        d.current = Some(QueueItem {
            key: "k0".into(),
            track: "t0".into(),
            source: QueueSource::Context { index: 0 },
            unavailable: false,
            added: false,
            queued: false,
            ordinal: 0,
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
        doc.shuffle = true;
        bump_revision(&mut doc, 99.0);
        let out = save(&doc).unwrap();
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["shuffle"], true);
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
        doc.extra.insert("shuffle".into(), "42".into());
        let v = save_value(&doc).unwrap();
        assert_eq!(v["shuffle"], false);
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
        assert!(matches!(load_value(v), Err(DocumentError::TooOld(0, 2))));
    }

    #[test]
    fn old_queue_model_documents_do_not_load() {
        // Schema 1: the Apple/YouTube model with a cursor and insertion list.
        let old = serde_json::json!({
            "schemaVersion": 1, "sessionId": "s", "scope": "a:b", "revision": 3,
            "updatedAt": 1.0, "context": null, "mode": "apple", "cursor": 0,
            "current": null, "history": [], "insertions": [], "shuffle": null,
            "repeat": "off", "autoplay": false,
            "transport": serde_json::to_value(TransportState::default()).unwrap(),
            "savedQueues": []
        });
        assert!(load(&old.to_string()).is_err());
        // Even when it claims the new version, the shape doesn't parse.
        let mut claimed = old.clone();
        claimed["schemaVersion"] = Value::from(SESSION_SCHEMA_VERSION);
        assert!(load(&claimed.to_string()).is_err());
    }

    #[test]
    fn validation() {
        assert!(validate(&doc_with_context(3)).is_ok());
        let mut d = doc_with_context(3);
        d.history.push(d.current.clone().unwrap());
        assert!(validate(&d).unwrap_err().to_string().contains("duplicate"));
        let mut d = doc_with_context(3);
        let mut next = d.current.clone().unwrap();
        next.key = "k1".into();
        d.upcoming.push(next);
        assert!(validate(&d).is_ok());
        d.current = None;
        assert!(validate(&d).is_err(), "upcoming with nothing current");
    }

    #[test]
    fn same_state_ignores_revision() {
        let a = doc_with_context(2);
        let mut b = a.clone();
        bump_revision(&mut b, 5.0);
        assert!(same_state(&a, &b));
        b.shuffle = true;
        assert!(!same_state(&a, &b));
        let mut c = a.clone();
        c.extra.insert("contextKeys".into(), "{}".into());
        assert!(!same_state(&a, &c));
        let mut d = a.clone();
        d.context.as_mut().unwrap().tracks.push("x".into());
        assert!(!same_state(&a, &d));
        let mut e = a.clone();
        e.transport.played_ms = 1;
        assert!(!same_state(&a, &e));
    }
}
