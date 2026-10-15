# ADR-0030: Input and action mapping

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

All bindings defined in one input config and remappable in settings; contexts (camera, tools,
UI); modifiers (Shift/Ctrl/Alt); mouse buttons and wheel; conflict detection; egui must
capture input when hovered/focused.

## Options considered

1. **leafwing-input-manager** — feature-rich; extra dependency; must track Bevy releases.
2. **Own small action layer (chosen)** — `Action` enum, `InputMap { Action → Vec<Binding> }`,
   ~300 lines, serde-friendly for settings.

## Decision

- `Binding { input: Key(KeyCode) | Mouse(MouseButton) | Wheel(Axis), modifiers: Mods,
  trigger: Press | Release | Hold | Tap(max_ms) }`; `ActionState` resource evaluated in
  `GameSet::Input` each frame (`just_pressed`, `pressed`, `tapped`, `axis`).
- Contexts: Global (pause, speeds, undo/redo, screenshots), Camera, Tool, Ui; egui
  `wants_pointer_input`/`wants_keyboard_input` suppress world actions.
- Defaults (README controls): WASD/arrows pan, wheel zoom, MMB drag rotate, Q/E 90° snap
  (tap), left-drag on empty ground pan, Space pause, 1/2/3 speeds, Ctrl+Z / Ctrl+Y undo/redo,
  B bulldoze, R road tool, Z plot tool, Esc cancel/menu, PgUp/PgDn road elevation, Tab cycle
  road mode, F follow selected vehicle, F12 screenshot, F3 perf overlay (dev).
- Settings UI lists actions with rebinding capture and conflict warnings; persisted in
  `settings.ron`.

## Consequences

- No dependency; full control; small maintenance cost.
