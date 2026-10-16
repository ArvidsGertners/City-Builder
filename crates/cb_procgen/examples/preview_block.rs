//! Software-rendered preview of a residential block (dev tool, English only): frontage →
//! subdivision → fitting → houses + yards on a gently sloped ground, drawn with a tiny z-buffer
//! rasterizer (flat shading, sun + ambient, baked AO) to PPM files.
//!
//! `cargo run -p cb_procgen --release --example preview_block -- <out_dir> [seed]`

use std::io::Write as _;

use cb_core::math as m;
use cb_geometry::CubicBezier2;
use cb_geometry::MeshData;
use cb_geometry::d2;
use cb_geometry::frontage::{Frontage, Part};
use cb_geometry::rect_fit::{self, FitParams, RectSpec};
use cb_geometry::subdivide::{ZoneParams, subdivide};
use cb_network::{CrossSection, Profile, RoadKind, SegmentGeom};
use cb_procgen::building::house;
use cb_procgen::road_mesh::{self, RoadMeshParams};
use cb_procgen::yard::{self, YardLot};
use glam::{Mat3, Vec2, Vec3};

const W: usize = 1600;
const H: usize = 1000;
const SS: usize = 2;

fn ground(p: Vec2) -> f32 {
    0.035 * p.x + 0.02 * p.y + 0.6 * cb_core::math::sin(p.x * 0.05) * cb_core::math::cos(p.y * 0.04)
}

struct Raster {
    w: usize,
    h: usize,
    color: Vec<Vec3>,
    depth: Vec<f32>,
}

struct Camera {
    eye: Vec3,
    rot: Mat3,
    f: f32,
}

impl Camera {
    fn orbit(
        target: Vec3,
        dist: f32,
        pitch_deg: f32,
        yaw_deg: f32,
        fov_deg: f32,
        h: usize,
    ) -> Self {
        let (p, y) = (pitch_deg.to_radians(), yaw_deg.to_radians());
        let back = Vec3::new(m::sin(y) * m::cos(p), m::sin(p), m::cos(y) * m::cos(p));
        let eye = target + back * dist;
        let fwd = (target - eye).normalize();
        let right = fwd.cross(Vec3::Y).normalize();
        let up = right.cross(fwd);
        let rot = Mat3::from_cols(right, up, -fwd).transpose();
        let f = 0.5 * h as f32 / m::tan(0.5 * fov_deg.to_radians());
        Self { eye, rot, f }
    }
    fn project(&self, p: Vec3, w: usize, h: usize) -> Option<Vec3> {
        let v = self.rot * (p - self.eye);
        if v.z > -0.5 {
            return None;
        }
        let z = -v.z;
        Some(Vec3::new(
            0.5 * w as f32 + self.f * v.x / z,
            0.5 * h as f32 - self.f * v.y / z,
            z,
        ))
    }
}

fn srgb_to_lin(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        m::powf((c + 0.055) / 1.055, 2.4)
    }
}

