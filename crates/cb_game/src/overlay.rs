//! Zoning view (ART_DIRECTION §6): plot fills at 35 % in the zone colour, a plot outline and
//! the lot subdivision lines (#F4EFE6 at 60 %), draped on the terrain. Shown while the zoning
//! view is toggled on or the plot tool is active. One mesh per plot, rebuilt from the sim's
//! `ZoningChanged` messages and from terrain changes under the plot.
//!
//! Zone patterns (C stripes, I cross-hatch) arrive with the commercial and industrial zones (M4).

use std::collections::BTreeMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use cb_core::ids::PlotId;
use cb_sim::catalog::ZoneKind;
use cb_sim::ids::Ids;
use cb_sim::terrain::{TerrainChanged, TerrainRes};
use cb_sim::zoning::{Lot, Plot, ZoningChanged};

use crate::tools::{ActiveTool, ZoningView};

/// Height of the overlay above the ground (m).
pub const DRAPE_LIFT_M: f32 = 0.12;
/// Longest fill triangle edge after draping subdivision (m).
pub const DRAPE_EDGE_M: f32 = 6.0;
/// Plot outline width (m).
pub const OUTLINE_W_M: f32 = 0.6;
/// Lot line width (m).
pub const LOT_LINE_W_M: f32 = 0.3;

/// Zone fill colour (sRGB) — ART_DIRECTION §6.
#[must_use]
pub fn zone_color(z: ZoneKind) -> [u8; 3] {
    match z {
        ZoneKind::Residential => [0x77, 0xB3, 0x6B],
        ZoneKind::Commercial => [0x5E, 0x8F, 0xC9],
        ZoneKind::Industrial => [0xD3, 0xB0, 0x4C],
    }
}

/// Draped overlay geometry (positions + linear RGBA).
#[derive(Default, Debug, Clone)]
pub struct DrapeMesh {
    /// Positions.
    pub positions: Vec<[f32; 3]>,
    /// Linear RGBA.
    pub colors: Vec<[f32; 4]>,
    /// Triangles.
    pub indices: Vec<u32>,
}

impl DrapeMesh {
    fn vertex(&mut self, p: Vec2, h: &dyn Fn(Vec2) -> f32, lift: f32, c: [f32; 4]) -> u32 {
        let i = self.positions.len() as u32;
        self.positions.push([p.x, h(p) + lift, p.y]);
        self.colors.push(c);
        i
    }

    /// Fills a simple polygon, subdividing triangles so the fill follows the terrain.
    pub fn fill(&mut self, poly: &[Vec2], h: &dyn Fn(Vec2) -> f32, c: [f32; 4]) {
        for t in cb_geometry::triangulate::triangulate(poly) {
            let [a, b, d] = t.map(|i| poly[i as usize]);
            self.tri(a, b, d, h, c, 0);
        }
    }

    fn tri(&mut self, a: Vec2, b: Vec2, d: Vec2, h: &dyn Fn(Vec2) -> f32, c: [f32; 4], depth: u32) {
        let (ab, bd, da) = (a.distance(b), b.distance(d), d.distance(a));
        let longest = ab.max(bd).max(da);
        if longest > DRAPE_EDGE_M && depth < 10 {
            // Split the longest edge (keeps triangles well shaped).
            if ab >= bd && ab >= da {
                let m = a.midpoint(b);
                self.tri(a, m, d, h, c, depth + 1);
                self.tri(m, b, d, h, c, depth + 1);
            } else if bd >= da {
                let m = b.midpoint(d);
                self.tri(a, b, m, h, c, depth + 1);
                self.tri(a, m, d, h, c, depth + 1);
            } else {
                let m = d.midpoint(a);
                self.tri(a, b, m, h, c, depth + 1);
                self.tri(m, b, d, h, c, depth + 1);
            }
            return;
        }
        let i = [a, b, d].map(|p| self.vertex(p, h, DRAPE_LIFT_M, c));
        self.indices.extend_from_slice(&i);
    }

