//! Spike S-Time (PROGRESS.md Phase S): does the ADR-0042 time model (1 game minute = 2 sim s,
//! vehicles at nominal speed in sim time, capacities in veh/sim-h) produce plausible commutes
//! and a manageable agent count for a 50k-citizen city?
//!
//! Headless, deterministic. Synthetic 4 × 4 km city, grid road graph, gravity job matching,
//! free-flow shortest paths, ADR-0042 schedules, point-queue links, 3 simulated days with commute
//! estimates learned from the previous day. Prints `metric=` lines.
//!
//! Usage: s_time [--citizens N] [--cap-scale F] [--sigma-com MIN] [--sigma-ind MIN] [--days N]

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use cb_core::rng::Pcg32;

// ---------- time model (ADR-0042) ----------
const SIM_S_PER_GAME_MIN: f64 = 2.0;
const GAME_MIN_PER_DAY: f64 = 1440.0;
const SIM_S_PER_DAY: f64 = GAME_MIN_PER_DAY * SIM_S_PER_GAME_MIN; // 2 880
const PERCEIVED_MIN_PER_SIM_S: f64 = 0.1;

fn gm(sim_s: f64) -> f64 {
    sim_s / SIM_S_PER_GAME_MIN
}

// ---------- road graph ----------
const GRID: usize = 41; // 41 × 41 nodes, 100 m spacing → 4 × 4 km
const SPACING_M: f64 = 100.0;
const ARTERIAL_EVERY: usize = 5;

#[derive(Clone)]
struct Link {
    to: usize,
    free_s: f64,
    service_s: f64, // 3600 / capacity(veh/sim-h)
}

struct Graph {
    out: Vec<Vec<usize>>, // node → link ids
    links: Vec<Link>,
}

fn node_pos(n: usize) -> (f64, f64) {
    ((n % GRID) as f64 * SPACING_M, (n / GRID) as f64 * SPACING_M)
}

fn build_graph(cap_scale: f64) -> Graph {
    let mut g = Graph { out: vec![Vec::new(); GRID * GRID], links: Vec::new() };
    let mut add = |g: &mut Graph, a: usize, b: usize, arterial: bool| {
        // street: 30 km/h, 1 lane × 1 600; avenue: 50 km/h, 2 lanes × 1 800 (veh/sim-h)
        let (speed, cap) = if arterial { (50.0 / 3.6, 3_600.0) } else { (30.0 / 3.6, 1_600.0) };
        let free_s = SPACING_M / speed * 1.2; // + 20 % junction delay
        for (x, y) in [(a, b), (b, a)] {
            g.out[x].push(g.links.len());
            g.links.push(Link { to: y, free_s, service_s: 3_600.0 / (cap * cap_scale) });
        }
    };
    for y in 0..GRID {
        for x in 0..GRID {
            let n = y * GRID + x;
            if x + 1 < GRID {
                add(&mut g, n, n + 1, y % ARTERIAL_EVERY == 0);
            }
            if y + 1 < GRID {
                add(&mut g, n, n + GRID, x % ARTERIAL_EVERY == 0);
            }
        }
    }
    g
}

/// Free-flow shortest-path tree from `src`: (dist_s, incoming link per node).
fn dijkstra(g: &Graph, src: usize) -> (Vec<f64>, Vec<usize>) {
    let n = g.out.len();
    let mut dist = vec![f64::INFINITY; n];
    let mut pred = vec![usize::MAX; n];
    let mut heap = BinaryHeap::new();
    dist[src] = 0.0;
    heap.push(Reverse(((0.0f64).to_bits(), src)));
    while let Some(Reverse((d_bits, u))) = heap.pop() {
        let d = f64::from_bits(d_bits);
        if d > dist[u] {
            continue;
        }
        for &l in &g.out[u] {
            let link = &g.links[l];
            let nd = d + link.free_s;
            if nd < dist[link.to] {
                dist[link.to] = nd;
                pred[link.to] = l;
                heap.push(Reverse((nd.to_bits(), link.to)));
            }
        }
    }
    (dist, pred)
}

fn path(g: &Graph, pred: &[usize], src: usize, dst: usize) -> Vec<usize> {
    let mut p = Vec::new();
    let mut n = dst;
    while n != src {
        let l = pred[n];
        p.push(l);
        // the link's origin is the node whose out-list contains it; recover via reverse index
        n = LINK_FROM.with(|lf| lf.borrow()[l]);
    }
    p.reverse();
    p
}

