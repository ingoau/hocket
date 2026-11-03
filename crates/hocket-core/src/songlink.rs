//! Song.link URLs for a song or an album, by way of Deezer.
//!
//! Song.link has no search URL: `song.link/s/<x>` takes a Spotify track id. So the
//! song is first found on Deezer, which needs no key: by ISRC when the server knows
//! it, else by searching "artist title" and keeping only a result whose artist and
//! title match. The answer is then `song.link/d/<deezer track id>` or
//! `album.link/d/<deezer album id>`.
//!
//! Network goes through [`LyricsHttp`] (a plain GET), so tests use a fake.

use serde::Deserialize;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

use crate::lyrics::LyricsHttp;

const DEEZER: &str = "https://api.deezer.com";

/// What is known about a song, for finding it on Deezer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SongFacts {
    pub title: String,
    pub artist: Option<String>,
    pub duration_ms: Option<u64>,
    pub isrc: Vec<String>,
}

/// The Song.link page for a song: by ISRC, else by search. None when nothing matches.
pub async fn song_url(http: &dyn LyricsHttp, song: &SongFacts) -> Option<String> {
    let isrcs = song
        .isrc
        .iter()
        .map(|s| s.replace(['-', ' '], ""))
        .filter(|s| valid_isrc(s));
    for isrc in isrcs {
        let url = format!("{DEEZER}/track/isrc:{isrc}");
        match http.get(&url).await {
            Ok(body) => {
                if let Some(id) = parse_isrc_track(&body) {
                    return Some(format!("https://song.link/d/{id}"));
                }
            }
            Err(e) => {
                tracing::debug!(target: "hocket_core", error = %e, "deezer isrc lookup failed")
            }
        }
    }
    let artist = song
        .artist
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())?;
    let title = song.title.trim();
    if title.is_empty() {
        return None;
    }
    let url = search_url("search", &format!("{artist} {title}"));
    let body = match http.get(&url).await {
        Ok(body) => body,
        Err(e) => {
            tracing::debug!(target: "hocket_core", error = %e, "deezer track search failed");
            return None;
        }
    };
    pick_track(&body, artist, title, song.duration_ms).map(|id| format!("https://song.link/d/{id}"))
}

/// The Album.link page for an album, by search. None when nothing matches.
pub async fn album_url(http: &dyn LyricsHttp, name: &str, artist: Option<&str>) -> Option<String> {
    let artist = artist.map(str::trim).filter(|a| !a.is_empty())?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let url = search_url("search/album", &format!("{artist} {name}"));
    let body = match http.get(&url).await {
        Ok(body) => body,
        Err(e) => {
            tracing::debug!(target: "hocket_core", error = %e, "deezer album search failed");
            return None;
        }
    };
    pick_album(&body, artist, name).map(|id| format!("https://album.link/d/{id}"))
}

fn search_url(path: &str, q: &str) -> String {
    let mut url = url::Url::parse(&format!("{DEEZER}/{path}")).expect("static url");
    url.query_pairs_mut()
        .append_pair("q", q)
        .append_pair("limit", "25");
    url.into()
}

