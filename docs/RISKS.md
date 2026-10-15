# Risks & Open Questions

Status: **Living document** (revised after the 2026-10-15 independent review) — update when a risk
materializes, is retired, or a question is answered. Likelihood/Impact: L/M/H. The six top risks
are proven by Phase S spikes before their real systems are built (PROGRESS.md, ADR-0058).

## 1. Risk register

| # | Risk | L | I | Mitigation | Spike / checkpoint |
|---|---|---|---|---|---|
| R1 | **Traffic at scale**: junction admission, lane plans on short links, gridlock, routing load; visible traffic not representing real load | H | H | 1D SoA agents, fixed partitions, exit-space reservations, lane plans with routing penalty, spawn queues, cost-unit routing budget, wait-for cycle breaking, ≥ 40 % visible-agent target; fallback ladder (forced signals, meso model, lower cap) (ADR-0045/0047/0048) | **S-T**; M9 soak |
| R2 | **Intersection geometry** on freeform, sloped, mixed-width roads incl. tram radii; steps at cut lines; "too close" frustration | H | H | Node-based generation with cut-back cap + spacing guide, per-vertex surface heights + landing grade, per-kind connector radii, polygon repair, randomized property tests; fallback: degree/angle caps, templates (ADR-0045) | S-J ☑ (ADR-0063): 0 invalid geometry; 0.65 % typed rejections on realistic layouts; open: p99 1.4 ms vs 0.4 ms target (M3 optimization) |
| R3 | **Plots → lots → buildings**: slivers, crossing rays, corner lots, unstable lot IDs, poor fits | H | H | Strip subdivision with bounded de-crossing, i_overlay booleans, repair loop, fit rule, stable-ID matching, corner frontage via fillets; fallback: frontage-drag only, parallel side edges, corner generator (ADR-0064) | S-P ☑ (ADR-0064): all gates pass on 10 000 plots + 28 598 edits, p99 1.63 ms; open: fan > 25° on very tight curves (rule relaxed), real plot-tool polygons in M2 |
| R4 | **Time model / trip scale**: unit confusion, peaks, concurrency vs agent cap, day-length effects | M | H | Fixed sim/game ratio, explicit units, day length as pacing only, backward scheduling, perceived time (ADR-0042) | **S-Time** |
| R5 | **Road–terrain interface incl. bridges**: terrain through roads at any LOD, cliffs between embankments, junction steps, abutment gaps, buried underpasses, preview flicker | H | H | Continuous compositing with cores, 0.15 m carve, 3.5 m shoulder, min-downsampled LOD in corridors, skirts, overpass bridging, span hysteresis, abutment pads (ADR-0065, ALGORITHMS §2–3) | S-G ☑ (ADR-0065): all gates pass incl. 100 % terrain ≤ road at LOD0–3, 0 cliffs, steps ≤ 0.7 mm, restamp 3 ms; open: real main-thread apply (M2), terrain-gen skirt depths |
| R6 | **Rendering pipeline assumptions**: custom vertex layout + derivative normals with Bevy 0.20 prepass/shadow paths, render-scale composite, memory budget, GPU-free shader validation | M | H | Spike first; fallbacks: standard layout with normals + tighter residency, offscreen composite, smoke-test-only validation (ADR-0049/0053) | **S-Render** |
| R7 | **Simulation performance** at 50k citizens (trip bursts, sub-rate spikes at 4×) | M | H | Time wheel, phase table with 0.6 ms slot cap, sliced daily systems, deterministic budgets; bench at 4× | M5 bench (tick p95 ≤ 1.0 ms) |
| R8 | **Look & feel not polished** (repetitive buildings, flat lighting) | M | H | ART_DIRECTION values, screenshot suites with objective metrics + semantic pass + agent review, procgen gallery, vertex AO + SSAO, M2 visual gate | M2 review |
| R9 | **Bevy 0.20 is one week old** (ecosystem lag, WESL dialect, early bugs) | H | M | Core Bevy + bevy_egui only; in-house inspector; small isolated shaders; pinned versions; upgrades only between milestones via ADR | S-Render; M1 smoke |
| R10 | **GPU/driver differences** on Windows and Metal not visible on lavapipe | M | M | Cross-backend features only; shader validation incl. MSL/HLSL (S-Render); real-GPU runs via the User verification queue; adapter info in crash reports | UV-1/UV-2 |
| R11 | **Procedural audio quality** without listening | H | M | Objective audio QA suite + spectrograms; listening pass ☑ᵤ; CC0 samples possible via superseding ADR | M13 |
| R12 | **Determinism drift** (iteration order, maps, partitions, float/libm, SIMD) | M | M | ADR-0043 rules enforced by clippy config; thread-count, shuffle, save/load and cross-OS hash tests | M1–M5 |
| R13 | **Save compatibility** breaks silently | L | H | Completeness rule, struct-map encoding, validation, catalog hash, fixtures from v1.0 (ADR-0044) | M5, M15 |
| R14 | **Scope** (≈ 150 agent-days) — polish suffers | H | H | Estimates vs actuals, 1.5× trigger, scope-cut ladder that never cuts brief features without asking, depth over breadth | every milestone |
| R15 | **Build/CI time** with full Bevy on three OSes | M | M | Workspace split, cached CI, `ci-release` profile, 640×360 smoke, CI ≤ 25 min target; local `dynamic_linking` only outside CI | M1 |
| R16 | **Retina fill rate** on base M4 | M | M | Auto render scale by GPU class (≤ 2.2 MP), Medium preset on Apple M-series, half-res SSAO, fewer bloom mips (ADR-0053) | UV-3 |
| R17 | **Trackpad/shortcut behavior** on macOS (gesture delivery, ⌘Q interception, Magic Mouse) | M | M | Scroll-unit policy + setting, drag precedence, synthetic-event tests, autosave-on-exit fallback, user check (ADR-0052) | UV-4 |
| R18 | **Unsigned macOS app** blocked by Gatekeeper / App Translocation | H | L | Ad-hoc signing, README instructions, translocation detection (ADR-0037/0040 amendments) | first release |
| R19 | **Memory** (merged meshes, props, render targets) on 16 GB unified memory | M | M | 20 B indexed vertices, residency rings, LRU caps, budgets reported by `--benchmark` (ADR-0049) | S-Render, M4 |

## 2. Open questions

All unconfirmed assumptions — with the default that will be implemented — are in
[ASSUMPTIONS.md](ASSUMPTIONS.md). The ones marked ❓ there are the decisions worth your input;
none blocks M1. Previously open questions resolved by decisions: inspector (ADR-0031), audio
(ADR-0019), power distribution (ADR-0026, ASSUMPTIONS B7), macOS (required, ADR-0037), save/load
core and l10n early (PROGRESS M1/M5).
