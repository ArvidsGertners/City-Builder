//! Error type for `cb_core`.

use thiserror::Error;

/// Errors produced by foundation utilities.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// An ID allocator ran out of identifiers (more than `u32::MAX - 1` allocations).
    #[error("id space exhausted for {kind}")]
    IdSpaceExhausted {
        /// Name of the ID kind.
        kind: &'static str,
    },
    /// A raw value of zero (or otherwise invalid) was used to construct an ID.
    #[error("invalid raw id {raw} for {kind}")]
    InvalidId {
        /// Name of the ID kind.
        kind: &'static str,
        /// The rejected raw value.
        raw: u32,
    },
    /// Money arithmetic overflowed.
    #[error("money overflow")]
    MoneyOverflow,
}
