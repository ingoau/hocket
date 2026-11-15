//! Pure parsers from InnerTube JSON to YouTube Music items. Layouts move
//! around, so items are found by walking for the renderers that carry them
//! (`musicResponsiveListItemRenderer`, `musicTwoRowItemRenderer`,
//! `playlistPanelVideoRenderer`) rather than by fixed paths; only page
//! headers and shelf titles are looked up by name.

use serde_json::Value;

/// What kind of video a watch id is, as YouTube Music labels it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoType {
    /// An "Art Track": the album audio. A song.
    Atv,
    /// An official music video.
    Omv,
    /// A user upload to YouTube.
    Ugc,
    /// One of the user's own uploads to YouTube Music. A song.
    Uploaded,
    /// A podcast episode.
    Episode,
    Other(String),
}

impl VideoType {
    pub fn parse(s: &str) -> VideoType {
        match s {
            "MUSIC_VIDEO_TYPE_ATV" => VideoType::Atv,
            "MUSIC_VIDEO_TYPE_OMV" => VideoType::Omv,
            "MUSIC_VIDEO_TYPE_UGC" => VideoType::Ugc,
            "MUSIC_VIDEO_TYPE_PRIVATELY_OWNED_TRACK" => VideoType::Uploaded,
            "MUSIC_VIDEO_TYPE_PODCAST_EPISODE" => VideoType::Episode,
            other => VideoType::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Named {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct YtSong {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<Named>,
    pub album: Option<Named>,
    pub duration_s: Option<u32>,
    /// `None` when the page didn't say (album pages often don't).
    pub video_type: Option<VideoType>,
    /// The entry's id within a playlist (edits address entries by it).
    pub set_video_id: Option<String>,
    pub thumbnail: Option<String>,
    pub explicit: bool,
    pub year: Option<u32>,
    /// "Remove from library" / "Add to library" tokens from the item's menu.
    pub feedback_tokens: Option<(String, String)>,
}

impl YtSong {
    /// The songs-only filter (`docs/providers.md`): Art Tracks and the user's
    /// uploads are songs; music videos, user videos and episodes are not.
    /// An item whose type the page didn't give counts as a song.
    pub fn is_song(&self) -> bool {
        matches!(
            self.video_type,
            None | Some(VideoType::Atv) | Some(VideoType::Uploaded)
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct YtAlbum {
    /// `MPREb_…` (or `FEmusic_library_privately_owned_release_detail…` for uploads).
    pub browse_id: String,
    pub title: String,
    pub artists: Vec<Named>,
    pub year: Option<u32>,
    pub thumbnail: Option<String>,
    /// "Album", "Single", "EP".
    pub kind: Option<String>,
    /// The album's `OLAK5uy_…` playlist (what "save to library" likes).
    pub playlist_id: Option<String>,
    pub explicit: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct YtArtist {
    pub browse_id: String,
    pub name: String,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct YtPlaylist {
    /// Without the `VL` browse prefix.
    pub id: String,
    pub title: String,
    pub author: Option<String>,
    pub count: Option<u32>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Song(YtSong),
    Album(YtAlbum),
    Artist(YtArtist),
    Playlist(YtPlaylist),
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// The text of a `{runs:[…]}` or `{simpleText}` value.
pub fn text(v: &Value) -> String {
    if let Some(s) = v.get("simpleText").and_then(Value::as_str) {
        return s.to_string();
    }
    v.get("runs")
        .and_then(Value::as_array)
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .unwrap_or_default()
}

fn runs(v: &Value) -> &[Value] {
    v.get("runs")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

fn str_at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut o = v;
    for p in path {
        o = o.get(*p)?;
    }
    o.as_str()
}

/// `(browseId, pageType)` of a navigation endpoint (or of a run).
fn browse_of(v: &Value) -> Option<(&str, Option<&str>)> {
    let b = v
        .get("navigationEndpoint")
        .and_then(|n| n.get("browseEndpoint"))
        .or_else(|| v.get("browseEndpoint"))?;
    let id = b.get("browseId")?.as_str()?;
    let page = str_at(
        b,
        &[
            "browseEndpointContextSupportedConfigs",
            "browseEndpointContextMusicConfig",
            "pageType",
        ],
    );
    Some((id, page))
}

fn watch_type(w: &Value) -> Option<VideoType> {
    str_at(
        w,
        &[
            "watchEndpointMusicSupportedConfigs",
            "watchEndpointMusicConfig",
            "musicVideoType",
        ],
    )
    .map(VideoType::parse)
}

/// The first `watchEndpoint` under `v` (a run, an overlay's play button).
fn find_watch(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            if let Some(w) = m.get("watchEndpoint") {
                return Some(w);
            }
            m.values().find_map(find_watch)
        }
        Value::Array(a) => a.iter().find_map(find_watch),
        _ => None,
    }
}

/// The first value under `key` anywhere in `v` (depth first).
pub fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(m) => {
            if let Some(x) = m.get(key) {
                return Some(x);
            }
            m.values().find_map(|c| find_key(c, key))
        }
        Value::Array(a) => a.iter().find_map(|c| find_key(c, key)),
        _ => None,
    }
}

/// The first `playlistId` under `v` that starts with `prefix`.
fn find_playlist_id(v: &Value, prefix: &str) -> Option<String> {
    match v {
        Value::Object(m) => {
            if let Some(id) = m.get("playlistId").and_then(Value::as_str) {
                if id.starts_with(prefix) {
                    return Some(id.to_string());
                }
            }
            m.values().find_map(|c| find_playlist_id(c, prefix))
        }
        Value::Array(a) => a.iter().find_map(|c| find_playlist_id(c, prefix)),
        _ => None,
    }
}

fn last_thumbnail(v: &Value) -> Option<String> {
    let thumbs = find_key(v, "thumbnails")?.as_array()?;
    thumbs
        .last()?
        .get("url")?
        .as_str()
        .map(|s| s.split('?').next().unwrap_or(s).to_string())
}

/// `6:10` → 370, `1:02:03` → 3723.
pub fn parse_duration(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    if parts.len() < 2 || parts.len() > 3 {
        return None;
    }
    let mut total = 0u32;
    for p in &parts {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        total = total * 60 + p.parse::<u32>().ok()?;
    }
    Some(total)
}

fn parse_year(s: &str) -> Option<u32> {
    let s = s.trim();
    (s.len() == 4 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())
        .flatten()
}

fn is_explicit(v: &Value) -> bool {
    v.get("badges")
        .or_else(|| v.get("subtitleBadges"))
        .map(|b| b.to_string().contains("MUSIC_EXPLICIT_BADGE"))
        .unwrap_or(false)
}

/// Artists, album, duration and year from a byline (` • `-separated runs).
#[derive(Default)]
struct Byline {
    artists: Vec<Named>,
    album: Option<Named>,
    duration_s: Option<u32>,
    year: Option<u32>,
    /// The first segment when it names the item's kind ("Song", "Album"…).
    kind: Option<String>,
    plain: Vec<String>,
}

const KINDS: [&str; 9] = [
    "Song", "Video", "Album", "Single", "EP", "Artist", "Playlist", "Episode", "Podcast",
];

fn byline(rs: &[Value]) -> Byline {
    let mut b = Byline::default();
    let mut first = true;
    for r in rs {
        let t = r.get("text").and_then(Value::as_str).unwrap_or_default();
        let trimmed = t.trim();
        if trimmed.is_empty() || matches!(trimmed, "•" | "," | "&") {
            continue;
        }
        match browse_of(r) {
            Some((id, page)) => {
                let named = Named {
                    id: Some(id.to_string()),
                    name: t.to_string(),
                };
                let album = page == Some("MUSIC_PAGE_TYPE_ALBUM")
                    || (page.is_none() && id.starts_with("MPRE"));
                if album {
                    b.album = Some(named);
                } else if page.is_none_or(|p| {
                    p == "MUSIC_PAGE_TYPE_ARTIST" || p == "MUSIC_PAGE_TYPE_USER_CHANNEL"
                }) {
                    b.artists.push(named);
                }
            }
            None => {
                if let Some(d) = parse_duration(trimmed) {
                    b.duration_s = Some(d);
                } else if let Some(y) = parse_year(trimmed) {
                    b.year = Some(y);
                } else if first && KINDS.contains(&trimmed) {
                    b.kind = Some(trimmed.to_string());
                } else {
                    b.plain.push(trimmed.to_string());
                }
            }
        }
        first = false;
    }
    b
}

fn flex_column(r: &Value, i: usize) -> Option<&Value> {
    r.get("flexColumns")?
        .as_array()?
        .get(i)?
        .get("musicResponsiveListItemFlexColumnRenderer")?
        .get("text")
}

fn feedback_tokens(r: &Value) -> Option<(String, String)> {
    let item = find_key(r.get("menu")?, "toggleMenuServiceItemRenderer")?;
    let add = find_key(item.get("defaultServiceEndpoint")?, "feedbackToken")?.as_str()?;
    let remove = find_key(item.get("toggledServiceEndpoint")?, "feedbackToken")?.as_str()?;
    Some((add.to_string(), remove.to_string()))
}

// ---------------------------------------------------------------------------
// items
// ---------------------------------------------------------------------------

/// One row of a list (search results, a playlist, an album, a library shelf).
pub fn item_from_list(r: &Value) -> Option<Item> {
    let col0 = flex_column(r, 0);
    let title = col0.map(text).unwrap_or_default();
    let col0_run = col0.map(runs).and_then(|rs| rs.first());
    let video_id = str_at(r, &["playlistItemData", "videoId"])
        .or_else(|| {
            col0_run.and_then(|x| str_at(x, &["navigationEndpoint", "watchEndpoint", "videoId"]))
        })
        .or_else(|| {
            r.get("overlay")
                .and_then(find_watch)
                .and_then(|w| w.get("videoId"))
                .and_then(Value::as_str)
        });
    let mut by = Byline::default();
    let n_cols = r
        .get("flexColumns")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    for i in 1..n_cols {
        let b = byline(flex_column(r, i).map(runs).unwrap_or(&[]));
        if by.kind.is_none() {
            by.kind = b.kind;
        }
        by.artists.extend(b.artists);
        by.album = by.album.or(b.album);
        by.duration_s = by.duration_s.or(b.duration_s);
        by.year = by.year.or(b.year);
        if i == 1 {
            by.plain.extend(b.plain);
        }
    }
    let fixed = r
        .get("fixedColumns")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .and_then(|c| c.get("musicResponsiveListItemFixedColumnRenderer"))
        .map(|c| text(&c["text"]));
    if let Some(d) = fixed.as_deref().and_then(parse_duration) {
        by.duration_s = by.duration_s.or(Some(d));
    }
    let thumbnail = r.get("thumbnail").and_then(last_thumbnail);
    if let Some(video_id) = video_id {
        let video_type = col0_run
            .and_then(|x| x.get("navigationEndpoint"))
            .and_then(|n| n.get("watchEndpoint"))
            .and_then(watch_type)
            .or_else(|| r.get("overlay").and_then(find_watch).and_then(watch_type));
        if by.artists.is_empty() {
            // "Various Artists", an upload's artist: a plain byline segment.
            if let Some(p) = by.plain.first() {
                by.artists.push(Named {
                    id: None,
                    name: p.clone(),
                });
            }
        }
        return Some(Item::Song(YtSong {
            video_id: video_id.to_string(),
            title,
            artists: by.artists,
            album: by.album,
            duration_s: by.duration_s,
            video_type,
            set_video_id: str_at(r, &["playlistItemData", "playlistSetVideoId"])
                .map(str::to_string),
            thumbnail,
            explicit: is_explicit(r),
            year: by.year,
            feedback_tokens: feedback_tokens(r),
        }));
    }
    let (id, page) = browse_of(r)?;
    Some(browse_item(
        id,
        page,
        title,
        by,
        thumbnail,
        is_explicit(r),
        r,
    ))
}

fn browse_item(
    id: &str,
    page: Option<&str>,
    title: String,
    by: Byline,
    thumbnail: Option<String>,
    explicit: bool,
    r: &Value,
) -> Item {
    let page = page.unwrap_or_else(|| {
        if id.starts_with("MPRE") {
            "MUSIC_PAGE_TYPE_ALBUM"
        } else if id.starts_with("UC") {
            "MUSIC_PAGE_TYPE_ARTIST"
        } else {
            "MUSIC_PAGE_TYPE_PLAYLIST"
        }
    });
    match page {
        "MUSIC_PAGE_TYPE_ALBUM" | "MUSIC_PAGE_TYPE_AUDIOBOOK" => {
            let playlist_id = find_key(r, "watchPlaylistEndpoint")
                .and_then(|w| w.get("playlistId"))
                .and_then(Value::as_str)
                .map(str::to_string);
            Item::Album(YtAlbum {
                browse_id: id.to_string(),
                title,
                artists: by.artists,
                year: by.year,
                thumbnail,
                kind: by.kind,
                playlist_id,
                explicit,
            })
        }
        "MUSIC_PAGE_TYPE_ARTIST"
        | "MUSIC_PAGE_TYPE_USER_CHANNEL"
        | "MUSIC_PAGE_TYPE_LIBRARY_ARTIST" => Item::Artist(YtArtist {
            browse_id: id.strip_prefix("MPLA").unwrap_or(id).to_string(),
            name: title,
            thumbnail,
        }),
        _ => {
            let count = by
                .plain
                .iter()
                .find_map(|p| p.split_whitespace().next()?.replace(',', "").parse().ok());
            Item::Playlist(YtPlaylist {
                id: id.strip_prefix("VL").unwrap_or(id).to_string(),
                title,
                author: by.artists.first().map(|a| a.name.clone()),
                count,
                thumbnail,
            })
        }
    }
}

/// One card of a carousel or grid.
pub fn item_from_two_row(r: &Value) -> Option<Item> {
    let title = r.get("title").map(text).unwrap_or_default();
    let by = byline(r.get("subtitle").map(runs).unwrap_or(&[]));
    let thumbnail = r.get("thumbnailRenderer").and_then(last_thumbnail);
    let nav = r.get("navigationEndpoint")?;
    if let Some(w) = nav.get("watchEndpoint") {
        let video_id = w.get("videoId")?.as_str()?;
        return Some(Item::Song(YtSong {
            video_id: video_id.to_string(),
            title,
            artists: by.artists,
            album: by.album,
            duration_s: by.duration_s,
            video_type: watch_type(w),
            thumbnail,
            explicit: is_explicit(r),
            year: by.year,
            ..Default::default()
        }));
    }
    let (id, page) = browse_of(nav)?;
    Some(browse_item(
        id,
        page,
        title,
        by,
        thumbnail,
        is_explicit(r),
        r,
    ))
}

/// One entry of a watch queue (radio, "up next").
pub fn item_from_panel(r: &Value) -> Option<YtSong> {
    let video_id = r.get("videoId")?.as_str()?;
    let by = byline(r.get("longBylineText").map(runs).unwrap_or(&[]));
    let watch = r
        .get("navigationEndpoint")
        .and_then(|n| n.get("watchEndpoint"));
    Some(YtSong {
        video_id: video_id.to_string(),
        title: r.get("title").map(text).unwrap_or_default(),
        artists: by.artists,
        album: by.album,
        duration_s: r
            .get("lengthText")
            .map(text)
            .as_deref()
            .and_then(parse_duration),
        video_type: watch.and_then(watch_type),
        set_video_id: r
            .get("playlistSetVideoId")
            .and_then(Value::as_str)
            .map(str::to_string),
        thumbnail: r.get("thumbnail").and_then(last_thumbnail),
        explicit: is_explicit(r),
        year: by.year,
        feedback_tokens: None,
    })
}

/// Every item under `v`, in document order.
pub fn collect_items(v: &Value) -> Vec<Item> {
    let mut out = vec![];
    walk_items(v, &mut out);
    out
}

fn walk_items(v: &Value, out: &mut Vec<Item>) {
    match v {
        Value::Object(m) => {
            if let Some(r) = m.get("musicResponsiveListItemRenderer") {
                out.extend(item_from_list(r));
                return;
            }
            if let Some(r) = m.get("musicTwoRowItemRenderer") {
                out.extend(item_from_two_row(r));
                return;
            }
            if let Some(r) = m.get("playlistPanelVideoWrapperRenderer") {
                // The song, not its counterpart (the music video).
                if let Some(p) = r.get("primaryRenderer") {
                    walk_items(p, out);
                }
                return;
            }
            if let Some(r) = m.get("playlistPanelVideoRenderer") {
                out.extend(item_from_panel(r).map(Item::Song));
                return;
            }
            for (k, c) in m {
                // Header buttons, menus and related-playlist continuations
                // name items that aren't on the page.
                if matches!(k.as_str(), "menu" | "header" | "continuations" | "overlay") {
                    continue;
                }
                walk_items(c, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| walk_items(c, out)),
        _ => {}
    }
}

pub fn songs(items: &[Item]) -> Vec<YtSong> {
    items
        .iter()
        .filter_map(|i| match i {
            Item::Song(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// pages
// ---------------------------------------------------------------------------

/// The continuation of the list a page is about: its playlist shelf, music
/// shelf or grid (not the "related" carousels after it).
pub fn continuation(v: &Value) -> Option<String> {
    const LISTS: [&str; 6] = [
        "musicPlaylistShelfRenderer",
        "musicShelfRenderer",
        "gridRenderer",
        "musicPlaylistShelfContinuation",
        "musicShelfContinuation",
        "gridContinuation",
    ];
    for key in LISTS {
        if let Some(list) = find_key(v, key) {
            if let Some(t) = list_continuation(list) {
                return Some(t);
            }
        }
    }
    // `appendContinuationItemsAction` answers end with their own token.
    if let Some(actions) = v.get("onResponseReceivedActions").and_then(Value::as_array) {
        for a in actions {
            if let Some(items) = find_key(a, "continuationItems").and_then(Value::as_array) {
                if let Some(t) = items.last().and_then(token_of_item) {
                    return Some(t);
                }
            }
        }
    }
    None
}

fn list_continuation(list: &Value) -> Option<String> {
    if let Some(t) = list
        .get("continuations")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("nextContinuationData"))
        .and_then(|c| c.get("continuation"))
        .and_then(Value::as_str)
    {
        return Some(t.to_string());
    }
    let items = list
        .get("contents")
        .or_else(|| list.get("items"))
        .and_then(Value::as_array)?;
    items.last().and_then(token_of_item)
}

fn token_of_item(item: &Value) -> Option<String> {
    item.get("continuationItemRenderer")?
        .get("continuationEndpoint")?
        .get("continuationCommand")?
        .get("token")?
        .as_str()
        .map(str::to_string)
}

/// Search results by kind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SearchResults {
    pub songs: Vec<YtSong>,
    pub albums: Vec<YtAlbum>,
    pub artists: Vec<YtArtist>,
    pub playlists: Vec<YtPlaylist>,
}

impl SearchResults {
    fn absorb(&mut self, items: Vec<Item>) {
        for i in items {
            match i {
                Item::Song(s) if !self.songs.iter().any(|x| x.video_id == s.video_id) => {
                    self.songs.push(s)
                }
                Item::Album(a) if !self.albums.iter().any(|x| x.browse_id == a.browse_id) => {
                    self.albums.push(a)
                }
                Item::Artist(a) if !self.artists.iter().any(|x| x.browse_id == a.browse_id) => {
                    self.artists.push(a)
                }
                Item::Playlist(p) if !self.playlists.iter().any(|x| x.id == p.id) => {
                    self.playlists.push(p)
                }
                _ => {}
            }
        }
    }
}

pub fn parse_search(v: &Value) -> SearchResults {
    let mut r = SearchResults::default();
    r.absorb(collect_items(v.get("contents").unwrap_or(v)));
    r
}

/// An album page: the album and its tracks. Album tracks are songs whatever
/// their video type (YouTube Music labels some album tracks as music videos).
pub fn parse_album(v: &Value, browse_id: &str) -> Option<(YtAlbum, Vec<YtSong>)> {
    let h = find_key(v, "musicResponsiveHeaderRenderer")
        .or_else(|| find_key(v, "musicDetailHeaderRenderer"))?;
    let sub = byline(h.get("subtitle").map(runs).unwrap_or(&[]));
    let mut artists = byline(h.get("straplineTextOne").map(runs).unwrap_or(&[])).artists;
    if artists.is_empty() {
        artists = sub.artists.clone();
    }
    let playlist_id = v
        .get("microformat")
        .and_then(|m| find_key(m, "urlCanonical"))
        .and_then(Value::as_str)
        .and_then(|u| u.split("list=").nth(1))
        .map(|s| s.split('&').next().unwrap_or(s).to_string())
        .or_else(|| {
            find_key(v, "watchPlaylistEndpoint")
                .and_then(|w| w.get("playlistId"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| find_playlist_id(v, "OLAK"));
    let album = YtAlbum {
        browse_id: browse_id.to_string(),
        title: h.get("title").map(text).unwrap_or_default(),
        artists: artists.clone(),
        year: sub.year,
        thumbnail: h.get("thumbnail").and_then(last_thumbnail),
        kind: sub.kind,
        playlist_id,
        explicit: is_explicit(h),
    };
    let body = find_key(v, "secondaryContents").unwrap_or(v);
    let mut tracks = songs(&collect_items(body));
    for (i, t) in tracks.iter_mut().enumerate() {
        t.video_type = None;
        if t.artists.is_empty() {
            t.artists = artists.clone();
        }
        t.album = Some(Named {
            id: Some(browse_id.to_string()),
            name: album.title.clone(),
        });
        t.year = t.year.or(album.year);
        t.thumbnail = t.thumbnail.clone().or(album.thumbnail.clone());
        let _ = i;
    }
    Some((album, tracks))
}

/// A playlist page (or a continuation of one): every entry in order, songs
/// and videos alike, with its `setVideoId`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistPage {
    pub playlist: YtPlaylist,
    pub entries: Vec<YtSong>,
    pub continuation: Option<String>,
}

pub fn parse_playlist(v: &Value, id: &str) -> PlaylistPage {
    let h = find_key(v, "musicResponsiveHeaderRenderer")
        .or_else(|| find_key(v, "musicDetailHeaderRenderer"));
    let title = h.and_then(|h| h.get("title")).map(text).unwrap_or_default();
    let author = h
        .and_then(|h| h.get("straplineTextOne").or_else(|| h.get("subtitle")))
        .map(|s| byline(runs(s)))
        .and_then(|b| {
            b.artists
                .first()
                .map(|a| a.name.clone())
                .or(b.plain.first().cloned())
        });
    let count = h
        .and_then(|h| h.get("secondSubtitle"))
        .map(|s| byline(runs(s)))
        .and_then(|b| {
            b.plain
                .iter()
                .find_map(|p| p.split_whitespace().next()?.replace(',', "").parse().ok())
        });
    let list = find_key(v, "musicPlaylistShelfRenderer")
        .or_else(|| find_key(v, "musicPlaylistShelfContinuation"))
        .or_else(|| find_key(v, "secondaryContents"))
        .unwrap_or(v);
    let entries = songs(&collect_items(list));
    PlaylistPage {
        playlist: YtPlaylist {
            id: id.strip_prefix("VL").unwrap_or(id).to_string(),
            title,
            author,
            count,
            thumbnail: h.and_then(|h| h.get("thumbnail")).and_then(last_thumbnail),
        },
        entries,
        continuation: continuation(v),
    }
}

/// One shelf of a page (home, an artist).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<Item>,
    /// "See all" (an artist's full song list, an album list).
    pub more: Option<String>,
}

pub fn parse_shelves(v: &Value) -> Vec<Shelf> {
    let Some(list) = find_key(v, "sectionListRenderer")
        .or_else(|| find_key(v, "sectionListContinuation"))
        .and_then(|s| s.get("contents"))
        .and_then(Value::as_array)
    else {
        return vec![];
    };
    list.iter()
        .filter_map(|section| {
            let (_, r) = section.as_object()?.iter().next()?;
            let title = r
                .get("header")
                .and_then(|h| find_key(h, "title").map(text).filter(|t| !t.is_empty()))
                .or_else(|| r.get("title").map(text))
                .unwrap_or_default();
            let more = r
                .get("bottomEndpoint")
                .and_then(browse_of)
                .or_else(|| {
                    r.get("header")
                        .and_then(|h| find_key(h, "moreContentButton"))
                        .and_then(|b| find_key(b, "browseEndpoint"))
                        .and_then(|b| b.get("browseId").and_then(Value::as_str))
                        .map(|id| (id, None))
                })
                .map(|(id, _)| id.to_string());
            let items = collect_items(r.get("contents")?);
            (!items.is_empty()).then_some(Shelf { title, items, more })
        })
        .collect()
}

/// An artist page: the artist and its shelves.
pub fn parse_artist(v: &Value, browse_id: &str) -> (YtArtist, Vec<Shelf>) {
    let h = v.get("header").and_then(|h| h.as_object()?.values().next());
    let artist = YtArtist {
        browse_id: browse_id.to_string(),
        name: h.and_then(|h| h.get("title")).map(text).unwrap_or_default(),
        thumbnail: h.and_then(|h| h.get("thumbnail")).and_then(last_thumbnail),
    };
    (artist, parse_shelves(v))
}

/// A `next` answer: the watch queue (radio) and the lyrics browse id.
pub fn parse_next(v: &Value) -> (Vec<YtSong>, Option<String>) {
    let tabs = find_key(v, "watchNextTabbedResultsRenderer")
        .and_then(|t| t.get("tabs"))
        .and_then(Value::as_array);
    let mut lyrics = None;
    let mut queue = vec![];
    for (i, tab) in tabs.into_iter().flatten().enumerate() {
        let t = &tab["tabRenderer"];
        if let Some((id, page)) = t.get("endpoint").and_then(browse_of) {
            if page == Some("MUSIC_PAGE_TYPE_TRACK_LYRICS") || id.starts_with("MPLYt") {
                lyrics = Some(id.to_string());
            }
        }
        if i == 0 {
            queue = songs(&collect_items(t.get("content").unwrap_or(&Value::Null)));
        }
    }
    if queue.is_empty() {
        // A continuation of the queue.
        queue = songs(&collect_items(
            v.get("continuationContents").unwrap_or(&Value::Null),
        ));
    }
    (queue, lyrics)
}

/// Lyrics text and their source line ("Source: LyricFind").
pub fn parse_lyrics(v: &Value) -> Option<(String, Option<String>)> {
    let d = find_key(v, "musicDescriptionShelfRenderer")?;
    let body = d.get("description").map(text)?.replace("\r\n", "\n");
    if body.trim().is_empty() {
        return None;
    }
    let footer = d.get("footer").map(text).filter(|f| !f.is_empty());
    Some((body, footer))
}

/// One audio format of a `player` answer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioFormat {
    pub itag: u32,
    pub mime: String,
    pub url: Option<String>,
    /// A ciphered URL (`s`, `sp`, `url`) the JS host must sign.
    pub cipher: Option<String>,
    pub bitrate: Option<u32>,
    pub content_length: Option<u64>,
    pub loudness_db: Option<f64>,
    pub duration_ms: Option<u64>,
    /// A DRC (dynamic-range-compressed) variant: never chosen.
    pub drc: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerInfo {
    pub status: String,
    pub reason: Option<String>,
    pub formats: Vec<AudioFormat>,
    pub expires_in_s: Option<u64>,
    /// No URLs, only a SABR stream: unusable here.
    pub sabr_only: bool,
    pub playback_url: Option<String>,
    pub watchtime_url: Option<String>,
    pub duration_s: Option<u32>,
}

pub fn parse_player(v: &Value) -> PlayerInfo {
    let ps = &v["playabilityStatus"];
    let sd = &v["streamingData"];
    let formats: Vec<AudioFormat> = sd
        .get("adaptiveFormats")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| {
            f.get("mimeType")
                .and_then(Value::as_str)
                .is_some_and(|m| m.starts_with("audio/"))
        })
        .map(|f| AudioFormat {
            itag: f.get("itag").and_then(Value::as_u64).unwrap_or(0) as u32,
            mime: f["mimeType"].as_str().unwrap_or_default().to_string(),
            url: f.get("url").and_then(Value::as_str).map(str::to_string),
            cipher: f
                .get("signatureCipher")
                .or_else(|| f.get("cipher"))
                .and_then(Value::as_str)
                .map(str::to_string),
            bitrate: f.get("bitrate").and_then(Value::as_u64).map(|b| b as u32),
            content_length: f
                .get("contentLength")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok()),
            loudness_db: f.get("loudnessDb").and_then(Value::as_f64),
            duration_ms: f
                .get("approxDurationMs")
                .and_then(Value::as_str)
                .and_then(|s| s.parse().ok()),
            drc: f.get("isDrc").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect();
    let any_url = formats
        .iter()
        .any(|f| f.url.is_some() || f.cipher.is_some());
    PlayerInfo {
        status: ps
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("UNKNOWN")
            .to_string(),
        reason: ps.get("reason").and_then(Value::as_str).map(str::to_string),
        sabr_only: !any_url && sd.get("serverAbrStreamingUrl").is_some()
            || (!any_url && !formats.is_empty()),
        formats,
        expires_in_s: sd
            .get("expiresInSeconds")
            .and_then(|e| e.as_str().and_then(|s| s.parse().ok()).or(e.as_u64())),
        playback_url: str_at(v, &["playbackTracking", "videostatsPlaybackUrl", "baseUrl"])
            .map(str::to_string),
        watchtime_url: str_at(
            v,
            &["playbackTracking", "videostatsWatchtimeUrl", "baseUrl"],
        )
        .map(str::to_string),
        duration_s: str_at(v, &["videoDetails", "lengthSeconds"]).and_then(|s| s.parse().ok()),
    }
}

/// The signed-in account's channel id and name, from `account/account_menu`.
pub fn parse_account(v: &Value) -> Option<(String, String)> {
    let h = find_key(v, "activeAccountHeaderRenderer")?;
    let name = h.get("accountName").map(text).unwrap_or_default();
    let channel = find_key(h, "browseId")
        .and_then(Value::as_str)
        .or_else(|| find_key(v, "channelId").and_then(Value::as_str))
        .map(str::to_string)?;
    Some((channel, name))
}

/// Whether the account has Premium: the account menu offers no "Get Music
/// Premium" upsell (a heuristic; Premium is detected, not chosen).
pub fn is_premium(account_menu: &Value) -> bool {
    let s = account_menu.to_string();
    !s.contains("Get Music Premium") && !s.contains("PREMIUM_UPSELL")
}

/// `{"status":"STATUS_SUCCEEDED", "playlistEditResults":[…]}`: the
/// `setVideoId`s an add created, in order.
pub fn parse_edit_result(v: &Value) -> Result<Vec<(String, String)>, String> {
    let status = v.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "STATUS_SUCCEEDED" {
        return Err(format!("playlist edit: {status}"));
    }
    Ok(v.get("playlistEditResults")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let d = r.get("playlistEditVideoAddedResultData")?;
            Some((
                d.get("videoId")?.as_str()?.to_string(),
                d.get("setVideoId")?.as_str()?.to_string(),
            ))
        })
        .collect())
}
