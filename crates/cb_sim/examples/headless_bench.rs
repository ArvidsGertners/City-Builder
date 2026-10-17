//! Headless tick benchmark (PERFORMANCE §8). M1: the empty schedules; M4: `--city 10k` builds the
//! mixed-zone citygen city of the `m4_city10k` scene first and reports its size and build time.
//! Usage: `cargo run -p cb_sim --release --example headless_bench -- [ticks] [--city 10k]
//! [--populate]` (`--populate` fills every home and matches jobs before measuring).

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
        if args.iter().any(|a| a == "--populate") {
            let t = Instant::now();
            let n = cb_sim::citygen::populate(s.world_mut());
            println!(
                "populate: {n} citizens in {:.1} s",
                t.elapsed().as_secs_f64()
            );
            label = "city10k_populated";
        }
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
    // The slowest phase-table slots (worst tick per tick-in-hour).
    let mut by_slot: std::collections::BTreeMap<u64, f64> = std::collections::BTreeMap::new();
    for (ms, t) in &worst {
        let e = by_slot.entry(*t).or_insert(0.0);
        *e = e.max(*ms);
    }
    let mut slots: Vec<(u64, f64)> = by_slot.into_iter().collect();
    slots.sort_by(|a, b| b.1.total_cmp(&a.1));
    let slots: Vec<String> = slots
        .iter()
        .take(10)
        .map(|(t, ms)| format!("{ms:.2}@{t}"))
        .collect();
    if std::env::var("CB_BENCH_CLASSES").is_ok() {
        let class = |t: u64| match t {
            500..=699 => "jobs",
            200..=263 | 300..=363 | 400..=463 => "fields",
            _ if t.is_multiple_of(2) => "commute",
            _ => "other",
        };
        let mut groups: std::collections::BTreeMap<&str, Vec<f64>> =
            std::collections::BTreeMap::new();
        for (ms, t) in &worst {
            groups.entry(class(*t)).or_default().push(*ms);
        }
        for (k, mut v) in groups {
            v.sort_by(f64::total_cmp);
            let q = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
            println!(
                "class={k} n={} p50={:.3} p95={:.3} p99={:.3}",
                v.len(),
                q(0.5),
                q(0.95),
                q(0.99)
            );
        }
    }
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
    println!("slowest slots (max ms@tick-in-hour): {}", slots.join(" "));
    let pop = s.world().resource::<cb_sim::citizens::PopulationStats>();
    let c = s.world().resource::<cb_sim::traffic::Traffic>().stats();
    println!(
        "population={} jobs_filled={}/{} commuters={} no_route={} commute_p50_min={:.0} p95_min={:.0}",
        pop.population,
        pop.jobs_filled,
        pop.jobs,
        c.commuters,
        c.no_route,
        c.p50_perceived_min,
        c.p95_perceived_min
    );
}
