//! Dedup: which songs, albums and artists on different accounts are the same
//! (`docs/providers.md`, Phase 3).
//!
//! The matcher is strict and errs towards leaving things apart. Two songs
//! match when they come from different sources and have the same normalised
//! artist, title and album, the same explicit flag, durations within
//! [`DURATION_SLACK_MS`], and (when both report one) the same ISRC. Titles
//! keep their qualifiers ("(Live)", "- 2011 Remaster", "(Clean)",
//! "(Acoustic Version)"), so versions stay apart; only spelling noise
//! (case, accents, quotes, brackets, "feat." credits) is folded away.
//!
//! Manual links win over the matcher: "same" joins two groups whatever the
//! matcher thinks, and "not the same" stops it from ever putting the pair in
//! one group. Each group has a representative, the copy on the
//! highest-priority account, which is the row the unified library lists.
//!
//! Everything here is pure; the actor feeds it the mirror and writes the
//! result to the `links` table.

use std::collections::{BTreeMap, HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

use crate::api::{SourceId, TrackRef};

/// How far apart two copies' durations may be.
pub const DURATION_SLACK_MS: u32 = 2_000;

/// One song as the matcher sees it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SongFacts {
    pub track: TrackRef,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: u32,
    pub explicit: bool,
    pub isrc: Option<String>,
}

/// One album or artist as the matcher sees it. `name` is the album's or
/// artist's name, `artist` the album's artist (none for an artist).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EntityFacts {
    pub source: SourceId,
    pub id: String,
    pub name: String,
    pub artist: Option<String>,
    pub year: Option<u32>,
}

/// A hand-made assertion about two songs: the same or not the same.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ManualLink {
    pub a: TrackRef,
    pub b: TrackRef,
    pub same: bool,
}

/// One member of a group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub track: TrackRef,
    /// Joined by a manual link rather than the matcher.
    pub manual: bool,
}

/// A set of copies of one song (two or more), representative first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Stable for the set of members: the smallest member reference.
    pub id: String,
    pub members: Vec<Member>,
}

impl Group {
    pub fn representative(&self) -> &TrackRef {
        &self.members[0].track
    }
}

/// An album or artist group, representative first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityGroup {
    pub id: String,
    pub members: Vec<(SourceId, String)>,
}

