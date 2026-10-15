# ADR-0060: CI v4 — macOS jobs on main, nightly and manual dispatch only

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1
- **Supersedes:** ADR-0054 (keeps the feature matrix, fast smoke test, cross-OS determinism job,
  duration target, outage policy and release workflow; changes only when macOS jobs run)

## Context

The user decided (2026-10-15) that macOS CI runs only on `main` and nightly, not on every push
(Apple Silicon runner minutes are expensive). Development happens on a feature branch, and
GitHub's scheduled workflows run only on the default branch, so without an extra trigger the
feature branch would never be checked on macOS.

## Options considered

1. **macOS only on push to main + schedule** — strictly as stated, but feature-branch work
   would get no macOS signal until merged.
2. **Plus manual `workflow_dispatch` of the nightly workflow on a branch (chosen)** — the agent
   triggers it once per milestone completion; cost stays at a handful of runs per milestone.

## Decision

- `ci.yml` on push and pull_request: Linux and Windows `check` jobs (default + `dev`
  features), fmt (Linux), `sim-isolation`, `render-smoke` (Linux), cross-OS determinism over
  Linux + Windows. The `check-macos` job runs only when `github.ref == 'refs/heads/main'`.
- `nightly.yml` on `schedule` (03:17 UTC, default branch) and `workflow_dispatch` (any branch):
  macOS `check` (default + `dev`), macOS smoke (informational), three-OS cross-OS determinism
  comparison, `--ignored` soak tests, informational benches, `trace_tracy` build on Linux.
- The `milestone-completion` skill dispatches `nightly.yml` on the working branch once per
  milestone and requires its macOS jobs to be green (or records "☑ CI-pending").
- `release.yml` (tags `v*` + `workflow_dispatch` with `publish: false`) builds all three targets
  as before.

## Consequences

- macOS regressions surface at milestone boundaries (or on main/nightly) instead of per push;
  acceptable given the cost; three-OS determinism is checked at the same cadence.
