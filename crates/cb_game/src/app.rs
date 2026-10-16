//! Plugin assembly (ARCHITECTURE §4, §10).

use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::{PresentMode, WindowResolution};
use bevy_egui::EguiPlugin;
use cb_l10n::keys;
use cb_sim::{DayLength, SimDriveSystems, SimPacing, SimPlugins};
use tracing::info;

use crate::bootstrap::cli::Cli;
use crate::bootstrap::crash;
use crate::bootstrap::logging::LogHandles;
use crate::debug::screenshot::{ScreenshotPlugin, Suite};
use crate::debug::smoke::{LogCounters, SmokePlugin, render_error_handler};
use crate::l10n::Localization;
use crate::platform::paths::AppPaths;
use crate::scene;
use crate::settings::Settings;
use crate::states::AppState;
use crate::ui::UiPlugin;
use crate::ui::crash_notice::LastCrash;

/// Frame-level ordering of the game's `Update` work.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GameSet {
    /// Input mapping, camera, picking, tools → `PlayerCommand`.
    Input,
    /// `SimEdit` once, then k × `SimTick`.
    SimDrive,
    /// Visuals from sim changes.
    ViewSync,
    /// Lighting, weather FX, overlays, audio, toasts.
    Presentation,
}

/// Everything resolved before Bevy starts.
pub struct Boot {
    /// Command line.
    pub cli: Cli,
    /// File locations.
    pub paths: AppPaths,
    /// Loaded settings.
    pub settings: Settings,
    /// Localization.
    pub l10n: Localization,
    /// Logging handles (the file guard stays in `main`).
    pub log_ring: crate::bootstrap::logging::LogRing,
    /// Error counters.
    pub log_counters: std::sync::Arc<crate::bootstrap::logging::ErrorCounters>,
    /// Screenshot suite, if requested.
    pub suite: Option<Suite>,
}

impl Boot {
    /// Collects the logging parts from `handles`.
    #[must_use]
    pub fn log_parts(
        handles: &LogHandles,
    ) -> (
        crate::bootstrap::logging::LogRing,
        std::sync::Arc<crate::bootstrap::logging::ErrorCounters>,
    ) {
        (
            handles.ring.clone(),
            std::sync::Arc::clone(&handles.counters),
        )
    }
}

/// Builds the full game app.
pub fn build(boot: Boot) -> App {
    let Boot {
        cli,
        paths,
        settings,
        l10n,
        log_ring,
        log_counters,
        suite,
    } = boot;
    let (width, height) = suite
        .as_ref()
        .map(|s| s.resolution)
        .or(cli.no_gpu_work.then_some((64, 64)))
        .or(cli.resolution)
        .unwrap_or((settings.display.width, settings.display.height));
    let verification_run = cli.smoke_test.is_some() || suite.is_some() || cli.benchmark.is_some();
    let title = l10n.0.tr(keys::APP_TITLE);
    info!("window: {width}x{height} \"{title}\"");

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title,
            resolution: WindowResolution::new(width, height),
            present_mode: if settings.display.vsync {
                PresentMode::AutoVsync
            } else {
                PresentMode::AutoNoVsync
            },
            resizable: !verification_run,
            ..default()
        }),
        ..default()
    }))
    .insert_resource(render_error_handler())
    .add_plugins(EguiPlugin::default())
    .add_plugins(SimPlugins {
        seed: cli
            .seed
            .or(suite.as_ref().and_then(|s| s.seed))
            .unwrap_or(1),
        terrain: cb_sim::TerrainSetup::Generate,
    })
    .init_state::<AppState>()
    .configure_sets(
        Update,
        (
            GameSet::Input,
            GameSet::SimDrive,
            GameSet::ViewSync,
            GameSet::Presentation,
        )
            .chain(),
    )
    .configure_sets(Update, SimDriveSystems.in_set(GameSet::SimDrive))
    .insert_resource(SimPacing {
        day_length: DayLength::from_minutes(settings.gameplay.day_length_min).unwrap_or_default(),
        hold: false,
    })
    .insert_resource(LogCounters(log_counters))
    .insert_resource(l10n)
    .add_plugins(UiPlugin)
    .add_plugins((
        crate::render::WorldRenderPlugin,
        crate::camera::CameraPlugin,
        crate::view::ViewPlugin,
        crate::picking::PickingPlugin,
        crate::tools::ToolsPlugin,
        crate::overlay::OverlayPlugin,
        crate::debug::lane_overlay::LaneOverlayPlugin,
    ))
    .add_systems(
        Startup,
        (scene::setup, scene::demo_town, enter_game).chain(),
    )
    .add_systems(Update, record_adapter_info);

    if !verification_run && let Some((path, text)) = crash::last_crash(&paths.crashes_dir) {
        app.insert_resource(LastCrash {
            path,
            text,
            crashes_dir: paths.crashes_dir.clone(),
        });
    }
    if let Some(frames) = cli.smoke_test {
        app.add_plugins(SmokePlugin { frames });
    }
    if let Some(suite) = suite {
        let out = cli
            .out
            .clone()
            .unwrap_or_else(|| paths.screenshots_dir.clone());
        app.add_plugins(ScreenshotPlugin { suite, out });
    }
    if let Some(scene) = cli.benchmark.clone() {
        if scene == "empty" {
            app.insert_resource(scene::NoDemoTown);
        }
        app.add_plugins(crate::debug::benchmark::BenchmarkPlugin {
            scene,
            no_gpu_work: cli.no_gpu_work,
            out_dir: paths.logs_dir.clone(),
            seed: cli.seed.unwrap_or(1),
        });
    }
    #[cfg(feature = "dev")]
    if !verification_run {
        app.insert_resource(crate::debug::dev_tools::ConsoleRing(log_ring.clone()))
            .add_plugins(crate::debug::dev_tools::DevToolsPlugin);
    }
    let _ = log_ring;
    app.insert_resource(settings).insert_resource(paths);
    app
}

fn enter_game(mut next: ResMut<NextState<AppState>>) {
    next.set(AppState::InGame);
}

/// Logs the GPU adapter once and hands it to the crash reporter.
fn record_adapter_info(adapter: Option<Res<RenderAdapterInfo>>, mut done: Local<bool>) {
    if *done {
        return;
    }
    let Some(a) = adapter else { return };
    let line = format!(
        "{} ({:?}, {:?}) driver {} {}",
        a.name, a.backend, a.device_type, a.driver, a.driver_info
    );
    info!("GPU adapter: {line}");
    crash::set_adapter_info(line);
    *done = true;
}
