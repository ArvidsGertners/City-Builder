//! Objective screenshot metrics (ADR-0058): luminance histogram and median, clipped and
//! pure-black shares, magenta (missing material / NaN) pixel count.

/// Metrics of one image.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ShotMetrics {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// 16-bin histogram of 8-bit luma (Rec. 709 weights on encoded values).
    pub luma_histogram: [u32; 16],
    /// Median luma (0–255).
    pub luma_median: u8,
    /// Share of pixels with luma ≥ 250 (%).
    pub clipped_pct: f64,
    /// Share of pixels whose brightest channel is < 8 (%).
    pub black_pct: f64,
    /// Pixels that look like the missing-material magenta (or NaN output).
    pub magenta_count: u64,
    /// Magenta pixels enclosed above and below by geometry in their column: with the clear
    /// colour set to magenta and no sky (`crack_check` shots), background showing through the
    /// terrain silhouette — LOD cracks or holes.
    pub holes: u64,
    /// Mean linear luminance (sRGB decoded, Rec. 709) — ADR-0050 calibration.
    pub lum_mean: f64,
    /// Median linear luminance (from 256 bins of the encoded luma).
    pub lum_median: f64,
}

/// sRGB-encoded byte → linear.
fn srgb_to_linear(v: u32) -> f64 {
    let s = f64::from(v) / 255.0;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Magenta after tonemapping: pure #FF00FF renders as ≈ (198, 75, 195) through TonyMcMapface,
/// so the test is by hue (red ≈ blue, both strong, green well below) rather than by value.
/// No palette colour (ART_DIRECTION) satisfies it.
fn is_magenta(px: &[u8]) -> bool {
    let (r, g, b) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]));
    r > 150 && b > 150 && r.abs_diff(b) < 30 && g * 3 < r.min(b) * 2
}

/// Magenta pixels with non-magenta pixels both above and below them in the same column (space
/// beside the map edge is open to the image border and does not count).
#[must_use]
pub fn holes_rgba8(width: u32, height: u32, rgba: &[u8]) -> u64 {
    let (w, h) = (width as usize, height as usize);
    if rgba.len() < w * h * 4 {
        return 0;
    }
    let mut holes = 0;
    for x in 0..w {
        let solid = |y: usize| !is_magenta(&rgba[(y * w + x) * 4..(y * w + x) * 4 + 4]);
        let (Some(first), Some(last)) =
            ((0..h).find(|&y| solid(y)), (0..h).rev().find(|&y| solid(y)))
        else {
            continue;
        };
        holes += (first..=last).filter(|&y| !solid(y)).count() as u64;
    }
    holes
}

/// Computes metrics for tightly packed RGBA8 pixels.
#[must_use]
pub fn metrics_rgba8(width: u32, height: u32, rgba: &[u8]) -> ShotMetrics {
    let mut m = ShotMetrics {
        width,
        height,
        ..ShotMetrics::default()
    };
    let mut counts = [0u64; 256];
    let (mut clipped, mut black, mut n) = (0u64, 0u64, 0u64);
    let lin: Vec<f64> = (0..256).map(srgb_to_linear).collect();
    let mut lum_sum = 0.0;
    let mut lum_counts = [0u64; 256];
    for px in rgba.chunks_exact(4) {
        let (r, g, b) = (u32::from(px[0]), u32::from(px[1]), u32::from(px[2]));
        let y = 0.2126 * lin[r as usize] + 0.7152 * lin[g as usize] + 0.0722 * lin[b as usize];
        lum_sum += y;
        lum_counts[((y * 255.0).round() as usize).min(255)] += 1;
        let luma = ((2126 * r + 7152 * g + 722 * b + 5000) / 10_000).min(255) as usize;
        counts[luma] += 1;
        m.luma_histogram[luma / 16] += 1;
        if luma >= 250 {
            clipped += 1;
        }
        if r.max(g).max(b) < 8 {
            black += 1;
        }
        if is_magenta(px) {
            m.magenta_count += 1;
        }
        n += 1;
    }
    if n == 0 {
        return m;
    }
    m.holes = holes_rgba8(width, height, rgba);
    let half = n.div_ceil(2);
    let mut acc = 0;
    for (v, c) in counts.iter().enumerate() {
        acc += c;
        if acc >= half {
            m.luma_median = v as u8;
            break;
        }
    }
    let mut acc = 0;
    for (v, c) in lum_counts.iter().enumerate() {
        acc += c;
        if acc >= half {
            m.lum_median = v as f64 / 255.0;
            break;
        }
    }
    m.lum_mean = lum_sum / n as f64;
    m.clipped_pct = 100.0 * clipped as f64 / n as f64;
    m.black_pct = 100.0 * black as f64 / n as f64;
    m
}

/// JSON object for one shot (no serde_json dependency; all values are numbers).
#[must_use]
pub fn to_json(name: &str, m: &ShotMetrics) -> String {
    let hist: Vec<String> = m.luma_histogram.iter().map(u32::to_string).collect();
    format!(
        "{{\"name\":{name:?},\"width\":{},\"height\":{},\"luma_median\":{},\"clipped_pct\":{:.3},\"black_pct\":{:.3},\"magenta_count\":{},\"holes\":{},\"lum_mean\":{:.4},\"lum_median\":{:.4},\"luma_histogram\":[{}]}}",
        m.width,
        m.height,
        m.luma_median,
        m.clipped_pct,
        m.black_pct,
        m.magenta_count,
        m.holes,
        m.lum_mean,
        m.lum_median,
        hist.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_on_synthetic_images() {
        // 2 black, 1 white, 1 magenta pixel.
        let px = [
            0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 0, 255, 255,
        ];
        let m = metrics_rgba8(2, 2, &px);
        assert_eq!(m.magenta_count, 1);
        // Tonemapped magenta counts too.
        assert_eq!(metrics_rgba8(1, 1, &[198, 75, 195, 255]).magenta_count, 1);
        // Terracotta roof, sage wall, sky: not magenta.
        for c in [[182, 101, 74], [183, 195, 160], [214, 230, 242]] {
            assert_eq!(
                metrics_rgba8(1, 1, &[c[0], c[1], c[2], 255]).magenta_count,
                0
            );
        }
        // Column: magenta sky, terrain, magenta crack, terrain, magenta beyond the edge -> one hole.
        let col = [
            255, 0, 255, 255, 90, 120, 60, 255, 255, 0, 255, 255, 90, 120, 60, 255, 255, 0, 255,
            255,
        ];
        assert_eq!(holes_rgba8(1, 5, &col), 1);
        assert!((m.black_pct - 50.0).abs() < 1e-9);
        assert!((m.clipped_pct - 25.0).abs() < 1e-9);
        assert_eq!(m.luma_histogram.iter().sum::<u32>(), 4);
        assert_eq!(m.luma_median, 0);
        let mid = metrics_rgba8(1, 1, &[128, 128, 128, 255]);
        assert_eq!(mid.luma_median, 128);
        assert!(to_json("x", &mid).contains("\"luma_median\":128"));
        // sRGB 188 ≈ linear 0.5.
        let half = metrics_rgba8(1, 1, &[188, 188, 188, 255]);
        assert!((half.lum_mean - 0.5).abs() < 0.01, "{}", half.lum_mean);
        assert!((half.lum_median - 0.5).abs() < 0.01);
    }
}
