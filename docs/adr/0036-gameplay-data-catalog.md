# ADR-0036: Gameplay data catalog in embedded RON

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M3–M7

## Context

Road types, zones, building families, services, utilities, transit, progression tiers,
economy and weather tables must be tunable without code changes, validated, and available
to headless tests without an asset server.

## Options considered

1. **Constants in code** — type-safe but tuning means recompiling and scattered numbers.
2. **Bevy assets loaded at runtime** — hot reload, but async loading and asset server in
   headless tests.
3. **RON files embedded with `include_str!`, parsed at startup into a validated `Catalog`
   (chosen)**; dev feature adds optional file-watch reload from the source directory.

## Decision

- `crates/cb_sim/data/{roads,zones,buildings,services,utilities,transit,progression,economy,
  weather}.ron` → `Catalog` resource; typed IDs (`RoadTypeId`, …) by stable string keys
  mapped to dense indices at load.
- Validation at startup and in a unit test (references resolve, ranges sane, every
  unlockable referenced by exactly one tier, localization keys exist).
- Saves store type keys as strings so catalog reordering never breaks saves.
- Future modding can layer additional RON files on top of the embedded catalog.

## Consequences

- Tuning without code edits, deterministic headless availability, a natural path to modding.

## Amendments

- 2026-10-15 (plan review): the check that catalog localization keys exist runs in a
  `cb_game` test (cb_sim does not depend on cb_l10n). The save header stores `catalog_hash`
  (ADR-0044).
