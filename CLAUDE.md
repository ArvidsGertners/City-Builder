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
   `docs/CONSTANTS.md`, `docs/ASSUMPTIONS.md`, and the ADRs in `docs/adr/` that cover it
   (index in `docs/adr/README.md`; follow "Superseded by" links to the current ADR).
   `docs/PLAN_REVIEW.md` explains why the plan looks the way it does.
3. **After every milestone and before ending any session**, update `docs/PROGRESS.md`:
   what was done, what is in progress, known bugs, and the *exact* next step
   (file/function level, so a cold start can resume without guessing).
4. Commit per milestone and at meaningful checkpoints (see *Git* below).
5. Finish every milestone with the `milestone-completion` skill (below); never mark a
   milestone done by hand. Status `☑ᵤ` (done, pending user verification) never blocks
   progress: queue requests in the PROGRESS.md *User verification queue* and continue
   (ADR-0058). Track estimate vs actual; over 1.5× apply the scope-cut ladder.

## Skills (recurring procedures)

- **When a multi-step procedure recurs** (e.g. finishing a milestone, adding a building
  type, adding an overlay, adding a player command, writing an ADR), **create a Claude Code
  skill** for it in `.claude/skills/<name>/SKILL.md` (YAML front matter `name` +
  `description` saying when to use it, then numbered steps with exact commands and files)
  and use that skill from then on. Rule of thumb: the second time you do the same
  multi-step thing, write the skill before doing it a third time. Keep skills current when
  the procedure changes.
- Existing skills:
  - `milestone-completion` — acceptance evidence, quality gates, visual verification, perf,
    PROGRESS.md, commit, push, CI on all platforms.
  - `write-adr` — new or superseding ADR with index and cross-reference updates.
  - `visual-verification` — screenshot suite render, self-review, save to `docs/screenshots/`.
- Candidates to create on first recurrence: `add-building-type`, `add-overlay`,
  `add-player-command`, `add-service-type`, `add-l10n-strings`, `run-spike`.

## Visual verification (mandatory for visual output)

- After any milestone with visual output (and after any change to shaders, lighting,
  palettes or procgen), render the milestone's screenshot suite from several camera angles —
  from M11 also times of day, from M12 also weather — using the `visual-verification`
  skill, **open and check every image yourself** against `docs/ART_DIRECTION.md` and the
  milestone's must-be-visible list, fix problems, and only then mark the milestone done.
- Save the images and a short review (`index.md`) in `docs/screenshots/<milestone>/` (e.g.
  `docs/screenshots/M2/`) and commit them so the user can review them too.

## Risk spikes (Phase S, right after M1 — ADR-0058)

- The highest-risk systems are proven by throwaway spikes before the real system is built:
  **S-Render** (material pipeline/vertex format/render scale/shader validation), **S-Time**
  (time model, trip scale), **S-J** (junction geometry), **S-P** (plots → lots → buildings),
  **S-G** (road–terrain interface incl. bridges), **S-T** (traffic at scale). Scope, cases,
  pass/fail numbers, time-box and fallback are in `docs/PROGRESS.md` (Phase S).
- Spikes live in `spikes/<name>/` (not workspace default members); tag `spike/<name>` before
  removing the code; record the outcome in PROGRESS.md and write a superseding ADR if a
  fallback is taken. Spike code is never promoted into `crates/` as-is.

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
**Required platforms** (ADR-0037): Windows 10/11 x86-64, Linux x86-64 (X11 + Wayland),
**macOS 14+ on Apple Silicon** (`aarch64-apple-darwin`, Metal). Every change must build and
pass tests on all three (CI runs all three).

```bash
# Linux system deps (Ubuntu/Debian)
sudo apt-get install -y pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
# macOS: Xcode command line tools only
xcode-select --install
# Windows: MSVC build tools (Visual Studio 2022 "Desktop development with C++")

cargo run -p cb_game                         # run the game (dev profile, opt-level 1 + deps at 3)
cargo run -p cb_game --features dev          # with debug tooling (perf overlay, inspector, stepping)
cargo run -p cb_game --release               # release build
cargo run -p cb_game --features dev -- --procgen-gallery   # procgen debug scene
cargo run -p cb_game -- --seed 1234 --sandbox              # new game with seed, sandbox mode

cargo fmt --all                              # format
cargo fmt --all -- --check                   # CI format check
cargo clippy --workspace --all-targets -- -D warnings                  # default features
cargo clippy --workspace --all-targets --features dev -- -D warnings   # debug tooling (never --all-features, ADR-0054)
cargo test --workspace                       # all tests (pure crates are fast; cb_sim is headless)
cargo test --workspace --features dev
cargo test -p cb_geometry                    # one crate
cargo test -p cb_sim --test traffic_multi_day -- --ignored   # slow headless soak tests

cargo run -p cb_sim --release --example headless_bench -- --days 10 --city large
cargo run -p cb_sim --release --example determinism_probe           # hashes compared across OSes in CI
cargo run -p cb_game --release -- --benchmark bench_city             # 60 s flythrough, perf report (real GPU)
cargo run -p cb_game --release -- --benchmark bench_city --no-gpu-work  # CPU budgets in the container
cargo run -p cb_game --release -- --smoke-test 60 --resolution 640x360
scripts/screenshot.sh M2                     # Xvfb + lavapipe: render screenshot suite M2
scripts/check_sim_isolation.sh               # fails if cb_sim depends on rendering crates
scripts/macos_bundle.sh                      # macOS: build CityBuilder.app (release profile `dist`)
```

