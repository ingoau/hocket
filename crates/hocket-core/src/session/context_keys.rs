//! Stable keys for upcoming context items (#97).
//!
//! Stored items (history, current, Playing Next) carry their own key. Upcoming
//! context items are derived, so their keys live beside the context: one slot
//! per unshuffled context index, kept in [`SessionDocument::extra`] under
//! [`CONTEXT_KEYS_FIELD`] so the public document shape is unchanged and older
//! peers carry it along untouched. A slot is moved, inserted and removed
//! together with its track, so a key follows its song through renumbering,
//! shuffle toggles and edits.
//!
//! When a context item becomes current it takes its slot's key and the slot
//! gets a spare one (the slot's track may come round again, after a Repeat-All
//! wrap or a reshuffle, while the played item is in history). Going back past
//! it puts its key back in the slot, so Next then Previous restores the slot
//! exactly.
//!
//! The field records a hash of the track list it was written for. An older
//! peer that edits the context leaves it stale; a stale or missing field reads
//! as absent, upcoming items fall back to `ctx-<index>`, and the next op
//! writes fresh keys. Either way an old key never names a different song: it
//! is either the same song or unknown.
//!
//! Most slots are a number `n` standing for `<base>.<n>`, so the field stays
//! small for long playlists; a slot holding any other key (a spare, or an item
//! moved in from Playing Next) stores it as a string.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::api::{QueueKey, SessionDocument, TrackId};

use super::Entropy;

