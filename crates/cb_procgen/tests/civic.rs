//! Civic generator set A (ALGORITHMS §11, ART_DIRECTION §5): volumes stay on the parcel,
//! triangle budgets hold, silhouettes are distinct between kinds and from zoned buildings on the
//! same parcel (LOD1 masks, 4 yaws, 128², IoU ≤ 0.6), and output is bit-identical (golden hash).

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_geometry::MeshData;
use cb_procgen::building::civic::{self, CivicKind, CivicLayout};
use cb_procgen::building::zoned::{self, LotShape, Zone};
use cb_procgen::paint::Frame;
use glam::{Vec2, Vec3};

const FLAT_GROUND_M: f32 = 10.0;

fn flat(_: Vec2) -> f32 {
    FLAT_GROUND_M
}

/// Parcel frame centred on the origin, front edge along +x at `z = -d/2`.
fn centred(kind: CivicKind) -> Frame {
    let (w, d) = kind.parcel_m();
    Frame {
        o: Vec2::new(-0.5 * w, -0.5 * d),
        ex: Vec2::X,
        ey: Vec2::Y,
    }
}

fn build(kind: CivicKind, frame: Frame, seed: u64) -> (CivicLayout, MeshData, MeshData) {
    let l = civic::layout(kind, frame, seed);
    let f = l.plan_foundation(&flat).expect("flat ground is buildable");
    let (lod0, lod1) = civic::mesh(&l, &f, 3);
    (l, lod0, lod1)
}

#[test]
fn civic_volumes_stay_on_parcel() {
    let ground = |p: Vec2| 3.0 + 0.01 * p.x - 0.008 * p.y;
    for kind in CivicKind::ALL {
        for seed in 0..40u64 {
            let a = seed as f32 * 0.37;
            let (s, c) = cb_core::math::sin_cos(a);
            let along = Vec2::new(c, s);
            let fr = civic::frame_at(kind, Vec2::new(100.0 * s, -70.0 * c), along);
            let l = civic::layout(kind, fr, seed);
            let parcel = l.parcel();
            for o in l.outlines() {
                for q in &o {
                    let inside = cb_geometry::polygon::contains(&parcel, *q)
                        || cb_geometry::polygon::boundary_distance(&parcel, *q) < 0.01;
                    assert!(
                        inside,
                        "{kind:?} seed {seed}: outline point {q} off the parcel"
                    );
                }
            }
            assert!(l.height_m() > 2.5, "{kind:?}: {}", l.height_m());
            let f = l
                .plan_foundation(&ground)
                .expect("gentle slope is buildable");
            let (lod0, _) = civic::mesh(&l, &f, 0);
            // Everything but the shoreline pipes stays within the parcel (plus fence posts).
            let (w, d) = kind.parcel_m();
            let reach = kind.pipe_reach_m();
            for p in &lod0.positions {
                let q = l.frame.local(Vec2::new(p[0], p[2]));
                let (u, v) = if l.mirrored {
                    (w - q.x, q.y)
                } else {
                    (q.x, q.y)
                };
                assert!(
                    (-0.3..=w + 0.3).contains(&u) && (-0.3..=d + reach + 0.3).contains(&v),
                    "{kind:?} seed {seed}: vertex at ({u:.2}, {v:.2}) off the parcel"
                );
            }
        }
    }
}

#[test]
fn civic_triangle_budget_and_lod1() {
    for kind in CivicKind::ALL {
        let mut worst = 0;
        for seed in 0..20u64 {
            let (_, lod0, lod1) = build(kind, centred(kind), seed);
            let (n0, n1) = (lod0.triangle_count(), lod1.triangle_count());
            worst = worst.max(n0);
            assert!(
                n0 <= kind.tri_budget(),
                "{kind:?} seed {seed}: {n0} > budget"
            );
            assert!(n1 * 2 <= n0, "{kind:?} seed {seed}: LOD1 {n1} vs LOD0 {n0}");
            assert!(n0 >= 150, "{kind:?}: suspiciously few triangles ({n0})");
        }
        println!("{kind:?}: worst LOD0 {worst} / {}", kind.tri_budget());
    }
}

