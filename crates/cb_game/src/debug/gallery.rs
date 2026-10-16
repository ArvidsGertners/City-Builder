//! `--procgen-gallery` (ALGORITHMS §11): a flat site with rows of lots, every building generated
//! straight from `cb_procgen` without the simulation. Row 0 walks the residential typologies
//! L1 → L5 along one frontage; then one row per zone and level (R L1–L5, C L1–L3, I L1–L3) on
//! alternately straight and curved frontages. In front: every vehicle type, then the civic set A
//! lineup. Debug tooling: labels are English only.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use cb_geometry::d2;
use cb_geometry::frontage::{Frontage, Prim};
use cb_geometry::rect_fit::{self, FitParams, RectSpec};
use cb_geometry::subdivide::{ZoneParams, subdivide};
use cb_procgen::building::civic::{self, CivicKind};
use cb_procgen::building::zoned::{self, LotShape, Massing, Zone};
use cb_procgen::paint::Painter;
use cb_procgen::yard::{self, YardLot};
use cb_sim::terrain::TerrainRes;

use crate::camera::CameraRig;
use crate::render::{WorldMaterials, world_mesh};
use crate::scene::{FeatureAnchor, NoDemoTown, SceneAnchor, SceneFeatures};

/// Depth of the civic lineup row including its street and gap (m).
const CIVIC_ROW_DEPTH_M: f64 = 110.0;

/// Depth of the vehicle row including its gap (m).
const VEHICLE_ROW_DEPTH_M: f64 = 30.0;

/// Gallery plugin (replaces the demo town).
pub struct GalleryPlugin;

impl Plugin for GalleryPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(NoDemoTown)
            .add_systems(Startup, build_gallery.after(crate::scene::demo_town))
            .add_systems(Update, draw_lots)
            .add_systems(EguiPrimaryContextPass, legend);
    }
}

/// Built gallery: row labels and lot outlines.
#[derive(Resource, Default)]
pub struct Gallery {
    /// Row labels, front to back.
    pub rows: Vec<String>,
    /// Lot outlines with ground height.
    pub lots: Vec<Vec<Vec3>>,
    /// Buildings generated.
    pub buildings: usize,
}

struct Row {
    label: String,
    zone: Zone,
    params: ZoneParams,
    fit: FitParams,
    levels: Vec<u8>,
    length: f64,
    depth: f64,
    curved: bool,
}

fn rows() -> Vec<Row> {
    let fit = |zone: Zone, zp: ZoneParams| FitParams {
        setback: zp.setback,
        rect: match zone {
            Zone::Residential => Some(RectSpec::HOUSE),
            Zone::Commercial => None,
            Zone::Industrial => Some(RectSpec::HALL),
        },
    };
    // Wider residential lots so L4–L5 blocks fit next to the houses.
    let wide = ZoneParams {
        w_min: 16.0,
        w_target: 20.0,
        w_max: 24.0,
        ..ZoneParams::RESIDENTIAL
    };
    let mut v = vec![Row {
        label: "R L1 → L5 progression".to_owned(),
        zone: Zone::Residential,
        params: wide,
        fit: fit(Zone::Residential, wide),
        levels: vec![1, 2, 3, 4, 5],
        length: 200.0,
        depth: 40.0,
        curved: false,
    }];
    let mut k = 0;
    for (zone, zp, max, depth, length) in [
        (Zone::Residential, wide, 5u8, 40.0, 160.0),
        (Zone::Commercial, ZoneParams::COMMERCIAL, 3, 40.0, 200.0),
        (Zone::Industrial, ZoneParams::INDUSTRIAL, 3, 70.0, 280.0),
    ] {
        for level in 1..=max {
            let tag = match zone {
                Zone::Residential => "R",
                Zone::Commercial => "C",
                Zone::Industrial => "I",
            };
            v.push(Row {
                label: format!("{tag} L{level}"),
                zone,
                params: zp,
                fit: fit(zone, zp),
                levels: vec![level],
                length,
                depth,
                curved: k % 2 == 1,
            });
            k += 1;
        }
    }
    v
}

