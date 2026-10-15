# Plan Review (2026-10-15)

## Process

- The user asked for a critical review by an agent that had not seen the plan's creation.
- A separate general-purpose agent received a **read-only snapshot** of the plan (commit
  `053daa3`) plus the user's brief, with instructions to be exhaustive and blunt. It did not
  modify the repository.
- It verified several claims against real crate sources from crates.io (Bevy 0.20.0,
  bevy_egui 0.43.1, egui fonts) and returned ≈ 310 findings:
  - A: 58 vague "will be handled" items;
  - B: 52 unconfirmed assumptions;
  - C: 5 risks with spikes;
  - D: 14 performance-budget problems with corrected tables;
  - E: 44 hidden milestone dependencies;
  - F: acceptance-criteria rewrites for M1–M16;
  - G: 118 other problems.
- Full text: [review/2026-10-15-independent-review.md](review/2026-10-15-independent-review.md).
- Resolutions below. **Adopted** = implemented as proposed. **Adapted** = the problem was
  accepted but solved differently, with the reason given. **Rejected** = not adopted, with
  the reason given. No finding was dismissed without a written resolution.

### Claims the reviewer found wrong (now fixed)

| Claim | Fix |
|---|---|
| egui UI runs in `Update` before the sim step | bevy_egui runs it in `PostUpdate` → ADR-0051 (one-frame UI latency, egui-gated input) |
| `shuffle_seed` usable in tests as configured | needs `bevy/debug` → cb_sim dev-dependency feature (ADR-0043) |
| egui default fonts show ⌘ ⌥ ⇧ ⌃ ⌫ | they don't → procedural shortcut glyph icons (ADR-0051) |
| A 30/60/120 frame cap is available | Bevy has none; bevy_framepace targets 0.19 → own limiter (ADR-0053) |
| `PanGesture` on macOS | iOS-only in winit → removed; two-finger scroll = pixel-unit wheel (ADR-0052) |

## Highest-severity findings and their resolution

| Finding | Resolution |
|---|---|
| G-D1 `derive_seed` XOR symmetric | Adopted: sequential absorption + collision/order tests (ADR-0043) |
| G-M1/C-4 time model inconsistent | Adapted: fixed 1 game min = 2 sim s; day length becomes pure pacing; all units explicit; spike S-Time (ADR-0042). Reason: generalizes the reviewer's "vehicle time dilation" option so balance and determinism never depend on the setting |
| G-S1/S2 save holes, regenerated terrain | Adopted: completeness rule, traffic/service/field state saved, base terrain saved (ADR-0044) |
| D1–D9 performance arithmetic | Adopted: PERFORMANCE.md rewritten (tick math, per-thread tables, extract on main thread, routing time cap, M4 GPU table, memory/entity/hitch budgets) |
| G-T1/T2 junction deadlock claim | Adopted: exit-space reservations, right-before-left resolution, all-way greedy release (ADR-0045) |
| G-G1–G4 road/terrain interface | Adopted: per-vertex junction surface + landing grade, continuous compositing, carve + 3.5 m shoulder, min-downsampled LODs, overpass bridging (ADR-0045/0046) |
| E hidden dependencies | Adopted: enabling work moved earlier (see the E table below) |
| G-P1 spikes undefined | Adopted: Phase S with six spikes in PROGRESS.md (ADR-0058) |
| G-W1/G-D7 verified API errors | Adopted (ADR-0051, ADR-0043) |
| G-D2/D3 machine-dependent results | Adopted, extended: fixed 64 partitions, libm facade, no glam SIMD types in sim code, cross-OS determinism CI job (ADR-0043/0054) |
| G-R1/R2 inadmissible ALT, cache keys | Adopted: increase-only variants, per-commuter variant keys, epoch-pure results (ADR-0048) |
| G-C1 agent cap as graphics setting | Adopted: `SimConfig::agent_cap` (ADR-0047) |
| F-0 user-dependent gates | Adopted: ☑ᵤ status + verification queue, `--no-gpu-work`, objective screenshot metrics and semantic pass, shot lists (ADR-0058) |
| G-I1–I6 input conflicts | Adopted (ADR-0052) |
| G-B1–B5 brief gaps | Adopted: `EditRoadGeometry`, per-approach controls, goods economy (ADR-0057), widening rule, undo refund window (ADR-0055) |

## A — "will be handled" without a concrete approach (all 58 filled in)

