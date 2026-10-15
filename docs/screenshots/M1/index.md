# M1 — screenshot suite (pipeline proof)

Purpose: prove the render → screenshot → metrics pipeline (`scripts/screenshot.sh M1`). The
scene is the M1 placeholder (`crates/cb_game/src/scene.rs`), replaced by real content in M2;
it is **not** an art-direction checkpoint.

- Renderer: lavapipe (llvmpipe, LLVM 20.1.2, Vulkan, CPU), Mesa 25.2.8, Xvfb, scale factor 1.
- Resolution 1600 × 900, UI hidden except shot 04. Metrics: `metrics.json`.

| Shot | Camera (focus, d, pitch, yaw) | Purpose | Metrics | Verdict |
|---|---|---|---|---|
| `M1-01-overview.png` | (0,0,0), 70 m, 32°, 35° | default camera, shadows, flat shading | median 178, clipped 0 %, black 0 %, magenta 0 | ✅ |
| `M1-02-street-level.png` | (0,3,0), 52 m, 6°, 80° | low angle, sky, eaves | median 189, clipped 0 %, black 0 %, magenta 0 | ✅ |
| `M1-03-top-down.png` | (0,0,0), 90 m, 72°, 0° | steep angle, shadow direction | median 177, clipped 0 %, black 0 %, magenta 0 | ✅ |
| `M1-04-dev-tools.png` | default | `--features dev`: perf overlay + sim controls, Inter font in egui | (manual capture) | ✅ |

Self-review:
- Found and fixed: roofs were 0.2 m narrower than the house depth (thin wall strip visible
  above the eaves); the street-level camera sat inside the house row. Both fixed before these
  captures. Dev windows overlapped; default layout fixed.
- Remaining (accepted for M1): placeholder geometry and lighting are untuned (ambient high,
  pastel look) — art direction starts in M2/S-Render. Software rendering: the perf overlay
  shows ~215 ms frames at 1600 × 900, irrelevant for GPU budgets.
