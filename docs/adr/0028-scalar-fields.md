# ADR-0028: Scalar fields (land value, pollution, noise) as internal rasters

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M5

## Context

Land value, ground pollution, noise and water pollution vary continuously over the map,
diffuse, and need fast sampling at arbitrary positions plus overlay rendering. The brief
forbids grids for placement, not for internal computation.

## Options considered

1. **Per-object values computed by summing nearby sources (spatial queries)** — exact at
   objects, no diffusion, expensive with many sources, overlays need interpolation anyway.
2. **Internal raster fields with bilinear sampling (chosen)**.

## Decision

- `ScalarField`: 512 × 512 cells (8 m) per field, f32, bilinear sampling at any continuous
  position, updated in slices (ALGORITHMS §19). Uploaded to the GPU as textures for
  overlays and sampled smoothly with contour lines/hatching.
- Fields never constrain placement; nothing snaps to them; they are invisible except as
  smooth overlays.

## Consequences

- O(1) sampling, natural diffusion, cheap overlays. 8 m resolution is well below lot sizes.

## Amendments

- 2026-10-15 (plan review): diffusion fields are double-buffered (read A, write B, swap after
  the last slice of the hourly update) to avoid slice seams; every field update has a slot in
  the sub-rate phase table (ADR-0042). Ground/water pollution and land-value smoothing are
  saved (ADR-0044); noise is recomputed. Noise attenuation: line sources (roads)
  `L0 − 10·log10(d/d0)`, point sources `L0 − 20·log10(d/d0)`.
