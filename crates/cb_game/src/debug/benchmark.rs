//! `--benchmark <scene> [--no-gpu-work]` (PERFORMANCE §13): a deterministic scene, a 60 s
//! scripted camera path, then a copy-paste friendly report on stdout and in the logs folder.
//!
//! Measured per frame: the main-thread update (`First` → `Last`) and the frame interval (which
//! includes waiting for the render thread and the GPU). `--no-gpu-work` renders a 64 × 64
//! window with shadows, SSAO and bloom off so container CPU numbers are not dominated by
//! lavapipe. M2 scenes: `m2_slice` and `bench_city` (the starting town with the scripted
//! features) and `empty` (terrain and scatter only: the engine baseline); larger citygen scenes
//! follow with M4.

use std::path::PathBuf;
use std::time::Instant;

use bevy::anti_alias::smaa::Smaa;
use bevy::pbr::ScreenSpaceAmbientOcclusion;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use tracing::{error, info};

use crate::app::GameSet;
use crate::camera::{CameraRig, CameraScripted, WorldCamera};
use crate::scene::{SceneAnchor, SceneFeatures};
use crate::view::{ViewBacklog, ViewState, ViewStep};

/// Scenes known to `--benchmark`.
pub const SCENES: [&str; 3] = ["m2_slice", "bench_city", "empty"];
/// Length of the camera path (s).
pub const PATH_S: f32 = 60.0;
/// Longest wait for the start-up mesh backlog before the path starts (s).
pub const WARMUP_MAX_S: f32 = 30.0;

/// One camera keyframe: time, anchor name (`town` = scene anchor), rig relative to it.
struct Key {
    t: f32,
    at: &'static str,
    focus: Vec3,
    distance: f32,
    pitch_deg: f32,
    yaw_deg: f32,
}

const PATH: [Key; 6] = [
    Key {
        t: 0.0,
        at: "town",
        focus: Vec3::ZERO,
        distance: 2000.0,
        pitch_deg: 60.0,
        yaw_deg: 0.0,
    },
    Key {
        t: 12.0,
        at: "town",
        focus: Vec3::ZERO,
        distance: 300.0,
        pitch_deg: 45.0,
        yaw_deg: 25.0,
    },
    Key {
        t: 24.0,
        at: "town",
        focus: Vec3::new(40.0, 0.0, 30.0),
        distance: 120.0,
        pitch_deg: 35.0,
        yaw_deg: 120.0,
    },
    Key {
        t: 36.0,
        at: "town",
        focus: Vec3::ZERO,
        distance: 45.0,
        pitch_deg: 20.0,
        yaw_deg: 240.0,
    },
    Key {
        t: 48.0,
        at: "slope",
        focus: Vec3::ZERO,
        distance: 150.0,
        pitch_deg: 40.0,
        yaw_deg: 60.0,
    },
    Key {
        t: 60.0,
        at: "town",
        focus: Vec3::ZERO,
        distance: 600.0,
        pitch_deg: 50.0,
        yaw_deg: 300.0,
    },
];

/// Percentiles of a sample set (ms).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// Median.
    pub p50: f32,
    /// 95th percentile.
    pub p95: f32,
    /// 99th percentile.
    pub p99: f32,
    /// 99.9th percentile.
    pub p999: f32,
    /// Maximum.
    pub max: f32,
}

impl Stats {
    /// Nearest-rank percentiles.
    #[must_use]
    pub fn of(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut v = samples.to_vec();
        v.sort_by(f32::total_cmp);
        let at = |q: f32| {
            let i = ((q * v.len() as f32).ceil() as usize).clamp(1, v.len()) - 1;
            v[i]
        };
        Self {
            p50: at(0.5),
            p95: at(0.95),
            p99: at(0.99),
            p999: at(0.999),
            max: v[v.len() - 1],
        }
    }

    fn line(&self, name: &str) -> String {
        format!(
            "{name:<10} p50 {:6.2}  p95 {:6.2}  p99 {:6.2}  p99.9 {:6.2}  max {:6.2} ms",
            self.p50, self.p95, self.p99, self.p999, self.max
        )
    }
}

