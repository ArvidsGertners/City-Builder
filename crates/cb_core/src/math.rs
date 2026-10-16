//! Portable math facade (ADR-0043).
//!
//! Simulation-affecting code must call these instead of the `f32`/`f64` transcendental methods
//! (enforced by clippy `disallowed-methods`). They forward to the pure-Rust `libm` crate, which
//! gives bit-identical results on x86-64 and aarch64. `sqrt`, `abs`, `floor`, `ceil`, `round`,
//! `min`/`max` are IEEE-exact everywhere and may be used directly.

/// π as `f32`.
pub const PI: f32 = core::f32::consts::PI;
/// τ = 2π as `f32`.
pub const TAU: f32 = core::f32::consts::TAU;

macro_rules! unary {
    ($($(#[$doc:meta])* $name:ident => $libm:ident;)+) => {$(
        $(#[$doc])*
        #[inline]
        #[must_use]
        pub fn $name(x: f32) -> f32 {
            libm::$libm(x)
        }
    )+};
}

unary! {
    /// Sine (radians).
    sin => sinf;
    /// Cosine (radians).
    cos => cosf;
    /// Tangent (radians).
    tan => tanf;
    /// Arcsine.
    asin => asinf;
    /// Arccosine.
    acos => acosf;
    /// Arctangent.
    atan => atanf;
    /// e^x.
    exp => expf;
    /// 2^x.
    exp2 => exp2f;
    /// Natural logarithm.
    ln => logf;
    /// Base-2 logarithm.
    log2 => log2f;
    /// Base-10 logarithm.
    log10 => log10f;
    /// Cube root.
    cbrt => cbrtf;
    /// Hyperbolic tangent.
    tanh => tanhf;
}

/// Four-quadrant arctangent of `y / x`.
#[inline]
#[must_use]
pub fn atan2(y: f32, x: f32) -> f32 {
    libm::atan2f(y, x)
}

/// `base^exponent`.
#[inline]
#[must_use]
pub fn powf(base: f32, exponent: f32) -> f32 {
    libm::powf(base, exponent)
}

/// `sqrt(x² + y²)` without intermediate overflow.
#[inline]
#[must_use]
pub fn hypot(x: f32, y: f32) -> f32 {
    libm::hypotf(x, y)
}

/// Sine and cosine together.
#[inline]
#[must_use]
pub fn sin_cos(x: f32) -> (f32, f32) {
    (sin(x), cos(x))
}

/// Linear interpolation `a + (b - a) * t` (plain mul/add, never fused).
#[inline]
#[must_use]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Hermite smoothstep of `x` between `edge0` and `edge1`, clamped to [0, 1].
#[inline]
#[must_use]
pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 == edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Degrees to radians.
#[inline]
#[must_use]
pub fn to_radians(deg: f32) -> f32 {
    deg * (PI / 180.0)
}

/// Radians to degrees.
#[inline]
#[must_use]
pub fn to_degrees(rad: f32) -> f32 {
    rad * (180.0 / PI)
}

/// `f64` natural logarithm (for statistics and RNG distributions).
#[inline]
#[must_use]
pub fn ln_f64(x: f64) -> f64 {
    libm::log(x)
}

/// `f64` cosine.
#[inline]
#[must_use]
pub fn cos_f64(x: f64) -> f64 {
    libm::cos(x)
}

/// `f64` sine.
#[inline]
#[must_use]
pub fn sin_f64(x: f64) -> f64 {
    libm::sin(x)
}

/// `f64` sine and cosine.
#[inline]
#[must_use]
pub fn sin_cos_f64(x: f64) -> (f64, f64) {
    (libm::sin(x), libm::cos(x))
}

/// `f64` four-quadrant arctangent of `y / x`.
#[inline]
#[must_use]
pub fn atan2_f64(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-6 * b.abs().max(1.0)
    }

    #[test]
    fn math_facade_reference_values() {
        assert_eq!(sin(0.0), 0.0);
        assert!(close(sin(PI / 2.0), 1.0));
        assert!(close(cos(PI), -1.0));
        assert!(close(exp(1.0), core::f32::consts::E));
        assert!(close(ln(core::f32::consts::E), 1.0));
        assert!(close(atan2(1.0, -1.0), 3.0 * PI / 4.0));
        assert!(close(powf(2.0, 10.0), 1024.0));
        assert!(close(hypot(3.0, 4.0), 5.0));
        assert_eq!(smoothstep(0.0, 1.0, 0.5), 0.5);
        assert_eq!(smoothstep(1.0, 1.0, 2.0), 1.0);
        // Bit-exact golden values pin libm's results (identical on every platform).
        assert_eq!(sin(1.0).to_bits(), 0x3f57_6aa4);
        assert_eq!(exp(0.5).to_bits(), 0x3fd3_094c);
    }
}