const MASK_N: usize = 128;
const MASK_PITCH_DEG: f32 = 30.0;
const YAWS_DEG: [f32; 4] = [0.0, 90.0, 180.0, 270.0];

/// Screen axes for a camera looking down `MASK_PITCH_DEG` from the horizontal toward `yaw`.
fn axes(yaw_deg: f32) -> (Vec3, Vec3) {
    let (sy, cy) = cb_core::math::sin_cos(yaw_deg.to_radians());
    let (sp, cp) = cb_core::math::sin_cos(MASK_PITCH_DEG.to_radians());
    let d = Vec3::new(cy * cp, -sp, sy * cp);
    let r = Vec3::new(-sy, 0.0, cy);
    (r, r.cross(d))
}

/// Triangles above the pad (the parcel skirt and ground covers are not silhouette).
fn solid_tris(m: &MeshData) -> Vec<[Vec3; 3]> {
    let pad = FLAT_GROUND_M + 0.05;
    m.indices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize])))
        .filter(|t| t.iter().any(|p| p.y > pad + 0.3) && !t.iter().all(|p| p.y <= pad))
        .collect()
}

fn project(tris: &[[Vec3; 3]], yaw: f32) -> Vec<[Vec2; 3]> {
    let (r, u) = axes(yaw);
    tris.iter()
        .map(|t| t.map(|p| Vec2::new(p.dot(r), p.dot(u))))
        .collect()
}

fn raster(tris: &[[Vec2; 3]], lo: Vec2, size: f32) -> Vec<bool> {
    let mut m = vec![false; MASK_N * MASK_N];
    let px = size / MASK_N as f32;
    for t in tris {
        let s = t.map(|p| (p - lo) / px);
        let bmin = s[0].min(s[1]).min(s[2]);
        let bmax = s[0].max(s[1]).max(s[2]);
        let edge = |a: Vec2, b: Vec2, p: Vec2| (b - a).perp_dot(p - a);
        let area = edge(s[0], s[1], s[2]);
        if area.abs() < 1e-6 {
            continue;
        }
        let (x0, x1) = (
            bmin.x.floor().max(0.0) as usize,
            (bmax.x.ceil() as usize).min(MASK_N),
        );
        let (y0, y1) = (
            bmin.y.floor().max(0.0) as usize,
            (bmax.y.ceil() as usize).min(MASK_N),
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w = [
                    edge(s[1], s[2], p),
                    edge(s[2], s[0], p),
                    edge(s[0], s[1], p),
                ];
                if w.iter().all(|v| v * area.signum() >= 0.0) {
                    m[y * MASK_N + x] = true;
                }
            }
        }
    }
    m
}

