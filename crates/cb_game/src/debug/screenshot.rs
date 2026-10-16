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
use crate::scene::{CameraPose, SceneAnchor};
use crate::view::ViewBacklog;

/// One shot.
#[derive(Clone, Debug, Deserialize)]
pub struct Shot {
    /// File stem.
    pub name: String,
    /// Camera.
    pub camera: CameraPose,
}

/// A suite.
#[derive(Clone, Debug, Deserialize)]
pub struct Suite {
    /// Window size in logical pixels.
    pub resolution: (u32, u32),
    /// Frames to wait after moving the camera.
    pub settle_frames: u32,
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
        })
        .add_systems(Last, drive);
    }
}

fn drive(
    mut commands: Commands,
    mut runner: ResMut<ScreenshotRunner>,
    mut rig: ResMut<CameraRig>,
    anchor: Option<Res<SceneAnchor>>,
    backlog: Res<ViewBacklog>,
    mut exit: MessageWriter<AppExit>,
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
        // Shots are placed relative to the scene anchor (the starting town).
        r.focus += anchor.map_or(Vec3::ZERO, |a| a.0);
        *rig = r;
    }
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
                    "shot {name}: {}x{} median={} clipped={:.2}% black={:.2}% magenta={}",
                    size.x,
                    size.y,
                    metrics.luma_median,
                    metrics.clipped_pct,
                    metrics.black_pct,
                    metrics.magenta_count
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
    }
}
