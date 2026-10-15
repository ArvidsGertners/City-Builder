# ADR-0017: Procedural generation determinism and seeds

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (core utilities), all later milestones

## Context

Every building/vehicle/tree must be unique yet deterministic (seed from world seed and
object ID); saves store seeds, never meshes; the simulation should be reproducible for
tests, replays and future multiplayer.

## Options considered

1. **`rand` crate with `StdRng`** — `StdRng` algorithm is explicitly not stable across
   versions; extra dependency weight.
2. **`rand_pcg`/`rand_chacha`** — stable algorithms, still pulls `rand_core` ecosystem.
3. **Own small PRNG + stable hash in `cb_core` (chosen)** — ~150 lines, fully under our
   control, no version drift.

## Decision

- `cb_core::hash`: stable 64-bit mixer (SplitMix64 finalizer / wyhash-style), never
  `std::hash::DefaultHasher` (unstable across Rust versions) or `RandomState`.
- `derive_seed(world_seed, Domain, id, extra)` for all content seeds (domains listed in
  ALGORITHMS §22).
- `cb_core::rng`: `Pcg32` streams for generators; counter-based `SimRng::for_event(stream,
  tick, id)` for sim randomness (stateless, order-independent, nothing to save).
- Distributions implemented locally (uniform, normal via Box–Muller, weighted choice,
  Poisson disk helper).
- Rules (CLAUDE.md): no HashMap iteration affecting results; sort by stable ID where order
  matters; no wall-clock or frame-time inputs to sim/procgen; float accumulation in stable
  order.
- Guarantee level: **bit-identical on the same platform and build**; cross-platform
  (Windows vs Linux x86-64) is best effort — IEEE basic ops are identical, but
  transcendental functions (`sin`, `exp`, `powf`) come from the platform libm and may differ
  in the last ulp. Visual procgen tolerates this. If lockstep multiplayer is ever added, sim
  math switches to `libm` (Bevy's `libm` feature exists) via a new ADR.

## Consequences

- Golden-hash tests per platform; reproducible bug reports (seed + command log).
- Slight duplication of well-known RNG code (small, tested).