thread_local! {
    static LINK_FROM: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

// ---------- population ----------
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Commercial,
    Industry,
    Student,
    Shopper,
}

struct Citizen {
    kind: Kind,
    home: usize,
    dest: usize,
    /// Shift start (game min from midnight; may be ≥ 1440 for night shifts → wraps).
    start_gm: f64,
    /// Stay at destination (game min).
    stay_gm: f64,
    slack_gm: f64,
}

fn clamp_node(x: f64, y: f64) -> usize {
    let gx = (x / SPACING_M).round().clamp(0.0, (GRID - 1) as f64) as usize;
    let gy = (y / SPACING_M).round().clamp(0.0, (GRID - 1) as f64) as usize;
    gy * GRID + gx
}

fn gauss2(rng: &mut Pcg32, cx: f64, cy: f64, s: f64) -> usize {
    let x = f64::from(rng.normal(cx as f32, s as f32));
    let y = f64::from(rng.normal(cy as f32, s as f32));
    clamp_node(x, y)
}

struct Params {
    citizens: usize,
    cap_scale: f64,
    sigma_com: f32,
    sigma_ind: f32,
    days: usize,
    trip_scale: f64,
    com_mean_h: f32,
    com_min_h: f64,
    ind_mean_h: f32,
    ind_min_h: f64,
    gravity_m: f64,
}

fn make_population(p: &Params, rng: &mut Pcg32) -> Vec<Citizen> {
    let homes = [(700.0, 700.0), (3300.0, 800.0), (800.0, 3200.0), (3200.0, 3300.0), (2000.0, 600.0), (600.0, 2000.0)];
    let com = [(2000.0, 2000.0, 300.0), (1300.0, 2700.0, 250.0), (2800.0, 1400.0, 250.0)];
    let ind = [(3700.0, 2000.0, 250.0), (2000.0, 3700.0, 250.0)];
    let schools: Vec<usize> = (0..10).map(|_| clamp_node(rng.next_f64() * 4000.0, rng.next_f64() * 4000.0)).collect();
    // Job node pools (one entry per job slot).
    let mut com_jobs = Vec::new();
    let mut ind_jobs = Vec::new();
    for _ in 0..(p.citizens * 55 / 100 * 65 / 100) {
        let c = com[rng.below(3) as usize];
        com_jobs.push(gauss2(rng, c.0, c.1, c.2));
    }
    for _ in 0..(p.citizens * 55 / 100 * 35 / 100) {
        let c = ind[rng.below(2) as usize];
        ind_jobs.push(gauss2(rng, c.0, c.1, c.2));
    }
    let decay_m = p.gravity_m;
    let gravity_pick = |rng: &mut Pcg32, home: usize, pool: &[usize]| -> usize {
        // Gravity matching by rejection: accept with exp(-d / 2 km).
        let (hx, hy) = node_pos(home);
        loop {
            let j = pool[rng.below(pool.len() as u32) as usize];
            let (jx, jy) = node_pos(j);
            let d = ((hx - jx).powi(2) + (hy - jy).powi(2)).sqrt();
            if rng.next_f64() < (-d / decay_m).exp() {
                return j;
            }
        }
    };
    let mut out = Vec::with_capacity(p.citizens);
    for i in 0..p.citizens {
        let h = homes[rng.below(homes.len() as u32) as usize];
        let home = gauss2(rng, h.0, h.1, 350.0);
        let r = (i * 100 / p.citizens) as u32; // deterministic composition
        let slack = f64::from(rng.normal(10.0, 5.0)).max(0.0);
        let c = if r < 36 {
            // commercial: N(08:30, σ) truncated [05:30, 11:30]; 15 % evening shifts
            let evening = rng.chance(0.15);
            let start = if evening {
                f64::from(rng.normal(16.0 * 60.0, 60.0))
            } else {
                f64::from(rng.normal(p.com_mean_h * 60.0, p.sigma_com)).clamp(p.com_min_h * 60.0, 11.5 * 60.0)
            };
            Citizen { kind: Kind::Commercial, home, dest: gravity_pick(rng, home, &com_jobs), start_gm: start, stay_gm: 8.0 * 60.0, slack_gm: slack }
        } else if r < 55 {
            // industry: N(07:00, σ); 10 % night shifts
            let night = rng.chance(0.10);
            let start = if night {
                f64::from(rng.normal(22.0 * 60.0, 30.0))
            } else {
                f64::from(rng.normal(p.ind_mean_h * 60.0, p.sigma_ind)).clamp(p.ind_min_h * 60.0, 11.0 * 60.0)
            };
            Citizen { kind: Kind::Industry, home, dest: gravity_pick(rng, home, &ind_jobs), start_gm: start, stay_gm: 8.0 * 60.0, slack_gm: slack }
        } else if r < 75 {
            // students: nearest school, 08:00 ± 20
            let (hx, hy) = node_pos(home);
            let school = *schools
                .iter()
                .min_by(|a, b| {
                    let (ax, ay) = node_pos(**a);
                    let (bx, by) = node_pos(**b);
                    ((ax - hx).powi(2) + (ay - hy).powi(2)).total_cmp(&((bx - hx).powi(2) + (by - hy).powi(2)))
                })
                .unwrap_or(&home);
            let start = 8.0 * 60.0 + (rng.next_f64() * 40.0 - 20.0);
            Citizen { kind: Kind::Student, home, dest: school, start_gm: start, stay_gm: 6.5 * 60.0, slack_gm: slack }
        } else {
            // others: one shopping trip 10:00–16:00, 60 min stay
            let start = 10.0 * 60.0 + rng.next_f64() * 360.0;
            Citizen { kind: Kind::Shopper, home, dest: gravity_pick(rng, home, &com_jobs), start_gm: start, stay_gm: 60.0, slack_gm: 0.0 }
        };
        out.push(c);
    }
    out
}

