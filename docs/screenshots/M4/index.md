# M4 — zone types, special buildings, leveling visuals: screenshot review

Three suites in `crates/cb_game/src/debug/screenshot_suites.ron`, seed 1234, rendered with
`scripts/screenshot.sh <suite>`:

- `M4` — the demo town plus the scripted M4 features from `cb_sim::citygen::m4_features`
  (`paused: true`, so the construction site stays at 50 %): a residential row set to active /
  declining / abandoned / ruined plus a half-built house, an angled street (35°) with an L-shaped
  and a skewed polygon-mode plot and decorations opposite, and the auto-placed town hall.
- `M4gallery` — `scene: "gallery"`, the procgen gallery (`--procgen-gallery`): civic set A
  lineup, the R L1 → L5 progression row, one row per zone and level, and every vehicle type.
- `M4city` — `scene: "city10k"`, the mixed-zone citygen city (10 812 zoned + 1 civic building,
  1 705 segments, 3 410 plots).

Renderer: lavapipe (llvmpipe, LLVM 20.1.2, Vulkan, CPU), Mesa 25.2.8, Xvfb, scale factor 1,
1600 × 900, stored as full-resolution JPEG (quality 92); `metrics-<suite>.json` come from the
lossless captures. Gates (all 24 shots): magenta/NaN 0, pure black 0 %, clipped ≤ 0.021 %
(M4-07, UI text), holes 0.

| Shot | Camera (anchor, d, pitch, yaw) | Must be visible | Verdict |
|---|---|---|---|
| [M4-01](M4-01-r-progression.jpg) | gallery_0, 120 m, 35°, 190° | cottage → apartment block typology progression | ✅ L1 cottages, L2/L3 two-storey houses, L4 and L5 flat-roof blocks with roof plant along one frontage |
| [M4-02a](M4-02a-commercial.jpg) | gallery_7, 150 m, 35°, 160° | cool palette; glass fronts, signs | ✅ blue-grey walls, glass ground floors with coloured sign bands; L3 towers on podiums |
| [M4-02b](M4-02b-industrial.jpg) | gallery_10, 200 m, 35°, 160° | muted palette; halls, tanks, chimneys | ✅ brick and grey sawtooth halls, stacks and silo (L3), a tank between halls (planned d 150 widened to include the L3 row) |
| [M4-03](M4-03-civic-front.jpg), [03a](M4-03a-civic-front-left.jpg), [03b](M4-03b-civic-front-right.jpg) | gallery_civic, 260 / 170 m, 30° / 28°, 180° | distinct silhouettes (front) | ✅ landfill terraces, pump house, outlet, coal plant (cooling towers, stack, boiler house), diesel plant, town hall (U plan, 27 m tower), school, clinic (pavilion + ward block), fire station (low bay + hose tower), police (main block + wing, mast) |
| [M4-04](M4-04-civic-back.jpg), [04a](M4-04a-civic-back-left.jpg), [04b](M4-04b-civic-back-right.jpg) | gallery_civic, 260 / 170 m, 30° / 28°, 0° | distinct silhouettes (back) | ✅ same types read from behind; planned d 200 replaced by an overview at 260 m (the 470 m lineup does not fit) plus two halves at 170 m |
| [M4-05](M4-05-construction.jpg) | construction, 45 m, 30°, 0° | scaffold + vertical reveal | ✅ scaffold stands full height, the building is revealed to half its height inside it (fixed during review: the scaffold shared the building's slot and was clamped with it) |
| [M4-06](M4-06-conditions.jpg) | conditions, 70 m, 30°, 180° | active / declining / abandoned / ruined side by side | ✅ right to left: active block, declining house (desaturated), abandoned house (darker, boarded windows), ruin (wall stubs, rubble), then the construction site (look strengthened during review: boards read as plywood, not dark glass) |
| [M4-07](M4-07-gallery-ui.jpg) | gallery, 1 100 m, 62°, 0°, UI | gallery grid with legend | ✅ all rows and the legend (114 buildings); the vehicle row at the back is off-frame here and shown in 07b–d |
| [M4-07b](M4-07b-vehicles-cars.jpg), [07c](M4-07c-vehicles-services.jpg), [07d](M4-07d-vehicles-large.jpg) | gallery_vehicles, 48 / 42 / 62 m, 30°, 160° | vehicles per type | ✅ hatchback, sedan, SUV, pickup, van, box truck, fire truck (ladder, beacon), ambulance (stripe, light bar), police (band, light bar), garbage truck, 12 m bus, 3-section tram with pantograph |
| [M4-08](M4-08-city-overview.jpg) | town (city10k), 2 000 m, 60°, 0° | dense city, LOD2 readable, no popping | ✅ red-roof residential grid around a grey commercial core and an industrial band; far chunks on LOD1 masses read as the same buildings |
| [M4-09](M4-09-city-mid.jpg), [09b](M4-09b-city-residential.jpg), [09c](M4-09c-city-industrial.jpg) | town / c10k_res / c10k_ind, 400 m, 45°, 30° | zones distinguishable at a glance | ✅ commercial blocks grey-blue and tall, residential red roofs and gardens, industrial brown halls with stacks; hilly central blocks stay empty (lots too steep for a foundation) |
| [M4-10](M4-10-lots.jpg), [10b](M4-10b-lots.jpg) | m4_lots, 120 m, 50°, 180° / 0° | angled and L-shaped lots, footprints follow lot shapes | ✅ the L plot (shallow and deep part) and the skewed plot; fences and hedges follow the outlines, buildings fit their lots (d 80 widened to show both plots) |
| [M4-11](M4-11-plot-tool.jpg) | m4_lots, 120 m, 60°, 180°, UI, zoning, tool | plot tool with a hand-drawn plot and its handles | ✅ polygon mode ("Outline") active, the L plot selected with vertex and edge-midpoint handles, ± lot panel |
| [M4-12](M4-12-town-hall.jpg), [12b](M4-12b-town-hall.jpg) | town_hall, 90 m, 30°, 180° / 0° | auto-placed town hall | ✅ town hall on its parcel at the town edge, facing the street |

## Issues found and fixed during review

- Scaffold clamped by the reveal height (it used the building's object slot) → it now uses the
  neutral slot 0 and stands full height (`view::sync_buildings`).
- Condition look too subtle; boarded windows read as dark glass → declining 45 % grey × 0.86,
  abandoned 70 % grey × 0.70, boards weathered plywood (`world.wesl::condition_color`).
- Gallery: trees covered the vehicle row (wider scatter exclusion); the vehicle row in front
  pushed coal plant and landfill onto steeper ground (row moved to the back; the gallery now
  levels a taller terrace instead of dropping a civic type when its parcel exceeds the limit).

## Known issues (not blocking)

- city10k: the flattest 3 km site is still hilly in the centre; steep central blocks have roads
  but no buildings, so the commercial core is sparser than intended. Revisit with citygen
  grading or a flatter-site search when the 50k city lands (M9).
- Car body colour is baked into the vertex colours per seed; the pooled vehicle view (M6/M9)
  moves it to a colour index.
