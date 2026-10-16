//! Segment style (GAME_DESIGN §3, DATA_MODEL §4.1): surface, grass strips and decorations
//! (tree rows, street lamps), with decoration placement evenly spaced along each sidewalk.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::cross_section::{CrossSection, ElementKind, RoadKind};
use crate::profile::Profile;
use crate::segment_geom::SegmentGeom;

/// Tree row spacing range (m).
pub const TREE_SPACING_M: (f32, f32) = (8.0, 24.0);
/// Street lamp spacing range (m).
pub const LAMP_SPACING_M: (f32, f32) = (20.0, 50.0);
/// Kerb height of sidewalks (m; matches the road mesh).
pub const SIDEWALK_LIFT_M: f32 = 0.15;
/// Width of a grass strip along the kerb (m).
pub const GRASS_STRIP_M: f32 = 1.0;
/// Decorations keep this far from the ends of the decorated range (m).
const END_MARGIN_M: f32 = 3.0;

/// Road surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoadSurface {
    /// Default.
    #[default]
    DarkAsphalt,
    /// Lighter asphalt.
    LightAsphalt,
    /// Cobblestone (−5 km/h, raises land value).
    Cobblestone,
    /// Unpaved (dirt paths only).
    Dirt,
}

/// Per-segment style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SegmentStyle {
    /// Surface of the carriageway.
    pub surface: RoadSurface,
    /// Grass strip along the kerb, `[left, right]` of the curve.
    pub grass_strip: [bool; 2],
    /// Tree rows on both sidewalks at this spacing (m).
    pub tree_spacing_m: Option<f32>,
    /// Street lamps on both sidewalks at this spacing (m).
    pub lamp_spacing_m: Option<f32>,
}

impl std::hash::Hash for SegmentStyle {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.surface.hash(h);
        self.grass_strip.hash(h);
        self.tree_spacing_m.map(f32::to_bits).hash(h);
        self.lamp_spacing_m.map(f32::to_bits).hash(h);
    }
}

/// Default style of a road type (dirt paths are unpaved).
#[must_use]
pub fn default_style(kind: RoadKind) -> SegmentStyle {
    SegmentStyle {
        surface: if kind == RoadKind::Dirt {
            RoadSurface::Dirt
        } else {
            RoadSurface::DarkAsphalt
        },
        ..SegmentStyle::default()
    }
}

/// Validation key of a style that does not fit the road type, if any.
#[must_use]
pub fn style_error(kind: RoadKind, cs: &CrossSection, st: &SegmentStyle) -> Option<&'static str> {
    let dirt_kind = kind == RoadKind::Dirt;
    if (st.surface == RoadSurface::Dirt) != dirt_kind {
        return Some("road-invalid-style-surface");
    }
    let has_sidewalks = cs.elements.iter().any(|e| e.kind == ElementKind::Sidewalk);
    let decorated = st.grass_strip.iter().any(|g| *g)
        || st.tree_spacing_m.is_some()
        || st.lamp_spacing_m.is_some();
    if decorated && !has_sidewalks {
        return Some("road-invalid-style-no-sidewalk");
    }
    let in_range = |v: Option<f32>, (lo, hi): (f32, f32)| v.is_none_or(|x| x >= lo && x <= hi);
    if !in_range(st.tree_spacing_m, TREE_SPACING_M) || !in_range(st.lamp_spacing_m, LAMP_SPACING_M)
    {
        return Some("road-invalid-style-spacing");
    }
    None
}

/// What a decoration is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DecorKind {
    /// Street tree.
    Tree,
    /// Street lamp.
    Lamp,
}

/// One placed decoration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecorItem {
    /// Kind.
    pub kind: DecorKind,
    /// Foot position (on the sidewalk surface).
    pub pos: Vec3,
    /// Centreline arc length it belongs to.
    pub s_m: f32,
    /// Toward the carriageway (unit, plan) — lamps lean their arm this way.
    pub toward_road: Vec2,
}

