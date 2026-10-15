//! Spike S-P — plots → lots → buildings (docs/PROGRESS.md Phase S).
//!
//! `cargo run --release -- [--random N] [--edits M] [--beta B] [--svg DIR]`
//! Prints `metric=` lines for every pass gate.

mod fit;
mod frontage;
mod geom;
mod subdivide;
mod svg;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Instant;

use frontage::{Frontage, Kind, PlotDef, Zone};
use geom::Poly;
use subdivide::{Hint, Output, Params};

struct Run {
    f: Frontage,
    plot: Poly,
    out: Output,
    fits: Vec<fit::Fit>,
    ms: f64,
}

fn run(
    def: &PlotDef,
    prims: &[frontage::Prim],
    depth: f64,
    hint: Option<&Hint>,
    prm: &Params,
) -> Option<Run> {
    let f = Frontage::build(prims, depth);
    let plot = f.plot_polygon(depth)?;
    let zp = def.zone.params();
    let t0 = Instant::now();
    let out = subdivide::subdivide(&f, &plot, &zp, depth, def.dn, def.seed, hint, prm);
    let idx = frontage::FrontageIndex::new(&f);
    let fits: Vec<fit::Fit> = out
        .lots
        .iter()
        .map(|l| fit::fit_lot(l, &f, &idx, def.zone, &zp))
        .collect();
    let ms = t0.elapsed().as_secs_f64() * 1e3;
    Some(Run {
        f,
        plot,
        out,
        fits,
        ms,
    })
}

#[derive(Default)]
struct Inv {
    plots: usize,
    lots: usize,
    lots_initial: usize,
    touch_fail: usize,
    area_fail: usize,
    max_area_err: f64,
    overlap_fail: usize,
    rect_fail_initial: usize,
    merged: usize,
    removed_splits: usize,
    orphans: usize,
    multi: usize,
    r_lots: usize,
    r_fit: usize,
    fp_outside: usize,
    fan_lots: usize,
    fan_fail: usize,
    spikes: usize,
}

fn check(r: &Run, zone: Zone, inv: &mut Inv) -> Vec<&'static str> {
    let mut why = Vec::new();
    let zp = zone.params();
    inv.plots += 1;
    inv.lots += r.out.lots.len();
    inv.lots_initial += r.out.stats.lots_initial;
    inv.rect_fail_initial += r.out.stats.rect_fail_initial;
    inv.merged += r.out.stats.merged;
    inv.removed_splits += r.out.stats.removed_splits;
    inv.orphans += r.out.stats.orphans;
    inv.multi += r.out.stats.multi_contact;
    let pa = geom::area(&r.plot);
    let la: f64 = r.out.lots.iter().map(|l| l.area).sum();
    let err = (la - pa).abs() / pa;
    inv.max_area_err = inv.max_area_err.max(err);
    if err > 0.005 {
        inv.area_fail += 1;
        why.push("area");
    }
    let bbs: Vec<_> = r.out.lots.iter().map(|l| geom::bbox(&l.poly)).collect();
    for i in 0..r.out.lots.len() {
        let l = &r.out.lots[i];
        if l.frontage_len < zp.w_min - 0.01 {
            inv.touch_fail += 1;
        }
        if geom::min_interior_angle_deg(&l.poly) < 15.0 {
            inv.spikes += 1;
        }
        if !l.corner {
            inv.fan_lots += 1;
            if l.fan_deg() > 25.0 {
                inv.fan_fail += 1;
            }
        }
        for j in i + 1..r.out.lots.len() {
            if geom::bbox_overlap(bbs[i], bbs[j], 0.01) {
                // Overlap = an intersection thicker than 5 grid units (1 mm grid; union re-snapping leaves ≤ 2.5 mm slivers).
                let thick = geom::intersect(&l.poly, &r.out.lots[j].poly)
                    .iter()
                    .map(|p| geom::area(p) / (0.5 * geom::perimeter(p)).max(1e-9))
                    .fold(0.0, f64::max);
                if thick > 0.005 {
                    inv.overlap_fail += 1;
                    why.push("overlap");
                }
            }
        }
        if zone == Zone::R {
            inv.r_lots += 1;
            if r.fits[i].rect.is_some() {
                inv.r_fit += 1;
            }
        }
        if let Some(fp) = &r.fits[i].footprint
            && !fit::rect_inside(fp, &l.poly)
        {
            inv.fp_outside += 1;
            why.push("footprint");
        }
    }
    if r.out.stats.merged > 0 {
        why.push("merged");
    }
    why
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 {
        0.0
    } else {
        100.0 * a as f64 / b as f64
    }
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    v[((v.len() - 1) as f64 * q).round() as usize]
}