/// Top-level document field holding the slots (see the module docs).
pub const CONTEXT_KEYS_FIELD: &str = "contextKeys";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum Slot {
    Serial(u32),
    Key(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContextKeys {
    base: String,
    /// Hash of the track list the slots line up with.
    of: String,
    slots: Vec<Slot>,
}

impl ContextKeys {
    /// Fresh slots for `n` tracks.
    pub(crate) fn fresh(entropy: &dyn Entropy, n: usize) -> ContextKeys {
        ContextKeys {
            base: entropy.next_key(),
            of: String::new(),
            slots: (0..n as u32).map(Slot::Serial).collect(),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    fn key_of(&self, slot: &Slot) -> QueueKey {
        match slot {
            Slot::Serial(n) => format!("{}.{n}", self.base),
            Slot::Key(k) => k.clone(),
        }
    }

    /// The key of the item at unshuffled `index`.
    pub(crate) fn key(&self, index: usize) -> Option<QueueKey> {
        self.slots.get(index).map(|s| self.key_of(s))
    }

    /// The serial `n` when `key` is exactly `<base>.<n>`.
    fn serial_of(&self, key: &str) -> Option<u32> {
        key.strip_prefix(self.base.as_str())
            .and_then(|rest| rest.strip_prefix('.'))
            .and_then(|n| n.parse().ok())
            .filter(|n: &u32| format!("{}.{n}", self.base) == key)
    }

    /// The slot that stands for `key`.
    pub(crate) fn slot_for(&self, key: &str) -> Slot {
        self.serial_of(key)
            .map(Slot::Serial)
            .unwrap_or_else(|| Slot::Key(key.to_string()))
    }

    /// Every slot's key differs from the others and from each of `stored`.
    /// Compared as slots (a key and the slot standing for it are the same),
    /// so no serial key is formatted: a long context has thousands.
    fn clear_of<'a>(&'a self, stored: impl Iterator<Item = &'a str>) -> bool {
        #[derive(PartialEq, Eq, Hash)]
        enum Id<'a> {
            Serial(u32),
            Key(&'a str),
        }
        let id = |key: &'a str| match self.serial_of(key) {
            Some(n) => Id::Serial(n),
            None => Id::Key(key),
        };
        let mut seen: HashSet<Id<'a>> = stored.map(id).collect();
        seen.reserve(self.slots.len());
        self.slots.iter().all(|s| {
            seen.insert(match s {
                Slot::Serial(n) => Id::Serial(*n),
                Slot::Key(k) => id(k),
            })
        })
    }

    /// Unshuffled index of the slot holding `key`.
    pub(crate) fn index_of(&self, key: &str) -> Option<u32> {
        let slot = self.slot_for(key);
        self.slots.iter().position(|s| *s == slot).map(|i| i as u32)
    }

    /// Hand out the key at `index` and put a spare in its place.
    pub(crate) fn take(&mut self, index: usize, entropy: &dyn Entropy) -> Option<QueueKey> {
        let key = self.key(index)?;
        self.slots[index] = Slot::Key(entropy.next_key());
        Some(key)
    }

    /// Put `key` (back) in the slot at `index`.
    pub(crate) fn set(&mut self, index: usize, key: &str) {
        let slot = self.slot_for(key);
        if let Some(s) = self.slots.get_mut(index) {
            *s = slot;
        }
    }

    pub(crate) fn remove(&mut self, index: usize) {
        if index < self.slots.len() {
            self.slots.remove(index);
        }
    }

    /// Insert a slot at `index`, holding `key` or a new one.
    pub(crate) fn insert(&mut self, index: usize, key: Option<&str>, entropy: &dyn Entropy) {
        let slot = match key {
            Some(k) => self.slot_for(k),
            None => Slot::Key(entropy.next_key()),
        };
        self.slots.insert(index.min(self.slots.len()), slot);
    }

    /// Slots for a new track list: each track keeps the key of the same track
    /// in the old list (in order, for repeats), the rest get new ones.
    pub(crate) fn realign(
        &self,
        old: &[TrackId],
        new: &[TrackId],
        entropy: &dyn Entropy,
    ) -> ContextKeys {
        let mut used = vec![false; old.len()];
        let slots = new
            .iter()
            .map(|t| {
                let found = old
                    .iter()
                    .enumerate()
                    .position(|(i, o)| !used[i] && o == t && i < self.slots.len());
                match found {
                    Some(i) => {
                        used[i] = true;
                        self.slots[i].clone()
                    }
                    None => Slot::Key(entropy.next_key()),
                }
            })
            .collect();
        ContextKeys {
            base: self.base.clone(),
            of: String::new(),
            slots,
        }
    }
}

/// FNV-1a over the track ids: cheap, and the same on every platform.
fn tracks_hash(tracks: &[TrackId]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for t in tracks {
        for b in t.as_bytes().iter().chain(std::iter::once(&0u8)) {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

fn tracks(doc: &SessionDocument) -> &[TrackId] {
    doc.context
        .as_ref()
        .map(|c| c.tracks.as_slice())
        .unwrap_or(&[])
}

/// The stored slots, without checking they fit the track list. Within an op
/// the reducer keeps them in step; [`read`] is for everyone else.
pub(crate) fn read_unchecked(doc: &SessionDocument) -> Option<ContextKeys> {
    doc.extra
        .get(CONTEXT_KEYS_FIELD)
        .and_then(|raw| serde_json::from_str(raw).ok())
}

/// The stored slots when they line up with the current track list.
pub(crate) fn read(doc: &SessionDocument) -> Option<ContextKeys> {
    let ts = tracks(doc);
    read_unchecked(doc).filter(|k| k.slots.len() == ts.len() && k.of == tracks_hash(ts))
}

/// Store slots for the current track list (or drop the field without one).
pub(crate) fn write(doc: &mut SessionDocument, mut keys: ContextKeys) {
    let ts = tracks(doc);
    if ts.is_empty() {
        doc.extra.remove(CONTEXT_KEYS_FIELD);
        return;
    }
    keys.of = tracks_hash(ts);
    if let Ok(raw) = serde_json::to_string(&keys) {
        doc.extra.insert(CONTEXT_KEYS_FIELD.into(), raw);
    }
}

/// Make sure the document has usable slots: present, lined up with the
/// track list, unique and clear of every stored key. Otherwise (no field yet,
/// a stale one from an older peer's edit, a clash) write fresh ones. Draws
/// entropy only when it writes, so every replica does the same.
pub fn ensure(doc: &mut SessionDocument, entropy: &dyn Entropy) {
    let n = tracks(doc).len();
    if n == 0 {
        doc.extra.remove(CONTEXT_KEYS_FIELD);
        return;
    }
    let usable = read(doc).is_some_and(|k| {
        k.clear_of(
            doc.history
                .iter()
                .chain(doc.current.iter())
                .chain(doc.insertions.iter())
                .map(|i| i.key.as_str()),
        )
    });
    if !usable {
        write(doc, ContextKeys::fresh(entropy, n));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::DeterministicEntropy;

    #[test]
    fn serial_slots_round_trip_their_keys() {
        let e = DeterministicEntropy::new(1);
        let mut k = ContextKeys::fresh(&e, 3);
        assert_eq!(k.key(2).as_deref(), Some("k1.2"));
        assert_eq!(k.slot_for("k1.2"), Slot::Serial(2));
        assert_eq!(k.slot_for("k1.02"), Slot::Key("k1.02".into()));
        assert_eq!(k.slot_for("k10.2"), Slot::Key("k10.2".into()));
        let taken = k.take(1, &e).unwrap();
        assert_eq!(taken, "k1.1");
        assert_eq!(k.key(1).as_deref(), Some("k2"));
        k.set(1, &taken);
        assert_eq!(k, ContextKeys::fresh(&DeterministicEntropy::new(1), 3));
        assert_eq!(k.index_of("k1.1"), Some(1));
        assert_eq!(k.index_of("k2"), None);
    }

    #[test]
    fn clear_of_compares_keys_as_written() {
        let e = DeterministicEntropy::new(1);
        let mut k = ContextKeys::fresh(&e, 3);
        assert!(k.clear_of(["x", "k1.02", "k10.2"].into_iter()));
        assert!(
            !k.clear_of(["x", "k1.2"].into_iter()),
            "a stored key in a slot"
        );
        k.slots[0] = Slot::Key("k9".into());
        assert!(k.clear_of(std::iter::empty()));
        assert!(!k.clear_of(["k9"].into_iter()));
        // A key slot spelling a serial one is the same key twice.
        k.slots[0] = Slot::Key("k1.2".into());
        assert!(!k.clear_of(std::iter::empty()));
        k.slots[0] = Slot::Key("k1.1".into());
        assert!(!k.clear_of(std::iter::empty()));
        k.slots[0] = Slot::Serial(2);
        assert!(!k.clear_of(std::iter::empty()));
    }

    #[test]
    fn realign_keeps_keys_by_track() {
        let e = DeterministicEntropy::new(1);
        let k = ContextKeys::fresh(&e, 3);
        let old: Vec<TrackId> = vec!["a".into(), "b".into(), "a".into()];
        let new: Vec<TrackId> = vec!["a".into(), "c".into(), "a".into(), "a".into()];
        let r = k.realign(&old, &new, &e);
        assert_eq!(r.key(0).as_deref(), Some("k1.0"));
        assert_eq!(r.key(1).as_deref(), Some("k2"));
        assert_eq!(r.key(2).as_deref(), Some("k1.2"));
        assert_eq!(r.key(3).as_deref(), Some("k3"));
    }
}
