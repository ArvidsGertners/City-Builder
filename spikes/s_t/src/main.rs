//! Spike S-T — traffic at scale (docs/PROGRESS.md Phase S).
//!
//! `cargo run --release -- [--case N] [--days D] [--threads T]` prints `metric=` lines.

mod net;
mod pool;
mod route;
mod sim;

use cb_core::rng::Pcg32;
use glam::Vec2;

use net::{Class, Ctrl, Net};
use route::{CachePolicy, Key, Router};
use sim::{NONE, On, Sim};

fn args() -> (Option<u32>, f32, usize) {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    (
        get("--case").and_then(|v| v.parse().ok()),
        get("--days").and_then(|v| v.parse().ok()).unwrap_or(2.0),
        get("--threads").and_then(|v| v.parse().ok()).unwrap_or(4),
    )
}

fn pct(v: &mut [f32], q: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f32::total_cmp);
    v[((v.len() - 1) as f32 * q) as usize]
}

fn report_common(tag: &str, sim: &mut Sim) {
    let st = &mut sim.stats;
    let completed = st.arrived + st.macro_spawn_timeout + st.despawn_stuck + st.despawn_cycle;
    let desp = st.despawn_stuck + st.despawn_cycle;
    println!(
        "metric={tag}.trips requested={} spawned={} arrived={} spawn_timeouts={} no_route={} despawn_stuck={} despawn_cycle={} per_1000={:.3} reroutes={} lane_changes={}",
        st.trips_requested,
        st.spawned,
        st.arrived,
        st.macro_spawn_timeout,
        st.trips_no_route,
        st.despawn_stuck,
        st.despawn_cycle,
        1000.0 * desp as f64 / completed.max(1) as f64,
        st.reroutes,
        st.lane_changes
    );
    println!(
        "metric={tag}.safety overlaps={} zone_conflicts={} (gate 0)",
        st.overlaps, st.zone_conflicts
    );
    let (mut a, mut b, mut c, mut d) = (
        st.t_par.clone(),
        st.t_seq.clone(),
        st.t_junc.clone(),
        st.t_route.clone(),
    );
    let veh: Vec<f32> = st.t_par.iter().zip(&st.t_seq).map(|(x, y)| x + y).collect();
    let mut veh = veh;
    let names = [
        "apply_par",
        "merge",
        "transfers",
        "releases",
        "lanes",
        "spawn",
        "stuck",
        "j_collect",
        "j_grant",
        "signals",
    ];
    let parts: Vec<String> = (0..10)
        .map(|k| {
            let mut v = st.t_parts[k].clone();
            format!("{}={:.3}", names[k], pct(&mut v, 0.95))
        })
        .collect();
    println!("metric={tag}.seq_breakdown_p95_ms {}", parts.join(" "));
    {
        let mut rn: Vec<f32> = st.req_n.iter().map(|x| *x as f32).collect();
        let mut ts = st.t_sort.clone();
        let mut bq: Vec<f32> = st.route_q_len.iter().map(|x| *x as f32).collect();
        let mut nb: Vec<f32> = st.batches.iter().map(|x| *x as f32).collect();
        let mut tr = st.t_route.clone();
        let tot_units: u64 = st.units.iter().sum();
        let tot_ms: f32 = tr.iter().sum();
        println!(
            "metric={tag}.cache_conditional repeat_requests={} hits={} hit_pct_of_repeats={:.1} absent={} rejected={}",
            st.repeat_requests,
            sim.router.hits,
            sim.router.hits as f64 * 100.0 / st.repeat_requests.max(1) as f64,
            sim.router.absent,
            sim.router.rejected
        );
        println!(
            "metric={tag}.routing_misses n={} units_per_miss={:.0}",
            sim.router.misses,
            (sim.router.units_total as f64 - sim.router.hits as f64)
                / sim.router.misses.max(1) as f64
        );
        println!(
            "metric={tag}.routing_detail ns_per_unit={:.0} batches_p99={:.0} route_queue_p99={:.0} route_queue_max={:.0} routing_p50_ms={:.3}",
            tot_ms as f64 * 1e6 / tot_units.max(1) as f64,
            pct(&mut nb, 0.99),
            pct(&mut bq.clone(), 0.99),
            pct(&mut bq, 1.0),
            pct(&mut tr, 0.5)
        );
        let mut rl: Vec<f32> = st.route_lat.iter().map(|x| *x as f32).collect();
        let mut rr: Vec<f32> = st.reroute_lat.iter().map(|x| *x as f32).collect();
        println!(
            "metric={tag}.route_latency_ticks trip_p50={:.0} trip_p99={:.0} trip_max={:.0} reroute_p99={:.0} reroute_max={:.0}",
            pct(&mut rl.clone(), 0.5),
            pct(&mut rl.clone(), 0.99),
            pct(&mut rl, 1.0),
            pct(&mut rr.clone(), 0.99),
            pct(&mut rr, 1.0)
        );
        {
            // Planned V/C per link and game hour (capacity: street 1 600, avenue 1 800 veh/sim-h/lane;
            // 1 game hour = 120 sim s, so demand per game hour × 30 = per sim-hour).
            let cap = |l: u32| {
                let k = &sim.net.links[l as usize];
                f32::from(k.lanes)
                    * if k.class == net::Class::Avenue {
                        1800.0
                    } else {
                        1600.0
                    }
            };
            let mut vc_max: std::collections::BTreeMap<u32, f32> =
                std::collections::BTreeMap::new();
            for (&(_, l), &n) in &st.demand {
                let vc = n as f32 * 30.0 / cap(l);
                let e = vc_max.entry(l).or_insert(0.0);
                *e = e.max(vc);
            }
            let over = vc_max.values().filter(|v| **v > 1.0).count();
            let mut all: Vec<f32> = vc_max.values().copied().collect();
            let mut at_stuck: Vec<f32> = st
                .stuck_targets
                .iter()
                .map(|l| *vc_max.get(l).unwrap_or(&0.0))
                .collect();
            let below = at_stuck.iter().filter(|v| **v <= 1.0).count();
            println!(
                "metric={tag}.demand_vc links_over_1={over} vc_p99={:.2} vc_max={:.2} stuck_target_vc_p50={:.2} stuck_with_target_vc_le_1={below}/{}",
                pct(&mut all.clone(), 0.99),
                pct(&mut all, 1.0),
                pct(&mut at_stuck.clone(), 0.5),
                at_stuck.len()
            );
            let _ = &mut at_stuck;
            // Node V/C against capacities measured in cases 1, 2, 5 (veh/sim-h): equal 1 846 total,
            // signal 457 per approach, ring 512 per ring node, priority: major links at link
            // capacity + 400 per minor approach.
            let ncap = |n: u32| -> f32 {
                let nd = &sim.net.nodes[n as usize];
                let inc: Vec<u32> = nd
                    .inc
                    .iter()
                    .copied()
                    .filter(|l| sim.net.links[*l as usize].alive)
                    .collect();
                match nd.ctrl {
                    net::NodeCtrl::None => f32::INFINITY,
                    net::NodeCtrl::Equal => 1846.0,
                    net::NodeCtrl::Signal => 457.0 * inc.len() as f32,
                    net::NodeCtrl::Ring => 512.0,
                    net::NodeCtrl::Priority => inc
                        .iter()
                        .map(|l| {
                            if sim.net.links[*l as usize].approach == net::Approach::Priority {
                                cap(*l)
                            } else {
                                400.0
                            }
                        })
                        .sum(),
                }
            };
            let mut nvc: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
            for (&(_, n), &d) in &st.node_demand {
                let vc = d as f32 * 30.0 / ncap(n);
                let e = nvc.entry(n).or_insert(0.0);
                *e = e.max(vc);
            }
            let nover = nvc.values().filter(|v| **v > 1.0).count();
            let feasible = st
                .stuck_nodes
                .iter()
                .filter(|(a, b)| {
                    *nvc.get(a).unwrap_or(&0.0) <= 1.0 && *nvc.get(b).unwrap_or(&0.0) <= 1.0
                })
                .count();
            let trips = st.arrived + st.despawn_stuck + st.despawn_cycle;
            println!(
                "metric={tag}.node_vc nodes_over_1={nover}/{} despawns_at_feasible_nodes={feasible} per_1000_feasible={:.2} (gate < 1 at V/C ≤ 1.0)",
                nvc.len(),
                feasible as f64 * 1000.0 / trips.max(1) as f64
            );
        }
        println!(
            "metric={tag}.junction_detail req_per_tick_p95={:.0} reeval_per_tick={:.2} sort_p95_ms={:.3}",
            pct(&mut rn, 0.95),
            st.reeval as f64 / st.req_n.len().max(1) as f64,
            pct(&mut ts, 0.95)
        );
    }
    println!(
        "metric={tag}.timing_ms vehicle_step_p95={:.3} (par p95 {:.3}, seq p95 {:.3}) junction_p95={:.3} routing_p99={:.3}",
        pct(&mut veh, 0.95),
        pct(&mut a, 0.95),
        pct(&mut b, 0.95),
        pct(&mut c, 0.95),
        pct(&mut d, 0.99)
    );
}

