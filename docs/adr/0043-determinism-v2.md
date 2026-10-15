# ADR-0043: Determinism v2 — seeds, partitions, portable math, map discipline

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (cb_core), enforced from M2
- **Supersedes:** ADR-0017

## Context

Review findings G-D1–G-D10: `derive_seed` XOR-combined independently mixed inputs (symmetric
and self-cancelling → seed collisions), "≥ 4 × core count" partitions made float reductions
machine-dependent, glam SIMD types reduce in different orders on SSE2 vs NEON, platform
libm differs across OSes, std `HashMap` appeared in sim data, `shuffle_seed` needs Bevy's
`debug` feature, and counter-based RNG calls lacked a purpose salt. The user owns a Windows
PC and an Apple Silicon Mac, so cross-platform saves and reproducible bug reports matter.

## Options considered

1. **Same-platform determinism only** (ADR-0017) — cheap, but saves and bug reports don't
   transfer between the user's machines; worldgen changes break old saves.
2. **Cross-platform determinism via fixed-point math** — robust but invasive everywhere.
3. **Cross-platform determinism via portable float discipline (chosen)**: IEEE basic ops and
   `sqrt` are correctly rounded everywhere; transcendental functions come from the pure-Rust
   `libm` crate; no SIMD-reduction types in sim-affecting code; fixed partitioning.

## Decision

- **Seeds**: sequential absorption with distinct odd constants:
  `h = mix64(world ^ K0); h = mix64(h ^ (domain as u64).wrapping_mul(K1));
   h = mix64(h ^ id.wrapping_mul(K2)); h = mix64(h ^ extra.wrapping_mul(K3))`.
  Tests: no collisions over 10⁶ (id, extra) pairs; swapping arguments changes the seed.
  Zoned buildings derive from `LotId` (+ level, + regrowth counter) per the brief; non-zoned
  from `BuildingId`.
- **Event RNG**: `SimRng::for_event(stream, purpose: u16, tick_or_day, id)`; a debug-only
  registry asserts unique `(stream, purpose)` per call site.
- **Partitions**: constant `SIM_PARTITIONS = 64` for all intra-system parallelism, independent
  of core count; reductions merge in partition order. Test `results_identical_1_4_8_threads`.
- **Portable math**: `cb_core::math` facade (`sin`, `cos`, `atan2`, `exp`, `ln`, `powf`, …)
  backed by `libm`; clippy `disallowed-methods` for the std transcendental `f32/f64` methods
  and `disallowed-types` for `glam::{Vec3A, Vec4, Mat3A, Mat4, Quat, Affine3A}` in all pure
  crates and `cb_sim` (cb_audio exempt: audio is cosmetic). Rust never contracts `a*b+c` into
  FMA implicitly; `mul_add` is disallowed in those crates.
- **Maps**: std `HashMap`/`HashSet` are disallowed in sim crates; use `LookupMap<K, V>`
  (`cb_core::collections`, fixed hasher, **no iteration API**), `BTreeMap`, or dense `Vec`s
  indexed by ID; iteration that affects results goes over sorted IDs.
- **ECS order**: sort by stable ID wherever order matters (unchanged).
- **Schedules**: `cb_sim` dev-dependency enables `bevy/debug` for `shuffle_seed`;
  `auto_insert_apply_deferred` is disabled in `SimEdit`/`SimTick` (explicit `ApplyDeferred`
  between sets).
- **Guarantee**: bit-identical sim state and worldgen on all three required platforms for the
  same build, seed and command log. Verified by the CI cross-OS job (ADR-0054). Visual-only
  mesh details may differ on GPUs, never in CPU data.
- If the cross-OS job uncovers an unfixable source (e.g. a dependency), the fallback is
  same-platform determinism; saves stay portable regardless because base terrain is saved
  (ADR-0044).

## Consequences

- Reproducible bug reports and portable saves across Windows, Linux and macOS.
- Slight cost from `libm` and scalar math (negligible in sim hot loops, measured in spikes).
