# ADR-0022: Logging and crash handling

- **Status:** Accepted
- **Date:** 2026-10-15
- **Milestone:** M1

## Context

Structured logging with `tracing`, configurable levels, logs written to file; panic hook
writing a crash report (message, backtrace, version, recent log lines) to a crash folder;
errors and reports must be copy-paste friendly for handing back to the developer agent.

## Options considered

1. **Bevy `LogPlugin` defaults only** — console only, no file, no crash report.
2. **`human-panic`** — friendly message but limited content, extra dependency.
3. **tracing-subscriber layers via `LogPlugin::custom_layer` + own panic hook (chosen)**.

## Decision

- Bevy `LogPlugin` (tracing) configured with: filter from `--log`/`CB_LOG`/settings
  (default `info,wgpu=warn,naga=warn`), a **file layer** (`tracing-appender` non-blocking,
  daily rolling `logs/city-builder.log`, keep 7 files, plain text, UTC timestamps, target and
  span fields), and a **ring-buffer layer** (last 500 formatted lines in memory, shared via
  `Arc<Mutex<VecDeque<String>>>` for the crash reporter and the in-game console).
  Headless tests/bench use the same layers without Bevy's LogPlugin.
- **Panic hook** (`std::panic::set_hook`, chained to the default hook): writes
  `crashes/crash-YYYYMMDD-HHMMSS.txt` containing, in fenced sections: summary line,
  panic message + location, thread name, `Backtrace::force_capture()`, game version + git
  hash + build profile, OS/arch, GPU adapter + backend + driver (captured at startup into a
  `OnceLock`), settings summary, sim summary (seed, tick, game time, population, last 20
  commands — updated in a `OnceLock<Mutex<…>>` each frame cheaply), last 200 log lines.
  It flushes the log appender and prints the report path to stderr.
- On next start, a `crashes/.last_crash` marker makes the main menu show "The game crashed
  last time" with *Copy report* (to clipboard via egui) and *Open folder* buttons.
- Bevy system errors: the fallback error handler logs (`error!`) with system name and
  counts occurrences; in debug builds the `--strict-errors` flag turns them into panics.
- Error messages for players are localized; logs stay English.

## Consequences

- Reports contain everything needed to reproduce (seed + recent commands + versions).
- Small amount of global state (`OnceLock`s) isolated in `bootstrap/crash.rs`.