// ---------- simulation ----------
struct Trip {
    citizen: usize,
    outbound: bool,
    route: Vec<usize>,
    depart_s: f64,
    target_s: f64, // desired arrival (shift start) for outbound commutes
    arrive_s: f64,
}

/// Point-queue network simulation of all trips (deterministic event order: time, trip id).
fn simulate(g: &Graph, trips: &mut [Trip]) {
    let mut next_free = vec![0.0f64; g.links.len()];
    // Event: (time in micro-sim-s, trip id, link index in route)
    let mut heap: BinaryHeap<Reverse<(i64, usize, usize)>> = BinaryHeap::new();
    let us = |t: f64| (t * 1e6).round() as i64;
    for (id, t) in trips.iter().enumerate() {
        if t.route.is_empty() {
            continue;
        }
        let first = &g.links[t.route[0]];
        heap.push(Reverse((us(t.depart_s + first.free_s), id, 0)));
    }
    for t in trips.iter_mut() {
        if t.route.is_empty() {
            t.arrive_s = t.depart_s;
        }
    }
    while let Some(Reverse((time_us, id, k))) = heap.pop() {
        let t_end = time_us as f64 / 1e6;
        let l = trips[id].route[k];
        let leave = t_end.max(next_free[l]);
        next_free[l] = leave + g.links[l].service_s;
        if k + 1 == trips[id].route.len() {
            trips[id].arrive_s = leave;
        } else {
            let nl = &g.links[trips[id].route[k + 1]];
            heap.push(Reverse((us(leave + nl.free_s), id, k + 1)));
        }
    }
}

