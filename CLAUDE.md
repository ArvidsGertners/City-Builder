# CLAUDE.md — City Builder

Instructions for any agent (or human) working in this repository.

## Session protocol (mandatory)

1. **At the start of every session, before doing anything else**, read:
   - this file,
   - `docs/ARCHITECTURE.md`,
   - `docs/PROGRESS.md`.
   Then continue with the step named under **Next step** in `docs/PROGRESS.md`.
2. Read the topic docs relevant to the task before touching that area:
   `docs/DATA_MODEL.md`, `docs/ALGORITHMS.md`, `docs/GAME_DESIGN.md`,
   `docs/ART_DIRECTION.md`, `docs/PERFORMANCE.md`, `docs/TESTING.md`, `docs/RISKS.md`,
   and the ADRs in `docs/adr/` that cover it (index in `docs/adr/README.md`).
3. **After every milestone and before ending any session**, update `docs/PROGRESS.md`:
   what was done, what is in progress, known bugs, and the *exact* next step
   (file/function level, so a cold start can resume without guessing).
4. Commit per milestone and at meaningful checkpoints (see *Git* below).

## Decisions and ADRs

- ADRs live in `docs/adr/NNNN-kebab-title.md` (template: `docs/adr/0000-template.md`).
- **Never silently deviate from an accepted ADR.** If a decision must change, write a new
  ADR that supersedes it, mark the old one `Superseded by NNNN`, and update the index.
- New significant decisions (new crate, new algorithm family, new data format, new
  cross-cutting rule) need an ADR before the code lands.
- Gameplay balance numbers live in `docs/GAME_DESIGN.md` and `crates/cb_sim/data/*.ron`;
  changing numbers does not need an ADR, changing rules does.

## Build, test, run

Toolchain is pinned in `rust-toolchain.toml` (stable, edition 2024).

```bash
# Linux system deps (Ubuntu/Debian)
sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev

cargo run -p cb_game                         # run the game (dev profile, opt-level 1 + deps at 3)
cargo run -p cb_game --features dev          # with debug tooling (perf overlay, inspector, stepping)
cargo run -p cb_game --release               # release build
cargo run -p cb_game --features dev -- --procgen-gallery   # procgen debug scene
cargo run -p cb_game -- --seed 1234 --sandbox              # new game with seed, sandbox mode

cargo fmt --all                              # format
cargo fmt --all -- --check                   # CI format check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace                       # all tests (pure crates are fast; cb_sim is headless)
cargo test -p cb_geometry                    # one crate
cargo test -p cb_sim --test traffic_multi_day -- --ignored   # slow headless soak tests

cargo run -p cb_sim --release --example headless_bench -- --days 10 --city large
scripts/screenshot.sh                        # Xvfb + lavapipe render smoke test + screenshots
```

A milestone is only **done** when `cargo fmt --check`, `cargo clippy ... -D warnings`
and `cargo test --workspace` pass locally *and* in CI, its acceptance criteria in
`docs/PROGRESS.md` are met, and `PROGRESS.md` is updated.

## Repository layout (short)

- `crates/cb_core` — IDs, RNG, stable hashing, spatial hash, units, time, money. No Bevy.
- `crates/cb_geometry` — splines, polylines, polygons, booleans, triangulation, subdivision, mesh data. No Bevy.
- `crates/cb_terrain` — heightfield, world generation, deformation stamps, water, materials, raycast. No Bevy.
- `crates/cb_network` — road cross-sections, segment geometry, vertical profiles, junctions, lanes, lane graph. No Bevy.
- `crates/cb_traffic` — routing, route cache, vehicle dynamics, intersection control, flow model. No Bevy.
- `crates/cb_procgen` — buildings, vehicles, props, road/junction/bridge meshes, icons. No Bevy.
- `crates/cb_l10n` — Fluent bundles and locale formatting. No Bevy.
- `crates/cb_audio` — procedural audio synthesis to PCM/WAV buffers. No Bevy.
- `crates/cb_sim` — the headless simulation (bevy_ecs/bevy_app only, **no rendering**).
- `crates/cb_game` — the game binary: rendering, camera, input, tools, UI, audio playback, debug.

Full tree and responsibilities: `docs/ARCHITECTURE.md` §3.

## Hard rules

### Architecture
- **Simulation never depends on rendering.** `cb_sim` and the pure crates must not depend
  on `bevy_render`, `bevy_pbr`, `bevy_ui`, `bevy_egui`, windowing or audio. CI builds
  `cb_sim` alone to enforce it.
- **All world mutations go through `PlayerCommand`s** (see ADR-0015). Presentation code
  reads sim state and emits commands; it never writes sim components or resources. Sim
  types keep fields `pub(crate)` with `pub` read accessors so this is compiler-enforced
  (ADR-0005).
- **No grid or tile-based placement anywhere.** Internal acceleration structures
  (spatial hash, field rasters) are allowed and are never exposed as placement units
  (ADR-0013, ADR-0028).
- Pure crates expose pure functions over plain data (`fn generate(input, params, seed) -> Output`).
  Keep Bevy types out of them; use `glam` (same version as `bevy_math`).
