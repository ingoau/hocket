//! The battery saver's `SlowSync` reaches the Connect engine: on a level
//! change, at session start, and from a level persisted across a restart.

#![cfg(feature = "sim")]

use hocket_core::api::*;
use hocket_core::core::test_support::{seeded_server, TestCore};

async fn set_level(t: &TestCore, level: &str) {
    t.run(Command::SetSetting {
        key: "battery.level".into(),
        value: format!("\"{level}\""),
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn medium_battery_saver_turns_slow_sync_on_in_the_engine() {
    let t = TestCore::start("slow-sync-level", seeded_server(2, 100.0)).await;
    assert_eq!(t.core.engine_slow_sync().await, Some(false));
    set_level(&t, "medium").await;
    assert_eq!(t.core.engine_slow_sync().await, Some(true));
    // Light keeps Connect at its normal pace; max slows it again.
    set_level(&t, "light").await;
    assert_eq!(t.core.engine_slow_sync().await, Some(false));
    set_level(&t, "max").await;
    assert_eq!(t.core.engine_slow_sync().await, Some(true));
    set_level(&t, "off").await;
    assert_eq!(t.core.engine_slow_sync().await, Some(false));
    t.core.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_persisted_medium_level_is_in_force_from_startup_and_session_start() {
    let t = TestCore::start("slow-sync-restart", seeded_server(2, 100.0)).await;
    set_level(&t, "medium").await;
    let t = t.restart("slow-sync-restart-2").await;
    let saver = t.snapshot().await.battery_saver;
    assert_eq!(saver.level, SaverLevel::Medium);
    assert!(saver.features.contains(&SaverFeature::SlowSync));
    // The engine built at session start already runs with it.
    assert_eq!(t.core.engine_slow_sync().await, Some(true));
    t.core.shutdown().await;
}
