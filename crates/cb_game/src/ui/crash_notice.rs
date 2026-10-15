//! "The game closed unexpectedly last time" window (ADR-0022; moves into the main menu in M15).

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use cb_l10n::{Arg, keys};
use tracing::warn;

use crate::bootstrap::crash;
use crate::l10n::Localization;
use crate::platform::open::open_folder;

/// The previous session's crash report, while the notice is shown.
#[derive(Resource, Debug)]
pub struct LastCrash {
    /// Report file.
    pub path: PathBuf,
    /// Report text (for copying).
    pub text: String,
    /// Crashes directory (marker location).
    pub crashes_dir: PathBuf,
}

pub(super) fn show(
    mut commands: Commands,
    mut contexts: EguiContexts,
    crash_info: Option<Res<LastCrash>>,
    l10n: Res<Localization>,
) {
    let Some(info) = crash_info else { return };
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let l = &l10n.0;
    let path_text = info.path.display().to_string();
    let mut dismiss = false;
    egui::Window::new(l.tr(keys::CRASH_NOTICE_TITLE))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(l.tr_args(
                keys::CRASH_NOTICE_BODY,
                &[("path", Arg::from(path_text.as_str()))],
            ));
            ui.horizontal(|ui| {
                if ui.button(l.tr(keys::CRASH_NOTICE_COPY)).clicked() {
                    ui.ctx().copy_text(info.text.clone());
                }
                if ui.button(l.tr(keys::CRASH_NOTICE_OPEN_FOLDER)).clicked()
                    && let Err(e) = open_folder(&info.crashes_dir)
                {
                    warn!("cannot open {}: {e}", info.crashes_dir.display());
                }
                if ui.button(l.tr(keys::CRASH_NOTICE_DISMISS)).clicked() {
                    dismiss = true;
                }
            });
        });
    if dismiss {
        crash::dismiss_last_crash(&info.crashes_dir);
        commands.remove_resource::<LastCrash>();
    }
}