/// Lateral offsets of decorations on each side (left, right): trees in the grass strip or 0.7 m
/// behind the kerb, lamps 0.4 m behind the kerb.
fn offsets(cs: &CrossSection, kind: DecorKind) -> [f32; 2] {
    let ch = cs.carriageway_half_m();
    let d = match kind {
        DecorKind::Tree => 0.5 * GRASS_STRIP_M + 0.2,
        DecorKind::Lamp => 0.4,
    };
    [ch + d, -(ch + d)]
}

/// Decorations of a segment over `[s0, s1]` (after junction cut-backs), skipping bridge spans
/// for trees: on each side, items exactly `spacing` apart along the offset line (arc length),
/// centred in the range; lamps on the right side are shifted by half a spacing so both sides
/// alternate.
#[must_use]
pub fn decor_items(
    geom: &SegmentGeom,
    profile: &Profile,
    cs: &CrossSection,
    style: &SegmentStyle,
    range: (f32, f32),
    spans: &[(f32, f32)],
) -> Vec<DecorItem> {
    let mut out = Vec::new();
    for (kind, spacing) in [
        (DecorKind::Tree, style.tree_spacing_m),
        (DecorKind::Lamp, style.lamp_spacing_m),
    ] {
        let Some(sp) = spacing else { continue };
        let offs = offsets(cs, kind);
        for (side, off) in offs.iter().enumerate() {
            let shift = if kind == DecorKind::Lamp && side == 1 {
                0.5 * sp
            } else {
                0.0
            };
            place_along(geom, range, *off, sp, shift, &mut |s, p, toward| {
                if kind == DecorKind::Tree && spans.iter().any(|(a, b)| s >= *a && s <= *b) {
                    return;
                }
                out.push(DecorItem {
                    kind,
                    pos: Vec3::new(p.x, profile.height_at(s) + SIDEWALK_LIFT_M, p.y),
                    s_m: s,
                    toward_road: toward,
                });
            });
        }
    }
    out
}

