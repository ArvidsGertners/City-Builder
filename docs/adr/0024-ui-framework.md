# ADR-0024: UI framework — egui via bevy_egui

- **Status:** Superseded by ADR-0051
- **Date:** 2026-10-15
- **Milestone:** M1 (debug), M8 (game UI)

## Context

A data-heavy city-builder UI: top bar, RCI bars, tool bar with flyouts, road customization,
budget/tax panel with graphs, transit line manager, info panels, overlay legends,
notifications, tooltips everywhere, menus, settings with key remapping; UI scaling,
high-contrast theme, localization with long German strings; debug tools.

## Options considered

1. **bevy_ui (+ bevy_feathers / ui widgets)** — retained, ECS-native, styleable; in 0.20
   gaining widgets (text input, number input, menus) but still young; data-heavy panels
   need a lot of boilerplate; tooltips/graphs/tables custom.
2. **egui via bevy_egui 0.43 (chosen)** — immediate mode, very productive for tables,
   sliders, graphs (painter), tooltips built in, wrapping text, `pixels_per_point` for UI
   scale, AccessKit support, one framework for game UI and debug tools.
3. Custom UI on top of meshes — far too much work.

## Decision

- All UI (game HUD, panels, menus, tutorial, debug) in egui, in
  `EguiPrimaryContextPass` systems under `GameSet::Ui`.
- Custom theme (`ui/theme.rs`) from ART_DIRECTION §9 (light warm + high-contrast dark),
  custom widgets (RCI bars, toolbar buttons with procedural icons, money labels, graph
  sparkline, toggle chips), procedural icon atlas registered as an egui texture.
- UI reads sim state read-only and emits `PlayerCommand`/`UiIntent`; no game logic in UI code.
- UI scale = settings scale × OS scale factor; layouts use wrapping and min/max widths
  (German-safe).
- World-space UI (problem icons, labels) is rendered in 3D (billboards), not egui.

## Consequences

- Fast iteration; consistent look via theme; immediate-mode cost is small for our panel
  counts (budget 1.5 ms).
- egui's look must be styled deliberately to feel like a game (rounded frames, shadows,
  custom widgets); fonts are egui defaults (RISKS Q6).
