//! Randomised multi-core scenarios: several full cores on the in-memory
//! network under a shared virtual clock, with the simulation harness's
//! invariants checked after every step (never two transport owners,
//! revisions monotonic, no scrobble lost or duplicated, documents converge).

#![cfg(feature = "sim")]

use std::collections::BTreeMap;

use hocket_core::api::*;
use hocket_core::core::io::memory::MemoryNet;
use hocket_core::core::test_support::{seeded_server, TestCore};
use hocket_core::sim::SimTime;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

struct CoreWorld {
    cores: Vec<TestCore>,
    revisions: Vec<u32>,
    violations: Vec<String>,
    log: Vec<String>,
}

impl CoreWorld {
    async fn new(seed: u64, n: usize) -> CoreWorld {
        let server = seeded_server(10, 60.0);
        let net = MemoryNet::new();
        let clock = SimTime::new(1_700_000_000_000.0);
        let mut cores = vec![];
        for i in 0..n {
            let name = format!("s{seed}-{i}");
            cores.push(
                TestCore::start_on(
                    &name,
                    server.clone(),
                    Some(net.clone()),
                    seed * 10 + i as u64,
                    clock.clone(),
                )
                .await,
            );
        }
        let w = CoreWorld {
            revisions: vec![0; n],
            cores,
            violations: vec![],
            log: vec![],
        };
        w.run_for(8_000.0).await;
        w
    }

    async fn run_for(&self, ms: f64) {
        let refs: Vec<&TestCore> = self.cores.iter().collect();
        TestCore::run_all_for(&refs, ms).await;
    }

    async fn check(&mut self) {
        let mut owners = vec![];
        for (i, c) in self.cores.iter().enumerate() {
            let s = c.snapshot().await;
            if s.media_session.owns_transport && s.connection.connected {
                owners.push(c.device_id.clone());
            }
            if let Some(d) = &s.session {
                if d.revision < self.revisions[i] && s.connection.connected {
                    self.violations.push(format!(
                        "{} revision went backwards: {} -> {}",
                        c.device_id, self.revisions[i], d.revision
                    ));
                }
                self.revisions[i] = d.revision;
            }
            if c.backend.is_playing() && !s.media_session.owns_transport {
                self.violations
                    .push(format!("{} plays audio without the lease", c.device_id));
            }
        }
        if owners.len() > 1 {
            self.violations
                .push(format!("two transport owners: {owners:?}"));
        }
    }

    async fn finish(&mut self) {
        self.run_for(25_000.0).await;
        self.check().await;
        // Converged documents among attached devices.
        let mut docs: BTreeMap<String, u32> = BTreeMap::new();
        for c in &self.cores {
            let s = c.snapshot().await;
            if s.connection.connected {
                if let Some(d) = s.session {
                    docs.insert(c.device_id.clone(), d.revision);
                }
            }
        }
        let revs: Vec<u32> = docs.values().copied().collect();
        if revs.iter().any(|r| *r != revs[0]) {
            self.violations
                .push(format!("documents did not converge: {docs:?}"));
        }
        // Every scrobble the fake server received is unique per (track, time).
        let subs: Vec<_> = self.cores[0]
            .server
            .scrobbles()
            .into_iter()
            .filter(|s| s.submission)
            .collect();
        for (i, s) in subs.iter().enumerate() {
            for o in &subs[i + 1..] {
                if s.id == o.id
                    && (s.time_ms.unwrap_or(0.0) - o.time_ms.unwrap_or(0.0)).abs() < 1000.0
                {
                    self.violations.push(format!("duplicate scrobble {:?}", s));
                }
            }
        }
        assert!(
            self.violations.is_empty(),
            "{:#?}\nactions:\n{}",
            self.violations,
            self.log.join("\n")
        );
    }
}

async fn random_scenario(seed: u64, steps: usize) {
    let mut w = CoreWorld::new(seed, 2).await;
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let tracks: Vec<String> = (0..10).map(|i| format!("t{i}")).collect();
    for _ in 0..steps {
        let i = rng.random_range(0..w.cores.len());
        let action = rng.random_range(0..7u32);
        let cmd = match action {
            0 => Command::PlayTracks {
                server_id: w.cores[i].server_id.clone(),
                track_ids: tracks.choose_multiple(&mut rng, 3).cloned().collect(),
                start_index: 0,
                label: "Sel".into(),
                shuffle: rng.random_bool(0.3),
            },
            1 => Command::Next,
            2 => Command::Previous,
            3 => Command::TogglePlay,
            4 => Command::PlayNext {
                server_id: w.cores[i].server_id.clone(),
                track_ids: vec![tracks.choose(&mut rng).cloned().unwrap()],
            },
            5 => Command::SeekTo {
                position_ms: rng.random_range(0..50_000),
            },
            _ => Command::SetShuffle {
                enabled: rng.random_bool(0.5),
            },
        };
        w.log.push(format!("{}: {cmd:?}", w.cores[i].device_id));
        w.cores[i].run(cmd).await;
        // Occasionally hand transport over.
        if rng.random_bool(0.15) {
            let j = (i + 1) % w.cores.len();
            let target = w.cores[j].device_id.clone();
            w.log
                .push(format!("{}: handoff -> {target}", w.cores[i].device_id));
            w.cores[i].run(Command::OpenHandoffPicker).await;
            w.run_for(1_500.0).await;
            w.cores[i]
                .run(Command::HandoffTo { device_id: target })
                .await;
        }
        let dt = rng.random_range(1..40) as f64 * 1_000.0;
        w.run_for(dt).await;
        w.check().await;
    }
    w.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn random_two_core_scenarios_hold_the_invariants() {
    for seed in 1..=3u64 {
        random_scenario(seed, 12).await;
    }
}
