//! Spike S-G — road–terrain interface incl. bridges (docs/PROGRESS.md Phase S).
//!
//! `cargo run -- [--roads 150] [--junctions 30] [--shots DIR]`
//! Prints `metric=` lines for every pass gate; `--shots` renders the lavapipe shot list.

mod road;
mod terrain;
mod view;
mod world;

use std::time::Instant;

use glam::Vec2;

use road::Road;
use terrain::{Mode, N, SP};
use world::{Features, World};

fn args() -> (usize, usize, Option<String>) {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| {
        a.iter()
            .position(|x| x == k)
            .and_then(|i| a.get(i + 1))
            .cloned()
    };
    (
        get("--roads").and_then(|v| v.parse().ok()).unwrap_or(150),
        get("--junctions")
            .and_then(|v| v.parse().ok())
            .unwrap_or(30),
        get("--shots"),
    )
}

/// Lateral sample offsets: centre, lanes, inside and outside the edge by 0.5 m.
fn offsets(r: &Road) -> [f32; 7] {
    let hw = r.hw();
    [
        0.0,
        0.5 * hw,
        -0.5 * hw,
        hw - 0.5,
        -(hw - 0.5),
        hw + 0.5,
        -(hw + 0.5),
    ]
}

struct Samples {
    n: usize,
    lod0_viol: usize,
    lod0_worst: f32,
    lodk_ok: [usize; 4],
    lodk_worst: [f32; 4],
    edge_gap: [usize; 4],
    edge_n: usize,
    /// Edge samples: depth of terrain below the road surface per LOD (for skirt sizing).
    edge_depth: [Vec<f32>; 4],
}

/// Road surface − rendered terrain at every LOD (ground parts + junction surfaces).
fn sample(w: &World, lods: &[(Vec<f32>, usize)]) -> Samples {
    let mut out = Samples {
        n: 0,
        lod0_viol: 0,
        lod0_worst: f32::NEG_INFINITY,
        lodk_ok: [0; 4],
        lodk_worst: [f32::NEG_INFINITY; 4],
        edge_gap: [0; 4],
        edge_n: 0,
        edge_depth: Default::default(),
    };
    let mut check = |p: Vec2, surf: f32, edge: bool| {
        out.n += 1;
        for (k, (g, gn)) in lods.iter().enumerate() {
            let cell = SP * (1 << k) as f32;
            let th = terrain::interp(g, *gn, cell, p);
            let ex = th - surf; // > 0: terrain above road
            if k == 0 {
                if ex > -0.03 {
                    out.lod0_viol += 1;
                    if std::env::var("SG_LOD0").is_ok() {
                        println!(
                            "lod0viol ({:.1},{:.1}) ex={ex:.3} surf={surf:.2} edge={edge}",
                            p.x, p.y
                        );
                    }
                }
                out.lod0_worst = out.lod0_worst.max(ex);
            }
            if ex <= 0.0 {
                out.lodk_ok[k] += 1;
            }
            out.lodk_worst[k] = out.lodk_worst[k].max(ex);
            // Gap under the 0.6 m road skirt at the outer edge.
            if edge && ex < -(0.6 + road::CARVE) - 0.5 {
                out.edge_gap[k] += 1;
            }
            if edge {
                out.edge_depth[k].push(-ex);
            }
        }
        if edge {
            out.edge_n += 1;
        }
    };
    for r in &w.roads {
        let len = r.len();
        let mut s = 0.0;
        while s <= len {
            let skip =
                r.on_span(s) || (r.cut0 > 0.0 && s < r.cut0) || (r.cut1 > 0.0 && s > len - r.cut1);
            if !skip {
                let p = r.point_at(s);
                let d = r.dir_at(s);
                let nrm = Vec2::new(-d.y, d.x);
                let h = r.h(s);
                for (q, off) in offsets(r).iter().enumerate() {
                    check(p + nrm * *off, h, q >= 5);
                }
            }
            s += 1.0;
        }
    }
    for jn in &w.junctions {
        let mut rr = 0.0;
        while rr <= jn.cut {
            let mut a = 0.0f32;
            while a < std::f32::consts::TAU {
                let p = jn.node + Vec2::new(a.cos(), a.sin()) * rr;
                check(p, road::junction_surface(jn, &w.roads, p), false);
                a += 5f32.to_radians();
            }
            rr += 1.0;
        }
    }
    out
}

