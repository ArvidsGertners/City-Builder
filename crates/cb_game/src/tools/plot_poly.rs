//! Freehand plot editing (ALGORITHMS §9, M4): polygon mode (click vertices; edges within 1.5 m
//! of a road edge snap and become frontage, done by the sim) and outline editing of a selected
//! plot by vertex and edge drags. Frontage-drag plots get an outline from their polygon on the
//! first such edit.

use bevy::prelude::*;

use super::{HANDLE_PICK_M, PlotDrag};

/// Outline vertices of a plot: the player's outline, or its polygon simplified to 0.5 m.
#[must_use]
pub fn outline_of(plot: &cb_sim::zoning::Plot) -> Vec<Vec2> {
    if !plot.outline().is_empty() {
        return plot.outline().to_vec();
    }
    let mut ring = plot.polygon().to_vec();
    if let Some(f) = ring.first().copied() {
        ring.push(f);
    }
    let mut s = cb_geometry::polyline::douglas_peucker(&ring, 0.5);
    if s.len() > 1 && s[0].distance(s[s.len() - 1]) < 1e-3 {
        s.pop();
    }
    s
}

/// The handle under `p`: a vertex, else an edge midpoint.
#[must_use]
pub fn pick(outline: &[Vec2], p: Vec2) -> Option<PlotDrag> {
    let n = outline.len();
    if let Some(k) = (0..n)
        .filter(|k| outline[*k].distance(p) < HANDLE_PICK_M)
        .min_by(|a, b| outline[*a].distance(p).total_cmp(&outline[*b].distance(p)))
    {
        return Some(PlotDrag::Vertex { k, to: p });
    }
    (0..n)
        .filter(|k| ((outline[*k] + outline[(k + 1) % n]) * 0.5).distance(p) < HANDLE_PICK_M)
        .min_by(|a, b| {
            let m = |k: usize| (outline[k] + outline[(k + 1) % n]) * 0.5;
            m(*a).distance(p).total_cmp(&m(*b).distance(p))
        })
        .map(|k| PlotDrag::Edge { k, grab: p, to: p })
}

/// The outline after a vertex or edge drag.
#[must_use]
pub fn apply(outline: &[Vec2], drag: PlotDrag) -> Vec<Vec2> {
    let mut v = outline.to_vec();
    let n = v.len();
    match drag {
        PlotDrag::Vertex { k, to } if k < n => v[k] = to,
        PlotDrag::Edge { k, grab, to } if k < n => {
            let d = to - grab;
            v[k] += d;
            v[(k + 1) % n] += d;
        }
        _ => {}
    }
    v
}

/// Vertex and edge-midpoint handles of an outline, and the polygon-mode stroke.
pub fn gizmos(
    gizmos: &mut Gizmos,
    outline: &[Vec2],
    drawing: &[Vec2],
    cursor: Option<Vec2>,
    p3: &dyn Fn(Vec2) -> Vec3,
) {
    let cream = Color::srgb_u8(0xF4, 0xEF, 0xE6);
    let n = outline.len();
    for k in 0..n {
        gizmos.sphere(Isometry3d::from_translation(p3(outline[k])), 1.4, cream);
        let m = (outline[k] + outline[(k + 1) % n]) * 0.5;
        gizmos.sphere(
            Isometry3d::from_translation(p3(m)),
            0.8,
            cream.with_alpha(0.7),
        );
    }
    if !drawing.is_empty() {
        let mut pts: Vec<Vec3> = drawing.iter().map(|q| p3(*q)).collect();
        if let Some(c) = cursor {
            pts.push(p3(c));
        }
        gizmos.linestrip(pts, Color::srgb(0.31, 0.70, 0.69));
        for q in drawing {
            gizmos.sphere(Isometry3d::from_translation(p3(*q)), 1.0, cream);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_prefers_vertices_and_edge_drag_moves_both_ends() {
        let o = vec![
            Vec2::ZERO,
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 30.0),
            Vec2::new(0.0, 30.0),
        ];
        assert_eq!(
            pick(&o, Vec2::new(39.0, 29.0)),
            Some(PlotDrag::Vertex {
                k: 2,
                to: Vec2::new(39.0, 29.0)
            })
        );
        let Some(d @ PlotDrag::Edge { k: 2, .. }) = pick(&o, Vec2::new(20.0, 31.0)) else {
            panic!("back edge midpoint")
        };
        let PlotDrag::Edge { grab, .. } = d else {
            unreachable!()
        };
        let moved = apply(
            &o,
            PlotDrag::Edge {
                k: 2,
                grab,
                to: grab + Vec2::Y * 8.0,
            },
        );
        assert_eq!(moved[2], Vec2::new(40.0, 38.0));
        assert_eq!(moved[3], Vec2::new(0.0, 38.0));
        assert_eq!(moved[0], o[0]);
        assert!(pick(&o, Vec2::new(20.0, 15.0)).is_none());
    }
}
