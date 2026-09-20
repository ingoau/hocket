//! Autoplay: an ordered provider chain that tops up the queue when it runs
//! out, and feeds the Related tab.
//!
//! Entry points for the actor:
//!
//! | Need | Call |
//! |---|---|
//! | Construct once, keep in the actor | [`AutoplayEngine::new`] `(AutoplaySettings, seed)` then [`AutoplayEngine::set_settings`] on change |
//! | Queue is running dry | [`AutoplayEngine::next_batch`] `(&dyn AutoplaySource, &SeedInput, want) → Vec<AutoplayPick>`; turn each into a queue item with [`AutoplayPick::to_queue_item`] |
//! | Related tab for one track | [`related`] `(&dyn AutoplaySource, &TrackSummary, count) → Vec<api::RelatedTrack>` |
//! | Persist / restore the recently-autoplayed exclusion set | [`AutoplayEngine::exclusion_ids`] / [`AutoplayEngine::restore_exclusion`] |
//! | Defaults | [`default_settings`], [`default_overrides`] |
//!
//! The network lives behind [`AutoplaySource`], implemented by the actor
//! over the subsonic client (+ the mirror for saved filters); tests use a
//! fake. Every pick carries `provider`, `reason` and `score` so the UI can
//! say why something was queued.

use std::collections::{HashSet, VecDeque};

use futures::future::BoxFuture;
use rand::seq::SliceRandom;
use rand::SeedableRng;

use crate::api::{
    AutoplayProvider, AutoplaySettings, ContextKind, FilterId, QueueItem, QueueKey, QueueSource,
    RelatedTrack, TrackId, TrackSummary,
};

/// A track with a sonic similarity score (`getSonicSimilarTracks`: 1.0 is
/// the same song, 0.0 the most different).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredTrack {
    pub track: TrackSummary,
    pub similarity: f64,
}

#[derive(Debug, thiserror::Error)]
pub enum AutoplayError {
    #[error("provider not supported by this server")]
    Unsupported,
    #[error("network: {0}")]
    Network(String),
    #[error("server: {0}")]
    Server(String),
    #[error("{0}")]
    Other(String),
}

/// What the chain needs from the outside world. Each call maps to one
/// endpoint; the implementation may serve from the metadata cache.
pub trait AutoplaySource: Send + Sync {
    /// `getSonicSimilarTracks(id, count)`. `Err(Unsupported)` when the server lacks the extension.
    fn sonic_similar<'a>(
        &'a self,
        track_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<ScoredTrack>, AutoplayError>>;
    /// `getSimilarSongs2(id, count)`.
    fn similar_songs<'a>(
        &'a self,
        track_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>>;
    /// `getTopSongs(artist, count)` — by artist *name* (Subsonic's contract), id given when known.
    fn top_songs<'a>(
        &'a self,
        artist_name: &'a str,
        artist_id: Option<&'a str>,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>>;
    /// `getRandomSongs(size)`.
    fn random_songs<'a>(
        &'a self,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>>;
    /// Evaluate a saved filter against the mirror and return up to `count` tracks (any order).
    fn filter_tracks<'a>(
        &'a self,
        filter_id: &'a str,
        count: u32,
    ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>>;
}

/// The session state autoplay seeds from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeedInput {
    /// Recently played / current tracks, **newest first**. The engine uses
    /// the first `seed_window` of them.
    pub recent: Vec<TrackSummary>,
    /// Kind of the current context, for per-context chain overrides.
    pub context: Option<ContextClass>,
    /// Everything already in the session (history, current, queue): never
    /// picked again.
    pub exclude: Vec<TrackId>,
}

/// Context classes that can override the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextClass {
    Album,
    Artist,
    Playlist,
    Genre,
    Filter,
    AdHoc,
    Autoplay,
}

impl ContextClass {
    pub fn of(kind: &ContextKind) -> ContextClass {
        match kind {
            ContextKind::Album { .. } => ContextClass::Album,
            ContextKind::Artist { .. } => ContextClass::Artist,
            ContextKind::Playlist { .. } => ContextClass::Playlist,
            ContextKind::Genre { .. } => ContextClass::Genre,
            ContextKind::Filter { .. } => ContextClass::Filter,
            ContextKind::AdHoc { .. } => ContextClass::AdHoc,
            ContextKind::Autoplay => ContextClass::Autoplay,
        }
    }
}

