//! Application states (ARCHITECTURE §7). M1 boots straight into `InGame`.

use bevy::state::state::States;

/// Top-level application state.
#[derive(States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum AppState {
    /// Paths, settings, l10n, crash notice.
    #[default]
    Boot,
    /// Main menu (M15).
    MainMenu,
    /// World generation or save restore (M3/M15).
    Loading,
    /// Playing.
    InGame,
    /// Procgen debug scene.
    ProcgenGallery,
}
