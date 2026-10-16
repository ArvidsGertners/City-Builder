//! Residential generation over subdivided plots: houses stay inside their lots, triangle budgets
//! (ART_DIRECTION §2, ALGORITHMS §11), LOD1 ratio, and golden hashes for cross-OS determinism.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use cb_geometry::d2;
use cb_geometry::frontage::{Frontage, Prim};
use cb_geometry::rect_fit::{self, FitParams, RectSpec};
use cb_geometry::subdivide::{ZoneParams, subdivide};
use cb_procgen::building::house;
use cb_procgen::surface::{self, Surface};
use cb_procgen::yard::{self, YARD_TRI_BUDGET, YardLot};
use glam::Vec2;

fn ground(p: Vec2) -> f32 {
    5.0 + 0.05 * p.x - 0.03 * p.y
}

fn plot_prims(r: &mut Pcg32) -> (Vec<Prim>, f64) {
    let d = f64::to_radians;
    let depth = 25.0 + 20.0 * r.next_f64();
    let prims = match r.below(4) {
        0 => vec![Prim::Line(60.0 + 120.0 * r.next_f64())],
        1 => vec![Prim::Arc(
            40.0 + 200.0 * r.next_f64(),
            -d(30.0 + 80.0 * r.next_f64()),
        )],
        2 => {
            let rad = 60.0 + 200.0 * r.next_f64();
            vec![Prim::Arc(rad, d(20.0 + 40.0 * r.next_f64()))]
        }
        _ => vec![
            Prim::Line(40.0 + 60.0 * r.next_f64()),
            Prim::Arc(8.0, d(90.0)),
            Prim::Line(40.0 + 60.0 * r.next_f64()),
        ],
    };
    (prims, depth)
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

/// Inside or within `tol` of the polygon.
fn inside(poly: &[Vec2], q: Vec2, tol: f32) -> bool {
    cb_geometry::polygon::contains(poly, q)
        || cb_geometry::polygon::boundary_distance(poly, q) <= tol
}

#[test]
fn houses_within_lots_and_budgets() {
    let mut rng = Pcg32::from_seed(0xB01D);
    let zp = ZoneParams::RESIDENTIAL;
    let fit_p = FitParams {
        setback: zp.setback,
        rect: Some(RectSpec::HOUSE),
    };
    let (mut n, mut tri_min, mut tri_max, mut yard_max, mut tri_sum) =
        (0usize, usize::MAX, 0usize, 0usize, 0usize);
    for plot_i in 0..150u64 {
        let (prims, depth) = plot_prims(&mut rng);
        let Some(f) = Frontage::from_prims(&prims, depth) else {
            continue;
        };
        let Some(plot) = f.plot_polygon(depth, 12.0) else {
            continue;
        };
        let out = subdivide(&f, &plot, &zp, depth, 0, plot_i, None);
        let fits = rect_fit::fit_lots(&out.lots, &f, &fit_p);
        for (k, (lot, fit)) in out.lots.iter().zip(&fits).enumerate() {
            let Some(fp) = fit.footprint else { continue };
            let poly = d2::to_f32(&lot.poly);
            for level in 1..=3u8 {
                let seed = plot_i * 1000 + k as u64 * 10 + u64::from(level);
                let l = house::layout(fp.map(glam::DVec2::as_vec2), level, seed);
                // Every volume inside the lot (no house crossing a lot line).
                for q in l.outlines() {
                    for c in q {
                        assert!(
                            inside(&poly, c, 0.01),
                            "plot {plot_i} lot {k} L{level}: corner {c} outside"
                        );
                    }
                }
                let Some(fd) = l.plan_foundation(&ground) else {
                    continue;
                };
                let (lod0, lod1) = house::mesh(&l, &fd, &ground, 7);
                let t = lod0.triangle_count();
                n += 1;
                tri_min = tri_min.min(t);
                tri_max = tri_max.max(t);
                tri_sum += t;
                assert!(
                    lod1.triangle_count() * 100 <= t * 20,
                    "LOD1 {} vs LOD0 {t}",
                    lod1.triangle_count()
                );
                // Walls and roofs stay over the lot (eaves may overhang the setback, not the line).
                for v in &lod0.positions {
                    assert!(
                        inside(&poly, Vec2::new(v[0], v[2]), 0.05),
                        "plot {plot_i} lot {k} L{level}: vertex outside lot"
                    );
                }
                let edges = rect_fit::lot_edge_kinds(lot, &f);
                let yl = YardLot {
                    polygon: &poly,
                    edges: &edges,
                    front_start: f.point_at(lot.s0).as_vec2(),
                    front_end: f.point_at(lot.s1).as_vec2(),
                };
                let ym = yard::yard(&l, &yl, fd.floor_y, &ground, 7, seed);
                yard_max = yard_max.max(ym.triangle_count());
                assert!(
                    ym.triangle_count() <= YARD_TRI_BUDGET,
                    "yard {} tris",
                    ym.triangle_count()
                );
                for (v, pk) in ym.positions.iter().zip(&ym.packed) {
                    // Tree crowns may overhang a neighbour; everything else stays on the lot.
                    if surface::kind_of(*pk) != Some(Surface::Foliage) {
                        let q = Vec2::new(v[0], v[2]);
                        assert!(
                            inside(&poly, q, 0.3),
                            "plot {plot_i} lot {k}: yard vertex outside by {} kind {:?}",
                            cb_geometry::polygon::boundary_distance(&poly, q),
                            surface::kind_of(*pk)
                        );
                    }
                }
            }
        }
    }
    let mean = tri_sum / n.max(1);
    println!("houses={n} tris {tri_min}..{tri_max} mean {mean} yard max {yard_max}");
    assert!(n > 500);
    // ART_DIRECTION §2: houses 300–1 200 triangles; small cottages may fall below the target.
    assert!(
        tri_min >= 120 && tri_max <= 1200 && mean >= 300,
        "{tri_min}..{tri_max} mean {mean}"
    );
}

#[test]
fn procgen_golden_hashes() {
    let fp = [
        Vec2::new(0.0, 0.0),
        Vec2::new(12.0, 0.0),
        Vec2::new(12.0, 14.0),
        Vec2::new(0.0, 14.0),
    ];
    let mut hashes = Vec::new();
    for level in 1..=3u8 {
        let l = house::layout(fp, level, 42);
        let fd = l.plan_foundation(&ground).unwrap();
        let (lod0, lod1) = house::mesh(&l, &fd, &ground, 1);
        hashes.push(mesh_hash(&lod0));
        hashes.push(mesh_hash(&lod1));
    }
    // Same input, same output (bit-identical); the constant pins it across operating systems.
    let again = {
        let l = house::layout(fp, 2, 42);
        let fd = l.plan_foundation(&ground).unwrap();
        mesh_hash(&house::mesh(&l, &fd, &ground, 1).0)
    };
    assert_eq!(again, hashes[2]);
    let combined = cb_core::hash::stable_hash(&hashes);
    println!("combined={combined:016x}");
    assert_eq!(
        combined, GOLDEN,
        "procgen output changed: update GOLDEN only for intended changes"
    );
}

const GOLDEN: u64 = 0x2abf_5207_7efa_8a8d;