    /// A closed outline of width `w` (inset by `w / 2` is not needed at these widths).
    pub fn outline(&mut self, poly: &[Vec2], w: f32, h: &dyn Fn(Vec2) -> f32, c: [f32; 4]) {
        let n = poly.len();
        for k in 0..n {
            self.line(poly[k], poly[(k + 1) % n], w, h, c);
        }
    }

    /// A draped line strip piece from `a` to `b`, split every few metres.
    pub fn line(&mut self, a: Vec2, b: Vec2, w: f32, h: &dyn Fn(Vec2) -> f32, c: [f32; 4]) {
        let len = a.distance(b);
        if len < 1e-3 {
            return;
        }
        let dir = (b - a) / len;
        let nrm = dir.perp() * (0.5 * w);
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        let lift = DRAPE_LIFT_M + 0.03;
        let mut prev: Option<[u32; 2]> = None;
        for s in 0..=steps {
            let p = a.lerp(b, s as f32 / steps as f32);
            let l = self.vertex(p + nrm, h, lift, c);
            let r = self.vertex(p - nrm, h, lift, c);
            if let Some([pl, pr]) = prev {
                self.indices.extend_from_slice(&[pl, pr, r, pl, r, l]);
            }
            prev = Some([l, r]);
        }
    }

    /// As a Bevy mesh (normals up, unlit material).
    #[must_use]
    pub fn into_mesh(self) -> Mesh {
        let normals = vec![[0.0, 1.0, 0.0]; self.positions.len()];
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

fn rgba(c: [u8; 3], a: f32) -> [f32; 4] {
    let l = Color::srgb_u8(c[0], c[1], c[2]).to_linear();
    [l.red, l.green, l.blue, a]
}

/// The overlay of one plot: fill, outline, and the lot lines between its lots.
#[must_use]
pub fn plot_overlay(plot: &Plot, lots: &[&Lot], h: &dyn Fn(Vec2) -> f32) -> DrapeMesh {
    let mut m = DrapeMesh::default();
    let zc = zone_color(plot.zone());
    m.fill(plot.polygon(), h, rgba(zc, 0.35));
    let line = rgba([0xF4, 0xEF, 0xE6], 0.6);
    for l in lots {
        m.outline(l.polygon(), LOT_LINE_W_M, h, line);
    }
    m.outline(plot.polygon(), OUTLINE_W_M, h, rgba(zc, 0.9));
    m
}

/// Overlay registry.
#[derive(Resource, Default)]
pub struct OverlayState {
    plots: BTreeMap<PlotId, Entity>,
    material: Option<Handle<StandardMaterial>>,
}

impl OverlayState {
    /// Plots with an overlay mesh.
    #[must_use]
    pub fn plot_count(&self) -> usize {
        self.plots.len()
    }
}

/// Overlay plugin.
pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayState>().add_systems(
            Update,
            (sync_plot_overlays, overlay_visibility)
                .chain()
                .in_set(crate::app::GameSet::ViewSync),
        );
    }
}

/// Marker of zoning overlay meshes.
#[derive(Component)]
pub struct ZoningOverlay;