/// Adjacent-sample slope jumps outside cores where stamps act (cliffs).
fn cliffs(w: &World) -> (usize, f32, usize, usize) {
    let mut infl = vec![false; N * N];
    // Distinct owners touching a sample (two-road interactions need a wall or spacing rule).
    let mut own = vec![(usize::MAX, false); N * N];
    for s in &w.stamps {
        for e in &s.entries {
            let gi = e.idx as usize;
            if !e.core {
                infl[gi] = true;
            }
            let o = &mut own[gi];
            if o.0 == usize::MAX {
                o.0 = s.owner;
            } else if o.0 != s.owner {
                o.1 = true;
            }
        }
    }
    let mut two = 0;
    let t = &w.t;
    let tan_max = road::CUT_SLOPE.max(road::FILL_SLOPE);
    let mut n = 0;
    let mut worst: f32 = 0.0;
    let mut checked = 0;
    for j in 0..N - 1 {
        for i in 0..N - 1 {
            let a = terrain::idx(i, j);
            for b in [terrain::idx(i + 1, j), terrain::idx(i, j + 1)] {
                // Outside corridors: not in a core, not under a bridge deck (abutment faces).
                if t.core[a]
                    || t.core[b]
                    || !(infl[a] || infl[b])
                    || !w.deck[a].is_nan()
                    || !w.deck[b].is_nan()
                {
                    continue;
                }
                checked += 1;
                let dh = (t.h[a] - t.h[b]).abs();
                let lim = (t.base[a] - t.base[b]).abs().max(tan_max * SP) + 0.05;
                if dh > lim && (own[a].1 || own[b].1) {
                    two += 1;
                    continue;
                }
                if dh > lim {
                    n += 1;
                    worst = worst.max(dh - lim);
                    if std::env::var("SG_CLIFF").is_ok() && dh - lim > 0.3 {
                        let p = terrain::pos(i, j);
                        let near: Vec<String> = w
                            .roads
                            .iter()
                            .filter_map(|r| {
                                let (sv, d) = r.project(p);
                                (d < r.hw() + 40.0).then(|| {
                                    format!(
                                        "r{}:s{:.0}/{:.0} d{:.0} span{}",
                                        r.id,
                                        sv,
                                        r.len(),
                                        d,
                                        r.on_span(sv)
                                    )
                                })
                            })
                            .collect();
                        let nj = w
                            .junctions
                            .iter()
                            .position(|jn| jn.node.distance(p) < jn.cut + 40.0);
                        println!(
                            "cliff ({:.0},{:.0}) ex={:.2} h={:.1}/{:.1} base={:.1}/{:.1} near={near:?} junction={nj:?}",
                            p.x,
                            p.y,
                            dh - lim,
                            t.h[a],
                            t.h[b],
                            t.base[a],
                            t.base[b]
                        );
                    }
                }
            }
        }
    }
    (n, worst, checked, two)
}

