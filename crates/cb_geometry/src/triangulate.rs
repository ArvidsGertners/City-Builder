//! Ear-clipping triangulation of simple polygons (holes are bridged by the caller via
//! `i_overlay` results, which never contain holes for lots and junction polygons).

use glam::Vec2;

use crate::polygon::is_ccw;
use crate::predicates::orient;

fn in_triangle(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    orient(a, b, p) >= 0 && orient(b, c, p) >= 0 && orient(c, a, p) >= 0
}

/// Triangulates a simple polygon; returns counter-clockwise index triples into `poly`.
/// Degenerate (collinear) vertices are skipped; never loops forever.
#[must_use]
pub fn triangulate(poly: &[Vec2]) -> Vec<[u32; 3]> {
    let n = poly.len();
    if n < 3 {
        return Vec::new();
    }
    let mut idx: Vec<usize> = if is_ccw(poly) {
        (0..n).collect()
    } else {
        (0..n).rev().collect()
    };
    let mut out = Vec::with_capacity(n - 2);
    let mut guard = 0;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) = (idx[(k + m - 1) % m], idx[k], idx[(k + 1) % m]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            let o = orient(a, b, c);
            if o < 0 {
                continue; // reflex
            }
            if o == 0 {
                idx.remove(k); // collinear: drop without emitting a sliver
                clipped = true;
                break;
            }
            let blocked = idx.iter().any(|&q| {
                q != ia
                    && q != ib
                    && q != ic
                    && in_triangle(poly[q], a, b, c)
                    && poly[q] != a
                    && poly[q] != b
                    && poly[q] != c
            });
            if !blocked {
                out.push([ia as u32, ib as u32, ic as u32]);
                idx.remove(k);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 && orient(poly[idx[0]], poly[idx[1]], poly[idx[2]]) > 0 {
        out.push([idx[0] as u32, idx[1] as u32, idx[2] as u32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polygon::area;

    fn tri_area(p: &[Vec2], t: &[[u32; 3]]) -> f32 {
        t.iter()
            .map(|t| area(&[p[t[0] as usize], p[t[1] as usize], p[t[2] as usize]]))
            .sum()
    }

    #[test]
    fn convex_and_concave() {
        let sq = vec![
            Vec2::ZERO,
            Vec2::new(4.0, 0.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(0.0, 4.0),
        ];
        let t = triangulate(&sq);
        assert_eq!(t.len(), 2);
        assert!((tri_area(&sq, &t) - 16.0).abs() < 1e-4);
        // L shape, clockwise input.
        let l: Vec<Vec2> = vec![
            Vec2::ZERO,
            Vec2::new(0.0, 6.0),
            Vec2::new(2.0, 6.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(6.0, 2.0),
            Vec2::new(6.0, 0.0),
        ];
        let t = triangulate(&l);
        assert_eq!(t.len(), 4);
        assert!((tri_area(&l, &t) - area(&l)).abs() < 1e-3);
        for tr in &t {
            assert!(
                orient(l[tr[0] as usize], l[tr[1] as usize], l[tr[2] as usize]) > 0,
                "CCW triangles"
            );
        }
    }

    #[test]
    fn collinear_vertices_and_degenerate() {
        let p = vec![
            Vec2::ZERO,
            Vec2::new(2.0, 0.0),
            Vec2::new(4.0, 0.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(0.0, 4.0),
        ];
        let t = triangulate(&p);
        assert!((tri_area(&p, &t) - 16.0).abs() < 1e-4);
        assert!(triangulate(&[Vec2::ZERO, Vec2::X]).is_empty());
    }
}
