# ADR-0055: Command pattern and undo/redo v2

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M2 (command framework without history), M3 (undo/redo, snapshot types)
- **Supersedes:** ADR-0015 (keeps serializable `PlayerCommand`s with stable IDs, `Command`
  trait with effect logs, restore with original IDs, compound commands, depth 100, cleared on
  load, policy commands not undoable)

## Context

Review findings G-B5, E2, E3, G-D6, G-S7, G-B1, G-B2, G-B6: 100 % undo refunds at any age
versus 0 % demolish refunds after a day was an exploit; the command framework was scheduled in
M3 although M2 tools must already emit commands; undo snapshots used save types that arrived in
M5; replay needed recorded application ticks; road geometry editing, per-approach controls and
free decorations had no commands.

## Decision

- **M2** ships the command framework (`PlayerCommand`, `CommandQueue`, `CommandSeq`,
  `CommandResult`, `ValidationError` with l10n keys, `apply_tick` recording); **M3** adds
  history, undo/redo and the `cb_sim::save::snapshot` types (no file format yet).
- New commands: `EditRoadGeometry { target: Node | Segment, new positions / control points }`
  (shares validation, junction regeneration and dependent re-pointing with `BuildRoad`),
  `SetApproachControl`, `PlaceProp` / `RemoveProp` (free decorations), `SetSimConfig`
  (non-undoable), `EmptyLandfill`.
- **Undo window**: entries expire 1 game day after application. Refund = 100 % within the first
  game hour, then linearly decreasing to 25 % (the demolish refund) at expiry. The undo label
  shows the refund amount.
- Undoing an action whose objects gained sim-driven dependents (e.g. grown buildings,
  residents) is allowed: buildings are removed, households go to `homeless_queue` (they may
  re-home elsewhere); the confirmation dialog lists consequences.
- Every command records `apply_tick`; the command log is versioned with the save format and a
  test replays a log to an identical state hash.

## Consequences

- No refund exploit; earlier availability of the command framework; replayable sessions.