fn print_inv(tag: &str, inv: &Inv) {
    println!("metric={tag}.plots value={}", inv.plots);
    println!("metric={tag}.lots value={}", inv.lots);
    println!("metric={tag}.touch_fail value={} (gate 0)", inv.touch_fail);
    println!(
        "metric={tag}.area_fail value={} max_err_pct={:.4} (gate 0, ±0.5 %)",
        inv.area_fail,
        inv.max_area_err * 100.0
    );
    println!(
        "metric={tag}.overlap_fail value={} (gate 0)",
        inv.overlap_fail
    );
    println!(
        "metric={tag}.rect_pass_before_merge_pct value={:.2} (gate ≥ 95)",
        100.0 - pct(inv.rect_fail_initial, inv.lots_initial)
    );
    println!(
        "metric={tag}.merged_pct value={:.2} (gate ≤ 3) removed_splits={} orphans={} multi_contact={}",
        pct(inv.merged, inv.lots_initial),
        inv.removed_splits,
        inv.orphans,
        inv.multi
    );
    println!(
        "metric={tag}.r_fit_pct value={:.2} (gate ≥ 90) r_lots={}",
        pct(inv.r_fit, inv.r_lots),
        inv.r_lots
    );
    println!(
        "metric={tag}.footprint_outside value={} (gate 0)",
        inv.fp_outside
    );
    println!(
        "metric={tag}.fan_gt25_pct value={:.2} ({} of {} non-corner lots)",
        pct(inv.fan_fail, inv.fan_lots),
        inv.fan_fail,
        inv.fan_lots
    );
    println!("metric={tag}.spike_lots value={}", inv.spikes);
}

