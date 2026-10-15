# ADR-0004: Lighting pipeline

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (static), M11 (cycle), M12 (weather)

## Context

Need soft sun shadows, ambient + AO so low-poly shapes read, sky gradient, distance fog,
water shading, readable stylized nights, and everything driven by day/night and weather.
Thousands of street lamps and headlights at night. Mid-range 2019 GPU.

## Options considered

1. **Bevy `Atmosphere` + physical sky** — beautiful, but physically dark nights and limited
   art direction of sunset/blue-night palettes; extra cost.
2. **Custom stylized sky + palette-driven lights (chosen)** — full art control via keyframes.
3. Real point lights for all lamps (clustered forward / deferred) — thousands of lights,
   too costly and visually noisy.
4. MSAA vs post-AA: Bevy's SSAO requires `Msaa::Off` (verified in 0.20 source).

## Decision

- **HDR camera**, `Tonemapping::TonyMcMapface` (soft highlight rolloff, stable hues),
  low-intensity `Bloom` for emissives, exposure driven by the lighting director.
- **One shadow-casting `DirectionalLight`** acting as sun by day and moon by night
  (crossfade through twilight); `CascadeShadowConfig` 3 cascades (Low: 2), max distance
  tied to camera zoom; `ShadowFilteringMethod::Gaussian` for soft, stable edges.
- **Ambient occlusion**: always-on baked vertex AO in procgen meshes, plus
  `ScreenSpaceAmbientOcclusion` (quality by preset) and `ContactShadows` on High/Ultra.
  Requires depth + normal prepass; anti-aliasing is **SMAA** (FXAA on Low, TAA optional on
  Ultra). `Msaa::Off`.
- **Sky**: custom `SkyMaterial` on a camera-centered dome: zenith/horizon gradient, sun and
  moon disks, stars at night, cloud layer coverage from weather.
- **Indirect light**: procedural environment cubemap (6 × 64² from sky colors, generated on
  the async pool when sky colors change noticeably) fed to `GeneratedEnvironmentMapLight`
  for diffuse/specular; small `AmbientLight` floor.
- **Fog**: `DistanceFog` (atmospheric falloff, directional inscattering toward the sun),
  colors/density from the lighting state; weather fog increases density; terrain/building
  shaders add height fog for the Fog weather.
- **Night lights without real lights**: emissive bulbs/windows/headlights + additive
  `LightPoolMaterial` decals (soft radial falloff) on roads and sidewalks under lamps and
  in front of vehicles. Ultra may enable up to 32 real point lights for the nearest lamps.
- **Cloud shadows**: world-space scrolling noise in world/terrain shaders modulates final
  lighting (multiplicative, strength from cloud coverage).
- Water: depth-based color (shallow → deep), shoreline foam from depth prepass difference,
  fresnel blend to sky color, sun specular; no SSR.
- All parameters come from `LightingState` computed by the director (ALGORITHMS §25)
  from keyframes in ART_DIRECTION.md.

## Consequences

- Full art control and predictable cost; night stays readable by design.
- MSAA unavailable with SSAO; SMAA quality is adequate for flat-shaded geometry.
- Absolute light units (lux/EV) are calibrated once in M2 against a reference screenshot.