| # | Topic | Resolution | Where |
|---|---|---|---|
| A1 | Balance targets | Adopted | GAME_DESIGN §14 |
| A2 | Constant ownership | Adopted | CONSTANTS.md |
| A3 | Demand terms | Adopted (growth per game hour, not per sim minute) | ALGORITHMS §15 |
| A4 | Immigration rate | Adopted | ALGORITHMS §16 |
| A5 | Lifecycle | Adopted | ALGORITHMS §16, ADR-0016 amendment |
| A6 | Incident numbers | Adopted (fire timing recomputed: ≈ 130 sim s to ruin) | ALGORITHMS §17 |
| A7 | Capacity allocation | Adopted | ADR-0056, ALGORITHMS §17 |
| A8 | Power curves, heating | Adopted | ALGORITHMS §18 |
| A9 | Full landfill | Adopted (`EmptyLandfill` or close → park) | ALGORITHMS §18, GAME_DESIGN §6 |
| A10 | Water pollution downstream | Adopted | ALGORITHMS §18 |
| A11 | Land value magnitudes | Adopted | ALGORITHMS §19 |
| A12 | Tree absorption | Adopted (from the deterministic density field) | ALGORITHMS §8, §19 |
| A13 | Leveling weights | Adopted + test | ALGORITHMS §20 |
| A14 | Goods economy | Adopted | ADR-0057, ALGORITHMS §27 |
| A15 | Bankruptcy "stall" | Adopted | ALGORITHMS §21 |
| A16 | Loans | Adopted | ALGORITHMS §21, GAME_DESIGN §9 |
| A17 | Ray de-crossing bound | Adopted | ALGORITHMS §10 |
| A18 | Deep plot depth | Adopted | ALGORITHMS §9 |
| A19 | Building "still fits" | Adopted | ADR-0014 amendment |
| A20 | Plot back edge | Adopted | ALGORITHMS §9 |
| A21 | Snap radius | Adopted | ALGORITHMS §1 |
| A22 | Crossing/profile ordering | Adopted (fixed point, ≤ 2 iterations) | ALGORITHMS §1 |
| A23 | Profile solver stopping | Adopted | ALGORITHMS §2 |
| A24 | Pier conflicts | Adopted | ALGORITHMS §3 |
| A25 | Taper | Adopted | ALGORITHMS §5 |
| A26 | Signal phase plans | Adopted | ALGORITHMS §6 |
| A27 | Pedestrians at junctions | Adopted | ALGORITHMS §6 |
| A28 | Background traffic at junctions | Adopted | ALGORITHMS §6, ADR-0047 |
| A29 | Cycle detection graph | Adopted | ALGORITHMS §13, ADR-0047 |
| A30 | Lane plans | Adopted | ALGORITHMS §13, ADR-0047 |
| A31 | Reroute budget | Adopted | ALGORITHMS §12 |
| A32 | Landmark recompute | Adopted | ADR-0048 |
| A33 | Agents on load | Adopted (agents saved) | ADR-0044 |
| A34 | Fields saved or not | Adopted (pollution/LV saved, noise recomputed) | ADR-0044, ADR-0028 amendment |
| A35 | Shader warm-up | Adopted | ARCHITECTURE §7 |
| A36 | City generator | Adopted (moved earlier, to M2) | ARCHITECTURE §3, PROGRESS M2 |
| A37 | Light calibration metrics | Adopted | ADR-0050 |
| A38 | Objective audio checks | Adopted | ADR-0058, PROGRESS M13 |
| A39 | Autosave "modal" | Adopted | ADR-0044 (inherits ADR-0020 + rule: tool drag ∨ dialog ∨ queue non-empty; retry 5 s, force after 60 s) |
| A40 | Transit itinerary, stops | Adopted | ADR-0035 amendment |
| A41 | Transit land-value bonus | Adopted | ADR-0035 amendment |
| A42 | Roads partly cleared of snow | Adopted | ALGORITHMS §25 |
| A43 | Icon clustering | Adopted | ART_DIRECTION §8 (48 px grid), PROGRESS M8 test |
| A44 | Notification throttle | Adopted | GAME_DESIGN §12 |
| A45 | Jam area | Adopted | ALGORITHMS §13 |
| A46 | Hitch scenarios | Adopted | PERFORMANCE §13 |
| A47 | Soak thresholds | Adopted | PROGRESS M9 |
| A48 | Traffic fallback | Adopted (fallback ladder) | ADR-0047, PROGRESS S-T |
| A49 | Commute factor tuning | Adopted (spike S-Time) | PROGRESS Phase S |
| A50 | Scope-cut ladder | Adapted: brief features are never cut without asking | PROGRESS |
| A51 | Inspector for pure-crate resources | Adopted (`DebugView`) | ADR-0031 amendment |
| A52 | Plots under new corridors and junctions | Adopted | ALGORITHMS §1 |
| A53 | Upgrade widening | Adopted | ALGORITHMS §1 |
| A54 | Spawning on occupied lanes | Adopted | ADR-0047 |
| A55 | Capture settle rule | Adopted | ADR-0058, visual-verification skill |
| A56 | LOD residency | Adopted | ADR-0049 |
| A57 | l10n key generation | Adopted | ADR-0021 (build.rs), PROGRESS M1 |
| A58 | Structural tooltips | Adopted | ADR-0051 |

