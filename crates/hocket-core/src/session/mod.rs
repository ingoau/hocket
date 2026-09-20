//! Session document, queue reducer, history, saved queues. Owner: core-session.
//!
//! # Entry points for the actor
//!
//! - [`Session`] wraps the live [`api::SessionDocument`](crate::api::SessionDocument)
//!   together with its policy, clock and entropy source:
//!   - `Session::new(doc, config, clock, entropy)` / `Session::empty(scope, session_id, ..)`
//!   - `session.apply(op, position_ms) -> Result<Vec<Effect>, ReduceError>` runs
//!     the pure reducer and adopts the result;
//!   - `session.derive() -> DerivedQueue` and `DerivedQueue::into_view(resolve)`
//!     build the `QueueView` for the UI once track summaries are resolved;
//!   - `session.replace(doc)` adopts a document from sync or undo;
//!   - `session.doc()` / `session.snapshot()` read it.
//! - [`reducer::reduce`] is the pure function underneath, with [`QueueOp`]
//!   (every queue-affecting command, `QueueOp::from_command` maps the public
//!   ones), [`ReduceCtx`] and [`Effect`].
//! - [`document`] loads and saves documents preserving unknown fields, bumps
//!   revisions and validates.
//! - [`saved`] holds the saved-queue policy, snapshotting, dedupe / eviction
//!   and `merge_saved_queues` (LWW sync).
//! - [`shuffle`] is the deterministic permutation.
//! - [`reducer::previous_should_restart`] tells the actor whether a Previous
//!   press should restart the track (position ≥ 3 s) instead of going back.
//!
//! Every queue mutation goes through the reducer. There is no other path.

pub mod document;
pub mod reducer;
pub mod saved;
pub mod shuffle;

use std::sync::Arc;

use parking_lot::Mutex;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::api::{EpochMs, Ms, QueueKey, SessionDocument, SessionId};
use crate::util::Clock;

pub use document::{load, new_document, save, validate, DocumentError, SESSION_SCHEMA_VERSION};
pub use reducer::{
    derive, previous_should_restart, reduce, AutoplayItem, DerivedQueue, Effect, QueueOp,
    ReduceCtx, ReduceError, DEFAULT_HISTORY_CAP,
};
pub use saved::{merge_saved_queues, SavedQueuePolicy};

/// Source of queue keys and shuffle seeds. Real builds use [`SystemEntropy`];
/// tests and the simulation harness use [`DeterministicEntropy`] so a scenario
/// replays byte-for-byte.
pub trait Entropy: Send + Sync {
    fn next_key(&self) -> QueueKey;
    fn next_seed(&self) -> u32;
}

/// UUID keys and OS-random seeds.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn next_key(&self) -> QueueKey {
        crate::util::new_id()
    }
    fn next_seed(&self) -> u32 {
        rand::random()
    }
}

/// Counter keys (`k1`, `k2`, …) and ChaCha-derived seeds from a fixed seed.
#[derive(Debug)]
pub struct DeterministicEntropy {
    counter: Mutex<u64>,
    rng: Mutex<ChaCha8Rng>,
}

impl DeterministicEntropy {
    pub fn new(seed: u64) -> Self {
        DeterministicEntropy {
            counter: Mutex::new(0),
            rng: Mutex::new(ChaCha8Rng::seed_from_u64(seed)),
        }
    }
}

impl Entropy for DeterministicEntropy {
    fn next_key(&self) -> QueueKey {
        let mut c = self.counter.lock();
        *c += 1;
        format!("k{}", *c)
    }
    fn next_seed(&self) -> u32 {
        self.rng.lock().next_u32()
    }
}

/// Tunables the settings subsystem feeds in.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionConfig {
    pub history_cap: usize,
    pub saved: SavedQueuePolicy,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            history_cap: DEFAULT_HISTORY_CAP,
            saved: SavedQueuePolicy::default(),
        }
    }
}

/// The live session: one document plus what the reducer needs around it.
pub struct Session {
    doc: SessionDocument,
    config: SessionConfig,
    clock: Arc<dyn Clock>,
    entropy: Arc<dyn Entropy>,
}

