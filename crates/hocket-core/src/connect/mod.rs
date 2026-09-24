//! Connect: multi-device sessions. Wire types, clock sync, leases/epochs,
//! election, discovery, handoff, replica, and the engine that ties them
//! together. Owner: core-connect.
//!
//! # The engine's contract with the actor
//!
//! [`Engine`] is a **pure, clock-injected state machine**. It never does I/O
//! and never reads the wall clock: it takes an `Arc<dyn util::Clock>` and the
//! actor drives it with [`Input`]s, acting on every [`Output`] it returns.
//! The same engine runs in the app (Android, desktop) and, through its
//! embedded [`Room`], as the LAN coordinator role; the hosted coordinator
//! binary drives a [`Room`] directly. There is one implementation of every
//! protocol rule.
//!
//! ```text
//!                 Input                          Output
//!   actor ───────────────────▶  Engine  ──────────────────────▶ actor
//!    WireIn{peer,msg}            │        WireOut{peer,msg}      → send on that socket
//!    Connected/Disconnected      │        Connect{candidates}    → open upstream socket
//!    PeerConnected/…             │        StartListener/Stop…    → LAN listener
//!    PeerDiscovered/PeerLost     │        Advertise(advert)      → mDNS
//!    LocalOp{op}                 │        DocumentChanged        → session subsystem adopts
//!    LocalStamp{…}               │        LeaseChanged           → playback may start/stop
//!    ClaimTransport/Release…     │        TakeTransport/Release… → playback backend
//!    OpenPicker/HandoffTo/…      │        PreBuffer/Discard…     → playback backend
//!    PreBufferReady{key}         │        PickerChanged/Devices… → UI events
//!    ScrobbleReached{…}          │        Scrobble{allowed}      → outbox
//!    SavedQueuesChanged/Setting… │        SavedQueuesMerged/…    → stores
//!    Tick                        │        ReplicaChanged         → persist (optional)
//! ```
//!
//! Rules the actor must follow:
//!
//! 1. **Call [`Engine::handle`] from one thread** (the actor loop) and act on
//!    every output in order. Outputs are cheap descriptions, never blocking.
//! 2. **`Tick` at least once a second** (the engine keeps its own timers:
//!    heartbeats every 5 s, clock pings, reconnect backoff, pre-buffer
//!    timeouts, lease lapse).
//! 3. **The engine owns the session document.** Every queue mutation is a
//!    [`SessionOp`] submitted through `Input::LocalOp`; the engine applies it
//!    optimistically through the injected [`SessionReducer`], orders it with
//!    the room, and rolls back on rejection. The actor renders from
//!    `Output::DocumentChanged` (or [`Engine::document`]). There is no other
//!    path to the document.
//! 4. **Transport stamps only on change.** Send `Input::LocalStamp` when
//!    play/pause/seek/track changes, never on a timer. Receivers extrapolate
//!    with [`clock::extrapolate`] from [`Engine::session_clock`].
//! 5. **Persist [`Engine::sync_base`] with the document** and pass it back
//!    to [`Engine::new`], so a restarted device can tell "behind" from
//!    "diverged" when it rejoins.
//! 6. `Output::FilePreviousStateAsSavedQueue` means: snapshot this document
//!    with `session::saved::snapshot`, upsert it locally and report it back
//!    through `Input::SavedQueuesChanged`. Never merge it.
//! 7. **Supply the LAN key.** Set [`EngineConfig::lan_key`] to
//!    `Some(auth::derive_lan_key(&scope, &password))` (the scope from
//!    [`wire::scope_key`], the password the user entered for that server)
//!    before building the engine, and rebuild it when the password changes.
//!    LAN rooms admit only devices that prove knowledge of this key and a
//!    device only follows a LAN leader that proves it back ([`auth`]); with
//!    `None` the engine neither serves nor follows LAN peers. The
//!    credential is sent only to a coordinator (`ConnectionTier::Coordinator`)
//!    and only over `wss://` (or `ws://` to loopback, or anywhere when
//!    [`EngineConfig::allow_insecure_coordinator`] is set from the
//!    `connect.allowInsecureCoordinator` setting) — never in a LAN `Hello`.
//! 8. **Persist [`Engine::known_scrobbled`]** next to the sync base and hand
//!    it back through [`Engine::restore_known_scrobbled`] on start, so a
//!    LAN leader that restarts still answers dedupe queries correctly.
//!
//! Everything in [`wire`] is the protocol; [`room`] is the coordinator role;
//! [`replica`] its store; [`lease`], [`clock`], [`election`], [`auth`] are
//! the pure pieces; [`discovery`] and [`transport`] are the I/O seams with
//! real (mDNS, tokio-tungstenite) implementations that the simulation fakes.

