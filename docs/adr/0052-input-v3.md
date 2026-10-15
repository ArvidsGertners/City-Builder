# ADR-0052: Input and action mapping v3

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2
- **Supersedes:** ADR-0039 (keeps the action layer, contexts, remapping, virtual `PRIMARY`
  modifier, logical-key mnemonics, scroll-unit policy, platform default table except the
  changes below)

## Context

Review findings G-I1–G-I6, G-W4, G-W5, B42, B43: length snapping hard-coded Ctrl (and on macOS
Ctrl+click is a secondary click); Alt was both rotate-drag and plot auto-clip; left-drag pan
collided with tool drags; `]`/`[` need AltGr on German layouts and Macs lack PgUp/PgDn;
`PanGesture` is iOS-only in winit; ⌘Q interception is unverified; Q/E had no continuous rotation.

## Decision

- **Drag precedence**: if a tool is active and the press hits a tool target (road edge, node,
  handle) or the tool is in drawing mode, the tool owns the drag. Left-drag on empty ground pans
  only when no tool is active; **Space + left-drag** always pans; MMB-drag, right-drag and
  Alt/⌥ + left-drag always rotate. Test `input_tool_drag_precedence`.
- **Modifiers as click modifiers**: only Shift (angle snap) and Alt/⌥ are used with clicks;
  Ctrl is never a click modifier. Length snapping and plot auto-clip are **toggles** (tool
  panel chips; length snap hotkey `L`).
- **Road elevation**: Alt/⌥ + wheel, `Physical(Comma)` / `Physical(Period)`, and Page Up/Down
  where present; key labels are rendered from the active keyboard layout.
- **Q/E**: tap (< 200 ms) snaps to the next 90°; hold rotates continuously (deg/s setting).
- **Trackpads (macOS)**: two-finger scroll arrives as pixel-unit `MouseWheel` → pan (policy
  setting unchanged); `PinchGesture` → zoom; `RotationGesture` → yaw. `PanGesture` is not
  used. Gesture behavior is a user-verification item (☑ᵤ).
- **⌘Q / window close**: attempt interception via `WindowCloseRequested`; fallback autosave on
  `AppExit`. Verification item on the M4.
- Backspace/Delete world actions are suppressed while egui wants keyboard input (including
  inspector search boxes); test covers it.

## Consequences

- No conflicts between camera and tools; layouts without AltGr gymnastics; laptop-friendly.
