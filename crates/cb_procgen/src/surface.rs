//! The packed `u32` vertex attribute (ADR-0049): bits 0–15 slot (per-object parameter buffer
//! index: tint, condition, selection), bits 16–19 surface kind (shader behaviour: night windows,
//! snow on roofs, season tint on vegetation, wetness on roads), bits 20–31 a per-kind id
//! (window id for the night lighting pattern).

/// Surface kinds understood by the world shader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Surface {
    /// Walls, plinths, generic solids.
    Solid = 0,
    /// Roofs (snow cover, wetness).
    Roof = 1,
    /// Window glass (emissive at night by window id).
    Window = 2,
    /// Foliage and grass (season tint).
    Foliage = 3,
    /// Trunks and branches (bare in winter).
    Trunk = 4,
    /// Paved ground: roads, driveways, sidewalks (wetness).
    Paved = 5,
    /// Painted markings (wetness, wear).
    Marking = 6,
    /// Bare ground / garden beds.
    Ground = 7,
}

/// Maximum id stored in bits 20–31.
pub const MAX_ID: u32 = (1 << 12) - 1;

/// Packs slot, kind and id.
#[must_use]
pub const fn pack(slot: u16, kind: Surface, id: u32) -> u32 {
    slot as u32 | ((kind as u32) << 16) | ((id & MAX_ID) << 20)
}

/// Surface kind of a packed value (`None` for unknown kinds).
#[must_use]
pub fn kind_of(packed: u32) -> Option<Surface> {
    use Surface::{Foliage, Ground, Marking, Paved, Roof, Solid, Trunk, Window};
    [Solid, Roof, Window, Foliage, Trunk, Paved, Marking, Ground]
        .get(((packed >> 16) & 0xF) as usize)
        .copied()
}

/// Slot of a packed value.
#[must_use]
pub const fn slot_of(packed: u32) -> u16 {
    (packed & 0xFFFF) as u16
}

/// Id of a packed value.
#[must_use]
pub const fn id_of(packed: u32) -> u32 {
    packed >> 20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_round_trip() {
        let p = pack(513, Surface::Window, 77);
        assert_eq!(slot_of(p), 513);
        assert_eq!(kind_of(p), Some(Surface::Window));
        assert_eq!(id_of(p), 77);
        assert_eq!(id_of(pack(0, Surface::Solid, 5000)), 5000 & MAX_ID);
    }
}