fn new_sim(net: Net, threads: usize, policy: CachePolicy) -> Sim {
    let router = Router::new(&net, policy);
    Sim::new(net, router, threads)
}

/// Star junction: centre + arms; returns (net, inbound links, outbound links per arm).
fn star(arms: &[(f32, Class, u8)], arm_len: f32, ctrl: Ctrl) -> (Net, Vec<u32>, Vec<u32>) {
    let mut net = Net::default();
    let c = net.add_node(Vec2::ZERO, ctrl);
    let mut inb = Vec::new();
    let mut outb = Vec::new();
    for &(deg, class, lanes) in arms {
        let d = Vec2::new(deg.to_radians().cos(), deg.to_radians().sin());
        let e = net.add_node(d * arm_len, Ctrl::Auto);
        inb.push(net.add_link(e, c, class, lanes));
        outb.push(net.add_link(c, e, class, lanes));
    }
    net.finalize();
    (net, inb, outb)
}

fn run(
    sim: &mut Sim,
    ticks: u64,
    mut demand: impl FnMut(&mut Sim, &mut Pcg32),
    rng: &mut Pcg32,
    mut each: impl FnMut(&Sim),
) {
    for _ in 0..ticks {
        demand(sim, rng);
        sim.step();
        each(sim);
    }
}