/// Walks the line offset by `off` from the centreline over `range` and calls `emit(s, point,
/// toward_road)` every `sp` metres of offset-line arc length.
fn place_along(
    geom: &SegmentGeom,
    range: (f32, f32),
    off: f32,
    sp: f32,
    shift: f32,
    emit: &mut dyn FnMut(f32, Vec2, Vec2),
) {
    let (s0, s1) = (range.0.max(0.0), range.1.min(geom.length()));
    if s1 - s0 < 2.0 * END_MARGIN_M {
        return;
    }
    // Offset line sampled every 0.5 m of centreline.
    let n = (((s1 - s0) / 0.5).ceil() as usize).max(1);
    let pts: Vec<(f32, Vec2, Vec2)> = (0..=n)
        .map(|k| {
            let s = s0 + (s1 - s0) * k as f32 / n as f32;
            let nrm = geom.dir_at(s).perp();
            (s, geom.point_at(s) + nrm * off, -nrm * off.signum())
        })
        .collect();
    let mut cum = vec![0.0f32; pts.len()];
    for k in 1..pts.len() {
        cum[k] = cum[k - 1] + pts[k].1.distance(pts[k - 1].1);
    }
    let total = cum[pts.len() - 1];
    let usable = total - 2.0 * END_MARGIN_M;
    if usable < 0.0 {
        return;
    }
    let count = (usable / sp).floor() as usize + 1;
    let first = END_MARGIN_M + 0.5 * (usable - (count - 1) as f32 * sp);
    // A shifted row (alternating lamps) starts half a spacing later and drops what overruns.
    let positions = (0..count)
        .map(|k| first + k as f32 * sp + shift)
        .filter(|l| *l <= total - END_MARGIN_M + 1e-3);
    for l in positions {
        let i = cum.partition_point(|c| *c <= l).clamp(1, pts.len() - 1);
        let (c0, c1) = (cum[i - 1], cum[i]);
        let t = if c1 > c0 { (l - c0) / (c1 - c0) } else { 0.0 };
        let s = pts[i - 1].0 + (pts[i].0 - pts[i - 1].0) * t;
        let p = pts[i - 1].1.lerp(pts[i].1, t);
        emit(s, p, pts[i].2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_geometry::CubicBezier2;

    fn styled(tree: Option<f32>, lamp: Option<f32>) -> SegmentStyle {
        SegmentStyle {
            tree_spacing_m: tree,
            lamp_spacing_m: lamp,
            ..SegmentStyle::default()
        }
    }

    fn quarter(r: f32) -> CubicBezier2 {
        let k = 0.552_284_8 * r;
        CubicBezier2::new(
            Vec2::new(r, 0.0),
            Vec2::new(r, k),
            Vec2::new(k, r),
            Vec2::new(0.0, r),
        )
    }

    #[test]
    fn decor_spacing_within_5pct_on_curves() {
        let cs = CrossSection::of(RoadKind::Avenue);
        for (curve, sp) in [
            (quarter(60.0), 12.0),
            (quarter(120.0), 24.0),
            (
                CubicBezier2::straight(Vec2::ZERO, Vec2::new(200.0, 0.0)),
                8.0,
            ),
        ] {
            let g = SegmentGeom::new(curve, cs.half_width_m());
            let p = Profile::flat(g.length(), 10.0);
            let items = decor_items(
                &g,
                &p,
                &cs,
                &styled(Some(sp), None),
                (5.0, g.length() - 5.0),
                &[],
            );
            assert!(items.len() >= 4);
            // Per side, consecutive trees are `sp` apart (chord vs arc ≤ 5 %).
            for side in [1.0f32, -1.0] {
                let row: Vec<&DecorItem> = items
                    .iter()
                    .filter(|it| (g.project(Vec2::new(it.pos.x, it.pos.z)).1 * side) > 0.0)
                    .collect();
                for w in row.windows(2) {
                    let d = Vec2::new(w[1].pos.x - w[0].pos.x, w[1].pos.z - w[0].pos.z).length();
                    assert!((d - sp).abs() / sp <= 0.05, "spacing {d} vs {sp}");
                }
            }
        }
    }

    #[test]
    fn decor_height_matches_profile_1cm() {
        let cs = CrossSection::of(RoadKind::Street);
        let g = SegmentGeom::new(quarter(80.0), cs.half_width_m());
        let p = Profile::new(vec![0.0, g.length()], vec![0.0, 0.08 * g.length()]);
        let items = decor_items(
            &g,
            &p,
            &cs,
            &styled(Some(10.0), Some(30.0)),
            (0.0, g.length()),
            &[],
        );
        assert!(items.iter().any(|i| i.kind == DecorKind::Lamp));
        for it in &items {
            let want = p.height_at(it.s_m) + SIDEWALK_LIFT_M;
            assert!((it.pos.y - want).abs() <= 0.01, "{} vs {want}", it.pos.y);
            // On the sidewalk, behind the kerb.
            let (_, d) = g.project(Vec2::new(it.pos.x, it.pos.z));
            assert!(d.abs() > cs.carriageway_half_m() && d.abs() < cs.half_width_m());
        }
    }

    #[test]
    fn no_trees_on_spans_and_style_validation() {
        let cs = CrossSection::of(RoadKind::Street);
        let g = SegmentGeom::new(
            CubicBezier2::straight(Vec2::ZERO, Vec2::new(200.0, 0.0)),
            6.0,
        );
        let p = Profile::flat(200.0, 0.0);
        let items = decor_items(
            &g,
            &p,
            &cs,
            &styled(Some(10.0), Some(25.0)),
            (0.0, 200.0),
            &[(50.0, 150.0)],
        );
        assert!(
            items
                .iter()
                .filter(|i| i.kind == DecorKind::Tree)
                .all(|i| i.s_m < 50.0 || i.s_m > 150.0)
        );
        assert!(
            items
                .iter()
                .any(|i| i.kind == DecorKind::Lamp && i.s_m > 50.0 && i.s_m < 150.0)
        );
        let dirt = CrossSection::of(RoadKind::Dirt);
        assert_eq!(
            style_error(RoadKind::Dirt, &dirt, &default_style(RoadKind::Dirt)),
            None
        );
        assert_eq!(
            style_error(RoadKind::Dirt, &dirt, &styled(Some(10.0), None)),
            Some("road-invalid-style-surface")
        );
        assert_eq!(
            style_error(RoadKind::Street, &cs, &styled(Some(30.0), None)),
            Some("road-invalid-style-spacing")
        );
    }
}
