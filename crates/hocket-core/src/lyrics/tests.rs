use futures::future::BoxFuture;

use super::*;
use crate::api::{LyricLine, LyricsTier, Track};

const KO_ENHANCED: &str = r#"{
  "subsonic-response": { "status": "ok", "version": "1.16.1", "lyricsList": { "structuredLyrics": [ {
  "kind": "main", "lang": "ko", "synced": true,
  "line": [ { "start": 2747, "value": "눈을 뜬 순간" }, { "start": 6214, "value": "모든 게 달라졌어" } ],
  "cueLine": [
    { "index": 0, "start": 2747, "end": 6214, "value": "눈을 뜬 순간", "cue": [
        { "start": 2747, "end": 3018, "value": "눈", "byteStart": 0, "byteEnd": 2 },
        { "start": 3018, "end": 3179, "value": "을", "byteStart": 3, "byteEnd": 5 },
        { "start": 3179, "end": 3582, "value": " ", "byteStart": 6, "byteEnd": 6 },
        { "start": 3582, "end": 4100, "value": "뜬", "byteStart": 7, "byteEnd": 9 },
        { "start": 4100, "end": 4500, "value": " ", "byteStart": 10, "byteEnd": 10 },
        { "start": 4500, "end": 5200, "value": "순", "byteStart": 11, "byteEnd": 13 },
        { "start": 5200, "end": 6214, "value": "간", "byteStart": 14, "byteEnd": 16 } ] },
    { "index": 1, "start": 6214, "end": 9000, "value": "모든 게 달라졌어", "cue": [
        { "start": 6214, "end": 6800, "value": "모", "byteStart": 0, "byteEnd": 2 },
        { "start": 6800, "end": 7200, "value": "든", "byteStart": 3, "byteEnd": 5 },
        { "start": 7200, "end": 7600, "value": " ", "byteStart": 6, "byteEnd": 6 },
        { "start": 7600, "end": 8000, "value": "게", "byteStart": 7, "byteEnd": 9 },
        { "start": 8000, "end": 8400, "value": " ", "byteStart": 10, "byteEnd": 10 },
        { "start": 8400, "end": 9000, "value": "달라졌어", "byteStart": 11, "byteEnd": 22 } ] } ] },
  { "kind": "pronunciation", "lang": "ko-Latn", "synced": true,
    "line": [ { "start": 2747, "value": "nuneul tteun sungan" }, { "start": 6214, "value": "modeun ge dallajyeosseo" } ] },
  { "kind": "translation", "lang": "en", "synced": true,
    "line": [ { "start": 2747, "value": "The moment I opened my eyes" }, { "start": 6214, "value": "Everything changed" } ] }
  ] } } }"#;

const AGENTS: &str = r#"{ "lyricsList": { "structuredLyrics": [ {
  "kind": "main", "lang": "eng", "synced": true,
  "line": [ { "start": 1000, "value": "You and I" }, { "start": 4000, "value": "Under this sky" }, { "start": 7000, "value": "Together tonight" }, { "start": 7500, "value": "(tonight)" } ],
  "agents": [ { "id": "lead", "role": "main", "name": "Chris Martin" }, { "id": "guest", "role": "voice", "name": "Jin" }, { "id": "choir", "role": "group", "name": "All" }, { "id": "bgv", "role": "bg" } ],
  "cueLine": [
    { "index": 0, "agentId": "lead", "start": 1000, "end": 4000, "value": "You and I", "cue": [
        { "start": 1000, "end": 1800, "value": "You ", "byteStart": 0, "byteEnd": 3 },
        { "start": 1800, "end": 2400, "value": "and ", "byteStart": 4, "byteEnd": 7 },
        { "start": 2400, "end": 3200, "value": "I", "byteStart": 8, "byteEnd": 8 } ] },
    { "index": 1, "agentId": "guest", "start": 4000, "end": 7000, "value": "Under this sky", "cue": [
        { "start": 4000, "end": 4800, "value": "Un", "byteStart": 0, "byteEnd": 1 },
        { "start": 4800, "end": 5400, "value": "der ", "byteStart": 2, "byteEnd": 5 },
        { "start": 5400, "end": 5900, "value": "this ", "byteStart": 6, "byteEnd": 10 },
        { "start": 5900, "end": 7000, "value": "sky", "byteStart": 11, "byteEnd": 13 } ] },
    { "index": 2, "agentId": "choir", "start": 7000, "end": 10000, "value": "Together tonight", "cue": [
        { "start": 7000, "end": 8000, "value": "To", "byteStart": 0, "byteEnd": 1 },
        { "start": 8000, "end": 8800, "value": "ge", "byteStart": 2, "byteEnd": 3 },
        { "start": 8800, "end": 9200, "value": "ther ", "byteStart": 4, "byteEnd": 8 },
        { "start": 9200, "end": 10000, "value": "tonight", "byteStart": 9, "byteEnd": 15 } ] },
    { "index": 3, "agentId": "bgv", "start": 7500, "end": 9500, "value": "(tonight)", "cue": [
        { "start": 7500, "value": "(to" }, { "end": 9500, "value": "night)" } ] }
  ] } ] } }"#;

