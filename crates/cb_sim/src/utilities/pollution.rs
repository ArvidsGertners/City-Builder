//! Water pollution along rivers (ALGORITHMS §18): sewage outlets inject their untreated flow at
//! the nearest river point; concentration is carried downstream and decays by 5 % per km. The
//! river is a polyline from source to mouth; concentration at a point is the decayed sum of
//! every injection upstream divided by the river's mixing flow.

use glam::Vec2;

/// Concentration per river point for injections `(point index, m³/h)`.
#[must_use]
pub fn advect(
    river: &[Vec2],
    injections: &[(usize, f32)],
    decay_per_km: f32,
    river_flow_m3h: f32,
) -> Vec<f32> {
    let mut mass = vec![0.0f32; river.len()];
    let mut carried = 0.0f32;
    for i in 0..river.len() {
        if i > 0 {
            let km = river[i].distance(river[i - 1]) / 1000.0;
            carried *= cb_core::math::powf(1.0 - decay_per_km, km);
        }
        for (k, flow) in injections {
            if *k == i {
                carried += *flow;
            }
        }
        mass[i] = carried;
    }
    mass.iter().map(|m| m / river_flow_m3h.max(1.0)).collect()
}

/// Index of the river point nearest to `p` and its distance.
#[must_use]
pub fn nearest_point(river: &[Vec2], p: Vec2) -> Option<(usize, f32)> {
    river
        .iter()
        .enumerate()
        .map(|(i, q)| (i, q.distance(p)))
        .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn water_pollution_advects_downstream() {
        let river: Vec<Vec2> = (0..=40).map(|k| Vec2::new(0.0, k as f32 * 100.0)).collect();
        let c = advect(&river, &[(10, 300.0)], 0.05, 3000.0);
        // Clean upstream of the outlet, full at it, decaying by 5 % per km downstream.
        assert!(c[..10].iter().all(|x| *x == 0.0));
        assert!((c[10] - 0.1).abs() < 1e-6);
        assert!((c[20] - 0.1 * 0.95).abs() < 1e-4, "{}", c[20]);
        assert!((c[30] - 0.1 * 0.95 * 0.95).abs() < 1e-4);
        assert!(c.windows(2).skip(10).all(|w| w[1] <= w[0]));
        // Two outlets add up.
        let c2 = advect(&river, &[(10, 300.0), (20, 300.0)], 0.05, 3000.0);
        assert!((c2[20] - (0.1 * 0.95 + 0.1)).abs() < 1e-4);
        assert_eq!(
            nearest_point(&river, Vec2::new(20.0, 1040.0)).map(|x| x.0),
            Some(10)
        );
    }
}
