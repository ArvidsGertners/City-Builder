# ADR-0020: Save format, settings persistence and migration

- **Status:** Superseded by ADR-0044
- **Date:** 2026-10-15
- **Milestone:** M5 (core), M15 (UI, autosave)

## Context

serde-based, versioned saves with migration from older versions; store only simulation
state, terrain seed and modifications, road splines/settings, intersection settings, plot
polygons, transit lines, time/weather state and seeds — never meshes; vehicles may be
regenerated. Autosave with rotating slots. Settings persisted separately.

## Options considered

Body encoding:
1. **RON/JSON** — readable, huge for 50k citizens, slow.
2. **bincode** — fast but project maintenance uncertain; not self-describing.
3. **postcard** — compact, stable; not self-describing → every field change needs a frozen
   copy of old structs for migration.
4. **MessagePack via `rmp-serde` with named fields (chosen)** — self-describing (additive
   fields via `#[serde(default)]` need no migration), compact enough with compression, and
   structural migrations can operate on a dynamic `rmpv::Value` tree.

Compression: zstd (C dependency, best ratio) vs **lz4_flex (chosen; pure Rust, very fast)**.

## Decision

- File layout: `b"CBSAVE\0\x01"` magic · `u32 format_version` · `u32 header_len` · header
  (MessagePack: game version, git hash, city name, seed, population, money, date, play time,
  created timestamp) · body (LZ4-compressed MessagePack of `SaveData`) · `u64` checksum
  (stable hash of the compressed body).
- `SaveData` = snapshot types in `cb_sim::save::snapshot` (DATA_MODEL §13), each entity
  stored by stable ID, sorted, so the snapshot is canonical (used for `state_hash`).
- Migration: `migrate(value: rmpv::Value, from: u32) -> Result<Value, MigrationError>`
  chain of `vN → vN+1` functions; additive changes use serde defaults without a version
  bump; breaking changes bump the version and add a migration + a golden fixture.
  Fixtures for every version live in `cb_sim/tests/fixtures/` and are loaded in CI.
- Writing: snapshot synchronously (fast), serialize/compress/write on the IO pool to
  `slot.tmp` then atomic rename. Autosave interval configurable (default 10 min real time),
  3 rotating slots, skipped while a modal command is in progress.
- Loading: header first (load menu shows metadata without decompressing body), then body;
  any error is reported with a localized message and logged with details; never panics.
- **Settings**: `settings.ron` (RON, human-editable) with `version` field; unknown/invalid
  fields fall back to defaults with a warning; written atomically. Bevy's `bevy_settings`
  is not used (new and still changing; we need explicit paths, versioning and validation).

## Consequences

- Small, fast saves (estimated 3–8 MB for a max city), resilient to additive changes.
- Migration code operates on untyped trees; covered by fixture tests.