const V1_LINE: &str = r#"{ "subsonic-response": { "lyricsList": { "structuredLyrics": [ {
  "displayArtist": "Muse", "displayTitle": "Hysteria", "lang": "xxx", "offset": -100, "synced": true,
  "line": [ { "start": 0, "value": "It's bugging me" }, { "start": 2000, "value": "Grating me" }, { "start": 3001, "value": "And twisting me around..." } ] } ] } } }"#;

#[test]
fn parses_raw_shapes_and_picks_main() {
    let r = LyricsListResponse::parse(KO_ENHANCED).unwrap();
    assert_eq!(r.entries().len(), 3);
    assert_eq!(r.entries()[0].cue_line[0].cue.len(), 7);
    assert_eq!(pick_main(r.entries()).unwrap().lang, "ko");
    let bare = LyricsListResponse::parse(AGENTS).unwrap();
    assert_eq!(bare.entries()[0].agents.len(), 4);
    let empty =
        LyricsListResponse::parse(r#"{"subsonic-response":{"status":"ok","lyricsList":{}}}"#)
            .unwrap();
    assert!(empty.entries().is_empty());
    assert!(adapt_response("t", &empty).is_none());
    let nothing = LyricsListResponse::parse(r#"{"subsonic-response":{"status":"ok"}}"#).unwrap();
    assert!(adapt_response("t", &nothing).is_none());
}

#[test]
fn syllable_tier_with_joins_and_translation() {
    let r = LyricsListResponse::parse(KO_ENHANCED).unwrap();
    let l = adapt_response("t1", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Syllable);
    assert_eq!(l.lang.as_deref(), Some("ko"));
    assert_eq!(l.lines.len(), 2);
    let first = &l.lines[0];
    assert_eq!(first.start_ms, Some(2747));
    assert_eq!(first.end_ms, Some(6214));
    let texts: Vec<&str> = first.syllables.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, vec!["눈", "을", "뜬", "순", "간"]);
    let joined: Vec<bool> = first.syllables.iter().map(|s| s.joined).collect();
    // 눈+을 joined, space, 뜬 alone, space, 순+간 joined, last never joined.
    assert_eq!(joined, vec![true, false, false, true, false]);
    assert_eq!(first.syllables[1].end_ms, 3179);
    assert_eq!(
        first.translation.as_deref(),
        Some("The moment I opened my eyes")
    );
    assert_eq!(l.lines[1].syllables.last().unwrap().text, "달라졌어");
    assert_eq!(l.lines[1].end_ms, Some(9000));
}

#[test]
fn agents_sides_and_background() {
    let r = LyricsListResponse::parse(AGENTS).unwrap();
    let l = adapt_response("t2", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Syllable);
    let sides: Vec<(&str, u32)> = l.agents.iter().map(|a| (a.id.as_str(), a.side)).collect();
    assert_eq!(
        sides,
        vec![("lead", 0), ("guest", 1), ("choir", 0), ("bgv", 0)]
    );
    assert_eq!(l.agents[0].name.as_deref(), Some("Chris Martin"));
    assert_eq!(l.lines[0].agent.as_deref(), Some("lead"));
    assert!(!l.lines[0].background);
    // "You " / "and " have trailing spaces: not joined.
    assert!(l.lines[0].syllables.iter().all(|s| !s.joined));
    assert_eq!(l.lines[0].syllables[0].text, "You");
    // "Un" + "der " → joined then not.
    assert!(l.lines[1].syllables[0].joined);
    assert!(!l.lines[1].syllables[1].joined);
    // Background line: flagged, and its cue timing is incomplete → no syllables (never fabricated).
    let bg = &l.lines[3];
    assert!(bg.background);
    assert_eq!(bg.agent.as_deref(), Some("bgv"));
    assert!(bg.syllables.is_empty());
    assert_eq!(bg.start_ms, Some(7500));
    assert_eq!(bg.end_ms, Some(9500));
}

#[test]
fn line_tier_with_server_offset_and_unsynced() {
    let r = LyricsListResponse::parse(V1_LINE).unwrap();
    let l = adapt_response("t3", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Line);
    assert_eq!(l.lang, None);
    assert_eq!(l.display_artist.as_deref(), Some("Muse"));
    // offset -100 (LRC convention: lyrics are late) → timestamps move later by 100.
    assert_eq!(l.lines[0].start_ms, Some(100));
    assert_eq!(l.lines[1].start_ms, Some(2100));
    assert_eq!(l.lines[0].end_ms, Some(2100));
    assert_eq!(l.lines[2].end_ms, None);
    assert!(l.lines.iter().all(|x| x.syllables.is_empty()));

    let unsynced = LyricsListResponse::parse(r#"{"lyricsList":{"structuredLyrics":[{"lang":"und","synced":false,"line":[{"value":"a"},{"value":"b"}]}]}}"#).unwrap();
    let u = adapt_response("t4", &unsynced).unwrap();
    assert_eq!(u.tier, LyricsTier::Unsynced);
    assert!(u.lines.iter().all(|x| x.start_ms.is_none()));
    assert_eq!(
        locate(&u, 5000.0),
        CursorState {
            effective_ms: 5000.0,
            ..Default::default()
        }
    );
}

#[test]
fn synced_entry_preferred_over_unsynced_main() {
    let r = LyricsListResponse::parse(
        r#"{"lyricsList":{"structuredLyrics":[{"lang":"en","synced":false,"line":[{"value":"plain"}]},{"lang":"en","synced":true,"line":[{"start":10,"value":"timed"}]}]}}"#,
    )
    .unwrap();
    let l = adapt_response("t", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Line);
    assert_eq!(l.lines[0].text, "timed");
}

#[test]
fn cursor_walks_lines_and_syllables() {
    let r = LyricsListResponse::parse(KO_ENHANCED).unwrap();
    let l = adapt_response("t1", &r).unwrap();
    let mut c = LyricsCursor::new();
    let s = c.locate(&l, 0.0);
    assert_eq!(s.line, None);
    assert!(s.active_lines.is_empty());
    let s = c.locate(&l, 2747.0);
    assert_eq!(s.line, Some(0));
    assert_eq!(s.syllable, Some(0));
    assert_eq!(s.syllable_progress, 0.0);
    let s = c.locate(&l, 3100.0);
    assert_eq!((s.line, s.syllable), (Some(0), Some(1)));
    assert!((s.syllable_progress - (3100.0 - 3018.0) / (3179.0 - 3018.0)).abs() < 1e-5);
    let s = c.locate(&l, 3300.0); // in the gap cue, still on 을 which has ended
    assert_eq!(s.syllable, Some(1));
    assert_eq!(s.syllable_progress, 1.0);
    let s = c.locate(&l, 6214.0);
    assert_eq!(s.line, Some(1));
    assert_eq!(s.active_lines, vec![1]);
    assert_eq!(s.syllable, Some(0));
    let s = c.locate(&l, 20_000.0);
    assert_eq!(s.line, Some(1));
    assert!(s.line_ended);
    assert_eq!(s.line_progress, 1.0);
    assert_eq!(s.syllable, Some(3));
    assert_eq!(s.syllable_progress, 1.0);
    // Seek backwards.
    let s = c.locate(&l, 4600.0);
    assert_eq!((s.line, s.syllable), (Some(0), Some(3)));
    assert_eq!(locate(&l, 4600.0), s);
}

#[test]
fn cursor_reports_overlapping_lines_and_offsets() {
    let r = LyricsListResponse::parse(AGENTS).unwrap();
    let mut l = adapt_response("t2", &r).unwrap();
    let s = locate(&l, 8000.0);
    assert_eq!(s.line, Some(3)); // background line started last
    assert_eq!(s.active_lines, vec![2, 3]);
    set_user_offset(&mut l, 1000); // show lyrics 1 s later
    let s = locate(&l, 8000.0);
    assert_eq!(s.effective_ms, 7000.0);
    assert_eq!(s.line, Some(2));
    assert_eq!(s.active_lines, vec![2]);
    let baked = apply_offset(&l);
    assert_eq!(baked.offset_ms, 0);
    assert_eq!(baked.lines[2].start_ms, Some(8000));
    assert_eq!(baked.lines[2].syllables[0].start_ms, 8000);
    assert_eq!(locate(&baked, 8000.0).line, Some(2));
    // Negative offset clamps at zero rather than wrapping.
    set_user_offset(&mut l, -2000);
    assert_eq!(apply_offset(&l).lines[0].start_ms, Some(0));
}

#[test]
fn lrc_parsing() {
    let doc = parse_lrc("[ar:Muse]\n[offset:+200]\n[00:41.16] It's bugging me\n[00:43.62][01:32.03] Grating me\n\n[00:45.9]And <00:46.00>twisting <00:46.50>me around\nno timestamp here\n[01:07.040]Give me your heart\n");
    assert_eq!(doc.offset_ms, 200);
    let got: Vec<(u32, &str)> = doc
        .lines
        .iter()
        .map(|l| (l.start_ms, l.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            (41_160, "It's bugging me"),
            (43_620, "Grating me"),
            (45_900, "And twisting me around"),
            (67_040, "Give me your heart"),
            (92_030, "Grating me")
        ]
    );
    let l = lrc_to_lyrics("t", &doc, crate::api::LyricsSource::External).unwrap();
    assert_eq!(l.tier, LyricsTier::Line);
    assert_eq!(l.lines[0].start_ms, Some(40_960));
    assert_eq!(l.lines[0].end_ms, Some(43_420));
    assert!(parse_lrc("just text\nmore text").lines.is_empty());
    assert!(lrc_to_lyrics("t", &parse_lrc(""), crate::api::LyricsSource::External).is_none());
}

struct FakeHttp {
    responses: std::collections::HashMap<String, Result<String, u16>>,
    seen: parking_lot::Mutex<Vec<String>>,
}

impl LyricsHttp for FakeHttp {
    fn get(&self, url: &str) -> BoxFuture<'_, Result<String, LyricsError>> {
        let url = url.to_string();
        Box::pin(async move {
            self.seen.lock().push(url.clone());
            match self.responses.get(&url) {
                Some(Ok(body)) => Ok(body.clone()),
                Some(Err(status)) => Err(LyricsError::Http {
                    status: *status,
                    body: String::new(),
                }),
                None => Err(LyricsError::Network(format!("unexpected {url}"))),
            }
        })
    }
}

#[tokio::test]
async fn lrclib_provider_maps_responses() {
    let req = LyricsRequest {
        track_id: "t".into(),
        title: "Hysteria".into(),
        artist: Some("Muse".into()),
        album: Some("Absolution".into()),
        duration_ms: Some(227_400),
    };
    let mk = |http: FakeHttp| LrclibProvider::with_base_url(http, "https://lrclib.test/");
    let url = mk(FakeHttp {
        responses: Default::default(),
        seen: Default::default(),
    })
    .url_for(&req);
    assert_eq!(url, "https://lrclib.test/api/get?track_name=Hysteria&artist_name=Muse&album_name=Absolution&duration=227");

    let synced = r#"{"id":1,"trackName":"Hysteria","artistName":"Muse","albumName":"Absolution","duration":227.0,"instrumental":false,"plainLyrics":"It's bugging me\nGrating me","syncedLyrics":"[00:41.16] It's bugging me\n[00:43.62] Grating me"}"#;
    let p = mk(FakeHttp {
        responses: [(url.clone(), Ok(synced.to_string()))].into(),
        seen: Default::default(),
    });
    let l = p.fetch(&req).await.unwrap().unwrap();
    assert_eq!(l.tier, LyricsTier::Line);
    assert_eq!(l.source, crate::api::LyricsSource::External);
    assert_eq!(l.lines[1].start_ms, Some(43_620));
    assert_eq!(p.id(), "lrclib");

    let plain = r#"{"instrumental":false,"plainLyrics":"la la\nla","syncedLyrics":null}"#;
    let p = mk(FakeHttp {
        responses: [(url.clone(), Ok(plain.to_string()))].into(),
        seen: Default::default(),
    });
    let l = p.fetch(&req).await.unwrap().unwrap();
    assert_eq!(l.tier, LyricsTier::Unsynced);
    assert_eq!(l.lines.len(), 2);

    let p = mk(FakeHttp {
        responses: [(url.clone(), Ok(r#"{"instrumental":true}"#.to_string()))].into(),
        seen: Default::default(),
    });
    assert!(p.fetch(&req).await.unwrap().is_none());

    let p = mk(FakeHttp {
        responses: [(url.clone(), Err(404))].into(),
        seen: Default::default(),
    });
    assert!(p.fetch(&req).await.unwrap().is_none());

    let p = mk(FakeHttp {
        responses: [(url.clone(), Err(500))].into(),
        seen: Default::default(),
    });
    assert!(matches!(
        p.fetch(&req).await,
        Err(LyricsError::Http { status: 500, .. })
    ));

    let p = mk(FakeHttp {
        responses: [(url.clone(), Ok("not json".into()))].into(),
        seen: Default::default(),
    });
    assert!(matches!(p.fetch(&req).await, Err(LyricsError::Decode(_))));

    let p = mk(FakeHttp {
        responses: Default::default(),
        seen: Default::default(),
    });
    let blank = LyricsRequest {
        title: "  ".into(),
        ..req.clone()
    };
    assert!(p.fetch(&blank).await.unwrap().is_none());
}

#[test]
fn cache_helpers_round_trip() {
    let r = LyricsListResponse::parse(AGENTS).unwrap();
    let l = adapt_response("t2", &r).unwrap();
    let json = to_cache_json(Some(&l));
    assert_eq!(from_cache_json(&json).unwrap().unwrap(), l);
    assert_eq!(to_cache_json(None), "");
    assert!(from_cache_json("").unwrap().is_none());
    assert_eq!(
        cache_key("s", "t", crate::api::LyricsSource::External),
        ("s".into(), "t".into(), "external")
    );
    assert_eq!(offset_state_key("abc"), "lyricsOffset:abc");
    let _ = Track::default();
}

/// Real Navidrome 0.64 answer to `getLyricsBySongId?enhanced=true`: 44
/// `line[]`s but 57 `cueLine`s, the extra 13 being background vocals under a
/// `__nd_bg__|v1` agent (role `bg`) that share their `index` with the line
/// they are sung over. Cue values are trimmed words/syllables whose word
/// boundaries live in the inclusive `byteStart`/`byteEnd` offsets.
const NAVIDROME_ENHANCED: &str = include_str!("../subsonic/fixtures/lyrics_enhanced.json");

#[test]
fn navidrome_enhanced_fixture_adapts_to_syllables_with_background_sub_voices() {
    let r = LyricsListResponse::parse(NAVIDROME_ENHANCED).unwrap();
    assert_eq!(r.entries().len(), 1);
    let entry = &r.entries()[0];
    assert_eq!(entry.line.len(), 44);
    assert_eq!(entry.cue_line.len(), 57);
    let l = adapt_response("tally", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Syllable);
    assert_eq!(l.display_title.as_deref(), Some("Tally"));
    assert_eq!(l.lang.as_deref(), Some("en"));
    // v1 is the main voice (left); the bg agent shares its side.
    let agents: Vec<(&str, u32)> = l.agents.iter().map(|a| (a.id.as_str(), a.side)).collect();
    assert_eq!(agents, vec![("v1", 0), ("__nd_bg__|v1", 0)]);

    // Every line[] becomes one main line; every extra cue line a sub-voice.
    assert_eq!(l.lines.len(), 57);
    let main: Vec<&LyricLine> = l.lines.iter().filter(|x| !x.background).collect();
    let bg: Vec<&LyricLine> = l.lines.iter().filter(|x| x.background).collect();
    assert_eq!(main.len(), 44);
    assert_eq!(bg.len(), 13);
    assert!(main.iter().all(|x| x.agent.as_deref() == Some("v1")));
    assert!(bg
        .iter()
        .all(|x| x.agent.as_deref() == Some("__nd_bg__|v1")));
    let main_texts: Vec<&str> = main.iter().map(|x| x.text.as_str()).collect();
    assert_eq!(main_texts[0], "I lost my rank and title");
    assert_eq!(
        main_texts[2], "Sold it all at a discount",
        "the bg part is not glued onto the main line"
    );
    assert!(!main_texts.contains(&"(Yeah, yeah)"));
    assert_eq!(main_texts[43], "Yeah, yeah");

    // Word boundaries from byte offsets: "I" is 0..=0 and "lost" starts at 2
    // (byte 1 is the space) → separate words; "ti" 19..=20 + "tle" 21..=23 are
    // contiguous → one word.
    let first = &l.lines[0];
    let texts: Vec<&str> = first.syllables.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, vec!["I", "lost", "my", "rank", "and", "ti", "tle"]);
    let joined: Vec<bool> = first.syllables.iter().map(|s| s.joined).collect();
    assert_eq!(joined, vec![false, false, false, false, false, true, false]);
    assert_eq!(first.syllables[0].start_ms, 13750);
    assert_eq!(first.syllables[0].end_ms, 14041);
    assert_eq!(first.syllables[6].end_ms, 15350);
    let fourth = &l.lines[4]; // line[3] "I wanted to progress things" (line[2]'s bg sits at 3)
    assert_eq!(fourth.text, "I wanted to progress things");
    let joined: Vec<bool> = fourth.syllables.iter().map(|s| s.joined).collect();
    // I | wan+ted | to | pro+gress | things
    assert_eq!(joined, vec![false, true, false, false, true, false, false]);

    // Gaps are kept as the server gave them: line 0 ends before line 1
    // starts, nothing is stretched to fill.
    assert_eq!(first.end_ms, Some(15350));
    assert_eq!(l.lines[1].start_ms, Some(15428));

    // The sub-voice line follows the line it is sung over, starts when its
    // first word does (not at the parent's start) and has its own syllables.
    let sold = &l.lines[2];
    assert_eq!(sold.start_ms, Some(16966));
    assert_eq!(sold.end_ms, Some(20517));
    let yeah = &l.lines[3];
    assert!(yeah.background);
    assert_eq!(yeah.text, "(Yeah, yeah)");
    assert_eq!(yeah.start_ms, Some(19243));
    assert_eq!(yeah.end_ms, Some(20517));
    let texts: Vec<&str> = yeah.syllables.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, vec!["(Yeah,", "yeah)"]);
    assert!(yeah.syllables.iter().all(|s| !s.joined));

    // Cursor: before the bg part starts only the main line is active; once
    // it does, both are (the renderer stacks the sub-voice under the line).
    let s = locate(&l, 18_000.0);
    assert_eq!(s.line, Some(2));
    assert_eq!(s.active_lines, vec![2]);
    let s = locate(&l, 19_500.0);
    assert_eq!(s.active_lines, vec![2, 3]);
    // All main-line syllable timings are monotone within the line.
    for line in &l.lines {
        for w in line.syllables.windows(2) {
            assert!(w[0].end_ms <= w[1].start_ms, "{:?}", line.text);
        }
    }
}

/// A cue line with an empty `cue[]` renders that line (or sub-voice) at line
/// tier; nothing is split up for it, while the rest keeps syllables.
#[test]
fn cue_line_without_cues_falls_back_to_line_tier_for_that_line_only() {
    let json = r#"{ "lyricsList": { "structuredLyrics": [ {
      "kind": "main", "lang": "en", "synced": true,
      "line": [ { "start": 1000, "value": "Hello there (hey)" }, { "start": 4000, "value": "Goodbye" } ],
      "agents": [ { "id": "v1", "role": "main" }, { "id": "__nd_bg__|v1", "role": "bg" } ],
      "cueLine": [
        { "index": 0, "start": 1000, "end": 3500, "value": "Hello there", "agentId": "v1", "cue": [
            { "start": 1000, "end": 1400, "value": "Hel", "byteStart": 0, "byteEnd": 2 },
            { "start": 1400, "end": 1900, "value": "lo", "byteStart": 3, "byteEnd": 4 },
            { "start": 1900, "end": 3500, "value": "there", "byteStart": 6, "byteEnd": 10 } ] },
        { "index": 0, "start": 1000, "end": 3500, "value": "(hey)", "agentId": "__nd_bg__|v1", "cue": [] },
        { "index": 1, "start": 4000, "end": 6000, "value": "Goodbye", "agentId": "v1", "cue": [] }
    ] } ] } }"#;
    let r = LyricsListResponse::parse(json).unwrap();
    let l = adapt_response("t", &r).unwrap();
    assert_eq!(l.tier, LyricsTier::Syllable);
    assert_eq!(l.lines.len(), 3);
    let texts: Vec<&str> = l.lines[0]
        .syllables
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(texts, vec!["Hel", "lo", "there"]);
    let joined: Vec<bool> = l.lines[0].syllables.iter().map(|s| s.joined).collect();
    assert_eq!(joined, vec![true, false, false]);
    let hey = &l.lines[1];
    assert!(hey.background && hey.syllables.is_empty());
    assert_eq!(hey.text, "(hey)");
    // No timed word to start from: the cue line's own start is all there is.
    assert_eq!((hey.start_ms, hey.end_ms), (Some(1000), Some(3500)));
    let bye = &l.lines[2];
    assert!(!bye.background && bye.syllables.is_empty());
    assert_eq!((bye.start_ms, bye.end_ms), (Some(4000), Some(6000)));
}

