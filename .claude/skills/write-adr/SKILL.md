---
name: write-adr
description: Write a new Architecture Decision Record for City Builder, or supersede an existing one — numbering, template, index, cross-references, CLAUDE.md and crate-selection updates. Use for every significant decision (new crate, algorithm family, data format, cross-cutting rule) and whenever an accepted ADR's decision must change.
---

# Write or supersede an ADR

## 1. Decide whether an ADR is needed
Needed for: a new dependency; a new algorithm family or data structure that other code
builds on; a file/save/network format; a cross-cutting rule (determinism, threading,
error handling); any change to the *decision* of an accepted ADR. Not needed for: balance
numbers (GAME_DESIGN.md + data RON), refactors that keep decisions, bug fixes.

## 2. Number and file
```bash
ls docs/adr | grep -E '^[0-9]{4}-' | sort | tail -1     # highest number
```
New file: `docs/adr/NNNN-kebab-case-title.md` (next number, 4 digits), copied from
`docs/adr/0000-template.md`.

## 3. Fill the template
- **Status** `Accepted` (or `Proposed` if it needs the user's decision — then ask), **Date**
  (today), **Milestone**.
- **Context**: forces and constraints, quoting the brief/requirements where relevant;
  what goes wrong if decided badly.
- **Options considered**: at least two real options with concrete pros/cons (performance,
  complexity, determinism, platforms: Windows, Linux, macOS Apple Silicon).
- **Decision**: precise enough to implement and review against (names of types/modules,
  numbers, rules).
- **Consequences**: positive, negative, follow-ups, revisit triggers.
- If it supersedes: add `- **Supersedes:** ADR-XXXX` under Status and state what changes
  and what stays.

## 4. Superseding an old ADR
- In the old ADR change **only** the status line to
  `- **Status:** Superseded by ADR-NNNN` (decision text stays as history).
- Grep for references to the old ADR and repoint them where the new one now applies:
  `grep -rn "ADR-XXXX" CLAUDE.md docs/ .claude/`

## 5. Update cross-references
- `docs/adr/README.md`: add the row (and mark the superseded one), update the topic
  mapping paragraph if it covers a required topic.
- New dependency → add a row to the crate table in the current crate-selection ADR (or
  supersede it if the policy changes) in the same commit.
- Rule-level decisions → update the "Hard rules" in `CLAUDE.md`.
- Affected docs (ARCHITECTURE, DATA_MODEL, ALGORITHMS, PERFORMANCE, TESTING, RISKS,
  PROGRESS) → update the text so no doc contradicts the ADR.

## 6. Commit
`adr: NNNN <title>` (plus the session's attribution trailer), unless it is part of a
milestone commit.