/// Benchmark state.
#[derive(Resource, Debug)]
pub struct Benchmark {
    scene: String,
    no_gpu_work: bool,
    out_dir: PathBuf,
    seed: u64,
    started: Option<Instant>,
    boot: Instant,
    frame_start: Option<Instant>,
    last_end: Option<Instant>,
    main_ms: Vec<f32>,
    frame_ms: Vec<f32>,
    /// Instants at the `Update` set boundaries this frame.
    marks: [Option<Instant>; 5],
    /// Per-segment times (ms): before `Update`, the four `GameSet`s, after `Update`.
    segments: [Vec<f32>; 6],
    /// Instants after each `ViewStep` this frame.
    view_marks: [Option<Instant>; 5],
    /// Per-step view-sync times (ms).
    view_steps: [Vec<f32>; 5],
    exit_in: Option<u32>,
}

/// Names of the timed main-thread segments.
const SEGMENTS: [&str; 6] = [
    "pre-update",
    "input",
    "sim-drive",
    "view-sync",
    "present",
    "post-update",
];

/// Per-frame sum of the game's own sets (input, sim-drive, view-sync, present): the part the
/// container budget covers (ADR-0058 amendment).
fn game_sets(segments: &[Vec<f32>; 6]) -> Vec<f32> {
    let n = segments[1..5].iter().map(Vec::len).min().unwrap_or(0);
    (0..n)
        .map(|f| segments[1..5].iter().map(|v| v[f]).sum())
        .collect()
}

fn mark<const N: usize>(mut bench: ResMut<Benchmark>) {
    bench.marks[N] = Some(Instant::now());
}

fn view_mark<const N: usize>(mut bench: ResMut<Benchmark>) {
    bench.view_marks[N] = Some(Instant::now());
}

/// Names of the view-sync steps (`ViewStep` order).
const VIEW_STEPS: [&str; 5] = [
    "view:network",
    "view:buildings",
    "view:zoning",
    "view:terrain",
    "view:scatter",
];

/// Benchmark plugin.
pub struct BenchmarkPlugin {
    /// Scene name.
    pub scene: String,
    /// Tiny render target, no shadows/SSAO/bloom.
    pub no_gpu_work: bool,
    /// Where the report goes (logs folder).
    pub out_dir: PathBuf,
    /// World seed (for the report).
    pub seed: u64,
}

impl Plugin for BenchmarkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Benchmark {
            scene: self.scene.clone(),
            no_gpu_work: self.no_gpu_work,
            out_dir: self.out_dir.clone(),
            seed: self.seed,
            started: None,
            boot: Instant::now(),
            frame_start: None,
            last_end: None,
            main_ms: Vec::with_capacity(8192),
            frame_ms: Vec::with_capacity(8192),
            marks: [None; 5],
            segments: Default::default(),
            view_marks: [None; 5],
            view_steps: Default::default(),
            exit_in: None,
        })
        .insert_resource(CameraScripted)
        .add_systems(PostStartup, strip_gpu_work)
        .add_systems(First, frame_start)
        .add_systems(
            Update,
            (
                mark::<0>.before(GameSet::Input),
                mark::<1>.after(GameSet::Input).before(GameSet::SimDrive),
                mark::<2>.after(GameSet::SimDrive).before(GameSet::ViewSync),
                mark::<3>
                    .after(GameSet::ViewSync)
                    .before(GameSet::Presentation),
                mark::<4>.after(GameSet::Presentation),
                view_mark::<0>
                    .after(ViewStep::Network)
                    .before(ViewStep::Buildings),
                view_mark::<1>
                    .after(ViewStep::Buildings)
                    .before(ViewStep::Zoning),
                view_mark::<2>
                    .after(ViewStep::Zoning)
                    .before(ViewStep::Terrain),
                view_mark::<3>
                    .after(ViewStep::Terrain)
                    .before(ViewStep::Scatter),
                view_mark::<4>
                    .after(ViewStep::Scatter)
                    .in_set(GameSet::ViewSync),
            ),
        )
        .add_systems(Last, frame_end);
    }
}