/// Crack between neighbouring chunks of LOD k and k+1 along shared edges (skirt depth needed).
fn skirt_need(
    lods: &[(Vec<f32>, usize)],
    w_core: &dyn Fn(usize) -> bool,
    base_at: &dyn Fn(Vec2) -> f32,
) -> Vec<(f32, f32)> {
    let chunks = (N - 1) / terrain::CHUNK;
    let mut out = Vec::new();
    for k in 0..lods.len() - 1 {
        let (fine, fgn) = &lods[k];
        let (coarse, cgn) = &lods[k + 1];
        let fcell = SP * (1 << k) as f32;
        let ccell = SP * (1 << (k + 1)) as f32;
        let mut worst: f32 = 0.0;
        let mut all: Vec<f32> = Vec::new();
        for c in 1..chunks {
            let x = (c * terrain::CHUNK) as f32 * SP;
            let mut z = 0.0;
            while z <= 4096.0 {
                for p in [Vec2::new(x, z), Vec2::new(z, x)] {
                    let a = terrain::interp(fine, *fgn, fcell, p);
                    let b = terrain::interp(coarse, *cgn, ccell, p);
                    let dd = (a - b).abs();
                    if dd > 3.0 && std::env::var("SG_SKIRT").is_ok() {
                        let gi =
                            terrain::idx((p.x / SP).round() as usize, (p.y / SP).round() as usize);
                        println!(
                            "crack lod{k} ({:.0},{:.0}) d={dd:.1} core={} fine={a:.1} coarse={b:.1} base={:.1}",
                            p.x,
                            p.y,
                            w_core(gi),
                            base_at(p)
                        );
                    }
                    all.push(dd);
                    worst = worst.max(dd);
                }
                z += fcell;
            }
        }
        all.sort_by(f32::total_cmp);
        out.push((all[(all.len() as f32 * 0.999) as usize], worst));
    }
    out
}

fn main() {
    let (n_roads, n_junctions, shots) = args();
    let t0 = Instant::now();
    let (mut w, f) = world::build(7, Mode::Continuous, n_roads, n_junctions);
    println!("spike=S-G build_s={:.1}", t0.elapsed().as_secs_f32());
    report(&mut w, &f);
    if let Some(dir) = shots {
        view::run(w, f, dir);
    }
}