/// Builds every row on a flat site and points the camera at it.
fn build_gallery(world: &mut World) {
    let Some(terrain) = world
        .get_resource::<TerrainRes>()
        .map(|t| t.terrain().clone())
    else {
        return;
    };
    let Some(mats) = world.get_resource::<WorldMaterials>().cloned() else {
        return;
    };
    let site = crate::scene::find_site(&terrain, 900.0);
    let ground = |p: Vec2| terrain.height_at(p);
    let rows = rows();
    let mut gallery = Gallery::default();
    let mut meshes: Vec<cb_geometry::MeshData> = Vec::new();
    let mut features = SceneFeatures::default();
    let mut exclusion = crate::view::ScatterExclusion::default();
    let total_depth: f64 =
        rows.iter().map(|r| r.depth + 30.0).sum::<f64>() + CIVIC_ROW_DEPTH_M + VEHICLE_ROW_DEPTH_M;
    let mut z = -0.5 * total_depth;
    // Every vehicle type parked nose to tail along a short street, front row.
    {
        use cb_procgen::vehicle::{self, VehicleKind};
        let gap = 3.0;
        let total: f32 = VehicleKind::ALL
            .iter()
            .map(|k| k.length_m() + gap)
            .sum::<f32>()
            - gap;
        let lane_z = site.y + z as f32 + 8.0;
        let line = [
            Vec2::new(site.x - 0.5 * total - 8.0, lane_z),
            Vec2::new(site.x + 0.5 * total + 8.0, lane_z),
        ];
        let mut road = Painter::new(0, 3);
        road.ground_strip(
            &line,
            8.0,
            0.05,
            cb_procgen::palette::ASPHALT,
            cb_procgen::surface::Surface::Paved,
            &ground,
        );
        meshes.push(road.finish());
        let mut u = site.x - 0.5 * total;
        for (i, kind) in VehicleKind::ALL.into_iter().enumerate() {
            let c = Vec2::new(u + 0.5 * kind.length_m(), lane_z);
            let base = Vec3::new(c.x, ground(c) + 0.06, c.y);
            meshes.extend(vehicle::meshes(kind, base, 0.0, 31 + i as u64, 0));
            u += kind.length_m() + gap;
        }
        exclusion.0.push(cb_geometry::polygon::convex_hull(&[
            line[0] - Vec2::Y * 6.0,
            line[1] - Vec2::Y * 6.0,
            line[1] + Vec2::Y * 6.0,
            line[0] + Vec2::Y * 6.0,
        ]));
        let c = Vec2::new(site.x, lane_z);
        features.0.insert(
            "gallery_vehicles".to_owned(),
            FeatureAnchor {
                pos: Vec3::new(c.x, ground(c), c.y),
                yaw_deg: 0.0,
                dir: Vec2::X,
            },
        );
        gallery.rows.push("Vehicles".to_owned());
    }
    z += VEHICLE_ROW_DEPTH_M;
    // Civic lineup (generator set A) along one straight street.
    {
        let gap = 8.0;
        let total: f32 = CivicKind::ALL
            .iter()
            .map(|k| k.parcel_m().0 + gap)
            .sum::<f32>()
            - gap;
        let front_z = site.y + z as f32 + 6.0;
        let mut u = site.x - 0.5 * total;
        let line = [
            Vec2::new(u - 10.0, front_z - 6.0),
            Vec2::new(u + total + 10.0, front_z - 6.0),
        ];
        let mut road = Painter::new(0, 2);
        road.ground_strip(
            &line,
            10.0,
            0.05,
            cb_procgen::palette::ASPHALT,
            cb_procgen::surface::Surface::Paved,
            &ground,
        );
        meshes.push(road.finish());
        let mut hull = vec![line[0] - Vec2::Y * 6.0, line[1] - Vec2::Y * 6.0];
        for (i, kind) in CivicKind::ALL.into_iter().enumerate() {
            let (w, d) = kind.parcel_m();
            let fr = civic::frame_at(kind, Vec2::new(u + 0.5 * w, front_z), Vec2::X);
            let l = civic::layout(kind, fr, 1 + i as u64);
            let parcel = l.parcel();
            hull.extend(parcel.iter().copied());
            gallery.lots.push(
                parcel
                    .iter()
                    .map(|p| Vec3::new(p.x, ground(*p) + 0.2, p.y))
                    .collect(),
            );
            if let Some(f) = l.plan_foundation(&ground) {
                meshes.push(civic::mesh(&l, &f, 0).0);
                gallery.buildings += 1;
            } else {
                tracing::warn!("gallery: {kind:?} parcel too steep");
            }
            let c = fr.at(0.5 * w, 0.5 * d);
            features.0.insert(
                format!("civic_{i}"),
                FeatureAnchor {
                    pos: Vec3::new(c.x, ground(c), c.y),
                    yaw_deg: 0.0,
                    dir: Vec2::X,
                },
            );
            u += w + gap;
        }
        exclusion.0.push(cb_geometry::polygon::convex_hull(&hull));
        let c = Vec2::new(site.x, front_z + 30.0);
        features.0.insert(
            "gallery_civic".to_owned(),
            FeatureAnchor {
                pos: Vec3::new(c.x, ground(c), c.y),
                yaw_deg: 0.0,
                dir: Vec2::X,
            },
        );
        gallery.rows.push("Civic set A".to_owned());
    }
    z += CIVIC_ROW_DEPTH_M;
    for (ri, row) in rows.iter().enumerate() {
        let prims = if row.curved {
            vec![Prim::Arc(320.0, row.length / 320.0)]
        } else {
            vec![Prim::Line(row.length)]
        };
        let Some(f) = Frontage::from_prims(&prims, row.depth) else {
            continue;
        };
        let Some(plot) = f.plot_polygon(row.depth, 0.0) else {
            continue;
        };
        // Row origin: frontage start, centred along x.
        let origin = site + Vec2::new(-0.5 * row.length as f32, z as f32);
        let to_w = |p: bevy::math::DVec2| origin + p.as_vec2();
        let out = subdivide(&f, &plot, &row.params, row.depth, 0, ri as u64 + 7, None);
        let fits = rect_fit::fit_lots(&out.lots, &f, &row.fit);
        let built_before = gallery.buildings;
        // A street strip along the frontage, on the side away from the lots.
        let mid = 0.5 * f.len();
        let lot_side = out.lots.first().map_or(1.0, |l| {
            let c = d2::to_f32(&l.poly).iter().copied().sum::<Vec2>() / l.poly.len() as f32;
            let t = f.tangent_at(mid).as_vec2();
            let p = f.point_at(mid).as_vec2();
            if t.perp().dot(c - p) > 0.0 { 1.0 } else { -1.0 }
        });
        let line: Vec<Vec2> = (0..=160)
            .map(|k| {
                let s = f.len() * f64::from(k) / 160.0;
                let n = f.tangent_at(s).as_vec2().perp() * -lot_side;
                to_w(f.point_at(s)) + n * 6.0
            })
            .collect();
        let mut road = Painter::new(0, 1);
        road.ground_strip(
            &line,
            10.0,
            0.05,
            cb_procgen::palette::ASPHALT,
            cb_procgen::surface::Surface::Paved,
            &ground,
        );
        meshes.push(road.finish());
        // No natural trees on the row: hull of street and lots.
        let mut row_pts: Vec<Vec2> = line
            .iter()
            .flat_map(|p| [*p + Vec2::splat(6.0), *p - Vec2::splat(6.0)])
            .collect();
        row_pts.extend(plot.iter().map(|p| to_w(*p)));
        exclusion
            .0
            .push(cb_geometry::polygon::convex_hull(&row_pts));
        for (k, (lot, fit)) in out.lots.iter().zip(&fits).enumerate() {
            let poly: Vec<Vec2> = lot.poly.iter().map(|p| to_w(*p)).collect();
            gallery.lots.push(
                poly.iter()
                    .map(|p| Vec3::new(p.x, ground(*p) + 0.2, p.y))
                    .collect(),
            );
            let Some(ins) = fit.inset.as_ref() else {
                continue;
            };
            let inset: Vec<Vec2> = ins.iter().map(|p| to_w(*p)).collect();
            let fp = fit.footprint.map(|q| q.map(to_w));
            let shape = LotShape {
                polygon: &poly,
                inset: &inset,
                front: (to_w(f.point_at(lot.s0)), to_w(f.point_at(lot.s1))),
                footprint: fp,
            };
            let level = row.levels[k % row.levels.len()];
            // Fall back to lower levels where a lot is too small (as growth does).
            let seed = (ri as u64) << 32 | k as u64;
            let built = (1..=level).rev().find_map(|lv| {
                let m = zoned::layout(row.zone, lv, &shape, seed)?;
                let fd = m.plan_foundation(&ground)?;
                Some((m, fd))
            });
            let Some((m, fd)) = built else { continue };
            let (mut lod0, _) = m.mesh(&fd, &ground, 0);
            if let Massing::House(h) = &m {
                let edges = rect_fit::lot_edge_kinds(lot, &f);
                let yl = YardLot {
                    polygon: &poly,
                    edges: &edges,
                    front_start: shape.front.0,
                    front_end: shape.front.1,
                };
                lod0.append(&yard::yard(h, &yl, fd.floor_y, &ground, 0, seed ^ 0xA5D));
            }
            meshes.push(lod0);
            gallery.buildings += 1;
        }
        let anchor = origin + Vec2::new(0.5 * row.length as f32, 0.5 * row.depth as f32);
        let name = format!("gallery_{ri}");
        features.0.insert(
            name,
            FeatureAnchor {
                pos: Vec3::new(anchor.x, ground(anchor), anchor.y),
                yaw_deg: 0.0,
                dir: Vec2::X,
            },
        );
        tracing::info!(
            "gallery row {ri} ({}): {} lots, {} buildings",
            row.label,
            out.lots.len(),
            gallery.buildings - built_before
        );
        gallery.rows.push(row.label.clone());
        z += row.depth + 30.0;
    }
    let y = ground(site);
    features.0.insert(
        "gallery".to_owned(),
        FeatureAnchor {
            pos: Vec3::new(site.x, y, site.y),
            yaw_deg: 0.0,
            dir: Vec2::X,
        },
    );
    tracing::info!(
        "procgen gallery: {} rows, {} buildings",
        gallery.rows.len(),
        gallery.buildings
    );
    let handles: Vec<Handle<Mesh>> = {
        let mut assets = world.resource_mut::<Assets<Mesh>>();
        meshes.iter().map(|m| assets.add(world_mesh(m))).collect()
    };
    for h in handles {
        world.spawn((
            Mesh3d(h),
            MeshMaterial3d(mats.world.clone()),
            Transform::default(),
        ));
    }
    world.insert_resource(features);
    world.insert_resource(exclusion);
    world.insert_resource(SceneAnchor(Vec3::new(site.x, y, site.y)));
    world.insert_resource(gallery);
    if let Some(mut rig) = world.get_resource_mut::<CameraRig>() {
        *rig = CameraRig {
            focus: Vec3::new(site.x, y, site.y),
            distance: 900.0,
            pitch_deg: 55.0,
            yaw_deg: 0.0,
        };
    }
}

fn draw_lots(mut gizmos: Gizmos, gallery: Option<Res<Gallery>>) {
    let Some(g) = gallery else { return };
    for lot in &g.lots {
        let mut pts = lot.clone();
        if let Some(f) = pts.first().copied() {
            pts.push(f);
        }
        gizmos.linestrip(pts, Color::srgba(0.95, 0.94, 0.9, 0.6));
    }
}

fn legend(
    mut contexts: EguiContexts,
    gallery: Option<Res<Gallery>>,
    visible: Res<crate::tools::UiVisible>,
) {
    let (Some(g), true) = (gallery, visible.0) else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else { return };
    egui::Area::new(egui::Id::new("gallery-legend"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 12.0))
        .show(ctx, |ui| {
            egui::Frame::window(ui.style()).show(ui, |ui| {
                ui.strong(format!("Procgen gallery: {} buildings", g.buildings));
                for (i, r) in g.rows.iter().enumerate() {
                    ui.label(format!("row {i}: {r}"));
                }
            });
        });
}
