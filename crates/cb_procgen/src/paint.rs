//! Mesh painting helpers shared by the generators: oriented flat faces (winding fixed from an
//! outward hint so callers never reason about vertex order), baked vertex AO
//! (ART_DIRECTION §2: ground contact −18 % over 0.6 m, under eaves −10 %) and per-face ±2 %
//! value noise.

use cb_core::rng::Pcg32;
use cb_geometry::MeshData;
use glam::{Vec2, Vec3};

use crate::palette::Rgb;
use crate::surface::{Surface, pack};

/// Ground-contact AO depth (m).
pub const CONTACT_AO_M: f32 = 0.6;
/// Ground-contact AO strength.
pub const CONTACT_AO: f32 = 0.18;
/// Under-eave AO strength.
pub const EAVE_AO: f32 = 0.10;

/// Builds one mesh.
pub struct Painter {
    /// The mesh being built.
    pub mesh: MeshData,
    rng: Pcg32,
    slot: u16,
    /// Height of ground contact for AO (vertices within 0.6 m above darken).
    pub contact_y: f32,
}

impl Painter {
    /// New painter for one object (`slot` into the per-object parameter buffer).
    #[must_use]
    pub fn new(slot: u16, seed: u64) -> Self {
        Self {
            mesh: MeshData::default(),
            rng: Pcg32::from_seed(seed),
            slot,
            contact_y: f32::NEG_INFINITY,
        }
    }

    /// Ground-contact AO factor at height `y`.
    #[must_use]
    pub fn contact_ao(&self, y: f32) -> f32 {
        let t = ((y - self.contact_y) / CONTACT_AO_M).clamp(0.0, 1.0);
        1.0 - CONTACT_AO * (1.0 - t)
    }

    /// Takes the finished mesh.
    #[must_use]
    pub fn finish(self) -> MeshData {
        self.mesh
    }

    /// Flat convex face (fan) with per-vertex AO factors; the winding is chosen so the face
    /// normal points along `outward`.
    pub fn face_ao(
        &mut self,
        pts: &[Vec3],
        ao: &[f32],
        c: Rgb,
        kind: Surface,
        id: u32,
        outward: Vec3,
    ) {
        let c = c.scale(1.0 + 0.02 * (2.0 * self.rng.next_f32() - 1.0));
        self.face_exact(pts, ao, c, kind, id, outward);
    }

    /// Like [`Self::face_ao`] but without the per-face colour jitter: for surfaces split into
    /// many small triangles (junction fans), where jitter would show the triangulation.
    pub fn face_exact(
        &mut self,
        pts: &[Vec3],
        ao: &[f32],
        c: Rgb,
        kind: Surface,
        id: u32,
        outward: Vec3,
    ) {
        if pts.len() < 3 {
            return;
        }
        let n = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
        let flip = n.dot(outward) < 0.0;
        let packed = pack(self.slot, kind, id);
        let base = self.mesh.vertex_count() as u32;
        for (p, a) in pts.iter().zip(ao) {
            self.mesh.vertex(*p, c.with_ao(*a), packed);
        }
        for k in 1..pts.len() as u32 - 1 {
            if flip {
                self.mesh.tri(base, base + k + 1, base + k);
            } else {
                self.mesh.tri(base, base + k, base + k + 1);
            }
        }
    }

    /// Flat convex face with ground-contact AO.
    pub fn face(&mut self, pts: &[Vec3], c: Rgb, kind: Surface, id: u32, outward: Vec3) {
        let ao: Vec<f32> = pts.iter().map(|p| self.contact_ao(p.y)).collect();
        self.face_ao(pts, &ao, c, kind, id, outward);
    }

    /// Vertical wall quad from `a` to `b` (plan points) between `y0` and `y1`, facing `out`
    /// (plan direction). Split at the contact-AO height so the darkening stays 0.6 m tall;
    /// `eave` darkens the top edge.
    pub fn wall(&mut self, a: Vec2, b: Vec2, y0: f32, y1: f32, c: Rgb, eave: bool, out: Vec2) {
        let outward = Vec3::new(out.x, 0.0, out.y);
        let v = |p: Vec2, y: f32| Vec3::new(p.x, y, p.y);
        let split = self.contact_y + CONTACT_AO_M;
        let top_ao = |s: &Self, y: f32| s.contact_ao(y) * if eave { 1.0 - EAVE_AO } else { 1.0 };
        if split > y0 + 0.05 && split < y1 - 0.05 {
            let (a0, a1) = (self.contact_ao(y0), self.contact_ao(split));
            self.face_ao(
                &[v(a, y0), v(b, y0), v(b, split), v(a, split)],
                &[a0, a0, a1, a1],
                c,
                Surface::Solid,
                0,
                outward,
            );
            let t = top_ao(self, y1);
            self.face_ao(
                &[v(a, split), v(b, split), v(b, y1), v(a, y1)],
                &[a1, a1, t, t],
                c,
                Surface::Solid,
                0,
                outward,
            );
        } else {
            let (a0, t) = (self.contact_ao(y0), top_ao(self, y1));
            self.face_ao(
                &[v(a, y0), v(b, y0), v(b, y1), v(a, y1)],
                &[a0, a0, t, t],
                c,
                Surface::Solid,
                0,
                outward,
            );
        }
    }

