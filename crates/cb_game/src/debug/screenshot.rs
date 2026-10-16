//! `--screenshot-suite <M> --out <dir>`: renders the suite's shots as PNG, writes
//! `metrics.json`, then exits (ADR-0058; driven by `scripts/screenshot.sh`).

use std::collections::BTreeMap;
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use serde::Deserialize;
use tracing::{error, info};

use super::screenshot_metrics::{ShotMetrics, metrics_rgba8, to_json};
use crate::camera::{CameraRig, CameraScripted};
use crate::picking::ScriptedCursor;
use crate::render::lighting::LightingState;
use crate::scene::{CameraPose, SceneAnchor, SceneFeatures};
use crate::tools::{ActiveTool, PlotTool, RoadTool, UiVisible, ZoningView};
use crate::view::ViewBacklog;

/// A scripted tool state for a shot.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum ShotTool {
    /// Road tool dragging straight up the `steep` feature (invalid ghost, reason and cost).
    RoadSteep,
    /// Plot tool with the plot at the `corner` feature selected (handles, ± panel).
    PlotEdit,
    /// Bulldozer with the dialog open for the street nearest the `slope` feature (dependent
    /// plots highlighted).
    BulldozeDialog,
    /// Road edit tool upgrading the through street of the `y30` feature to an avenue (segment
    /// highlight, cost label, mode bar).
    EditUpgrade,
    /// Road tool placing a 35 m roundabout preset beside the `y30` feature (ring ghost, radius
    /// and cost label).
    RoundaboutPreset,
}

/// One shot.
#[derive(Clone, Debug, Deserialize)]
pub struct Shot {
    /// File stem.
    pub name: String,
    /// Camera (relative to the feature anchor `at`, or the town; yaw relative to the anchor).
    pub camera: CameraPose,
    /// Feature anchor (`SceneFeatures`); the town anchor when absent.
    #[serde(default)]
    pub at: Option<String>,
    /// Lighting hour (noon when absent).
    #[serde(default)]
    pub hour: Option<f32>,
    /// Draw the game UI.
    #[serde(default)]
    pub ui: bool,
    /// Zoning view on.
    #[serde(default)]
    pub zoning: bool,
    /// Scripted tool state.
    #[serde(default)]
    pub tool: Option<ShotTool>,
    /// Render with a magenta background and no sky dome; the `holes` metric must be 0.
    #[serde(default)]
    pub crack_check: bool,
    /// Lane graph debug overlay on.
    #[serde(default)]
    pub lanes: bool,
}

/// A suite.
#[derive(Clone, Debug, Deserialize)]
pub struct Suite {
    /// Window size in logical pixels.
    pub resolution: (u32, u32),
    /// Frames to wait after moving the camera.
    pub settle_frames: u32,
    /// World seed (unless `--seed` is given).
    #[serde(default)]
    pub seed: Option<u64>,
    /// Shots in order.
    pub shots: Vec<Shot>,
}

#[derive(Deserialize)]
struct SuiteFile {
    suites: BTreeMap<String, Suite>,
}

/// Loads a suite from the embedded `screenshot_suites.ron`.
pub fn load_suite(name: &str) -> Result<Suite, String> {
    let file: SuiteFile = ron::from_str(include_str!("screenshot_suites.ron"))
        .map_err(|e| format!("screenshot_suites.ron: {e}"))?;
    file.suites.get(name).cloned().ok_or_else(|| {
        format!(
            "unknown suite {name:?}; known: {:?}",
            file.suites.keys().collect::<Vec<_>>()
        )
    })
}

/// Runner state.
#[derive(Resource, Debug)]
pub struct ScreenshotRunner {
    suite: Suite,
    out: PathBuf,
    index: usize,
    waited: u32,
    pending: bool,
    results: Vec<(String, ShotMetrics)>,
    exit_in: Option<u32>,
    saved_clear: Option<Color>,
}

/// Screenshot plugin.
pub struct ScreenshotPlugin {
    /// Suite to render.
    pub suite: Suite,
    /// Output directory.
    pub out: PathBuf,
}

impl Plugin for ScreenshotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(ScreenshotRunner {
            suite: self.suite.clone(),
            out: self.out.clone(),
            index: 0,
            waited: 0,
            pending: false,
            results: Vec::new(),
            exit_in: None,
            saved_clear: None,
        })
        .add_systems(Last, drive);
    }
}

