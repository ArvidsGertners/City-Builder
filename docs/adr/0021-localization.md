# ADR-0021: Localization approach

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1 (pipeline), M15 (complete German)

## Context

English and German for v1; all user-facing strings in localization files; per-language
number, currency, date and time formatting; plurals; German strings ~30 % longer.

## Options considered

1. **Hand-rolled key → string tables (RON)** — simple, but no plural/select rules.
2. **gettext (.po)** — mature tooling, C-ish ecosystem, weaker parameter formatting.
3. **Project Fluent (`fluent-bundle`) (chosen)** — Rust-native, plural/select, named
   arguments, designed for game/UI strings.
4. Full ICU4X for formatting — heavy dependency for two locales.

## Decision

- `cb_l10n` crate with `fluent-bundle`, `unic-langid`; `.ftl` files per locale and domain
  (`ui.ftl`, `tools.ftl`, `notifications.ftl`, `tutorial.ftl`, `buildings.ftl`, `tiers.ftl`)
  embedded with `include_str!` (no runtime asset loading); English is the fallback.
- Keys are dotted (`tool.road.straight`); code references keys through a generated/checked
  constant module (`keys.rs`) so typos fail to compile; a test verifies every key exists in
  every locale and placeholders match.
- Formatting implemented in `cb_l10n::format` (numbers with grouping/decimal separators,
  money with locale pattern — en `$1,234`, de `1.234 €` —, percentages, perceived durations,
  calendar date "Year 3, Spring, Day 2", 24 h time for both locales) and passed to Fluent as
  pre-formatted string args (Fluent's own NUMBER is not locale-complete).
- Language switch at runtime re-renders UI immediately (egui is immediate mode).
- Sim messages carry keys + typed args (`L10nArg`), never strings.

## Consequences

- Translators/players can edit `.ftl` easily; compile-time key safety.
- Adding a locale = new folder + formatting rules entry.

## Amendments

- 2026-10-15 (M1): Fluent message IDs cannot contain dots, so keys use dashes
  (`tool-road-straight`; groups by prefix). The generated constants are `keys::TOOL_ROAD_STRAIGHT`.
  The OS locale is read from `LC_ALL`/`LC_MESSAGES`/`LANG`; Windows and Finder-launched macOS
  apps fall back to English unless the settings choose a language (OS API lookup: M15).
