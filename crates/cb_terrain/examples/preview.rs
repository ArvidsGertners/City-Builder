//! Writes a top-down shaded preview of a generated world as a binary PPM (developer tool).
//!
//! `cargo run -p cb_terrain --release --example preview -- <seed> <out.ppm>`

use std::io::Write;

use cb_terrain::materials::TerrainMaterial;
use cb_terrain::{WorldGenParams, generate};
use glam::{Vec2, Vec3};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let seed: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1234);
    let out = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| format!("preview_{seed}.ppm"));
    let w = generate(&WorldGenParams::default(), seed)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let px = 1024usize;
    let half = w.base.half_extent_m();
    let light = Vec3::new(-0.5, 0.75, -0.4).normalize();
    let mut img = Vec::with_capacity(px * px * 3);
    for y in 0..px {
        for x in 0..px {
            let p = Vec2::new(
                (x as f32 + 0.5) / px as f32 * 2.0 * half - half,
                (y as f32 + 0.5) / px as f32 * 2.0 * half - half,
            );
            let (i, j) = w.base.nearest(p);
            let m = w.materials.get(i, j);
            let pal = [
                [110.0, 160.0, 80.0],
                [150.0, 120.0, 80.0],
                [130.0, 125.0, 120.0],
                [220.0, 205.0, 150.0],
                [50.0, 100.0, 55.0],
            ];
            let mut c = [0.0f32; 3];
            for k in 0..5 {
                for (ch, v) in c.iter_mut().enumerate() {
                    *v += pal[k][ch] * f32::from(m[k]) / 255.0;
                }
            }
            let shade = w.base.normal_at(p).dot(light).max(0.0) * 0.8 + 0.3;
            let hgt = w.base.get(i, j) / 180.0;
            for v in &mut c {
                *v = *v * shade * (0.85 + 0.3 * hgt);
            }
            if let Some(level) = w.water.level(i, j) {
                let depth = (level - w.base.get(i, j)).max(0.0);
                let t = (depth / 6.0).min(1.0);
                c = [70.0 - 40.0 * t, 140.0 - 60.0 * t, 170.0 - 30.0 * t];
            }
            img.extend(c.iter().map(|v| v.clamp(0.0, 255.0) as u8));
        }
    }
    // Outside connections in red.
    for oc in &w.outside {
        for q in oc.path.windows(2) {
            for t in 0..32 {
                let p = q[0] + (q[1] - q[0]) * (t as f32 / 32.0);
                let x = ((p.x + half) / (2.0 * half) * px as f32) as usize;
                let y = ((p.y + half) / (2.0 * half) * px as f32) as usize;
                if x < px && y < px {
                    img[(y * px + x) * 3..(y * px + x) * 3 + 3].copy_from_slice(&[220, 30, 30]);
                }
            }
        }
    }
    let mut f = std::fs::File::create(&out)?;
    write!(f, "P6\n{px} {px}\n255\n")?;
    f.write_all(&img)?;
    let (lo, hi) = w.base.min_max();
    let rock = w
        .materials
        .data()
        .iter()
        .filter(|m| m[TerrainMaterial::Rock as usize] > 128)
        .count() as f32
        / w.materials.data().len() as f32;
    let forest =
        w.forest.data().iter().filter(|v| **v > 128).count() as f32 / w.forest.data().len() as f32;
    println!(
        "wrote {out}: rivers {} lakes {} buildable {:.1} km² height {lo:.0}–{hi:.0} m rock {:.1} % forest {:.0} %",
        w.rivers.len(),
        w.lakes.len(),
        w.report.buildable_km2,
        rock * 100.0,
        forest * 100.0
    );
    Ok(())
}
