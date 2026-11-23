//! Pages of a live catalogue (YouTube Music): albums, artists and songs
//! outside the library are answered from the `catalogue` table, fetched
//! from the provider when it has nothing yet, and kept for next time.

use std::sync::Arc;

use tokio::sync::oneshot;

use crate::api::*;
use crate::core::actor::Actor;
use crate::provider::Provider;

/// What an album or artist query wants back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CatalogueWant {
    AlbumDetail,
    AlbumTracks,
    ArtistDetail,
    ArtistAlbums,
    ArtistTopSongs,
}

impl Actor {
    /// The provider of a live catalogue whose ids look like `id`.
    fn catalogue_provider(&self, id: &str) -> Option<Arc<dyn Provider>> {
        let ytm = id.starts_with("MPRE")
            || id.starts_with("UC")
            || id.starts_with("FEmusic_library_privately_owned");
        if !ytm {
            return None;
        }
        self.provider_for(crate::ytm::SOURCE)
    }

    /// Whether an album or artist query should go to the catalogue.
    pub(crate) fn is_catalogue_id(&self, id: &str) -> bool {
        self.catalogue_provider(id).is_some()
            || self.db.catalogue_album(id).ok().flatten().is_some()
            || self.db.catalogue_artist(id).ok().flatten().is_some()
    }

    fn catalogue_answer(&self, id: &str, want: CatalogueWant) -> Option<QueryResult> {
        match want {
            CatalogueWant::AlbumDetail | CatalogueWant::AlbumTracks => {
                let a = self.db.catalogue_album(id).ok().flatten()?;
                if a.tracks.is_empty() {
                    return None;
                }
                Some(match want {
                    CatalogueWant::AlbumDetail => QueryResult::AlbumDetail(Some(a.album)),
                    _ => QueryResult::TrackList(
                        self.db.catalogue_album_tracks(id).unwrap_or_default(),
                    ),
                })
            }
            CatalogueWant::ArtistDetail | CatalogueWant::ArtistAlbums => {
                let a = self.db.catalogue_artist(id).ok().flatten()?;
                Some(match want {
                    CatalogueWant::ArtistDetail => QueryResult::ArtistDetail(Some(a.artist)),
                    _ => QueryResult::Albums(AlbumPage {
                        total: a.albums.len() as u32,
                        items: a.albums,
                        offset: 0,
                    }),
                })
            }
            CatalogueWant::ArtistTopSongs => None,
        }
    }

    fn catalogue_empty(want: CatalogueWant) -> QueryResult {
        match want {
            CatalogueWant::AlbumDetail => QueryResult::AlbumDetail(None),
            CatalogueWant::ArtistDetail => QueryResult::ArtistDetail(None),
            CatalogueWant::ArtistAlbums => QueryResult::Albums(AlbumPage {
                items: vec![],
                offset: 0,
                total: 0,
            }),
            CatalogueWant::AlbumTracks | CatalogueWant::ArtistTopSongs => {
                QueryResult::TrackList(vec![])
            }
        }
    }

    /// Answer from the catalogue, else fetch the page live.
    pub(crate) fn catalogue_query(
        &mut self,
        id: String,
        want: CatalogueWant,
        count: u32,
        reply: oneshot::Sender<QueryResult>,
    ) {
        if let Some(a) = self.catalogue_answer(&id, want) {
            let _ = reply.send(a);
            return;
        }
        let Some(provider) = self.catalogue_provider(&id).filter(|_| !self.is_offline()) else {
            let _ = reply.send(Self::catalogue_empty(want));
            return;
        };
        let db = self.db.clone();
        let clock = self.clock.clone();
        self.spawn(async move {
            let now = clock.now_ms();
            let answer = match want {
                CatalogueWant::AlbumDetail | CatalogueWant::AlbumTracks => {
                    match provider.album(&id).await {
                        Ok((album, tracks)) => {
                            let _ = db.catalogue_put_album(&album, &tracks, now);
                            Some(match want {
                                CatalogueWant::AlbumDetail => QueryResult::AlbumDetail(Some(album)),
                                _ => QueryResult::TrackList(with_mirror_state(&db, tracks)),
                            })
                        }
                        Err(e) => {
                            tracing::debug!(target: "hocket_core", error = %e, "catalogue album");
                            None
                        }
                    }
                }
                CatalogueWant::ArtistDetail | CatalogueWant::ArtistAlbums => {
                    match provider.artist(&id).await {
                        Ok((artist, albums)) => {
                            let _ = db.catalogue_put_artist(&artist, &albums, now);
                            Some(match want {
                                CatalogueWant::ArtistDetail => {
                                    QueryResult::ArtistDetail(Some(artist))
                                }
                                _ => QueryResult::Albums(AlbumPage {
                                    total: albums.len() as u32,
                                    items: albums,
                                    offset: 0,
                                }),
                            })
                        }
                        Err(e) => {
                            tracing::debug!(target: "hocket_core", error = %e, "catalogue artist");
                            None
                        }
                    }
                }
                CatalogueWant::ArtistTopSongs => match provider.artist_top_tracks(&id).await {
                    Ok(mut tracks) => {
                        let _ = db.catalogue_put_tracks(&tracks, now);
                        tracks.truncate(count.max(1) as usize);
                        Some(QueryResult::TrackList(with_mirror_state(&db, tracks)))
                    }
                    Err(_) => None,
                },
            };
            let _ = reply.send(answer.unwrap_or_else(|| Self::catalogue_empty(want)));
        });
    }
}

/// Catalogue tracks with what the mirror knows of them (loved, offline).
fn with_mirror_state(db: &crate::db::Db, tracks: Vec<Track>) -> Vec<Track> {
    tracks
        .into_iter()
        .map(|t| {
            let r = TrackRef::new(t.server_id.clone(), t.id.clone());
            match db.track_ref(&r) {
                Ok(Some(m)) if m.title == t.title || !m.title.is_empty() => Track {
                    loved: m.loved,
                    offline: m.offline,
                    play_count: m.play_count,
                    ..t
                },
                _ => t,
            }
        })
        .collect()
}
