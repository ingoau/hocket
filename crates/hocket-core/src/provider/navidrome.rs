//! Navidrome (OpenSubsonic) behind the [`Provider`] trait.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use url::Url;

use super::{
    unsupported, HomeShelf, LibraryPage, Provider, ProviderFuture, ProviderWrite, ResolvedStream,
    StreamRequest,
};
use crate::api::{self, AccountKind, EpochMs, ProviderCapabilities};
use crate::subsonic::convert::{album_from_id3, artist_from_id3, track_from_child};
use crate::subsonic::transport::DownloadOutcome;
use crate::subsonic::types::StructuredLyrics;
use crate::subsonic::{Search3Page, StarTarget, StreamOptions, SubsonicApi};

/// A Navidrome account. Its id is its source: Navidrome ids are only valid
/// on the server that issued them.
pub struct NavidromeProvider {
    api: Arc<dyn SubsonicApi>,
}

impl NavidromeProvider {
    pub fn new(api: Arc<dyn SubsonicApi>) -> NavidromeProvider {
        NavidromeProvider { api }
    }

    pub fn api(&self) -> &Arc<dyn SubsonicApi> {
        &self.api
    }

    fn sid(&self) -> &str {
        self.api.server_id()
    }

    /// Subsonic stream options for a request.
    pub fn stream_options(req: &StreamRequest) -> StreamOptions {
        match &req.profile {
            Some(p) => StreamOptions {
                format: p.format.clone(),
                max_bit_rate: p.max_bit_rate,
                time_offset_s: req.offset_s,
                estimate_content_length: true,
            },
            None => StreamOptions {
                time_offset_s: req.offset_s,
                ..Default::default()
            },
        }
    }
}

fn star_targets(targets: &[api::RatingTarget]) -> Vec<StarTarget> {
    targets
        .iter()
        .map(|t| match t {
            api::RatingTarget::Track { id } => StarTarget::Song(id.clone()),
            api::RatingTarget::Album { id } => StarTarget::Album(id.clone()),
        })
        .collect()
}

impl Provider for NavidromeProvider {
    fn account_id(&self) -> &str {
        self.sid()
    }

    fn kind(&self) -> AccountKind {
        AccountKind::Navidrome
    }

    fn source(&self) -> &str {
        self.sid()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        let caps = self.api.capabilities();
        ProviderCapabilities {
            library_sync: true,
            search: true,
            browse: true,
            stream: true,
            download: true,
            transcode: true,
            cover_art: true,
            star: true,
            rate: true,
            playlists: true,
            smart_playlists: caps.native_api,
            report_play: true,
            lyrics: true,
            radio: true,
            home_shelves: false,
            share: true,
        }
    }

