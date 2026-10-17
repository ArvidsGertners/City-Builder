//! Headless tick benchmark (PERFORMANCE §8). M1: the empty schedules; M4: `--city 10k` builds the
//! mixed-zone citygen city of the `m4_city10k` scene first and reports its size and build time.
//! Usage: `cargo run -p cb_sim --release --example headless_bench -- [ticks] [--city 10k]`.

use std::time::Instant;

use cb_sim::testing::Scenario;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ticks: u64 = args.iter().find_map(|a| a.parse().ok()).unwrap_or(28_800);
    let city = args.windows(2).any(|w| w[0] == "--city" && w[1] == "10k");
    let mut s = Scenario::new(1);
    let mut label = "empty";
    if city {
        let t = Instant::now();
        let rep = cb_sim::citygen::grid(
            s.world_mut(),
            &cb_sim::citygen::city10k_spec(glam::Vec2::ZERO),
        );
        println!(
            "citygen 10k: {:.1} s, {} segments, {} plots ({} rejected), {} buildings",
            t.elapsed().as_secs_f64(),
            rep.segments,
            rep.plots,
            rep.plots_rejected,
            rep.buildings + rep.civic
        );
        label = "city10k";
    }
    s.run_ticks(1);
    let mut samples = Vec::with_capacity(usize::try_from(ticks).unwrap_or(0));
    let mut worst: Vec<(f64, u64)> = Vec::new();
    for _ in 0..ticks {
        let t = Instant::now();
        s.run_ticks(1);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        samples.push(ms);
        worst.push((ms, s.ticks() % cb_core::time::TICKS_PER_GAME_HOUR));
    }
    // The slowest ticks with their tick-in-hour (phase-table slot).
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    let slots: Vec<String> = worst
        .iter()
        .take(6)
        .map(|(ms, t)| format!("{ms:.2}@{t}"))
        .collect();
    samples.sort_by(f64::total_cmp);
    let pick = |q: f64| {
        let i = ((samples.len() as f64 - 1.0) * q).round() as usize;
        samples.get(i).copied().unwrap_or(0.0)
    };
    println!(
        "bench={label} ticks={ticks} p50_ms={:.4} p95_ms={:.4} p99_ms={:.4} max_ms={:.4}",
        pick(0.5),
        pick(0.95),
        pick(0.99),
        pick(1.0)
    );
    println!("slowest (ms@tick-in-hour): {}", slots.join(" "));
}
