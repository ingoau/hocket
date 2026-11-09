//! When not to mix. [`should_mix`] is checked before planning; a `NoMix`
//! means the next source carries no plan and plays gapless (or cuts) as it
//! does with transitions off.
//!
//! Metadata the library has today: album id, disc and track number, genre
//! (one string, possibly a `;`/`,`/`/`-separated list), album title and the
//! album's OpenSubsonic release types. There is no gapless tag (`iTunPGAP`)
//! in the mirror yet, so [`MixTrack::gapless`] is `None` until the scanner
//! data carries it; the "joins without a gap" rule works from analysis
//! records instead (see [`is_gapless_join`]).

use crate::api::{AnalysisRecord, Track, TransitionMode};

/// The facts about one track the rules read.
#[derive(Debug, Clone, Copy, Default)]
pub struct MixTrack<'a> {
    pub album_id: Option<&'a str>,
    pub disc_number: Option<u32>,
    pub track_number: Option<u32>,
    pub genre: Option<&'a str>,
    pub album_title: Option<&'a str>,
    /// The album's release types, lowercased (`Album::release_types`).
    pub release_types: &'a [String],
    /// The file's gapless flag when known (`iTunPGAP`); `None` today.
    pub gapless: Option<bool>,
    /// The track's usable analysis record, if any.
    pub record: Option<&'a AnalysisRecord>,
}

impl<'a> MixTrack<'a> {
    /// From a library track. Release types and the record are added by the caller.
    pub fn from_track(t: &'a Track) -> Self {
        Self {
            album_id: t.album_id.as_deref(),
            disc_number: t.disc_number,
            track_number: t.track_number,
            genre: t.genre.as_deref(),
            album_title: t.album.as_deref(),
            ..Default::default()
        }
    }

    pub fn with_release_types(mut self, release_types: &'a [String]) -> Self {
        self.release_types = release_types;
        self
    }

    pub fn with_record(mut self, record: Option<&'a AnalysisRecord>) -> Self {
        self.record = record;
        self
    }
}

/// Playback state the rules depend on.
#[derive(Debug, Clone, Copy, Default)]
pub struct MixContext {
    pub mode: TransitionMode,
    pub repeat_one: bool,
    /// The sleep timer will stop at the end of the current track.
    pub sleep_end_of_track: bool,
    /// A Cast renderer is playing.
    pub cast_active: bool,
    /// Bit-perfect / exclusive output.
    pub exclusive_output: bool,
}

