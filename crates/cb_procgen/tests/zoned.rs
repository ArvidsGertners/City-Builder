//! Zoned generation over subdivided plots (ALGORITHMS §11, GAME_DESIGN §4): every typology of
//! every zone and level stays inside its lot, keeps its triangle budget and LOD1 ratio, puts
//! windows only within wall edges, and is bit-identical for the same input (golden hash).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_geometry::d2;
use cb_geometry::frontage::{Frontage, Prim};
use cb_geometry::rect_fit::{self, FitParams, RectSpec};
use cb_geometry::subdivide::{ZoneParams, subdivide};
use cb_procgen::building::block::{self, WINDOW_END_MARGIN_M};
use cb_procgen::building::zoned::{self, LotShape, Massing, Zone};
use cb_procgen::surface::{self, Surface};
use glam::Vec2;

fn ground(p: Vec2) -> f32 {
    5.0 + 0.02 * p.x - 0.015 * p.y
}

fn plot_prims(r: &mut Pcg32, scale: f64) -> (Vec<Prim>, f64) {
    let d = f64::to_radians;
    let depth = scale * (25.0 + 20.0 * r.next_f64());
    let prims = match r.below(4) {
        0 => vec![Prim::Line(scale * (60.0 + 120.0 * r.next_f64()))],
        1 => vec![Prim::Arc(
            scale * (60.0 + 200.0 * r.next_f64()),
            -d(30.0 + 60.0 * r.next_f64()),
        )],
        2 => {
            let rad = scale * (80.0 + 200.0 * r.next_f64());
            vec![Prim::Arc(rad, d(20.0 + 40.0 * r.next_f64()))]
        }
        _ => vec![
            Prim::Line(scale * (40.0 + 60.0 * r.next_f64())),
            Prim::Arc(10.0, d(90.0)),
            Prim::Line(scale * (40.0 + 60.0 * r.next_f64())),
        ],
    };
    (prims, depth)
}

fn inside(poly: &[Vec2], q: Vec2, tol: f32) -> bool {
    cb_geometry::polygon::contains(poly, q)
        || cb_geometry::polygon::boundary_distance(poly, q) <= tol
}

fn params(zone: Zone) -> (ZoneParams, FitParams, f64) {
    match zone {
        Zone::Residential => {
            let zp = ZoneParams::RESIDENTIAL;
            (
                zp,
                FitParams {
                    setback: zp.setback,
                    rect: Some(RectSpec::HOUSE),
                },
                1.0,
            )
        }
        Zone::Commercial => {
            let zp = ZoneParams::COMMERCIAL;
            (
                zp,
                FitParams {
                    setback: zp.setback,
                    rect: None,
                },
                1.0,
            )
        }
        Zone::Industrial => {
            let zp = ZoneParams::INDUSTRIAL;
            (
                zp,
                FitParams {
                    setback: zp.setback,
                    rect: Some(RectSpec::HALL),
                },
                1.6,
            )
        }
    }
}

struct Stats {
    n: usize,
    tri_min: usize,
    tri_max: usize,
    tri_sum: usize,
    per_level: Vec<usize>,
}

/// Lays out and meshes `target` (lot, level) pairs of `zone` (levels `lo..=hi`) and checks every
/// invariant; returns triangle statistics.
fn run_zone(zone: Zone, lo: u8, hi: u8, target: usize, seed: u64) -> Stats {
    let mut rng = Pcg32::from_seed(seed);
    let (zp, fit_p, scale) = params(zone);
    let mut st = Stats {
        n: 0,
        tri_min: usize::MAX,
        tri_max: 0,
        tri_sum: 0,
        per_level: vec![0; 6],
    };
    let mut tried = 0usize;
    let mut plot_i = 0u64;
    while tried < target && plot_i < 20_000 {
        plot_i += 1;
        let (prims, depth) = plot_prims(&mut rng, scale);
        let depth = depth.min(zp.d_max);
        let Some(f) = Frontage::from_prims(&prims, depth) else {
            continue;
        };
        let Some(plot) = f.plot_polygon(depth, 12.0) else {
            continue;
        };
        let out = subdivide(&f, &plot, &zp, depth, 0, plot_i, None);
        let fits = rect_fit::fit_lots(&out.lots, &f, &fit_p);
        for (k, (lot, fit)) in out.lots.iter().zip(&fits).enumerate() {
            let Some(ins) = fit.inset.as_ref() else {
                continue;
            };
            let poly = d2::to_f32(&lot.poly);
            let inset = d2::to_f32(ins);
            let shape = LotShape {
                polygon: &poly,
                inset: &inset,
                front: (f.point_at(lot.s0).as_vec2(), f.point_at(lot.s1).as_vec2()),
                footprint: fit.footprint.map(|q| q.map(glam::DVec2::as_vec2)),
            };
            for level in lo..=hi {
                tried += 1;
                let s = plot_i * 1000 + k as u64 * 10 + u64::from(level);
                let Some(m) = zoned::layout(zone, level, &shape, s) else {
                    continue;
                };
                let Some(fd) = m.plan_foundation(&ground) else {
                    continue;
                };
                let (lod0, lod1) = m.mesh(&fd, &ground, 3);
                check(zone, level, &m, &lod0, &lod1, &poly, plot_i, k);
                let t = lod0.triangle_count();
                st.n += 1;
                st.per_level[level as usize] += 1;
                st.tri_min = st.tri_min.min(t);
                st.tri_max = st.tri_max.max(t);
                st.tri_sum += t;
            }
        }
    }
    println!(
        "{zone:?}: {} built of {tried} tried, tris {}..{} mean {}, per level {:?}",
        st.n,
        st.tri_min,
        st.tri_max,
        st.tri_sum / st.n.max(1),
        &st.per_level[1..]
    );
    st
}