    fn library_page(&self, offset: u32, size: u32) -> ProviderFuture<'_, LibraryPage> {
        Box::pin(async move {
            let r = self
                .api
                .search3("", Search3Page::songs(size, offset))
                .await?;
            let sid = self.sid();
            let n = r.song.len() as u32;
            Ok(LibraryPage {
                tracks: r
                    .song
                    .iter()
                    .filter(|c| !c.is_dir)
                    .map(|c| track_from_child(sid, c))
                    .collect(),
                next_offset: (n >= size && size > 0).then_some(offset + n),
                ..Default::default()
            })
        })
    }

    fn search(&self, query: &str, limit: u32) -> ProviderFuture<'_, LibraryPage> {
        let query = query.to_string();
        Box::pin(async move {
            let page = Search3Page {
                artist_count: limit,
                artist_offset: 0,
                album_count: limit,
                album_offset: 0,
                song_count: limit,
                song_offset: 0,
            };
            let r = self.api.search3(&query, page).await?;
            let sid = self.sid();
            Ok(LibraryPage {
                tracks: r.song.iter().map(|c| track_from_child(sid, c)).collect(),
                albums: r.album.iter().map(|a| album_from_id3(sid, a)).collect(),
                artists: r.artist.iter().map(|a| artist_from_id3(sid, a)).collect(),
                next_offset: None,
            })
        })
    }

    fn album(&self, id: &str) -> ProviderFuture<'_, (api::Album, Vec<api::Track>)> {
        let id = id.to_string();
        Box::pin(async move {
            let a = self.api.album(&id).await?;
            let sid = self.sid();
            Ok((
                album_from_id3(sid, &a.album),
                a.song.iter().map(|c| track_from_child(sid, c)).collect(),
            ))
        })
    }

    fn artist(&self, id: &str) -> ProviderFuture<'_, (api::Artist, Vec<api::Album>)> {
        let id = id.to_string();
        Box::pin(async move {
            let a = self.api.artist(&id).await?;
            let sid = self.sid();
            Ok((
                artist_from_id3(sid, &a.artist),
                a.album.iter().map(|al| album_from_id3(sid, al)).collect(),
            ))
        })
    }

    fn track(&self, id: &str) -> ProviderFuture<'_, api::Track> {
        let id = id.to_string();
        Box::pin(async move {
            let c = self.api.song(&id).await?;
            if c.is_dir {
                return Err(super::ProviderError::NotFound(format!("{id} is a folder")));
            }
            Ok(track_from_child(self.sid(), &c))
        })
    }

    fn resolve_stream(&self, id: &str, req: &StreamRequest) -> ProviderFuture<'_, ResolvedStream> {
        let url = self.api.stream_url(id, &Self::stream_options(req));
        let transcoded = req
            .profile
            .as_ref()
            .is_some_and(|p| p.format.is_some() || p.max_bit_rate.is_some());
        let container = if transcoded {
            req.profile
                .as_ref()
                .and_then(|p| p.format.clone())
                .or(req.suffix.clone())
        } else {
            req.suffix.clone()
        };
        let stream = ResolvedStream {
            url,
            headers: HashMap::new(),
            expires_at: None,
            container,
            codec: None,
            gain_db: None,
            // Navidrome ignores `Range` on a transcode and may on the original.
            range_capable: !transcoded,
            transcoded,
        };
        Box::pin(async move { Ok(stream) })
    }

    fn download_url(&self, id: &str) -> Option<Url> {
        Some(self.api.download_url(id))
    }

    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Option<Url> {
        Some(self.api.cover_art_url(id, size))
    }

    fn download_to_file<'a>(
        &'a self,
        stream: &ResolvedStream,
        dest: &'a Path,
    ) -> ProviderFuture<'a, DownloadOutcome> {
        self.api.download_to_file(stream.url.clone(), dest)
    }

    fn write(&self, write: ProviderWrite) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            match write {
                ProviderWrite::Star(t) => self.api.star(&star_targets(&t)).await,
                ProviderWrite::Unstar(t) => self.api.unstar(&star_targets(&t)).await,
                ProviderWrite::Rate { id, rating } => self.api.set_rating(&id, rating).await,
                ProviderWrite::CreatePlaylist { name, track_ids } => self
                    .api
                    .create_playlist(&name, &track_ids)
                    .await
                    .map(|_| ()),
                ProviderWrite::DeletePlaylist { id } => self.api.delete_playlist(&id).await,
            }
        })
    }

    fn report_play(
        &self,
        id: &str,
        played_at: Option<EpochMs>,
        submission: bool,
    ) -> ProviderFuture<'_, ()> {
        let id = id.to_string();
        Box::pin(async move { self.api.scrobble(&id, played_at, submission).await })
    }

    fn lyrics(&self, id: &str) -> ProviderFuture<'_, Vec<StructuredLyrics>> {
        let id = id.to_string();
        Box::pin(async move { self.api.lyrics_by_song_id(&id).await })
    }

    fn radio(&self, seed: &str, count: u32) -> ProviderFuture<'_, Vec<api::Track>> {
        let seed = seed.to_string();
        Box::pin(async move {
            let songs = self.api.similar_songs2(&seed, count).await?;
            let sid = self.sid();
            Ok(songs.iter().map(|c| track_from_child(sid, c)).collect())
        })
    }

    fn home_shelves(&self) -> ProviderFuture<'_, Vec<HomeShelf>> {
        unsupported("home shelves")
    }

    fn navidrome(&self) -> Option<&Arc<dyn SubsonicApi>> {
        Some(&self.api)
    }
}
