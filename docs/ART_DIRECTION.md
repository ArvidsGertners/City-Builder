# Art Direction

Status: **Accepted (Phase 1 plan)**. All colors are sRGB hex; shaders convert to linear.
Every value here is data in `cb_procgen::palette` / `cb_game::lighting::keyframes`; tune
there and update this doc. Screenshots for review are produced with `scripts/screenshot.sh`
(fixed seed, fixed camera set, keyframe times 08:30/12:00/19:00/23:00, each weather; exposure from
the lighting director, ADR-0058).

## 1. Pillars

1. **Soft, warm, tactile** — a model-railway/diorama town: rounded proportions, gentle
   slopes, warm light, nothing harsh. Townscaper's charm, Islanders' clarity, but buildings
   proportioned like a real central-European town so it reads as a plausible place.
2. **Readability first** — at any zoom, time and weather, the player can tell roads from
   plots, R from C from I, services from zoned buildings, vehicles from scenery.
   Silhouette and value (lightness) contrast carry identity; hue is secondary.
3. **Low-poly, flat-shaded, untextured** — faceted geometry with flat or vertex-gradient
   colors. Detail comes from shape, not from texture. No outlines.
4. **Coherent, slightly desaturated palettes** — no pure saturated primaries; chroma kept in
   a band (OKLCH C ≈ 0.04–0.12) except signals (traffic lights, sirens, UI alerts).
5. **Lighting does the mood** — color temperature and fog carry time of day, season and
   weather; materials stay simple so lighting reads.

## 2. Geometry and material style

| Aspect | Rule |
|---|---|
| Shading | Buildings, props, vehicles: flat normals (per-face). Terrain: smooth normals with slight color posterization by slope/height bands (4–6 bands) so it reads low-poly without noisy facets. Water: flat with animated soft normals. |
| Polycount | Houses 300–1 200 tris LOD0, apartment blocks ≤ 3 000, vehicles 150–400, trees 60–180, lamps 40. |
| Bevels | Corners of large volumes get a single small chamfer (0.1–0.2 m) to catch light. |
| Roughness/metal | All matte: roughness 0.85–0.95, metallic 0 except glass (roughness 0.25, reflective via env map), water, wet roads (roughness down to 0.35). |
| Vertex AO | Baked per vertex in procgen: ground contact darkening (−18 % over 0.6 m), inner corners and under eaves (−10 %). Always on (cheap, works without SSAO). |
| Color variation | Per building: HSV jitter (±3° hue, ±4 % saturation, ±5 % value) within its palette entry; per-face ±2 % value noise to avoid flat monotony. |
| Edges/outlines | None. Separation via value contrast and AO. |
| Emissive | Windows at night, lamp bulbs, vehicle lights, signs, transit vehicle interiors. Bloom picks them up softly. |

## 3. Lighting moods (time of day)

Keyframes (linear interpolation in OKLab between keys, cubic in time). Intensities are
relative to noon (`sun_rel`, `amb_rel`); the absolute calibration (lux/EV) is fixed in M2
by matching the noon reference screenshot, then all moods scale from it.

| Time | Sun color | sun_rel | Ambient color | amb_rel | Sky zenith | Sky horizon | Fog color | Exposure offset |
|---|---|---|---|---|---|---|---|---|
| 00:00 night | moon #9DB2DF | 0.10 | #3A4F80 | 0.14 | #142039 | #2E4170 | #263760 | +1.2 EV |
| 05:00 blue hour | moon→sun #A9B6E0 | 0.08 | #55618F | 0.16 | #24325A | #8C7FA3 | #6C6A8E | +1.0 EV |
| 06:30 sunrise | #FFB27A | 0.45 | #A88B9A | 0.45 | #5F7FB8 | #FFC594 | #E8B79A | +0.8 EV |
| 08:30 morning | #FFE2BF | 0.85 | #B5C4D6 | 0.75 | #6F9ED6 | #CFE0EE | #D7E2EA | 0 |
| 12:00 noon | #FFF3E2 | 1.00 | #BFD0E2 | 0.80 | #6A9FDC | #D6E6F2 | #DCE7EE | 0 |
| 16:30 afternoon | #FFE4C2 | 0.90 | #C3C7D3 | 0.75 | #6F9AD0 | #E3E1DC | #E2DED6 | 0 |
| 19:00 sunset | #FF9E5E | 0.50 | #B48A8A | 0.50 | #4F68A8 | #FFAE78 | #E9A882 | +0.6 EV |
| 20:00 dusk | #C88AA0 | 0.12 | #6A6A98 | 0.18 | #2A3766 | #B98AA0 | #7E7398 | +0.9 EV |
| 22:00 night | moon #9DB2DF | 0.10 | #3A4F80 | 0.14 | #142039 | #2E4170 | #263760 | +1.2 EV |