    /// Box over a counter-clockwise plan quad/polygon: walls, optional top, no bottom.
    pub fn prism(&mut self, poly: &[Vec2], y0: f32, y1: f32, c: Rgb, top: Option<Rgb>, eave: bool) {
        let n = poly.len();
        let ctr = poly.iter().copied().sum::<Vec2>() / n as f32;
        for k in 0..n {
            let (a, b) = (poly[k], poly[(k + 1) % n]);
            let e = (b - a).normalize_or_zero();
            let mut out = Vec2::new(e.y, -e.x);
            if out.dot((a + b) * 0.5 - ctr) < 0.0 {
                out = -out;
            }
            self.wall(a, b, y0, y1, c, eave, out);
        }
        if let Some(tc) = top {
            let pts: Vec<Vec3> = poly.iter().map(|p| Vec3::new(p.x, y1, p.y)).collect();
            self.face(&pts, tc, Surface::Solid, 0, Vec3::Y);
        }
    }

    /// Axis box in a local frame: `o + ex·u + ey·v` for `u ∈ [u0, u1]`, `v ∈ [v0, v1]`.
    pub fn frame_box(
        &mut self,
        fr: &Frame,
        r: [f32; 4],
        y0: f32,
        y1: f32,
        c: Rgb,
        top: Option<Rgb>,
    ) {
        let q = fr.rect(r);
        self.prism(&q, y0, y1, c, top, false);
    }

    /// Flat strip draped on the ground along a polyline (`width` m, `lift` m above ground),
    /// split every ≤ 2 m so it follows the terrain.
    pub fn ground_strip(
        &mut self,
        line: &[Vec2],
        width: f32,
        lift: f32,
        c: Rgb,
        kind: Surface,
        ground: &dyn Fn(Vec2) -> f32,
    ) {
        for w in line.windows(2) {
            let (a, b) = (w[0], w[1]);
            let len = a.distance(b);
            if len < 1e-3 {
                continue;
            }
            let d = (b - a) / len;
            let side = Vec2::new(-d.y, d.x) * (0.5 * width);
            let steps = (len / 2.0).ceil().max(1.0) as usize;
            for k in 0..steps {
                let p0 = a + d * (len * k as f32 / steps as f32);
                let p1 = a + d * (len * (k + 1) as f32 / steps as f32);
                let q = [p0 - side, p1 - side, p1 + side, p0 + side];
                let pts: Vec<Vec3> = q
                    .iter()
                    .map(|p| Vec3::new(p.x, ground(*p) + lift, p.y))
                    .collect();
                self.face_ao(&pts, &[1.0; 4], c, kind, 0, Vec3::Y);
            }
        }
    }
}

/// Orthonormal plan frame `o + ex·u + ey·v` (counter-clockwise: `ey` is `ex` rotated +90° in
/// the `(x, z)` plane).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Origin.
    pub o: Vec2,
    /// First axis (unit).
    pub ex: Vec2,
    /// Second axis (unit).
    pub ey: Vec2,
}

impl Frame {
    /// World point of local `(u, v)`.
    #[must_use]
    pub fn at(&self, u: f32, v: f32) -> Vec2 {
        self.o + self.ex * u + self.ey * v
    }

    /// World direction of local `(u, v)`.
    #[must_use]
    pub fn dir(&self, u: f32, v: f32) -> Vec2 {
        self.ex * u + self.ey * v
    }

    /// Local coordinates of a world point.
    #[must_use]
    pub fn local(&self, p: Vec2) -> Vec2 {
        let d = p - self.o;
        Vec2::new(d.dot(self.ex), d.dot(self.ey))
    }

    /// World rectangle `[u0, v0, u1, v1]` as a counter-clockwise quad (if the frame is).
    #[must_use]
    pub fn rect(&self, r: [f32; 4]) -> [Vec2; 4] {
        [
            self.at(r[0], r[1]),
            self.at(r[2], r[1]),
            self.at(r[2], r[3]),
            self.at(r[0], r[3]),
        ]
    }

    /// World 3D point of local `(u, v)` at height `y`.
    #[must_use]
    pub fn p3(&self, u: f32, v: f32, y: f32) -> Vec3 {
        let p = self.at(u, v);
        Vec3::new(p.x, y, p.y)
    }

    /// World 3D direction of local `(u, v)` and vertical `y`.
    #[must_use]
    pub fn d3(&self, u: f32, v: f32, y: f32) -> Vec3 {
        let d = self.dir(u, v);
        Vec3::new(d.x, y, d.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_follow_outward_hint() {
        let mut p = Painter::new(0, 1);
        let q = [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, 1.0), Vec3::Z];
        p.face(&q, Rgb(1, 2, 3), Surface::Solid, 0, Vec3::Y);
        p.face(&q, Rgb(1, 2, 3), Surface::Solid, 0, -Vec3::Y);
        let m = p.finish();
        assert!(m.face_normal(0).y > 0.9 && m.face_normal(1).y > 0.9);
        assert!(m.face_normal(2).y < -0.9 && m.face_normal(3).y < -0.9);
    }

    #[test]
    fn contact_ao_band() {
        let mut p = Painter::new(0, 1);
        p.contact_y = 10.0;
        assert!((p.contact_ao(10.0) - 0.82).abs() < 1e-6);
        assert!((p.contact_ao(10.6) - 1.0).abs() < 1e-6);
        p.wall(
            Vec2::ZERO,
            Vec2::X,
            10.0,
            13.0,
            Rgb(200, 200, 200),
            true,
            -Vec2::Y,
        );
        let m = p.finish();
        assert_eq!(m.triangle_count(), 4, "split at the AO band");
        assert!(m.face_normal(0).z < -0.9);
    }
}
