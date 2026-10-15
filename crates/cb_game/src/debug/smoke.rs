//! `--smoke-test N`: render N frames, then exit 0 only if no GPU validation error occurred
//! (TESTING §6). Prints `frames=N validation_errors=K` to stdout and the log.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::prelude::*;
use bevy::render::error_handler::{RenderError, RenderErrorHandler, RenderErrorPolicy};
use tracing::{error, info};

use crate::bootstrap::logging::ErrorCounters;

/// Render errors reported by wgpu through Bevy's handler (process-wide: the handler is a
/// plain function pointer).
static RENDER_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Total wgpu errors seen through Bevy's render error handler.
#[must_use]
pub fn render_errors() -> u64 {
    RENDER_ERRORS.load(Ordering::Relaxed)
}

fn handle_render_error(
    error: &RenderError,
    main_world: &mut World,
    _render: &mut World,
) -> RenderErrorPolicy {
    RENDER_ERRORS.fetch_add(1, Ordering::Relaxed);
    error!(
        "render error ({:?}): {}; quitting",
        error.ty, error.description
    );
    main_world.write_message(AppExit::error());
    RenderErrorPolicy::StopRendering
}

/// Bevy's default policy (quit on any render error) plus counting.
#[must_use]
pub fn render_error_handler() -> RenderErrorHandler {
    RenderErrorHandler(handle_render_error)
}

/// Error counters from the logging layer.
#[derive(Resource, Clone)]
pub struct LogCounters(pub Arc<ErrorCounters>);

/// Smoke-test progress.
#[derive(Resource, Debug)]
pub struct SmokeTest {
    /// Frames to render.
    pub frames: u32,
    seen: u32,
}

/// Smoke-test plugin.
pub struct SmokePlugin {
    /// Frames to render.
    pub frames: u32,
}

impl Plugin for SmokePlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SmokeTest {
            frames: self.frames,
            seen: 0,
        })
        .add_systems(Last, count_frames);
    }
}

fn count_frames(
    mut smoke: ResMut<SmokeTest>,
    counters: Option<Res<LogCounters>>,
    mut exit: MessageWriter<AppExit>,
) {
    smoke.seen += 1;
    if smoke.seen < smoke.frames {
        return;
    }
    let logged = counters.map_or(0, |c| c.0.gpu_validation.load(Ordering::Relaxed));
    let errors = logged.max(render_errors());
    println!("frames={} validation_errors={errors}", smoke.seen);
    info!("frames={} validation_errors={errors}", smoke.seen);
    exit.write(if errors == 0 {
        AppExit::Success
    } else {
        AppExit::from_code(1)
    });
}
