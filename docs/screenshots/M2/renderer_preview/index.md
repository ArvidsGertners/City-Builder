# M2 renderer preview (interim, not the M2 suite)

First frames of the real renderer (lavapipe, `--seed 1234 --screenshot-suite M2dev`): generated
terrain, the demo town from `citygen::grid` (29 segments, 58 plots, 240 houses), world material
with derivative flat normals, terrain material, cascaded shadows, SSAO, focus fog, noon keyframe.

| File | Review |
|---|---|
| `town.jpg` | grid town, junction fillets with sidewalks, dashed centre lines, varied roofs, yards |
| `town_block.jpg` | houses face the streets, fences/hedges, sheds, trees; shadows soft; grass verges |
| `street.jpg` | street level: curbs, markings, hedges; distant fog without fog over the town |

Open points for the M2 suite: sky dome too grey at the map edge in the overview, forest masses
read as speckles at 2 km, the scripted M2 scenes (slope embankment, cut, shore) still to build.
