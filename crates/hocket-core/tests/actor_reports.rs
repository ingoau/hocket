//! Save to Coordinator, the client side (`docs/reports.md`): a full core
//! uploads reports to a small fake coordinator that implements the spec's
//! HTTP API. The grant is handed to the core directly (the test hook
//! `Internal::ReportGrant`); how it is taken from a diagnostics answer is
//! tested in `hocket_core::reports`.

#![cfg(feature = "sim")]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde_json::{json, Value};

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::core::Internal;
use hocket_core::reports::{self, HeldGrant};

const GRANT: &str = "grant-token";
const MIB: usize = 1 << 20;

// ---------------------------------------------------------------------------
// The fake coordinator
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Report {
    key: String,
    body: Value,
    files: Vec<(u64, Vec<u8>)>,
}

impl Report {
    fn complete(&self) -> bool {
        self.files
            .iter()
            .all(|(size, got)| got.len() as u64 == *size)
    }

    fn files_json(&self) -> Value {
        self.files
            .iter()
            .enumerate()
            .map(|(i, (size, got))| json!({"index": i, "size": size, "received": got.len()}))
            .collect()
    }
}

/// What the fake does wrong, by PUT number (counting from 1, every PUT).
#[derive(Default, Clone)]
struct Faults {
    /// Answer this PUT with 400 (nothing stored).
    reject_put: Option<usize>,
    /// Store this PUT's bytes but answer 409 with the new offset, as if the
    /// client had missed the answer to an earlier one.
    conflict_put: Option<usize>,
    /// Answer this PUT with 503 (nothing stored).
    unavailable_put: Option<usize>,
    /// Never answer this PUT.
    stall_put: Option<usize>,
}

#[derive(Default)]
struct Fake {
    reports: HashMap<String, Report>,
    faults: Faults,
    puts: usize,
    gets: usize,
    posts: usize,
}

type Shared = Arc<Mutex<Fake>>;

fn bearer(headers: &HeaderMap) -> String {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_string()
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error": code, "detail": code}))).into_response()
}

async fn create(State(s): State<Shared>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    let mut f = s.lock();
    f.posts += 1;
    if bearer(&headers) != GRANT {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let id = format!("R-20261101-{:04}", f.reports.len() + 1);
    let files: Vec<(u64, Vec<u8>)> = body["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["size"].as_u64().unwrap(), vec![]))
        .collect();
    let report = Report {
        key: format!("key-{id}"),
        body,
        files,
    };
    let answer = json!({"id": id, "uploadKey": report.key, "files": report.files_json()});
    f.reports.insert(id, report);
    (StatusCode::CREATED, Json(answer)).into_response()
}

async fn status(
    State(s): State<Shared>,
    UrlPath(id): UrlPath<String>,
    headers: HeaderMap,
) -> Response {
    let mut f = s.lock();
    f.gets += 1;
    let Some(r) = f.reports.get(&id) else {
        return error(StatusCode::NOT_FOUND, "notFound");
    };
    if bearer(&headers) != r.key {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    Json(json!({"id": id, "files": r.files_json(), "complete": r.complete()})).into_response()
}

async fn upload(
    State(s): State<Shared>,
    UrlPath((id, index)): UrlPath<(String, usize)>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let stall = {
        let mut f = s.lock();
        f.puts += 1;
        let n = f.puts;
        let faults = f.faults.clone();
        if faults.reject_put == Some(n) {
            return error(StatusCode::BAD_REQUEST, "badRequest");
        }
        if faults.unavailable_put == Some(n) {
            return error(StatusCode::SERVICE_UNAVAILABLE, "disabled");
        }
        faults.stall_put == Some(n)
    };
    if stall {
        std::future::pending::<()>().await;
    }
    let mut f = s.lock();
    let conflict = f.faults.conflict_put == Some(f.puts);
    let Some(r) = f.reports.get_mut(&id) else {
        return error(StatusCode::NOT_FOUND, "notFound");
    };
    if bearer(&headers) != r.key {
        return error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    let offset: usize = headers["upload-offset"].to_str().unwrap().parse().unwrap();
    let (size, got) = &mut r.files[index];
    if offset != got.len() {
        let received = got.len();
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "conflict", "received": received})),
        )
            .into_response();
    }
    if (offset + body.len()) as u64 > *size {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "tooLarge");
    }
    got.extend_from_slice(&body);
    let received = got.len();
    if conflict {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "conflict", "received": received})),
        )
            .into_response();
    }
    let complete = r.complete();
    Json(json!({"received": received, "complete": complete})).into_response()
}

