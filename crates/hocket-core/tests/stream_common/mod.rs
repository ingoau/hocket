//! Helpers shared by the stream-cache and prefetch actor tests.

#![allow(dead_code)]

use std::path::PathBuf;

use hocket_core::api::*;
use hocket_core::core::stream_reader::{StreamError, StreamInfo};
use hocket_core::core::test_support::TestCore;

/// A started core with a synced mirror whose backend reads through the
/// core (`hocket-stream://` sources).
pub async fn core_stream(t: &TestCore) {
    t.wait_until(30_000.0, |t| {
        t.events
            .all()
            .iter()
            .any(|e| matches!(e, Event::SyncProgress { progress } if progress.finished))
    })
    .await;
    t.run(Command::SetBackendCapabilities { core_stream: true })
        .await;
}

pub async fn source(t: &TestCore, id: &str) -> MediaSource {
    match t
        .query(Query::MediaSource {
            track_id: id.into(),
        })
        .await
    {
        QueryResult::Source(Some(s)) => s,
        other => panic!("no source for {id}: {other:?}"),
    }
}

/// Read a stream from `offset` to its end (or error).
pub async fn read_from(
    t: &TestCore,
    url: &str,
    offset: u64,
) -> Result<(StreamInfo, Vec<u8>), StreamError> {
    let info = t.core.stream_open(url, offset, None).await?;
    let mut out = vec![];
    loop {
        match t.core.stream_read(info.handle, 64 * 1024).await {
            Ok(b) if b.is_empty() => break,
            Ok(b) => out.extend_from_slice(&b),
            Err(e) => {
                t.core.stream_close(info.handle);
                return Err(e);
            }
        }
    }
    t.core.stream_close(info.handle);
    Ok((info, out))
}

pub async fn offline(t: &TestCore, id: &str) -> OfflineState {
    match t.query(Query::Track { id: id.into() }).await {
        QueryResult::TrackDetail(Some(tr)) => tr.offline,
        other => panic!("no track {id}: {other:?}"),
    }
}

/// Poll (real time, the cache write happens on the runtime) until the
/// track reaches `want`.
pub async fn wait_offline(t: &TestCore, id: &str, want: OfflineState) {
    for _ in 0..400 {
        t.core.settle().await;
        if offline(t, id).await == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("{id} never became {want:?} (is {:?})", offline(t, id).await);
}

/// Files in the stream-cache directory of the core's server.
pub fn stream_files(t: &TestCore) -> Vec<PathBuf> {
    let dir = t.dir.path().join("cache").join("stream").join(&t.server_id);
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

pub fn temp_files(t: &TestCore) -> Vec<PathBuf> {
    stream_files(t)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "part"))
        .collect()
}

/// Poll until no temp file is left.
pub async fn wait_no_temp(t: &TestCore) {
    for _ in 0..400 {
        if temp_files(t).is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("temp files left: {:?}", temp_files(t));
}

pub fn no_credentials(url: &str) -> bool {
    let q = url.split_once('?').map(|(_, q)| q).unwrap_or("");
    !q.split('&')
        .any(|kv| ["t", "s", "p", "u", "apiKey"].contains(&kv.split('=').next().unwrap_or("")))
        && !url.contains("secret")
}