/// Twelve letters and digits (tags may write `GB-AYE-92-00070`; the dashes go first). Anything
/// else is not worth a request, and must not reach the path.
fn valid_isrc(s: &str) -> bool {
    s.len() == 12 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[derive(Deserialize)]
struct DeezerArtist {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct DeezerItem {
    id: u64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    title_short: Option<String>,
    #[serde(default)]
    duration: Option<u64>,
    artist: Option<DeezerArtist>,
}

#[derive(Deserialize)]
struct DeezerList {
    #[serde(default)]
    data: Vec<DeezerItem>,
}

/// The track id in a `track/isrc:` answer; None for Deezer's `{"error":…}`.
pub fn parse_isrc_track(body: &str) -> Option<u64> {
    #[derive(Deserialize)]
    struct Found {
        id: Option<u64>,
    }
    serde_json::from_str::<Found>(body)
        .ok()?
        .id
        .filter(|id| *id > 0)
}

/// The best search result for this song: same artist and title, nearest in length.
pub fn pick_track(body: &str, artist: &str, title: &str, duration_ms: Option<u64>) -> Option<u64> {
    let list: DeezerList = serde_json::from_str(body).ok()?;
    let want = fold(title);
    let want_base = fold(&base_title(title));
    // A title of symbols only ("?", "♥") folds to nothing and would equal any other.
    if want.is_empty() {
        return None;
    }
    let secs = duration_ms.map(|ms| ms / 1000);
    list.data
        .iter()
        .filter(|t| t.id > 0 && artist_matches(artist, t.artist.as_ref()))
        .filter_map(|t| {
            let full = fold(&t.title);
            let short = t.title_short.as_deref().map(fold);
            // 0: the same title. 1: the same once "(Remastered)" and the like are gone.
            let rank = if full == want {
                0
            } else if fold(&base_title(&t.title)) == want_base
                || short.as_deref() == Some(want_base.as_str())
            {
                1
            } else {
                return None;
            };
            let off = match (secs, t.duration) {
                (Some(a), Some(b)) => a.abs_diff(b),
                _ => 0,
            };
            // A version minutes away (a live take, an extended mix) is a different recording.
            if off > 10 {
                return None;
            }
            Some(((rank, off), t.id))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, id)| id)
}

/// The best album search result: same artist and name.
pub fn pick_album(body: &str, artist: &str, name: &str) -> Option<u64> {
    let list: DeezerList = serde_json::from_str(body).ok()?;
    let want = fold(name);
    let want_base = fold(&base_title(name));
    if want.is_empty() {
        return None;
    }
    list.data
        .iter()
        .filter(|a| a.id > 0 && artist_matches(artist, a.artist.as_ref()))
        .filter_map(|a| {
            let rank = if fold(&a.title) == want {
                0
            } else if fold(&base_title(&a.title)) == want_base {
                1
            } else {
                return None;
            };
            Some((rank, a.id))
        })
        .min_by_key(|(rank, _)| *rank)
        .map(|(_, id)| id)
}

/// Deezer names one main artist; the server may name several ("A feat. B", "A, B"). So Deezer's
/// words must open ours, word for word: "Air" is not "Airbourne", nor "Queen" "Queens of…".
fn artist_matches(ours: &str, theirs: Option<&DeezerArtist>) -> bool {
    let Some(theirs) = theirs.map(|a| a.name.as_str()) else {
        return false;
    };
    let (ours_words, theirs_words) = (words(ours), words(theirs));
    if ours_words.is_empty() || theirs_words.is_empty() {
        // "!!!" has no words: only the same name will do.
        let raw = |s: &str| s.trim().to_lowercase();
        return !raw(ours).is_empty() && raw(ours) == raw(theirs);
    }
    ours_words.starts_with(&theirs_words)
}

/// The title without a bracketed tail: "Creep (Remastered 2009)" → "Creep".
fn base_title(s: &str) -> String {
    let cut = s.find(['(', '[']).unwrap_or(s.len());
    let base = s[..cut].trim();
    let base = base.split(" - ").next().unwrap_or(base).trim();
    if base.is_empty() {
        s.to_string()
    } else {
        base.to_string()
    }
}

/// Lowercase words of letters and digits, accents dropped: "Beyoncé feat. X" → ["beyonce", "feat", "x"].
fn words(s: &str) -> Vec<String> {
    let folded: String = s
        .to_lowercase()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .collect();
    folded
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Lowercase letters and digits only, accents dropped: "Beyoncé" → "beyonce".
fn fold(s: &str) -> String {
    s.to_lowercase()
        .nfd()
        .filter(|c| !is_combining_mark(*c))
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::LyricsError;
    use futures::future::BoxFuture;
    use std::sync::Mutex;

    struct FakeHttp {
        answers: Vec<(&'static str, Result<String, u16>)>,
        asked: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn new(answers: Vec<(&'static str, Result<String, u16>)>) -> Self {
            Self {
                answers,
                asked: Mutex::new(vec![]),
            }
        }
    }

    impl LyricsHttp for FakeHttp {
        fn get(&self, url: &str) -> BoxFuture<'_, Result<String, LyricsError>> {
            self.asked.lock().unwrap().push(url.to_string());
            let hit = self
                .answers
                .iter()
                .find(|(p, _)| url.contains(p))
                .map(|(_, r)| r.clone());
            Box::pin(async move {
                match hit {
                    Some(Ok(body)) => Ok(body),
                    Some(Err(status)) => Err(LyricsError::Http {
                        status,
                        body: String::new(),
                    }),
                    None => Err(LyricsError::Http {
                        status: 404,
                        body: String::new(),
                    }),
                }
            })
        }
    }

    const SEARCH: &str = r#"{"data":[
        {"id":16921423,"title":"Beyoncé - Halo","title_short":"Beyoncé - Halo","duration":263,"artist":{"name":"Deborah Gonçalves"}},
        {"id":7512203,"title":"Halo (Live)","title_short":"Halo","duration":392,"artist":{"name":"Beyoncé"}},
        {"id":2553265,"title":"Halo","title_short":"Halo","duration":262,"artist":{"name":"Beyoncé"}}
    ],"total":3}"#;

    #[test]
    fn picks_the_same_song_by_the_same_artist() {
        assert_eq!(
            pick_track(SEARCH, "Beyonce", "Halo", Some(261_000)),
            Some(2553265)
        );
        assert_eq!(
            pick_track(SEARCH, "Beyoncé feat. Someone", "Halo", None),
            Some(2553265)
        );
    }

    #[test]
    fn a_remaster_matches_but_a_different_length_does_not() {
        assert_eq!(
            pick_track(SEARCH, "Beyoncé", "Halo (Remastered)", Some(262_000)),
            Some(2553265)
        );
        let live_only = r#"{"data":[{"id":7512203,"title":"Halo (Live)","title_short":"Halo","duration":392,"artist":{"name":"Beyoncé"}}]}"#;
        assert_eq!(
            pick_track(live_only, "Beyoncé", "Halo", Some(262_000)),
            None
        );
    }

    #[test]
    fn artists_match_word_for_word() {
        let deezer = |name: &str| DeezerArtist { name: name.into() };
        assert!(artist_matches(
            "Beyoncé feat. Jay-Z",
            Some(&deezer("Beyonce"))
        ));
        assert!(artist_matches("AC/DC", Some(&deezer("AC/DC"))));
        assert!(!artist_matches("Airbourne", Some(&deezer("Air"))));
        assert!(!artist_matches(
            "Queen",
            Some(&deezer("Queens of the Stone Age"))
        ));
        assert!(!artist_matches("Muse", Some(&deezer("Museum"))));
        assert!(artist_matches("!!!", Some(&deezer("!!!"))));
        assert!(!artist_matches("!!!", Some(&deezer("Beyoncé"))));
        assert!(!artist_matches("Beyoncé", Some(&deezer("!!!"))));
        assert!(!artist_matches("Beyoncé", None));
    }

    #[test]
    fn a_title_of_symbols_matches_nothing() {
        let body = r#"{"data":[{"id":1,"title":"♥","duration":200,"artist":{"name":"Beyoncé"}}]}"#;
        assert_eq!(pick_track(body, "Beyoncé", "?", None), None);
        assert_eq!(pick_album(body, "Beyoncé", "?"), None);
    }

    #[test]
    fn nothing_from_another_artist_or_another_title() {
        assert_eq!(pick_track(SEARCH, "Radiohead", "Halo", None), None);
        assert_eq!(pick_track(SEARCH, "Beyoncé", "Crazy in Love", None), None);
        assert_eq!(pick_track(r#"{"data":[]}"#, "Beyoncé", "Halo", None), None);
        assert_eq!(pick_track("not json", "Beyoncé", "Halo", None), None);
    }

    #[test]
    fn picks_the_album_not_the_anniversary_or_the_tribute() {
        let body = r#"{"data":[
            {"id":1103308,"title":"Strung Out On OK Computer","artist":{"name":"Vitamin String Quartet"}},
            {"id":43197211,"title":"OK Computer OKNOTOK 1997 2017","artist":{"name":"Radiohead"}},
            {"id":14879699,"title":"OK Computer","artist":{"name":"Radiohead"}}
        ]}"#;
        assert_eq!(pick_album(body, "Radiohead", "OK Computer"), Some(14879699));
        assert_eq!(pick_album(body, "Radiohead", "Kid A"), None);
    }

    #[test]
    fn isrc_answer_or_error() {
        assert_eq!(
            parse_isrc_track(r#"{"id":138547415,"title":"Creep"}"#),
            Some(138547415)
        );
        assert_eq!(
            parse_isrc_track(
                r#"{"error":{"type":"DataException","message":"no data","code":800}}"#
            ),
            None
        );
        assert_eq!(parse_isrc_track("<html>"), None);
    }

    #[tokio::test]
    async fn isrc_first_then_search() {
        let http = FakeHttp::new(vec![(
            "track/isrc:GBAYE9200070",
            Ok(r#"{"id":138547415}"#.into()),
        )]);
        let song = SongFacts {
            title: "Creep".into(),
            artist: Some("Radiohead".into()),
            duration_ms: Some(238_000),
            isrc: vec!["GB-AYE-92-00070".into()],
        };
        assert_eq!(
            song_url(&http, &song).await.as_deref(),
            Some("https://song.link/d/138547415")
        );
        assert_eq!(http.asked.lock().unwrap().len(), 1);

        let http = FakeHttp::new(vec![
            ("track/isrc:", Ok(r#"{"error":{"code":800}}"#.into())),
            ("/search?", Ok(SEARCH.into())),
        ]);
        let song = SongFacts {
            title: "Halo".into(),
            artist: Some("Beyoncé".into()),
            duration_ms: Some(262_000),
            isrc: vec!["USSM10804556".into(), "../bad".into()],
        };
        assert_eq!(
            song_url(&http, &song).await.as_deref(),
            Some("https://song.link/d/2553265")
        );
        let asked = http.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert_eq!(
            asked[1],
            "https://api.deezer.com/search?q=Beyonc%C3%A9+Halo&limit=25"
        );
    }

    #[tokio::test]
    async fn no_artist_or_no_network_is_none() {
        let http = FakeHttp::new(vec![]);
        let song = SongFacts {
            title: "Halo".into(),
            ..Default::default()
        };
        assert_eq!(song_url(&http, &song).await, None);
        let song = SongFacts {
            title: "Halo".into(),
            artist: Some("Beyoncé".into()),
            ..Default::default()
        };
        assert_eq!(song_url(&http, &song).await, None);
        assert_eq!(
            album_url(&http, "OK Computer", Some("Radiohead")).await,
            None
        );
        assert_eq!(album_url(&http, "OK Computer", None).await, None);
    }

    proptest::proptest! {
        #[test]
        fn parsers_never_panic(body in ".*", artist in ".*", title in ".*") {
            let _ = parse_isrc_track(&body);
            let _ = pick_track(&body, &artist, &title, Some(1));
            let _ = pick_album(&body, &artist, &title);
            let _ = base_title(&title);
            let _ = words(&artist);
        }
    }
}
