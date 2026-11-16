//! YouTube Music analysis on the coordinator (docs/providers.md, Phase 4
//! follow-up): misses are queued by video id, the worker resolves its own
//! googlevideo URL (never a device's), analyses itag 140 and stores the
//! record under `AnalysisKey::ytm`; the owner can sign it in with a session
//! sealed to its key, nobody else can. InnerTube and googlevideo are a fake
//! transport answering from recorded fixtures; no network.

mod common;

use std::sync::Arc;

use parking_lot::Mutex;

use common::*;
use hocket_coordinator::analysis::AnalysisStore;
use hocket_coordinator::ytm::{YtmAnalysis, SESSION_FILE};
use hocket_coordinator::{serve, App, Args};
use hocket_core::connect::wire::{AnalysisKey, Msg, COORD_YTM_CONTEXT};
use hocket_core::identity::crypto::{envelope, DeviceSecret};
use hocket_core::ytm::fake::{fake, fx};
use hocket_core::ytm::http::{FakeYtmHttp, HttpRequest, HttpResponse};

const VIDEO: &str = "4D7u5KF7SP8";

/// Six seconds of 120 BPM clicks as a 16-bit mono WAV (what "googlevideo" serves).
fn click_wav(seconds: f64) -> Vec<u8> {
    let rate = 22_050u32;
    let n = (seconds * f64::from(rate)) as usize;
    let mut pcm = vec![0i16; n];
    for start in (0..n).step_by(rate as usize / 2) {
        for i in 0..(rate as usize / 20) {
            if start + i >= n {
                break;
            }
            let env = (-(i as f64) / (0.01 * f64::from(rate))).exp();
            let s = 0.8 * env * (2.0 * std::f64::consts::PI * 1_000.0 * i as f64 / 22_050.0).sin();
            pcm[start + i] = (s * 32_000.0) as i16;
        }
    }
    let data_len = (n * 2) as u32;
    let mut out = Vec::with_capacity(44 + n * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The TV app's answer: the visionOS one with its 140 format ciphered (`s`
/// into `sig`) and an `n` to transform, or `refused`.
fn tv_player(visionos: &serde_json::Value, refused: bool) -> serde_json::Value {
    if refused {
        return serde_json::json!({"playabilityStatus": {"status": "LOGIN_REQUIRED", "reason": "Sign in to confirm you're not a bot"}});
    }
    let mut tv = visionos.clone();
    for f in tv["streamingData"]["adaptiveFormats"]
        .as_array_mut()
        .unwrap()
    {
        if let Some(url) = f["url"].as_str().map(str::to_string) {
            let url = url.replace("c=VISIONOS", "c=TVHTML5") + "&n=abcd";
            let cipher = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("s", "SIGxyz")
                .append_pair("sp", "sig")
                .append_pair("url", &url)
                .finish();
            let o = f.as_object_mut().unwrap();
            o.remove("url");
            o.insert("signatureCipher".into(), cipher.into());
        }
    }
    tv
}

/// A player EJS has already preprocessed (the shape `jsc` reads back):
/// reverse the signature, upper-case `n`.
const PREPROCESSED: &str =
    "_result.sig = (s) => s.split('').reverse().join(''); _result.n = (n) => n.toUpperCase() + '_ok';";

/// InnerTube from the fixtures (the visionOS player answer, its 140 format
/// resized to `media`; the TV app's as [`tv_player`]), `iframe_api` naming
/// the player `fakeplay`, and googlevideo serving `media` in ranges. Logs
/// the googlevideo URLs fetched.
fn transport(media: Vec<u8>) -> (FakeYtmHttp, Arc<Mutex<Vec<String>>>) {
    transport_tv(media, false)
}

fn transport_tv(media: Vec<u8>, tv_refused: bool) -> (FakeYtmHttp, Arc<Mutex<Vec<String>>>) {
    let len = media.len();
    let mut player = fx("player_visionos");
    for f in player["streamingData"]["adaptiveFormats"]
        .as_array_mut()
        .unwrap()
    {
        if f["itag"] == 140 {
            f["contentLength"] = len.to_string().into();
            let url = f["url"]
                .as_str()
                .unwrap()
                .replace("clen=5983881", &format!("clen={len}"));
            f["url"] = url.into();
        }
    }
    let tv = tv_player(&player, tv_refused);
    let inner = fake(Arc::new(Mutex::new(vec![
        ("VISIONOS", player),
        ("TVHTML5", tv),
    ])));
    let fetched = Arc::new(Mutex::new(vec![]));
    let log = fetched.clone();
    let http = FakeYtmHttp::new(move |r: &HttpRequest| {
        if r.url == "https://www.youtube.com/iframe_api" {
            return Some(HttpResponse::json(
                200,
                r"var scriptUrl = 'https:\/\/www.youtube.com\/s\/player\/fakeplay\/www-widgetapi.vflset\/www-widgetapi.js';",
            ));
        }
        if !r.url.contains(".googlevideo.com/videoplayback") {
            return inner.answer(r);
        }
        log.lock().push(r.url.clone());
        let range = r.header("Range")?.strip_prefix("bytes=")?;
        let (a, b) = range.split_once('-')?;
        let a: usize = a.parse().ok()?;
        let b: usize = b.parse::<usize>().ok()?.min(len - 1);
        Some(HttpResponse {
            status: 206,
            body: media[a..=b].to_vec().into(),
            content_range: Some(format!("bytes {a}-{b}/{len}")),
            content_type: Some("audio/mp4".into()),
        })
    });
    (http, fetched)
}

#[tokio::test]
async fn misses_are_queued_and_the_worker_resolves_its_own_url_for_the_140_record() {
    let (http, fetched) = transport(click_wav(6.0));
    let ytm = YtmAnalysis::open(
        Arc::new(http.clone()),
        Arc::new(hocket_core::util::WallClock),
        None,
    );
    // Only YouTube Music keys with a video id are queued, once each.
    let keys = [
        AnalysisKey::ytm(VIDEO, 140),
        AnalysisKey::ytm(VIDEO, 140),
        AnalysisKey::ytm("not a video id", 140),
        AnalysisKey::navidrome("https://music.example/", "t1", None, None),
    ];
    assert_eq!(ytm.push(keys.iter()), 1);
    assert_eq!(ytm.queued(), 1);

    let store = AnalysisStore::in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let mut analyser = None;
    ytm.analyse_one(&mut analyser, &store, VIDEO, tmp.path())
        .await
        .unwrap();
    let key = AnalysisKey::ytm(VIDEO, 140);
    let record = store
        .get(&key.provider, &key.fingerprint)
        .unwrap()
        .expect("stored");
    assert!(
        (5_500..=6_500).contains(&record.duration_ms),
        "{}",
        record.duration_ms
    );
    assert_eq!(
        record.source.size_bytes, None,
        "devices know it by duration"
    );
    // The URL came from its own player call (visionOS: no session, no JS).
    assert!(http.endpoints().iter().any(|e| e == "player"));
    let urls = fetched.lock().clone();
    assert!(
        !urls.is_empty()
            && urls
                .iter()
                .all(|u| u.contains("c=VISIONOS") && u.contains("itag=140"))
    );
    // Nothing left behind.
    assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
}

async fn start_with(
    args: Args,
    http: FakeYtmHttp,
) -> (Arc<App>, String, tokio::sync::oneshot::Sender<()>) {
    std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
    std::env::set_var("no_proxy", "127.0.0.1,localhost");
    let app = App::with_ytm_http(
        args,
        Arc::new(|| hocket_core::util::now_ms() as u64),
        Some(Arc::new(http)),
    )
    .unwrap();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let (addr, _server) = serve(app.clone(), "127.0.0.1:0".parse().unwrap(), async {
        let _ = stop_rx.await;
    })
    .await
    .unwrap();
    (app, format!("ws://{addr}/ws"), stop_tx)
}

fn status(got: &[Msg]) -> Option<(bool, bool, Option<String>, Option<String>)> {
    got.iter().find_map(|m| match m {
        Msg::CoordYtmStatus {
            enabled,
            signed_in,
            account,
            error,
        } => Some((*enabled, *signed_in, account.clone(), error.clone())),
        _ => None,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_owner_signs_it_in_with_a_sealed_session_and_nobody_else_can() {
    let dir = tempfile::tempdir().unwrap();
    let (http, _) = transport(click_wav(1.0));
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    args.analysis_ytm = true;
    let (app, url, _stop) = start_with(args, http.clone()).await;
    let kx = app.identities().keys().kx;
    let mut owner = owner(&app, &url, "o1").await;
    let mut ws = join(&app, &url, &mut owner).await;
    let is_status = |m: &Msg| matches!(m, Msg::CoordYtmStatus { .. });

    // Anonymous to begin with.
    ws_send(&mut ws, Msg::CoordYtmQuery).await;
    assert_eq!(
        status(&until(&mut ws, is_status).await),
        Some((true, false, None, None))
    );

    // A session sealed for another key is refused, and nothing changes.
    let wrong = envelope(
        &DeviceSecret::generate().public().kx,
        COORD_YTM_CONTEXT,
        b"SAPISID=abc; SID=x",
    )
    .unwrap();
    ws_send(
        &mut ws,
        Msg::CoordYtmSignIn {
            envelope: Some(wrong),
        },
    )
    .await;
    let (_, signed_in, _, error) = status(&until(&mut ws, is_status).await).unwrap();
    assert!(!signed_in && error.is_some());

    // Sealed to the coordinator: checked against the account menu, kept privately.
    let sealed = envelope(
        &kx,
        COORD_YTM_CONTEXT,
        b"SAPISID=abc; __Secure-3PAPISID=abc; SID=x",
    )
    .unwrap();
    ws_send(
        &mut ws,
        Msg::CoordYtmSignIn {
            envelope: Some(sealed),
        },
    )
    .await;
    assert_eq!(
        status(&until(&mut ws, is_status).await),
        Some((true, true, Some("Test User".into()), None))
    );
    assert!(http.endpoints().iter().any(|e| e == "account/account_menu"));
    let path = dir.path().join(SESSION_FILE);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("SAPISID=abc"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // It survives a restart.
    let again = YtmAnalysis::open(
        Arc::new(http.clone()),
        Arc::new(hocket_core::util::WallClock),
        Some(dir.path()),
    );
    assert!(again.status().signed_in);

    // Another identity the owner invited gets no answer at all.
    let mut guest = invited(&app, &url, &owner.id.clone(), "g1").await;
    let mut gws = join(&app, &url, &mut guest).await;
    let sealed = envelope(&kx, COORD_YTM_CONTEXT, b"SAPISID=evil").unwrap();
    ws_send(
        &mut gws,
        Msg::CoordYtmSignIn {
            envelope: Some(sealed),
        },
    )
    .await;
    ws_send(&mut gws, Msg::CoordYtmQuery).await;
    assert_eq!(status(&until(&mut gws, is_status).await), None);
    assert!(app.ytm().unwrap().status().signed_in);

    // Signing out forgets the session.
    ws_send(&mut ws, Msg::CoordYtmSignIn { envelope: None }).await;
    assert_eq!(
        status(&until(&mut ws, is_status).await),
        Some((true, false, None, None))
    );
    assert!(!path.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_analysis_ytm_the_owner_is_told_it_is_off() {
    let (http, _) = transport(click_wav(1.0));
    let (app, url, _stop) = start_with(Args::for_tests(), http).await;
    let mut owner = owner(&app, &url, "o1").await;
    let mut ws = join(&app, &url, &mut owner).await;
    ws_send(&mut ws, Msg::CoordYtmQuery).await;
    let got = until(&mut ws, |m| matches!(m, Msg::CoordYtmStatus { .. })).await;
    assert_eq!(status(&got), Some((false, false, None, None)));
}

async fn ws_send(ws: &mut Ws, m: Msg) {
    use futures::SinkExt;
    ws.send(frame(m)).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_members_youtube_music_miss_is_queued_by_video_id() {
    let (http, _) = transport(click_wav(1.0));
    let mut args = Args::for_tests();
    args.analysis_ytm = true;
    let (app, url, _stop) = start_with(args, http.clone()).await;
    let mut owner = owner(&app, &url, "o1").await;
    let mut ws = join(&app, &url, &mut owner).await;
    ws_send(
        &mut ws,
        Msg::AnalysisFetch {
            wants: vec![hocket_core::connect::wire::AnalysisWant {
                track_id: hocket_core::api::TrackRef::new("ytm", VIDEO),
                key: AnalysisKey::ytm(VIDEO, 140),
            }],
        },
    )
    .await;
    until(&mut ws, |m| matches!(m, Msg::AnalysisRecords { .. })).await;
    assert_eq!(app.ytm().unwrap().queued(), 1);
    // Only the video id travelled: no URL of the device's is ever taken.
    assert!(!http.endpoints().iter().any(|e| e == "player"));
}

/// The coordinator signed in, its session in `ytm-session.json`, its
/// puzzles solved by the embedded EJS over [`PREPROCESSED`].
fn signed_in(http: &FakeYtmHttp, dir: &std::path::Path) -> YtmAnalysis {
    std::fs::write(
        dir.join(SESSION_FILE),
        r#"{"cookies":"SAPISID=abc; SID=x","channel":"UCme","name":"Test User"}"#,
    )
    .unwrap();
    let http: Arc<dyn hocket_core::ytm::http::YtmHttp> = Arc::new(http.clone());
    let clock: Arc<dyn hocket_core::util::Clock> = Arc::new(hocket_core::util::WallClock);
    let js = hocket_coordinator::ytm_js::EmbeddedJsHost::new(http.clone(), clock.clone())
        .with_preprocessed("fakeplay", 20_500, PREPROCESSED);
    YtmAnalysis::open_with_js(http, clock, Some(dir), Arc::new(js))
}

fn player_calls(http: &FakeYtmHttp) -> Vec<HttpRequest> {
    http.log
        .lock()
        .iter()
        .filter(|r| r.url.contains("/youtubei/v1/player"))
        .cloned()
        .collect()
}

#[tokio::test]
async fn signed_in_the_worker_streams_with_its_session_through_the_tv_app() {
    let (http, fetched) = transport_tv(click_wav(6.0), false);
    let dir = tempfile::tempdir().unwrap();
    let ytm = signed_in(&http, dir.path());
    assert!(ytm.status().signed_in);
    let store = AnalysisStore::in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ytm.analyse_one(&mut None, &store, VIDEO, tmp.path())
        .await
        .unwrap();
    let key = AnalysisKey::ytm(VIDEO, 140);
    assert!(store
        .get(&key.provider, &key.fingerprint)
        .unwrap()
        .is_some());
    // One player call: the TV app, with the session and the player's timestamp.
    let calls = player_calls(&http);
    assert_eq!(calls.len(), 1);
    let body = calls[0].json();
    assert_eq!(body["context"]["client"]["clientName"], "TVHTML5");
    assert_eq!(
        body["playbackContext"]["contentPlaybackContext"]["signatureTimestamp"],
        20_500
    );
    assert_eq!(calls[0].header("Cookie"), Some("SAPISID=abc; SID=x"));
    assert!(calls[0]
        .header("Authorization")
        .unwrap()
        .starts_with("SAPISIDHASH "));
    // Its own URL, signed and `n`-transformed in QuickJS.
    let urls = fetched.lock().clone();
    assert!(!urls.is_empty());
    for u in &urls {
        assert!(u.contains("c=TVHTML5") && u.contains("itag=140"), "{u}");
        assert!(u.contains("&n=ABCD_ok") && u.contains("&sig=zyxGIS"), "{u}");
    }
}

#[tokio::test]
async fn signed_in_the_worker_falls_back_to_the_anonymous_chain_when_the_tv_app_refuses() {
    let (http, fetched) = transport_tv(click_wav(6.0), true);
    let dir = tempfile::tempdir().unwrap();
    let ytm = signed_in(&http, dir.path());
    let store = AnalysisStore::in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    ytm.analyse_one(&mut None, &store, VIDEO, tmp.path())
        .await
        .unwrap();
    let clients: Vec<String> = player_calls(&http)
        .iter()
        .map(|r| {
            r.json()["context"]["client"]["clientName"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(clients, ["TVHTML5", "VISIONOS"]);
    assert!(player_calls(&http)[1].header("Cookie").is_none());
    assert!(fetched.lock().iter().all(|u| u.contains("c=VISIONOS")));
}
