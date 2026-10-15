//! Headless tick benchmark (PERFORMANCE §8). M1: the empty schedules; later milestones add
//! `citygen` cities. Usage: `cargo bench-sim [ticks]`.

use std::time::Instant;

use cb_sim::testing::Scenario;

fn main() {
    let ticks: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(28_800);
    let mut s = Scenario::new(1);
    s.run_ticks(1);
    let mut samples = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    for _ in 0..ticks {
        let t = Instant::now();
        s.run_ticks(1);
        samples.push(t.elapsed().as_secs_f64() * 1e3);
    }
    samples.sort_by(f64::total_cmp);
    let pick = |q: f64| {
        let i = ((samples.len() as f64 - 1.0) * q).round() as usize;
        samples.get(i).copied().unwrap_or(0.0)
    };
    println!(
        "bench=empty ticks={ticks} p50_ms={:.4} p95_ms={:.4} p99_ms={:.4} max_ms={:.4}",
        pick(0.5),
        pick(0.95),
        pick(0.99),
        pick(1.0)
    );
}
