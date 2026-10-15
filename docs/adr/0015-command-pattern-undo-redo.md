# ADR-0015: Command pattern and undo/redo

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M3 (roads/plots), later milestones add commands for their actions

## Context

All player construction actions (roads, plots, buildings, decorations, intersections,
transit stops and lines, demolishing) must be undoable with Ctrl+Z/Ctrl+Y, with refunds,
limited history, cleared on load. The same mechanism should serve determinism, replays and
future lockstep multiplayer.

## Options considered

1. **Snapshot the world per action** — trivial correctness, huge memory/time cost.
2. **Inverse operations per command with recorded effects (chosen)** — compact, precise.
3. **Event sourcing of the whole sim** — replays everything; too slow for undo in a
   running city (sim-driven changes would also replay).

## Decision

- Every player action is a serializable `PlayerCommand` (stable IDs only, ADR-0025) queued
  as a `Message` and applied in `EditSet::Commands` in `CommandSeq` order, tagged with the
  tick.
- Undoable commands implement `Command { validate, apply, undo, redo, label }`. `apply`
  records an **effect log**: allocated IDs, removed-object snapshots (using the save
  snapshot types, so all components round-trip), modified authored fields (old values),
  money delta. `undo` reverts using the log and restores objects **with their original
  IDs** (IDs are never reused). `redo` re-applies with the recorded IDs so later history
  entries stay valid.
- Compound actions (e.g. road placement that splits two segments, creates a junction and
  removes trees) are one command; the editor operations it calls append to the same log.
- Sim-driven consequences (buildings grown on a plot, residents) are not commands. Undoing
  a plot removes its lots and any grown buildings (the snapshot records the plot only;
  growth restarts on redo). Undoing a demolish restores snapshotted buildings and their
  households where the snapshot captured them.
- `UndoHistory`: depth 100, cleared on load/new game; redo cleared on any new command.
  Policy commands (taxes, budgets, loans) are commands but not undoable.
- Undo validates preconditions (objects referenced by the log still exist/are unchanged);
  on failure it reports `undo.blocked` with a reason and keeps the stack.
- Refund on undo = recorded cost (100 %).

## Consequences

- Precise, memory-light undo; the command stream doubles as a replay log.
- Every new player action must ship with its command + undo test (TESTING.md).
