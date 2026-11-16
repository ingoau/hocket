//! The shared play log a room keeps: plays pushed to the coordinator are
//! acknowledged, persisted with the room, and handed to a device that asks
//! after the room was unloaded, from its mark.

mod common;

use std::time::Duration;

use futures::SinkExt;

use common::*;
use hocket_coordinator::Args;
use hocket_core::api::TrackRef;
use hocket_core::connect::wire::{Msg, PlayLogMark, PlayRecord};

fn play(track: &str, at: f64) -> PlayRecord {
    PlayRecord {
        track_id: TrackRef::new("acct", track),
        played_at: at,
        played_ms: 120_000,
        device_id: "a".into(),
        seq: 0,
    }
}

fn read(got: &[Msg]) -> Vec<(String, u32, Vec<String>)> {
    got.iter()
        .filter_map(|m| match m {
            Msg::PlaysLog {
                log_id,
                after,
                plays,
            } => Some((
                log_id.clone(),
                *after,
                plays.iter().map(|p| p.track_id.id.clone()).collect(),
            )),
            _ => None,
        })
        .collect()
}

async fn close(mut ws: Ws) {
    ws.close(None).await.unwrap();
    drop(ws);
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_play_log_is_persisted_and_read_back_from_a_mark() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = Args::for_tests();
    args.data_dir = Some(dir.path().to_path_buf());
    let (app, url, _stop) = start(args).await;
    let mut a = owner(&app, &url, "a").await;
    let now = app.now_ms() as f64;

    let mut ws = join(&app, &url, &mut a).await;
    ws.send(frame(Msg::PlaysPush {
        batch: 1,
        plays: vec![play("t1", now - 60_000.0), play("t2", now - 30_000.0)],
    }))
    .await
    .unwrap();
    let got = until(&mut ws, |m| matches!(m, Msg::PlaysLog { .. })).await;
    let log_id = got
        .iter()
        .find_map(|m| match m {
            Msg::PlaysPushed { batch: 1, log_id } => Some(log_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("acknowledged: {got:?}"));
    assert_eq!(
        read(&got),
        vec![(log_id.clone(), 0, vec!["t1".into(), "t2".into()])]
    );
    close(ws).await;
    assert_eq!(app.room_count(), 0, "the room was unloaded");

    let ask = |marks: Vec<PlayLogMark>| frame(Msg::PlaysRequest { marks });
    let mut ws = join(&app, &url, &mut a).await;
    ws.send(ask(vec![])).await.unwrap();
    let got = until(&mut ws, |m| matches!(m, Msg::PlaysLog { .. })).await;
    assert_eq!(
        read(&got),
        vec![(log_id.clone(), 0, vec!["t1".into(), "t2".into()])]
    );
    ws.send(ask(vec![PlayLogMark {
        log_id: log_id.clone(),
        seq: 1,
    }]))
    .await
    .unwrap();
    let got = until(&mut ws, |m| matches!(m, Msg::PlaysLog { .. })).await;
    assert_eq!(read(&got), vec![(log_id, 1, vec!["t2".into()])]);
    close(ws).await;
}