/// Case, accents and typography folded: lowercase, no combining marks,
/// curly quotes and dashes made plain.
pub fn fold(s: &str) -> String {
    s.nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{2032}' | '`' | '´' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{2033}' => '"',
            '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
            '[' | '{' => '(',
            ']' | '}' => ')',
            c => c,
        })
        .collect()
}

/// Letters, digits and parentheses, single-spaced. "&" reads as "and".
fn squash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() || c == '(' || c == ')' {
            if space && !out.is_empty() && !out.ends_with('(') && c != ')' {
                out.push(' ');
            }
            space = false;
            out.push(c);
        } else if c == '&' {
            if !out.is_empty() && !out.ends_with('(') {
                out.push(' ');
            }
            out.push_str("and");
            space = true;
        } else if c == '\'' {
            // "don't" and "dont" are one word.
        } else {
            space = true;
        }
    }
    out.trim().to_string()
}

/// Whether a bracketed part is only a featuring credit.
fn is_feat(part: &str) -> bool {
    let p = part.trim_start();
    ["feat.", "feat ", "ft.", "ft ", "featuring ", "with "]
        .iter()
        .any(|f| p.starts_with(f))
}

/// Strip a trailing unbracketed "feat. X" (the credit, not the song).
fn strip_trailing_feat(s: &str) -> &str {
    for marker in [" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some(i) = s.find(marker) {
            return &s[..i];
        }
    }
    s
}

/// A song or album title as the matcher compares it. Qualifiers stay: a
/// " - Live" suffix becomes "(live)", like the bracketed form, so the two
/// spellings of one version agree while versions still differ. Featuring
/// credits go.
pub fn title_key(title: &str) -> String {
    let folded = fold(title);
    // "Song - Live at X" -> "Song (Live at X)"; only the first " - ".
    let folded = match folded.find(" - ") {
        Some(i) => format!("{} ({})", &folded[..i], &folded[i + 3..]),
        None => folded,
    };
    // Drop bracketed featuring credits.
    let mut out = String::with_capacity(folded.len());
    let mut rest = folded.as_str();
    while let Some(open) = rest.find('(') {
        let Some(close) = rest[open..].find(')').map(|c| open + c) else {
            break;
        };
        out.push_str(&rest[..open]);
        let inner = &rest[open + 1..close];
        if !is_feat(inner) {
            out.push('(');
            out.push_str(inner);
            out.push(')');
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    squash(strip_trailing_feat(&out))
}

/// An artist name as the matcher compares it: the primary credit only (so
/// "A feat. B" and "A" agree), and "The" dropped.
pub fn artist_key(artist: &str) -> String {
    let folded = fold(artist);
    let primary = strip_trailing_feat(&folded);
    let primary = primary.split([';', '/']).next().unwrap_or(primary).trim();
    let s = squash(primary);
    s.strip_prefix("the ").map(str::to_string).unwrap_or(s)
}

fn ref_key(r: &TrackRef) -> String {
    format!("{}\u{1}{}", r.source, r.id)
}

/// Whether two songs are copies of one recording (their keys already agree).
fn compatible(a: &SongFacts, b: &SongFacts) -> bool {
    if a.track.source == b.track.source {
        return false;
    }
    if a.duration_ms.abs_diff(b.duration_ms) > DURATION_SLACK_MS {
        return false;
    }
    match (&a.isrc, &b.isrc) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
        _ => true,
    }
}

/// What the matcher compares exactly: artist, title, album and the explicit
/// flag. `None` for a song with no title or no artist (never matched).
fn song_key(s: &SongFacts) -> Option<String> {
    let title = title_key(&s.title);
    let artist = artist_key(s.artist.as_deref().unwrap_or(""));
    if title.is_empty() || artist.is_empty() {
        return None;
    }
    let album = title_key(s.album.as_deref().unwrap_or(""));
    Some(format!(
        "{artist}\u{1}{title}\u{1}{album}\u{1}{}",
        s.explicit
    ))
}

struct Sets {
    parent: Vec<usize>,
}

impl Sets {
    fn new(n: usize) -> Sets {
        Sets {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[b.max(a)] = a.min(b);
        }
    }
}

/// Group the songs. `priority` lists sources from first to last (an unlisted
/// source sorts after them); it picks each group's representative. Manual
/// links name songs that may not be in `songs` (another device's account):
/// those join the group too, so a queue item can find its way to a copy.
pub fn group_songs(
    songs: &[SongFacts],
    manual: &[ManualLink],
    priority: &[SourceId],
) -> Vec<Group> {
    // Every song the matcher or a link names, indexed.
    let mut index: HashMap<TrackRef, usize> = HashMap::new();
    let mut refs: Vec<TrackRef> = vec![];
    let mut facts: Vec<Option<&SongFacts>> = vec![];
    fn slot(r: &TrackRef, index: &mut HashMap<TrackRef, usize>, refs: &mut Vec<TrackRef>) -> usize {
        *index.entry(r.clone()).or_insert_with(|| {
            refs.push(r.clone());
            refs.len() - 1
        })
    }
    for s in songs {
        let i = slot(&s.track, &mut index, &mut refs);
        facts.resize(refs.len(), None);
        facts[i] = Some(s);
    }
    let mut not_same: HashSet<(usize, usize)> = HashSet::new();
    let mut same: Vec<(usize, usize)> = vec![];
    for l in manual {
        if l.a == l.b {
            continue;
        }
        let a = slot(&l.a, &mut index, &mut refs);
        let b = slot(&l.b, &mut index, &mut refs);
        if l.same {
            same.push((a, b));
        } else {
            not_same.insert((a.min(b), a.max(b)));
        }
    }
    facts.resize(refs.len(), None);
    let n = refs.len();
    let mut sets = Sets::new(n);
    let mut manual_member = vec![false; n];
    for &(a, b) in &same {
        sets.union(a, b);
        manual_member[a] = true;
        manual_member[b] = true;
    }

    // Candidate pairs from the matcher, closest durations first.
    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in facts.iter().enumerate() {
        if let Some(key) = f.and_then(song_key) {
            buckets.entry(key).or_default().push(i);
        }
    }
    let mut pairs: Vec<(u32, usize, usize)> = vec![];
    for members in buckets.values() {
        for (x, &i) in members.iter().enumerate() {
            for &j in &members[x + 1..] {
                let (fi, fj) = (facts[i].unwrap(), facts[j].unwrap());
                if compatible(fi, fj) {
                    pairs.push((fi.duration_ms.abs_diff(fj.duration_ms), i.min(j), i.max(j)));
                }
            }
        }
    }
    pairs.sort();

    // Members of each set, kept current as sets merge (for the checks below).
    let mut members: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = sets.find(i);
        members.entry(root).or_default().push(i);
    }
    for (_, i, j) in pairs {
        let (ri, rj) = (sets.find(i), sets.find(j));
        if ri == rj {
            continue;
        }
        let (mi, mj) = (&members[&ri], &members[&rj]);
        // One copy per source, and never across a "not the same".
        let sources: HashSet<&str> = mi.iter().map(|&k| refs[k].source.as_str()).collect();
        if mj
            .iter()
            .any(|&k| sources.contains(refs[k].source.as_str()))
        {
            continue;
        }
        if mi
            .iter()
            .any(|&x| mj.iter().any(|&y| not_same.contains(&(x.min(y), x.max(y)))))
        {
            continue;
        }
        sets.union(i, j);
        let root = sets.find(i);
        let mut all = members.remove(&ri).unwrap_or_default();
        all.extend(members.remove(&rj).unwrap_or_default());
        members.insert(root, all);
    }

    let rank = |r: &TrackRef| {
        priority
            .iter()
            .position(|s| s == &r.source)
            .unwrap_or(priority.len())
    };
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for list in members.values() {
        if list.len() < 2 {
            continue;
        }
        let mut list: Vec<usize> = list.clone();
        list.sort_by(|&a, &b| (rank(&refs[a]), &refs[a]).cmp(&(rank(&refs[b]), &refs[b])));
        let id = list
            .iter()
            .map(|&k| ref_key(&refs[k]))
            .min()
            .unwrap_or_default();
        groups.insert(
            id.clone(),
            Group {
                id,
                members: list
                    .iter()
                    .map(|&k| Member {
                        track: refs[k].clone(),
                        manual: manual_member[k],
                    })
                    .collect(),
            },
        );
    }
    groups.into_values().collect()
}

/// Group albums or artists across sources: the same normalised name and
/// artist (albums), and the same year when both have one. One copy per
/// source; the highest-priority source's copy first.
pub fn group_entities(items: &[EntityFacts], priority: &[SourceId]) -> Vec<EntityGroup> {
    let mut buckets: BTreeMap<String, Vec<&EntityFacts>> = BTreeMap::new();
    for e in items {
        let name = title_key(&e.name);
        if name.is_empty() {
            continue;
        }
        let artist = e.artist.as_deref().map(artist_key).unwrap_or_default();
        buckets
            .entry(format!("{artist}\u{1}{name}"))
            .or_default()
            .push(e);
    }
    let rank = |s: &str| {
        priority
            .iter()
            .position(|p| p == s)
            .unwrap_or(priority.len())
    };
    let mut out = vec![];
    for mut list in buckets.into_values() {
        if list.len() < 2 {
            continue;
        }
        list.sort_by(|a, b| {
            (rank(&a.source), &a.source, &a.id).cmp(&(rank(&b.source), &b.source, &b.id))
        });
        // Greedy: each entry joins the first group it fits, else starts one.
        let mut groups: Vec<Vec<&EntityFacts>> = vec![];
        for e in list {
            let fits = groups.iter_mut().find(|g| {
                g.iter().all(|m| {
                    m.source != e.source
                        && match (m.year, e.year) {
                            (Some(x), Some(y)) => x == y,
                            _ => true,
                        }
                })
            });
            match fits {
                Some(g) => g.push(e),
                None => groups.push(vec![e]),
            }
        }
        for g in groups.into_iter().filter(|g| g.len() > 1) {
            let id = g
                .iter()
                .map(|m| format!("{}\u{1}{}", m.source, m.id))
                .min()
                .unwrap_or_default();
            out.push(EntityGroup {
                id,
                members: g.iter().map(|m| (m.source.clone(), m.id.clone())).collect(),
            });
        }
    }
    out
}

#[cfg(test)]
#[path = "links_tests.rs"]
mod tests;