A milestone is only **done** when fmt check, both clippy runs and both test runs pass
locally *and* in CI on all three OSes, its acceptance criteria in `docs/PROGRESS.md` have
evidence, visual verification is done (if visual), and `PROGRESS.md` is updated in the same
commit.

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

### Platforms (ADR-0037, ADR-0052, ADR-0040, ADR-0053)
- Platform-specific code only in `cb_game::platform` (paths, modifier mapping, gestures,
  window quirks). No `#[cfg(target_os)]` elsewhere. Platform logic is a pure function of
  injected inputs so it is tested on every OS.
- **Never hard-code Ctrl.** Shortcuts use the virtual `Mods::PRIMARY` (Cmd on macOS, Ctrl
  elsewhere); mnemonic letter shortcuts bind *logical* keys, positional controls (WASD,
  Q/E) bind *physical* keys. Shortcut labels in UI come from the platform glyph formatter.
- **Logical pixels** for all UI/input/tool math; physical pixels only for render targets.
  Picking goes through `picking::cursor_ray` (DPI- and render-scale-aware).
- Files go only to the per-OS locations from ADR-0040 (`AppPaths`), never next to the
  executable or inside the `.app` bundle (except portable mode).
- Trackpad parity: every camera action must be possible without a mouse (pinch, two-finger
  scroll/rotate, ⌥/Alt-drag, Q/E).

### Architecture
- **Simulation never depends on rendering.** `cb_sim` and the pure crates must not depend
  on `bevy_render`, `bevy_pbr`, `bevy_ui`, `bevy_egui`, windowing or audio. CI builds
  `cb_sim` alone to enforce it.
- **All world mutations go through `PlayerCommand`s** (see ADR-0055). Presentation code
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

### Determinism (ADR-0043) — sim state must be bit-identical on Windows, Linux and macOS
- Seeds: `cb_core::hash::derive_seed(world_seed, Domain::X, id, extra)` (sequential
  absorption). Zoned buildings seed from `LotId`. Never `RandomState`, `rand::thread_rng`,
  time-based seeds or pointer addresses.
- Event randomness: `SimRng::for_event(stream, purpose, tick_or_day, id)` — every call site
  has its own `purpose`.
- **Maps**: std `HashMap`/`HashSet` are disallowed in sim crates (clippy). Use `LookupMap`
  (no iteration), `BTreeMap`, or dense `Vec`s; iterate sorted by stable ID wherever order
  matters (ECS query order is not stable across save/load).
- **Math**: transcendental functions only via `cb_core::math` (libm); no `glam`
  `Vec3A/Vec4/Mat4/Quat/Affine3A` and no `mul_add` in pure crates or `cb_sim`
  (clippy-enforced).
- **Parallelism**: `SIM_PARTITIONS = 64` fixed partitions, reductions in partition order —
  never partition by core count.
- Sim code never reads wall-clock time, frame time, camera, window, graphics settings or the
  day-length pacing setting.
- Structural `SimEdit` work completes before the next tick (never split across frames);
  commands record `apply_tick`.

### Units (ADR-0042)
- Tick `dt = 0.1 sim s`; 1 game minute = 2 sim s. Traffic flows/capacities in veh per
  **sim-hour**; EMAs, epochs, timeouts in **sim seconds**; economy/demand/incidents per **game**
  hour/day. Put the unit in the name (`_per_sim_h`, `_sim_s`, `_game_h`).

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
- Constants with stability/balance impact get a row in `docs/CONSTANTS.md` (owning test +
  target metric).

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

- Locations (ADR-0040):

  | | Windows | Linux | macOS |
  |---|---|---|---|
  | Settings | `%APPDATA%\CityBuilder\settings.ron` | `~/.config/city-builder/settings.ron` | `~/Library/Application Support/CityBuilder/settings.ron` |
  | Saves | `%APPDATA%\CityBuilder\saves\` | `~/.local/share/city-builder/saves/` | `~/Library/Application Support/CityBuilder/saves/` |
  | Logs | `%LOCALAPPDATA%\CityBuilder\logs\` | `~/.local/state/city-builder/logs/` | `~/Library/Logs/CityBuilder/` |
  | Crash reports | `%LOCALAPPDATA%\CityBuilder\crashes\` | `~/.local/state/city-builder/crashes/` | `~/Library/Logs/CityBuilder/crashes/` |

  (Linux paths honor `XDG_*_HOME`.) Portable mode: `portable.txt` next to the executable
  (macOS: next to the `.app`) → `./userdata/`. Dev/test override: `CB_USERDATA=<dir>`.
- Crash reports (`crash-<timestamp>.txt`) are copy-paste friendly: version, git hash,
  OS + arch, GPU adapter/backend, settings summary, sim summary, backtrace, last 200 log lines.
- `RUST_LOG`-style filters: `--log "info,cb_traffic=debug"` or `CB_LOG` env var.
