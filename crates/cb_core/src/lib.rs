//! Foundation types shared by every City Builder crate.
//!
//! Everything here is engine-independent and deterministic across Windows, Linux and macOS
//! (ADR-0043): stable IDs ([`ids`]), seed derivation and stable hashing ([`hash`]),
//! counter-based and stream RNGs ([`rng`]), a portable math facade over `libm` ([`math`]),
//! iteration-free lookup maps ([`collections`]), the loose spatial hash ([`spatial`]), unit and
//! coordinate conventions ([`units`]), the tick-based time model ([`time`], ADR-0042) and
//! integer money ([`money`]).

pub mod collections;
pub mod error;
pub mod hash;
pub mod ids;
pub mod math;
pub mod money;
pub mod rng;
pub mod spatial;
pub mod time;
pub mod units;

#[cfg(any(test, feature = "testing"))]
pub mod test_util;

pub use error::CoreError;
pub use money::Money;