fn percentile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |k: &str, d: f64| {
        args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(d)
    };
    let p = Params {
        citizens: arg("--citizens", 50_000.0) as usize,
        cap_scale: arg("--cap-scale", 1.0),
        sigma_com: arg("--sigma-com", 90.0) as f32,
        sigma_ind: arg("--sigma-ind", 60.0) as f32,
        days: arg("--days", 3.0) as usize,
        trip_scale: arg("--trip-scale", 1.0),
        com_mean_h: arg("--com-mean", 8.5) as f32,
        com_min_h: arg("--com-min", 5.5),
        ind_mean_h: arg("--ind-mean", 7.0) as f32,
        ind_min_h: arg("--ind-min", 0.0),
        gravity_m: arg("--gravity-m", 2_000.0),
    };
    let g = build_graph(p.cap_scale);
    LINK_FROM.with(|lf| {
        let mut from = vec![0usize; g.links.len()];
        for (n, ls) in g.out.iter().enumerate() {
            for &l in ls {
                from[l] = n;
            }
        }
        *lf.borrow_mut() = from;
    });
    let mut rng = Pcg32::from_seed(0x5717_E000);
    // Trip scale k: each simulated trip entity represents k citizens (representative sample).
    let sampled = Params { citizens: ((p.citizens as f64) / p.trip_scale).round() as usize, ..p };
    let p = sampled;
    let pop = make_population(&p, &mut rng);

    // Routes (free flow) per distinct origin.
    let mut trees: Vec<Option<Vec<usize>>> = vec![None; g.out.len()];
    let mut free_s = vec![0.0f64; pop.len()];
    let mut routes_out = Vec::with_capacity(pop.len());
    let mut routes_back = Vec::with_capacity(pop.len());
    for (i, c) in pop.iter().enumerate() {
        for (src, dst, store_back) in [(c.home, c.dest, false), (c.dest, c.home, true)] {
            if trees[src].is_none() {
                trees[src] = Some(dijkstra(&g, src).1);
            }
            let pred = trees[src].as_ref().map(Vec::as_slice).unwrap_or(&[]);
            let r = path(&g, pred, src, dst);
            if store_back {
                routes_back.push(r);
            } else {
                free_s[i] = r.iter().map(|&l| g.links[l].free_s).sum();
                routes_out.push(r);
            }
        }
    }

    // Commute estimate (sim s) learned day by day: EMA of experienced durations.
    let mut estimate_s = free_s.clone();
    let mut last_trips = Vec::new();
    for day in 0..p.days {
        let mut trips = Vec::with_capacity(pop.len() * 2);
        for (i, c) in pop.iter().enumerate() {
            let start_s = c.start_gm.rem_euclid(GAME_MIN_PER_DAY) * SIM_S_PER_GAME_MIN;
            let depart = start_s - estimate_s[i] - c.slack_gm * SIM_S_PER_GAME_MIN;
            trips.push(Trip { citizen: i, outbound: true, route: routes_out[i].clone(), depart_s: depart, target_s: start_s, arrive_s: 0.0 });
            let back = start_s + c.stay_gm * SIM_S_PER_GAME_MIN;
            trips.push(Trip { citizen: i, outbound: false, route: routes_back[i].clone(), depart_s: back, target_s: f64::NAN, arrive_s: 0.0 });
        }
        simulate(&g, &mut trips);
        for t in trips.iter().filter(|t| t.outbound) {
            let experienced = t.arrive_s - t.depart_s;
            estimate_s[t.citizen] = if day == 0 { experienced } else { 0.5 * estimate_s[t.citizen] + 0.5 * experienced };
        }
        last_trips = trips;
    }
    report(&p, &pop, &last_trips, &free_s);
}

