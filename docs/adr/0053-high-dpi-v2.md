# ADR-0053: High-DPI displays and render scale v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (spike S-Render), M2 (implementation), M15 (settings UI)
- **Supersedes:** ADR-0041 (keeps logical-pixel rule, native-resolution UI, offscreen world
  render + composite, DPI-aware picking with tests)

## Context

Review findings D6, G-W3, G-W7, B10, B12: the Auto rule targeted 3.7 MP rendered pixels for an
M4 base GPU with roughly 0.36× the memory bandwidth of the PC reference; Auto arithmetic was
wrong for the 14" MacBook Pro and 4K; the frame cap had no mechanism (no limiter in Bevy;
`bevy_framepace` targets Bevy 0.19); the composite pass needed a concrete mechanism with
sprites/bevy_ui disabled.

## Decision

- **Render scale steps** are exact fractions `{1, 5/6, 3/4, 2/3, 3/5, 1/2}`. `Auto` picks the
  largest step whose rendered pixel count is ≤ the GPU-class target: **2.2 MP** for integrated
  GPUs (incl. Apple M-series base; wgpu `DeviceType::IntegratedGpu`), **3.7 MP** for discrete
  GPUs. Examples: MBA 13" (4.26 MP) → 2/3; MBP 14" (5.94 MP) → 3/5; 1080p → 1; 4K discrete
  → 2/3 (3.69 MP).
- Apple M-series default graphics preset: **Medium** (2 cascades, half-res SSAO, 4 bloom mips).
- **Composite**: the world `Camera3d` renders into an offscreen HDR target; an orthographic
  composite camera (order 1, own `RenderLayers`) draws a full-screen triangle mesh with a small
  unlit upscale material (bilinear + CAS when scale < 1); egui renders on the composite camera.
  Spike S-Render compares this with `MainPassResolutionOverride` and records the choice.
- **Frame limiter** in `cb_game::platform::window`: precise sleep in `Last` against the target
  frame time; `PresentMode::AutoNoVsync` when the cap is below the refresh rate; caps 30/60/120/
  unlimited, default 60 (ProMotion included).
- **Display modes**: windowed (sizes: 1280×720, 1600×900, 1920×1080, 2560×1440, or current),
  borderless fullscreen (all platforms; native macOS fullscreen space), exclusive fullscreen
  (Windows/Linux).

## Consequences

- Achievable GPU budgets on Apple Silicon; frame caps that work; defined composite mechanism.