/// Mean IoU over the four yaws of two meshes placed on the same parcel, both masks drawn in one
/// window around both silhouettes.
fn silhouette_iou(a: &MeshData, b: &MeshData) -> f32 {
    let (ta, tb) = (solid_tris(a), solid_tris(b));
    let mut sum = 0.0;
    for yaw in YAWS_DEG {
        let (pa, pb) = (project(&ta, yaw), project(&tb, yaw));
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for t in pa.iter().chain(&pb) {
            for p in t {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        let size = (hi - lo).max_element().max(1.0) * 1.02;
        let lo = 0.5 * (lo + hi) - Vec2::splat(0.5 * size);
        let (ma, mb) = (raster(&pa, lo, size), raster(&pb, lo, size));
        let inter = ma.iter().zip(&mb).filter(|(x, y)| **x && **y).count();
        let union = ma.iter().zip(&mb).filter(|(x, y)| **x || **y).count();
        sum += inter as f32 / union.max(1) as f32;
    }
    sum / YAWS_DEG.len() as f32
}

#[test]
fn silhouette_iou_between_service_types_le_0_6() {
    let lods: Vec<Vec<MeshData>> = CivicKind::ALL
        .iter()
        .map(|&k| (0..4).map(|s| build(k, centred(k), s).2).collect())
        .collect();
    let mut worst = (0.0f32, CivicKind::PoliceStation, CivicKind::PoliceStation);
    for (i, &ka) in CivicKind::ALL.iter().enumerate() {
        for (j, &kb) in CivicKind::ALL.iter().enumerate().skip(i + 1) {
            let iou = (0..4)
                .map(|s| silhouette_iou(&lods[i][s], &lods[j][s]))
                .fold(0.0f32, f32::max);
            if iou > 0.4 {
                println!("P {ka:?} vs {kb:?}: {iou:.3}");
            }
            if iou > worst.0 {
                worst = (iou, ka, kb);
            }
        }
    }
    println!("worst pair: {worst:?}");
    assert!(worst.0 <= 0.6, "silhouettes too similar: {worst:?}");
}

/// A zoned building on the same parcel: setbacks inset the parcel; houses and halls get a
/// fitted rectangle (capped like `RectSpec::HOUSE` / `HALL`) at the front of the inset.
fn zoned_on_parcel(kind: CivicKind, zone: Zone, level: u8, seed: u64) -> Option<MeshData> {
    let (w, d) = kind.parcel_m();
    let fr = centred(kind);
    let sb = match zone {
        Zone::Residential => [3.0, 1.5, 4.0],
        Zone::Commercial => [1.0, 0.0, 2.0],
        Zone::Industrial => [4.0, 2.0, 4.0],
    };
    let poly = fr.rect([0.0, 0.0, w, d]).to_vec();
    let inset = fr.rect([sb[1], sb[0], w - sb[1], d - sb[2]]).to_vec();
    let (mw, md) = match zone {
        Zone::Residential => (16.0, 16.0),
        _ => (50.0, 40.0),
    };
    let fw = (w - 2.0 * sb[1]).min(mw);
    let fd = (d - sb[0] - sb[2]).min(md);
    let u0 = 0.5 * (w - fw);
    let fp = fr.rect([u0, sb[0], u0 + fw, sb[0] + fd]);
    let shape = LotShape {
        polygon: &poly,
        inset: &inset,
        front: (fr.at(0.0, 0.0), fr.at(w, 0.0)),
        footprint: Some(fp),
    };
    let m = zoned::layout(zone, level, &shape, seed)?;
    let f = m.plan_foundation(&flat)?;
    Some(m.mesh(&f, &flat, 0).1)
}

#[test]
fn silhouette_iou_vs_zoned_same_footprint_le_0_6() {
    let mut worst = (0.0f32, CivicKind::PoliceStation, Zone::Residential, 0u8);
    for kind in CivicKind::ALL {
        for seed in 0..8u64 {
            let lod1 = build(kind, centred(kind), seed).2;
            for zone in [Zone::Residential, Zone::Commercial, Zone::Industrial] {
                for level in 1..=zone.max_level() {
                    let Some(z) = zoned_on_parcel(kind, zone, level, seed + 11) else {
                        continue;
                    };
                    let iou = silhouette_iou(&lod1, &z);
                    if iou > worst.0 {
                        worst = (iou, kind, zone, level);
                    }
                    if iou > 0.55 {
                        println!("Z {kind:?} s{seed} {zone:?}{level} {iou:.3}");
                    }
                }
            }
        }
    }
    println!("worst: {worst:?}");
    assert!(
        worst.0 <= 0.6,
        "civic silhouette too close to a zoned one: {worst:?}"
    );
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

const GOLDEN: u64 = 0xb838_a135_3e42_c37b;

#[test]
fn civic_golden_hash() {
    let mut all = Vec::new();
    for kind in CivicKind::ALL {
        for seed in [1u64, 2] {
            let fr = civic::frame_at(kind, Vec2::new(12.5, -7.25), Vec2::new(0.8, 0.6));
            let l = civic::layout(kind, fr, seed);
            let f = l
                .plan_foundation(&|p: Vec2| 4.0 + 0.004 * p.x)
                .expect("buildable");
            let (lod0, lod1) = civic::mesh(&l, &f, 7);
            all.push(mesh_hash(&lod0));
            all.push(mesh_hash(&lod1));
        }
    }
    let got = cb_core::hash::stable_hash(&all);
    println!("civic golden: {got:#018x}");
    assert_eq!(
        got, GOLDEN,
        "civic output changed (update GOLDEN if intended)"
    );
}