fn lin_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * m::powf(c, 1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

impl Raster {
    fn new(w: usize, h: usize) -> Self {
        let mut color = vec![Vec3::ZERO; w * h];
        for (i, c) in color.iter_mut().enumerate() {
            let t = (i / w) as f32 / h as f32;
            *c = Vec3::new(0.55, 0.70, 0.86).lerp(Vec3::new(0.85, 0.88, 0.86), t);
        }
        Self {
            w,
            h,
            color,
            depth: vec![f32::INFINITY; w * h],
        }
    }

    fn draw(&mut self, cam: &Camera, m: &MeshData) {
        let sun = Vec3::new(-0.45, 0.8, -0.35).normalize();
        for t in 0..m.triangle_count() {
            let idx = [m.indices[3 * t], m.indices[3 * t + 1], m.indices[3 * t + 2]];
            let wp = idx.map(|i| Vec3::from_array(m.positions[i as usize]));
            let n = (wp[1] - wp[0]).cross(wp[2] - wp[0]).normalize_or_zero();
            // Back-face culling: faces point away from the camera.
            if n.dot(cam.eye - wp[0]) <= 0.0 {
                continue;
            }
            let Some(s0) = cam.project(wp[0], self.w, self.h) else {
                continue;
            };
            let Some(s1) = cam.project(wp[1], self.w, self.h) else {
                continue;
            };
            let Some(s2) = cam.project(wp[2], self.w, self.h) else {
                continue;
            };
            let light = 0.42 + 0.75 * n.dot(sun).max(0.0) + 0.08 * n.y.max(0.0);
            let cols = idx.map(|i| {
                let c = m.colors[i as usize];
                Vec3::new(srgb_to_lin(c[0]), srgb_to_lin(c[1]), srgb_to_lin(c[2]))
                    * (f32::from(c[3]) / 255.0)
                    * light
            });
            let (minx, maxx) = (
                s0.x.min(s1.x).min(s2.x).floor().max(0.0) as i32,
                s0.x.max(s1.x).max(s2.x).ceil().min(self.w as f32 - 1.0) as i32,
            );
            let (miny, maxy) = (
                s0.y.min(s1.y).min(s2.y).floor().max(0.0) as i32,
                s0.y.max(s1.y).max(s2.y).ceil().min(self.h as f32 - 1.0) as i32,
            );
            let area = (s1.x - s0.x) * (s2.y - s0.y) - (s1.y - s0.y) * (s2.x - s0.x);
            if area.abs() < 1e-6 {
                continue;
            }
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((s1.x - px) * (s2.y - py) - (s1.y - py) * (s2.x - px)) / area;
                    let w1 = ((s2.x - px) * (s0.y - py) - (s2.y - py) * (s0.x - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let z = 1.0 / (w0 / s0.z + w1 / s1.z + w2 / s2.z);
                    let k = y as usize * self.w + x as usize;
                    if z < self.depth[k] {
                        self.depth[k] = z;
                        let c =
                            (cols[0] * (w0 / s0.z) + cols[1] * (w1 / s1.z) + cols[2] * (w2 / s2.z))
                                * z;
                        // Distance haze.
                        let fog = (1.0 - m::exp(-z / 900.0)).clamp(0.0, 0.6);
                        self.color[k] = c.lerp(Vec3::new(0.78, 0.84, 0.88), fog);
                    }
                }
            }
        }
    }

    fn save(&self, path: &str) -> std::io::Result<()> {
        let (ow, oh) = (self.w / SS, self.h / SS);
        let mut out = Vec::with_capacity(ow * oh * 3);
        for y in 0..oh {
            for x in 0..ow {
                let mut acc = Vec3::ZERO;
                for dy in 0..SS {
                    for dx in 0..SS {
                        acc += self.color[(y * SS + dy) * self.w + x * SS + dx];
                    }
                }
                let c = acc / (SS * SS) as f32;
                out.extend_from_slice(&[lin_to_srgb(c.x), lin_to_srgb(c.y), lin_to_srgb(c.z)]);
            }
        }
        let mut f = std::fs::File::create(path)?;
        write!(f, "P6\n{ow} {oh}\n255\n")?;
        f.write_all(&out)
    }
}

/// Grass ground.
fn context(lo: Vec2, hi: Vec2) -> MeshData {
    let mut m = MeshData::default();
    let step = 4.0;
    let (nx, nz) = (
        ((hi.x - lo.x) / step) as usize,
        ((hi.y - lo.y) / step) as usize,
    );
    for j in 0..nz {
        for i in 0..nx {
            let p = |a: usize, b: usize| {
                let q = Vec2::new(lo.x + a as f32 * step, lo.y + b as f32 * step);
                Vec3::new(q.x, ground(q) - 0.02, q.y)
            };
            let shade = if (i * 7 + j * 13) % 5 == 0 {
                [150, 184, 114, 255]
            } else {
                [141, 176, 106, 255]
            };
            let q = [p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1)];
            let n = (q[1] - q[0]).cross(q[2] - q[0]);
            if n.y > 0.0 {
                m.quad(q, shade, 0);
            } else {
                m.quad([q[0], q[3], q[2], q[1]], shade, 0);
            }
        }
    }
    m
}