/// Poisson-ish arrivals: rate per sim second.
fn arrivals(rng: &mut Pcg32, rate: f32) -> u32 {
    let mut n = 0;
    let p = rate * sim::DT;
    let mut x = p;
    while x > 0.0 {
        if rng.next_f32() < x.min(1.0) {
            n += 1;
        }
        x -= 1.0;
    }
    n
}

fn case1(threads: usize) {
    // 4-way equal-priority street junction, saturated for 1 game hour (1 800 sim s).
    let (net, inb, outb) = star(
        &[
            (0.0, Class::Street, 1),
            (90.0, Class::Street, 1),
            (180.0, Class::Street, 1),
            (270.0, Class::Street, 1),
        ],
        300.0,
        Ctrl::Auto,
    );
    let mut sim = new_sim(net, threads, CachePolicy::Epoch);
    let mut rng = Pcg32::from_seed(1);
    let mut idle = 0.0f32;
    let mut worst_idle = 0.0f32;
    let mut last_grants = 0;
    run(
        &mut sim,
        18_000,
        |s, r| {
            for a in 0..4 {
                for _ in 0..arrivals(r, 0.35) {
                    let d = (a + 1 + r.below(3) as usize) % 4;
                    s.request(Key {
                        o: inb[a],
                        d: outb[d],
                        variant: 0,
                    });
                }
            }
        },
        &mut rng,
        |s| {
            let waiting = s.veh.iter().any(|x| {
                x.on != On::Off
                    && x.want != NONE
                    && !x.granted
                    && x.v < 0.5
                    && matches!(x.on, On::Lane(l) if s.net.lanes[l as usize].len - x.s < 3.0)
            });
            // Idle = nobody in the junction (no vehicle on a connector, none granted) while some wait.
            let busy = s.conn_q.iter().any(|q| !q.is_empty())
                || s.veh.iter().any(|x| x.on != On::Off && x.granted);
            if s.stats.grants != last_grants || !waiting || busy {
                idle = 0.0;
            } else {
                idle += sim::DT;
                worst_idle = worst_idle.max(idle);
            }
            last_grants = s.stats.grants;
        },
    );
    println!(
        "metric=case1.max_idle_while_waiting_s value={worst_idle:.1} (gate ≤ 6) throughput_veh_h={:.0}",
        sim.stats.arrived as f32 * 2.0
    );
    report_common("case1", &mut sim);
}

