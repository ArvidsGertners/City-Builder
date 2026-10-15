# ADR-0037: Platform support — Windows, Linux and macOS on Apple Silicon

- **Status:** Accepted (determinism paragraph superseded by ADR-0043)
- **Date:** 2026-10-15
- **Milestone:** M1 (build/CI), every milestone (verification)
- **Refines:** ADR-0001 (engine choice unchanged; platform scope widened)

## Context

The brief originally required Windows and Linux with macOS as best effort (RISKS Q7). macOS
on Apple Silicon (reference machine: Apple M4) is now a **required** platform. Apple Silicon
differs in GPU API (Metal via wgpu), unified memory, Retina/high-DPI displays, ProMotion
refresh rates, trackpad-centric input, Cmd-based shortcuts, per-OS file locations, app
bundles and Gatekeeper.

## Options considered

1. **Keep macOS best effort** — rejected by the user.
2. **Support macOS through the same code with a thin platform layer (chosen)** — Bevy/wgpu/
   winit/cpal already support macOS + Metal; differences are isolated in one module.
3. **Universal (x86_64 + arm64) macOS binaries** — not required; Intel Macs are out of scope.

## Decision

Required targets (all must build, pass clippy/tests in CI, and run):

| Platform | Target triple | GPU API | Minimum OS (assumption A-P1) |
|---|---|---|---|
| Windows 10/11 | `x86_64-pc-windows-msvc` | DX12 (Vulkan fallback) | Windows 10 22H2 |
| Linux | `x86_64-unknown-linux-gnu` | Vulkan (X11 + Wayland) | glibc 2.35 (Ubuntu 22.04) |
| macOS on Apple Silicon | `aarch64-apple-darwin` | Metal | macOS 14 Sonoma |

Rules:
- Platform-specific code lives only in `cb_game::platform` (paths, modifier mapping,
  gesture handling, window/fullscreen quirks, bundle detection). `#[cfg(target_os = …)]`
  appears nowhere else. Platform logic is written as pure functions with injected inputs
  (environment, OS enum) so its tests run on every CI host.
- GPU features: only wgpu features available on all three backends (storage buffers in
  vertex/fragment stages, compute for SSAO, Rgba16Float HDR targets, timestamp queries
  optional). No push constants, no multi-draw-indirect-count dependence (Bevy falls back).
  Metal-specific shader failures are caught by a GPU-free shader validation test (compose
  every WESL shader with `bevy_shader::ShaderCache` using a naga-based `load_module`, then
  translate to SPIR-V/HLSL/MSL); feasibility is proven in spike S6 (M1). Fallback if the
  spike fails: Linux lavapipe smoke test + informational macOS smoke job + the user's M4 run
  at visual milestones.
- Memory budget on Apple Silicon counts CPU + GPU together (unified memory): ≤ 3.5 GB
  process total on a 16 GB machine.
- Thread pools: Bevy task pools use all cores (P + E cores on M4); intra-system work is
  split into many small deterministic partitions (≥ 4 × cores) so efficiency cores don't
  become stragglers; partition → result merging stays in stable order (ADR-0017).
- Distribution on macOS: `CityBuilder.app` bundle (Info.plist: `CFBundleIdentifier`
  `org.citybuilder.CityBuilder` (assumption A-P2), `NSHighResolutionCapable = true`,
  `LSMinimumSystemVersion = 14.0`), ad-hoc code-signed, zipped. Not notarized in v1
  (needs an Apple Developer ID — assumption A-P3); the README documents first launch via
  right-click → Open.
- Input, file locations, high-DPI and CI are specified in ADR-0039, ADR-0040, ADR-0041 and
  ADR-0038.
- Determinism (ADR-0017) remains same-platform; aarch64 and x86_64 may differ in libm
  results. Rust does not contract `a*b+c` into FMA without explicit `mul_add`, so basic
  arithmetic stays identical.

## Consequences

- One code base, three platforms; macOS adds a CI runner and a bundling script.
- Real-GPU verification on Apple Silicon depends on the user's M4 at visual/perf
  milestones (GitHub's Apple Silicon runners have no reliable GPU for rendering tests).

## Amendments

- 2026-10-15 (plan review): the partitioning rule is replaced by ADR-0043 (`SIM_PARTITIONS = 64`,
  hardware-independent); the FMA sentence is complemented by ADR-0043's portable math rules
  (libm, no glam SIMD types in sim code). "No push constants" means *our* shaders. A quarantined
  `.app` started from Downloads runs under App Translocation; portable mode is ignored there
  with a notice (ADR-0040).
- 2026-10-15 (plan review): the sentence "Determinism (ADR-0017) remains same-platform" is
  superseded by ADR-0043 (bit-identical sim state across all three platforms, verified by the
  cross-OS CI job). Input, CI and high-DPI are now specified by ADR-0052, ADR-0054 and ADR-0053.
