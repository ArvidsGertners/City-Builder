//! Engine-agnostic mesh data in the world vertex format (ADR-0049): position, colour + AO
//! (`[u8; 4]`, linear-ish sRGB bytes, alpha = ambient occlusion) and a packed `u32` (slot /
//! material flags). Normals are not stored: flat shading comes from screen-space derivatives.

use glam::{Vec2, Vec3};

use crate::triangulate::triangulate;

/// Mesh buffers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Positions.
    pub positions: Vec<[f32; 3]>,
    /// Colour (rgb) + ambient occlusion (a).
    pub colors: Vec<[u8; 4]>,
    /// Packed per-vertex slot/flags.
    pub packed: Vec<u32>,
    /// Triangle list (counter-clockwise front faces).
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Vertex count.
    #[must_use]
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Triangle count.
    #[must_use]
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Adds a vertex; returns its index.
    pub fn vertex(&mut self, p: Vec3, color: [u8; 4], packed: u32) -> u32 {
        self.positions.push(p.to_array());
        self.colors.push(color);
        self.packed.push(packed);
        (self.positions.len() - 1) as u32
    }

    /// Adds a triangle.
    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend_from_slice(&[a, b, c]);
    }

    /// Adds a quad `a b c d` (counter-clockwise) as two triangles with its own vertices.
    pub fn quad(&mut self, q: [Vec3; 4], color: [u8; 4], packed: u32) {
        let i: Vec<u32> = q.iter().map(|p| self.vertex(*p, color, packed)).collect();
        self.indices
            .extend_from_slice(&[i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    /// Adds a horizontal polygon at height `y` facing up (`up = true`) or down.
    pub fn polygon(&mut self, poly: &[Vec2], y: f32, up: bool, color: [u8; 4], packed: u32) {
        let base = self.positions.len() as u32;
        for p in poly {
            self.vertex(Vec3::new(p.x, y, p.y), color, packed);
        }
        for t in triangulate(poly) {
            // Polygon XZ is CCW in (x, z); seen from +Y that is clockwise, so flip for "up".
            if up {
                self.tri(base + t[0], base + t[2], base + t[1]);
            } else {
                self.tri(base + t[0], base + t[1], base + t[2]);
            }
        }
    }

    /// Vertical walls along a closed polygon from `y0` to `y1`, facing outward.
    pub fn walls(&mut self, poly: &[Vec2], y0: f32, y1: f32, color: [u8; 4], packed: u32) {
        let ccw = crate::polygon::to_ccw(poly);
        let n = ccw.len();
        for k in 0..n {
            let (a, b) = (ccw[k], ccw[(k + 1) % n]);
            self.quad(
                [
                    Vec3::new(a.x, y0, a.y),
                    Vec3::new(a.x, y1, a.y),
                    Vec3::new(b.x, y1, b.y),
                    Vec3::new(b.x, y0, b.y),
                ],
                color,
                packed,
            );
        }
    }

    /// Prism (extruded polygon) with top and walls.
    pub fn prism(&mut self, poly: &[Vec2], y0: f32, y1: f32, color: [u8; 4], packed: u32) {
        self.walls(poly, y0, y1, color, packed);
        self.polygon(poly, y1, true, color, packed);
    }

    /// Appends another mesh.
    pub fn append(&mut self, other: &Self) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.colors.extend_from_slice(&other.colors);
        self.packed.extend_from_slice(&other.packed);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    /// Geometric normal of triangle `k` (for tests and debug).
    #[must_use]
    pub fn face_normal(&self, k: usize) -> Vec3 {
        let p = |i: u32| Vec3::from_array(self.positions[i as usize]);
        let (a, b, c) = (
            p(self.indices[3 * k]),
            p(self.indices[3 * k + 1]),
            p(self.indices[3 * k + 2]),
        );
        (b - a).cross(c - a).normalize_or(Vec3::Y)
    }

    /// Axis-aligned bounds.
    #[must_use]
    pub fn bounds(&self) -> (Vec3, Vec3) {
        self.positions.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(lo, hi), p| {
                let v = Vec3::from_array(*p);
                (lo.min(v), hi.max(v))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prism_faces_point_outward_and_up() {
        let sq = vec![
            Vec2::new(-1.0, -1.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(-1.0, 1.0),
        ];
        let mut m = MeshData::default();
        m.prism(&sq, 0.0, 3.0, [200, 200, 200, 255], 0);
        assert_eq!(m.triangle_count(), 4 * 2 + 2);
        for k in 0..m.triangle_count() {
            let n = m.face_normal(k);
            let p = |i: u32| Vec3::from_array(m.positions[i as usize]);
            let c = (p(m.indices[3 * k]) + p(m.indices[3 * k + 1]) + p(m.indices[3 * k + 2])) / 3.0;
            let out = Vec3::new(c.x, 0.0, c.z).normalize_or(Vec3::Y);
            if n.y.abs() > 0.5 {
                assert!(n.y > 0.0, "top faces up");
            } else {
                assert!(n.dot(out) > 0.5, "wall {k} faces outward: {n}");
            }
        }
        let (lo, hi) = m.bounds();
        assert_eq!((lo.y, hi.y), (0.0, 3.0));
    }
}
