# ADR-0041: High-DPI displays and render scale

- **Status:** Superseded by ADR-0053
- **Date:** 2026-10-15
- **Milestone:** M1 (spike), M2 (implementation), M15 (settings UI)

## Context

Apple Silicon Macs use Retina displays (scale factor 2.0, e.g. 2560 × 1664 physical on a
13" M4 MacBook Air, 3024 × 1964 on a 14" Pro); Windows/Linux users have 125–200 % scaling and
4K monitors. Rendering the 3D world at full physical resolution multiplies fill-rate cost
(SSAO, shadows sampling, fog, post) by 2–4×; UI must stay sharp; cursor picking must map
logical cursor coordinates to the 3D camera correctly at any scale.

## Options considered

1. **Always render at native physical resolution** — sharp, but misses 60 FPS on base M4 at
   Retina with SSAO + shadows (estimated 4+ MP).
2. **Override the window scale factor to 1.0** — cheap, but blurs UI and text.
3. **Separate 3D render scale with native-resolution UI (chosen)**: render the world camera at
   `render_scale × physical size`, upscale to the window, draw egui at native resolution.

## Decision

- **Coordinate rule**: all UI layout, input handling and tool math use *logical* pixels;
  only render targets use physical pixels. egui `pixels_per_point = window.scale_factor() ×
  ui_scale_setting` (bevy_egui handles the OS factor; we multiply the user setting).
- **Render scale** setting: `Auto | 50–100 %` (steps 50/67/75/85/100). `Auto` picks the
  largest step with `physical_pixels × s² ≤ 3.7 MP` (≈ 2560 × 1440): 1080p/1440p → 100 %,
  13–14" Retina → 85 %, 4K → 67 %.
- **Implementation (baseline)**: the world `Camera3d` renders HDR → tonemapped into an
  offscreen `Image` sized `ceil(physical × s)`; a composite camera draws it full-screen with a
  bilinear upscale + `ContrastAdaptiveSharpening` when `s < 1`; egui renders on the window
  at native resolution. **Alternative under evaluation** (spike S6 in M1): Bevy 0.20's
  `MainPassResolutionOverride` (renders the main pass at a smaller physical resolution);
  adopted only if it works with SSAO, bloom, SMAA and our materials — recorded in PROGRESS.md.
- **Picking**: `picking::cursor_ray(window, world_camera)` converts the logical cursor
  position to normalized viewport coordinates, then to the world camera's render target
  coordinates before `Camera::viewport_to_world`. Unit tests cover scale factors
  {1.0, 1.25, 1.5, 2.0} × render scales {1.0, 0.85, 0.67, 0.5}: rays through the window
  center and corners must hit identical ground points.
- Screen-space sizes (problem icons, gizmo lines, selection outlines, light-pool fade
  distances) are defined in logical pixels and multiplied by the scale factor.
- Frame pacing: `PresentMode::AutoVsync`; frame cap setting 30 / 60 / 120 / unlimited
  (ProMotion displays default to 60 to save battery; 120 allowed). The sim is independent
  of frame rate (ADR-0006).
- Screenshot suites render at a fixed 1600 × 900 physical resolution with scale factor 1.0
  so images are comparable across machines.

## Consequences

- Sharp UI everywhere, controllable GPU cost on Retina/4K, correct picking at any DPI.
- One extra full-screen pass when `s < 1` (≈ 0.2 ms on M4).
