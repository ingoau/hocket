//! A live check of the coordinator's JS path (ignored in CI; needs the
//! network): anonymous, every client but the TV app skipped, the worker
//! resolves through the embedded EJS solver (the real `base.js`) and
//! downloads and analyses the track in bounded ranges.
//!
//! ```sh
//! HOCKET_LIVE_YTM=1 HOCKET_YTM_FORCE_WEB=1 cargo test -p hocket-coordinator --test ytm_live -- --ignored --nocapture
//! ```

use std::sync::Arc;

use futures::future::BoxFuture;
use parking_lot::Mutex;

use hocket_coordinator::analysis::AnalysisStore;
use hocket_coordinator::ytm::YtmAnalysis;
use hocket_core::connect::wire::AnalysisKey;
use hocket_core::ytm::http::{HttpRequest, HttpResponse, ReqwestYtmHttp, YtmHttp};

/// URL, status, range.
type Log = Arc<Mutex<Vec<(String, u16, Option<String>)>>>;

/// Logs what was asked (anonymous: no session in any request).
struct Logged(ReqwestYtmHttp, Log);

impl YtmHttp for Logged {
    fn send(&self, req: HttpRequest) -> BoxFuture<'static, Result<HttpResponse, String>> {
        let log = self.1.clone();
        let url = req.url.clone();
        let range = req.header("Range").map(str::to_string);
        let f = self.0.send(req);
        Box::pin(async move {
            let r = f.await?;
            log.lock().push((url, r.status, range));
            Ok(r)
        })
    }
}

#[tokio::test]
#[ignore = "live network: HOCKET_LIVE_YTM=1 HOCKET_YTM_FORCE_WEB=1"]
async fn the_tv_app_resolves_and_downloads_through_the_embedded_solver() {
    if std::env::var_os("HOCKET_LIVE_YTM").is_none() {
        eprintln!("HOCKET_LIVE_YTM unset: skipped");
        return;
    }
    assert!(
        std::env::var_os("HOCKET_YTM_FORCE_WEB").is_some(),
        "set HOCKET_YTM_FORCE_WEB=1 so only the TV app is tried"
    );
    let _ = tracing_subscriber::fmt()
        .with_env_filter("hocket=debug,hocket_coordinator=debug")
        .try_init();
    let log = Arc::new(Mutex::new(vec![]));
    let http = Arc::new(Logged(ReqwestYtmHttp::new().unwrap(), log.clone()));
    let ytm = YtmAnalysis::open(http, Arc::new(hocket_core::util::WallClock), None);
    let store = AnalysisStore::in_memory().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let video = std::env::var("HOCKET_LIVE_YTM_VIDEO").unwrap_or_else(|_| "4D7u5KF7SP8".into());
    let started = std::time::Instant::now();
    let r = ytm.analyse_one(&mut None, &store, &video, tmp.path()).await;
    for (url, status, range) in log.lock().iter() {
        let short: String = url
            .split('?')
            .next()
            .unwrap_or("")
            .chars()
            .take(90)
            .collect();
        let client = url
            .split(['?', '&'])
            .find_map(|p| p.strip_prefix("c="))
            .unwrap_or("");
        eprintln!(
            "{status} {short} {client} {}",
            range.as_deref().unwrap_or("")
        );
    }
    eprintln!("took {:?}", started.elapsed());
    r.unwrap();
    let key = AnalysisKey::ytm(&video, 140);
    let rec = store
        .get(&key.provider, &key.fingerprint)
        .unwrap()
        .expect("stored");
    eprintln!("analysed: {} ms, BPM {:?}", rec.duration_ms, rec.bpm);
    let log = log.lock();
    assert!(log
        .iter()
        .any(|(u, s, _)| u.contains("googlevideo.com/videoplayback")
            && u.contains("c=TVHTML5")
            && *s == 206));
}
