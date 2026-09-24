//! Byte spans of a partially cached stream: sorted, non-overlapping,
//! half-open `[start, end)` ranges, adjacent and overlapping ones merged.
//! Persisted in `cache_entries.spans` as `start-end,start-end`.

/// A set of half-open byte ranges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanSet {
    spans: Vec<(u64, u64)>,
}

impl SpanSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// One span `[start, end)` (empty when `end <= start`).
    pub fn of(start: u64, end: u64) -> Self {
        let mut s = Self::new();
        s.insert(start, end);
        s
    }

    pub fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    pub fn clear(&mut self) {
        self.spans.clear();
    }

    pub fn spans(&self) -> &[(u64, u64)] {
        &self.spans
    }

    /// Add `[start, end)`, merging with anything it touches.
    pub fn insert(&mut self, start: u64, end: u64) {
        if end <= start {
            return;
        }
        let (mut s, mut e) = (start, end);
        let mut out = Vec::with_capacity(self.spans.len() + 1);
        let mut placed = false;
        for &(a, b) in &self.spans {
            if b < s {
                out.push((a, b));
            } else if a > e {
                if !placed {
                    out.push((s, e));
                    placed = true;
                }
                out.push((a, b));
            } else {
                s = s.min(a);
                e = e.max(b);
            }
        }
        if !placed {
            out.push((s, e));
        }
        self.spans = out;
    }

    /// Every span of `other` added.
    pub fn union(&mut self, other: &SpanSet) {
        for &(a, b) in &other.spans {
            self.insert(a, b);
        }
    }

    /// End of the span holding byte `pos`, if one does.
    pub fn end_at(&self, pos: u64) -> Option<u64> {
        self.spans
            .iter()
            .find(|(a, b)| *a <= pos && pos < *b)
            .map(|(_, b)| *b)
    }

    /// Start of the first span beginning after `pos`.
    pub fn next_start_after(&self, pos: u64) -> Option<u64> {
        self.spans.iter().map(|(a, _)| *a).find(|a| *a > pos)
    }

    /// Whether `[0, total)` is covered.
    pub fn covers(&self, total: u64) -> bool {
        total > 0
            && self
                .spans
                .first()
                .is_some_and(|(a, b)| *a == 0 && *b >= total)
    }

    /// Bytes held.
    pub fn bytes(&self) -> u64 {
        self.spans.iter().map(|(a, b)| b - a).sum()
    }

    /// End of the last span (the file must be at least this long).
    pub fn max_end(&self) -> u64 {
        self.spans.last().map_or(0, |(_, b)| *b)
    }

    /// Bytes held from 0 contiguously.
    pub fn prefix(&self) -> u64 {
        match self.spans.first() {
            Some((0, b)) => *b,
            _ => 0,
        }
    }

    /// Drop everything at or past `limit`.
    pub fn truncate(&mut self, limit: u64) {
        self.spans.retain(|(a, _)| *a < limit);
        if let Some(last) = self.spans.last_mut() {
            last.1 = last.1.min(limit);
        }
    }

    /// `start-end,start-end` (empty for no spans).
    pub fn encode(&self) -> String {
        self.spans
            .iter()
            .map(|(a, b)| format!("{a}-{b}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parse [`encode`](Self::encode)'s output; anything malformed is
    /// dropped (a cache index never fails a read).
    pub fn decode(text: &str) -> SpanSet {
        let mut s = SpanSet::new();
        for part in text.split(',') {
            let Some((a, b)) = part.trim().split_once('-') else {
                continue;
            };
            if let (Ok(a), Ok(b)) = (a.parse::<u64>(), b.parse::<u64>()) {
                s.insert(a, b);
            }
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_merges_adjacent_and_overlapping() {
        let mut s = SpanSet::new();
        s.insert(10, 20);
        s.insert(30, 40);
        assert_eq!(s.spans(), &[(10, 20), (30, 40)]);
        s.insert(20, 25);
        assert_eq!(s.spans(), &[(10, 25), (30, 40)], "adjacent merges");
        s.insert(0, 5);
        s.insert(22, 31);
        assert_eq!(s.spans(), &[(0, 5), (10, 40)]);
        s.insert(5, 5);
        assert_eq!(s.spans(), &[(0, 5), (10, 40)], "empty ignored");
        s.insert(3, 12);
        assert_eq!(s.spans(), &[(0, 40)]);
        assert!(s.covers(40) && !s.covers(41));
        assert_eq!(s.bytes(), 40);
    }

    #[test]
    fn lookups() {
        let mut s = SpanSet::of(100, 200);
        s.insert(300, 400);
        assert_eq!(s.end_at(100), Some(200));
        assert_eq!(s.end_at(199), Some(200));
        assert_eq!(s.end_at(200), None);
        assert_eq!(s.end_at(50), None);
        assert_eq!(s.next_start_after(0), Some(100));
        assert_eq!(s.next_start_after(150), Some(300));
        assert_eq!(s.next_start_after(300), None);
        assert_eq!(s.max_end(), 400);
        assert_eq!(s.prefix(), 0);
        assert_eq!(SpanSet::of(0, 7).prefix(), 7);
        s.truncate(350);
        assert_eq!(s.spans(), &[(100, 200), (300, 350)]);
        s.truncate(250);
        assert_eq!(s.spans(), &[(100, 200)]);
    }

    #[test]
    fn encode_round_trips_and_tolerates_garbage() {
        let mut s = SpanSet::of(0, 10);
        s.insert(20, 30);
        assert_eq!(s.encode(), "0-10,20-30");
        assert_eq!(SpanSet::decode(&s.encode()), s);
        assert_eq!(SpanSet::decode(""), SpanSet::new());
        assert_eq!(SpanSet::decode("x,5-1,3-9, 9-12"), SpanSet::of(3, 12));
        let mut u = SpanSet::of(5, 15);
        u.union(&s);
        assert_eq!(u.spans(), &[(0, 15), (20, 30)]);
    }
}