pub mod auth;
pub mod clock;
pub mod discovery;
pub mod election;
pub mod engine;
pub mod lease;
pub mod replica;
pub mod room;
pub mod session_adapter;
pub mod transport;
pub mod wire;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::api::{EpochMs, Ms, SessionDocument, SessionId};

pub use engine::{DocChange, Engine, EngineConfig, Input, Output, ResumeOfferDraft, SyncBase};
pub use room::{LanAuth, Room, RoomConfig, RoomInput, RoomOutput};
pub use session_adapter::RealReducer;
pub use wire::{Msg, SessionOp, WireMessage};

/// Handle of one connection, assigned by the transport layer (a socket id).
/// Opaque to the engine; it only routes on it.
pub type PeerId = String;

/// The peer id of the in-process loopback between an engine and its own
/// room. Never times out, never verified.
pub const LOOPBACK: &str = "loopback";

/// Why a reducer could not apply an op. Turned into an [`wire::OpReject`]
/// by the room and a rollback by the engine.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ReduceFailure(pub String);

/// Everything an op application needs besides the document.
#[derive(Debug, Clone, PartialEq)]
pub struct OpContext {
    /// Session-clock time to stamp the document with.
    pub now: EpochMs,
    /// Seed for keys and shuffle seeds: derived from the op id so every
    /// replica materialises the same keys.
    pub seed: u32,
    /// Prefix for generated queue keys (the op id), unique per op.
    pub key_prefix: String,
    /// Playback position of the current item as far as the applier knows
    /// (snapshots carry it). Replicas that aren't playing pass 0.
    pub position_ms: Ms,
}

/// The seam to the session reducer. Implementations must be **deterministic**
/// (same document, op and context → same result on every replica) and
/// **total** for well-formed documents (never panic). They must not touch
/// `revision`, `sessionId` or `scope`: the engine and room set those.
pub trait SessionReducer: Send + Sync {
    fn apply(
        &self,
        doc: &SessionDocument,
        op: &SessionOp,
        ctx: &OpContext,
    ) -> Result<SessionDocument, ReduceFailure>;
}

/// Shared handle to a reducer.
pub type ReducerHandle = Arc<dyn SessionReducer>;

/// Derive the op context every replica uses for an op.
pub fn op_context(op_id: &str, now: EpochMs, position_ms: Ms) -> OpContext {
    OpContext {
        now,
        seed: seed_from(op_id),
        key_prefix: op_id.to_string(),
        position_ms,
    }
}

/// FNV-1a over the op id: stable, cheap, good enough for a shuffle seed.
pub fn seed_from(s: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// Apply an op to a document at a given target revision: runs the reducer
/// (handling [`SessionOp::Replace`] here so every reducer gets it for free),
/// then pins `revision`, `sessionId` and `scope`.
pub fn apply_op(
    reducer: &dyn SessionReducer,
    doc: &SessionDocument,
    op: &SessionOp,
    ctx: &OpContext,
    revision: u32,
) -> Result<SessionDocument, ReduceFailure> {
    let mut next = match op {
        SessionOp::Replace { document } => {
            let mut d = document.clone();
            d.transport.lease = doc.transport.lease.clone();
            d
        }
        other => reducer.apply(doc, other, ctx)?,
    };
    next.revision = revision;
    next.session_id = doc.session_id.clone();
    next.scope = doc.scope.clone();
    next.updated_at = ctx.now;
    Ok(next)
}

/// True when two documents are the same apart from revision, timestamps and
/// transport (which the lease and stamps own).
pub fn same_session_state(a: &SessionDocument, b: &SessionDocument) -> bool {
    let mut x = a.clone();
    let mut y = b.clone();
    x.revision = 0;
    y.revision = 0;
    x.updated_at = 0.0;
    y.updated_at = 0.0;
    x.transport = Default::default();
    y.transport = Default::default();
    x == y
}

/// Nothing worth keeping: no context and nothing current or queued.
pub fn doc_is_trivial(doc: &SessionDocument) -> bool {
    doc.context.is_none() && doc.current.is_none() && doc.insertions.is_empty()
}

/// The last state a device confirmed with a remote room, persisted next to
/// the document so a rejoin can tell "behind" from "diverged".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPoint {
    pub session_id: SessionId,
    pub revision: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_is_stable_and_spread() {
        assert_eq!(seed_from("abc"), seed_from("abc"));
        assert_ne!(seed_from("abc"), seed_from("abd"));
        assert_ne!(seed_from(""), seed_from("a"));
    }

    #[test]
    fn same_session_state_ignores_revision_and_transport() {
        let a = crate::session::new_document("s", "id".into(), 1.0);
        let mut b = a.clone();
        b.revision = 9;
        b.updated_at = 99.0;
        b.transport.played_ms = 5;
        assert!(same_session_state(&a, &b));
        b.autoplay = true;
        assert!(!same_session_state(&a, &b));
        assert!(doc_is_trivial(&a));
    }
}