fn report(w: &mut World, f: &Features) {
    let bridges: usize = w.roads.iter().filter(|r| !r.spans.is_empty()).count();
    println!(
        "metric=scenario roads={} junctions={} pairs={} river_bridges={} valley_bridges={} overpasses={} roads_with_spans={bridges} rejected_conflict={} rejected_steep={}",
        w.roads.len(),
        w.junctions.len(),
        f.pairs.len(),
        f.river_bridges
            .iter()
            .filter(|r| !w.roads[**r].spans.is_empty())
            .count(),
        f.valley_bridges
            .iter()
            .filter(|r| !w.roads[**r].spans.is_empty())
            .count(),
        f.overpass_upper
            .iter()
            .filter(|r| !w.roads[**r].spans.is_empty())
            .count(),
        w.rejected_conflict,
        w.rejected_steep
    );
    let lods_min: Vec<(Vec<f32>, usize)> =
        (0..4).map(|k| terrain::lod_grid(&w.t, k, true)).collect();
    {
        let s = sample(w, &lods_min);
        println!(
            "metric=coremin_lod under_road_pct=[{:.3},{:.3},{:.3}] edge_gap_pct=[{:.2},{:.2},{:.2}] (comparison: min over core neighbourhood)",
            100.0 * s.lodk_ok[1] as f64 / s.n as f64,
            100.0 * s.lodk_ok[2] as f64 / s.n as f64,
            100.0 * s.lodk_ok[3] as f64 / s.n as f64,
            100.0 * s.edge_gap[1] as f64 / s.edge_n as f64,
            100.0 * s.edge_gap[2] as f64 / s.edge_n as f64,
            100.0 * s.edge_gap[3] as f64 / s.edge_n as f64
        );
        let core_of = |gi: usize| w.t.core[gi];
        let base_of = |p: Vec2| terrain::interp(&w.t.base, N, SP, p);
        for (k, (p999, v)) in skirt_need(&lods_min, &core_of, &base_of).iter().enumerate() {
            println!(
                "metric=coremin_skirt_need_lod{k}_{} p99.9_m={p999:.3} max_m={v:.3}",
                k + 1
            );
        }
    }
    let tl = Instant::now();
    let lods: Vec<(Vec<f32>, usize)> = (0..4)
        .map(|k| terrain::lod_grid_constrained(&w.t, k))
        .collect();
    let chunks = ((N - 1) / terrain::CHUNK).pow(2);
    println!(
        "metric=lod1_3_constrained_full_map_ms value={:.0} per_chunk_ms={:.3}",
        tl.elapsed().as_secs_f64() * 1e3,
        tl.elapsed().as_secs_f64() * 1e3 / chunks as f64
    );
    let s = sample(w, &lods);
    println!(
        "metric=lod0_under_road viol={} of {} worst_excess_m={:.3} (gate 0 with terrain ≤ road − 0.03)",
        s.lod0_viol, s.n, s.lod0_worst
    );
    for k in 1..4 {
        println!(
            "metric=lod{k}_under_road pct={:.4} worst_excess_m={:.3} (gate LOD1 ≥ 99.9)",
            100.0 * s.lodk_ok[k] as f64 / s.n as f64,
            s.lodk_worst[k]
        );
    }
    let mut s = s;
    for k in 0..4 {
        let v = &mut s.edge_depth[k];
        v.sort_by(f32::total_cmp);
        println!(
            "metric=lod{k}_road_edge_depth_m p99={:.2} p99.9={:.2} max={:.2} (road skirt needed)",
            v[v.len() * 99 / 100],
            v[v.len() * 999 / 1000],
            v[v.len() - 1]
        );
    }
    for k in 0..4 {
        println!(
            "metric=lod{k}_edge_gap_below_skirt pct={:.3} (terrain > 1.25 m below the road edge)",
            100.0 * s.edge_gap[k] as f64 / s.edge_n.max(1) as f64
        );
    }
    let decim: Vec<(Vec<f32>, usize)> = (0..2).map(|k| terrain::lod_grid(&w.t, k, false)).collect();
    let sd = sample(
        w,
        &[
            decim[0].clone(),
            decim[1].clone(),
            decim[1].clone(),
            decim[1].clone(),
        ],
    );
    println!(
        "metric=lod1_plain_decimation_under_road pct={:.4} (comparison: no min-downsampling)",
        100.0 * sd.lodk_ok[1] as f64 / sd.n as f64
    );
    let (nc, worst, checked, two) = cliffs(w);
    println!(
        "metric=cliffs_continuous n={nc} of {checked} worst_excess_m={worst:.3} (gate 0) two_road_interactions={two}"
    );
    let core_of = |gi: usize| w.t.core[gi];
    let base_of = |p: Vec2| terrain::interp(&w.t.base, N, SP, p);
    for (k, (p999, v)) in skirt_need(&lods, &core_of, &base_of).iter().enumerate() {
        println!(
            "metric=skirt_need_lod{k}_{} p99.9_m={p999:.3} max_m={v:.3}",
            k + 1
        );
    }
    // Junction cut-line steps.
    let mut jstep: f32 = 0.0;
    for jn in &w.junctions {
        for &(ri, at_start) in &jn.approaches {
            let r = &w.roads[ri];
            let s = if at_start { jn.cut } else { r.len() - jn.cut };
            let p = r.point_at(s);
            let d = r.dir_at(s);
            let nrm = Vec2::new(-d.y, d.x);
            for off in offsets(r).iter().take(5) {
                let q = p + nrm * *off;
                jstep = jstep.max((road::junction_surface(jn, &w.roads, q) - r.h(s)).abs());
            }
        }
    }
    println!("metric=junction_cutline_step_max_m value={jstep:.4} (gate ≤ 0.02)");
    // Abutment: deck and road share the profile; check the evaluated end heights anyway.
    let mut astep: f32 = 0.0;
    for r in &w.roads {
        for &(a, b) in &r.spans {
            for se in [a, b] {
                astep = astep.max((r.h(se - 1e-3) - r.h(se + 1e-3)).abs());
            }
        }
    }
    println!("metric=abutment_step_max_m value={astep:.4} (gate ≤ 0.02)");
    // Piers inside other corridors.
    let mut pin = 0;
    let mut npiers = 0;
    for r in &w.roads {
        for (p, _, _) in &r.piers {
            npiers += 1;
            for o in &w.roads {
                if o.id == r.id {
                    continue;
                }
                let (_, d) = o.project(*p);
                if d <= o.hw() + road::SHOULDER {
                    pin += 1;
                }
            }
        }
    }
    if std::env::var("SG_DEBUG").is_ok() {
        let r = &w.roads[0];
        for (k, sv) in r.ps.iter().enumerate().step_by(4) {
            let p = r.point_at(*sv);
            println!(
                "prof0 s={sv:.0} h={:.2} base={:.2} wet={}",
                r.ph[k],
                terrain::interp(&w.t.base, N, SP, p),
                !w.t.water[terrain::idx((p.x / SP).round() as usize, (p.y / SP).round() as usize)]
                    .is_nan()
            );
        }
        for r in &w.roads {
            for (p, s, _) in &r.piers {
                for o in &w.roads {
                    if o.id != r.id && o.project(*p).1 <= o.hw() + road::SHOULDER {
                        let (i, j) = ((p.x / SP).round() as usize, (p.y / SP).round() as usize);
                        println!(
                            "pier road={} s={s:.0} in road={} spans={:?} owner_here={} dist={:.1} lim={:.1}",
                            r.id,
                            o.id,
                            r.spans,
                            w.owner[terrain::idx(i, j)],
                            o.project(*p).1,
                            o.hw() + road::SHOULDER
                        );
                    }
                }
            }
        }
    }
    println!(
        "metric=piers_in_corridors value={pin} of {npiers} unresolved_spans={} (gate 0)",
        w.pier_unresolved
    );

    // Overpass leaves the lower corridor unchanged: rebuild without the upper roads.
    if let Some(&lo) = f.overpass_lower.first() {
        let lower = &w.roads[lo];
        let keep = w.t.h.clone();
        let upper_ids: Vec<usize> = f.overpass_upper.clone();
        let mut t2 = terrain::Terrain {
            base: w.t.base.clone(),
            water: w.t.water.clone(),
            h: w.t.h.clone(),
            core: w.t.core.clone(),
        };
        let filtered: Vec<terrain::Stamp> = w
            .stamps
            .iter()
            .filter(|s| !upper_ids.contains(&s.owner))
            .map(|s| terrain::Stamp {
                id: s.id,
                owner: s.owner,
                prio: s.prio,
                aabb: s.aabb,
                entries: s.entries.clone(),
            })
            .collect();
        terrain::composite(&mut t2, &filtered, (0, 0, N - 1, N - 1), w.mode);
        let mut changed = 0;
        let mut total = 0;
        for (gi, own) in w.owner.iter().enumerate() {
            if *own as usize == lo + 1 {
                total += 1;
                if t2.h[gi].to_bits() != keep[gi].to_bits() {
                    changed += 1;
                }
            }
        }
        println!(
            "metric=overpass_lower_core_changed value={changed} of {total} samples (gate 0) lower_len={:.0}",
            lower.len()
        );
    }

    // Stamp removal restores the base bitwise.
    {
        let mut t2 = terrain::Terrain {
            base: w.t.base.clone(),
            water: w.t.water.clone(),
            h: w.t.h.clone(),
            core: w.t.core.clone(),
        };
        terrain::composite(&mut t2, &[], (0, 0, N - 1, N - 1), w.mode);
        let diff =
            t2.h.iter()
                .zip(&t2.base)
                .filter(|(a, b)| a.to_bits() != b.to_bits())
                .count();
        println!("metric=stamp_removal_restores_base diff_samples={diff} (gate 0)");
    }

    // Max-weight variant (old ADR-0012) for comparison.
    {
        let mut t2 = terrain::Terrain {
            base: w.t.base.clone(),
            water: w.t.water.clone(),
            h: w.t.h.clone(),
            core: w.t.core.clone(),
        };
        terrain::composite(&mut t2, &w.stamps, (0, 0, N - 1, N - 1), Mode::MaxWeight);
        let saved = std::mem::replace(&mut w.t, t2);
        let (nc, worst, checked, two) = cliffs(w);
        w.t = saved;
        println!(
            "metric=cliffs_max_weight n={nc} of {checked} worst_excess_m={worst:.3} two_road_interactions={two} (comparison)"
        );
    }

    // Parallel pairs.
    for (a, b, gap, dh) in &f.pairs {
        println!("metric=pair a={a} b={b} gap_m={gap:.1} dh_m={dh:.1}");
    }

    timing(w);
}

