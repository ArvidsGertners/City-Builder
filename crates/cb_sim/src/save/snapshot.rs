//! Object snapshots (ADR-0055): the player-built objects (road nodes and segments, plots, lots,
//! buildings) by stable id, before/after deltas between two snapshots, and restoring a delta
//! with the original ids. Derived state (junction geometry, terrain stamps, lane graph) is
//! rebuilt from the restored objects.

use std::collections::BTreeMap;

use bevy::ecs::world::World;
use cb_core::ids::{BuildingId, LotId, NodeId, PlotId, SegmentId, StableId};

use crate::buildings::Building;
use crate::ids::{Ids, Registry};
use crate::roads::{RoadNode, RoadSegment};
use crate::zoning::{Lot, Plot};

/// All player-built objects by stable id.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectSet {
    /// Road nodes.
    pub nodes: BTreeMap<NodeId, RoadNode>,
    /// Road segments.
    pub segments: BTreeMap<SegmentId, RoadSegment>,
    /// Plots.
    pub plots: BTreeMap<PlotId, Plot>,
    /// Lots.
    pub lots: BTreeMap<LotId, Lot>,
    /// Buildings.
    pub buildings: BTreeMap<BuildingId, Building>,
}

/// Target state per id: `Some(value)` puts the object, `None` removes it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Delta {
    /// Road nodes.
    pub nodes: BTreeMap<NodeId, Option<RoadNode>>,
    /// Road segments.
    pub segments: BTreeMap<SegmentId, Option<RoadSegment>>,
    /// Plots.
    pub plots: BTreeMap<PlotId, Option<Plot>>,
    /// Lots.
    pub lots: BTreeMap<LotId, Option<Lot>>,
    /// Buildings.
    pub buildings: BTreeMap<BuildingId, Option<Building>>,
}

impl Delta {
    /// Nothing to change.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.segments.is_empty()
            && self.plots.is_empty()
            && self.lots.is_empty()
            && self.buildings.is_empty()
    }
}

fn collect<K, C>(world: &World, reg: &Registry<K>) -> BTreeMap<K, C>
where
    K: StableId + Ord + Copy,
    C: bevy::ecs::component::Component + Clone,
{
    reg.iter()
        .filter_map(|(id, e)| world.get::<C>(e).map(|c| (id, c.clone())))
        .collect()
}

/// Snapshot of every player-built object.
#[must_use]
pub fn capture(world: &World) -> ObjectSet {
    let Some(ids) = world.get_resource::<Ids>() else {
        return ObjectSet::default();
    };
    ObjectSet {
        nodes: collect(world, &ids.nodes),
        segments: collect(world, &ids.segments),
        plots: collect(world, &ids.plots),
        lots: collect(world, &ids.lots),
        buildings: collect(world, &ids.buildings),
    }
}

fn diff_map<K: Ord + Copy, V: Clone + PartialEq>(
    a: &BTreeMap<K, V>,
    b: &BTreeMap<K, V>,
    to_a: &mut BTreeMap<K, Option<V>>,
    to_b: &mut BTreeMap<K, Option<V>>,
) {
    for (k, va) in a {
        match b.get(k) {
            Some(vb) if vb == va => {}
            vb => {
                to_a.insert(*k, Some(va.clone()));
                to_b.insert(*k, vb.cloned());
            }
        }
    }
    for (k, vb) in b {
        if !a.contains_key(k) {
            to_a.insert(*k, None);
            to_b.insert(*k, Some(vb.clone()));
        }
    }
}

/// Deltas between two snapshots: `(back to a, forward to b)`.
#[must_use]
pub fn diff(a: &ObjectSet, b: &ObjectSet) -> (Delta, Delta) {
    let (mut to_a, mut to_b) = (Delta::default(), Delta::default());
    diff_map(&a.nodes, &b.nodes, &mut to_a.nodes, &mut to_b.nodes);
    diff_map(
        &a.segments,
        &b.segments,
        &mut to_a.segments,
        &mut to_b.segments,
    );
    diff_map(&a.plots, &b.plots, &mut to_a.plots, &mut to_b.plots);
    diff_map(&a.lots, &b.lots, &mut to_a.lots, &mut to_b.lots);
    diff_map(
        &a.buildings,
        &b.buildings,
        &mut to_a.buildings,
        &mut to_b.buildings,
    );
    (to_a, to_b)
}

/// Applies a delta: removals (buildings → lots → plots → segments → nodes), then puts (nodes →
/// segments → plots → lots → buildings) with original ids, then rebuilds the touched junctions;
/// lots without their plot and buildings without their lot (sim-grown since) are removed.
pub fn apply(world: &mut World, d: &Delta) {
    for (id, v) in &d.buildings {
        if v.is_none() {
            crate::buildings::remove_building(world, *id);
        }
    }
    for (id, v) in &d.lots {
        if v.is_none() {
            crate::zoning::restore_remove_lot(world, *id);
        }
    }
    for (id, v) in &d.plots {
        if v.is_none() {
            crate::zoning::restore_remove_plot(world, *id);
        }
    }
    let mut touched: Vec<NodeId> = Vec::new();
    for (id, v) in &d.segments {
        if v.is_none() {
            if let Some(s) = crate::roads::segment(world, *id) {
                touched.extend([s.start(), s.end()]);
            }
            crate::roads::restore_remove_segment(world, *id);
        }
    }
    for (id, v) in &d.nodes {
        if v.is_none() {
            crate::roads::restore_remove_node(world, *id);
        }
    }
    for (id, v) in &d.nodes {
        if let Some(n) = v {
            crate::roads::restore_put_node(world, n.clone());
            touched.push(*id);
        }
    }
    for s in d.segments.values().flatten() {
        touched.extend([s.start(), s.end()]);
        crate::roads::restore_put_segment(world, s.clone());
    }
    for p in d.plots.values().flatten() {
        crate::zoning::restore_put_plot(world, p.clone());
    }
    for l in d.lots.values().flatten() {
        crate::zoning::restore_put_lot(world, l.clone());
    }
    for b in d.buildings.values().flatten() {
        crate::buildings::restore_put_building(world, b.clone());
    }
    let alive = |w: &World, n: &NodeId| {
        w.get_resource::<Ids>()
            .is_some_and(|i| i.nodes.get(*n).is_some())
    };
    touched.retain(|n| alive(world, n));
    crate::roads::restore_refresh(world, &touched);
    remove_orphans(world);
}

/// Lots whose plot is gone and buildings whose lot is gone.
fn remove_orphans(world: &mut World) {
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let lots: Vec<LotId> = ids
        .lots
        .iter()
        .filter(|(_, e)| {
            world
                .get::<Lot>(*e)
                .is_some_and(|l| ids.plots.get(l.plot()).is_none())
        })
        .map(|(id, _)| id)
        .collect();
    for l in lots {
        crate::zoning::restore_remove_lot(world, l);
    }
    let Some(ids) = world.get_resource::<Ids>() else {
        return;
    };
    let buildings: Vec<BuildingId> = ids
        .buildings
        .iter()
        .filter(|(_, e)| {
            world
                .get::<Building>(*e)
                .is_some_and(|b| ids.lots.get(b.lot()).is_none())
        })
        .map(|(id, _)| id)
        .collect();
    for b in buildings {
        crate::buildings::remove_building(world, b);
    }
}
