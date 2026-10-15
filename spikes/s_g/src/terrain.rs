//! Heightfield, stamps, ADR-0046 compositing, LOD downsampling and the render triangulation.

use glam::Vec2;

/// Samples per side (4096 m at 2 m).
pub const N: usize = 2049;
/// Sample spacing (m).
pub const SP: f32 = 2.0;
/// Render chunk size in cells (128 m).
pub const CHUNK: usize = 64;

#[inline]
pub fn idx(i: usize, j: usize) -> usize {
    j * N + i
}

pub fn pos(i: usize, j: usize) -> Vec2 {
    Vec2::new(i as f32 * SP, j as f32 * SP)
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn value_noise(p: Vec2, seed: u32) -> f32 {
    let x0 = p.x.floor();
    let y0 = p.y.floor();
    let fx = p.x - x0;
    let fy = p.y - y0;
    let (xi, yi) = (x0 as i32, y0 as i32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    let ab = a + (b - a) * sx;
    let cd = c + (d - c) * sx;
    ab + (cd - ab) * sy
}

fn fbm(p: Vec2, octaves: u32, seed: u32) -> f32 {
    let mut amp = 0.5;
    let mut f = 1.0;
    let mut acc = 0.0;
    for o in 0..octaves {
        acc += amp * value_noise(p * f, seed + o * 101);
        f *= 2.0;
        amp *= 0.5;
    }
    acc
}

/// River centreline z(x) and water level W(x).
pub fn river_z(x: f32) -> f32 {
    2300.0 + 420.0 * (x / 650.0).sin() + 120.0 * (x / 190.0).sin()
}
pub fn river_level(x: f32) -> f32 {
    22.0 + 10.0 * (1.0 - x / 4096.0)
}

/// Synthetic valley (for valley bridges): segment a→b, 30 m deep V.
pub const VALLEY: [Vec2; 2] = [Vec2::new(900.0, 3000.0), Vec2::new(1500.0, 3900.0)];

pub struct Terrain {
    pub base: Vec<f32>,
    /// Water surface (NaN = dry).
    pub water: Vec<f32>,
    pub h: Vec<f32>,
    /// Inside a core (any stamp) — LOD min-downsampling and sampler.
    pub core: Vec<bool>,
}

pub fn generate(seed: u32) -> Terrain {
    let mut base = vec![0.0f32; N * N];
    let mut water = vec![f32::NAN; N * N];
    let (va, vb) = (VALLEY[0], VALLEY[1]);
    for j in 0..N {
        for i in 0..N {
            let p = pos(i, j);
            let macro_h = fbm(p / 2500.0, 4, seed) * 150.0;
            let ridge = {
                let r = 1.0 - (fbm(p / 600.0, 3, seed + 7) * 2.0 - 1.0).abs();
                r * r * 45.0 * (macro_h / 150.0)
            };
            let detail = fbm(p / 120.0, 2, seed + 13) * 3.0;
            let mut h = macro_h + ridge + detail;
            // Lowland toward the river.
            let rz = river_z(p.x);
            let d = (p.y - rz).abs();
            let w = river_level(p.x);
            let low = w + 3.0 + d * 0.03 + detail;
            let t = ((d - 60.0) / 500.0).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            h = low + (h - low).max(0.0) * t;
            // Channel.
            if d < 40.0 {
                let bed = w - 4.0;
                let bank = if d < 12.0 {
                    bed
                } else {
                    bed + (low - bed) * ((d - 12.0) / 28.0).clamp(0.0, 1.0)
                };
                h = h.min(bank);
                if d < 15.0 {
                    water[idx(i, j)] = w;
                }
            }
            // Valley.
            let ab = vb - va;
            let tt = ((p - va).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
            let dv = p.distance(va + ab * tt);
            if dv < 80.0 {
                // Tapered ends (no vertical step in the base).
                let end = (tt.min(1.0 - tt) / 0.15).clamp(0.0, 1.0);
                h -= 30.0 * (1.0 - dv / 80.0) * end * end * (3.0 - 2.0 * end);
            }
            base[idx(i, j)] = h;
        }
    }
    Terrain {
        h: base.clone(),
        core: vec![false; N * N],
        base,
        water,
    }
}

/// One stamp contribution at a sample.
#[derive(Clone, Copy)]
pub struct Entry {
    pub idx: u32,
    pub t: f32,
    pub w: f32,
    pub core: bool,
}

pub struct Stamp {
    pub id: u32,
    /// Owning road index (usize::MAX = junction).
    pub owner: usize,
    /// road 2 > abutment 1 > pad 0.
    pub prio: u8,
    pub aabb: (usize, usize, usize, usize),
    pub entries: Vec<Entry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// ADR-0046: cores override; elsewhere base + max fills + min cuts.
    Continuous,
    /// Old ADR-0012: highest weight wins.
    MaxWeight,
}

/// Recomposites `rect` (inclusive sample bounds) from all stamps.
pub fn composite(
    t: &mut Terrain,
    stamps: &[Stamp],
    rect: (usize, usize, usize, usize),
    mode: Mode,
) {
    let (i0, j0, i1, j1) = rect;
    let w = i1 - i0 + 1;
    let n = w * (j1 - j0 + 1);
    let mut core_t = vec![f32::NAN; n];
    let mut core_key = vec![(0u8, u32::MAX); n];
    let mut fill = vec![0.0f32; n];
    let mut cut = vec![0.0f32; n];
    let mut best_w = vec![0.0f32; n];
    let mut best_t = vec![0.0f32; n];
    for s in stamps {
        let (a0, b0, a1, b1) = s.aabb;
        if a1 < i0 || a0 > i1 || b1 < j0 || b0 > j1 {
            continue;
        }
        for e in &s.entries {
            let gi = e.idx as usize;
            let (i, j) = (gi % N, gi / N);
            if i < i0 || i > i1 || j < j0 || j > j1 {
                continue;
            }
            let k = (j - j0) * w + (i - i0);
            if e.core {
                // Highest priority, then lowest stamp id.
                let key = (s.prio, s.id);
                let cur = core_key[k];
                if core_t[k].is_nan() || key.0 > cur.0 || (key.0 == cur.0 && key.1 < cur.1) {
                    core_t[k] = e.t;
                    core_key[k] = key;
                }
            } else {
                let d = e.w * (e.t - t.base[gi]);
                if d > 0.0 {
                    fill[k] = fill[k].max(d);
                } else {
                    cut[k] = cut[k].min(d);
                }
                if e.w > best_w[k] {
                    best_w[k] = e.w;
                    best_t[k] = e.t;
                }
            }
        }
    }
    for j in j0..=j1 {
        for i in i0..=i1 {
            let k = (j - j0) * w + (i - i0);
            let gi = idx(i, j);
            let b = t.base[gi];
            if !core_t[k].is_nan() {
                t.h[gi] = core_t[k];
                t.core[gi] = true;
                continue;
            }
            t.core[gi] = false;
            t.h[gi] = match mode {
                Mode::Continuous => {
                    if fill[k] == 0.0 && cut[k] == 0.0 {
                        b
                    } else {
                        b + fill[k] + cut[k]
                    }
                }
                Mode::MaxWeight => {
                    if best_w[k] > 0.0 {
                        b + best_w[k] * (best_t[k] - b)
                    } else {
                        b
                    }
                }
            };
        }
    }
}

/// Height on the render triangulation of a grid with `step` samples per cell (diagonal
/// (i,j)–(i+1,j+1)).
pub fn interp(grid: &[f32], gn: usize, cell: f32, p: Vec2) -> f32 {
    let fx = (p.x / cell).clamp(0.0, (gn - 1) as f32 - 1e-4);
    let fz = (p.y / cell).clamp(0.0, (gn - 1) as f32 - 1e-4);
    let (i, j) = (fx.floor() as usize, fz.floor() as usize);
    let (u, v) = (fx - i as f32, fz - j as f32);
    let h00 = grid[j * gn + i];
    let h10 = grid[j * gn + i + 1];
    let h01 = grid[(j + 1) * gn + i];
    let h11 = grid[(j + 1) * gn + i + 1];
    if u >= v {
        // Triangle (00, 10, 11).
        h00 + (h10 - h00) * u + (h11 - h10) * v
    } else {
        // Triangle (00, 11, 01).
        h00 + (h11 - h01) * u + (h01 - h00) * v
    }
}

/// LOD k grid by constrained decimation: plain decimation, then the smallest vertex lowering
/// (least squares over barycentric weights, Gauss–Seidel) so that at every core sample the
/// coarse surface is ≤ the core height (never above the road, no sag beyond what is needed).
pub fn lod_grid_constrained(t: &Terrain, k: u32) -> (Vec<f32>, usize) {
    let step = 1usize << k;
    let gn = (N - 1) / step + 1;
    let mut g: Vec<f32> = (0..gn * gn)
        .map(|v| t.h[idx((v % gn) * step, (v / gn) * step)])
        .collect();
    if k == 0 {
        return (g, gn);
    }
    let cores: Vec<usize> = (0..N * N).filter(|gi| t.core[*gi]).collect();
    for _ in 0..12 {
        let mut worst: f32 = 0.0;
        for &gi in &cores {
            let (i, j) = (gi % N, gi / N);
            let (ci, cj) = ((i / step).min(gn - 2), (j / step).min(gn - 2));
            let u = (i - ci * step) as f32 / step as f32;
            let v = (j - cj * step) as f32 / step as f32;
            let v00 = cj * gn + ci;
            let v10 = v00 + 1;
            let v01 = v00 + gn;
            let v11 = v01 + 1;
            let (verts, bw) = if u >= v {
                ([v00, v10, v11], [1.0 - u, u - v, v])
            } else {
                ([v00, v11, v01], [1.0 - v, u, v - u])
            };
            let interp: f32 = (0..3).map(|q| bw[q] * g[verts[q]]).sum();
            let c = t.h[gi] - 0.005;
            if interp > c {
                let s2: f32 = bw.iter().map(|b| b * b).sum();
                let d = (interp - c) / s2.max(1e-6);
                for q in 0..3 {
                    g[verts[q]] -= d * bw[q];
                }
                worst = worst.max(interp - c);
            }
        }
        if worst < 1e-4 {
            break;
        }
    }
    (g, gn)
}

/// LOD k grid (step 2^k samples): min over the ±step neighbourhood where any core sample is
/// in it (never above the road), plain decimation elsewhere.
pub fn lod_grid(t: &Terrain, k: u32, use_min: bool) -> (Vec<f32>, usize) {
    let step = 1usize << k;
    let gn = (N - 1) / step + 1;
    let mut g = vec![0.0f32; gn * gn];
    for vj in 0..gn {
        for vi in 0..gn {
            let (ci, cj) = (vi * step, vj * step);
            let mut h = t.h[idx(ci, cj)];
            if use_min && k > 0 {
                let i0 = ci.saturating_sub(step);
                let j0 = cj.saturating_sub(step);
                let i1 = (ci + step).min(N - 1);
                let j1 = (cj + step).min(N - 1);
                let mut any = false;
                let mut m = f32::INFINITY;
                for j in j0..=j1 {
                    for i in i0..=i1 {
                        let gi = idx(i, j);
                        if t.core[gi] {
                            any = true;
                            m = m.min(t.h[gi]);
                        }
                    }
                }
                // Never above any core sample whose fine cell this vertex's coarse cells cover.
                if any {
                    h = h.min(m);
                }
            }
            g[vj * gn + vi] = h;
        }
    }
    (g, gn)
}
