# ADR-0044: Save format v2 — completeness rule, saved base terrain, validation

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M3 (snapshot types), M5 (format), M9 (traffic state), M15 (UI, autosave)
- **Supersedes:** ADR-0020 (keeps MessagePack + LZ4, header layout, migration chain on
  `rmpv::Value`, atomic writes, autosave rotation, RON settings)

## Context

Review findings G-S1–G-S7, A33, A34: the save omitted in-flight agents, junction/signal
state, route cache contents, flow EMAs, admission state, incidents and accumulating fields,
so "save → load → continue" could not equal a continuous run from M9 on; regenerating base
terrain from the seed on load breaks saves whenever worldgen changes between versions (and
risked cross-platform drift); rmp-serde writes structs as arrays unless configured; loaded
data was not validated.

## Decision

- **Completeness rule**: *everything that influences future simulation results is saved*;
  only data that is a pure deterministic function of saved data may be omitted. Saved:
  - authored state (roads, profiles, styles, structure spans, node controls, plots, lots,
    buildings, props placed by the player, stops, lines, policies);
  - **base terrain**: heights as `u16` (3 mm steps over the map's height range), water
    surface field (`u16`, sparse rows), forest mask (`u8`), plus stamps; ≈ 3–5 MB after LZ4;
  - sim state: population tables, trips (with transit legs), economy, demand smoothing,
    progression, weather + forecast, clock, ID allocators, command sequence, stats;
  - traffic state: agents (lane as `(SegmentId, dir, lane index)` + `s/len`, `v`, route links,
    lane plan, held zones and exit reservations, timers), junction controllers (phase, timers,
    queues, reservations), flow EMAs, admission controller, routing queue, route cache entries
    with epochs, ALT landmark tables + recompute progress;
  - services: incidents, vehicles out, coverage loads; transit runs with passengers;
  - fields with memory: ground and water pollution, land-value smoothing (`u16` quantized).
  Omitted (derived): lane/link graphs, junction geometry, spatial indices, coverage reach,
  noise field, materials, scatter, meshes, undo history.
- Encoding: MessagePack with **`Serializer::with_struct_map()`** (named fields) so additive
  `#[serde(default)]` fields need no migration; a test decodes a fixture with a missing field.
- **Validation**: `validate_snapshot()` checks referential integrity (dangling IDs, segments
  without nodes, lots outside plots, agents on missing lanes, …) and returns typed errors
  before any entity is spawned; a fuzz test mutates fixtures and asserts error-not-panic.
- Header additions: `catalog_hash` (load UI warns on mismatch), optional
  `PresentationState` extension (camera pose, overlay, open panels — not sim state).
- `PlayerCommand` serialization is versioned with the save format (replay logs).
- **Fixture policy**: format version `0` ("dev") until the v1.0 release (M15). Until then
  fixtures are regenerated when the format changes and a synthetic `v0 → v1` migration test
  keeps the framework exercised. From v1.0, every breaking change bumps the version and adds
  a migration and a golden fixture (CI).
- Autosave (interval and 3 rotating slots as in ADR-0020) is deferred while a tool drag is active,
  a dialog is open or the command queue is non-empty; it retries every 5 s and is forced
  between frames after 60 s of deferral. Snapshot cost ≤ 10 ms on the main thread.
- Load sequence: header → body → migrate → `validate_snapshot` → restore terrain (no
  regeneration) → spawn entities with original IDs → rebuild derived structures → restore
  traffic/service state onto rebuilt lanes.

## Consequences

- Save/load continuation is bit-exact with traffic on (tested); saves survive worldgen changes
  and move between platforms. Saves grow to ≈ 10–15 MB for a max city (acceptable).

## Amendments

- 2026-10-17 (M5, format v0 implemented in `cb_sim::save`): framing is `MAGIC "CBSAVE\0\0" |
  format version u32 LE | header length u32 LE | header | body | checksum u64 LE` (stable hash of
  everything before it); a mismatch is `SaveError::Checksum`. Base heights are stored as `u16`
  steps of 1/256 m above the minimum — lossless because worldgen already quantizes to
  `HEIGHT_QUANTUM_M` — and fall back to raw `f32` when a terrain (test valleys) is not on that
  grid, so a loaded game never differs from the continuous one. Fields are stored as `f32`
  rasters for the same reason. Because sliced passes carry state across ticks (field slice
  inputs, the ADR-0068 day accumulators), those inputs are saved too: a save made at any tick
  continues bit-identically (`save_load_continue_equals_continuous`). Roads are saved as their
  authored inputs (curve, profile, kind, variant, style, spans, piers, controls); segment
  geometry, cut-backs, junctions and all terrain stamps are rebuilt through the undo restore
  path, then `DirtyPlots` is cleared (restored plots already match their roads). Not in v0:
  the command log (replay) and undo history; the LZ4 wrapper and autosave live in the game.