/// Genre families that are never mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenreClass {
    Classical,
    SpokenWord,
    Live,
    Soundtrack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoMixReason {
    ModeOff,
    RepeatOne,
    SleepEndOfTrack,
    CastActive,
    ExclusiveOutput,
    /// The next track follows on the same album in track order.
    AlbumSequence,
    /// A file is tagged gapless.
    GaplessTag,
    /// The pair runs into each other with no silence (from the records).
    GaplessJoin,
    Genre(GenreClass),
    /// A live release (release type or album-title marker).
    LiveRelease,
    /// A soundtrack or spoken-word release type.
    ReleaseType(GenreClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixDecision {
    Mix,
    NoMix(NoMixReason),
}

impl MixDecision {
    pub fn is_mix(self) -> bool {
        self == MixDecision::Mix
    }
}

/// Whether `a` (outgoing) → `b` (incoming) may be mixed. The first rule that
/// matches wins, in the order: playback context, album sequence, gapless,
/// genre, release.
pub fn should_mix(a: &MixTrack<'_>, b: &MixTrack<'_>, ctx: &MixContext) -> MixDecision {
    use NoMixReason::*;
    let no = MixDecision::NoMix;
    if !ctx.mode.is_on() {
        return no(ModeOff);
    }
    if ctx.repeat_one {
        return no(RepeatOne);
    }
    if ctx.sleep_end_of_track {
        return no(SleepEndOfTrack);
    }
    if ctx.cast_active {
        return no(CastActive);
    }
    if ctx.exclusive_output {
        return no(ExclusiveOutput);
    }
    if is_album_sequence(a, b) {
        return no(AlbumSequence);
    }
    if a.gapless == Some(true) || b.gapless == Some(true) {
        return no(GaplessTag);
    }
    if let (Some(ra), Some(rb)) = (a.record, b.record) {
        if is_gapless_join(ra, rb) {
            return no(GaplessJoin);
        }
    }
    for t in [a, b] {
        if let Some(class) = t.genre.and_then(genre_class) {
            return no(Genre(class));
        }
    }
    for t in [a, b] {
        if let Some(reason) = release_reason(t) {
            return no(reason);
        }
    }
    MixDecision::Mix
}

/// Same album id and `b` is the next track: same disc and track n+1, or the
/// next disc's track 1. A missing disc number counts as disc 1.
pub fn is_album_sequence(a: &MixTrack<'_>, b: &MixTrack<'_>) -> bool {
    let (Some(aa), Some(ba)) = (a.album_id, b.album_id) else {
        return false;
    };
    if aa != ba {
        return false;
    }
    let (Some(at), Some(bt)) = (a.track_number, b.track_number) else {
        return false;
    };
    let ad = a.disc_number.unwrap_or(1);
    let bd = b.disc_number.unwrap_or(1);
    (ad == bd && bt == at + 1) || (bd == ad + 1 && bt == 1)
}

/// Tolerance for "no gap" at the join.
const JOIN_EDGE_MS: u32 = 50;
/// How loud (relative to the track) an end must be to count as running on.
const JOIN_LEVEL_LU: f32 = 10.0;

/// A segue: `a` is still audible at its very end, at a level near its body,
/// and `b` is audible from its very start likewise. Mixing that pair would
/// smear a join the artist made deliberately.
pub fn is_gapless_join(a: &AnalysisRecord, b: &AnalysisRecord) -> bool {
    if a.duration_ms.saturating_sub(a.audible_end_ms) > JOIN_EDGE_MS
        || b.audible_start_ms > JOIN_EDGE_MS
    {
        return false;
    }
    let loud = |r: &AnalysisRecord, v: Option<&f32>| match (r.integrated_lufs, v) {
        (Some(i), Some(&v)) => v >= i - JOIN_LEVEL_LU,
        _ => false,
    };
    loud(a, a.tail.lufs.last()) && loud(b, b.head.lufs.first())
}

fn words(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
}

/// The no-mix family of a genre string, if any. Matches whole words, so
/// "Hardcore" is not "score" and "Alive" is not "live".
pub fn genre_class(genre: &str) -> Option<GenreClass> {
    let ws: Vec<String> = words(genre).collect();
    let has = |w: &str| ws.iter().any(|x| x == w);
    let has_pair = |a: &str, b: &str| ws.windows(2).any(|p| p[0] == a && p[1] == b);
    if has("classical") || has("opera") || has("orchestral") || has("baroque") {
        return Some(GenreClass::Classical);
    }
    if has("audiobook")
        || has("audiobooks")
        || has("podcast")
        || has("podcasts")
        || has("spokenword")
        || has_pair("spoken", "word")
    {
        return Some(GenreClass::SpokenWord);
    }
    if has("live") {
        return Some(GenreClass::Live);
    }
    if has("soundtrack") || has("soundtracks") || has("score") || has("ost") {
        return Some(GenreClass::Soundtrack);
    }
    None
}

fn release_reason(t: &MixTrack<'_>) -> Option<NoMixReason> {
    for rt in t.release_types {
        let rt: String = rt
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        match rt.as_str() {
            "live" => return Some(NoMixReason::LiveRelease),
            "soundtrack" => return Some(NoMixReason::ReleaseType(GenreClass::Soundtrack)),
            "spokenword" | "audiobook" | "audiodrama" | "interview" => {
                return Some(NoMixReason::ReleaseType(GenreClass::SpokenWord))
            }
            _ => {}
        }
    }
    if t.album_title.is_some_and(has_live_marker) {
        return Some(NoMixReason::LiveRelease);
    }
    None
}

/// "Album (Live)", "[Live at X]", "Live at Wembley", "Live in Paris",
/// "Name - Live", "Name Live": the album-title markers of a live release.
pub fn has_live_marker(title: &str) -> bool {
    let t = title.to_lowercase();
    let ws: Vec<String> = words(&t).collect();
    if t.contains("(live") || t.contains("[live") || t.contains("- live") {
        return true;
    }
    if ws.last().is_some_and(|w| w == "live") {
        return true;
    }
    ws.windows(2)
        .any(|p| p[0] == "live" && matches!(p[1].as_str(), "at" | "in" | "from" | "on"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MixContext {
        MixContext {
            mode: TransitionMode::AutoMix,
            ..Default::default()
        }
    }

    fn t(album: &'static str, disc: u32, n: u32) -> MixTrack<'static> {
        MixTrack {
            album_id: Some(album),
            disc_number: Some(disc),
            track_number: Some(n),
            ..Default::default()
        }
    }

    #[test]
    fn mixes_unrelated_tracks() {
        assert_eq!(
            should_mix(&t("x", 1, 3), &t("y", 1, 4), &ctx()),
            MixDecision::Mix
        );
        assert!(should_mix(&MixTrack::default(), &MixTrack::default(), &ctx()).is_mix());
    }

    #[test]
    fn context_rules() {
        let (a, b) = (t("x", 1, 1), t("y", 1, 1));
        let check = |c: MixContext, r: NoMixReason| {
            assert_eq!(should_mix(&a, &b, &c), MixDecision::NoMix(r));
        };
        check(MixContext::default(), NoMixReason::ModeOff);
        check(
            MixContext {
                repeat_one: true,
                ..ctx()
            },
            NoMixReason::RepeatOne,
        );
        check(
            MixContext {
                sleep_end_of_track: true,
                ..ctx()
            },
            NoMixReason::SleepEndOfTrack,
        );
        check(
            MixContext {
                cast_active: true,
                ..ctx()
            },
            NoMixReason::CastActive,
        );
        check(
            MixContext {
                exclusive_output: true,
                ..ctx()
            },
            NoMixReason::ExclusiveOutput,
        );
        let crossfade = MixContext {
            mode: TransitionMode::Crossfade,
            ..Default::default()
        };
        assert!(should_mix(&a, &b, &crossfade).is_mix());
    }

    #[test]
    fn album_sequence() {
        let seq = MixDecision::NoMix(NoMixReason::AlbumSequence);
        assert_eq!(should_mix(&t("x", 1, 3), &t("x", 1, 4), &ctx()), seq);
        assert_eq!(should_mix(&t("x", 1, 12), &t("x", 2, 1), &ctx()), seq);
        // Out of order, or another album: mix.
        assert!(should_mix(&t("x", 1, 4), &t("x", 1, 3), &ctx()).is_mix());
        assert!(should_mix(&t("x", 1, 3), &t("x", 1, 5), &ctx()).is_mix());
        assert!(should_mix(&t("x", 1, 3), &t("x", 2, 4), &ctx()).is_mix());
        assert!(should_mix(&t("x", 1, 3), &t("y", 1, 4), &ctx()).is_mix());
        // Missing disc numbers count as disc 1.
        let mut a = t("x", 1, 3);
        a.disc_number = None;
        assert_eq!(should_mix(&a, &t("x", 1, 4), &ctx()), seq);
    }

    #[test]
    fn genres() {
        use GenreClass::*;
        for (g, c) in [
            ("Classical", Some(Classical)),
            ("Opera; Vocal", Some(Classical)),
            ("Orchestral", Some(Classical)),
            ("Audiobook", Some(SpokenWord)),
            ("Spoken Word", Some(SpokenWord)),
            ("Podcast", Some(SpokenWord)),
            ("Live", Some(Live)),
            ("Rock/Live", Some(Live)),
            ("Soundtrack", Some(Soundtrack)),
            ("Film Score", Some(Soundtrack)),
            ("Hardcore", None),
            ("Alive Rock", None),
            ("House", None),
            ("Electronic, Techno", None),
        ] {
            assert_eq!(genre_class(g), c, "{g}");
        }
        let mut b = t("y", 1, 1);
        b.genre = Some("Classical");
        assert_eq!(
            should_mix(&t("x", 1, 1), &b, &ctx()),
            MixDecision::NoMix(NoMixReason::Genre(Classical))
        );
    }

    #[test]
    fn live_and_release_types() {
        for title in [
            "Stop Making Sense (Live)",
            "Alive [Live at Budokan]",
            "Live at Wembley",
            "Live in Paris 1975",
            "MTV Unplugged - Live",
            "Unplugged Live",
        ] {
            assert!(has_live_marker(title), "{title}");
        }
        for title in ["Alive", "Live Through This Twice", "Deliver", "Olive Tree"] {
            assert!(!has_live_marker(title), "{title}");
        }
        let types = vec!["album".to_string(), "live".to_string()];
        let a = t("x", 1, 1).with_release_types(&types);
        assert_eq!(
            should_mix(&a, &t("y", 1, 1), &ctx()),
            MixDecision::NoMix(NoMixReason::LiveRelease)
        );
        let types = vec!["Soundtrack".to_string()];
        let b = t("y", 1, 1).with_release_types(&types);
        assert_eq!(
            should_mix(&t("x", 1, 1), &b, &ctx()),
            MixDecision::NoMix(NoMixReason::ReleaseType(GenreClass::Soundtrack))
        );
        let mut c = t("z", 1, 1);
        c.album_title = Some("Live at the Apollo");
        assert_eq!(
            should_mix(&t("x", 1, 1), &c, &ctx()),
            MixDecision::NoMix(NoMixReason::LiveRelease)
        );
    }

    #[test]
    fn gapless_tag_and_join() {
        let mut a = t("x", 1, 1);
        a.gapless = Some(true);
        assert_eq!(
            should_mix(&a, &t("y", 1, 1), &ctx()),
            MixDecision::NoMix(NoMixReason::GaplessTag)
        );

        let rec = |end_gap: u32, start: u32, edge_lufs: f32| AnalysisRecord {
            duration_ms: 200_000,
            audible_start_ms: start,
            audible_end_ms: 200_000 - end_gap,
            integrated_lufs: Some(-10.0),
            head: crate::api::LoudnessCurve {
                start_ms: 0,
                hop_ms: 100,
                lufs: vec![edge_lufs, -10.0],
            },
            tail: crate::api::LoudnessCurve {
                start_ms: 140_000,
                hop_ms: 100,
                lufs: vec![-10.0, edge_lufs],
            },
            ..Default::default()
        };
        let segue = rec(0, 0, -12.0);
        let ra = MixTrack {
            record: Some(&segue),
            ..t("x", 1, 1)
        };
        let rb = MixTrack {
            record: Some(&segue),
            ..t("y", 1, 1)
        };
        assert_eq!(
            should_mix(&ra, &rb, &ctx()),
            MixDecision::NoMix(NoMixReason::GaplessJoin)
        );
        // A fade-out end, or a gap, is not a join.
        let faded = rec(0, 0, -40.0);
        assert!(!is_gapless_join(&faded, &segue));
        let gap = rec(2_000, 0, -12.0);
        assert!(!is_gapless_join(&gap, &segue));
        let late = rec(0, 800, -12.0);
        assert!(!is_gapless_join(&segue, &late));
    }

    #[test]
    fn from_track_reads_library_fields() {
        let track = Track {
            album_id: Some("al".into()),
            disc_number: Some(2),
            track_number: Some(7),
            genre: Some("Jazz".into()),
            album: Some("Kind of Blue".into()),
            ..Default::default()
        };
        let m = MixTrack::from_track(&track);
        assert_eq!(m.album_id, Some("al"));
        assert_eq!(m.disc_number, Some(2));
        assert_eq!(m.track_number, Some(7));
        assert_eq!(m.genre, Some("Jazz"));
        assert_eq!(m.album_title, Some("Kind of Blue"));
        assert_eq!(m.gapless, None);
    }
}
