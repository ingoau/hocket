//! Snapshot test of the AutoMix planner over the committed corpus pairs
//! (`src/automix/corpus/`): analysis records of real tracks (made by
//! `hocket-mixlab batch --reference`) re-planned and compared with the
//! reference plans, so any planner change shows up here.
//!
//! After an intended planner change, re-bless the plans (no audio needed):
//! `HOCKET_BLESS=1 cargo test -p hocket-core --test automix_corpus`.

use std::path::PathBuf;

use hocket_core::api::AnalysisRecord;
use hocket_core::automix::record::compact;
use hocket_core::automix::{
    plan_transition, PlanInput, PlanTrack, TransitionMode, TransitionPlan, TransitionSettings,
};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/automix/corpus")
}

fn pairs() -> Vec<(String, String, String)> {
    let text = std::fs::read_to_string(dir().join("pairs.txt")).unwrap();
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut it = line.split_whitespace();
            let (a, b) = (it.next()?, it.next()?);
            Some((a.into(), b.into(), it.collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

fn record(id: &str) -> AnalysisRecord {
    let path = dir().join("records").join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap()
}

#[test]
fn records_are_compact() {
    for (a, b, _) in pairs() {
        for id in [a, b] {
            let r = record(&id);
            assert_eq!(compact(r.clone()), r, "record {id} is not compacted");
        }
    }
}

#[test]
fn corpus_plans_match_the_reference() {
    let settings = TransitionSettings {
        mode: TransitionMode::AutoMix,
        crossfade_seconds: 6.0,
    };
    let bless = std::env::var_os("HOCKET_BLESS").is_some();
    let mut failures = Vec::new();
    let pairs = pairs();
    assert!(pairs.len() >= 10, "pair list too short");
    for (a, b, label) in pairs {
        let (ra, rb) = (record(&a), record(&b));
        let plan = plan_transition(&PlanInput {
            settings: &settings,
            a: PlanTrack {
                duration_ms: ra.duration_ms,
                record: Some(&ra),
            },
            b: PlanTrack {
                duration_ms: rb.duration_ms,
                record: Some(&rb),
            },
            fixed_tail_start_ms: None,
        })
        .unwrap();
        let path = dir().join("plans").join(format!("{a}_{b}.json"));
        if bless {
            std::fs::write(&path, serde_json::to_string_pretty(&plan).unwrap() + "\n").unwrap();
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let reference: TransitionPlan = serde_json::from_str(&text).unwrap();
        if plan != reference {
            failures.push(format!(
                "{a} -> {b} ({label}):\n  reference {}\n  now       {}",
                serde_json::to_string(&reference).unwrap(),
                serde_json::to_string(&plan).unwrap()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "plans changed (HOCKET_BLESS=1 to accept):\n{}",
        failures.join("\n")
    );
}