#[derive(Default, Clone)]
struct EditAcc {
    edits: usize,
    old: usize,
    kept: usize,
    per_edit_fail: usize,
    rejected: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let n_random: usize = get("--random")
        .and_then(|v| v.parse().ok())
        .unwrap_or(10_000);
    let n_edits: usize = get("--edits").and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let beta: f64 = get("--beta").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let svg_dir = get("--svg");
    let only: Option<u64> = get("--only").and_then(|v| v.parse().ok());
    let rect_min: f64 = get("--rect-min")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.55);
    let prm = Params { beta, rect_min };
    println!("spike=S-P beta={beta} random={n_random} edits={n_edits}");

    // Showcase.
    let show = frontage::showcase();
    let mut inv_show = Inv::default();
    let mut show_runs: Vec<(PlotDef, Run)> = Vec::new();
    for d in &show {
        if let Some(r) = run(d, &d.prims, d.depth, None, &prm) {
            let why = check(&r, d.zone, &mut inv_show);
            if std::env::var("SP_SHOW")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                == Some(show_runs.len())
            {
                println!(
                    "stats={:?} seams={:?} cuts={:?}",
                    r.out.stats,
                    r.out.seams,
                    r.out
                        .cuts
                        .iter()
                        .map(|c| format!(
                            "({:.1},{:.1})->({:.1},{:.1})",
                            c[0].x, c[0].y, c[1].x, c[1].y
                        ))
                        .collect::<Vec<_>>()
                );
                for l in &r.out.lots {
                    println!(
                        "lot s=[{:.2},{:.2}] fl={:.2} area={:.1} fan={:.1} dir0={:?} dir1={:?}",
                        l.s0,
                        l.s1,
                        l.frontage_len,
                        l.area,
                        l.fan_deg(),
                        l.dir0,
                        l.dir1
                    );
                }
            }
            if !why.is_empty() && std::env::var("SP_DEBUG").is_ok() {
                println!(
                    "showcase fail kind={} depth={} why={why:?}",
                    d.kind.name(),
                    d.depth
                );
            }
            show_runs.push((d.clone(), r));
        } else {
            println!("showcase plot invalid: {:?}", d.kind);
        }
    }
    print_inv("showcase", &inv_show);
    let mut fan_worst = 0.0f64;
    let mut corner_both = 0;
    let mut corner_lots = 0;
    for (d, r) in &show_runs {
        for l in &r.out.lots {
            if !l.corner {
                fan_worst = fan_worst.max(l.fan_deg());
                if l.fan_deg() > 25.0 {
                    println!(
                        "showcase_fan kind={} zone={} prims={:?} fan={:.1} w={:.1} turn={:.1}",
                        d.kind.name(),
                        d.zone.name(),
                        d.prims,
                        l.fan_deg(),
                        l.s1 - l.s0,
                        r.f.turn(l.s0, l.s1).to_degrees()
                    );
                }
            } else {
                corner_lots += 1;
                // Fronts both roads: frontage interval covers ≥ 2 m beyond the fillet on both sides.
                if let Some(c) = r.f.seams.iter().find(|c| c.is_corner)
                    && l.s0 <= c.s0 - 2.0
                    && l.s1 >= c.s1 + 2.0
                {
                    corner_both += 1;
                }
            }
        }
        let _ = d;
    }
    println!("metric=showcase.fan_max_deg value={fan_worst:.1} (review ≤ 25)");
    println!("metric=showcase.corner_lots_front_both value={corner_both}/{corner_lots}");

    // Random set.
    let mut inv = Inv::default();
    let mut panics = 0usize;
    let mut invalid = 0usize;
    let mut times: Vec<f64> = Vec::with_capacity(n_random);
    let mut by_kind: Vec<(Kind, Inv)> = [
        Kind::Straight,
        Kind::Convex,
        Kind::Concave,
        Kind::SCurve,
        Kind::Corner,
        Kind::TwoSeg,
    ]
    .into_iter()
    .map(|k| (k, Inv::default()))
    .collect();
    let mut edit_acc: Vec<(&str, EditAcc)> = [
        "drag+0.25",
        "drag-0.25",
        "drag+1",
        "drag-1",
        "drag+3",
        "drag-3",
        "n+1",
        "n-1",
        "depth+5",
        "depth-5",
    ]
    .into_iter()
    .map(|n| (n, EditAcc::default()))
    .collect();
    let mut inv_edit = Inv::default();
    let mut worst: Vec<(f64, PlotDef)> = Vec::new();
    for i in 0..n_random {
        let seed = cb_core::hash::stable_hash(&(0x5_9000u64, i as u64));
        if only.is_some_and(|o| o != i as u64) {
            continue;
        }
        let def = frontage::random_plot(seed);
        let res = catch_unwind(AssertUnwindSafe(|| {
            run(&def, &def.prims, def.depth, None, &prm)
        }));
        let r = match res {
            Err(_) => {
                panics += 1;
                continue;
            }
            Ok(None) => {
                invalid += 1;
                continue;
            }
            Ok(Some(r)) => r,
        };
        times.push(r.ms);
        let why = check(&r, def.zone, &mut inv);
        if only.is_some() {
            println!("stats={:?}", r.out.stats);
            for (k, l) in r.out.lots.iter().enumerate() {
                println!(
                    "lot {k} s=[{:.2},{:.2}] fl={:.2} area={:.1} rect={:.3} corner={} n={} poly={:?}",
                    l.s0,
                    l.s1,
                    l.frontage_len,
                    l.area,
                    l.rect,
                    l.corner,
                    l.poly.len(),
                    l.poly
                        .iter()
                        .map(|v| (v.x * 100.0).round() / 100.0)
                        .zip(l.poly.iter().map(|v| (v.y * 100.0).round() / 100.0))
                        .collect::<Vec<_>>()
                );
            }
            for i in 0..r.out.lots.len() {
                for j in i + 1..r.out.lots.len() {
                    let a: f64 = geom::intersect(&r.out.lots[i].poly, &r.out.lots[j].poly)
                        .iter()
                        .map(|p| geom::area(p))
                        .sum();
                    if a > 0.01 {
                        println!(
                            "overlap {i} {j} area={a:.2} at={:?}",
                            geom::intersect(&r.out.lots[i].poly, &r.out.lots[j].poly)
                        );
                    }
                }
            }
        }
        if !why.is_empty() && std::env::var("SP_DEBUG").is_ok() {
            println!(
                "fail i={i} kind={} zone={} depth={:.1} prims={:?} why={why:?}",
                def.kind.name(),
                def.zone.name(),
                def.depth,
                def.prims
            );
        }
        if let Some((_, k)) = by_kind.iter_mut().find(|(k, _)| *k == def.kind) {
            check(&r, def.zone, k);
        }
        let min_rect = r.out.lots.iter().map(|l| l.rect).fold(1.0, f64::min);
        let key = if r.out.stats.orphans > 0 {
            -1.0
        } else if r.out.stats.merged > 0 {
            0.0
        } else {
            min_rect
        };
        worst.push((key, def.clone()));

        if i < n_edits {
            let hint_pts: Vec<_> = r.out.splits.iter().map(|s| s.1).collect();
            let old_iv: Vec<_> = r
                .out
                .lots
                .iter()
                .map(|l| (r.f.point_at(l.s0), r.f.point_at(l.s1)))
                .collect();
            let n_old = r.out.lots.len();
            let n_iv = r.out.splits.len() + 1;
            for (name, acc) in edit_acc.iter_mut() {
                let (prims, depth, n) = match *name {
                    "drag+0.25" => (frontage::drag_end(&def.prims, 0.25), def.depth, n_iv),
                    "drag-0.25" => (frontage::drag_end(&def.prims, -0.25), def.depth, n_iv),
                    "drag+1" => (frontage::drag_end(&def.prims, 1.0), def.depth, n_iv),
                    "drag-1" => (frontage::drag_end(&def.prims, -1.0), def.depth, n_iv),
                    "drag+3" => (frontage::drag_end(&def.prims, 3.0), def.depth, n_iv),
                    "drag-3" => (frontage::drag_end(&def.prims, -3.0), def.depth, n_iv),
                    "n+1" => (def.prims.clone(), def.depth, n_iv + 1),
                    "n-1" => (def.prims.clone(), def.depth, n_iv.saturating_sub(1).max(1)),
                    "depth+5" => (def.prims.clone(), def.depth + 5.0, n_iv),
                    _ => (def.prims.clone(), (def.depth - 5.0).max(15.0), n_iv),
                };
                // Concave depth rule still applies to edits.
                let depth = match (def.kind, prims.first()) {
                    (Kind::Concave, Some(frontage::Prim::Arc(rad, _))) => depth.min(0.8 * rad),
                    _ => depth,
                };
                if name.starts_with('n') && (n == n_iv || n < 1) {
                    continue;
                }
                let hint = Hint {
                    split_pts: hint_pts.clone(),
                    n,
                    merged_pts: r.out.merged_pts.clone(),
                };
                let res = catch_unwind(AssertUnwindSafe(|| {
                    run(&def, &prims, depth, Some(&hint), &prm)
                }));
                let Ok(Some(r2)) = res else {
                    if res.is_err() {
                        panics += 1;
                    }
                    continue;
                };
                if r2.out.stats.rejected_edit {
                    acc.rejected += 1;
                    continue;
                }
                let _ = check(&r2, def.zone, &mut inv_edit);
                let m = subdivide::match_ids(&r2.f, &old_iv, &r2.out.lots, 0.6);
                let kept = m.iter().filter(|x| x.is_some()).count();
                acc.edits += 1;
                acc.old += n_old;
                acc.kept += kept;
                let need = if name.starts_with('n') {
                    (n_old as f64 - 2.0) / n_old as f64
                } else {
                    0.9
                };
                if (kept as f64) < need * n_old as f64 - 1e-9 {
                    acc.per_edit_fail += 1;
                    if std::env::var("SP_EDIT").is_ok() {
                        println!(
                            "  base_splits={:?} base_merged={:?} new_splits={:?} new_merged={:?}",
                            r.out
                                .splits
                                .iter()
                                .map(|v| (v.0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>(),
                            r.out
                                .merged_pts
                                .iter()
                                .map(|p| (r.f.project(*p).0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>(),
                            r2.out
                                .splits
                                .iter()
                                .map(|v| (v.0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>(),
                            r2.out
                                .merged_pts
                                .iter()
                                .map(|p| (r2.f.project(*p).0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>()
                        );
                        println!(
                            "editfail i={i} {name} kind={} n_old={n_old} n_new={} kept={kept} old_iv={:?} new_iv={:?} st={:?}",
                            def.kind.name(),
                            r2.out.lots.len(),
                            r.out
                                .lots
                                .iter()
                                .map(|l| (l.s0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>(),
                            r2.out
                                .lots
                                .iter()
                                .map(|l| (l.s0 * 10.0).round() / 10.0)
                                .collect::<Vec<_>>(),
                            r2.out.stats
                        );
                    }
                }
            }
        }
    }
    println!("metric=random.panics value={panics} (gate 0)");
    println!("metric=random.invalid_plots value={invalid}");
    print_inv("random", &inv);
    for (k, ki) in &by_kind {
        println!(
            "metric=kind.{} plots={} lots={} rect_pass={:.2} merged={:.2} r_fit={:.2} fan_gt25={:.2} touch_fail={} overlap={} area_fail={}",
            k.name(),
            ki.plots,
            ki.lots,
            100.0 - pct(ki.rect_fail_initial, ki.lots_initial),
            pct(ki.merged, ki.lots_initial),
            pct(ki.r_fit, ki.r_lots),
            pct(ki.fan_fail, ki.fan_lots),
            ki.touch_fail,
            ki.overlap_fail,
            ki.area_fail
        );
    }
    let p50 = quantile(&mut times, 0.5);
    let p99 = quantile(&mut times, 0.99);
    let max = quantile(&mut times, 1.0);
    geom::PROF.with(|p| {
        let p = p.borrow();
        println!("prof_ms_total rays={:.0} slice={:.0} assign={:.0} repair={:.0} classify={:.0} inset={:.0} rect={:.0}", p[0], p[1], p[2], p[3], p[4], p[5], p[6]);
    });
    println!("metric=random.time_ms p50={p50:.3} p99={p99:.3} max={max:.3} (gate p99 ≤ 2)");
    print_inv("edits", &inv_edit);
    let mut small_old = 0;
    let mut small_kept = 0;
    for (name, a) in &edit_acc {
        println!(
            "metric=edit.{name} edits={} kept_pct={:.2} per_edit_fail={} rejected={}",
            a.edits,
            pct(a.kept, a.old),
            a.per_edit_fail,
            a.rejected
        );
        if matches!(*name, "drag+0.25" | "drag-0.25" | "drag+1" | "drag-1") {
            small_old += a.old;
            small_kept += a.kept;
        }
    }
    println!(
        "metric=edit.drag_le_1m_kept_pct value={:.2} (gate ≥ 90)",
        pct(small_kept, small_old)
    );

    if let Some(dir) = svg_dir {
        let _ = std::fs::create_dir_all(&dir);
        let cells: Vec<svg::Cell> = show_runs
            .iter()
            .map(|(d, r)| svg::Cell {
                title: format!(
                    "{} {} d={:.0} n={}",
                    d.kind.name(),
                    d.zone.name(),
                    d.depth,
                    r.out.lots.len()
                ),
                f: &r.f,
                plot: &r.plot,
                out: &r.out,
                fits: &r.fits,
                ids: (0..r.out.lots.len()).collect(),
            })
            .collect();
        let _ = svg::sheet(&cells, 6, &format!("{dir}/showcase.svg"));
        // Worst 24 random plots by min rectangularity / merges.
        worst.sort_by(|a, b| a.0.total_cmp(&b.0));
        let wr: Vec<(PlotDef, Run)> = worst
            .iter()
            .take(24)
            .filter_map(|(_, d)| run(d, &d.prims, d.depth, None, &prm).map(|r| (d.clone(), r)))
            .collect();
        let cells: Vec<svg::Cell> = wr
            .iter()
            .map(|(d, r)| svg::Cell {
                title: format!(
                    "{} {} d={:.0} n={} m={}",
                    d.kind.name(),
                    d.zone.name(),
                    d.depth,
                    r.out.lots.len(),
                    r.out.stats.merged
                ),
                f: &r.f,
                plot: &r.plot,
                out: &r.out,
                fits: &r.fits,
                ids: (0..r.out.lots.len()).collect(),
            })
            .collect();
        let _ = svg::sheet(&cells, 6, &format!("{dir}/worst.svg"));
        // Random sample (first 24).
        let rs: Vec<(PlotDef, Run)> = (0..24u64)
            .filter_map(|i| {
                let d = frontage::random_plot(cb_core::hash::stable_hash(&(0x5_9000u64, i)));
                run(&d, &d.prims, d.depth, None, &prm).map(|r| (d, r))
            })
            .collect();
        let cells: Vec<svg::Cell> = rs
            .iter()
            .map(|(d, r)| svg::Cell {
                title: format!(
                    "{} {} d={:.0} n={}",
                    d.kind.name(),
                    d.zone.name(),
                    d.depth,
                    r.out.lots.len()
                ),
                f: &r.f,
                plot: &r.plot,
                out: &r.out,
                fits: &r.fits,
                ids: (0..r.out.lots.len()).collect(),
            })
            .collect();
        let _ = svg::sheet(&cells, 6, &format!("{dir}/random.svg"));
        println!("svg written to {dir}");
    }
}
