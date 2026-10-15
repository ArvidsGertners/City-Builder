# ADR-0005: Simulation / rendering separation

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

The brief requires a simulation fully separated from rendering, runnable and testable
headless, while staying idiomatic Bevy (plugins, system sets, events). Determinism and a
future dedicated server / lockstep multiplayer must not be blocked.

## Options considered

1. **Pure-Rust simulation without Bevy**, wrapped by the game — maximal isolation, but
   duplicates ECS-like infrastructure and fights the "idiomatic Bevy" requirement.
2. **Separate `SubApp`/World for the sim** with explicit extraction to the main world —
   strong isolation, enables running sim in parallel with the frame, but doubles data and
   adds an extraction layer for every feature.
3. **Same main World, separate crates and schedules (chosen)** — sim plugins (bevy_ecs) in
   `cb_sim`, presentation plugins in `cb_game`; the sim owns its components/resources; the
   presentation reads them and writes only `PlayerCommand`s.

## Decision

- `cb_sim` is a library of Bevy plugins using only bevy_app/bevy_ecs/bevy_state/bevy_time/
  bevy_tasks; it runs in a `MinimalPlugins` app in tests and in the full app in the game.
- The sim runs in its own schedules `SimEdit` and `SimTick` (ADR-0006), driven explicitly.
- **Visual entities are separate from sim entities.** View plugins maintain registries
  (`stable ID → visual entity or chunk slot`) and react to change detection, removal
  observers and dirty messages. Sim entities carry no `Mesh3d`, `Transform` or materials.
  (Sim positions are sim data: `RoadNode::pos`, lot polygons, building placement.)
- Presentation systems take sim data **read-only** (`Query<&T>`, `Res<T>`). This is enforced
  by the compiler: sim component/resource fields are `pub(crate)` and exposed through `pub`
  accessor methods (`&self` only); all mutating methods are `pub(crate)`. `cb_game` therefore
  cannot change sim state except by sending `PlayerCommand`s. The only presentation-writable
  sim resources are the driver API: `SimSpeed`, `SimStepRequest`, debug overrides (`dev`,
  routed through `DebugCommand`s).
- Table- and graph-shaped state (agents, population, lane graph, fields, terrain) is held in
  resources wrapping pure-crate types; discrete world objects are entities.

## Consequences

- One World keeps data access simple and fast (no extraction copy); render extraction still
  runs pipelined in Bevy's render sub-app.
- Sim cannot run concurrently with the presentation systems of the same frame; acceptable
  under the CPU budget (PERFORMANCE.md). If needed later, the sim can move to a SubApp
  without changing its plugins (they don't reference presentation types).
- Headless tests exercise exactly the production sim code.