fn strip_gpu_work(
    mut commands: Commands,
    bench: Res<Benchmark>,
    cams: Query<Entity, With<WorldCamera>>,
    mut lights: Query<&mut DirectionalLight>,
) {
    if !bench.no_gpu_work {
        return;
    }
    for e in &cams {
        commands
            .entity(e)
            .remove::<(ScreenSpaceAmbientOcclusion, Bloom, Smaa)>();
    }
    for mut l in &mut lights {
        l.shadow_maps_enabled = false;
    }
}

fn frame_start(mut bench: ResMut<Benchmark>) {
    bench.frame_start = Some(Instant::now());
}

/// Camera rig at path time `t` (linear focus/angles, logarithmic distance).
fn rig_at(t: f32, anchor: Vec3, features: Option<&SceneFeatures>) -> CameraRig {
    let base = |k: &Key| {
        let (pos, yaw) = match (k.at, features.and_then(|f| f.0.get(k.at))) {
            ("town", _) | (_, None) => (anchor, 0.0),
            (_, Some(f)) => (f.pos, f.yaw_deg),
        };
        CameraRig {
            focus: pos + k.focus,
            distance: k.distance,
            pitch_deg: k.pitch_deg,
            yaw_deg: k.yaw_deg + yaw,
        }
    };
    let i = PATH
        .iter()
        .rposition(|k| k.t <= t)
        .unwrap_or(0)
        .min(PATH.len() - 2);
    let (a, b) = (base(&PATH[i]), base(&PATH[i + 1]));
    let u = ((t - PATH[i].t) / (PATH[i + 1].t - PATH[i].t)).clamp(0.0, 1.0);
    let u = u * u * (3.0 - 2.0 * u);
    CameraRig {
        focus: a.focus.lerp(b.focus, u),
        distance: (a.distance.ln() + (b.distance.ln() - a.distance.ln()) * u).exp(),
        pitch_deg: a.pitch_deg + (b.pitch_deg - a.pitch_deg) * u,
        yaw_deg: a.yaw_deg + (b.yaw_deg - a.yaw_deg) * u,
    }
}

fn frame_end(world: &mut World) {
    let now = Instant::now();
    let Some(mut bench) = world.remove_resource::<Benchmark>() else {
        return;
    };
    if let Some(n) = bench.exit_in.as_mut() {
        if *n == 0 {
            world.write_message(AppExit::Success);
        } else {
            *n -= 1;
        }
        world.insert_resource(bench);
        return;
    }
    let anchor = world
        .get_resource::<SceneAnchor>()
        .map_or(Vec3::ZERO, |a| a.0);
    match bench.started {
        None => {
            // Wait for the start-up mesh backlog, then start the path.
            let backlog = world.get_resource::<ViewBacklog>().map_or(0, |b| b.0);
            if backlog == 0 || now.duration_since(bench.boot).as_secs_f32() > WARMUP_MAX_S {
                info!("benchmark: path starts (backlog {backlog})");
                bench.started = Some(now);
            }
            let rig = rig_at(0.0, anchor, world.get_resource::<SceneFeatures>());
            world.insert_resource(rig);
        }
        Some(start) => {
            let t = now.duration_since(start).as_secs_f32();
            if let Some(s) = bench.frame_start {
                bench
                    .main_ms
                    .push(now.duration_since(s).as_secs_f32() * 1000.0);
                // Segment boundaries: frame start, the five marks, frame end.
                let mut bounds = vec![Some(s)];
                bounds.extend(bench.marks);
                bounds.push(Some(now));
                for i in 0..SEGMENTS.len() {
                    if let (Some(a), Some(b)) = (bounds[i], bounds[i + 1]) {
                        bench.segments[i].push(b.duration_since(a).as_secs_f32() * 1000.0);
                    }
                }
                // View steps: from the start of view-sync through each step's end mark.
                let mut prev = bench.marks[2];
                for i in 0..VIEW_STEPS.len() {
                    if let (Some(a), Some(b)) = (prev, bench.view_marks[i]) {
                        bench.view_steps[i].push(b.duration_since(a).as_secs_f32() * 1000.0);
                    }
                    prev = bench.view_marks[i];
                }
            }
            if let Some(l) = bench.last_end {
                bench
                    .frame_ms
                    .push(now.duration_since(l).as_secs_f32() * 1000.0);
            }
            if t >= PATH_S {
                report(world, &bench);
                bench.exit_in = Some(2);
            } else {
                let rig = rig_at(t, anchor, world.get_resource::<SceneFeatures>());
                world.insert_resource(rig);
            }
        }
    }
    bench.last_end = Some(now);
    world.insert_resource(bench);
}