/// One chain override: for this context class, use this provider order.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextOverride {
    pub context: ContextClass,
    pub chain: Vec<AutoplayProvider>,
}

/// A picked track with the explanation the UI shows.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoplayPick {
    pub track: TrackSummary,
    pub provider: AutoplayProvider,
    pub reason: String,
    pub score: Option<f64>,
}

impl AutoplayPick {
    pub fn to_queue_item(&self, key: QueueKey) -> QueueItem {
        QueueItem {
            key,
            track_id: self.track.id.clone(),
            source: QueueSource::Autoplay {
                provider: self.provider,
                reason: self.reason.clone(),
                score: self.score,
            },
            unavailable: false,
        }
    }

    pub fn to_related(&self) -> RelatedTrack {
        RelatedTrack {
            track: self.track.clone(),
            provider: self.provider,
            score: self.score,
            reason: self.reason.clone(),
        }
    }
}

/// The full chain in the design's order.
pub const DEFAULT_CHAIN: [AutoplayProvider; 5] = [
    AutoplayProvider::SonicSimilarity,
    AutoplayProvider::SimilarSongs,
    AutoplayProvider::TopSongs,
    AutoplayProvider::Random,
    AutoplayProvider::SavedFilter,
];

/// Seed window of 5, sonic threshold 0.5, exclusion of the last 100 picks.
pub fn default_settings() -> AutoplaySettings {
    AutoplaySettings {
        chain: DEFAULT_CHAIN.to_vec(),
        seed_window: 5,
        min_similarity: Some(0.5),
        exclusion_window: 100,
        filter_id: None,
    }
}

/// Album contexts prefer Navidrome's similar-songs (album-mates and
/// collaborators) before sonic drift; artist contexts lead with top songs.
pub fn default_overrides() -> Vec<ContextOverride> {
    use AutoplayProvider::*;
    vec![
        ContextOverride {
            context: ContextClass::Album,
            chain: vec![SimilarSongs, SonicSimilarity, TopSongs, Random, SavedFilter],
        },
        ContextOverride {
            context: ContextClass::Artist,
            chain: vec![TopSongs, SimilarSongs, SonicSimilarity, Random, SavedFilter],
        },
    ]
}

/// Holds settings, the exclusion set and a deterministic RNG.
pub struct AutoplayEngine {
    settings: AutoplaySettings,
    overrides: Vec<ContextOverride>,
    recently_autoplayed: VecDeque<TrackId>,
    rng: rand_chacha::ChaCha8Rng,
}

impl AutoplayEngine {
    pub fn new(settings: AutoplaySettings, rng_seed: u64) -> Self {
        Self {
            settings: sanitise(settings),
            overrides: default_overrides(),
            recently_autoplayed: VecDeque::new(),
            rng: rand_chacha::ChaCha8Rng::seed_from_u64(rng_seed),
        }
    }

    pub fn settings(&self) -> &AutoplaySettings {
        &self.settings
    }

    pub fn set_settings(&mut self, settings: AutoplaySettings) {
        self.settings = sanitise(settings);
        self.trim_exclusion();
    }

    pub fn overrides(&self) -> &[ContextOverride] {
        &self.overrides
    }

    pub fn set_overrides(&mut self, overrides: Vec<ContextOverride>) {
        self.overrides = overrides;
    }

    /// The chain that applies to a context.
    pub fn chain_for(&self, context: Option<ContextClass>) -> Vec<AutoplayProvider> {
        let chain = context
            .and_then(|c| self.overrides.iter().find(|o| o.context == c))
            .map(|o| o.chain.clone())
            .unwrap_or_else(|| self.settings.chain.clone());
        // Overrides only reorder; providers the user disabled stay disabled.
        chain
            .into_iter()
            .filter(|p| self.settings.chain.contains(p))
            .collect()
    }

    /// Recently autoplayed ids, oldest first (persist in `saved_state`).
    pub fn exclusion_ids(&self) -> Vec<TrackId> {
        self.recently_autoplayed.iter().cloned().collect()
    }

    pub fn restore_exclusion(&mut self, ids: Vec<TrackId>) {
        self.recently_autoplayed = ids.into_iter().collect();
        self.trim_exclusion();
    }

    fn remember(&mut self, id: &str) {
        self.recently_autoplayed.push_back(id.to_string());
        self.trim_exclusion();
    }

