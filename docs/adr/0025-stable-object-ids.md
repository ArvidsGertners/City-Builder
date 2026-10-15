# ADR-0025: Stable object IDs and entity mapping

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2

## Context

Bevy `Entity` values are not stable across save/load and can be recycled; undo needs to
restore objects by identity; procgen seeds derive from object identity; commands and saves
must reference objects robustly; future networking needs shared identities.

## Decision

- Typed `u32` IDs per kind (`NodeId`, `SegmentId`, `PlotId`, `LotId`, `BuildingId`,
  `StopId`, `LineId`, `HouseholdId`, `CitizenId`, …) from `IdAllocators` (monotonic,
  persisted, **never reused** — undo restores the original ID).
- Every sim entity carries `ObjectId<K>`; `IdMap<K>` maps ID → `Entity` (lookup only; never
  iterated for logic). Maintained by observers on add/remove.
- Commands, snapshots, messages to the presentation, cross-references between sim
  components and seeds use IDs; `Entity` is used only for in-frame ECS access.
- Dense SoA stores (vehicles, lanes) use generational IDs internal to their store; they are
  not persisted as identities.

## Consequences

- Robust saves/undo/seeds; a small lookup cost (`IdMap`) when following references.
- `u32` space (4 billion per kind) is ample since IDs are never reused.

## Amendments

- 2026-10-15 (plan review): IDs are `NonZeroU32` newtypes so `Option<Id>` stays 4 bytes.
