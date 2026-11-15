//! Home's shelves (`docs/providers.md`, Phase 3): each provider supplies
//! some, and Hocket adds its own Continue listening. Home's section list
//! (`home.sections`, synced) orders and hides them by id.
//!
//! An account the core mirrors (Navidrome) gets its shelves from the mirror,
//! so Home never waits on the network: Recently added, Most played and
//! Recently played, the lists Home showed before providers. A provider
//! without a mirror answers `Provider::home_shelves` live (YouTube Music,
//! Phase 4).

use crate::api::{Album, AlbumResumePoint, HomeShelf, HomeShelfKind, SortOrder, Track, TrackRef};
use crate::core::actor::Actor;
use crate::db::queries::AlbumQuery;

/// How long a live home feed is shown before it is fetched again.
const LIVE_HOME_TTL_MS: f64 = 30.0 * 60_000.0;

/// How many items a shelf holds.
const SHELF_SIZE: u32 = 20;

impl Actor {
    pub(crate) fn home_shelves(&self) -> Vec<HomeShelf> {
        let mut out = vec![self.continue_listening()];
        let several = self.accounts.iter().count() > 1;
        for a in self.accounts.iter() {
            if !a.info.features.library_sync {
                continue;
            }
            let label = |title: &str| {
                if several && !a.info.name.is_empty() {
                    format!("{title} \u{b7} {}", a.info.name)
                } else {
                    title.to_string()
                }
            };
            let source = a.info.source.clone();
            let shelves = [
                (
                    "recentlyAdded",
                    "Recently added",
                    SortOrder::DateAdded,
                    true,
                ),
                ("mostPlayed", "Most played", SortOrder::PlayCount, false),
            ];
            for (id, title, sort, default_on) in shelves {
                let albums = self
                    .db
                    .albums(&AlbumQuery {
                        server_id: source.clone(),
                        artist_id: None,
                        genre: None,
                        sort,
                        descending: true,
                        page: Some(crate::api::Page {
                            offset: 0,
                            limit: SHELF_SIZE,
                        }),
                    })
                    .unwrap_or_default();
                out.push(HomeShelf {
                    id: format!("{}/{id}", a.info.id),
                    account_id: Some(a.info.id.clone()),
                    title: label(title),
                    kind: HomeShelfKind::Albums,
                    albums,
                    tracks: vec![],
                    playlists: vec![],
                    default_on,
                });
            }
            out.push(HomeShelf {
                id: format!("{}/recentlyPlayed", a.info.id),
                account_id: Some(a.info.id.clone()),
                title: label("Recently played"),
                kind: HomeShelfKind::Albums,
                albums: self
                    .db
                    .albums_recently_played(&source, SHELF_SIZE)
                    .unwrap_or_default(),
                tracks: vec![],
                playlists: vec![],
                default_on: false,
            });
        }
        out.extend(self.live_home_shelves());
        out
    }

    /// The shelves of providers that answer them live (YouTube Music's home
    /// feed), from the last answer; a stale or missing one is refreshed in
    /// the background and announced with `LibraryChanged {tables: ["home"]}`.
    fn live_home_shelves(&self) -> Vec<HomeShelf> {
        let now = self.now();
        let mut out = vec![];
        let mut seen = std::collections::HashSet::new();
        for a in self.accounts.iter() {
            let Some(p) = a.provider.clone() else {
                continue;
            };
            if !a.info.features.home_shelves || !seen.insert(a.info.source.clone()) {
                continue;
            }
            let cached = self.live_home.lock().get(&a.info.id).cloned();
            let stale = cached
                .as_ref()
                .is_none_or(|(at, _)| now - at > LIVE_HOME_TTL_MS);
            if let Some((_, shelves)) = cached {
                out.extend(shelves);
            }
            if stale && self.live_home_fetching.lock().insert(a.info.id.clone()) {
                let account = a.info.id.clone();
                let cache = self.live_home.clone();
                let fetching = self.live_home_fetching.clone();
                let db = self.db.clone();
                let tx = self.tx.clone();
                self.rt.spawn(async move {
                    let got = p.home_shelves().await;
                    fetching.lock().remove(&account);
                    let Ok(shelves) = got else { return };
                    let mut list = vec![];
                    for s in shelves {
                        let _ = db.catalogue_put_tracks(&s.tracks, now);
                        let _ = db.catalogue_put_albums(&s.albums, now);
                        let kind = if !s.tracks.is_empty() {
                            HomeShelfKind::Tracks
                        } else if !s.albums.is_empty() {
                            HomeShelfKind::Albums
                        } else {
                            HomeShelfKind::Playlists
                        };
                        list.push(HomeShelf {
                            id: format!("{account}/{}", s.id),
                            account_id: Some(account.clone()),
                            title: s.title,
                            kind,
                            albums: s.albums,
                            tracks: s.tracks,
                            playlists: s.playlists,
                            default_on: true,
                        });
                    }
                    cache.lock().insert(account.clone(), (now, list));
                    let _ = tx.send(crate::core::ActorMsg::Internal(
                        crate::core::Internal::LiveHomeFetched {
                            account_id: account,
                        },
                    ));
                });
            }
        }
        out
    }

    pub(crate) fn on_live_home_fetched(&mut self, account_id: String) {
        self.emit(crate::api::Event::LibraryChanged {
            server_id: account_id,
            tables: vec!["home".into()],
            ids: vec![],
        });
    }

    /// Albums left part way (album resume), newest first; then albums of
    /// the latest plays.
    fn continue_listening(&self) -> HomeShelf {
        let mut albums: Vec<Album> = vec![];
        let mut points: Vec<AlbumResumePoint> = self.resume_points();
        points.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        for p in points {
            if let Some(a) = self.album_for(&p.source, &p.album_id) {
                if !albums
                    .iter()
                    .any(|x| x.id == a.id && x.server_id == a.server_id)
                {
                    albums.push(a);
                }
            }
        }
        for h in self.db.recently_played(SHELF_SIZE * 2).unwrap_or_default() {
            if albums.len() >= SHELF_SIZE as usize {
                break;
            }
            let Some(album_id) = h.track.album_id.clone() else {
                continue;
            };
            if let Some(a) = self.album_for(&h.track.server_id, &album_id) {
                if !albums
                    .iter()
                    .any(|x| x.id == a.id && x.server_id == a.server_id)
                {
                    albums.push(a);
                }
            }
        }
        HomeShelf {
            id: "hocket/continueListening".into(),
            account_id: None,
            title: "Continue listening".into(),
            kind: HomeShelfKind::Albums,
            albums,
            tracks: vec![],
            playlists: vec![],
            default_on: true,
        }
    }

    fn album_for(&self, source: &str, id: &str) -> Option<Album> {
        self.db
            .album(id)
            .ok()
            .flatten()
            .filter(|a| source.is_empty() || a.server_id == source)
    }

    /// Songs "Link to…" can offer: other accounts' songs matching `query`
    /// (the song's own title when empty), best first.
    pub(crate) fn link_candidates(&self, track: &TrackRef, query: &str) -> Vec<Track> {
        let query = if query.trim().is_empty() {
            self.db
                .track_ref(track)
                .ok()
                .flatten()
                .map(|t| t.title)
                .unwrap_or_default()
        } else {
            query.to_string()
        };
        let mut out = vec![];
        for a in self.accounts.iter() {
            if a.info.source == track.source {
                continue;
            }
            out.extend(
                self.db
                    .search_tracks(&a.info.source, &query, SHELF_SIZE)
                    .unwrap_or_default(),
            );
        }
        out
    }
}