fn case2(threads: usize) {
    // Signalised avenue × avenue (2 lanes per direction; through+right and protected-left
    // phases per axis). Phase A measures saturation headways and capacity; then V/C runs.
    let arms: Vec<(f32, Class, u8)> = [0.0, 90.0, 180.0, 270.0]
        .iter()
        .map(|d| (*d, Class::Avenue, 2))
        .collect();
    let mk = || star(&arms, 450.0, Ctrl::Signal);
    let dest = |r: &mut Pcg32, a: usize| {
        let x = r.next_f32();
        if x < 0.7 {
            (a + 2) % 4
        } else if x < 0.85 {
            (a + 1) % 4
        } else {
            (a + 3) % 4
        }
    };
    // A: saturation.
    let (net, inb, outb) = mk();
    let mut sim = new_sim(net, threads, CachePolicy::Epoch);
    sim.record_passes = true;
    let mut rng = Pcg32::from_seed(2);
    let mut queued_at_onset: Vec<(u64, Vec<u32>)> = Vec::new();
    let mut prev_stage = 9u8;
    let mut prev_phase = 99usize;
    for _ in 0..18_000u64 {
        for a in 0..4 {
            for _ in 0..arrivals(&mut rng, 0.6) {
                let d = dest(&mut rng, a);
                sim.request(Key {
                    o: inb[a],
                    d: outb[d],
                    variant: 0,
                });
            }
        }
        sim.step();
        let sg = sim.signals[0];
        if sg.stage == 0 && (prev_stage != 0 || prev_phase != sg.phase) {
            // Green onset: per lane, the vehicles standing within 80 m whose movement is green.
            let ph = &sim.net.nodes[0].phases[sg.phase];
            let tb = sim.net.nodes[0].phase_turns[sg.phase];
            for &l in ph {
                let k = &sim.net.links[l as usize];
                for i in 0..k.lanes {
                    let lane = (k.lane0 + u32::from(i)) as usize;
                    let len = sim.net.lanes[lane].len;
                    let mut q = Vec::new();
                    for &v in &sim.lane_q[lane] {
                        let x = &sim.veh[v as usize];
                        if len - x.s > 80.0 || x.v > 1.0 {
                            break;
                        }
                        if x.want == NONE || sim.net.conns[x.want as usize].turn.bit() & tb == 0 {
                            break;
                        }
                        q.push(v);
                    }
                    if q.len() >= 6 {
                        queued_at_onset.push((sim.tick, q));
                    }
                }
            }
        }
        prev_stage = sg.stage;
        prev_phase = sg.phase;
    }
    let mut pass_ticks: std::collections::HashMap<u32, Vec<u64>> = std::collections::HashMap::new();
    for p in &sim.stats.passes {
        pass_ticks.entry(p.3).or_default().push(p.2);
    }
    let mut hws: Vec<f32> = Vec::new();
    for (onset, q) in &queued_at_onset {
        // First pass of each queued vehicle after the onset, within the same green (≤ 40 s).
        let t: Vec<u64> = q
            .iter()
            .filter_map(|v| {
                pass_ticks.get(v).and_then(|ts| {
                    ts.iter()
                        .copied()
                        .find(|t| *t >= *onset && *t < onset + 400)
                })
            })
            .collect();
        if t.len() >= 6 {
            // From the 4th vehicle on (start-up lost time excluded).
            for w in t[3..].windows(2) {
                if w[1] > w[0] {
                    hws.push((w[1] - w[0]) as f32 * sim::DT);
                }
            }
        }
    }
    let mean_hw = hws.iter().sum::<f32>() / hws.len().max(1) as f32;
    let cap_veh_s = sim.stats.arrived as f32 / 1800.0 / 4.0; // per approach
    println!(
        "metric=case2.saturation_headway_s mean={mean_hw:.2} samples={} discharge_veh_h_per_lane_green={:.0} (gate ≥ 1500)",
        hws.len(),
        3600.0 / mean_hw.max(0.01)
    );
    println!(
        "metric=case2.capacity_per_approach_veh_h value={:.0}",
        cap_veh_s * 3600.0
    );
    report_common("case2_saturated", &mut sim);
    for vc in [0.9f32, 1.2] {
        let (net, inb, outb) = mk();
        let mut sim = new_sim(net, threads, CachePolicy::Epoch);
        let mut rng = Pcg32::from_seed(22);
        let high = 6000u64;
        let mut rec = f32::INFINITY;
        let mut peak = 0usize;
        for t in 0..14_000u64 {
            let rate = if t < high {
                vc * cap_veh_s
            } else {
                0.3 * cap_veh_s
            };
            for a in 0..4 {
                for _ in 0..arrivals(&mut rng, rate) {
                    let d = dest(&mut rng, a);
                    sim.request(Key {
                        o: inb[a],
                        d: outb[d],
                        variant: 0,
                    });
                }
            }
            sim.step();
            let waiting: usize = sim.spawn_q.values().map(|v| v.len()).sum();
            let stopped = sim
                .veh
                .iter()
                .filter(|x| x.on != On::Off && x.v < 0.5)
                .count();
            if t < high {
                peak = peak.max(waiting + stopped);
            } else if rec.is_infinite() && waiting == 0 && stopped <= 12 {
                rec = (t - high) as f32 * sim::DT;
            }
        }
        println!(
            "metric=case2_vc{vc}.recovery_s value={rec:.0} (gate ≤ 180 at V/C 1.2) peak_queue={peak}"
        );
        report_common(&format!("case2_vc{vc}"), &mut sim);
    }
}

fn case3(threads: usize) {
    // One-way 3-lane avenue with two signals 25 m (lane length) apart; 60 % turn left at the
    // second node from the rightmost lane (2 changes needed; plans should start upstream).
    let mut net = Net::default();
    let w = net.add_node(Vec2::new(-400.0, 0.0), Ctrl::Auto);
    let a = net.add_node(Vec2::ZERO, Ctrl::Signal);
    let b = net.add_node(Vec2::new(61.0, 0.0), Ctrl::Signal);
    let e = net.add_node(Vec2::new(400.0, 0.0), Ctrl::Auto);
    let an = net.add_node(Vec2::new(0.0, 300.0), Ctrl::Auto);
    let as_ = net.add_node(Vec2::new(0.0, -300.0), Ctrl::Auto);
    let bn = net.add_node(Vec2::new(61.0, 300.0), Ctrl::Auto);
    let bs = net.add_node(Vec2::new(61.0, -300.0), Ctrl::Auto);
    let up = net.add_link(w, a, Class::Avenue, 3);
    let block = net.add_link(a, b, Class::Avenue, 3);
    let down = net.add_link(b, e, Class::Avenue, 3);
    let _ = (block, down);
    net.add_road(a, an, Class::Street, 1, 1);
    net.add_road(a, as_, Class::Street, 1, 1);
    net.add_road(b, bn, Class::Street, 1, 1);
    net.add_road(b, bs, Class::Street, 1, 1);
    net.finalize();
    println!(
        "metric=case3.block_lane_len_m value={:.1}",
        net.links[block as usize].len
    );
    let left_out = net.nodes[b as usize]
        .out
        .iter()
        .copied()
        .find(|l| net.links[*l as usize].to == bn)
        .unwrap_or(down);
    let cross_in: Vec<u32> = [an, as_, bn, bs]
        .iter()
        .filter_map(|n| net.nodes[*n as usize].out.first().copied())
        .collect();
    let sinks: Vec<u32> = [as_, bs, an]
        .iter()
        .filter_map(|n| net.nodes[*n as usize].inc.first().copied())
        .collect();
    let mut sim = new_sim(net, threads, CachePolicy::Epoch);
    let mut rng = Pcg32::from_seed(3);
    run(
        &mut sim,
        18_000,
        |s, r| {
            for _ in 0..arrivals(r, 0.45) {
                let d = if r.next_f32() < 0.6 { left_out } else { down };
                s.request(Key {
                    o: up,
                    d,
                    variant: 0,
                });
            }
            for &ci in &cross_in {
                for _ in 0..arrivals(r, 0.05) {
                    let d = sinks[r.below(sinks.len() as u32) as usize];
                    s.request(Key {
                        o: ci,
                        d,
                        variant: 0,
                    });
                }
            }
        },
        &mut rng,
        |_| {},
    );
    report_common("case3", &mut sim);
}

