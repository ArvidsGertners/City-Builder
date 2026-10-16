# ADR-0058: Verification process for a single GPU-less agent

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (tooling), every milestone

## Context

The developer is one AI agent in a Linux container without a GPU (lavapipe software Vulkan
under Xvfb). The user owns a Windows/Linux GPU PC and an Apple M4 Mac and asked that the
agent implement all milestones without stopping, verify visuals itself, and prove risky
approaches with throwaway spikes first. Review findings F-0, G-P1–G-P8, A37, A38, A50, G-X1
showed gates depending on user hardware, meaningless CPU numbers under lavapipe, missing shot
lists, no estimates and no scope-cut rule.

## Decision

- **Phase S (risk spikes)** between M1 and M2: S-T (traffic at scale), S-J (junction
  geometry), S-P (plots → lots → buildings), S-Time (time model and trip scale), S-G
  (road–terrain interface incl. bridges), S-Render (material pipeline, derivative normals,
  custom vertex layout, render scale, GPU-free shader validation). Each has scope, test cases,
  pass/fail numbers, a time-box and a fallback in PROGRESS.md. Spike code lives in `spikes/`,
  is kept on a `spike/<name>` tag after removal, and outcomes are logged (superseding ADR if
  the approach changes).
- **Status ☑ᵤ "done, pending user verification"**: user runs (real GPU smoke/benchmark/
  screenshots, trackpad gestures, ⌘Q, listening pass) never block progress. Requests are
  batched in the **User verification queue** in PROGRESS.md (item, milestone, exact command,
  expected result, status); reported discrepancies become bugs.
- **CPU budgets in the container** use `--benchmark <scene> --no-gpu-work` (64 × 64 target,
  shadows/SSAO off) so lavapipe rasterization doesn't pollute main/render-thread numbers. GPU
  budgets come only from user runs.
- **Objective visual checks** accompany self-review: `--screenshot-metrics` (luminance
  histogram, clipped %, pure-black %, magenta/NaN pixels) and a **semantic pass** rendering
  each shot with flat class colors (terrain, water, road, R, C, I, service, vehicle, tree,
  UI) for per-shot assertions (e.g. "vehicle pixels ≥ 1.5 %") and class separation in the
  beauty pass (mean-color ΔE00 ≥ 12). Captures wait for zero pending mesh jobs and pipelines,
  ≥ 30 settled frames and finished crossfades. Times of day use keyframe times; exposure is
  the director's.
- **Objective audio checks**: loudness per class (±1 LU of targets), true peak ≤ −1 dBTP,
  seamless loops, no DC/NaN, spectral centroid ranges, plus spectrogram PNGs for self-review.
- **Estimates**: every milestone and spike carries an estimate in agent-days and the actual;
  when a milestone exceeds 1.5× its estimate the **scope-cut ladder** in PROGRESS.md is applied
  in order (and logged).
- PROGRESS.md must be updated in the same commit as any milestone-status change (skill step).

## Consequences

- Continuous progress without waiting on the user; measurable quality gates; early detection
  of runaway scope.

## Amendments

- 2026-10-16 (M2, user decision): the container's main thread is not a usable proxy for the
  whole frame — on the 4-core llvmpipe container the engine alone (`--benchmark empty`: terrain
  and scatter, no town) measures p95 11.2 ms. Container CPU criteria therefore budget the
  **game's own sets** (`input + sim-drive + view-sync + present` from the benchmark's per-set
  report) at ≤ 4 ms p95 combined, and record the `empty` baseline next to every result; the
  full main-thread and render-thread budgets (PERFORMANCE §3) are verified on the user's PC and
  M4 (☑ᵤ, User verification queue).