    fn trim_exclusion(&mut self) {
        while self.recently_autoplayed.len() > self.settings.exclusion_window as usize {
            self.recently_autoplayed.pop_front();
        }
    }

    /// Picks up to `want` tracks. Providers run in chain order; each one
    /// contributes what it can after dedupe, and the next is consulted only
    /// while the batch is short. Errors and empties fall through.
    pub async fn next_batch(
        &mut self,
        source: &dyn AutoplaySource,
        seeds: &SeedInput,
        want: u32,
    ) -> Vec<AutoplayPick> {
        let want = want as usize;
        let mut picks: Vec<AutoplayPick> = Vec::new();
        if want == 0 {
            return picks;
        }
        let mut seen: HashSet<TrackId> = seeds.exclude.iter().cloned().collect();
        seen.extend(seeds.recent.iter().map(|t| t.id.clone()));
        seen.extend(self.recently_autoplayed.iter().cloned());
        let window: Vec<&TrackSummary> = seeds
            .recent
            .iter()
            .take(self.settings.seed_window.max(1) as usize)
            .collect();
        // Over-fetch so dedupe against a long history still leaves enough.
        let fetch = (want as u32).saturating_mul(2).clamp(10, 100);

        for provider in self.chain_for(seeds.context) {
            if picks.len() >= want {
                break;
            }
            let candidates = match provider {
                AutoplayProvider::SonicSimilarity => self.sonic(source, &window, fetch).await,
                AutoplayProvider::SimilarSongs => Self::similar(source, &window, fetch).await,
                AutoplayProvider::TopSongs => Self::top(source, &window, fetch).await,
                AutoplayProvider::Random => Self::random(source, fetch).await,
                AutoplayProvider::SavedFilter => self.saved_filter(source, fetch).await,
            };
            for c in candidates {
                if picks.len() >= want {
                    break;
                }
                if seen.insert(c.track.id.clone()) {
                    picks.push(c);
                }
            }
        }
        for p in &picks {
            self.remember(&p.track.id);
        }
        picks
    }

