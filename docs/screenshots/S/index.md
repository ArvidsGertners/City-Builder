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
