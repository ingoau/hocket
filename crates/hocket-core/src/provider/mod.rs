//! Music providers: the source-neutral seam between the core and a music
//! service (`docs/providers.md`, Phase 1).
//!
//! A [`Provider`] serves one account. It answers in the core's own types
//! (`api::Track`, `api::Album`…), resolves a track to a [`ResolvedStream`],
//! and reports what it can do through [`ProviderCapabilities`]. A method for
//! something the provider can't do answers [`ProviderError::Unsupported`]
//! (see [`unsupported`]).
//!
//! [`NavidromeProvider`] wraps [`SubsonicApi`], which stays Navidrome's own
//! client and test seam. Subsystems whose work is Subsonic through and
//! through (the mirror sync's endpoint walk, smart playlists over the native
//! API, the outbox's playlist edits) reach the client through
//! [`Provider::navidrome`] after choosing the provider by account or source.

mod navidrome;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use futures::future::BoxFuture;
use url::Url;

pub use navidrome::NavidromeProvider;

use crate::api::{
    self, AccountId, AccountKind, EpochMs, ProviderCapabilities, SourceId, TrackId,
    TranscodingProfile,
};
use crate::subsonic::transport::DownloadOutcome;
use crate::subsonic::types::StructuredLyrics;
use crate::subsonic::SubsonicApi;

/// The error every provider answers with. Its variants (auth, network, not
/// found, forbidden, server, protocol, unsupported, io) are the generic set
/// the outbox, jobs and problems already classify.
pub use crate::subsonic::SubsonicError as ProviderError;

pub type ProviderResult<T> = Result<T, ProviderError>;
pub type ProviderFuture<'a, T> = BoxFuture<'a, ProviderResult<T>>;

/// The capability error for a method the provider doesn't offer.
pub fn unsupported<'a, T: Send + 'a>(what: &str) -> ProviderFuture<'a, T> {
    let e = ProviderError::Unsupported(what.to_string());
    Box::pin(async move { Err(e) })
}

/// How to fetch one track's audio, as the provider hands it out. Not to be
/// confused with `api::MediaSource` (what a backend loads, which may be a
/// local file or the loopback proxy) or `downloads::StreamTarget` (what a
/// loopback handle stands for).
#[derive(Clone, PartialEq)]
pub struct ResolvedStream {
    pub url: Url,
    /// Request headers the URL needs (a User-Agent, a cookie). Empty for Navidrome.
    pub headers: HashMap<String, String>,
    /// When the URL stops working; `None` when it doesn't expire.
    pub expires_at: Option<EpochMs>,
    /// Container suffix the bytes come in (`flac`, `mp3`, `m4a`), when known.
    pub container: Option<String>,
    /// Codec inside the container, when it differs from what the suffix says.
    pub codec: Option<String>,
    /// Track gain the provider reports (dB), when it has one and no tag does.
    pub gain_db: Option<f64>,
    /// Whether the server honours `Range` requests on this URL.
    pub range_capable: bool,
    /// Whether the bytes are a transcode (so tags and lengths are estimates).
    pub transcoded: bool,
}

impl std::fmt::Debug for ResolvedStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedStream")
            .field("url", &api::redact_url(self.url.as_str()))
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("expires_at", &self.expires_at)
            .field("container", &self.container)
            .field("range_capable", &self.range_capable)
            .field("transcoded", &self.transcoded)
            .finish()
    }
}

/// What `resolve_stream` should produce.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamRequest {
    /// The transcoding profile in force (`None`: the original).
    pub profile: Option<TranscodingProfile>,
    /// Start this far into the track, for servers that can (`transcodeOffset`).
    pub offset_s: Option<u32>,
    /// The original's suffix, when the caller knows it from the mirror.
    pub suffix: Option<String>,
}

/// One page of a paged library walk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryPage {
    pub tracks: Vec<api::Track>,
    pub albums: Vec<api::Album>,
    pub artists: Vec<api::Artist>,
    /// Where the next page starts; `None` at the end.
    pub next_offset: Option<u32>,
}

/// One shelf of a provider's home feed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HomeShelf {
    pub id: String,
    pub title: String,
    pub tracks: Vec<api::Track>,
    pub albums: Vec<api::Album>,
    pub playlists: Vec<api::Playlist>,
}