#[allow(clippy::too_many_arguments, reason = "Bevy system parameters")]
fn sync_plot_overlays(
    mut commands: Commands,
    mut zoning: MessageReader<ZoningChanged>,
    mut terrain_changed: MessageReader<TerrainChanged>,
    ids: Option<Res<Ids>>,
    plots: Query<&Plot>,
    lots: Query<&Lot>,
    terrain: Option<Res<TerrainRes>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut state: ResMut<OverlayState>,
) {
    let Some(ids) = ids else { return };
    let mut dirty: Vec<PlotId> = Vec::new();
    for m in zoning.read() {
        match *m {
            ZoningChanged::Plot(id) | ZoningChanged::PlotRemoved(id) => dirty.push(id),
            ZoningChanged::Lot(_) | ZoningChanged::LotRemoved(_) => {}
        }
    }
    if let Some(t) = &terrain {
        let h = t.terrain().height();
        let half = 0.5 * (h.n() - 1) as f32 * h.spacing_m();
        let world = |i: usize| -half + i as f32 * h.spacing_m();
        for msg in terrain_changed.read() {
            let r = msg.rect;
            let (lo, hi) = (
                Vec2::new(world(r.i0), world(r.j0)),
                Vec2::new(world(r.i1), world(r.j1)),
            );
            for (id, e) in ids.plots.iter() {
                if let Ok(p) = plots.get(e)
                    && p.polygon().iter().any(|v| {
                        v.x >= lo.x - 2.0
                            && v.x <= hi.x + 2.0
                            && v.y >= lo.y - 2.0
                            && v.y <= hi.y + 2.0
                    })
                {
                    dirty.push(id);
                }
            }
        }
    }
    if dirty.is_empty() {
        return;
    }
    dirty.sort_unstable();
    dirty.dedup();
    tracing::debug!("zoning overlay: rebuilding {} plots", dirty.len());
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                fog_enabled: false,
                depth_bias: 20.0,
                // Draped geometry is wound counter-clockwise in (x, z), which faces down.
                cull_mode: None,
                ..default()
            })
        })
        .clone();
    let ground = |p: Vec2| terrain.as_ref().map_or(0.0, |t| t.terrain().height_at(p));
    for id in dirty {
        if let Some(e) = state.plots.remove(&id) {
            commands.entity(e).despawn();
        }
        let Some(plot) = ids.plots.get(id).and_then(|e| plots.get(e).ok()) else {
            continue;
        };
        let plot_lots: Vec<&Lot> = plot
            .lots()
            .iter()
            .filter_map(|l| ids.lots.get(*l).and_then(|e| lots.get(e).ok()))
            .collect();
        let mesh = plot_overlay(plot, &plot_lots, &ground).into_mesh();
        let e = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                Visibility::Hidden,
                NotShadowCaster,
                ZoningOverlay,
            ))
            .id();
        state.plots.insert(id, e);
    }
}

fn overlay_visibility(
    view: Res<ZoningView>,
    tool: Res<ActiveTool>,
    mut q: Query<&mut Visibility, With<ZoningOverlay>>,
) {
    let on = view.0 || matches!(*tool, ActiveTool::Plot(_));
    let want = if on {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut q {
        if v.set_if_neq(want) {
            tracing::debug!("zoning overlay visibility {want:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drape_fill_subdivides_and_follows_ground() {
        let poly = [
            Vec2::new(0.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(60.0, 30.0),
            Vec2::new(0.0, 30.0),
        ];
        let h = |p: Vec2| 0.1 * p.x;
        let mut m = DrapeMesh::default();
        m.fill(&poly, &h, [1.0; 4]);
        assert!(m.indices.len() / 3 >= 2 * 10 * 5, "{}", m.indices.len() / 3);
        for t in m.indices.chunks_exact(3) {
            let p = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
            for k in 0..3 {
                let (a, b) = (p[k], p[(k + 1) % 3]);
                let d = Vec2::new(a[0] - b[0], a[2] - b[2]).length();
                assert!(d <= DRAPE_EDGE_M + 1e-3, "edge {d}");
            }
            for v in p {
                assert!((v[1] - (0.1 * v[0] + DRAPE_LIFT_M)).abs() < 1e-4);
            }
        }
        let area: f32 = m
            .indices
            .chunks_exact(3)
            .map(|t| {
                let p = [t[0], t[1], t[2]]
                    .map(|i| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]));
                0.5 * (p[1] - p[0]).perp_dot(p[2] - p[0]).abs()
            })
            .sum();
        assert!((area - 1800.0).abs() < 0.5, "{area}");
    }

    #[test]
    fn drape_line_degenerate_is_empty() {
        let mut m = DrapeMesh::default();
        m.line(Vec2::ONE, Vec2::ONE, 0.3, &|_| 0.0, [1.0; 4]);
        assert!(m.positions.is_empty() && m.indices.is_empty());
    }
}
