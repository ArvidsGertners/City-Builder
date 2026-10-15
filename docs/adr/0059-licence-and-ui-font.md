# ADR-0059: Licence (proprietary) and UI font (Inter, SIL OFL)

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1
- **Supersedes:** ADR-0051 only in its font/glyph clauses ("shortcut glyphs are procedural SDF
  icons … No font asset"); everything else in ADR-0051 stays.

## Context

The user decided (2026-10-15): the project is **proprietary, all rights reserved**; one
open-licence font (SIL OFL, e.g. Inter or Noto Sans) with full German support may be added.
egui's default fonts lack ⌥ ⌃ ⌫ and only carry ⌘/⇧ in fallback fonts (verified in the review);
Ubuntu-Light is thin for a game UI.

## Options considered

1. **Inter 4.001 (static Regular + SemiBold TTF)** — designed for UI text, excellent at small
   sizes, complete Latin incl. ÄÖÜäöüßẞ „“ € and (verified with fontTools) arrows and
   ⌘ ⌥ ⇧ ⌃ ⌫ ⏎ ⎋ ⇥; ≈ 410 KB per weight.
2. **Noto Sans** — broad coverage, but modifier symbols live in separate Noto Symbols fonts
   (more files).
3. **Variable Inter** — one file, but egui (ab_glyph) renders only the default instance.

## Decision

- **Licence:** `LICENSE` at the repo root: "Copyright (c) 2026 ArvidsGertners. All rights
  reserved." Every crate sets `license-file = "../../LICENSE"` (via workspace) and
  `publish = false`. Third-party notices live in `THIRD_PARTY_NOTICES.md` (font licences now;
  a generated list of crate licences is added to release artifacts by M16).
- **Font:** Inter 4.001 Regular and SemiBold (static TTF) in `crates/cb_game/assets/fonts/`
  with `OFL.txt`, embedded via `include_bytes!` and installed as egui's proportional family
  (Regular body, SemiBold headings); egui's default fonts remain as fallback; Hack stays for
  monospace debug views. This is the only asset file in the project.
- **Shortcut glyphs** (⌘ ⌥ ⇧ ⌃ ⌫) are rendered as text with Inter. A test
  (`font_covers_required_glyphs`) asserts coverage of German characters, €, arrows and the
  modifier glyphs; if a future font change loses a glyph, the SDF-icon fallback from ADR-0051
  is used for that glyph.

## Consequences

- Sharper, more game-appropriate UI text; correct macOS shortcut labels; ≈ 0.8 MB binary size.
- The OFL requires shipping the licence text with the font (done via `OFL.txt` and notices).