fn case4(threads: usize) {
    // 2 → 1 lane merge.
    let mut net = Net::default();
    let a = net.add_node(Vec2::new(-500.0, 0.0), Ctrl::Auto);
    let m = net.add_node(Vec2::ZERO, Ctrl::Auto);
    let b = net.add_node(Vec2::new(500.0, 0.0), Ctrl::Auto);
    let l1 = net.add_link(a, m, Class::Avenue, 2);
    let l2 = net.add_link(m, b, Class::Street, 1);
    net.finalize();
    let mut sim = new_sim(net, threads, CachePolicy::Epoch);
    let mut rng = Pcg32::from_seed(4);
    run(
        &mut sim,
        18_000,
        |s, r| {
            for _ in 0..arrivals(r, 0.42) {
                s.request(Key {
                    o: l1,
                    d: l2,
                    variant: 0,
                });
            }
        },
        &mut rng,
        |_| {},
    );
    println!(
        "metric=case4.throughput_veh_h value={:.0}",
        sim.stats.arrived as f32 * 2.0
    );
    report_common("case4", &mut sim);
}

fn ring_net() -> (Net, Vec<u32>, Vec<u32>) {
    let mut net = Net::default();
    let ring: Vec<u32> = (0..6)
        .map(|k| {
            let a = (k as f32 * 60.0).to_radians();
            net.add_node(Vec2::new(a.cos(), a.sin()) * 30.0, Ctrl::Auto)
        })
        .collect();
    for k in 0..6 {
        net.add_link(ring[k], ring[(k + 1) % 6], Class::Ring, 1);
    }
    let mut inb = Vec::new();
    let mut outb = Vec::new();
    for k in 0..6 {
        let a = (k as f32 * 60.0).to_radians();
        let e = net.add_node(Vec2::new(a.cos(), a.sin()) * 300.0, Ctrl::Auto);
        inb.push(net.add_link(e, ring[k], Class::Street, 1));
        outb.push(net.add_link(ring[k], e, Class::Street, 1));
    }
    net.finalize();
    (net, inb, outb)
}

fn case5(threads: usize) {
    // 6-entry single-lane roundabout (radius 30 m): measure capacity under saturation, then run
    // at V/C 1.0 of that capacity.
    let mut cap_veh_h = 0.0f32;
    for (phase, rate) in [("saturated", 0.16f32), ("vc1", 0.0)] {
        let (net, inb, outb) = ring_net();
        let mut sim = new_sim(net, threads, CachePolicy::Epoch);
        let mut rng = Pcg32::from_seed(5);
        let r_entry = if phase == "vc1" {
            cap_veh_h / 3600.0 / 6.0
        } else {
            rate
        };
        run(
            &mut sim,
            18_000,
            |s, r| {
                for k in 0..6 {
                    for _ in 0..arrivals(r, r_entry) {
                        let d = (k + 1 + r.below(5) as usize) % 6;
                        s.request(Key {
                            o: inb[k],
                            d: outb[d],
                            variant: 0,
                        });
                    }
                }
            },
            &mut rng,
            |_| {},
        );
        let thr = sim.stats.arrived as f32 * 2.0;
        if phase == "saturated" {
            cap_veh_h = thr;
        }
        println!(
            "metric=case5_{phase}.throughput_veh_h value={thr:.0} entry_rate_veh_h={:.0}",
            r_entry * 3600.0 * 6.0
        );
        report_common(&format!("case5_{phase}"), &mut sim);
    }
}