impl Session {
    pub fn new(
        doc: SessionDocument,
        config: SessionConfig,
        clock: Arc<dyn Clock>,
        entropy: Arc<dyn Entropy>,
    ) -> Session {
        Session {
            doc,
            config,
            clock,
            entropy,
        }
    }

    /// A fresh empty session for a scope (`serverId:username`).
    pub fn empty(
        scope: &str,
        session_id: SessionId,
        config: SessionConfig,
        clock: Arc<dyn Clock>,
        entropy: Arc<dyn Entropy>,
    ) -> Session {
        let doc = new_document(scope, session_id, clock.now_ms());
        Session::new(doc, config, clock, entropy)
    }

    pub fn doc(&self) -> &SessionDocument {
        &self.doc
    }

    /// A clone of the document (undo snapshots, sync writes).
    pub fn snapshot(&self) -> SessionDocument {
        self.doc.clone()
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: SessionConfig) {
        self.config = config;
    }

    /// Adopt a document from sync or undo. Validation is the caller's call
    /// (`document::validate`); the reducer tolerates anything.
    pub fn replace(&mut self, doc: SessionDocument) {
        self.doc = doc;
    }

    /// Apply an op. On error the document is untouched.
    pub fn apply(&mut self, op: QueueOp, position_ms: Ms) -> Result<Vec<Effect>, ReduceError> {
        let ctx = ReduceCtx {
            now: self.clock.now_ms(),
            position_ms,
            history_cap: self.config.history_cap,
            saved: self.config.saved.clone(),
            entropy: self.entropy.as_ref(),
        };
        let (doc, effects) = reduce(&self.doc, op, &ctx)?;
        self.doc = doc;
        Ok(effects)
    }

    pub fn derive(&self) -> DerivedQueue {
        derive(&self.doc)
    }

    pub fn now(&self) -> EpochMs {
        self.clock.now_ms()
    }
}

#[cfg(test)]
mod proptests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ContextKind, PlayContextArgs, QueueContext, SortOrder};

    struct FixedClock(f64);
    impl Clock for FixedClock {
        fn now_ms(&self) -> f64 {
            self.0
        }
    }

    #[test]
    fn session_wrapper_applies_and_derives() {
        let mut s = Session::empty(
            "srv:user",
            "sess".into(),
            SessionConfig::default(),
            Arc::new(FixedClock(1000.0)),
            Arc::new(DeterministicEntropy::new(1)),
        );
        assert_eq!(s.doc().updated_at, 1000.0);
        let context = QueueContext {
            server_id: "srv".into(),
            kind: ContextKind::AdHoc {
                label: "sel".into(),
            },
            label: "sel".into(),
            sort: SortOrder::Default,
            tracks: vec!["a".into(), "b".into()],
        };
        let fx = s
            .apply(
                QueueOp::PlayContext {
                    args: PlayContextArgs {
                        context,
                        start_index: Some(0),
                        shuffle: false,
                        save_outgoing: true,
                    },
                },
                0,
            )
            .unwrap();
        assert!(matches!(fx[0], Effect::CurrentChanged { .. }));
        assert_eq!(s.derive().upcoming.len(), 1);
        assert_eq!(s.doc().revision, 1);
        let err = s
            .apply(QueueOp::JumpToQueueItem { key: "nope".into() }, 0)
            .unwrap_err();
        assert_eq!(err, ReduceError::UnknownKey("nope".into()));
        assert_eq!(s.doc().revision, 1, "errors leave the document untouched");
        let snap = s.snapshot();
        s.apply(QueueOp::Next, 0).unwrap();
        s.replace(snap.clone());
        assert_eq!(s.doc(), &snap);
    }

    #[test]
    fn entropy_sources() {
        let d = DeterministicEntropy::new(5);
        assert_eq!(d.next_key(), "k1");
        assert_eq!(d.next_key(), "k2");
        let a = d.next_seed();
        let d2 = DeterministicEntropy::new(5);
        assert_eq!(d2.next_seed(), a);
        let s = SystemEntropy;
        assert_ne!(s.next_key(), s.next_key());
    }
}