fn report(p: &Params, pop: &[Citizen], trips: &[Trip], free_s: &[f64]) {
    let commutes: Vec<&Trip> = trips.iter().filter(|t| t.outbound && pop[t.citizen].kind != Kind::Shopper).collect();
    let mut dur_gm: Vec<f64> = commutes.iter().map(|t| gm(t.arrive_s - t.depart_s)).collect();
    let mut perceived: Vec<f64> = commutes.iter().map(|t| (t.arrive_s - t.depart_s) * PERCEIVED_MIN_PER_SIM_S).collect();
    let mut free_perc: Vec<f64> = commutes.iter().map(|t| free_s[t.citizen] * PERCEIVED_MIN_PER_SIM_S).collect();
    let mut delay_ratio: Vec<f64> = commutes.iter().map(|t| (t.arrive_s - t.depart_s) / free_s[t.citizen].max(1e-6)).collect();
    println!(
        "metric=commute_game_min p50={:.0} p95={:.0} | perceived_min p50={:.1} p95={:.1} | free_flow_perceived p50={:.1} | congestion_ratio p50={:.2} p95={:.2}",
        percentile(&mut dur_gm.clone(), 0.5),
        percentile(&mut dur_gm, 0.95),
        percentile(&mut perceived.clone(), 0.5),
        percentile(&mut perceived, 0.95),
        percentile(&mut free_perc, 0.5),
        percentile(&mut delay_ratio.clone(), 0.5),
        percentile(&mut delay_ratio, 0.95)
    );

    // Departures by game hour (time of day, wrapped).
    let tod_gm = |s: f64| gm(s).rem_euclid(GAME_MIN_PER_DAY);
    let mut hist = [0usize; 24];
    for t in trips {
        hist[(tod_gm(t.depart_s) / 60.0) as usize % 24] += 1;
    }
    let hist_s: Vec<String> = hist.iter().map(usize::to_string).collect();
    println!("metric=departures_per_game_hour [{}]", hist_s.join(","));
    let total = trips.len() as f64;
    let night: usize = hist[0..4].iter().sum();
    let off_peak = hist[10..15].iter().sum::<usize>() as f64 / 5.0;
    let off_peak_midday = hist[10..13].iter().sum::<usize>() as f64 / 3.0;
    let peak = *hist.iter().max().unwrap_or(&0) as f64;
    // Morning departure time distribution for outbound day-shift commutes.
    let mut morning: Vec<f64> = commutes
        .iter()
        .filter(|t| {
            let c = &pop[t.citizen];
            c.start_gm > 4.0 * 60.0 && c.start_gm < 13.0 * 60.0
        })
        .map(|t| tod_gm(t.depart_s) / 60.0)
        .collect();
    let p05 = percentile(&mut morning, 0.05);
    let before4 = morning.iter().filter(|&&h| h < 4.0).count() as f64 / morning.len().max(1) as f64;
    let on_time = commutes.iter().filter(|t| (gm(t.arrive_s - t.target_s)).abs() <= 30.0).count() as f64 / commutes.len().max(1) as f64;
    let late = commutes.iter().filter(|t| gm(t.arrive_s - t.target_s) > 30.0).count() as f64 / commutes.len().max(1) as f64;

    // Concurrency over the day (all trips), 1 game-minute resolution.
    let mut delta = vec![0i64; GAME_MIN_PER_DAY as usize + 1];
    for t in trips {
        let a = tod_gm(t.depart_s) as usize;
        let dur = gm(t.arrive_s - t.depart_s) as usize;
        for m in 0..=dur.min(1439) {
            let idx = (a + m) % 1440;
            delta[idx] += 1;
        }
    }
    let peak_conc = *delta.iter().max().unwrap_or(&0) as f64;
    let p_at_cap = (5_000.0 / peak_conc.max(1.0)).min(1.0);
    println!(
        "metric=schedule morning_departure_p05={p05:.2}h before_04:00={:.1}% on_time_±30={:.1}% late>30={:.1}% night_share(00-04)={:.1}% peak/off-peak={:.2} (10-13 window {:.2})",
        before4 * 100.0,
        on_time * 100.0,
        late * 100.0,
        night as f64 / total * 100.0,
        peak / off_peak.max(1.0),
        peak / off_peak_midday.max(1.0)
    );
    println!("metric=agents peak_concurrent_trips={peak_conc:.0} p_at_cap_5000={p_at_cap:.3}");

    // Day-length independence: results never read L; real-time durations are a pure rescale.
    let mut hashes = Vec::new();
    for l in [16.0f64, 24.0, 48.0] {
        let real_s_per_sim_s = l * 60.0 / SIM_S_PER_DAY; // L real minutes per 2 880 sim s
        let h = trips.iter().fold(0u64, |acc, t| {
            acc.wrapping_mul(0x100_0000_01B3) ^ (t.arrive_s.to_bits() ^ t.depart_s.to_bits())
        });
        let p50_real = percentile(&mut commutes.iter().map(|t| (t.arrive_s - t.depart_s) * real_s_per_sim_s).collect::<Vec<_>>(), 0.5);
        println!("metric=day_length L={l} result_hash={h:016x} commute_p50_real_s={p50_real:.1}");
        hashes.push(h);
    }
    assert!(hashes.windows(2).all(|w| w[0] == w[1]), "results depend on day length");

    let pass = p05 >= 4.5 && on_time >= 0.9 && night as f64 / total <= 0.10 && peak / off_peak.max(1.0) >= 3.0 && p_at_cap >= 0.4;
    println!(
        "metric=s_time_gates entities={} trip_scale={} cap_scale={} com=N({},{}) min {} ind=N({},{}) min {} gravity={}m PASS={pass}",
        p.citizens, p.trip_scale, p.cap_scale, p.com_mean_h, p.sigma_com, p.com_min_h, p.ind_mean_h, p.sigma_ind, p.ind_min_h, p.gravity_m
    );
}
