//! World generation constraints and determinism (PROGRESS M2 checklist).

use std::time::Instant;

use cb_terrain::generate::{MIN_BUILDABLE_KM2, SHORE_REACH_M};
use cb_terrain::{WaterBodyKind, WorldGenParams, generate};

fn state_hash(w: &cb_terrain::World) -> u64 {
    w.content_hash()
}

#[test]
fn worldgen_features_seed_1_to_20() {
    let params = WorldGenParams::default();
    let mut worst_s = 0.0f64;
    for seed in 1..=20u64 {
        let t = Instant::now();
        let w = generate(&params, seed).expect("valid params");
        let secs = t.elapsed().as_secs_f64();
        worst_s = worst_s.max(secs);
        let rivers = w
            .water
            .bodies()
            .iter()
            .filter(|b| **b == WaterBodyKind::River)
            .count();
        let lakes = w
            .water
            .bodies()
            .iter()
            .filter(|b| **b == WaterBodyKind::Lake)
            .count();
        println!(
            "seed {seed}: {secs:.2} s rivers {rivers} lakes {lakes} buildable {:.2} km² shore {:.0} m connections {} rerolls {:?} relaxed {:?}",
            w.report.buildable_km2,
            w.report.shore_distance_m,
            w.outside.len(),
            w.report.rerolls,
            w.report.relaxed
        );
        assert!((1..=2).contains(&rivers), "seed {seed}: rivers {rivers}");
        assert!((1..=3).contains(&lakes), "seed {seed}: lakes {lakes}");
        assert!(
            w.report.buildable_km2 >= MIN_BUILDABLE_KM2,
            "seed {seed}: buildable {}",
            w.report.buildable_km2
        );
        assert!(
            w.report.shore_distance_m <= SHORE_REACH_M,
            "seed {seed}: shore {}",
            w.report.shore_distance_m
        );
        assert!(
            (2..=3).contains(&w.outside.len()),
            "seed {seed}: connections {}",
            w.outside.len()
        );
        assert!(
            w.report.relaxed.is_empty(),
            "seed {seed}: relaxed {:?}",
            w.report.relaxed
        );
    }
    // ≤ 4 s per world in optimized builds; unoptimized test builds only report.
    println!("worst generation time {worst_s:.2} s");
    if !cfg!(debug_assertions) {
        assert!(worst_s <= 4.0, "generation took {worst_s:.2} s");
    }
}

#[test]
fn terrain_generation_deterministic() {
    let params = WorldGenParams::default();
    let a = generate(&params, 1234).expect("valid params");
    let b = generate(&params, 1234).expect("valid params");
    assert_eq!(state_hash(&a), state_hash(&b));
    let c = generate(&params, 1235).expect("valid params");
    assert_ne!(state_hash(&a), state_hash(&c));
}

#[test]
#[ignore = "diagnostic"]
fn river_trace_diagnostic() {
    for seed in 1..=20u64 {
        let w = generate(&WorldGenParams::default(), seed).expect("valid");
        println!(
            "seed {seed}: rivers {:?}",
            w.rivers.iter().map(|r| r.points.len()).collect::<Vec<_>>()
        );
    }
}
