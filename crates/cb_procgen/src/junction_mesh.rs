//! Junction surface meshes (ALGORITHMS §5): the carriageway polygon with per-vertex heights,
//! raised sidewalk strips around every corner with curb faces toward the carriageway and skirts
//! on the outside. Crosswalks and stop lines come with the lane graph (M3).

use cb_geometry::MeshData;
use cb_network::junction::Junction;
use glam::{Vec2, Vec3};

use crate::paint::Painter;
use crate::palette;
use crate::road_mesh::{CURB_H_M, SKIRT_M};
use crate::surface::Surface;

/// Resamples a 3D polyline to `n + 1` points evenly by plan arc length.
fn resample(pts: &[Vec3], n: usize) -> Vec<Vec3> {
    let mut cum = vec![0.0f32; pts.len()];
    for k in 1..pts.len() {
        cum[k] = cum[k - 1] + Vec2::new(pts[k].x - pts[k - 1].x, pts[k].z - pts[k - 1].z).length();
    }
    let total = cum.last().copied().unwrap_or(0.0);
    (0..=n)
        .map(|q| {
            let s = total * q as f32 / n as f32;
            let k = cum
                .partition_point(|v| *v <= s)
                .clamp(1, pts.len().max(2) - 1)
                - 1;
            let seg = (cum[k + 1] - cum[k]).max(1e-9);
            pts[k].lerp(pts[k + 1], ((s - cum[k]) / seg).clamp(0.0, 1.0))
        })
        .collect()
}

/// Builds the mesh of one junction.
#[must_use]
pub fn junction_mesh(j: &Junction, slot: u16) -> MeshData {
    let mut p = Painter::new(slot, u64::from(slot) ^ 0x7C0D);
    // Carriageway.
    let plan: Vec<Vec2> = j.inner.iter().map(|v| Vec2::new(v.x, v.z)).collect();
    let tris = cb_geometry::triangulate::triangulate(&plan);
    for t in tris {
        let pts = [
            j.inner[t[0] as usize],
            j.inner[t[1] as usize],
            j.inner[t[2] as usize],
        ];
        p.face_ao(
            &pts,
            &[1.0; 3],
            palette::ASPHALT,
            Surface::Paved,
            0,
            Vec3::Y,
        );
    }
    let center = j.inner.iter().copied().sum::<Vec3>() / j.inner.len().max(1) as f32;
    let up = Vec3::Y * CURB_H_M;
    for c in &j.corners {
        if c.outer.len() < 2 || c.inner.len() < 2 {
            continue;
        }
        let n = c.outer.len().max(c.inner.len()) - 1;
        let (o, i) = (resample(&c.outer, n), resample(&c.inner, n));
        for k in 0..n {
            p.face_ao(
                &[i[k] + up, i[k + 1] + up, o[k + 1] + up, o[k] + up],
                &[1.0; 4],
                palette::SIDEWALK,
                Surface::Paved,
                0,
                Vec3::Y,
            );
            // Curb face toward the carriageway.
            let mid = (i[k] + i[k + 1]) * 0.5;
            let toward = Vec3::new(center.x - mid.x, 0.0, center.z - mid.z);
            p.face_ao(
                &[i[k], i[k + 1], i[k + 1] + up, i[k] + up],
                &[0.85, 0.85, 1.0, 1.0],
                palette::CURB,
                Surface::Solid,
                0,
                toward,
            );
            // Skirt outside.
            let mo = (o[k] + o[k + 1]) * 0.5;
            let away = Vec3::new(mo.x - center.x, 0.0, mo.z - center.z);
            let down = Vec3::Y * SKIRT_M;
            p.face_ao(
                &[o[k] - down, o[k + 1] - down, o[k + 1] + up, o[k] + up],
                &[0.8, 0.8, 1.0, 1.0],
                palette::CURB.scale(0.9),
                Surface::Solid,
                0,
                away,
            );
        }
    }
    p.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::Polyline2;
    use cb_network::Profile;
    use cb_network::junction::{Approach, build};

    #[test]
    fn x_junction_mesh() {
        let apps: Vec<Approach> = [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y]
            .iter()
            .map(|d| Approach {
                centerline: Polyline2::new((0..=40).map(|k| *d * (k as f32 * 2.5))),
                half_width_m: 6.0,
                carriageway_half_m: 4.0,
                corner_radius_m: 6.0,
                profile: Profile::flat(100.0, 3.0),
                reversed: false,
                lanes: Vec::new(),
            })
            .collect();
        let j = build(&apps).unwrap();
        let m = junction_mesh(&j, 2);
        assert!(
            m.triangle_count() > 20 && m.triangle_count() < 600,
            "{}",
            m.triangle_count()
        );
        let (lo, hi) = m.bounds();
        assert!(lo.y >= 3.0 - SKIRT_M - 1e-4 && hi.y <= 3.0 + CURB_H_M + 1e-4);
        // Carriageway faces up.
        assert!(m.face_normal(0).y > 0.9);
    }
}