`sun_rel`/`amb_rel` are **pre-exposure** (ADR-0050): effective = value × 2^EV. Night effective
ambient = 0.14 × 2^1.2 ≈ 0.32, i.e. ≈ 40 % of noon ambient (0.80), so emissive windows and lamps
(also scaled by exposure) clearly out-shine the ambient.
Night rules: never pitch black — minimum effective ambient 0.30; the scene reads as
"day-for-night" blue. Windows `#FFC877`, street lamps `#FFD59A` with soft warm pools,
headlights `#FFF2D6`, tail lights `#E0584A` (desaturated red), transit interiors `#FFE7B0`.
Seasonal sun: winter noon `sun_rel × 0.85`, color cooler (#FFF6EC), lower sun elevation;
summer warmer (#FFF0D8).

Shadows: one shadow-casting directional light (sun or moon), soft (Gaussian PCF), shadow
color lifted by ambient — shadows are never darker than ~35 % of lit value at noon.

## 4. Weather moods (modifiers applied on top of time of day)

| Weather | sun_rel × | Ambient shift | Sky tint (mix) | Fog density × | Extras |
|---|---|---|---|---|---|
| Clear | 1.0 | — | — | 1.0 | crisp shadows, light cloud shadows drifting |
| Cloudy | 0.45 | +0.15 cooler grey | #A9B6C2 @ 60 % | 1.4 | large slow cloud shadows, softer contrast |
| Rain | 0.28 | +0.20 blue-grey | #8E9AA6 @ 75 % | 2.2 | rain streaks, wet roads (darker, glossy), puddle mask, ripples on water |
| Thunderstorm | 0.18 | +0.20 slate | #5F6B78 @ 85 % | 2.6 | heavy rain, lightning (sky flash + bolt; reduced in accessibility mode), wind sway ×2 |
| Fog | 0.50 | +0.25 neutral | #C9CFD3 @ 70 % | 6.0 (height fog) | silhouettes fade by 250–400 m; lamps glow in fog at night |
| Snow | 0.55 | +0.30 cool white | #C7D0D8 @ 70 % | 2.0 | snowflakes, snow accumulation on roofs/terrain/trees (normal-up mask), roads partly cleared |

Readability in bad weather: zone/road/vehicle value contrasts are preserved by limiting fog
to distant terrain (focus-distance fog starting ≥ 120 m from the camera focus, ADR-0050) and by boosting emissive/vehicle
lights slightly in rain/fog/snow.

## 5. Palettes

### Terrain and nature (summer base; season tints in §5.6)

| Element | Colors |
|---|---|
| Grass | #8DB06A base, variation #9CBB72 / #7FA35E |
| Meadow flowers (sparse vertex specks) | #E8D6A0, #D9A9B8 |
| Forest floor | #6E8B54 |
| Dirt / shoulders | #B39066, #A07E57 |
| Rock | #9A9590, #857F79 |
| Sand / shore | #E2D1A6 |
| Water shallow → deep | #7CC4C0 → #3D7FA3; foam #F4F1E6 |
| Tree foliage (deciduous) | #6F9B4E, #7FA857, #5E8A46; trunks #7A5A43 |
| Conifers | #4E7A52, #5C8659 |

### Roads

| Element | Color |
|---|---|
| Dark asphalt | #4A4E54 |
| Light asphalt | #6C7076 |
| Cobblestone | #8C8178 (subtle stone pattern from geometry/vertex color, no texture) |
| Dirt path | #A88A62 |
| Lane markings | #EDE6D3; avenue center double line #E3C35A (muted yellow) |
| Sidewalk | #C9C2B4; curb #B5AE9F |
| Bus lane tint | #A65D52 (muted red-brown) |
| Tram rails | #6F6A66 with #9A948D head |

### Zones (buildings)

| Zone | Walls | Roofs | Accents |
|---|---|---|---|
| Residential (warm) | #EFE3CB cream, #E8C9A9 peach, #EADBA0 pale yellow, #B7C3A0 sage, #D9C3B0 sand | #B6654A terracotta, #9E5543 red slate, #7D5A44 brown, #5E6670 dark slate | shutters/doors #5F7F8E, #8E5B4A, #6E7F5A; window frames #F4EFE6 |
| Commercial (cool) | #C9D3DA light grey-blue, #8E9EAD slate, #E9ECEC white | flat roofs #7C8288 | glass #7FA6B5 (reflective), signs #D98B6E coral, #D9B45A mustard, #5FA3A0 teal |
| Industrial (muted) | #A9A69C concrete, #8C8C78 olive grey, #9B6B4D rust | #6F716C, corrugated #8A8C86 | #B9A266 faded mustard, #5D6266 steel; hazard stripes #D9B45A/#4A4E54 |

### Services, utilities, transit (signature colors; silhouettes carry identity first)

| Type | Body | Signature |
|---|---|---|
| Police | #DADDE2 | band #5872A3 |
| Fire | #B9554A brick | doors #D9C9A8 |
| Health | #F2F0EB | cross #4FA38F |
| Education | #D3A65B yellow brick | roofs #7D5A44 |
| Power | #6B6F76 concrete | stripes #E2B24A |
| Water | #6FA8C7 | tanks #DCE3E8 |
| Sewage | #8B9A6B | basins #6E8C8C |
| Garbage | #7E8B5A | #A9A69C |
| Bus depot / stops | #3F8F8B | — |
| Tram depot / stops | #8C5A86 | — |
| Town hall / landmarks | #E8DCC4 stone | roof #4F6A7A copper patina |

### Vehicles

Cars: #C8553D, #3E6E8E, #E8E1D3, #4F5D4A, #D9A441, #6B6E73, #2F3A45, #9C3D54, #7FA0B5,
#B9A58A. Trucks: #D9D4C7 boxes with cab colors from the car palette. Fire truck #C0453A,
ambulance #F2F0EB + #4FA38F stripe, police #E9ECEC + #3D5A8C, garbage truck #6E8B4E,
buses #E8E1D3 + line color band, trams #F0E9DA + line color lower body.

### Season tints (shader multipliers on vegetation & grass, interpolated across the season)

| Season | Grass | Deciduous foliage | Notes |
|---|---|---|---|
| Spring | #9CC27A | #8DBF62 + blossom specks #F1D3DC on 15 % of trees | fresh |
| Summer | #8DB06A | #6F9B4E | base |
| Autumn | #B8A862 | mix #D08B3E / #C4693F / #D9B45A by tree seed | conifers unchanged |
| Winter | #9A9F7E (no snow) | bare branches #7A6A5C | snow #EEF2F5, snow shadow #C9D6E3 |

## 6. Plot, zone and tool visuals

- Zoning view (plot tool active / zoning overlay): plot fill at 35 % opacity + 2 px-equivalent
  world-space outline, zone colors R #77B36B, C #5E8FC9, I #D3B04C, each with a pattern
  (R solid, C diagonal stripes, I cross-hatch) so zones read without color.
- Lot subdivision lines: thin #F4EFE6 at 60 %.
- Preview ghosts: valid = translucent #EDE6D3 (white-ish) with cyan-teal edge #4FB3B0;
  invalid = #D0643B with diagonal stripe pattern and a reason label; cost label follows the
  cursor.
- Selection highlight: soft rim brighten (shader) + #F4EFE6 outline pulse, never a hue change.

## 7. Overlays (data views)

The world desaturates to 25 % and lightens slightly (lighting kept so shapes read); data is
drawn on terrain (field rasters), roads (per-segment ribbons) and buildings (tint).

| Palette | Use | Default | Deuteranopia / Protanopia | Tritanopia |
|---|---|---|---|---|
| Sequential | land value, density, ridership, noise, pollution intensity | cividis-like: #00204D → #7C7B78 → #FFEA46 | same (CVD-safe) | same |
| Diverging good↔bad | happiness, coverage, congestion, service level | good #3E9C7F → neutral #EDE6D3 → bad #D0643B | good #3B76AF → #EDE6D3 → bad #E08B2E | good #2A9D8F → #EDE6D3 → bad #C8414B |
| Binary status | power/water/sewage served vs not | served #3E9C7F, unserved #D0643B + hatching | #3B76AF / #E08B2E + hatching | #2A9D8F / #C8414B + hatching |
| Categorical (lines, zones) | transit lines | 12 colors from Okabe–Ito-derived set: #0072B2 #E69F00 #009E73 #CC79A7 #56B4E9 #D55E00 #F0E442 #7F7F7F #882255 #44AA99 #117733 #332288 | same set (CVD-safe) | same |

Redundant encodings (always on, not only in CVD modes): iso-contour lines every 0.2 on
field overlays, hatching density for "bad" regions, flow chevrons on congestion overlay
(speed of chevrons = traffic speed), icons on buildings for binary status. High-contrast mode
increases outline thickness, uses pure white/black for neutral, and raises overlay opacity.

## 8. Problem icons and world labels

Billboards (procedural SDF icon atlas, ADR-0049) in rounded-square badges: badge #F6F1E7
with icon stroke #3B3A36; severity ring: warning #E0A43A, critical #C4544A. Scale stays
constant on screen within a range, fades beyond 1.2 km, clusters on a 48 logical-px screen grid when
> 3 fall into one cell (badge shows count + worst severity; click zooms to the cluster).
Icons: no road (road with slash), no power (bolt), no water (drop), sewage (pipe), garbage
(bin), fire (flame), crime (mask), sick (cross), no workers (person outline), no customers
(bag), no goods (crate), abandoned (broken window), pollution (cloud), noise (wave).

## 9. UI style

- **Theme**: light warm panels `#F6F1E7` at 94 % opacity, 1 px border `#D8CFBF`, corner
  radius 8, soft drop shadow (8 px, 18 % #3B3A36). Text `#3B3A36`, secondary `#6E6A60`.
  Accent teal `#3F8F8B`; success `#5E9E5A`; warning `#D98B3A`; error `#C4544A`; money
  positive `#4E8C55`, negative `#B5534A`.
- **Dark/high-contrast theme**: panels `#1E2124`, text `#FFFFFF`, accents saturated +20 %,
  borders 2 px `#FFFFFF` at 60 %.
- **Typography**: Inter Regular for body at 15 logical px
  (UI scale 1.0), headings 19–22 px; monospace (Hack) only in debug tools. UI scale
  75–200 % on top of the OS scale factor (Retina 2.0 renders text at native sharpness,
  ADR-0053). Shortcut hints use platform glyphs (⌘ ⌥ ⇧ ⌃ ⌫ on macOS) rendered with Inter.
- **Font**: Inter 4.001 (SIL OFL) — Regular for body, SemiBold for headings and numbers in the top
  bar; egui defaults as fallback; Hack monospace for debug views (ADR-0059).
  German strings are ~30 % longer: layouts use wrap/flex, no fixed-width labels.
- **Iconography**: one procedural line-icon set (2 px stroke at 24 px, rounded caps), the
  same atlas as world icons. Toolbar buttons 44 × 44 px with icon + tooltip
  (name, hotkey, cost, unlock condition when locked).
- **Layout**: top bar (money, Δ/day, population, happiness, date/time, weather + forecast,
  speed); left: RCI bars; bottom center: tool bar with category flyouts; right: contextual
  panels (info, road customization, line manager); notifications stack at top-right; overlay
  picker bottom-right with legend.
- **Motion**: panels slide/fade 120 ms; hover 80 ms; no bounce. Reduced-motion setting
  disables slides.
- **Tooltips** on every interactive element, 400 ms delay, max width 320 px.

## 10. VFX

- Construction: scaffold frame mesh + vertical reveal clip in shader (0 → 100 % over the
  construction time), small dust puffs (instanced quads).
- Smoke from chimneys/stacks: soft billboards rising and fading (grey-white #E4E2DE), denser
  for industry; fire: orange-yellow flickering emissive cards + dark smoke column.
- Rain: thin streaks, 25° slant by wind, density by intensity; splash dots on roads near the
  camera. Snow: soft flakes, slow drift. Both in a camera-following volume.
- Lightning: sky flash (≤ 120 ms, max brightness capped; disabled/softened by "reduce
  flashes"), procedural jagged bolt mesh.
- Water: gentle normal waves, shoreline foam band by depth, sun glints.
- Wind: tree and grass sway in vertex shader driven by weather wind.
