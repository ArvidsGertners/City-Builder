//! Times terrain edits and chunk meshing on a generated 4 km world (developer tool).
//!
//! `cargo run -p cb_terrain --release --example bench_terrain`

use std::time::Instant;

use cb_terrain::chunk_mesh::{self, LODS};
use cb_terrain::scatter;
use cb_terrain::stamps::{Stamp, StampShape, priority};
use cb_terrain::{Terrain, WorldGenParams, generate};
use glam::{Vec2, Vec3};

fn main() {
    let t0 = Instant::now();
    let world = generate(&WorldGenParams::default(), 1234).expect("valid params");
    let gen_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    let mut t = Terrain::from_world(world);
    println!(
        "generate {gen_ms:.0} ms, Terrain::from_world {:.0} ms",
        t1.elapsed().as_secs_f64() * 1e3
    );
    // 300 m roads following the terrain + 1 m.
    let mut stamp_ms = Vec::new();
    for r in 0..40u32 {
        let a = Vec2::new(-1500.0 + r as f32 * 70.0, -800.0 + (r % 7) as f32 * 160.0);
        let centerline: Vec<Vec3> = (0..=30)
            .map(|k| {
                let p = a + Vec2::new(k as f32 * 10.0, cb_core::math::sin(k as f32 * 0.2) * 20.0);
                Vec3::new(p.x, t.height_at(p) + 1.0, p.y)
            })
            .collect();
        let s = Instant::now();
        t.add_stamp(Stamp {
            id: r + 1,
            priority: priority::ROAD,
            shape: StampShape::Corridor {
                centerline,
                core_half_width_m: 7.5,
                carve_m: 0.15,
            },
        });
        stamp_ms.push(s.elapsed().as_secs_f64() * 1e3);
    }
    stamp_ms.sort_by(f64::total_cmp);
    println!(
        "add 300 m road stamp: p50 {:.2} ms, max {:.2} ms",
        stamp_ms[stamp_ms.len() / 2],
        stamp_ms[stamp_ms.len() - 1]
    );
    let side = chunk_mesh::chunks_per_side(&t);
    for lod in 0..LODS {
        let s = Instant::now();
        let mut verts = 0;
        for cz in 0..side {
            for cx in 0..side {
                verts += chunk_mesh::build(&t, cx, cz, lod).positions.len();
            }
        }
        let ms = s.elapsed().as_secs_f64() * 1e3;
        println!(
            "LOD{lod}: {} chunks in {ms:.0} ms ({:.3} ms/chunk), {verts} vertices",
            side * side,
            ms / (side * side) as f64
        );
    }
    let s = Instant::now();
    let mut n = 0;
    for cx in -16..16 {
        for cz in -16..16 {
            n += scatter::scatter_cell(&t, 1234, (cx, cz)).len();
        }
    }
    println!(
        "scatter 1024 cells (1 km²): {n} instances in {:.0} ms",
        s.elapsed().as_secs_f64() * 1e3
    );
    let s = Instant::now();
    let mut hits = 0;
    for k in 0..10_000 {
        let o = Vec3::new(
            -1000.0 + (k % 100) as f32 * 20.0,
            600.0,
            -1000.0 + (k / 100) as f32 * 20.0,
        );
        hits += usize::from(
            t.raycast(o, Vec3::new(0.3, -1.0, 0.45).normalize(), 5000.0)
                .is_some(),
        );
    }
    println!(
        "10 000 raycasts: {hits} hits in {:.1} ms",
        s.elapsed().as_secs_f64() * 1e3
    );
}