/// A write a provider applies to the user's library.
#[derive(Debug, Clone, PartialEq)]
pub enum ProviderWrite {
    Star(Vec<api::RatingTarget>),
    Unstar(Vec<api::RatingTarget>),
    Rate {
        id: String,
        rating: u32,
    },
    CreatePlaylist {
        name: String,
        track_ids: Vec<TrackId>,
    },
    DeletePlaylist {
        id: String,
    },
}

/// A music service account, as the core uses it.
pub trait Provider: Send + Sync + 'static {
    fn account_id(&self) -> &str;
    fn kind(&self) -> AccountKind;
    /// Where this provider's track ids are valid.
    fn source(&self) -> &str;
    fn capabilities(&self) -> ProviderCapabilities;

    // -- library -----------------------------------------------------------
    /// One page of the full track list, for a mirror sync that walks it.
    fn library_page(&self, offset: u32, size: u32) -> ProviderFuture<'_, LibraryPage>;
    /// A live search (the mirror answers first; this is the server batch).
    fn search(&self, query: &str, limit: u32) -> ProviderFuture<'_, LibraryPage>;
    /// One album with its tracks.
    fn album(&self, id: &str) -> ProviderFuture<'_, (api::Album, Vec<api::Track>)>;
    /// One artist with their albums.
    fn artist(&self, id: &str) -> ProviderFuture<'_, (api::Artist, Vec<api::Album>)>;
    fn track(&self, id: &str) -> ProviderFuture<'_, api::Track>;

    // -- playback ----------------------------------------------------------
    fn resolve_stream(&self, id: &str, req: &StreamRequest) -> ProviderFuture<'_, ResolvedStream>;
    /// The original file, for a pin that keeps it (`None`: pins keep the stream).
    fn download_url(&self, id: &str) -> Option<Url>;
    fn cover_art_url(&self, id: &str, size: Option<u32>) -> Option<Url>;
    /// Fetch a resolved stream (or download URL) to a file, as a pin keeps it.
    fn download_to_file<'a>(
        &'a self,
        stream: &ResolvedStream,
        dest: &'a Path,
    ) -> ProviderFuture<'a, DownloadOutcome>;

    // -- writes and reports --------------------------------------------------
    fn write(&self, write: ProviderWrite) -> ProviderFuture<'_, ()>;
    /// A play: `submission` false is "now playing", true a finished scrobble.
    fn report_play(
        &self,
        id: &str,
        played_at: Option<EpochMs>,
        submission: bool,
    ) -> ProviderFuture<'_, ()>;

    // -- extras --------------------------------------------------------------
    fn lyrics(&self, id: &str) -> ProviderFuture<'_, Vec<StructuredLyrics>>;
    /// Tracks like this one (autoplay, "radio").
    fn radio(&self, seed: &str, count: u32) -> ProviderFuture<'_, Vec<api::Track>>;
    fn home_shelves(&self) -> ProviderFuture<'_, Vec<HomeShelf>>;

    /// The Subsonic client behind a Navidrome provider: for the work that is
    /// Subsonic through and through (mirror sync, smart playlists, playlist
    /// edits by index, the probe). `None` for every other kind.
    fn navidrome(&self) -> Option<&Arc<dyn SubsonicApi>> {
        None
    }
}

/// The providers installed right now, by account, highest priority first.
/// Cheap to clone; handed to background work that must route by source.
#[derive(Clone, Default)]
pub struct ProviderSet {
    providers: Vec<Arc<dyn Provider>>,
}

impl ProviderSet {
    pub fn new(providers: Vec<Arc<dyn Provider>>) -> ProviderSet {
        ProviderSet { providers }
    }

    pub fn single(provider: Arc<dyn Provider>) -> ProviderSet {
        ProviderSet {
            providers: vec![provider],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Provider>> {
        self.providers.iter()
    }

    pub fn account(&self, id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.account_id() == id)
    }

    /// The provider whose ids are valid in `source`: the highest-priority
    /// one when several share it.
    pub fn for_source(&self, source: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.iter().find(|p| p.source() == source)
    }

    /// The Subsonic client for `source`, when it is a Navidrome account.
    pub fn subsonic(&self, source: &str) -> Option<Arc<dyn SubsonicApi>> {
        self.for_source(source).and_then(|p| p.navidrome().cloned())
    }

    pub fn sources(&self) -> Vec<SourceId> {
        self.providers
            .iter()
            .map(|p| p.source().to_string())
            .collect()
    }

    pub fn accounts(&self) -> Vec<AccountId> {
        self.providers
            .iter()
            .map(|p| p.account_id().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests;
