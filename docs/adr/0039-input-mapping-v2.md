# ADR-0039: Input and action mapping v2 — platform conventions and trackpads

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2
- **Supersedes:** ADR-0030 (keeps the own action layer, contexts and remapping; adds the
  platform-primary modifier, logical-key shortcuts, trackpad gestures, rotation
  alternatives and macOS shortcuts)

## Context

macOS is now required (ADR-0037). Mac users expect Cmd-based shortcuts (Cmd+Z / Cmd+Shift+Z),
use trackpads (pinch to zoom, two-finger scroll to pan, two-finger rotate) and often have no
middle mouse button and no Page Up/Down keys. Separately, ADR-0030 bound letters by physical
key position, which puts Ctrl+Z on the key labeled "Y" for German QWERTZ users — wrong for
a game shipping in German.

## Options considered

1. **Per-platform hard-coded bindings** — duplicates defaults and remapping logic.
2. **Virtual `Primary` modifier + binding kinds (positional vs. logical) + gesture inputs in
   the existing action layer (chosen)**.
3. Adopt `leafwing-input-manager` — still an extra dependency; doesn't solve the logical-key
   and gesture policy for us.

## Decision

- `Binding { input, mods, trigger }` where `input` is one of
  `Physical(KeyCode)` (positional: WASD, Q/E, number keys), `Logical(Key)` (mnemonic letters:
  Z, Y, R, B, F, S — matched via Bevy's `ButtonInput<Key>`), `Mouse(MouseButton)`,
  `Wheel { unit: Line | Pixel | Any }`, `Gesture(Pinch | Rotate | Pan)`.
- `Mods` contains a virtual **`PRIMARY`** = Cmd (`SuperLeft/Right`) on macOS, Ctrl on
  Windows/Linux, resolved once at startup by `cb_game::platform`. Defaults use `PRIMARY`;
  UI shows platform glyphs (⌘ ⌥ ⇧ ⌃ on macOS, "Ctrl/Alt/Shift" elsewhere).
- Default bindings (all remappable):

| Action | Windows / Linux | macOS |
|---|---|---|
| Undo | Ctrl+Z | ⌘Z |
| Redo | Ctrl+Y, Ctrl+Shift+Z | ⌘⇧Z |
| Quick save / settings | Ctrl+S / — | ⌘S / ⌘, |
| Quit (with save prompt) | Alt+F4 (window close) | ⌘Q (intercepted → save prompt) |
| Pan | WASD / arrows / left-drag empty ground | same + two-finger scroll (trackpad) |
| Zoom | mouse wheel | mouse wheel + pinch |
| Rotate free | MMB drag · Alt+left-drag · right-drag | MMB drag · ⌥+left-drag · right-drag · two-finger rotate |
| Rotate 90° snap | Q / E tap | Q / E tap |
| Road elevation | Page Up/Down · `]` / `[` | `]` / `[` (Fn+↑/↓ also works) |
| Bulldoze selection | Delete | ⌫ (Backspace) / Delete |
| Screenshot | F12 | ⌘⇧2 (F12 if available) |

- **Wheel vs. trackpad**: `MouseWheel` events with `MouseScrollUnit::Line` → zoom;
  `MouseScrollUnit::Pixel` (trackpads, also Magic Mouse) → pan by default; `PinchGesture` →
  zoom toward the cursor; `RotationGesture` → yaw; `PanGesture` (macOS) → pan. Setting
  "Scroll input: Auto / Wheel zooms / Scroll pans" resolves ambiguous devices (Magic Mouse
  users choose "Wheel zooms"). Requires Bevy feature `gestures` in cb_game.
- **Right mouse**: click (press + release < 6 px movement, < 250 ms) = cancel/back in tools;
  drag = rotate. On macOS a two-finger click / Ctrl+click arrives as a right click.
- **Linux note**: some desktop environments reserve Alt+drag for window moving; the settings
  tooltip says so and right-drag / MMB remain available.
- Text input in egui uses egui's own command-key handling (already Cmd on macOS); world
  shortcuts are suppressed while egui wants keyboard input.
- Conflict detection in the rebinding UI considers `PRIMARY` resolution per platform.

## Consequences

- Correct shortcuts on all platforms and keyboard layouts; laptops without a mouse are
  fully playable.
- Gesture handling code paths are only exercised on macOS hardware; unit tests cover the
  mapping logic with synthetic events on every OS.