## B — assumptions

All 52 items (plus B53) are tracked in [ASSUMPTIONS.md](ASSUMPTIONS.md), each with:
- its current default;
- whether the review changes resolved it;
- whether it needs the user's input (marked ❓).

## C — highest-risk parts and spikes

- **Five spikes adopted.** S-T (traffic), S-J (junctions), S-P (plots → buildings), S-Time
  (time model) and S-G (road–terrain incl. bridges) follow the reviewer's scope, cases,
  pass/fail numbers, time-boxes and fallbacks.
- **One spike added: S-Render.** It covers the material pipeline, the new vertex format with
  derivative normals, render scale and GPU-free shader validation, all of which ADR-0049 and
  ADR-0053 now depend on.
- **Placement:** Phase S in PROGRESS.md, between M1 and M2.
- **Bridges** are covered inside S-G, as the reviewer suggested.

## D — performance budgets

**Adopted:** D1–D8, D12, D13, D14, and the corrected per-thread tables. PERFORMANCE.md was
rewritten. Within the D tables, routing gets 0.65 ms in the per-tick budget so the tick sums to
3.0 ms.

**Adapted:**
- **D9/D10 (memory).** The reviewer proposed instancing trees and props, plus quantized
  positions. Instead:
  - indexed 20-byte vertices with flat shading from screen-space derivatives, so no normals
    and shared vertices;
  - props stay merged per chunk, with residency rings;
  - a 256 MB LRU of per-building meshes.

  This keeps Bevy's standard shadow and prepass pipelines, so no custom instanced pipeline is
  needed. Spike S-Render verifies it (ADR-0049).
- **D11 (vehicles).** The reviewer proposed a single instance buffer. Instead, vehicles stay
  pooled entities, at most 3 000 within the 15 000 renderable-entity budget, to keep Bevy's
  shadow and auto-instancing paths. Revisit if ViewSync or extract exceed budget in S-T or M9.

## E — hidden milestone dependencies

| IDs | Resolution | Where |
|---|---|---|
| E1 | Phase S defined | PROGRESS |
| E2, E3 | Command framework in M2, snapshot types in M3 | ADR-0055, PROGRESS M2/M3 |
| E4 | Economy stub + catalog in M2 | PROGRESS M2 |
| E5, E43 | Router, cache, flow model (macro) in M5 | ADR-0048, PROGRESS M5 |
| E6, E24 | Vehicle store, ghost vehicles, vehicle view and α in M6 | PROGRESS M6 |
| E7 | Fire VFX in M6 | PROGRESS M6 |
| E8 | Settings persistence in M1 | PROGRESS M1, ARCHITECTURE §14 |
| E9, E10 | UI foundation (theme, widgets, dialogs, toasts, toolbar) in M2 | ADR-0051, PROGRESS M2/M7 |
| E11 | Tutorial replay via pause menu / `--tutorial` | GAME_DESIGN §13, PROGRESS M14 |
| E12 | UI anchors in M8 | ADR-0051, PROGRESS M8 |
| E13 | Worldgen shore guarantee in M2 | ALGORITHMS §7 |
| E14, E15 | citygen + scripted benchmark scenes from M2 | PROGRESS M2/M4 |
| E16 | Instant-growth stub in M2 | PROGRESS M2 |
| E17 | Debug level/condition commands in M4 | PROGRESS M4 |
| E18 | Signal plan + stub phase clock in M3 | PROGRESS M3 |
| E19, E20, E32 | Tram/bus variants and tram radii in the M3 junction generator | ADR-0045, PROGRESS M3 |
| E21, E42 | Progression default all-unlocked until M7; `--sandbox` CLI | GAME_DESIGN §4, PROGRESS M7 |
| E22, E34 | Traffic state saving decided in M5 (ADR-0044); agent cap moved out of graphics | ADR-0044, ADR-0047 |
| E23 | Fixture policy (v0 dev format until v1.0) | ADR-0044 |
| E25 | Happiness stubs listed with a test | GAME_DESIGN §10 |
| E26 | Garbage trucks switch in M9 with a test | ALGORITHMS §18, PROGRESS M9 |
| E27 | CVD palettes + switch in M8 | PROGRESS M8 |
| E28 | Runtime render-scale change in M2 | PROGRESS M2 |
| E29–E31 | `SetSimConfig` + dev panels before M15 | ADR-0055, PROGRESS M5 |
| E33 | Vehicle info panel real in M9 | PROGRESS M9 |
| E35, E36 | `LightingState` + no-op shader uniforms in M2 | PROGRESS M2 |
| E37 | Chunk merging from M2 | PROGRESS M2 |
| E38 | Deck picking in M3 | ADR-0034 amendment, PROGRESS M3 |
| E39 | Crash notice as an egui window from M1 | ADR-0022 amendment |
| E40 | Smoke scene extended each milestone | ADR-0054, milestone-completion skill |
| E41 | `UiIntent::PlaySound` hooks in M8 | PROGRESS M8 |
| E44 | Mac input checks are non-blocking | PROGRESS UV-4 |

