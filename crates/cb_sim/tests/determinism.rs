//! Determinism (ADR-0043, TESTING §5): the same seed and command sequence give the same state
//! hash at every day boundary, and results do not depend on the worker thread count (the sim
//! schedules are single-threaded and intra-system parallelism uses fixed partitions,
//! ADR-0061). The cross-OS comparison runs `determinism_probe` in CI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::catalog::ZoneKind;
use cb_sim::citygen::{self, GridSpec};
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::testing::Scenario;
use cb_sim::{GrowthMode, SimConfig, TerrainSetup};
use glam::Vec2;

const THREADS_ENV: &str = "CB_TEST_THREADS";

/// A growing town with a few commands at fixed ticks; daily hashes.
fn run(mut s: Scenario, days: u64) -> Vec<u64> {
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (4, 3),
            mixed: true,
            grow: false,
            ..GridSpec::default()
        },
    );
    let mut s = s.config(SimConfig {
        growth: GrowthMode::Normal,
        ..SimConfig::default()
    });
    let mut hashes = Vec::new();
    for day in 0..days {
        s.run_ticks(5 * TICKS_PER_GAME_HOUR);
        let r = s.command(PlayerCommand::SetTaxRate {
            zone: ZoneKind::Residential,
            bp: 800 + 100 * u16::try_from(day).unwrap(),
        });
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
        let y = 300.0 + 40.0 * day as f32;
        let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
            kind: RoadKind::Street,
            curve: CubicBezier2::straight(Vec2::new(-150.0, y), Vec2::new(150.0, y)),
            start: Endpoint::Free,
            end: Endpoint::Free,
        }));
        assert!(r.outcome.is_ok(), "{:?}", r.outcome);
        s.run_ticks(TICKS_PER_GAME_DAY - 5 * TICKS_PER_GAME_HOUR);
        hashes.push(s.state_hash());
    }
    hashes
}

#[test]
fn same_seed_same_commands_same_hash_daily() {
    let a = run(Scenario::new(21), 3);
    let b = run(Scenario::new(21), 3);
    assert_eq!(a, b);
    let c = run(Scenario::new(22), 1);
    assert_ne!(a[0], c[0], "the seed matters");
}

#[test]
fn results_identical_1_4_8_threads() {
    if let Ok(n) = std::env::var(THREADS_ENV) {
        let threads: usize = n.parse().unwrap();
        let s = Scenario::with_threads(21, TerrainSetup::None, threads);
        let h = run(s, 2);
        println!("HASHES={h:?}");
        return;
    }
    // Task pools are process-wide: run each thread count in its own test process.
    let exe = std::env::current_exe().unwrap();
    let mut seen = Vec::new();
    for threads in [1, 4, 8] {
        let out = std::process::Command::new(&exe)
            .args(["--exact", "results_identical_1_4_8_threads", "--nocapture"])
            .env(THREADS_ENV, threads.to_string())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "{threads} threads: {text}");
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix("HASHES="))
            .unwrap_or_else(|| panic!("no hashes from the {threads}-thread run: {text}"))
            .to_owned();
        seen.push((threads, line));
    }
    assert!(
        seen.windows(2).all(|w| w[0].1 == w[1].1),
        "thread count changed results: {seen:?}"
    );
}
