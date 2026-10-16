# M3 — full road system screenshot suite

Suite `M3` (`crates/cb_game/src/debug/screenshot_suites.ron`), seed 1234, rendered with
`scripts/screenshot.sh M3`. Scene: the M2 starting town plus the scripted M3 features from
`cb_sim::citygen::m3_features`, each on its own site: a river bridge, a valley viaduct with a
street under it (the overpass), a curved cobblestone avenue with every decoration, an avenue ×
street X with traffic lights, a Y at 30°, a 5-way junction, a one-way pair with a dirt path, and
a roundabout (r 30 m, four street arms). Shots are framed relative to named anchors
(`SceneFeatures`).

- Renderer: lavapipe (llvmpipe, LLVM 20.1.2, Vulkan, CPU), Mesa 25.2.8, Xvfb, scale factor 1,
  1600 × 900, stored as full-resolution JPEG (quality 92); `metrics.json` comes from the lossless
  captures.
- Gates (all 18 shots): magenta/NaN 0, pure black 0 %, clipped ≤ 0.01 %, holes 0.

| Shot | Camera (anchor, d, pitch, yaw) | Must be visible | Verdict |
|---|---|---|---|
| [M3-01](M3-01-t-junction.jpg) | corner, 60 m, 40°, 0° | cut-back corners, fillets, crosswalks, lines behind crosswalks, lane arrows | ✅ the yielding side street has the dashed give-way line with shark teeth; the priority road carries none (control-aware markings, fixed during review) |
| [M3-02](M3-02-lights-x.jpg) | lights_x, 80 m, 40°, 30° | median opening, signal heads, turn arrows | ✅ solid stop lines carry the signal poles on all four arms |
| [M3-03](M3-03-y30.jpg) | y30, 60 m, 40°, 0° | no overlapping meshes, clean acute corner | ✅ |
| [M3-04](M3-04-five-way.jpg) | five_way, 80 m, 45°, 0° | simple polygon, crosswalks on all arms | ✅ two opposite arms priority, three give way |
| [M3-05](M3-05-slope-junction.jpg) | corner, 50 m, 18°, 90° | no steps between approaches and junction surface | ✅ reframed (the planned d 40 / 15° / 60° view was blocked by a house) |
| [M3-06](M3-06-river-bridge.jpg) | river_bridge, 200 m, 25°, 20° | deck, railings, piers in water, abutments meeting embankments | ✅ |
| [M3-07](M3-07-valley-bridge.jpg) | valley_bridge, 250 m, 15°, 10° | tapered tall piers, smooth deck profile | ✅ |
| [M3-08](M3-08-overpass.jpg) | overpass, 80 m, 20°, 60° | lower road unaffected, clearance visible | ✅ the street runs under the viaduct; the lower corridor is untouched |
| [M3-09](M3-09-decorated-avenue.jpg) | decorated, 50 m, 40°, 20° | even spacing on a curve; lamps upright on slope | ✅ cobblestone surface, grass strips, tree rows in the strips, alternating lamps |
| [M3-10](M3-10-lane-overlay.jpg) | five_way, 60 m, 60°, 0°, UI, lanes | all connectors drawn, zones distinct | ✅ lanes blue, connectors green/orange, conflict zones red |
| [M3-11](M3-11-one-way-dirt.jpg) | one_way, 150 m, 55°, 0°, focus +70 m z | arrows; dirt path material | ✅ reframed so the dirt path (brown) is in view; one-way arrows on both streets |
| [M3-12](M3-12-bulldoze-dialog.jpg) | slope, 160 m, 50°, 20°, UI | three choices, dependents highlighted | ✅ |
| [M3-13](M3-13-lights-yaw0.jpg) / [14](M3-14-lights-yaw120.jpg) / [15](M3-15-lights-yaw240.jpg) | lights_x at yaw 0/120/240 | consistent from all sides | ✅ |
| [M3-16](M3-16-roundabout.jpg) | roundabout, 80 m, 50°, 0° | ring, yield markings on entries, splitter islands | ✅ fixed during review: the first render had crosswalks, stop lines and arrows on the ring itself and no islands |
| [M3-17](M3-17-edit-upgrade.jpg) | y30, 70 m, 45°, 0°, UI, edit tool | segment highlight, cost label, mode bar | ✅ added with the Edit road tool (M3.8) |
| [M3-18](M3-18-roundabout-tool.jpg) | y30, 110 m, 55°, 0°, UI, road tool | ring ghost, radius and cost label | ✅ added with the roundabout preset (M3.8) |

Also fixed during review: a faint radial "sunburst" on junction surfaces came from the painter's
per-face colour jitter on the many fan triangles; junction carriageways are now painted without
jitter (`Painter::face_exact`).
