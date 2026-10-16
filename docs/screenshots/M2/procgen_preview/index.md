# M2 procgen preview (interim, not the M2 suite)

Software-rasterized with `cargo run -p cb_procgen --release --example preview_block -- <dir> [seed]`
(flat shading, one sun, baked AO; no shadows, fog or post-processing — the real look comes with
the M2 renderer). Purpose: judge massing, palettes, proportions and yard layout early.

| File | Content | Review |
|---|---|---|
| `street_y0.png` | curved street (S-curve, 12 m street mesh), lots on both sides from the road-edge frontage, R L1–L3 | houses face the frontage; lots follow the curve; fences/hedges on one side per shared edge; sheds, beds, trees in back yards |
| `street_overview.png` | same street from 300 m | lot fans on convex/concave sides look even; variety of roofs (gable/hip, 4 roof colours) |
| `houses_low.png` | low angle on a block (corner plot set) | plinths, steps, door canopies, shutters, window sills and mullions; a duplex with two wall colours under one roof |

Known gaps (fixed in the M2 renderer, not here): no shadows/sky lighting, the ground is a flat
grass plane without terrain materials, no junction surfaces.