/// Synthetic 4 × 4 km city: jittered grid (≈ 75 m), avenues every 8th line (signals at
/// avenue × avenue), one-way street rows, removed segments (T), diagonals (Y/5-way), a ring.
pub fn city(seed: u64) -> (Net, Vec<u32>, Vec<u32>) {
    let mut rng = Pcg32::from_seed(seed);
    let n = 54usize;
    let sp = 4000.0 / (n - 1) as f32;
    let mut net = Net::default();
    let is_av = |i: usize| i % 8 == 4;
    let mut ids = vec![0u32; n * n];
    let ring_c = (n / 2 + 1, n / 2 + 1);
    for j in 0..n {
        for i in 0..n {
            let jit = if is_av(i) || is_av(j) { 0.0 } else { 8.0 };
            let p = Vec2::new(
                i as f32 * sp + (rng.next_f32() - 0.5) * jit,
                j as f32 * sp + (rng.next_f32() - 0.5) * jit,
            );
            let ctrl = if is_av(i) && is_av(j) {
                Ctrl::Signal
            } else {
                Ctrl::Auto
            };
            ids[j * n + i] = net.add_node(p, ctrl);
        }
    }
    let mut diag = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            let (ci, cj) = ring_c;
            let near_ring = i.abs_diff(ci) <= 1 && j.abs_diff(cj) <= 1;
            // Horizontal segment (i,j)–(i+1,j).
            if i + 1 < n && !near_ring {
                let av = is_av(j);
                let removed = !av && rng.chance(0.06);
                if !removed {
                    let (a, b) = (ids[j * n + i], ids[j * n + i + 1]);
                    if av {
                        net.add_road(a, b, Class::Avenue, 2, 2);
                    } else if j % 4 == 2 {
                        if (j / 4) % 2 == 0 {
                            net.add_road(a, b, Class::Street, 2, 0)
                        } else {
                            net.add_road(b, a, Class::Street, 2, 0)
                        }
                    } else {
                        net.add_road(a, b, Class::Street, 1, 1);
                    }
                }
            }
            if j + 1 < n && !near_ring {
                let av = is_av(i);
                let removed = !av && rng.chance(0.06);
                if !removed {
                    let (a, b) = (ids[j * n + i], ids[(j + 1) * n + i]);
                    if av {
                        net.add_road(a, b, Class::Avenue, 2, 2);
                    } else {
                        net.add_road(a, b, Class::Street, 1, 1);
                    }
                }
            }
            // Diagonals (Y / 5-way), not in neighbouring cells.
            if i + 1 < n
                && j + 1 < n
                && !is_av(i)
                && !is_av(j)
                && !is_av(i + 1)
                && !is_av(j + 1)
                && !near_ring
                && rng.chance(0.12)
            {
                let free = (0..3).all(|dj| {
                    (0..3).all(|di| {
                        let (x, y) = ((i + di).saturating_sub(1), (j + dj).saturating_sub(1));
                        !diag[y.min(n - 1) * n + x.min(n - 1)]
                    })
                });
                if free {
                    diag[j * n + i] = true;
                    net.add_road(
                        ids[j * n + i],
                        ids[(j + 1) * n + i + 1],
                        Class::Street,
                        1,
                        1,
                    );
                }
            }
        }
    }
    // 6-entry ring around the centre node.
    let (ci, cj) = ring_c;
    let c = net.nodes[ids[cj * n + ci] as usize].pos;
    let ring: Vec<u32> = (0..6)
        .map(|k| {
            let a = (k as f32 * 60.0 + 15.0).to_radians();
            net.add_node(c + Vec2::new(a.cos(), a.sin()) * 30.0, Ctrl::Auto)
        })
        .collect();
    for k in 0..6 {
        net.add_link(ring[k], ring[(k + 1) % 6], Class::Ring, 1);
    }
    let around = [
        (ci + 2, cj),
        (ci + 2, cj + 2),
        (ci, cj + 2),
        (ci - 2, cj),
        (ci - 2, cj - 2),
        (ci, cj - 2),
    ];
    for (k, (x, y)) in around.iter().enumerate() {
        net.add_road(ring[k], ids[y * n + x], Class::Street, 1, 1);
    }
    net.finalize();
    // Residential origins = street links; jobs concentrated on avenue-adjacent links.
    let homes: Vec<u32> = (0..net.links.len() as u32)
        .filter(|l| net.links[*l as usize].class == Class::Street)
        .collect();
    let jobs: Vec<u32> = (0..net.links.len() as u32)
        .filter(|l| {
            let k = &net.links[*l as usize];
            let p = (net.nodes[k.from as usize].pos + net.nodes[k.to as usize].pos) * 0.5;
            let cell = |v: f32| ((v / sp).round() as usize) % 8;
            k.class != Class::Ring
                && (cell(p.x) == 4
                    || cell(p.y) == 4
                    || ((p - Vec2::splat(2000.0)).length() < 600.0 && k.class == Class::Street))
        })
        .collect();
    (net, homes, jobs)
}

/// Daily activity shape (0..1) by game hour: peaks 7–9 and 16–18.
fn shape(h: f32) -> f32 {
    let g = |c: f32, w: f32| (-(h - c) * (h - c) / (2.0 * w * w)).exp();
    (0.15 + 0.85 * g(8.0, 1.2).max(g(17.0, 1.4)) + 0.35 * g(12.5, 2.5)).min(1.0)
}