#[allow(clippy::too_many_arguments)]
fn check(
    zone: Zone,
    level: u8,
    m: &Massing,
    lod0: &MeshData,
    lod1: &MeshData,
    poly: &[Vec2],
    plot_i: u64,
    k: usize,
) {
    let t = lod0.triangle_count();
    assert!(
        t <= m.tri_budget(),
        "{zone:?} L{level} plot {plot_i} lot {k}: {t} triangles > {}",
        m.tri_budget()
    );
    assert!(
        lod1.triangle_count() > 0
            && (lod1.triangle_count() * 100 <= t * 45 || lod1.triangle_count() <= 60),
        "{zone:?} L{level}: LOD1 {} vs LOD0 {t}",
        lod1.triangle_count()
    );
    for (v, pk) in lod0.positions.iter().zip(&lod0.packed) {
        let q = Vec2::new(v[0], v[2]);
        assert!(
            inside(poly, q, 0.05),
            "{zone:?} L{level} plot {plot_i} lot {k}: vertex {q} ({:?}) outside the lot by {}",
            surface::kind_of(*pk),
            cb_geometry::polygon::boundary_distance(poly, q)
        );
    }
    for o in m.outlines() {
        for c in o {
            assert!(
                inside(poly, c, 0.02),
                "{zone:?} L{level} plot {plot_i} lot {k}: outline corner {c} outside by {}",
                cb_geometry::polygon::boundary_distance(poly, c)
            );
        }
    }
}

#[test]
fn procgen_r_levels_within_lot_10k() {
    let st = run_zone(Zone::Residential, 1, 5, 10_000, 0x5E51);
    assert!(st.n > 4_000, "{}", st.n);
    // Every level is reachable on ordinary residential plots.
    assert!(
        st.per_level[1..=5].iter().all(|n| *n > 100),
        "{:?}",
        st.per_level
    );
}

#[test]
fn procgen_c_levels_within_lot_10k() {
    let st = run_zone(Zone::Commercial, 1, 3, 10_000, 0xC0FE);
    assert!(st.n > 5_000, "{}", st.n);
    assert!(
        st.per_level[1..=3].iter().all(|n| *n > 300),
        "{:?}",
        st.per_level
    );
}

#[test]
fn procgen_i_levels_within_lot_10k() {
    let st = run_zone(Zone::Industrial, 1, 3, 10_000, 0x1D57);
    assert!(st.n > 5_000, "{}", st.n);
    assert!(
        st.per_level[1..=3].iter().all(|n| *n > 300),
        "{:?}",
        st.per_level
    );
}

#[test]
fn procgen_triangle_budget_per_type() {
    // Per-typology means stay in the art-direction bands (ART_DIRECTION §2): houses ≥ 300,
    // apartment blocks and commercial denser, all under their budgets (checked per building in
    // the 10k tests).
    for (zone, lo, hi, min_mean) in [
        (Zone::Residential, 4, 5, 500),
        (Zone::Commercial, 1, 3, 200),
        (Zone::Industrial, 1, 3, 150),
    ] {
        let st = run_zone(zone, lo, hi, 1_500, 0xB0D6 + u64::from(lo));
        let mean = st.tri_sum / st.n.max(1);
        assert!(mean >= min_mean, "{zone:?} L{lo}–L{hi} mean {mean}");
    }
}

