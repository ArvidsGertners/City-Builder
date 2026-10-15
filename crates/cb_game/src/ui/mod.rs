//! egui UI (ADR-0051). M1: font setup and the crash notice.

pub mod crash_notice;
pub mod font;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass};

/// UI plugin.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            EguiPrimaryContextPass,
            (install_fonts, crash_notice::show).chain(),
        );
    }
}

/// Installs Inter once, as soon as the primary egui context exists.
fn install_fonts(mut contexts: EguiContexts, mut done: Local<bool>) {
    if *done {
        return;
    }
    if let Ok(ctx) = contexts.ctx_mut() {
        font::install(ctx);
        *done = true;
    }
}