fn case6(
    threads: usize,
    cap: usize,
    days: f32,
    policy: CachePolicy,
    bulldoze: bool,
    tag: &str,
) -> u64 {
    let (net, homes, jobs) = city(6);
    if tag.ends_with("_5000") || tag == "case8_t1" {
        let segs = net.links.len();
        println!(
            "metric=city links={} nodes={} conns={} zones={} homes={} jobs={}",
            segs,
            net.nodes.len(),
            net.conns.len(),
            net.zones.len(),
            homes.len(),
            jobs.len()
        );
    }
    let mut sim = new_sim(net, threads, policy);
    sim.check = true;
    let mut rng = Pcg32::from_seed(66);
    // Commuter pool: households with a home link, work link and route variant.
    let pool: Vec<(u32, u32, u8)> = (0..cap * 6)
        .map(|_| {
            (
                homes[rng.below(homes.len() as u32) as usize],
                jobs[rng.below(jobs.len() as u32) as usize],
                rng.below(4) as u8,
            )
        })
        .collect();
    let ticks = (days * 28_800.0) as u64;
    let mut peak = 0usize;
    let mut bulldozed = false;
    let mut hit_by_day: Vec<(u64, u64)> = Vec::new();
    for t in 0..ticks {
        let hour = (t % 28_800) as f32 / 1200.0;
        if t % 28_800 == 0 && t > 0 {
            hit_by_day.push((sim.router.hits, sim.router.misses));
        }
        let target = (cap as f32 * shape(hour)) as usize;
        let active =
            sim.active() + sim.route_q.len() + sim.spawn_q.values().map(|q| q.len()).sum::<usize>();
        if active < target {
            for _ in 0..(target - active).min(25) {
                let (h, w, var) = pool[rng.below(pool.len() as u32) as usize];
                let to_work = (5.0..12.0).contains(&hour)
                    || ((12.0..16.0).contains(&hour) && rng.chance(0.5));
                let (o, d) = if to_work { (h, w) } else { (w, h) };
                sim.request(Key { o, d, variant: var });
            }
        }
        if bulldoze && !bulldozed && t == 28_800 + 8 * 1200 {
            // Bulldoze a central avenue run (both directions, 6 segments).
            let victims: Vec<u32> = (0..sim.net.links.len() as u32)
                .filter(|l| {
                    let k = &sim.net.links[*l as usize];
                    let p = (sim.net.nodes[k.from as usize].pos + sim.net.nodes[k.to as usize].pos)
                        * 0.5;
                    k.class == Class::Avenue
                        && (p.y - 4.0 * 4000.0 / 53.0 * 3.0).abs() < 5.0
                        && (1700.0..2200.0).contains(&p.x)
                })
                .collect();
            for l in &victims {
                sim.bulldoze(*l);
            }
            println!("metric={tag}.bulldozed_links value={}", victims.len());
            bulldozed = true;
        }
        sim.step();
        peak = peak.max(sim.active());
    }
    let (h, m) = (sim.router.hits, sim.router.misses);
    let day2 = hit_by_day.first().map(|(h1, m1)| {
        let (dh, dm) = (h - h1, m - m1);
        100.0 * dh as f64 / (dh + dm).max(1) as f64
    });
    println!(
        "metric={tag}.cache policy={:?} hit_pct={:.1} day2_hit_pct={} (target ≥ 80) entries={} peak_agents={peak}",
        policy,
        100.0 * h as f64 / (h + m).max(1) as f64,
        day2.map_or("n/a".into(), |v| format!("{v:.1}")),
        sim.router.cache.len()
    );
    let mut u: Vec<f32> = sim.stats.units.iter().map(|v| *v as f32).collect();
    println!(
        "metric={tag}.route_units p99={:.0} max={:.0} (budget 50 000)",
        pct(&mut u, 0.99),
        pct(&mut u, 1.0)
    );
    report_common(tag, &mut sim);
    sim.hash()
}

