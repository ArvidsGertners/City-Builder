# ADR-0050: Lighting pipeline v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (static), M11 (cycle), M12 (weather)
- **Supersedes:** ADR-0004 (keeps HDR + TonyMcMapface, bloom, one shadow-casting directional
  light for sun and moon, Gaussian-filtered CSM, SSAO + baked AO, contact shadows on High+,
  SMAA with `Msaa::Off`, procedural sky dome, procedural environment map, fake light pools
  instead of real lamp lights, cloud shadows)

## Context

Review findings G-W8, G-W9, G-C7, D13, A37: camera-distance `DistanceFog` cannot fade the far
landscape at 2.4 km zoom without erasing the city; sampling the depth prepass for water foam in
a transparent pass is unverified in Bevy 0.20; night ambient × exposure offset was brighter than
noon; environment-map regeneration could spike every few seconds at 4×; light calibration
had no measurable target.

## Decision

- **Fog**: custom fog in World/Terrain/Water materials driven by distance to the camera
  *focus* (`fog = 1 − exp(−density × max(0, |p − focus| − r0))`, `r0` grows with zoom) plus
  height fog for Fog weather; `DistanceFog` is used only for the sky dome horizon.
- **Water**: depth and shoreline foam from a per-vertex `water_depth` attribute baked into the
  water mesh (water level − terrain height); no depth-prepass sampling.
- **Light units**: `sun_rel`/`amb_rel` keyframe values are **pre-exposure**. Night exposure
  offsets are reduced to +1.0 … +1.3 EV with night `amb_rel` ≈ 0.14 so emissive windows read
  brighter than ambient; ART_DIRECTION §3 holds the corrected table.
- Environment map regeneration at most once per 2 real seconds (and on time jumps).
- **Calibration metrics** (from `--screenshot-metrics`): noon median post-tonemap luminance
  0.18–0.24; lit grass within ΔE00 < 6 of #8DB06A; shadow/lit ratio 0.35–0.50; clipped pixels
  ≤ 0.3 %; night mean luminance 0.30–0.45 × noon; zone class ΔE00 ≥ 10 at night.

## Consequences

- Distant landscape fades without hiding the city; foam is deterministic and cheap; lighting
  quality is checkable by the agent without human review.

## Amendments

- 2026-10-15 (spike S-Render): focus fog is applied after `apply_pbr_lighting`, whose output is
  pre-exposed scene luminance; the fog colour is therefore specified as scene luminance
  (≈ albedo × illuminance / π) and multiplied by `view.exposure`, never as a 0–1 display colour.
