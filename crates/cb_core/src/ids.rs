//! Typed stable object IDs (ADR-0025).
//!
//! IDs are `NonZeroU32` newtypes allocated monotonically and **never reused**, so undo can
//! restore an object with its original identity and `Option<Id>` stays 4 bytes. They are used
//! in commands, saves, seeds and cross-references; `bevy::Entity` is only used within a frame.

use crate::CoreError;
use core::marker::PhantomData;
use core::num::NonZeroU32;
use serde::{Deserialize, Serialize};

/// Common behaviour of every stable ID type.
pub trait StableId: Copy + Eq + Ord + core::hash::Hash + core::fmt::Debug {
    /// Human-readable name of the ID kind (for errors and logs).
    const KIND: &'static str;
    /// Builds an ID from its raw value; `None` for zero.
    fn from_raw(raw: u32) -> Option<Self>;
    /// The raw non-zero value.
    fn raw(self) -> u32;
}

macro_rules! define_ids {
    ($($(#[$doc:meta])* $name:ident),+ $(,)?) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// Builds the ID from a raw value; errors on zero.
            pub fn try_new(raw: u32) -> Result<Self, CoreError> {
                NonZeroU32::new(raw)
                    .map(Self)
                    .ok_or(CoreError::InvalidId { kind: stringify!($name), raw })
            }
            /// The raw non-zero value.
            #[must_use]
            pub fn get(self) -> u32 {
                self.0.get()
            }
        }

        impl StableId for $name {
            const KIND: &'static str = stringify!($name);
            fn from_raw(raw: u32) -> Option<Self> {
                NonZeroU32::new(raw).map(Self)
            }
            fn raw(self) -> u32 {
                self.0.get()
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }
    )+};
}

define_ids!(
    /// A road graph node.
    NodeId,
    /// A road segment (one cubic Bézier between two nodes).
    SegmentId,
    /// A player-drawn plot.
    PlotId,
    /// A lot produced by plot subdivision.
    LotId,
    /// Any building (zoned, service, utility, transit, special).
    BuildingId,
    /// A transit stop.
    StopId,
    /// A transit line.
    LineId,
    /// A household.
    HouseholdId,
    /// A citizen.
    CitizenId,
    /// An outside road connection at the map edge.
    OutsideConnectionId,
    /// A service incident (fire, crime, medical, garbage overflow).
    IncidentId,
    /// A terrain deformation stamp.
    StampId,
    /// A trip (commute, shopping, delivery, service run).
    TripId,
    /// A player-placed decoration.
    PropId,
);

/// Monotonic allocator for one ID kind. Persisted in saves; never hands out an ID twice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdAllocator<K> {
    next: u32,
    #[serde(skip)]
    _kind: PhantomData<K>,
}

impl<K: StableId> Default for IdAllocator<K> {
    fn default() -> Self {
        Self {
            next: 1,
            _kind: PhantomData,
        }
    }
}

impl<K: StableId> IdAllocator<K> {
    /// A fresh allocator whose first ID is 1.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates the next ID.
    pub fn allocate(&mut self) -> Result<K, CoreError> {
        // `u32::MAX` is never handed out so `next` can always be incremented.
        if self.next == u32::MAX {
            return Err(CoreError::IdSpaceExhausted { kind: K::KIND });
        }
        let id = K::from_raw(self.next).ok_or(CoreError::IdSpaceExhausted { kind: K::KIND })?;
        self.next += 1;
        Ok(id)
    }

    /// The raw value the next allocation will return.
    #[must_use]
    pub fn peek_next(&self) -> u32 {
        self.next
    }

    /// Makes sure future allocations never collide with `id` (used when restoring objects).
    pub fn reserve(&mut self, id: K) {
        self.next = self.next.max(id.raw().saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_reject_zero_and_roundtrip() {
        assert!(NodeId::try_new(0).is_err());
        let id = NodeId::try_new(7).unwrap();
        assert_eq!(id.get(), 7);
        assert_eq!(size_of::<Option<NodeId>>(), 4);
    }

    #[test]
    fn allocator_is_monotonic_and_never_reuses() {
        let mut alloc = IdAllocator::<LotId>::new();
        let a = alloc.allocate().unwrap();
        let b = alloc.allocate().unwrap();
        assert!(b > a);
        alloc.reserve(LotId::try_new(100).unwrap());
        assert_eq!(alloc.allocate().unwrap().get(), 101);
        // Reserving an older ID never moves the counter backwards.
        alloc.reserve(a);
        assert_eq!(alloc.peek_next(), 102);
    }

    #[test]
    fn allocator_reports_exhaustion() {
        let mut alloc = IdAllocator::<TripId> {
            next: u32::MAX,
            _kind: PhantomData,
        };
        assert!(alloc.allocate().is_err());
    }
}
