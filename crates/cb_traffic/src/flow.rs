//! Macroscopic flow model (ALGORITHMS §13): BPR link travel times from volume and capacity and
//! the congestion factor used for epoch costs.

/// BPR α.
pub const BPR_ALPHA: f32 = 0.15;
/// BPR β.
pub const BPR_BETA: f32 = 4.0;

/// Travel time under load: `t0 (1 + α (V/C)^β)`.
#[must_use]
pub fn bpr(t0: f32, volume: f32, capacity: f32) -> f32 {
    let x = (volume / capacity.max(1.0)).max(0.0);
    t0 * (1.0 + BPR_ALPHA * cb_core::math::powf(x, BPR_BETA))
}

/// Congestion factor `v_eff / v_free = t0 / t`, clamped to `[0.2, 1]`.
#[must_use]
pub fn speed_factor(volume: f32, capacity: f32) -> f32 {
    (1.0 / bpr(1.0, volume, capacity)).clamp(0.2, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpr_shape() {
        assert!((bpr(10.0, 0.0, 1000.0) - 10.0).abs() < 1e-6);
        assert!((bpr(10.0, 1000.0, 1000.0) - 11.5).abs() < 1e-4);
        assert!(speed_factor(5000.0, 1000.0) >= 0.2);
        assert!(speed_factor(100.0, 1000.0) > 0.99);
    }
}