/// Re-stamp + LOD0 re-mesh of a 300 m road.
fn timing(w: &mut World) {
    use world::Opts;
    let mut times = Vec::new();
    let mut main_t = Vec::new();
    let mut done = 0;
    let mut rng = cb_core::rng::Pcg32::from_seed(99);
    while done < 40 {
        let p = Vec2::new(
            400.0 + rng.next_f32() * 3300.0,
            400.0 + rng.next_f32() * 3300.0,
        );
        let d = Vec2::new(rng.next_f32() - 0.5, rng.next_f32() - 0.5).normalize_or(Vec2::X);
        let ctrl = [
            p,
            p + d * 100.0,
            p + d * 200.0 + Vec2::new(-d.y, d.x) * 20.0,
            p + d * 300.0,
        ];
        let Some(ri) = w.add_road(&ctrl, road::Kind::Street, &Opts::default()) else {
            continue;
        };
        done += 1;
        // Measure: remove stamp → re-solve profile → re-stamp → composite → LOD0 re-mesh.
        let t0 = Instant::now();
        let rid = w.roads[ri].id;
        let (removed, kept): (Vec<terrain::Stamp>, Vec<terrain::Stamp>) =
            std::mem::take(&mut w.stamps)
                .into_iter()
                .partition(|s| s.owner == rid);
        w.stamps = kept;
        let rect = world::union_rect(removed.iter().map(|s| s.aabb));
        terrain::composite(&mut w.t, &w.stamps, rect, w.mode);
        // Re-solve the profile on the restored terrain.
        let r = &w.roads[rid];
        let pts: Vec<Vec2> = r.ps.iter().map(|s| r.point_at(*s)).collect();
        let g: Vec<f32> = pts.iter().map(|q| road::ground(&w.t, *q)).collect();
        let n = g.len();
        let inp = road::ProfileInput {
            g: g.clone(),
            w: vec![1.0; n],
            lo: vec![f32::NEG_INFINITY; n],
            h0: g[0],
            h1: g[n - 1],
            grade: r.kind.grade(),
            land0: 0.0,
            land1: 0.0,
        };
        if let Some(h) = road::solve_profile(&r.ps, &inp) {
            road::set_profile(&mut w.roads[rid], h);
        }
        let id = w.next_id;
        w.next_id += 1;
        let st = road::corridor_stamp(&w.roads[rid], &w.t, id);
        let rect = world::union_rect([rect, st.aabb].into_iter());
        w.stamps.push(st);
        terrain::composite(&mut w.t, &w.stamps, rect, w.mode);
        let meshes = view::remesh_chunks(&w.t, rect);
        let async_ms = t0.elapsed().as_secs_f64() * 1e3;
        let t1 = Instant::now();
        let uploaded: Vec<view::ChunkMesh> = meshes.iter().map(|m| m.clone()).collect();
        let main_ms = t1.elapsed().as_secs_f64() * 1e3;
        std::hint::black_box(&uploaded);
        times.push(async_ms);
        main_t.push(main_ms);
    }
    times.sort_by(f64::total_cmp);
    main_t.sort_by(f64::total_cmp);
    println!(
        "metric=restamp_300m_ms p50={:.2} max={:.2} (gate ≤ 25 async) main_thread_apply_proxy_ms p50={:.3} p95={:.3} max={:.3} (gate ≤ 1.5)",
        times[times.len() / 2],
        times[times.len() - 1],
        main_t[main_t.len() / 2],
        main_t[main_t.len() * 95 / 100],
        main_t[main_t.len() - 1]
    );
}