/// Byte offsets are inclusive and count UTF-8 bytes; a join is decided by
/// the bytes between two cues, so multi-byte text and punctuation inside a
/// word both work, and offsets that do not match the line fall back to the
/// cues' own whitespace.
#[test]
fn byte_offsets_decide_joins_for_multibyte_text_and_punctuation() {
    let json = r#"{ "lyricsList": { "structuredLyrics": [ {
      "kind": "main", "lang": "fr", "synced": true,
      "line": [ { "start": 0, "value": "Ça va très bien" }, { "start": 5000, "value": "well-known café" }, { "start": 9000, "value": "a b" } ],
      "cueLine": [
        { "index": 0, "start": 0, "end": 4000, "value": "Ça va très bien", "cue": [
            { "start": 0, "end": 500, "value": "Ç", "byteStart": 0, "byteEnd": 1 },
            { "start": 500, "end": 1000, "value": "a", "byteStart": 2, "byteEnd": 2 },
            { "start": 1000, "end": 2000, "value": "va", "byteStart": 4, "byteEnd": 5 },
            { "start": 2000, "end": 3000, "value": "très", "byteStart": 7, "byteEnd": 11 },
            { "start": 3000, "end": 4000, "value": "bien", "byteStart": 13, "byteEnd": 16 } ] },
        { "index": 1, "start": 5000, "end": 8000, "value": "well-known café", "cue": [
            { "start": 5000, "end": 6000, "value": "well", "byteStart": 0, "byteEnd": 3 },
            { "start": 6000, "end": 7000, "value": "known", "byteStart": 5, "byteEnd": 9 },
            { "start": 7000, "end": 7500, "value": "ca", "byteStart": 11, "byteEnd": 12 },
            { "start": 7500, "end": 8000, "value": "fé", "byteStart": 13, "byteEnd": 15 } ] },
        { "index": 2, "start": 9000, "end": 9900, "value": "a b", "cue": [
            { "start": 9000, "end": 9500, "value": "a", "byteStart": 40, "byteEnd": 40 },
            { "start": 9500, "end": 9900, "value": "b", "byteStart": 42, "byteEnd": 42 } ] }
    ] } ] } }"#;
    let r = LyricsListResponse::parse(json).unwrap();
    let l = adapt_response("t", &r).unwrap();
    let words = |i: usize| -> Vec<(String, bool)> {
        l.lines[i]
            .syllables
            .iter()
            .map(|s| (s.text.clone(), s.joined))
            .collect()
    };
    assert_eq!(
        words(0),
        vec![
            ("Ç".into(), true),
            ("a".into(), false),
            ("va".into(), false),
            ("très".into(), false),
            ("bien".into(), false)
        ]
    );
    assert_eq!(
        words(1),
        vec![
            ("well-".into(), true),
            ("known".into(), false),
            ("ca".into(), true),
            ("fé".into(), false)
        ]
    );
    // Offsets that point outside the line: back to the whitespace rule, and
    // trimmed cues with no whitespace of their own read as one word.
    assert_eq!(words(2), vec![("a".into(), true), ("b".into(), false)]);
}