#[test]
fn facade_no_window_crosses_edge() {
    let mut rng = Pcg32::from_seed(0xFAC4);
    let (mut slots, mut blocks) = (0usize, 0usize);
    for zone in [Zone::Residential, Zone::Commercial] {
        let (zp, fit_p, scale) = params(zone);
        for plot_i in 0..300u64 {
            let (prims, depth) = plot_prims(&mut rng, scale);
            let Some(f) = Frontage::from_prims(&prims, depth.min(zp.d_max)) else {
                continue;
            };
            let Some(plot) = f.plot_polygon(depth.min(zp.d_max), 12.0) else {
                continue;
            };
            let out = subdivide(&f, &plot, &zp, depth.min(zp.d_max), 0, plot_i, None);
            for (k, (lot, fit)) in out
                .lots
                .iter()
                .zip(rect_fit::fit_lots(&out.lots, &f, &fit_p))
                .enumerate()
            {
                let Some(ins) = fit.inset.as_ref() else {
                    continue;
                };
                let (poly, inset) = (d2::to_f32(&lot.poly), d2::to_f32(ins));
                let shape = LotShape {
                    polygon: &poly,
                    inset: &inset,
                    front: (f.point_at(lot.s0).as_vec2(), f.point_at(lot.s1).as_vec2()),
                    footprint: None,
                };
                let levels: &[u8] = if zone == Zone::Residential {
                    &[4, 5]
                } else {
                    &[1, 2, 3]
                };
                for &level in levels {
                    let Some(l) = block::layout(zone, level, &shape, plot_i * 100 + k as u64)
                    else {
                        continue;
                    };
                    blocks += 1;
                    for w in block::facade_plan(&l) {
                        slots += 1;
                        let t = &l.tiers[w.tier as usize];
                        let n = t.poly.len();
                        let len = t.poly[w.edge].distance(t.poly[(w.edge + 1) % n]);
                        assert!(
                            w.s0 >= WINDOW_END_MARGIN_M - 1e-3
                                && w.s1 <= len - WINDOW_END_MARGIN_M + 1e-3
                                && w.s1 > w.s0,
                            "{zone:?} L{level}: window {}..{} on a {len} m wall",
                            w.s0,
                            w.s1
                        );
                        let top = l.tier_top(t);
                        assert!(
                            w.y0 >= t.base_y - 1e-3 && w.y1 <= top + 1e-3 && w.y1 > w.y0,
                            "{zone:?} L{level}: window {}..{} outside tier {}..{top}",
                            w.y0,
                            w.y1,
                            t.base_y
                        );
                    }
                    // The mesh draws one glass face per planned window (plus a shop door).
                    let fd = cb_procgen::building::foundation::plan(l.outline(), &[5.0; 4], false)
                        .unwrap();
                    let (m, _) = block::mesh(&l, &fd, 0);
                    let glass = m
                        .packed
                        .iter()
                        .filter(|p| surface::kind_of(**p) == Some(Surface::Window))
                        .count()
                        / 4;
                    assert!(glass >= block::facade_plan(&l).len(), "{glass} glass faces");
                }
            }
        }
    }
    println!("blocks={blocks} window slots={slots}");
    assert!(blocks > 300 && slots > 5_000);
}

fn mesh_hash(m: &MeshData) -> u64 {
    let mut bits: Vec<u64> = Vec::with_capacity(m.positions.len() * 5 + m.indices.len());
    for (p, (c, k)) in m.positions.iter().zip(m.colors.iter().zip(&m.packed)) {
        bits.extend(p.iter().map(|v| u64::from(v.to_bits())));
        bits.push(u64::from(u32::from_le_bytes(*c)));
        bits.push(u64::from(*k));
    }
    bits.extend(m.indices.iter().map(|i| u64::from(*i)));
    cb_core::hash::stable_hash(&bits)
}

#[test]
fn zoned_golden_hashes() {
    // A 30 × 40 m rectangular lot; R L4–L5 and C L1–L3 on its inset, I L1–L3 on a hall.
    let lot = vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(30.0, 0.0),
        Vec2::new(30.0, 40.0),
        Vec2::new(0.0, 40.0),
    ];
    let inset = vec![
        Vec2::new(2.0, 3.0),
        Vec2::new(28.0, 3.0),
        Vec2::new(28.0, 36.0),
        Vec2::new(2.0, 36.0),
    ];
    let hall = [
        Vec2::new(2.0, 3.0),
        Vec2::new(28.0, 3.0),
        Vec2::new(28.0, 30.0),
        Vec2::new(2.0, 30.0),
    ];
    let shape = LotShape {
        polygon: &lot,
        inset: &inset,
        front: (Vec2::ZERO, Vec2::new(30.0, 0.0)),
        footprint: Some(hall),
    };
    let mut hashes = Vec::new();
    for (zone, levels) in [
        (Zone::Residential, 4..=5u8),
        (Zone::Commercial, 1..=3),
        (Zone::Industrial, 1..=3),
    ] {
        for level in levels {
            let m = zoned::layout(zone, level, &shape, 99).expect("fits");
            let fd = m.plan_foundation(&ground).expect("flat enough");
            let (a, b) = m.mesh(&fd, &ground, 1);
            hashes.push(mesh_hash(&a));
            hashes.push(mesh_hash(&b));
            // Same input, same output.
            let again = zoned::layout(zone, level, &shape, 99).unwrap();
            assert_eq!(mesh_hash(&again.mesh(&fd, &ground, 1).0), mesh_hash(&a));
        }
    }
    let combined = cb_core::hash::stable_hash(&hashes);
    println!("combined={combined:016x}");
    assert_eq!(
        combined, GOLDEN,
        "zoned procgen output changed: update GOLDEN only for intended changes"
    );
}

const GOLDEN: u64 = 0x460e_179b_dd13_ec99;
