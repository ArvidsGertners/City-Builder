//! Palettes from `docs/ART_DIRECTION.md` §5 and the per-building colour variation of §2
//! (HSV jitter ±3° hue, ±4 % saturation, ±5 % value; per-face ±2 % value noise).
//!
//! Colours are sRGB bytes; the game linearizes them when it builds GPU meshes (ADR-0049).

use cb_core::rng::Pcg32;

/// An sRGB colour (`a` is not stored here; meshes put AO in alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// `#RRGGBB` as a constant.
#[must_use]
pub const fn hex(v: u32) -> Rgb {
    Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl Rgb {
    /// With an AO factor in alpha (0..=1).
    #[must_use]
    pub fn with_ao(self, ao: f32) -> [u8; 4] {
        [
            self.0,
            self.1,
            self.2,
            (ao.clamp(0.0, 1.0) * 255.0).round() as u8,
        ]
    }

    /// Value (brightness) scaled by `k`.
    #[must_use]
    pub fn scale(self, k: f32) -> Self {
        let f = |c: u8| (f32::from(c) * k).round().clamp(0.0, 255.0) as u8;
        Self(f(self.0), f(self.1), f(self.2))
    }

    /// Linear blend toward `o` by `t`.
    #[must_use]
    pub fn mix(self, o: Self, t: f32) -> Self {
        let f = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Self(f(self.0, o.0), f(self.1, o.1), f(self.2, o.2))
    }

    /// HSV jitter: `dh` degrees, `ds` and `dv` as fractions.
    #[must_use]
    pub fn jitter_hsv(self, dh: f32, ds: f32, dv: f32) -> Self {
        let (h, s, v) = rgb_to_hsv(self);
        hsv_to_rgb(
            (h + dh).rem_euclid(360.0),
            (s * (1.0 + ds)).clamp(0.0, 1.0),
            (v * (1.0 + dv)).clamp(0.0, 1.0),
        )
    }

    /// The per-building variation of ART_DIRECTION §2 drawn from `rng`.
    #[must_use]
    pub fn building_variant(self, rng: &mut Pcg32) -> Self {
        let mut u = || 2.0 * rng.next_f32() - 1.0;
        let (dh, ds, dv) = (3.0 * u(), 0.04 * u(), 0.05 * u());
        self.jitter_hsv(dh, ds, dv)
    }
}

fn rgb_to_hsv(c: Rgb) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(c.0) / 255.0,
        f32::from(c.1) / 255.0,
        f32::from(c.2) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max <= 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let f = |t: f32| ((t + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(f(r), f(g), f(b))
}

/// Picks one entry.
#[must_use]
pub fn pick(rng: &mut Pcg32, set: &[Rgb]) -> Rgb {
    set[rng.below(set.len() as u32) as usize]
}

/// Residential walls (cream, peach, pale yellow, sage, sand).
pub const R_WALLS: [Rgb; 5] = [
    hex(0xEFE3CB),
    hex(0xE8C9A9),
    hex(0xEADBA0),
    hex(0xB7C3A0),
    hex(0xD9C3B0),
];
/// Residential roofs (terracotta, red slate, brown, dark slate).
pub const R_ROOFS: [Rgb; 4] = [hex(0xB6654A), hex(0x9E5543), hex(0x7D5A44), hex(0x5E6670)];
/// Shutters and doors.
pub const R_ACCENTS: [Rgb; 3] = [hex(0x5F7F8E), hex(0x8E5B4A), hex(0x6E7F5A)];
/// Window frames.
pub const WINDOW_FRAME: Rgb = hex(0xF4EFE6);
/// Window glass by day (night emission comes from the surface flag).
pub const GLASS: Rgb = hex(0x55707E);
/// Plinths, steps, chimney caps.
pub const STONE: Rgb = hex(0x9A9590);
/// Chimney brick.
pub const BRICK: Rgb = hex(0x9E5543);

/// Grass base and variations.
pub const GRASS: [Rgb; 3] = [hex(0x8DB06A), hex(0x9CBB72), hex(0x7FA35E)];
/// Garden beds / dirt.
pub const DIRT: [Rgb; 2] = [hex(0xB39066), hex(0xA07E57)];
/// Deciduous foliage.
pub const FOLIAGE: [Rgb; 3] = [hex(0x6F9B4E), hex(0x7FA857), hex(0x5E8A46)];
/// Conifers.
pub const CONIFER: [Rgb; 2] = [hex(0x4E7A52), hex(0x5C8659)];
/// Trunks.
pub const TRUNK: Rgb = hex(0x7A5A43);
/// Hedges (darker foliage).
pub const HEDGE: Rgb = hex(0x5E8A46);
/// Flowers (sparse specks).
pub const FLOWERS: [Rgb; 2] = [hex(0xE8D6A0), hex(0xD9A9B8)];
/// Wooden fences and sheds.
pub const WOOD: [Rgb; 3] = [hex(0xE9E2D2), hex(0x9C7A5B), hex(0x7D5A44)];

/// Dark asphalt.
pub const ASPHALT: Rgb = hex(0x4A4E54);
/// Light asphalt (driveways, dirt-free paths).
pub const ASPHALT_LIGHT: Rgb = hex(0x6C7076);
/// Dirt path.
pub const DIRT_PATH: Rgb = hex(0xA88A62);
/// Track ballast (standalone tram tracks).
pub const BALLAST: Rgb = hex(0x8C8478);
/// Lane markings.
pub const MARKING: Rgb = hex(0xEDE6D3);
/// Avenue centre double line.
pub const MARKING_YELLOW: Rgb = hex(0xE3C35A);
/// Sidewalk.
pub const SIDEWALK: Rgb = hex(0xC9C2B4);
/// Curb.
pub const CURB: Rgb = hex(0xB5AE9F);
/// Paving stones (paths, patios).
pub const PAVING: Rgb = hex(0xBDB4A4);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trip_and_jitter_bounds() {
        for c in R_WALLS.iter().chain(&R_ROOFS).chain(&FOLIAGE) {
            let (h, s, v) = rgb_to_hsv(*c);
            let back = hsv_to_rgb(h, s, v);
            assert!((i32::from(back.0) - i32::from(c.0)).abs() <= 1);
            assert!((i32::from(back.1) - i32::from(c.1)).abs() <= 1);
            assert!((i32::from(back.2) - i32::from(c.2)).abs() <= 1);
        }
        let mut rng = Pcg32::from_seed(1);
        for _ in 0..100 {
            let c = R_WALLS[0].building_variant(&mut rng);
            let (h0, _, v0) = rgb_to_hsv(R_WALLS[0]);
            let (h, _, v) = rgb_to_hsv(c);
            // ±3° jitter plus 8-bit quantisation (≈ 2° at the low saturation of cream).
            assert!(
                (h - h0).abs() <= 6.0 || (h - h0).abs() >= 354.0,
                "{h} vs {h0}"
            );
            assert!((v - v0).abs() <= 0.05 * v0 + 0.01);
        }
        assert_eq!(hex(0x4A4E54), Rgb(0x4A, 0x4E, 0x54));
    }
}