## F — acceptance criteria

**Adopted.** Every milestone in PROGRESS.md now has:
- named tests;
- numeric gates;
- container CPU gates via `--no-gpu-work`;
- ☑ᵤ GPU gates;
- for visual milestones (M2, M3, M4, M6, M7, M8, M9, M10, M11, M12, M14, M15, M16), shot
  lists with camera framing and must-be-visible items;
- global metric gates.

## G — other problems

| IDs | Resolution | Where |
|---|---|---|
| G-D1–G-D10 | Adopted. G-D3 adapted: a "no glam SIMD types in sim code" rule instead of glam's `scalar-math`, which would slow Bevy | ADR-0043 |
| G-S1–G-S7 | Adopted | ADR-0044 |
| G-M1, G-M2 | Adopted (time model) | ADR-0042 |
| G-M3 | Adopted | ADR-0056 |
| G-M4 | Adopted | ADR-0028 amendment, ARCHITECTURE §5.3 |
| G-M5 | Adopted | ADR-0016 amendment, DATA_MODEL §6 |
| G-M6 | Adopted (SCC, negative cache) | ADR-0048, ALGORITHMS §14 |
| G-M7 | Adopted | ALGORITHMS §17 |
| G-M8 | Adapted: spike S-Time sets the target at ≥ 40 % visible agents; meso fallback in S-T | ADR-0047 |
| G-M9 | Adopted | ADR-0048 |
| G-M10 | Adopted | ADR-0028 amendment |
| G-M11 | Adopted (tooltip wording) | GAME_DESIGN §3 |
| G-M12 | Adopted (weighted sampling) | ALGORITHMS §15 |
| G-T1–G-T8 | Adopted; G-T4 adds a roundabout controller and preset | ADR-0045, ALGORITHMS §6 |
| G-G1–G-G10 | Adopted | ADR-0045, ADR-0046, ALGORITHMS §1–5, §11 |
| G-R1–G-R4 | Adopted | ADR-0048 |
| G-C1–G-C14 | Adopted: contradictions removed. G-C4: German is written alongside English from M1 (the key test enforces it), and M15 checks 0 fallback lookups. G-C7: night exposure corrected | ADR-0047, ADR-0050, ART_DIRECTION §3, TESTING |
| G-W1–G-W15 | Adopted. G-W9 adapted: water depth baked per vertex, not sampled from a heightfield texture | ADR-0049–0054, ADR-0037 amendment |
| G-I1–G-I6 | Adopted | ADR-0052 |
| G-B1–G-B10 | Adopted (incl. free decorations, walkers from stop queues, LotId seeds, display modes) | ADR-0055, 0057, 0053, 0035 amendment, GAME_DESIGN §3 |
| G-P1–G-P8 | Adopted | ADR-0058, ADR-0054, PROGRESS, skills |
| G-O1–G-O10 | Adopted | ADR-0034/0040/0019/0022 amendments, ARCHITECTURE |
| G-X1–G-X4 | Adopted (estimates, scope ladder, shrinker, M4 set A/B split, music excluded) | PROGRESS, ADR-0023/0029/0019 amendments |
