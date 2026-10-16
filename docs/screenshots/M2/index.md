# M2 — vertical slice screenshot suite

Suite `M2` (`crates/cb_game/src/debug/screenshot_suites.ron`), seed 1234, rendered with
`scripts/screenshot.sh M2`. Scene: the demo town (4 × 3 street grid, 240 houses) plus the
scripted M2 features from `cb_sim::citygen::m2_features`: a curved street on an 8 % slope with
a side street and a corner plot, a crest crossing (cut 6.1 m), a dip crossing (fill 6.7 m, houses
on its far side only), a street to the lake shore — 53 more houses. Shots are framed relative to
named anchors (`SceneFeatures`).

- Renderer: lavapipe (llvmpipe, LLVM 20.1.2, Vulkan, CPU), Mesa 25.2.8, Xvfb, scale factor 1,
  1600 × 900. Stored as full-resolution JPEG (quality 92) to keep the repository small;
  `metrics.json` was computed from the lossless captures.
- Gates (all shots): magenta/NaN 0 (the two crack-check shots use a magenta background on
  purpose), pure black 0 %, clipped ≤ 0.01 %, crack-check holes 0.
- Calibration (ADR-0050 + M2 amendment): lit grass ΔE00 3.1 to #8DB06A; noon mean/median
  luminance 0.32/0.38 (M2-02); night/noon mean 0.31 (M2-11 vs M2-02).

| Shot | Camera (anchor, d, pitch, yaw) | Must be visible | Verdict |
|---|---|---|---|
| [M2-01](M2-01-overview.jpg) | map centre, 3 600 m, 62°, 0° | river, lake, hills with rock in steep valleys, forest masses, sand banks; haze at the edges, none over the town | ✅ forest masses read as speckle at this distance (far trees are 4–8-triangle cards) |
| [M2-02](M2-02-neighbourhood.jpg) | slope, 300 m, 45°, 20°, zoning view | curved street, plots with lot lines, houses facing the frontage | ✅ zoning fills are subtle at 300 m (35 % on grass), outlines and lot lines visible |
| [M2-03](M2-03-block-yaw0.jpg) | slope, 120 m, 35°, 0° | yards (fence, shed, garden, driveway), varied roofs, no house across a lot line | ✅ |
| [M2-04](M2-04-block-yaw120.jpg) | slope, 120 m, 35°, 120° | same block from 120° | ✅ junction ground clipping (user report) fixed |
| [M2-05](M2-05-block-yaw240.jpg) | slope, 120 m, 35°, 240° | same block from 240° | ✅ |
| [M2-06](M2-06-fill.jpg) | fill, 60 m, 20°, 165° | smooth embankment, no terrain through the road, skirts invisible, house on plinth/stilts | ✅ stilt and plinth houses on the fill; the 1:1.5 graded slope reads as a gentle bank |
| [M2-07](M2-07-cut.jpg) | cut, 40 m, 20°, 0° | cut face with dirt/rock material, clean road edges | ✅ |
| [M2-08](M2-08-shore.jpg) | shore, 80 m, 30°, 30° | depth gradient, foam band, sky fresnel, sand | ✅ |
| [M2-09](M2-09-morning.jpg) | M2-02 framing, 08:30 | long soft morning shadows | ✅ |
| [M2-10](M2-10-sunset.jpg) | M2-02 framing, 19:00 | warm sunset | ✅ (sunset offset raised to +1.4 EV) |
| [M2-11](M2-11-night.jpg) | M2-02 framing, 23:00 | blue night with ambient floor, houses readable | ✅ readable; the grass keeps a green cast (no night colour grade until M11) |
| [M2-12](M2-12-road-tool.jpg) | steep, 150 m, 40°, 0°, UI | invalid ghost (striped coral) with "too steep" and live cost | ✅ |
| [M2-13](M2-13-plot-tool.jpg) | corner, 120 m, 50°, 30°, UI | plot handles, depth handle, lot ± control, subdivision lines | ✅ |
| [M2-14](M2-14-lod-band.jpg) | town, 360 m, 25°, 200° | no holes or popping at LOD bands | ✅ |
| [M2-14b](M2-14b-lod-crack-check.jpg) | as M2-14, magenta background, no sky | 0 background pixels inside the terrain silhouette | ✅ holes 0 |
| [M2-14c](M2-14c-lod-crack-check-wide.jpg) | map, 3 600 m, 30°, 45°, magenta background | all LODs and merged far blocks: no cracks | ✅ holes 0 |

## Found and fixed during review

- Ground poking through junction surfaces (user report on M2-04; also M2-03, M2-13): junction
  terrain stamps (ADR-0065 amendment), test `junction_surface_above_terrain`.
- Zoning overlay and road ghost invisible: draped meshes are counter-clockwise in (x, z), i.e.
  facing down; culling disabled for them.
- Corner plot without houses: the scripted scene skipped growth on lots created before its
  snapshot; test `m2_scripted_lots_all_grow`.
- Sky dome and clear colour too dark/grey (unlit, not exposure-scaled); dome now scaled by
  `FOG_LUMINANCE × exposure` and continued below the horizon (space beyond the map edge reads
  as haze).
- Lighting calibration (exposure, night, sunset) per ADR-0050 amendment.
- Overview showed no river (framed on the town) → framed on the map centre; fill shot was
  blocked by trees and had no houses → new framing and one-sided plots on the fill crossing.
- Magenta detection missed tonemapped magenta (pure #FF00FF renders ≈ (198, 75, 195)) →
  hue-based test for the missing-material gate and the crack check.

## Known issues (accepted for M2)

- Faint radial shading fan on junction asphalt (large flat triangles; PROGRESS Known bugs).
- Night keeps a green cast; night colour grading and emissive windows arrive in M11.
- Zoning fills are hard to see from far away; patterns (C/I) and stronger outlines come with
  the other zones in M4.
- lavapipe renders may differ slightly from GPUs; the user's PC/M4 run is UV-2.