/// Shot state applied when a shot starts.
#[derive(bevy::ecs::system::SystemParam)]
struct ShotState<'w, 's> {
    lighting: ResMut<'w, LightingState>,
    ui: ResMut<'w, UiVisible>,
    zoning: ResMut<'w, ZoningView>,
    lanes: ResMut<'w, crate::debug::lane_overlay::LaneOverlay>,
    tool: ResMut<'w, ActiveTool>,
    cursor: ResMut<'w, ScriptedCursor>,
    features: Option<Res<'w, SceneFeatures>>,
    ids: Option<Res<'w, cb_sim::ids::Ids>>,
    plots: Query<'w, 's, &'static cb_sim::zoning::Plot>,
    lots: Query<'w, 's, &'static cb_sim::zoning::Lot>,
    segs: Query<'w, 's, &'static cb_sim::roads::RoadSegment>,
    dialog: ResMut<'w, crate::tools::BulldozeDialog>,
    terrain: Option<Res<'w, cb_sim::terrain::TerrainRes>>,
    clear: ResMut<'w, ClearColor>,
    dome: Query<'w, 's, &'static mut Visibility, With<crate::render::lighting::SkyDome>>,
}

impl ShotState<'_, '_> {
    /// Crack-check background (re-applied every frame: lighting may rewrite the clear colour).
    fn background(&mut self, crack_check: bool, saved: &mut Option<Color>) {
        let want = if crack_check {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for mut v in &mut self.dome {
            v.set_if_neq(want);
        }
        if crack_check {
            saved.get_or_insert(self.clear.0);
            self.clear.0 = Color::srgb(1.0, 0.0, 1.0);
        } else if let Some(c) = saved.take() {
            self.clear.0 = c;
        }
    }

    fn apply(&mut self, shot: &Shot) {
        self.lighting.hour = shot.hour.unwrap_or(12.0);
        self.ui.0 = shot.ui;
        self.zoning.0 = shot.zoning;
        self.lanes.0 = shot.lanes;
        self.cursor.0 = None;
        *self.tool = ActiveTool::None;
        *self.dialog = crate::tools::BulldozeDialog::default();
        let feature = |name: &str| self.features.as_ref().and_then(|f| f.0.get(name).copied());
        match shot.tool {
            None => {}
            Some(ShotTool::RoadSteep) => {
                if let Some(f) = feature("steep") {
                    let p = Vec2::new(f.pos.x, f.pos.z);
                    // `dir` of the steep feature points uphill.
                    let up = f.dir;
                    let (a, b) = (p - up * 40.0, p + up * 40.0);
                    *self.tool = ActiveTool::Road(Box::new(RoadTool {
                        points: vec![a],
                        ..Default::default()
                    }));
                    let y = self
                        .terrain
                        .as_ref()
                        .map_or(f.pos.y, |t| t.terrain().height_at(b));
                    self.cursor.0 = Some(Vec3::new(b.x, y, b.y));
                }
            }
            Some(ShotTool::EditUpgrade) => {
                if let Some(f) = feature("y30") {
                    let p = Vec2::new(f.pos.x, f.pos.z) - Vec2::X * 55.0;
                    *self.tool = ActiveTool::RoadEdit(Box::default());
                    let y = self
                        .terrain
                        .as_ref()
                        .map_or(f.pos.y, |t| t.terrain().height_at(p));
                    self.cursor.0 = Some(Vec3::new(p.x, y, p.y));
                }
            }
            Some(ShotTool::RoundaboutPreset) => {
                if let Some(f) = feature("y30") {
                    let c = Vec2::new(f.pos.x, f.pos.z) + Vec2::new(-40.0, -70.0);
                    let edge = c + Vec2::X * 35.0;
                    *self.tool = ActiveTool::Road(Box::new(RoadTool {
                        mode: crate::tools::RoadMode::Roundabout,
                        points: vec![c],
                        ..Default::default()
                    }));
                    let y = self
                        .terrain
                        .as_ref()
                        .map_or(f.pos.y, |t| t.terrain().height_at(edge));
                    self.cursor.0 = Some(Vec3::new(edge.x, y, edge.y));
                }
            }
            Some(ShotTool::PlotEdit) => {
                let c = feature("corner").map(|f| Vec2::new(f.pos.x, f.pos.z));
                let selected = c.and_then(|c| {
                    let ids = self.ids.as_ref()?;
                    ids.plots
                        .iter()
                        .filter_map(|(id, e)| {
                            let p = self.plots.get(e).ok()?;
                            let n = p.polygon().len().max(1) as f32;
                            let centroid = p.polygon().iter().copied().sum::<Vec2>() / n;
                            Some((centroid.distance(c), id))
                        })
                        .min_by(|x, y| x.0.total_cmp(&y.0))
                        .map(|x| x.1)
                });
                *self.tool = ActiveTool::Plot(PlotTool {
                    selected,
                    ..Default::default()
                });
            }
            Some(ShotTool::BulldozeDialog) => {
                let Some(c) = feature("slope").map(|f| Vec2::new(f.pos.x, f.pos.z)) else {
                    return;
                };
                *self.tool = ActiveTool::Bulldoze;
                // The street nearest the feature that has plots along it.
                let mut best: Option<(f32, cb_core::ids::SegmentId)> = None;
                for seg in &self.segs {
                    let d = seg.geom().project(c).1.abs();
                    let fronted = self.plots.iter().any(|p| p.segments().contains(&seg.id()));
                    if fronted && best.is_none_or(|b| d < b.0) {
                        best = Some((d, seg.id()));
                    }
                }
                if let Some((_, sid)) = best {
                    let mut deps = Vec::new();
                    let mut houses = 0;
                    for p in self.plots.iter().filter(|p| p.segments().contains(&sid)) {
                        deps.push(p.id());
                        houses += p
                            .lots()
                            .iter()
                            .filter(|l| {
                                self.ids
                                    .as_ref()
                                    .and_then(|i| i.lots.get(**l))
                                    .and_then(|e| self.lots.get(e).ok())
                                    .is_some_and(|l| l.building().is_some())
                            })
                            .count();
                    }
                    deps.sort();
                    self.dialog.open(sid, deps, houses);
                }
            }
        }
    }
}

fn drive(
    mut commands: Commands,
    mut runner: ResMut<ScreenshotRunner>,
    mut rig: ResMut<CameraRig>,
    anchor: Option<Res<SceneAnchor>>,
    backlog: Res<ViewBacklog>,
    mut exit: MessageWriter<AppExit>,
    mut state: ShotState,
) {
    commands.insert_resource(CameraScripted);
    if let Some(n) = runner.exit_in.as_mut() {
        if *n == 0 {
            exit.write(AppExit::Success);
        } else {
            *n -= 1;
        }
        return;
    }
    if runner.pending {
        return;
    }
    let Some(shot) = runner.suite.shots.get(runner.index).cloned() else {
        let body: Vec<String> = runner
            .results
            .iter()
            .map(|(n, m)| format!("  {}", to_json(n, m)))
            .collect();
        let json = format!("[\n{}\n]\n", body.join(",\n"));
        let path = runner.out.join("metrics.json");
        if let Err(e) = std::fs::write(&path, json) {
            error!("cannot write {}: {e}", path.display());
        }
        info!(
            "screenshot suite done: {} shots in {}",
            runner.results.len(),
            runner.out.display()
        );
        // A few frames for the PNG writes to finish.
        runner.exit_in = Some(3);
        return;
    };
    if runner.waited == 0 {
        let mut r = shot.camera.rig();
        // Shots are placed relative to a feature anchor (yaw relative to it) or the town.
        let feature = shot.at.as_ref().and_then(|n| {
            let f = state.features.as_ref().and_then(|f| f.0.get(n).copied());
            if f.is_none() {
                error!("shot {}: no feature anchor {n:?}", shot.name);
            }
            f
        });
        if let Some(f) = feature {
            r.focus += f.pos;
            r.yaw_deg += f.yaw_deg;
        } else {
            r.focus += anchor.map_or(Vec3::ZERO, |a| a.0);
        }
        *rig = r;
        state.apply(&shot);
    }
    let mut saved = runner.saved_clear.take();
    state.background(shot.crack_check, &mut saved);
    runner.saved_clear = saved;
    runner.waited += 1;
    // Wait for the settle frames and for chunk work near the new view to finish.
    if runner.waited < runner.suite.settle_frames || (backlog.0 > 0 && runner.waited < 600) {
        return;
    }
    runner.waited = 0;
    runner.pending = true;
    let path = runner.out.join(format!("{}.png", shot.name));
    if let Err(e) = std::fs::create_dir_all(&runner.out) {
        error!("cannot create {}: {e}", runner.out.display());
    }
    let name = shot.name.clone();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(
            move |captured: On<ScreenshotCaptured>, mut runner: ResMut<ScreenshotRunner>| {
                let image = &captured.image;
                let size = image.size();
                let metrics = image
                    .clone()
                    .try_into_dynamic()
                    .map(|d| metrics_rgba8(size.x, size.y, d.to_rgba8().as_raw()))
                    .unwrap_or_default();
                info!(
                    "shot {name}: {}x{} median={} lum_mean={:.3} lum_median={:.3} clipped={:.2}% black={:.2}% magenta={} holes={}",
                    size.x,
                    size.y,
                    metrics.luma_median,
                    metrics.lum_mean,
                    metrics.lum_median,
                    metrics.clipped_pct,
                    metrics.black_pct,
                    metrics.magenta_count,
                    metrics.holes
                );
                runner.results.push((name.clone(), metrics));
                runner.pending = false;
                runner.index += 1;
            },
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suites_parse() {
        let s = load_suite("M1").unwrap();
        assert_eq!(s.resolution, (1600, 900));
        assert!(s.shots.len() >= 3);
        assert!(load_suite("nope").is_err());
        let m2 = load_suite("M2").unwrap();
        assert_eq!(m2.shots.len(), 16);
        assert_eq!(m2.seed, Some(1234));
        assert!(m2.shots.iter().any(|s| s.tool == Some(ShotTool::PlotEdit)));
    }
}
