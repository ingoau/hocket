//! The coordinator's embedded JS host (`ytm_js`): yt-dlp's EJS solver in
//! QuickJS, through the core's `JsHost` interface. A synthetic preprocessed
//! player stands in for YouTube's `base.js`; no network.

use std::sync::Arc;

use hocket_coordinator::ytm_js::{run_jsc, EmbeddedJsHost};
use hocket_core::ytm::clients::ClientKind;
use hocket_core::ytm::http::{FakeYtmHttp, HttpResponse};
use hocket_core::ytm::player::{JsHost, JsRequest};

const API: &str = r"var scriptUrl = 'https:\/\/www.youtube.com\/s\/player\/fakeplay\/www-widgetapi.vflset\/www-widgetapi.js';";

fn host(preprocessed: &str) -> (EmbeddedJsHost, FakeYtmHttp) {
    let http = FakeYtmHttp::new(|r| {
        (r.url == "https://www.youtube.com/iframe_api").then(|| HttpResponse::json(200, API))
    });
    let h = EmbeddedJsHost::new(
        Arc::new(http.clone()),
        Arc::new(hocket_core::util::WallClock),
    )
    .with_preprocessed("fakeplay", 20_500, preprocessed);
    (h, http)
}

#[tokio::test]
async fn solves_signature_and_n_through_ejs_in_quickjs() {
    let (h, http) = host(
        "_result.sig = (s) => s.split('').reverse().join(''); _result.n = (n) => n + '-' + n.length;",
    );
    assert_eq!(h.clients(), [ClientKind::Tv]);
    let prep = h
        .solve(JsRequest::Prepare {
            video_id: "4D7u5KF7SP8".into(),
            visitor_data: "v".into(),
        })
        .await
        .unwrap();
    assert_eq!(prep.signature_timestamp, Some(20_500));
    assert_eq!(prep.po_token, None, "no BotGuard on the coordinator");
    let a = h
        .solve(JsRequest::Decipher {
            video_id: "4D7u5KF7SP8".into(),
            formats: vec![
                "s=A%2FBC&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D140%26n%3Dqwerty".into(),
                "https://rr1.googlevideo.com/videoplayback?itag=251&n=qwerty&sig=done".into(),
            ],
        })
        .await
        .unwrap();
    assert_eq!(
        a.urls,
        [
            "https://rr1.googlevideo.com/videoplayback?itag=140&n=qwerty-6&sig=CB%2FA",
            "https://rr1.googlevideo.com/videoplayback?itag=251&n=qwerty-6&sig=done",
        ]
    );
    // The player stayed cached: iframe_api only, never base.js.
    assert!(http.log.lock().iter().all(|r| !r.url.contains("base.js")));
}

#[tokio::test]
async fn a_solver_failure_is_an_error_not_a_url() {
    let (h, _) = host("_result.sig = (s) => { throw new Error('boom'); }; _result.n = null;");
    let e = h
        .solve(JsRequest::Decipher {
            video_id: "x".into(),
            formats: vec![
                "s=AB&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fn%3Dq".into(),
            ],
        })
        .await
        .unwrap_err();
    assert!(e.contains("boom") || e.contains("Failed to extract"), "{e}");
    // A player EJS can't parse is reported too.
    let out = run_jsc(r#"{"type":"player","player":"this is not a player","requests":[{"type":"n","challenges":["a"]}]}"#.into()).await;
    match out {
        Err(e) => assert!(e.starts_with("EJS"), "{e}"),
        Ok(o) => assert!(
            !o.contains(r#""type":"result","responses":[{"type":"result""#),
            "{o}"
        ),
    }
}
