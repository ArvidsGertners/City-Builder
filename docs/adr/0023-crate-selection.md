# ADR-0023: Crate selection and dependency policy

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1; updated whenever a dependency is added

## Context

"Minimal dependencies; justify each crate." Determinism and long-term maintenance favor
owning small, critical algorithms; complex, well-tested domains (robust polygon booleans,
localization, compression) favor proven crates.

## Policy

- A new dependency needs: a gap we cannot fill in < ~300 lines of tested code, active
  maintenance, permissive license (MIT/Apache-2.0/BSD/Zlib/CC0), no C/C++ toolchain
  requirement unless unavoidable, and an entry below in the same commit.
- Prefer crates already in Bevy's dependency tree (no extra compile cost).
- Versions are declared once in `[workspace.dependencies]`; `Cargo.lock` is committed.

## Selected crates

| Crate | Version | Used by | Why | Alternatives rejected |
|---|---|---|---|---|
| `bevy` | 0.20 | cb_sim (core features), cb_game | Engine (ADR-0001) | — |
| `bevy_egui` | 0.43 | cb_game | Game + debug UI (ADR-0024); supports Bevy 0.20 | bevy_ui (see ADR-0024) |
| `glam` | 0.33 (= bevy_math's) | pure crates | Math types identical to Bevy's | own vectors |
| `serde` (+derive) | 1 | most crates | Save/settings/catalog serialization | — |
| `ron` | 0.12 | cb_sim (catalog), cb_game (settings) | Human-readable data/settings, Bevy-ecosystem standard | serde_json (no comments, less ergonomic) |
| `rmp-serde`, `rmpv` | 1.3 | cb_sim (save) | Self-describing compact save body + dynamic migration (ADR-0020) | bincode, postcard |
| `lz4_flex` | 0.14 | cb_game (save IO) | Pure-Rust fast compression | zstd (C dep), flate2 |
| `i_overlay` | 9 | cb_geometry | Robust polygon booleans/offsetting (integer-snapped) for plots, lots, junctions | own Weiler–Atherton (fragile), `geo` (large tree), clipper2 (C++) |
| `thiserror` | 2 | all library crates | Error enums | anyhow (only in binary/tests if at all) |
| `smallvec` | 1 | most crates | Small inline lists in hot data (already in Bevy's tree) | Vec |
| `tracing` | 0.1 | all | Structured logging (Bevy uses it) | log |
| `tracing-subscriber` | 0.3 | cb_game, tests | Layers, filters (in Bevy's tree) | — |
| `tracing-appender` | 0.2 | cb_game | Non-blocking rolling file writer | own writer thread |
| `fluent-bundle`, `unic-langid` | 0.16 / 0.9 | cb_l10n | Localization (ADR-0021) | gettext, own tables |

Explicitly **not** used (own implementations instead): `rand`/`rand_*` (ADR-0017),
`noise`/`fastnoise-lite` (deterministic f32 gradient noise in `cb_terrain::noise`),
`rstar` (ADR-0013), `petgraph` (graphs are domain-specific SoA), `earcutr` (ear clipping
in `cb_geometry`), `leafwing-input-manager` (ADR-0030), `directories` (paths in
`bootstrap/paths.rs`), `chrono`/`time` (UTC formatting helper in `cb_core::time`),
`proptest`/`criterion` (seeded randomized tests; bench example), `bevy_hanabi` (GPU
particles via our own vertex-animated materials), `bevy-inspector-egui` (ADR-0031, until it
supports Bevy 0.20; then behind feature `inspector`).

Dev/CI tooling (not linked into the game): GitHub Actions `actions/checkout`,
`dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `softprops/action-gh-release`.

## Consequences

- Small, auditable dependency tree beyond Bevy; more code of our own in core utilities,
  all covered by tests.

## Amendments

- 2026-10-15 (plan review): added `libm` (pure-Rust portable transcendental math for
  cross-platform determinism, ADR-0043; tiny, no dependencies). Transitively via bevy_egui's
  `manage_clipboard` feature: `arboard` (clipboard on Windows/macOS/X11) and
  `smithay-clipboard` (Wayland); bevy_egui is used with `default-features = false`
  (ADR-0051). A minimal test shrinker (halving counts/ranges) lives in `cb_core::test_util`
  instead of adopting proptest.

| Crate | Version | Used by | Why |
|---|---|---|---|
| `libm` | 0.2 | all pure crates, cb_sim (via `cb_core::math`) | portable `sinf/cosf/expf/powf/atan2f…` identical on x86-64 and aarch64 |
| `arboard`, `smithay-clipboard` | (bevy_egui's) | cb_game (transitive) | copy crash reports / seeds to clipboard |
