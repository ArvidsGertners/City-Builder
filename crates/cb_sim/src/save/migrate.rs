//! Save migrations (ADR-0044): a body older than [`super::format::FORMAT_VERSION`] is decoded
//! into a dynamic MessagePack value and passed through one step per version before it is
//! deserialized. Format 0 is the development format, so the list is empty until v1.0 (M15);
//! a synthetic `v0 → v1` test keeps the framework exercised.

use rmpv::Value;

use super::format::SaveError;

/// One migration step: rewrites a body of version `from` into version `from + 1`.
#[derive(Clone, Copy, Debug)]
pub struct Migration {
    /// Source version.
    pub from: u32,
    /// The rewrite.
    pub step: fn(&mut Value) -> Result<(), String>,
}

/// Migrations of the shipped formats, in version order.
pub const MIGRATIONS: &[Migration] = &[];

/// Migrates `body` from version `from` to `to` with `steps`.
///
/// # Errors
/// [`SaveError::Migration`] if a step is missing or fails.
pub fn migrate(
    mut body: Value,
    from: u32,
    to: u32,
    steps: &[Migration],
) -> Result<Value, SaveError> {
    for v in from..to {
        let m = steps
            .iter()
            .find(|m| m.from == v)
            .ok_or_else(|| SaveError::Migration(v, "no migration step".into()))?;
        (m.step)(&mut body).map_err(|e| SaveError::Migration(v, e))?;
    }
    Ok(body)
}

/// The value of `key` in a MessagePack map (struct-map encoding).
pub fn field_mut<'a>(map: &'a mut Value, key: &str) -> Option<&'a mut Value> {
    let Value::Map(entries) = map else {
        return None;
    };
    entries
        .iter_mut()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

/// Renames a field of a MessagePack map; `false` if it was missing.
pub fn rename_field(map: &mut Value, from: &str, to: &str) -> bool {
    let Value::Map(entries) = map else {
        return false;
    };
    for (k, _) in entries.iter_mut() {
        if k.as_str() == Some(from) {
            *k = Value::from(to);
            return true;
        }
    }
    false
}
