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
  (`M2`, `M3`, …, `S` for spikes). Each suite = deterministic scenario (seed + scripted
  `PlayerCommand` list, sim fast-forwarded headless to a fixed tick) + list of shots
  `{ id, camera: {focus, yaw, pitch, distance}, time_of_day, weather, overlay, ui: bool,
  assertions: [semantic-pass checks] }`.
- The shot list and the "must be visible" items for each suite are copied in
  `docs/PROGRESS.md` under the milestone's acceptance criteria. Minimum per suite:
  overview (high), mid (city block), close-up (street level), three yaw angles (0°, 120°,
  240°) for at least one subject; from M11: keyframe times (05:00, 06:30, 08:30, 12:00, 16:30,
  19:00, 20:00, 23:00); from M12: each weather at noon + rain and snow at night.

## 3. Render
- 1600 × 900 PNG at scale factor 1.0, UI hidden unless the shot says `ui: true`,
  deterministic seed, **exposure from the lighting director** (not fixed). File names
  `<suite>-<nn>-<id>.png`. Keep each image ≤ 1.5 MB; if larger, note it.
- **Settle rule**: capture only when pending mesh jobs = 0, pending pipelines = 0, ≥ 30 frames
  since the last change, and LOD crossfades finished.
- The run also writes `capture.json` (git hash, settings preset, adapter = lavapipe, timings)
  and, with `--screenshot-metrics`, `metrics.json` per shot: luminance histogram and median,
  clipped %, pure-black (< 8/255) %, magenta/NaN pixel count, plus the **semantic pass** (same
  camera rendered with flat class colors: terrain, water, road, R, C, I, service, vehicle, tree,
  UI) with the shot's assertions evaluated (e.g. vehicle pixels ≥ 1.5 %) and the beauty-pass
  class mean-color separation (ΔE00 ≥ 12).

## 4. Inspect every image yourself
First check `metrics.json`: any failed gate (magenta/NaN > 0, pure black > 0.5 % in daylight,
failed semantic assertion, calibration metrics from ADR-0050) blocks completion.
Then open each PNG with the Read tool and check, writing findings per shot:
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
- lavapipe renders are correct but slow and may differ from GPUs (precision of derivatives and
  compute passes). At M2 compare a lavapipe suite with the user's GPU suite once and record the
  metric deltas. At visual milestones add a real-GPU confirmation request to the User
  verification queue (☑ᵤ, non-blocking); `--screenshot-suite` works the same on their machines.