fn main() {
    let (case, days, threads) = args();
    let all = case.is_none();
    let want = |c: u32| all || case == Some(c);
    println!("spike=S-T threads={threads}");
    if want(1) {
        case1(threads);
    }
    if want(2) {
        case2(threads);
    }
    if want(3) {
        case3(threads);
    }
    if want(4) {
        case4(threads);
    }
    if want(5) {
        case5(threads);
    }
    if case == Some(9) {
        // Diagnostic: straight 1-lane pass-through (no control), saturated.
        for lanes_out in [1u8, 2] {
            let mut net = Net::default();
            let a = net.add_node(Vec2::new(-600.0, 0.0), Ctrl::Auto);
            let m = net.add_node(Vec2::ZERO, Ctrl::Auto);
            let b = net.add_node(Vec2::new(600.0, 0.0), Ctrl::Auto);
            let l1 = net.add_link(a, m, Class::Street, 1);
            let l2 = net.add_link(m, b, Class::Street, lanes_out);
            net.finalize();
            let mut sim = new_sim(net, threads, CachePolicy::Epoch);
            sim.record_passes = true;
            let mut rng = Pcg32::from_seed(9);
            run(
                &mut sim,
                6000,
                |s, r| {
                    for _ in 0..arrivals(r, 1.0) {
                        s.request(Key {
                            o: l1,
                            d: l2,
                            variant: 0,
                        });
                    }
                },
                &mut rng,
                |_| {},
            );
            let p = &sim.stats.passes;
            let n = p.iter().filter(|x| x.2 > 3000).count();
            println!(
                "metric=diag.passthrough out_lanes={lanes_out} veh_h={:.0} spawned={} grants={}",
                n as f32 / 300.0 * 3600.0,
                sim.stats.spawned,
                sim.stats.grants
            );
        }
    }
    if case == Some(10) {
        // Diagnostic: one saturated straight approach (1 lane) through a signal.
        let (net, inb, outb) = star(
            &[
                (0.0, Class::Avenue, 1),
                (90.0, Class::Avenue, 1),
                (180.0, Class::Avenue, 1),
            ],
            500.0,
            Ctrl::Signal,
        );
        let mut sim = new_sim(net, threads, CachePolicy::Epoch);
        sim.record_passes = true;
        let mut rng = Pcg32::from_seed(10);
        // Pre-fill: many requests at once.
        for _ in 0..80 {
            sim.request(Key {
                o: inb[2],
                d: outb[0],
                variant: 0,
            });
        }
        run(
            &mut sim,
            3000,
            |s, r| {
                for _ in 0..arrivals(r, 0.8) {
                    s.request(Key {
                        o: inb[2],
                        d: outb[0],
                        variant: 0,
                    });
                }
            },
            &mut rng,
            |s| {
                if s.tick % 50 == 0 {
                    let sg = s.signals[0];
                    let l = s.net.links[inb[2] as usize].lane0 as usize;
                    let q = &s.lane_q[l];
                    let front = q.first().map(|v| {
                        let x = &s.veh[*v as usize];
                        format!("s={:.1} v={:.1} want={} gr={}", x.s, x.v, x.want, x.granted)
                    });
                    println!(
                        "t={:.0} phase={} stage={} t={:.1} queue={} front={front:?} passes={}",
                        s.tick as f32 * 0.1,
                        sg.phase,
                        sg.stage,
                        sg.t,
                        q.len(),
                        s.stats.passes.len()
                    );
                }
            },
        );
        let p: Vec<u64> = sim.stats.passes.iter().map(|x| x.2).collect();
        let _ = &p;
        let hw: Vec<f32> = p.windows(2).map(|w| (w[1] - w[0]) as f32 * 0.1).collect();
        println!(
            "metric=diag.signal passes={} headways_first30={:?}",
            p.len(),
            &hw[..hw.len().min(30)]
        );
    }
    if case == Some(62) {
        // Router micro-benchmark: random (o, d) on the city, single thread and 4-way pool.
        let (net, homes, jobs) = city(1);
        let router = Router::new(&net, CachePolicy::Epoch);
        let mut sc = route::Scratch::default();
        let mut rng = Pcg32::new(7, 3);
        let keys: Vec<Key> = (0..400)
            .map(|i| Key {
                o: homes[rng.below(homes.len() as u32) as usize],
                d: jobs[rng.below(jobs.len() as u32) as usize],
                variant: (i % 4) as u8,
            })
            .collect();
        let t = std::time::Instant::now();
        let mut units = 0u64;
        for k in &keys {
            units += router.search(&mut sc, k.o, k.d, k.variant).2;
        }
        let el = t.elapsed().as_secs_f64();
        println!(
            "metric=router.single searches={} units={} ns_per_unit={:.0} us_per_search={:.0}",
            keys.len(),
            units,
            el * 1e9 / units as f64,
            el * 1e6 / keys.len() as f64
        );
        let pool = pool::Pool::new(threads);
        let t = std::time::Instant::now();
        let mut u2 = 0;
        for ch in keys.chunks(route::BATCH) {
            for (i, k) in ch.iter().enumerate() {
                router.start(i, &[(k.o, 0)], k.d, k.variant);
            }
            loop {
                let u = router.step_slots(&pool, 2000);
                u2 += u;
                if u == 0 {
                    break;
                }
            }
            for i in 0..ch.len() {
                let _ = router.take_done(i);
            }
        }
        let el = t.elapsed().as_secs_f64();
        println!(
            "metric=router.pool threads={threads} units={u2} wall_ns_per_unit={:.0}",
            el * 1e9 / u2 as f64
        );
    }
    if case == Some(61) {
        let cap: usize = std::env::args()
            .skip_while(|a| a != "--cap")
            .nth(1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(5000);
        case6(
            threads,
            cap,
            days,
            CachePolicy::Revalidate,
            false,
            &format!("case6_{cap}"),
        );
    }
    if want(6) {
        // Feasible demand for the despawn gate (V/C ≤ 1.0 at almost every node).
        case6(
            threads,
            1500,
            days,
            CachePolicy::Revalidate,
            false,
            "case6_1500",
        );
        case6(
            threads,
            5000,
            days,
            CachePolicy::Epoch,
            false,
            "case6_epoch_5000",
        );
        case6(
            threads,
            5000,
            days,
            CachePolicy::Revalidate,
            false,
            "case6_5000",
        );
        case6(
            threads,
            8000,
            days,
            CachePolicy::Revalidate,
            false,
            "case6_8000",
        );
        case6(
            threads,
            12000,
            days,
            CachePolicy::Revalidate,
            false,
            "case6_12000",
        );
    }
    if want(7) {
        case6(
            threads,
            5000,
            days,
            CachePolicy::Revalidate,
            true,
            "case7_5000",
        );
    }
    if want(8) {
        let hs: Vec<u64> = [1usize, 4, 8]
            .iter()
            .map(|t| {
                case6(
                    *t,
                    5000,
                    0.25,
                    CachePolicy::Revalidate,
                    false,
                    &format!("case8_t{t}"),
                )
            })
            .collect();
        println!(
            "metric=case8.hash_equal value={} hashes={hs:x?} (gate true)",
            hs.windows(2).all(|w| w[0] == w[1])
        );
    }
}
