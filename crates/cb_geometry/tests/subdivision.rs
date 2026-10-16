//! Plot subdivision and building fitting invariants (ADR-0064) over the six frontage families
//! of spike S-P: straight, convex, concave, S-curve, corner, two-segment.

use cb_core::rng::Pcg32;
use cb_geometry::boolean::f64ops;
use cb_geometry::d2::{self, Poly};
use cb_geometry::frontage::{Frontage, Prim, drag_end};
use cb_geometry::rect_fit::{self, FitParams, RectSpec};
use cb_geometry::subdivide::{self, Output, ZoneParams};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Straight,
    Convex,
    Concave,
    SCurve,
    Corner,
    TwoSeg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    R,
    C,
    I,
}

impl Zone {
    fn params(self) -> ZoneParams {
        match self {
            Self::R => ZoneParams::RESIDENTIAL,
            Self::C => ZoneParams::COMMERCIAL,
            Self::I => ZoneParams::INDUSTRIAL,
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code, reason = "kind is printed in failure messages")]
struct PlotDef {
    kind: Kind,
    zone: Zone,
    prims: Vec<Prim>,
    depth: f64,
    seed: u64,
    dn: i32,
}

fn range(r: &mut Pcg32, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * r.next_f64()
}

fn random_plot(seed: u64) -> PlotDef {
    let mut r = Pcg32::from_seed(seed);
    let kind = match r.below(6) {
        0 => Kind::Straight,
        1 => Kind::Convex,
        2 => Kind::Concave,
        3 => Kind::SCurve,
        4 => Kind::Corner,
        _ => Kind::TwoSeg,
    };
    let zone = match r.weighted_index(&[0.6, 0.25, 0.15]) {
        Some(0) => Zone::R,
        Some(1) => Zone::C,
        _ => Zone::I,
    };
    let zp = zone.params();
    let lmin = (zp.w_min * 1.2).max(20.0);
    let mut depth = range(&mut r, 15.0, 80.0);
    let prims = match kind {
        Kind::Straight => vec![Prim::Line(range(&mut r, lmin, 250.0))],
        Kind::Convex => {
            let rad = range(&mut r, 30.0, 300.0);
            let len = range(&mut r, lmin, 250.0).min(rad * 150f64.to_radians());
            vec![Prim::Arc(rad, -len / rad)]
        }
        Kind::Concave => {
            let rad = range(&mut r, 25.0, 300.0);
            depth = depth.min(0.8 * rad);
            let len = range(&mut r, lmin, 250.0).min(rad * 150f64.to_radians());
            vec![Prim::Arc(rad, len / rad)]
        }
        Kind::SCurve => {
            let r1 = range(&mut r, 40.0, 300.0);
            let r2 = range(&mut r, 40.0, 300.0);
            let l1 = range(&mut r, lmin * 0.5, 120.0).min(r1 * 90f64.to_radians());
            let l2 = range(&mut r, lmin * 0.5, 120.0).min(r2 * 90f64.to_radians());
            let first_convex = r.chance(0.5);
            let (s1, s2) = if first_convex {
                (-1.0, 1.0)
            } else {
                (1.0, -1.0)
            };
            depth = depth.min(0.8 * if first_convex { r2 } else { r1 });
            vec![Prim::Arc(r1, s1 * l1 / r1), Prim::Arc(r2, s2 * l2 / r2)]
        }
        Kind::Corner => {
            let alpha = range(&mut r, 60.0, 120.0);
            let rf = range(&mut r, 6.0, 12.0);
            let la = range(&mut r, lmin * 0.6, 150.0);
            let lb = range(&mut r, lmin * 0.6, 150.0);
            vec![
                Prim::Line(la),
                Prim::Arc(rf, (180.0 - alpha).to_radians()),
                Prim::Line(lb),
            ]
        }
        Kind::TwoSeg => {
            let kink = range(&mut r, 10.0, 40.0) * if r.chance(0.5) { 1.0 } else { -1.0 };
            let la = range(&mut r, lmin * 0.5, 130.0);
            let lb = range(&mut r, lmin * 0.5, 130.0);
            vec![
                Prim::Line(la),
                Prim::Arc(10.0, kink.to_radians()),
                Prim::Line(lb),
            ]
        }
    };
    let dn = r.below(7) as i32 - 3;
    PlotDef {
        kind,
        zone,
        prims,
        depth,
        seed,
        dn,
    }
}

fn case_seed(i: u64) -> u64 {
    cb_core::hash::stable_hash(&(0x5_9000u64, i))
}

struct Run {
    f: Frontage,
    plot: Poly,
    out: Output,
}

fn run(def: &PlotDef, prims: &[Prim], depth: f64, hint: Option<&subdivide::Hint>) -> Option<Run> {
    let f = Frontage::from_prims(prims, depth)?;
    let plot = f.plot_polygon(depth, 12.0)?;
    let out = subdivide::subdivide(&f, &plot, &def.zone.params(), depth, def.dn, def.seed, hint);
    Some(Run { f, plot, out })
}

fn fit_params(zone: Zone) -> FitParams {
    FitParams {
        setback: zone.params().setback,
        rect: (zone == Zone::R).then_some(RectSpec::HOUSE),
    }
}

/// Thickness of the thickest intersection component (area / half perimeter).
fn overlap_thickness(a: &Poly, b: &Poly) -> f64 {
    f64ops::intersect(a, b)
        .iter()
        .map(|p| d2::area(p) / (0.5 * d2::perimeter(p)).max(1e-9))
        .fold(0.0, f64::max)
}

#[test]
fn subdivide_random_10k_invariants() {
    let mut lots = 0usize;
    let mut lots_initial = 0usize;
    let mut rect_fail = 0usize;
    let mut merged = 0usize;
    let mut worst_area_err = 0.0f64;
    for i in 0..10_000u64 {
        let def = random_plot(case_seed(i));
        let Some(r) = run(&def, &def.prims, def.depth, None) else {
            continue;
        };
        let zp = def.zone.params();
        let pa = d2::area(&r.plot);
        let la: f64 = r.out.lots.iter().map(|l| l.area).sum();
        let err = (la - pa).abs() / pa;
        worst_area_err = worst_area_err.max(err);
        assert!(err <= 0.005, "case {i} {def:?}: area error {err}");
        let bbs: Vec<_> = r.out.lots.iter().map(|l| d2::bbox(&l.poly)).collect();
        for (a, l) in r.out.lots.iter().enumerate() {
            assert!(
                l.frontage_len >= zp.w_min - 0.01,
                "case {i} {def:?}: lot {a} frontage {}",
                l.frontage_len
            );
            for b in a + 1..r.out.lots.len() {
                if d2::bbox_overlap(bbs[a], bbs[b], 0.01) {
                    let t = overlap_thickness(&l.poly, &r.out.lots[b].poly);
                    assert!(t <= 0.005, "case {i} {def:?}: lots {a}/{b} overlap {t}");
                }
            }
        }
        lots += r.out.lots.len();
        lots_initial += r.out.stats.lots_initial;
        rect_fail += r.out.stats.rect_fail_initial;
        merged += r.out.stats.merged;
    }
    let rect_pass = 1.0 - rect_fail as f64 / lots_initial as f64;
    let merged_frac = merged as f64 / lots_initial as f64;
    println!(
        "lots={lots} rect_pass={rect_pass:.4} merged={merged_frac:.4} area_err={worst_area_err:.2e}"
    );
    assert!(rect_pass >= 0.95, "rectangularity pass {rect_pass}");
    assert!(merged_frac <= 0.03, "merged {merged_frac}");
}

#[test]
fn building_footprint_within_lot_10k() {
    let (mut r_lots, mut r_fit) = (0usize, 0usize);
    for i in 0..10_000u64 {
        let def = random_plot(case_seed(i));
        let Some(r) = run(&def, &def.prims, def.depth, None) else {
            continue;
        };
        let fits = rect_fit::fit_lots(&r.out.lots, &r.f, &fit_params(def.zone));
        for (l, fit) in r.out.lots.iter().zip(&fits) {
            if let Some(fp) = &fit.footprint {
                assert!(
                    rect_fit::rect_inside(fp, &l.poly),
                    "case {i} {def:?}: footprint outside lot"
                );
                assert!(d2::signed_area(fp) > 0.0);
            }
            if def.zone == Zone::R {
                r_lots += 1;
                r_fit += usize::from(fit.rect.is_some());
            }
        }
    }
    let frac = r_fit as f64 / r_lots as f64;
    println!("r_lots={r_lots} fit={frac:.4}");
    assert!(frac >= 0.90, "R lots fitting a house: {frac}");
}

#[test]
fn subdivide_fan_le_25_or_tight_curve() {
    let (mut checked, mut over) = (0usize, 0usize);
    for i in 0..3_000u64 {
        let def = random_plot(case_seed(i));
        let Some(r) = run(&def, &def.prims, def.depth, None) else {
            continue;
        };
        let w_min = def.zone.params().w_min;
        // Tightest frontage radius of the plot (arcs only).
        let r_min = def
            .prims
            .iter()
            .filter_map(|p| match *p {
                Prim::Arc(rad, _) => Some(rad),
                Prim::Line(_) => None,
            })
            .fold(f64::INFINITY, f64::min);
        // ADR-0064 (amended): fan ≤ max(25°, w_dense / r) with the densest whole-lot width
        // w_dense = max(w_min + 2, L / ⌊L / w_min⌋); repair merges are exempt. +3°: the solver
        // counts turn at polyline vertices (arcs sampled every ≤ 3°), the fan is measured
        // between ray directions, so they differ by up to one sampling step.
        let len = r.f.len();
        let w_dense = (w_min + 2.0).max(len / (len / w_min).floor().max(1.0));
        let cap = 25f64.max((w_dense / r_min).to_degrees()) + 3.0;
        for l in r.out.lots.iter().filter(|l| !l.corner) {
            checked += 1;
            over += usize::from(l.fan_deg() > 25.0);
            assert!(
                l.merged || l.fan_deg() <= cap,
                "case {i} {def:?}: fan {} cap {cap}",
                l.fan_deg()
            );
        }
    }
    let frac = over as f64 / checked as f64;
    println!("non-corner lots={checked} fan>25: {frac:.4}");
    assert!(frac <= 0.02, "{frac}");
}

#[test]
fn subdivide_corner_lot_fronts_both_roads() {
    let d = f64::to_radians;
    let cases = [
        (
            vec![Prim::Line(70.0), Prim::Arc(8.0, d(120.0)), Prim::Line(70.0)],
            Zone::R,
            30.0,
        ),
        (
            vec![Prim::Line(80.0), Prim::Arc(8.0, d(90.0)), Prim::Line(60.0)],
            Zone::R,
            30.0,
        ),
        (
            vec![Prim::Line(90.0), Prim::Arc(10.0, d(90.0)), Prim::Line(90.0)],
            Zone::C,
            35.0,
        ),
        (
            vec![Prim::Line(60.0), Prim::Arc(10.0, d(60.0)), Prim::Line(80.0)],
            Zone::R,
            30.0,
        ),
    ];
    for (k, (prims, zone, depth)) in cases.into_iter().enumerate() {
        let def = PlotDef {
            kind: Kind::Corner,
            zone,
            prims,
            depth,
            seed: 0x5EED_0010 + k as u64,
            dn: 0,
        };
        let r = run(&def, &def.prims, def.depth, None).unwrap();
        let c = *r.f.corner().unwrap();
        let corner_lots: Vec<_> = r.out.lots.iter().filter(|l| l.corner).collect();
        assert_eq!(corner_lots.len(), 1, "case {k}");
        let l = corner_lots[0];
        assert!(
            l.s0 <= c.s0 - 2.0 && l.s1 >= c.s1 + 2.0,
            "case {k}: corner lot [{}, {}] vs fillet [{}, {}]",
            l.s0,
            l.s1,
            c.s0,
            c.s1
        );
    }
}

/// Old lot intervals as world points (what the sim keeps per lot ID).
fn old_intervals(r: &Run) -> Vec<(glam::DVec2, glam::DVec2)> {
    r.out
        .lots
        .iter()
        .map(|l| (r.f.point_at(l.s0), r.f.point_at(l.s1)))
        .collect()
}

#[test]
fn lot_ids_stable_under_small_drags() {
    let (mut old, mut kept) = (0usize, 0usize);
    for i in 0..1_000u64 {
        let def = random_plot(case_seed(i));
        let Some(base) = run(&def, &def.prims, def.depth, None) else {
            continue;
        };
        let hint = base.out.hint(base.out.intervals());
        for delta in [0.25, -0.25, 1.0, -1.0] {
            let prims = drag_end(&def.prims, delta);
            let Some(r) = run(&def, &prims, def.depth, Some(&hint)) else {
                continue;
            };
            let m = subdivide::match_ids(&r.f, &old_intervals(&base), &r.out.lots);
            old += base.out.lots.len();
            kept += m.iter().filter(|x| x.is_some()).count();
        }
    }
    let frac = kept as f64 / old as f64;
    println!("drag <= 1 m kept {frac:.4}");
    assert!(frac >= 0.90, "{frac}");
}

#[test]
fn lot_ids_n_plus_minus_one_keep_n_minus_2() {
    let (mut edits, mut ok) = (0usize, 0usize);
    for i in 0..1_000u64 {
        let def = random_plot(case_seed(i));
        let Some(base) = run(&def, &def.prims, def.depth, None) else {
            continue;
        };
        let n_iv = base.out.intervals();
        let n_old = base.out.lots.len();
        for n in [n_iv + 1, n_iv.saturating_sub(1)] {
            if n < 1 || n == n_iv {
                continue;
            }
            let hint = base.out.hint(n);
            let Some(r) = run(&def, &def.prims, def.depth, Some(&hint)) else {
                continue;
            };
            if r.out.stats.rejected_edit {
                continue;
            }
            let kept = subdivide::match_ids(&r.f, &old_intervals(&base), &r.out.lots)
                .iter()
                .flatten()
                .count();
            edits += 1;
            ok += usize::from(kept as f64 >= (n_old as f64 - 2.0) - 1e-9);
        }
    }
    let frac = ok as f64 / edits as f64;
    println!("±1 edits={edits} meeting (N-2)/N: {frac:.4}");
    assert!(frac >= 0.99, "{frac}");
}

/// Golden hash of the six S-P frontage families (ADR-0064); a change means subdivision output
/// changed — intended changes update the value (and the cross-OS probe follows).
#[test]
fn subdivision_golden_hash_six_families() {
    let h = subdivide::reference_families_hash();
    eprintln!("reference_families_hash = {h:#018x}");
    assert_eq!(h, GOLDEN_SUBDIVISION);
}

const GOLDEN_SUBDIVISION: u64 = 0x4f1d_ee65_63b7_dec7;