    async fn sonic(
        &self,
        source: &dyn AutoplaySource,
        window: &[&TrackSummary],
        fetch: u32,
    ) -> Vec<AutoplayPick> {
        let threshold = self.settings.min_similarity.unwrap_or(0.0);
        let mut per_seed: Vec<Vec<AutoplayPick>> = Vec::new();
        for seed in window {
            match source.sonic_similar(&seed.id, fetch).await {
                Ok(list) => {
                    let mut items: Vec<AutoplayPick> = list
                        .into_iter()
                        .filter(|s| s.similarity >= threshold && s.track.id != seed.id)
                        .map(|s| AutoplayPick {
                            reason: format!("Sounds like {}", describe(seed)),
                            score: Some(s.similarity),
                            provider: AutoplayProvider::SonicSimilarity,
                            track: s.track,
                        })
                        .collect();
                    items.sort_by(|a, b| {
                        b.score
                            .partial_cmp(&a.score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    per_seed.push(items);
                }
                Err(AutoplayError::Unsupported) => {
                    tracing::debug!("sonic similarity unsupported; falling through");
                    return Vec::new();
                }
                Err(e) => {
                    tracing::warn!(error = %e, "sonic similarity failed for seed {}", seed.id)
                }
            }
        }
        interleave(per_seed)
    }

    async fn similar(
        source: &dyn AutoplaySource,
        window: &[&TrackSummary],
        fetch: u32,
    ) -> Vec<AutoplayPick> {
        let mut per_seed = Vec::new();
        for seed in window {
            match source.similar_songs(&seed.id, fetch).await {
                Ok(list) => per_seed.push(
                    list.into_iter()
                        .filter(|t| t.id != seed.id)
                        .map(|t| AutoplayPick {
                            track: t,
                            provider: AutoplayProvider::SimilarSongs,
                            reason: format!("Similar to {}", describe(seed)),
                            score: None,
                        })
                        .collect(),
                ),
                Err(e) => tracing::warn!(error = %e, "similar songs failed for seed {}", seed.id),
            }
        }
        interleave(per_seed)
    }

    async fn top(
        source: &dyn AutoplaySource,
        window: &[&TrackSummary],
        fetch: u32,
    ) -> Vec<AutoplayPick> {
        let mut artists: Vec<(&str, Option<&str>)> = Vec::new();
        for seed in window {
            if let Some(name) = seed.artist.as_deref().filter(|n| !n.trim().is_empty()) {
                if !artists.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
                    artists.push((name, seed.artist_id.as_deref()));
                }
            }
        }
        let mut per_artist = Vec::new();
        for (name, id) in artists {
            match source.top_songs(name, id, fetch).await {
                Ok(list) => per_artist.push(
                    list.into_iter()
                        .map(|t| AutoplayPick {
                            track: t,
                            provider: AutoplayProvider::TopSongs,
                            reason: format!("Top song by {name}"),
                            score: None,
                        })
                        .collect(),
                ),
                Err(e) => tracing::warn!(error = %e, "top songs failed for {name}"),
            }
        }
        interleave(per_artist)
    }

    async fn random(source: &dyn AutoplaySource, fetch: u32) -> Vec<AutoplayPick> {
        match source.random_songs(fetch).await {
            Ok(list) => list
                .into_iter()
                .map(|t| AutoplayPick {
                    track: t,
                    provider: AutoplayProvider::Random,
                    reason: "Random pick".into(),
                    score: None,
                })
                .collect(),
            Err(e) => {
                tracing::warn!(error = %e, "random songs failed");
                Vec::new()
            }
        }
    }

    async fn saved_filter(&mut self, source: &dyn AutoplaySource, fetch: u32) -> Vec<AutoplayPick> {
        let Some(filter_id) = self.settings.filter_id.clone() else {
            return Vec::new();
        };
        match source
            .filter_tracks(&filter_id, fetch.saturating_mul(5))
            .await
        {
            Ok(mut list) => {
                list.shuffle(&mut self.rng);
                list.truncate(fetch as usize);
                list.into_iter()
                    .map(|t| AutoplayPick {
                        track: t,
                        provider: AutoplayProvider::SavedFilter,
                        reason: "From your autoplay filter".into(),
                        score: None,
                    })
                    .collect()
            }
            Err(e) => {
                tracing::warn!(error = %e, "saved filter failed");
                Vec::new()
            }
        }
    }
}

/// Related tracks for one track: sonic matches (scored) first, then similar
/// songs, deduplicated. No exclusion set: the tab explains, it doesn't queue.
pub async fn related(
    source: &dyn AutoplaySource,
    track: &TrackSummary,
    count: u32,
) -> Vec<RelatedTrack> {
    let mut out: Vec<RelatedTrack> = Vec::new();
    let mut seen: HashSet<TrackId> = HashSet::from([track.id.clone()]);
    if let Ok(list) = source.sonic_similar(&track.id, count).await {
        let mut list = list;
        list.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for s in list {
            if out.len() as u32 >= count {
                break;
            }
            if seen.insert(s.track.id.clone()) {
                out.push(RelatedTrack {
                    track: s.track,
                    provider: AutoplayProvider::SonicSimilarity,
                    score: Some(s.similarity),
                    reason: format!("Sounds like {}", describe(track)),
                });
            }
        }
    }
    if (out.len() as u32) < count {
        if let Ok(list) = source.similar_songs(&track.id, count).await {
            for t in list {
                if out.len() as u32 >= count {
                    break;
                }
                if seen.insert(t.id.clone()) {
                    out.push(RelatedTrack {
                        track: t,
                        provider: AutoplayProvider::SimilarSongs,
                        score: None,
                        reason: format!("Similar to {}", describe(track)),
                    });
                }
            }
        }
    }
    out
}

fn describe(t: &TrackSummary) -> String {
    match &t.artist {
        Some(a) if !a.is_empty() => format!("{} – {}", a, t.title),
        _ => t.title.clone(),
    }
}

/// Round-robin merge so results drift across every seed rather than being
/// dominated by the newest.
fn interleave(mut lists: Vec<Vec<AutoplayPick>>) -> Vec<AutoplayPick> {
    let total: usize = lists.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(total);
    let mut queues: Vec<std::collections::vec_deque::VecDeque<AutoplayPick>> =
        lists.drain(..).map(VecDeque::from).collect();
    while out.len() < total {
        for q in &mut queues {
            if let Some(p) = q.pop_front() {
                out.push(p);
            }
        }
    }
    out
}

fn sanitise(mut s: AutoplaySettings) -> AutoplaySettings {
    if s.chain.is_empty() {
        s.chain = DEFAULT_CHAIN.to_vec();
    }
    let mut dedup: Vec<AutoplayProvider> = Vec::new();
    for p in s.chain {
        if !dedup.contains(&p) {
            dedup.push(p);
        }
    }
    s.chain = dedup;
    s.seed_window = s.seed_window.clamp(1, 50);
    s.exclusion_window = s.exclusion_window.min(5000);
    s.min_similarity = s.min_similarity.map(|m| m.clamp(0.0, 1.0));
    s.filter_id = s.filter_id.filter(|f: &FilterId| !f.is_empty());
    s
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use parking_lot::Mutex;

    use super::*;

    fn ts(id: &str, artist: &str) -> TrackSummary {
        TrackSummary {
            id: id.into(),
            server_id: "s".into(),
            title: format!("T{id}"),
            artist: Some(artist.into()),
            artist_id: Some(format!("a-{artist}")),
            ..Default::default()
        }
    }

    #[derive(Default)]
    struct Fake {
        sonic: HashMap<String, Vec<ScoredTrack>>,
        sonic_unsupported: bool,
        similar: HashMap<String, Vec<TrackSummary>>,
        top: HashMap<String, Vec<TrackSummary>>,
        random: Vec<TrackSummary>,
        random_fails: bool,
        filter: HashMap<String, Vec<TrackSummary>>,
        calls: Mutex<Vec<String>>,
    }

    impl AutoplaySource for Fake {
        fn sonic_similar<'a>(
            &'a self,
            track_id: &'a str,
            count: u32,
        ) -> BoxFuture<'a, Result<Vec<ScoredTrack>, AutoplayError>> {
            Box::pin(async move {
                self.calls.lock().push(format!("sonic:{track_id}:{count}"));
                if self.sonic_unsupported {
                    return Err(AutoplayError::Unsupported);
                }
                Ok(self.sonic.get(track_id).cloned().unwrap_or_default())
            })
        }
        fn similar_songs<'a>(
            &'a self,
            track_id: &'a str,
            _count: u32,
        ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
            Box::pin(async move {
                self.calls.lock().push(format!("similar:{track_id}"));
                Ok(self.similar.get(track_id).cloned().unwrap_or_default())
            })
        }
        fn top_songs<'a>(
            &'a self,
            artist_name: &'a str,
            _artist_id: Option<&'a str>,
            _count: u32,
        ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
            Box::pin(async move {
                self.calls.lock().push(format!("top:{artist_name}"));
                Ok(self.top.get(artist_name).cloned().unwrap_or_default())
            })
        }
        fn random_songs<'a>(
            &'a self,
            _count: u32,
        ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
            Box::pin(async move {
                self.calls.lock().push("random".into());
                if self.random_fails {
                    Err(AutoplayError::Network("down".into()))
                } else {
                    Ok(self.random.clone())
                }
            })
        }
        fn filter_tracks<'a>(
            &'a self,
            filter_id: &'a str,
            _count: u32,
        ) -> BoxFuture<'a, Result<Vec<TrackSummary>, AutoplayError>> {
            Box::pin(async move {
                self.calls.lock().push(format!("filter:{filter_id}"));
                Ok(self.filter.get(filter_id).cloned().unwrap_or_default())
            })
        }
    }

    fn scored(id: &str, s: f64) -> ScoredTrack {
        ScoredTrack {
            track: ts(id, "X"),
            similarity: s,
        }
    }

    #[tokio::test]
    async fn sonic_first_with_threshold_and_explanations() {
        let fake = Fake {
            sonic: [(
                "seed1".to_string(),
                vec![
                    scored("lo", 0.2),
                    scored("hi", 0.9),
                    scored("mid", 0.6),
                    scored("seed1", 1.0),
                ],
            )]
            .into(),
            ..Default::default()
        };
        let mut engine = AutoplayEngine::new(default_settings(), 1);
        let seeds = SeedInput {
            recent: vec![ts("seed1", "A")],
            ..Default::default()
        };
        let picks = engine.next_batch(&fake, &seeds, 5).await;
        let ids: Vec<&str> = picks.iter().map(|p| p.track.id.as_str()).collect();
        assert_eq!(ids, vec!["hi", "mid"]); // sorted by score, below-threshold and self dropped
        assert_eq!(picks[0].provider, AutoplayProvider::SonicSimilarity);
        assert_eq!(picks[0].score, Some(0.9));
        assert_eq!(picks[0].reason, "Sounds like A – Tseed1");
        let item = picks[0].to_queue_item("k1".into());
        assert_eq!(
            item.source,
            QueueSource::Autoplay {
                provider: AutoplayProvider::SonicSimilarity,
                reason: "Sounds like A – Tseed1".into(),
                score: Some(0.9)
            }
        );
        assert_eq!(picks[1].to_related().track.id, "mid");
        // Falls through to the rest of the chain because sonic was short: similar, top, random, (no filter id).
        let calls = fake.calls.lock().clone();
        assert_eq!(
            calls,
            vec!["sonic:seed1:10", "similar:seed1", "top:A", "random"]
        );
    }

    #[tokio::test]
    async fn seed_window_interleaves_and_dedupes_against_session() {
        let fake = Fake {
            sonic_unsupported: true,
            similar: [
                (
                    "new".to_string(),
                    vec![ts("n1", "N"), ts("shared", "N"), ts("n2", "N")],
                ),
                (
                    "old".to_string(),
                    vec![ts("o1", "O"), ts("shared", "O"), ts("inqueue", "O")],
                ),
                ("ancient".to_string(), vec![ts("z1", "Z")]),
            ]
            .into(),
            ..Default::default()
        };
        let mut settings = default_settings();
        settings.seed_window = 2;
        let mut engine = AutoplayEngine::new(settings, 1);
        let seeds = SeedInput {
            recent: vec![ts("new", "N"), ts("old", "O"), ts("ancient", "Z")],
            context: None,
            exclude: vec!["inqueue".into()],
        };
        let picks = engine.next_batch(&fake, &seeds, 10).await;
        let ids: Vec<&str> = picks.iter().map(|p| p.track.id.as_str()).collect();
        assert_eq!(ids, vec!["n1", "o1", "shared", "n2"]);
        let calls = fake.calls.lock().clone();
        assert!(!calls.iter().any(|c| c.contains("ancient")), "{calls:?}");
        assert_eq!(calls[0], "sonic:new:20"); // unsupported → abandoned after the first seed
        assert!(!calls.contains(&"sonic:old:20".to_string()));
        // Exclusion set now holds the picks; a second batch never repeats them.
        assert_eq!(engine.exclusion_ids(), vec!["n1", "o1", "shared", "n2"]);
        let again = engine.next_batch(&fake, &seeds, 10).await;
        assert!(again.is_empty());
    }

    #[tokio::test]
    async fn exclusion_window_is_bounded_and_restorable() {
        let random: Vec<TrackSummary> = (0..30).map(|i| ts(&format!("r{i}"), "R")).collect();
        let fake = Fake {
            sonic_unsupported: true,
            random,
            ..Default::default()
        };
        let mut settings = default_settings();
        settings.exclusion_window = 4;
        settings.chain = vec![AutoplayProvider::Random];
        let mut engine = AutoplayEngine::new(settings, 7);
        let seeds = SeedInput::default();
        let a = engine.next_batch(&fake, &seeds, 3).await;
        assert_eq!(a.len(), 3);
        let b = engine.next_batch(&fake, &seeds, 3).await;
        assert_eq!(
            b.iter().map(|p| p.track.id.as_str()).collect::<Vec<_>>(),
            vec!["r3", "r4", "r5"]
        );
        assert_eq!(engine.exclusion_ids(), vec!["r2", "r3", "r4", "r5"]);
        let mut fresh = AutoplayEngine::new(engine.settings().clone(), 7);
        fresh.restore_exclusion(engine.exclusion_ids());
        let c = fresh.next_batch(&fake, &seeds, 1).await;
        assert_eq!(c[0].track.id, "r0");
        assert_eq!(c[0].reason, "Random pick");
    }

    #[tokio::test]
    async fn context_overrides_reorder_but_respect_disabled_providers() {
        let fake = Fake {
            sonic: [("s".to_string(), vec![scored("sonic1", 0.9)])].into(),
            similar: [("s".to_string(), vec![ts("sim1", "A")])].into(),
            top: [("A".to_string(), vec![ts("top1", "A")])].into(),
            ..Default::default()
        };
        let mut engine = AutoplayEngine::new(default_settings(), 1);
        let seeds = |ctx| SeedInput {
            recent: vec![ts("s", "A")],
            context: ctx,
            exclude: vec![],
        };
        assert_eq!(
            engine.next_batch(&fake, &seeds(None), 1).await[0].track.id,
            "sonic1"
        );
        assert_eq!(
            engine
                .next_batch(&fake, &seeds(Some(ContextClass::Album)), 1)
                .await[0]
                .track
                .id,
            "sim1"
        );
        assert_eq!(
            engine
                .next_batch(&fake, &seeds(Some(ContextClass::Artist)), 1)
                .await[0]
                .track
                .id,
            "top1"
        );
        // Disable similar songs entirely: the album override skips it.
        let mut s = default_settings();
        s.chain = vec![AutoplayProvider::SonicSimilarity, AutoplayProvider::Random];
        let mut engine = AutoplayEngine::new(s, 1);
        assert_eq!(
            engine.chain_for(Some(ContextClass::Album)),
            vec![AutoplayProvider::SonicSimilarity, AutoplayProvider::Random]
        );
        assert_eq!(
            engine
                .next_batch(&fake, &seeds(Some(ContextClass::Album)), 1)
                .await[0]
                .track
                .id,
            "sonic1"
        );
        assert_eq!(
            ContextClass::of(&ContextKind::Genre { name: "x".into() }),
            ContextClass::Genre
        );
    }

    #[tokio::test]
    async fn failures_fall_through_to_saved_filter() {
        let fake = Fake {
            sonic_unsupported: true,
            random_fails: true,
            filter: [(
                "f1".to_string(),
                (0..8).map(|i| ts(&format!("f{i}"), "F")).collect(),
            )]
            .into(),
            ..Default::default()
        };
        let mut settings = default_settings();
        settings.filter_id = Some("f1".into());
        let mut engine = AutoplayEngine::new(settings, 3);
        let seeds = SeedInput {
            recent: vec![ts("s", "A")],
            ..Default::default()
        };
        let picks = engine.next_batch(&fake, &seeds, 3).await;
        assert_eq!(picks.len(), 3);
        assert!(picks
            .iter()
            .all(|p| p.provider == AutoplayProvider::SavedFilter));
        assert_eq!(picks[0].reason, "From your autoplay filter");
        let ids: Vec<&str> = picks.iter().map(|p| p.track.id.as_str()).collect();
        // Shuffled deterministically from the seed; distinct.
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 3);
        let calls = fake.calls.lock().clone();
        assert_eq!(
            calls,
            vec!["sonic:s:10", "similar:s", "top:A", "random", "filter:f1"]
        );
        assert!(engine.next_batch(&fake, &seeds, 0).await.is_empty());
    }

    #[tokio::test]
    async fn related_merges_sonic_and_similar() {
        let fake = Fake {
            sonic: [(
                "t".to_string(),
                vec![scored("a", 0.3), scored("b", 0.8), scored("t", 1.0)],
            )]
            .into(),
            similar: [(
                "t".to_string(),
                vec![ts("b", "X"), ts("c", "X"), ts("d", "X")],
            )]
            .into(),
            ..Default::default()
        };
        let r = related(&fake, &ts("t", "Me"), 3).await;
        let got: Vec<(&str, AutoplayProvider, Option<f64>)> = r
            .iter()
            .map(|x| (x.track.id.as_str(), x.provider, x.score))
            .collect();
        assert_eq!(
            got,
            vec![
                ("b", AutoplayProvider::SonicSimilarity, Some(0.8)),
                ("a", AutoplayProvider::SonicSimilarity, Some(0.3)),
                ("c", AutoplayProvider::SimilarSongs, None)
            ]
        );
        assert_eq!(r[0].reason, "Sounds like Me – Tt");
        let none = related(&Fake::default(), &ts("t", "Me"), 3).await;
        assert!(none.is_empty());
    }

    #[test]
    fn settings_are_sanitised() {
        let s = sanitise(AutoplaySettings {
            chain: vec![],
            seed_window: 0,
            min_similarity: Some(7.0),
            exclusion_window: 1_000_000,
            filter_id: Some(String::new()),
        });
        assert_eq!(s.chain, DEFAULT_CHAIN.to_vec());
        assert_eq!(s.seed_window, 1);
        assert_eq!(s.min_similarity, Some(1.0));
        assert_eq!(s.exclusion_window, 5000);
        assert_eq!(s.filter_id, None);
        let s = sanitise(AutoplaySettings {
            chain: vec![AutoplayProvider::Random, AutoplayProvider::Random],
            ..default_settings()
        });
        assert_eq!(s.chain, vec![AutoplayProvider::Random]);
        assert_eq!(default_settings().seed_window, 5);
        assert_eq!(default_settings().exclusion_window, 100);
    }
}
