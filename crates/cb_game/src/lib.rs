//! City Builder game: bootstrap (paths → settings → logging → crash hook → l10n) and the
//! Bevy app with rendering, UI and debug tooling. `main.rs` only parses the command line.

pub mod app;
pub mod bootstrap;
pub mod camera;
pub mod debug;
pub mod l10n;
pub mod overlay;
pub mod picking;
pub mod platform;
pub mod render;
pub mod scene;
pub mod settings;
pub mod states;
pub mod tools;
pub mod ui;
pub mod view;

use std::process::ExitCode;

use bevy::app::AppExit;
use cb_l10n::L10n;
use tracing::{error, info, warn};

use crate::bootstrap::cli::Cli;
use crate::bootstrap::{crash, logging};
use crate::platform::Os;
use crate::platform::paths::{self, PathMode};
use crate::settings::persist::{self, LoadSource};

/// Version line for `--version` and logs.
#[must_use]
pub fn version_line() -> String {
    format!(
        "City Builder {} ({}, {}, {} {})",
        env!("CARGO_PKG_VERSION"),
        env!("CB_GIT_HASH"),
        env!("CB_BUILD_PROFILE"),
        Os::current().name(),
        std::env::consts::ARCH
    )
}

/// Runs the game with parsed arguments.
#[must_use]
pub fn run(cli: Cli) -> ExitCode {
    let paths = paths::resolve_current();
    let loaded = persist::load(&paths.settings_file);

    let env_filter = std::env::var("CB_LOG").ok();
    let filter = logging::choose_filter(
        cli.log.as_deref(),
        env_filter.as_deref(),
        Some(loaded.settings.log_filter.as_str()),
    );
    let log = logging::init(&paths.logs_dir, &filter);
    crash::install(&paths.crashes_dir, log.ring.clone());

    info!("{}", version_line());
    info!(
        "paths: {:?}; settings {}",
        paths.mode,
        paths.settings_file.display()
    );
    if paths.mode == PathMode::TempFallback {
        warn!("no home directory found; using the temp directory for settings, saves and logs");
    }
    for w in &loaded.warnings {
        warn!("{w}");
    }
    let settings = loaded.settings;
    crash::set_settings_summary(settings.summary());
    if loaded.source == LoadSource::Defaults
        && let Err(e) = persist::save(&paths.settings_file, &settings)
    {
        warn!(
            "cannot write default settings to {}: {e}",
            paths.settings_file.display()
        );
    }

    let env = |k: &str| std::env::var(k).ok();
    let locale = l10n::choose_locale(
        cli.language.as_deref(),
        settings.language,
        l10n::system_locale(&env),
    );
    let l10n = match L10n::new(locale) {
        Ok(l) => l10n::Localization(l),
        Err(e) => {
            error!("localization failed to load: {e}");
            return ExitCode::FAILURE;
        }
    };
    info!("locale: {}", locale.code());

    if cli.debug_panic {
        panic!("debug panic requested with --debug-panic");
    }

    let suite = match cli
        .screenshot_suite
        .as_deref()
        .map(debug::screenshot::load_suite)
    {
        None => None,
        Some(Ok(s)) => Some(s),
        Some(Err(e)) => {
            error!("{e}");
            return ExitCode::from(2);
        }
    };

    if let Some(b) = cli.benchmark.as_deref()
        && !debug::benchmark::SCENES.contains(&b)
    {
        error!(
            "unknown benchmark scene {b:?}; known: {:?}",
            debug::benchmark::SCENES
        );
        return ExitCode::from(2);
    }

    let (log_ring, log_counters) = app::Boot::log_parts(&log);
    let mut app = app::build(app::Boot {
        cli,
        paths,
        settings,
        l10n,
        log_ring,
        log_counters,
        suite,
    });
    let exit = app.run();
    info!("exit: {exit:?}");
    drop(log);
    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}