/// Subdivides one road side and adds houses, yards and lot lines to the scene.
fn populate(
    f: &Frontage,
    depth: f64,
    seed: u64,
    scene: &mut Vec<MeshData>,
    stats: &mut [usize; 3],
) -> Option<()> {
    let plot = f.plot_polygon(depth, 12.0)?;
    let zp = ZoneParams::RESIDENTIAL;
    let out = subdivide(f, &plot, &zp, depth, 0, seed, None);
    let fits = rect_fit::fit_lots(
        &out.lots,
        f,
        &FitParams {
            setback: zp.setback,
            rect: Some(RectSpec::HOUSE),
        },
    );
    for (k, (lot, fit)) in out.lots.iter().zip(&fits).enumerate() {
        let Some(fp) = fit.footprint else { continue };
        let fp = fp.map(glam::DVec2::as_vec2);
        let level = 1 + ((seed as usize + k) % 3) as u8;
        let l = house::layout(fp, level, seed * 1000 + k as u64);
        let Some(fd) = l.plan_foundation(&ground) else {
            continue;
        };
        let (lod0, _lod1) = house::mesh(&l, &fd, &ground, k as u16);
        let poly = d2::to_f32(&lot.poly);
        let edges = rect_fit::lot_edge_kinds(lot, f);
        let yl = YardLot {
            polygon: &poly,
            edges: &edges,
            front_start: f.point_at(lot.s0).as_vec2(),
            front_end: f.point_at(lot.s1).as_vec2(),
        };
        let ym = yard::yard(&l, &yl, fd.floor_y, &ground, k as u16, seed ^ k as u64);
        stats[0] += lod0.triangle_count();
        stats[1] += ym.triangle_count();
        stats[2] += 1;
        scene.push(lod0);
        scene.push(ym);
        let mut lines = MeshData::default();
        let n = poly.len();
        for i in 0..n {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let dd = (b - a).normalize_or_zero();
            let nn = Vec2::new(-dd.y, dd.x) * 0.12;
            let v = |p: Vec2| Vec3::new(p.x, ground(p) + 0.06, p.y);
            let q = [v(a - nn), v(b - nn), v(b + nn), v(a + nn)];
            let up = (q[1] - q[0]).cross(q[2] - q[0]).y > 0.0;
            lines.quad(
                if up { q } else { [q[0], q[3], q[2], q[1]] },
                [244, 239, 230, 255],
                0,
            );
        }
        scene.push(lines);
    }
    Some(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "target/preview_block".into());
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(7);
    std::fs::create_dir_all(&dir)?;
    let mut scene: Vec<MeshData> = Vec::new();
    let mut stats = [0usize; 3];
    // A curved street (S-curve), houses on both sides; frontage = the road edges.
    let curve = CubicBezier2::new(
        Vec2::new(-150.0, 0.0),
        Vec2::new(-40.0, -10.0),
        Vec2::new(10.0, 90.0),
        Vec2::new(140.0, 70.0),
    );
    let g = SegmentGeom::new(curve, 6.0);
    let s: Vec<f32> = (0..=((g.length() / 4.0) as usize))
        .map(|k| k as f32 * 4.0)
        .chain(std::iter::once(g.length()))
        .collect();
    let h: Vec<f32> = s.iter().map(|&t| ground(g.point_at(t)) + 0.15).collect();
    let prof = Profile::new(s, h);
    scene.push(road_mesh::segment_mesh(
        &g,
        &prof,
        &CrossSection::of(RoadKind::Street),
        &RoadMeshParams::whole(g.length()),
        0,
    ));
    let depth = 34.0;
    let left: Vec<glam::DVec2> = g.left.points().iter().map(|v| v.as_dvec2()).collect();
    let right: Vec<glam::DVec2> = g
        .right
        .points()
        .iter()
        .rev()
        .map(|v| v.as_dvec2())
        .collect();
    for (k, side) in [left, right].into_iter().enumerate() {
        let f = Frontage::new(&[Part::Points(side)], depth).ok_or("frontage")?;
        populate(&f, depth, seed + k as u64, &mut scene, &mut stats);
    }
    let (lo, hi) = (Vec2::new(-200.0, -80.0), Vec2::new(190.0, 150.0));
    scene.push(context(lo, hi));
    println!(
        "houses={} avg_house_tris={} avg_yard_tris={}",
        stats[2],
        stats[0] / stats[2].max(1),
        stats[1] / stats[2].max(1)
    );
    let center = Vec2::new(0.0, 35.0);
    let target = Vec3::new(center.x, ground(center), center.y);
    let near_p = g.point_at(0.35 * g.length());
    let near = Vec3::new(near_p.x, ground(near_p), near_p.y);
    let shots = [
        ("overview", target, 300.0, 45.0, 20.0),
        ("block_y0", near, 120.0, 35.0, 0.0),
        ("block_y120", near, 120.0, 35.0, 120.0),
        ("block_y240", near, 120.0, 35.0, 240.0),
        ("low", near, 45.0, 18.0, 200.0),
    ];
    for (name, tgt, dist, pitch, yaw) in shots {
        let cam = Camera::orbit(tgt, dist, pitch, yaw, 40.0, H * SS);
        let mut r = Raster::new(W * SS, H * SS);
        for m in &scene {
            r.draw(&cam, m);
        }
        r.save(&format!("{dir}/{name}.ppm"))?;
    }
    Ok(())
}
