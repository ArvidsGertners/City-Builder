//! Save core (ARCHITECTURE §11, ADR-0044): object snapshots shared with undo/redo (ADR-0055),
//! the file format and the migration framework.

pub mod format;
pub mod migrate;
pub mod snapshot;

pub use format::{Header, SaveError, load, read_header, save};
