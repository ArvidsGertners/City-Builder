---
name: visual-verification
description: Render the City Builder screenshot suite for a milestone from fixed camera angles (and, from M11/M12 on, times of day and weather), inspect every image against ART_DIRECTION.md and the milestone's must-be-visible list, and save the images plus a review to docs/screenshots/<milestone>/. Use at the end of every milestone with visual output and after any change to shaders, lighting, palettes or procgen.
---

# Visual verification

## 1. Prerequisites (Linux container)
```bash
sudo apt-get install -y mesa-vulkan-drivers xvfb    # lavapipe software Vulkan + virtual X server
```
`scripts/screenshot.sh` wraps: `xvfb-run -s "-screen 0 1920x1080x24" cargo run -p cb_game --release -- --screenshot-suite <SUITE> --out docs/screenshots/<SUITE>/`.

## 2. Suite definition
- Suites live in `crates/cb_game/src/debug/screenshot_suites.ron`, one per milestone
  (`M2`, `M3`, …). Each suite = deterministic scenario (seed + scripted `PlayerCommand`
  list, sim fast-forwarded headless to a fixed tick) + list of shots
  `{ id, camera: {focus, yaw, pitch, distance}, time_of_day, weather, overlay, ui: bool }`.
- The shot list and the "must be visible" items for each suite are copied in
  `docs/PROGRESS.md` under the milestone's acceptance criteria. Minimum per suite:
  overview (high), mid (city block), close-up (street level), three yaw angles (0°, 120°,
  240°) for at least one subject; from M11: 07:00, 12:00, 18:30, 23:00; from M12: each
  weather at noon + rain and snow at night.

## 3. Render
- 1600 × 900 PNG, UI hidden unless the shot says `ui: true`, deterministic seed, fixed
  exposure. File names `<suite>-<nn>-<id>.png`. Keep each image ≤ 1.5 MB (low-poly scenes
  compress well); if larger, note it — don't commit huge files.
- The run also writes `capture.json` (git hash, settings preset, adapter = lavapipe,
  timings).

## 4. Inspect every image yourself
Open each PNG with the Read tool and check, writing findings per shot:
- Every "must be visible" item for the shot is clearly visible (name it).
- Art direction: palettes within ART_DIRECTION.md; no pure saturated colors; shadows soft
  and not black; AO reads; fog not hiding the subject; night readable (if applicable).
- Readability: roads vs plots vs zones vs vehicles vs services distinguishable.
- Artifacts: z-fighting, gaps/cracks at LOD or terrain–road seams, floating or sunken
  objects, inverted normals, flickering-prone thin geometry, overlapping buildings, texture
  stretching (shouldn't exist), shader errors (magenta/black), aliasing, missing shadows.
- Consistency across the three yaw angles (must look good from all sides).
Fix problems and re-render; repeat until no blocking issue remains. Minor issues may be
recorded as known issues in PROGRESS.md with a plan.

## 5. Save for the user
- Commit images under `docs/screenshots/<suite>/` with `index.md`: table of shots
  (thumbnail link, camera, time/weather, what it demonstrates, verdict), list of issues
  found and fixed, remaining known issues, render settings, git hash.
- Note in the PROGRESS.md log that the suite was reviewed, with the path.
- lavapipe renders are correct but slow and may differ slightly from GPUs (no MSAA
  differences since we use SMAA; SSAO identical). At visual milestones ask the user for a
  real-GPU confirmation run on Windows/Linux and Apple Silicon (`--screenshot-suite` works
  the same on their machines).