fn report(world: &mut World, bench: &Benchmark) {
    let adapter = world.get_resource::<RenderAdapterInfo>().map_or_else(
        || "unknown".to_owned(),
        |a| format!("{} ({:?})", a.name, a.backend),
    );
    let (w, h) = world
        .query::<&Window>()
        .iter(world)
        .next()
        .map_or((0.0, 0.0), |w| (w.width(), w.height()));
    let meshes = world.query::<&Mesh3d>().iter(world).count();
    let entities = world.entities().len();
    let (buildings, chunks) = world
        .get_resource::<ViewState>()
        .map_or((0, 0), |v| (v.building_count(), v.terrain_chunks()));
    let ticks = world
        .get_resource::<cb_sim::SimClock>()
        .map_or(0, |c| c.tick().0);
    let (main, frame) = (Stats::of(&bench.main_ms), Stats::of(&bench.frame_ms));
    let text = format!(
        "City Builder benchmark\n\
         scene      {} (seed {})\n\
         build      {} {}\n\
         adapter    {adapter}\n\
         window     {w}x{h} logical, no-gpu-work {}\n\
         frames     {} in {PATH_S} s ({:.1} fps)\n\
         {}\n{}\n{}\
         entities   {entities} ({meshes} meshes, {buildings} building visuals, {chunks} terrain chunk-LODs)\n\
         sim ticks  {ticks}\n",
        bench.scene,
        bench.seed,
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        bench.no_gpu_work,
        bench.frame_ms.len(),
        bench.frame_ms.len() as f32 / PATH_S,
        main.line("main"),
        frame.line("frame"),
        bench
            .segments
            .iter()
            .zip(SEGMENTS)
            .map(|(v, n)| format!("  {}\n", Stats::of(v).line(n)))
            .collect::<String>()
            + &format!(
                "{}\n",
                Stats::of(&game_sets(&bench.segments)).line("game sets")
            )
            + &bench
                .view_steps
                .iter()
                .zip(VIEW_STEPS)
                .map(|(v, n)| format!("  {}\n", Stats::of(v).line(n)))
                .collect::<String>(),
    );
    println!("{text}");
    info!("benchmark report:\n{text}");
    let path = bench.out_dir.join(format!("benchmark-{}.txt", bench.scene));
    if let Err(e) =
        std::fs::create_dir_all(&bench.out_dir).and_then(|()| std::fs::write(&path, &text))
    {
        error!("cannot write {}: {e}", path.display());
    } else {
        info!("benchmark report written to {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_nearest_rank() {
        let v: Vec<f32> = (1..=1000).map(|i| i as f32).collect();
        let s = Stats::of(&v);
        assert_eq!(
            (s.p50, s.p95, s.p99, s.p999, s.max),
            (500.0, 950.0, 990.0, 999.0, 1000.0)
        );
        assert_eq!(Stats::of(&[]), Stats::default());
        assert_eq!(Stats::of(&[3.0]).p50, 3.0);
    }

    #[test]
    fn camera_path_is_continuous() {
        let mut prev = rig_at(0.0, Vec3::ZERO, None);
        let mut t = 0.0;
        while t <= PATH_S {
            let r = rig_at(t, Vec3::ZERO, None);
            assert!(r.distance >= 40.0 && r.distance <= 2000.0 + 1e-3);
            assert!(r.focus.distance(prev.focus) < 5.0, "jump at {t}");
            prev = r;
            t += 0.05;
        }
    }
}
