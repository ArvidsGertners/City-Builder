# Phase S — spike evidence

## S-Render (2026-10-15)

lavapipe (llvmpipe, Vulkan, CPU), 1600 × 900, spike `spikes/s_render`. Not an art-direction
checkpoint (lighting is uncalibrated; that is M2).

| Image | What it proves | Metrics |
|---|---|---|
| `S-Render-01-lit.png` | `WorldMaterial` (ExtendedMaterial + WESL) with the ADR-0049 20-byte vertex layout; faceted shading from screen-space derivatives; CSM shadows cast by the custom-layout meshes; SSAO contact shading; baked vertex AO (darker wall bases); per-slot tints from a `ShaderBuffer`; SMAA, bloom, TonyMcMapface | 0 % pure black, 0 magenta, 0 shader errors |
| `S-Render-02-prepass-normals.png` | The normal prepass (what SSAO reads) receives the derivative normals: one constant colour per face | within-face normal variance 0.00019 (gate ≤ 0.02) |
| `S-Render-03-composite-2_3.png` | Render scale 2/3 via offscreen target + composite camera (softer, correctly framed) | mean abs. difference to native 0.47/255 |
| `S-Render-04-focus-fog.png` | Focus-distance fog `1 − exp(−d·max(0,|p−focus|−r0))` at an exaggerated density (0.04/m, r0 10 m): the house at the focus keeps its colour, others haze | — |

`MainPassResolutionOverride` at 2/3 rendered **bit-identical** to native: it is a render-world
hook for upscalers (DLSS) and needs an upscale node; the composite path is used (ADR-0053
amendment).

## S-J (2026-10-15)

SVG dumps of canonical cases from `spikes/s_j` (ADR-0063), rendered with headless Chromium.
Junction surface polygon, cut lines, crosswalks, lane connectors and conflict zones.

| Image | What it shows |
|---|---|
| `S-J-01-X.png` | Street X-junction: simple polygon, connectors for every allowed movement, no same-lane crossings |
| `S-J-02-5-way.png` | 5-way junction with freeform angles: angle-aware corner allowance, valid polygon |
| `S-J-03-avenue-x-avenue.png` | Avenue × avenue: multi-lane monotone lane assignment, conflict zones |
| `S-J-04-tram-turn-on-avenue-median.png` | Tram turn on the avenue median within the tram radius limit |
| `S-J-05-roundabout-ring-node.png` | Roundabout ring node |
| `S-J-06-8-slope.png` | Junction on an 8 % slope: per-vertex surface, cut-line step 0.0 mm |

## S-P (2026-10-15)

SVG contact sheets from `spikes/s_p` (ADR-0064), rendered with headless Chromium. Grey band =
road; coloured polygons = lots (orange = corner lot); dashed = setback inset; brown = R L1–L3
footprint (largest inscribed rectangle clamped to 16 × 16 m at the front); red dashed = seam
(medial axis of a fillet/kink). Labels: rectangularity / fan (red if < 0.55 or fan > 25°).

| Image | What it proves | Self-review |
|---|---|---|
| `S-P-01-showcase.png` | 24-plot showcase: straight R/C/I, convex r 30–300, concave r 25–300, S-curves, corners 60–120°, kinks ±15–35° | Lots follow curves and face the road; corner lots front both roads (4/4); kinks split at the bend; fans ≤ 25° except the tightest curves (r 25/30 m, max 27°); every R lot but the narrowest concave pies has a footprint |
| `S-P-02-random.png` | First 24 random plots | No slivers, overlaps or landlocked lots; deep plots keep backyards; I lots on r ≈ 40 m fan 33–37° (w_min 24 m — inherent, see ADR-0064) |
| `S-P-03-worst.png` | 24 worst random plots (orphan pieces, merges, lowest rectangularity) — all corner plots | Acute deep corners give pie/trapezoid lots along the seam (rect ≥ 0.6); orphans come from the generator's corridor clip notches and merge cleanly; nothing invalid |

## S-G (2026-10-15)

lavapipe, 1600 × 900 (stored at 1280 × 720), spike `spikes/s_g` (ADR-0065). Terrain chunks pick
LOD by distance (0 < 350 m, 1 < 900 m, 2 < 2 km, else 3) unless a shot forces a LOD within 700 m.
Placeholder colours (not art direction): grass, brown = carved/stamped corridor, grey = steep
cut/fill, roads with 2 m walks, concrete decks and piers.

| Image | What it proves | Self-review |
|---|---|---|
| `S-G-01-river-bridge.png` | River bridge: span classified over water, deck clearance, smooth embankments | Deck continuous with the road; banks smooth; no terrain through the road |
| `S-G-02-river-bridge-abutment.png` | Abutment and soft end cap at the span start | Embankment wraps the abutment; no wall, no terrain above the deck |
| `S-G-03-valley-bridge.png` | Two viaducts over the 30 m valley; piers evenly spaced | Piers stand on the valley sides, ends meet the ground cleanly |
| `S-G-04-valley-bridge-lod1.png` | Same area with LOD1 forced within 700 m | No terrain poking through roads at LOD1; zig-zag band on the ridge is vertex-colour aliasing of the slope threshold (cosmetic, spike colouring) |
| `S-G-05-overpass.png` | Avenue over a street: span over the lower corridor, ramps at max grade | Lower road untouched; piers outside its corridor |
| `S-G-06-under-overpass.png` | Driver's view along the lower street under the overpass | Clearance visible; piers clear of the walks |
| `S-G-07-junction-slope.png` | 4-way junction on a sloped hillside (LOD0) | Approaches meet the junction surface without steps; terrain below everywhere |
| `S-G-08-junction-slope-lod1.png` | 3-way junction on a slope with LOD1 forced | Still no terrain through the junction or the approaches |
| `S-G-09-parallel-pair.png` | Two parallel roads 9–19 m apart at different heights | Continuous slope between them (max-fill/min-cut), no cliff |
| `S-G-10-parallel-pair-lod2.png` | Pair with LOD2 forced | Coarse terrain stays under both roads; small dark edge gaps hidden by skirts |
| `S-G-11-hillside-cut-fill.png` | Road with the deepest cut in the random set | 1:1 cut slope (grey) blends smoothly into the hill |
| `S-G-12-aerial-lod-rings.png` | Whole map with distance LOD rings, river, junctions, bridges | No cracks between LOD rings (skirts), roads visible at all distances |
