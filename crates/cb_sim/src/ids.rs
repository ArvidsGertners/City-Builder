//! Stable ID allocation and the ID → entity index (DATA_MODEL §1). Allocators never reuse an
//! ID; the index is ordered (`BTreeMap`) so systems can iterate objects by stable ID, which is
//! the only iteration order allowed where results depend on order (ADR-0043).

use std::collections::BTreeMap;

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use cb_core::ids::{BuildingId, IdAllocator, LotId, NodeId, PlotId, SegmentId, StableId};

/// Allocators and the ordered index for one ID kind.
#[derive(Clone, Debug)]
pub struct Registry<K: StableId + Ord> {
    alloc: IdAllocator<K>,
    map: BTreeMap<K, Entity>,
}

impl<K: StableId + Ord> Default for Registry<K> {
    fn default() -> Self {
        Self {
            alloc: IdAllocator::new(),
            map: BTreeMap::new(),
        }
    }
}

impl<K: StableId + Ord + Copy> Registry<K> {
    /// Allocates a fresh ID.
    pub(crate) fn allocate(&mut self) -> Result<K, cb_core::CoreError> {
        self.alloc.allocate()
    }

    /// Registers an entity for `id`.
    pub(crate) fn insert(&mut self, id: K, e: Entity) {
        self.map.insert(id, e);
    }

    /// Unregisters `id`.
    pub(crate) fn remove(&mut self, id: K) -> Option<Entity> {
        self.map.remove(&id)
    }

    /// Entity of `id`.
    #[must_use]
    pub fn get(&self, id: K) -> Option<Entity> {
        self.map.get(&id).copied()
    }

    /// All IDs in ascending order with their entities.
    pub fn iter(&self) -> impl Iterator<Item = (K, Entity)> + '_ {
        self.map.iter().map(|(k, e)| (*k, *e))
    }

    /// Number of live objects.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// No live objects.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// All sim object registries.
#[derive(Resource, Default, Clone, Debug)]
pub struct Ids {
    /// Road nodes.
    pub nodes: Registry<NodeId>,
    /// Road segments.
    pub segments: Registry<SegmentId>,
    /// Plots.
    pub plots: Registry<PlotId>,
    /// Lots.
    pub lots: Registry<LotId>,
    /// Buildings.
    pub buildings: Registry<BuildingId>,
    /// Next terrain stamp id (stamps are owned by roads and buildings).
    pub(crate) next_stamp: u32,
}

impl Ids {
    /// Allocates a terrain stamp id.
    pub(crate) fn stamp(&mut self) -> u32 {
        self.next_stamp = self.next_stamp.wrapping_add(1).max(1);
        self.next_stamp
    }
}
