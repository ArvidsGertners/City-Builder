---
name: milestone-completion
description: Finish a City Builder milestone — verify acceptance criteria with evidence, run fmt/clippy/tests, visual verification with screenshots, performance check, update PROGRESS.md and docs, commit "M<N>: …", push, confirm CI on all three platforms. Use whenever a milestone (or spike phase) is believed complete, and before ending a session mid-milestone (steps 6–9 only).
---

# Milestone completion

Follow every step in order. Do not mark a milestone done while any step is failing;
record the failure in `docs/PROGRESS.md` instead.

## 1. Identify the milestone
- Open `docs/PROGRESS.md`, find the milestone section `### M<N> — …` and copy its
  acceptance checklist into a scratch list.

## 2. Verify every acceptance criterion with evidence
For each checkbox, produce concrete evidence and note it next to the item:
- automated test name(s) that prove it (`cargo test -p <crate> <test_name>` output), or
- a headless scenario/bench result with numbers, or
- a screenshot ID from step 4 (for visual criteria), or
- a log excerpt / command output.
A criterion without evidence is not done. If a criterion turned out to be wrong or
infeasible, do not silently drop it: either fix the plan with the `write-adr` skill
(when a decision changes) or ask the user (when scope changes).

## 3. Quality gates (all must pass locally)
```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo build -p cb_sim && scripts/check_sim_isolation.sh      # sim has no render deps
```
Also run the milestone's `#[ignore]` soak tests if PROGRESS.md lists any.

## 4. Visual verification (milestones with visual output)
Run the `visual-verification` skill for this milestone. It produces
`docs/screenshots/M<N>/` with the required shot list and a review in `index.md`.
Any "must be visible" item that is not visible blocks completion.

## 5. Performance check (from M2 on)
- Run `cargo run -p cb_sim --release --example headless_bench -- --preset <milestone preset>`
  and, from M2 on, `cargo run -p cb_game --release -- --benchmark <scene>` (offscreen
  under Xvfb+lavapipe gives CPU-side numbers only; GPU numbers come from the user's
  machines — ask for a run on Windows/Linux GPU and Apple Silicon at M2, M4, M9, M11, M16).
- Compare against `docs/PERFORMANCE.md` budgets; record numbers in the PROGRESS.md log.
  Over budget → fix, or record as a known issue with a plan.

## 6. Update documentation
- `docs/PROGRESS.md`: milestone status ☑ (or ◐ with what remains), tick each criterion,
  "Known bugs" (with repro), "Next step" = exact file/function-level first action of the
  next milestone, log entry with date, evidence summary and perf numbers.
- Any doc whose content changed: ARCHITECTURE, DATA_MODEL, ALGORITHMS, GAME_DESIGN,
  ART_DIRECTION, PERFORMANCE, TESTING, RISKS (retire/raise risks), README (controls).
- Decision changes → `write-adr` skill (superseding ADR), never silent edits.
- New recurring procedure discovered during the milestone → create a skill
  (`.claude/skills/<name>/SKILL.md`) per CLAUDE.md.

## 7. Commit
```bash
git add -A
git status --short   # check: no target/, saves, logs, stray large files (screenshots only under docs/screenshots/)
git commit -F <msgfile>
```
Message format:
```
M<N>: <one-line summary>

<what was built, 3–8 bullets>

Acceptance:
- [x] <criterion> — <evidence>
...

Perf: <key numbers>
Known issues: <none | list>
```
plus the attribution trailer lines required by the current session instructions.

## 8. Push
`git push -u origin <designated branch>`; on network failure retry up to 4 times with
2 s, 4 s, 8 s, 16 s backoff. Never push to another branch.

## 9. Confirm CI
Check the workflow run for the pushed commit (GitHub MCP tools: list workflow runs / get
job logs). All jobs (ubuntu, windows, macos-arm64, sim-isolation, render-smoke) must be
green. If red: diagnose, fix, re-run steps 3, 7–9. Record the CI run URL in the
PROGRESS.md log.

## 10. Continue
Start the next milestone at the "Next step" you just wrote (no pause for approval unless
a decision genuinely needs the user).
