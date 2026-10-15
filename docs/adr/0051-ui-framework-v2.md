# ADR-0051: UI framework v2 — egui scheduling, glyphs, structural tooltips

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (foundation), M8 (full UI)
- **Supersedes:** ADR-0024 (keeps egui via bevy_egui 0.43 for all UI, custom theme and widgets,
  world-space icons in 3D)

## Context

Review findings G-W1, G-W2, G-W6, A58, E9, E12: bevy_egui 0.43 runs `EguiPrimaryContextPass`
from `PostUpdate`, so UI cannot run in `Update` before the sim step (verified); egui's default
fonts lack ⌥ ⌃ ⌫ and only have ⌘/⇧ in fallback fonts (verified); bevy_egui's default
features pull `bevy_ui`, picking, `webbrowser`; "tooltips everywhere" had no mechanism;
the tutorial needs UI anchors; tool feedback in M2/M3 needs the theme long before M8.

## Decision

- `bevy_egui = { version = "0.43", default-features = false, features = ["render",
  "default_fonts", "manage_clipboard"] }` (`accesskit` evaluated in M8; `manage_clipboard`
  brings `arboard`/`smithay-clipboard`, listed in ADR-0023).
- UI systems run in `EguiPrimaryContextPass` (PostUpdate). Commands they emit are applied by
  the next frame's `SimEdit` (one-frame UI latency, documented). World input in
  `GameSet::Input` is gated on the previous frame's egui "wants pointer/keyboard" state.
- **Shortcut glyphs** are procedural SDF icons from the icon atlas, drawn by a `shortcut_chip`
  widget; running text uses localized words ("Cmd", "Option", "Shift", "Ctrl"). No font asset.
- **Structural tooltips**: `ui::widgets::{button, toggle, slider, tool_button, …}` require a
  tooltip `Key`; a lint test fails on direct `ui.button(`/`egui::Button` use in `cb_game/src/ui`.
- **Anchors**: widgets register `AnchorKey → Rect` each frame (`ui::anchors`) for tutorial
  highlights and screenshot assertions.
- The theme, widgets, dialogs, toasts and a minimal toolbar land in **M2**; M8 completes HUD,
  panels, overlays and info views.

## Consequences

- Correct scheduling, no tofu glyphs, tooltips enforced by construction, tutorial-ready UI.