/// A fake coordinator on a free port; its `ws://…/ws` URL.
async fn serve_fake(faults: Faults) -> (Shared, String) {
    let shared: Shared = Arc::new(Mutex::new(Fake {
        faults,
        ..Default::default()
    }));
    let app = Router::new()
        .route("/reports", post(create))
        .route("/reports/{id}", get(status))
        .route("/reports/{id}/files/{index}", put(upload))
        .layer(axum::extract::DefaultBodyLimit::max(16 * MIB))
        .with_state(shared.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (shared, format!("ws://127.0.0.1:{port}/ws"))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn grant(t: &TestCore, url: &str, token: &str) {
    t.core
        .post_internal(Internal::ReportGrant(HeldGrant {
            token: token.into(),
            expires_at: t.clock.now_ms() + 3_600_000.0,
            coordinator_url: url.into(),
        }))
        .unwrap();
}

/// A file of `len` bytes that are not all alike.
fn file(dir: &Path, name: &str, len: usize) -> PathBuf {
    let path = dir.join(name);
    let bytes: Vec<u8> = (0..len).map(|i| (i * 7 + i / 4096) as u8).collect();
    std::fs::write(&path, bytes).unwrap();
    path
}

fn attachment(path: &Path, kind: ReportAttachmentKind, delete_after: bool) -> ReportAttachment {
    ReportAttachment {
        path: path.to_string_lossy().into_owned(),
        kind,
        delete_after,
    }
}

fn start(t: &TestCore, attachments: Vec<ReportAttachment>) {
    t.dispatch(Command::StartReportUpload {
        note: "Queue jumped back two songs".into(),
        bundle: "the platform's bundle\n".into(),
        attachments,
    });
}

async fn status_of(t: &TestCore) -> Option<ReportUploadStatus> {
    match t.query(Query::ReportUpload).await {
        QueryResult::ReportUpload(s) => s,
        other => panic!("{other:?}"),
    }
}

/// Wait (in real time: the upload runs on the network) until the upload is
/// in `state`.
async fn wait_for(t: &TestCore, state: ReportUploadState) -> ReportUploadStatus {
    for _ in 0..1500 {
        if let Some(s) = status_of(t).await {
            if s.state == state {
                return s;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {state:?}: {:?}", status_of(t).await);
}

async fn wait_until(mut f: impl FnMut() -> bool) {
    for _ in 0..1500 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out");
}

fn upload_events(t: &TestCore) -> Vec<Option<ReportUploadStatus>> {
    t.events
        .all()
        .into_iter()
        .filter_map(|e| match e {
            Event::ReportUploadChanged { status } => Some(status),
            _ => None,
        })
        .collect()
}

fn staging(t: &TestCore) -> PathBuf {
    reports::staging_dir(&t.data_dir())
}

/// The report the fake holds has exactly these files' bytes.
fn assert_received(fake: &Shared, id: &str, files: &[&[u8]]) {
    let f = fake.lock();
    let r = &f.reports[id];
    assert!(r.complete());
    assert_eq!(r.files.len(), files.len() + 1);
    assert_eq!(r.files[0].1, b"the platform's bundle\n");
    for (i, bytes) in files.iter().enumerate() {
        assert!(r.files[i + 1].1 == *bytes, "file {} differs", i + 1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_report_goes_up_in_chunks_and_cleans_up() {
    let (fake, url) = serve_fake(Faults::default()).await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let shot = file(files.path(), "screenshot-1.jpg", 300_000);
    let recording = file(files.path(), "screen-20261101-140211.mp4", 9 * MIB + 123);
    let shot_bytes = std::fs::read(&shot).unwrap();
    let recording_bytes = std::fs::read(&recording).unwrap();
    grant(&t, &url, GRANT);
    t.events.clear();
    start(
        &t,
        vec![
            attachment(&shot, ReportAttachmentKind::Screenshot, false),
            attachment(&recording, ReportAttachmentKind::Recording, true),
        ],
    );
    let done = wait_for(&t, ReportUploadState::Done).await;
    let id = done.report_id.clone().unwrap();
    assert_eq!(done.total_bytes, (22 + 300_000 + 9 * MIB + 123) as f64);
    assert_eq!(done.sent_bytes, done.total_bytes);
    assert_eq!(done.error, None);

    {
        let f = fake.lock();
        assert_eq!(f.posts, 1);
        assert_eq!(f.puts, 1 + 1 + 3, "the recording in three 4 MiB chunks");
        let body = &f.reports[&id].body;
        assert_eq!(body["note"], "Queue jumped back two songs");
        assert_eq!(body["device"], "Device a");
        assert!(body["client"]
            .as_str()
            .unwrap()
            .starts_with("Hocket Linux "));
        let files = body["files"].as_array().unwrap();
        assert_eq!(files[0]["kind"], "bundle");
        assert!(files[0]["name"]
            .as_str()
            .unwrap()
            .starts_with("hocket-diagnostics-"));
        assert_eq!(files[1]["kind"], "screenshot");
        assert_eq!(files[1]["name"], "screenshot-1.jpg");
        assert_eq!(files[2]["kind"], "recording");
        assert_eq!(files[2]["name"], "screen-20261101-140211.mp4");
    }
    assert_received(&fake, &id, &[&shot_bytes, &recording_bytes]);

    assert!(!staging(&t).exists(), "job.json and the bundle are gone");
    assert!(!recording.exists(), "the platform's copy is deleted");
    assert!(shot.exists(), "a file the platform kept is not");

    let seen = upload_events(&t);
    let first = seen.first().unwrap().as_ref().unwrap();
    assert_eq!(first.state, ReportUploadState::Uploading);
    assert_eq!(first.sent_bytes, 0.0);
    assert_eq!(seen.last().unwrap().as_ref(), Some(&done));
    assert!(seen
        .iter()
        .flatten()
        .any(|s| s.report_id.is_some() && s.state == ReportUploadState::Uploading));

    // Done stays until dismissed.
    assert_eq!(status_of(&t).await, Some(done));
    t.run(Command::DismissReportUpload).await;
    assert_eq!(status_of(&t).await, None);
    assert_eq!(upload_events(&t).last().unwrap(), &None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_upload_resumes_on_retry_where_it_stopped() {
    // The second chunk of the recording is refused.
    let (fake, url) = serve_fake(Faults {
        reject_put: Some(3),
        ..Default::default()
    })
    .await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let recording = file(files.path(), "rec.webm", 10 * MIB);
    grant(&t, &url, GRANT);
    start(
        &t,
        vec![attachment(
            &recording,
            ReportAttachmentKind::Recording,
            false,
        )],
    );
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    assert_eq!(failed.error, Some(ReportUploadError::Server));
    assert!(failed.error_detail.as_deref().unwrap().contains("400"));
    let id = failed.report_id.clone().unwrap();
    assert!(staging(&t).join("job.json").exists());
    assert_eq!(fake.lock().reports[&id].files[1].1.len(), 4 * MIB);

    t.run(Command::RetryReportUpload).await;
    let done = wait_for(&t, ReportUploadState::Done).await;
    assert_eq!(done.report_id.as_deref(), Some(id.as_str()));
    let f = fake.lock();
    assert_eq!(f.posts, 1, "the same report carried on");
    assert!(f.gets >= 1, "the offsets came from the coordinator");
    // bundle, chunk 1, refused, then chunks 2 and 3: nothing was sent twice.
    assert_eq!(f.puts, 5);
    drop(f);
    assert_received(&fake, &id, &[&std::fs::read(&recording).unwrap()]);
    assert!(!staging(&t).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_answer_moves_the_upload_to_the_coordinators_offset() {
    let (fake, url) = serve_fake(Faults {
        conflict_put: Some(2),
        ..Default::default()
    })
    .await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let recording = file(files.path(), "rec.mp4", 9 * MIB);
    grant(&t, &url, GRANT);
    start(
        &t,
        vec![attachment(
            &recording,
            ReportAttachmentKind::Recording,
            false,
        )],
    );
    let done = wait_for(&t, ReportUploadState::Done).await;
    let id = done.report_id.unwrap();
    assert_eq!(fake.lock().puts, 4, "nothing went twice");
    assert_received(&fake, &id, &[&std::fs::read(&recording).unwrap()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_grant_is_not_allowed_and_dismiss_cleans_up() {
    let (fake, url) = serve_fake(Faults::default()).await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let shot = file(files.path(), "s.png", 1000);
    grant(&t, &url, "not-the-grant");
    start(
        &t,
        vec![attachment(&shot, ReportAttachmentKind::Screenshot, true)],
    );
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    assert_eq!(failed.error, Some(ReportUploadError::NotAllowed));
    assert!(failed.error_detail.unwrap().contains("401"));
    assert_eq!(failed.report_id, None);
    assert_eq!(fake.lock().posts, 1, "a refusal is not retried on its own");
    assert!(staging(&t).exists());

    t.events.clear();
    t.run(Command::DismissReportUpload).await;
    assert_eq!(status_of(&t).await, None);
    assert_eq!(upload_events(&t), vec![None]);
    assert!(!staging(&t).exists());
    assert!(!shot.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_grant_nothing_starts_until_one_comes() {
    let (fake, url) = serve_fake(Faults::default()).await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let shot = file(files.path(), "s.jpg", 1000);
    let shot_bytes = std::fs::read(&shot).unwrap();
    start(
        &t,
        vec![attachment(&shot, ReportAttachmentKind::Screenshot, true)],
    );
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    assert_eq!(failed.error, Some(ReportUploadError::NotAllowed));
    assert_eq!(fake.lock().posts, 0);
    assert!(shot.exists(), "kept for Retry");

    grant(&t, &url, GRANT);
    t.run(Command::RetryReportUpload).await;
    let done = wait_for(&t, ReportUploadState::Done).await;
    assert_received(&fake, done.report_id.as_deref().unwrap(), &[&shot_bytes]);
    assert!(!shot.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn too_many_or_unreadable_attachments_fail_before_anything_goes() {
    let (fake, url) = serve_fake(Faults::default()).await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let shot = file(files.path(), "s.jpg", 10);
    grant(&t, &url, GRANT);
    start(
        &t,
        vec![attachment(&shot, ReportAttachmentKind::Screenshot, false); 6],
    );
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    assert_eq!(failed.error, Some(ReportUploadError::Unreadable));
    start(
        &t,
        vec![attachment(
            &files.path().join("gone.jpg"),
            ReportAttachmentKind::Screenshot,
            false,
        )],
    );
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    assert_eq!(failed.error, Some(ReportUploadError::Unreadable));
    assert!(failed.error_detail.unwrap().contains("gone.jpg"));
    assert_eq!(fake.lock().posts, 0);
    assert!(!staging(&t).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_report_is_ignored_while_one_uploads_and_dismiss_cancels_it() {
    let (fake, url) = serve_fake(Faults {
        stall_put: Some(2),
        ..Default::default()
    })
    .await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let recording = file(files.path(), "rec.mp4", 5 * MIB);
    grant(&t, &url, GRANT);
    start(
        &t,
        vec![attachment(
            &recording,
            ReportAttachmentKind::Recording,
            true,
        )],
    );
    wait_until(|| fake.lock().puts == 2).await;
    let uploading = status_of(&t).await.unwrap();
    assert_eq!(uploading.state, ReportUploadState::Uploading);
    start(&t, vec![]);
    t.core.settle().await;
    assert_eq!(fake.lock().posts, 1, "the second one was ignored");
    assert_eq!(
        status_of(&t).await.unwrap().started_at,
        uploading.started_at
    );

    t.run(Command::DismissReportUpload).await;
    assert_eq!(status_of(&t).await, None);
    assert!(!staging(&t).exists());
    assert!(!recording.exists());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        status_of(&t).await,
        None,
        "the cancelled task says nothing more"
    );
    assert!(!staging(&t).exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_carries_on_after_a_restart() {
    let (fake, url) = serve_fake(Faults {
        stall_put: Some(3),
        ..Default::default()
    })
    .await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    let files = tempfile::tempdir().unwrap();
    let recording = file(files.path(), "rec.mp4", 9 * MIB);
    let expected = std::fs::read(&recording).unwrap();
    grant(&t, &url, GRANT);
    start(
        &t,
        vec![attachment(
            &recording,
            ReportAttachmentKind::Recording,
            true,
        )],
    );
    wait_until(|| fake.lock().puts == 3).await;
    let job = reports::load_job(&staging(&t)).unwrap().unwrap();
    assert_eq!(job.state, ReportUploadState::Uploading);
    let id = job
        .report_id
        .clone()
        .expect("the report id is saved at once");

    let t = t.restart("a").await;
    let done = wait_for(&t, ReportUploadState::Done).await;
    assert_eq!(done.report_id.as_deref(), Some(id.as_str()));
    let f = fake.lock();
    assert_eq!(f.posts, 1);
    assert!(f.gets >= 1);
    assert!(f.reports[&id].files[1].1 == expected);
    drop(f);
    assert!(!staging(&t).exists());
    assert!(!recording.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_upload_is_offered_again_after_a_restart() {
    let (fake, url) = serve_fake(Faults {
        reject_put: Some(1),
        ..Default::default()
    })
    .await;
    let t = TestCore::start("a", seeded_server(1, 100.0)).await;
    grant(&t, &url, GRANT);
    start(&t, vec![]);
    let failed = wait_for(&t, ReportUploadState::Failed).await;
    let t = t.restart("a").await;
    assert_eq!(status_of(&t).await, Some(failed.clone()));
    t.run(Command::RetryReportUpload).await;
    let done = wait_for(&t, ReportUploadState::Done).await;
    assert_eq!(done.report_id, failed.report_id);
    assert!(fake.lock().reports[done.report_id.as_deref().unwrap()].complete());
}

/// The uploader on its own: a 503 is tried again after the backoff.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transient_failures_are_retried_with_backoff() {
    let (fake, url) = serve_fake(Faults {
        unavailable_put: Some(2),
        ..Default::default()
    })
    .await;
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("report-upload");
    let rec = file(dir.path(), "r.mp4", 5 * MIB);
    let mut job = reports::stage(
        &staging,
        reports::NewReport {
            note: "n",
            bundle: "b",
            attachments: &[attachment(&rec, ReportAttachmentKind::Recording, false)],
            base_url: reports::base_url(&url).unwrap(),
            client: "test".into(),
            device: "test".into(),
            started_at: 0.0,
        },
    )
    .unwrap();
    let client = reports::http_client("test").unwrap();
    let events = Arc::new(Mutex::new(vec![]));
    let sink = events.clone();
    let result = reports::upload(
        &client,
        &staging,
        &mut job,
        Some(GRANT),
        &[Duration::from_millis(10)],
        Arc::new(move |e| sink.lock().push(e)),
    )
    .await;
    assert_eq!(result, Ok(()));
    assert_eq!(fake.lock().puts, 4);
    assert_eq!(job.sent_bytes(), job.total_bytes());
    let events = events.lock().clone();
    assert!(matches!(events[0], reports::UploadEvent::Created { .. }));
    assert_eq!(
        events.last(),
        Some(&reports::UploadEvent::Sent {
            sent: job.total_bytes()
        })
    );
    // The saved job has what a restart needs.
    let saved = reports::load_job(&staging).unwrap().unwrap();
    assert_eq!(saved.report_id, job.report_id);
    assert!(saved.upload_key.is_some());

    // With no retries left, the 503 is the answer.
    let (_fake, url) = serve_fake(Faults {
        unavailable_put: Some(1),
        ..Default::default()
    })
    .await;
    job.base_url = reports::base_url(&url).unwrap();
    job.report_id = None;
    job.upload_key = None;
    for f in &mut job.files {
        f.received = 0;
    }
    let err = reports::upload(
        &client,
        &staging,
        &mut job,
        Some(GRANT),
        &[],
        Arc::new(|_| {}),
    )
    .await
    .unwrap_err();
    assert_eq!(err.error, ReportUploadError::Server);
    assert!(err.transient);
}