- One Bevy plugin per feature; systems are placed in the documented system sets with
  explicit ordering (`docs/ARCHITECTURE.md` §5). No unordered conflicting systems in sim
  schedules — ambiguity detection is set to `Error` for `SimEdit` and `SimTick`.
- Cross-system communication uses Bevy `Message`s (buffered) or observers (`On<E>`) as
  documented; no ad-hoc global state, no `static mut`.
- Incremental updates only: regenerate meshes, terrain, coverage, routes and connectivity
  for changed areas (dirty sets / change detection), never "rebuild everything" in steady state.

### Determinism (ADR-0017)
- Seeds are derived with `cb_core::hash::derive_seed(world_seed, Domain::X, id, extra)`.
  Never use `std::collections::hash_map::RandomState`, `rand::thread_rng`, time-based seeds
  or pointer addresses for anything that affects sim state or generated content.
- Never let `HashMap`/`HashSet` iteration order affect results. Use `BTreeMap`, sorted
  vectors, or collect-and-sort by stable ID. ECS query iteration order is **not** stable
  across save/load: sort by stable ID whenever order matters (RNG draws, allocation,
  float accumulation).
- Sim randomness uses counter-based RNG: `SimRng::for_event(stream, tick, id)`.
- Sim code must not read wall-clock time, frame time, camera, or settings that are not part
  of the saved sim configuration.

### Error handling
- **No `unwrap()`/`expect()` in game or sim logic.** Allowed only in tests, in `const`/startup
  code where failure is a programmer error *and* documented with `// INVARIANT:`, and in
  build scripts. Clippy `unwrap_used`/`expect_used` are denied outside tests.
- Library crates return `Result<_, ThisCrateError>` (`thiserror`). Bevy systems that can
  fail return `Result` and log via the configured error handler; never panic on bad input
  (player input, save files, settings files).
- Validation failures for player actions are values (`ValidationError` with a localization
  key), not errors in logs.

### Code style
- `cargo fmt` (rustfmt.toml in repo), clippy pedantic subset configured in workspace lints.
- Doc comments (`///`) on every public item; module-level `//!` docs describing
  responsibility. Explain *why*, not *what*.
- Naming: types `UpperCamelCase`, modules `snake_case`, Bevy plugins `FooPlugin`,
  system sets `FooSet`, messages are nouns in past tense for facts (`SegmentRemoved`) or
  imperative for requests (`RequestRoute`).
- Units in names when ambiguous: `_m`, `_mps`, `_s`, `_min`, `_deg`/`_rad`. World space is
  meters, Y-up, right-handed; ground plane is XZ; 2D geometry uses `Vec2(x, z)`
  (ADR-0033).
- Money is `cb_core::Money` (i64 cents). Never use floats for balances.
- Prefer `SmallVec` for tiny per-object lists; avoid per-tick allocations in hot loops
  (reuse buffers stored in `Local<>` or resources).
- `#[must_use]` on pure functions returning values; `#[inline]` only with a benchmark.
- No new dependency without updating ADR-0023 (crate selection) in the same commit.

### Localization (ADR-0021)
- **No user-facing string literals in code.** Every string shown to the player goes
  through `t!("key")` / `L10n::tr` with keys in `crates/cb_l10n/locales/{en,de}/*.ftl`.
  A test fails if a key is missing in any locale.
- Numbers, money, dates and times are formatted with `cb_l10n::format::*`, never `format!`.

### Tests
- Every bug fix comes with a regression test where feasible.
- Geometry, network, traffic and procgen changes need unit tests including edge cases
  (degenerate, self-intersecting, tiny, sharp-curve inputs).
- Sim features need a headless integration test in `crates/cb_sim/tests/`.
- See `docs/TESTING.md` for the full strategy and naming.

### Performance
- Budgets are in `docs/PERFORMANCE.md`. Any system expected to exceed 0.5 ms per frame
  needs a `tracing` span (`info_span!`) and a note in the perf doc.
- Hot paths: no `Box<dyn>` per entity, no `String` building, no `Vec` growth per tick.

## Git

- Work on the designated feature branch; never push to other branches without permission.
- Commit messages: `area: imperative summary` (e.g. `network: generate junction cut-backs`),
  body explains why. Milestone commits: `M<N>: <summary>` with the acceptance checklist.
- Do not commit generated meshes, saves, logs, crash reports or `target/`.

## Debugging help

- Logs: `<data_dir>/logs/city-builder.log` (rotating). Crash reports:
  `<data_dir>/crashes/crash-<timestamp>.txt` — copy-paste friendly, include version,
  git hash, OS, GPU adapter, settings summary, sim summary, backtrace, last 200 log lines.
- `<data_dir>` = `$XDG_DATA_HOME/city-builder` (Linux), `%APPDATA%\CityBuilder` (Windows),
  `~/Library/Application Support/CityBuilder` (macOS); or `./userdata` if a `portable.txt`
  file sits next to the executable.
- `RUST_LOG`-style filters: `--log "info,cb_traffic=debug"` or `CB_LOG` env var.
