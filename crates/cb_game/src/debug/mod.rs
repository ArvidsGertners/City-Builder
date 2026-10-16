//! Debug and verification tooling (ADR-0031, ADR-0058). Smoke test and screenshot suites are
//! always available (CI needs them in default builds); interactive tools need feature `dev`.
//! Developer-facing strings here are English and exempt from localization (CLAUDE.md).

pub mod benchmark;
pub mod screenshot;
pub mod screenshot_metrics;
pub mod smoke;

#[cfg(feature = "dev")]
pub mod dev_tools;
