//! Player-placed decorations (DATA_MODEL §3, GAME_DESIGN §3): trees, benches, planters,
//! fountains and flower beds placed freely with the decoration tool (`PlaceProp`). Each adds a
//! land value bonus around it that falls off linearly with distance.

use bevy::ecs::component::Component;
use bevy::ecs::message::Message;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::hash::{Domain, derive_seed};
use cb_core::ids::PropId;
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::commands::ValidationError;
use crate::ids::Ids;

pub use cb_procgen::decor::DecorKind;

/// Price and effect of a decoration kind.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecorSpec {
    /// Price (cents).
    pub cost_cents: i64,
    /// Land value bonus at the prop.
    pub lv_bonus: f32,
    /// Reach of the bonus (m).
    pub radius_m: f32,
    /// Footprint radius for overlap checks (m).
    pub footprint_r_m: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
struct PropsFile {
    tree: DecorSpec,
    tree_cluster: DecorSpec,
    bench: DecorSpec,
    planter: DecorSpec,
    fountain: DecorSpec,
    flower_bed: DecorSpec,
}

const PROPS_RON: &str = include_str!("../data/props.ron");

/// The spec of a decoration kind (embedded data; the file is checked by a test).
#[must_use]
pub fn spec(kind: DecorKind) -> DecorSpec {
    let fallback = DecorSpec {
        cost_cents: 0,
        lv_bonus: 0.0,
        radius_m: 1.0,
        footprint_r_m: 1.0,
    };
    let Ok(f) = ron::from_str::<PropsFile>(PROPS_RON) else {
        return fallback;
    };
    match kind {
        DecorKind::Tree => f.tree,
        DecorKind::TreeCluster => f.tree_cluster,
        DecorKind::Bench => f.bench,
        DecorKind::Planter => f.planter,
        DecorKind::Fountain => f.fountain,
        DecorKind::FlowerBed => f.flower_bed,
    }
}

/// A placed decoration.
#[derive(Component, Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PlacedProp {
    pub(crate) id: PropId,
    pub(crate) kind: DecorKind,
    pub(crate) pos: Vec3,
    pub(crate) yaw: f32,
    pub(crate) seed: u64,
}

impl PlacedProp {
    /// Stable id.
    #[must_use]
    pub fn id(&self) -> PropId {
        self.id
    }
    /// Kind.
    #[must_use]
    pub fn kind(&self) -> DecorKind {
        self.kind
    }
    /// Position on the ground.
    #[must_use]
    pub fn pos(&self) -> Vec3 {
        self.pos
    }
    /// Rotation about Y (radians).
    #[must_use]
    pub fn yaw(&self) -> f32 {
        self.yaw
    }
    /// Variation seed.
    #[must_use]
    pub fn seed(&self) -> u64 {
        self.seed
    }
}

/// Decoration changes for view sync.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropChanged {
    /// Placed (or restored).
    Placed(PropId),
    /// Removed.
    Removed(PropId),
}

/// Land value bonus of the placed decorations at `p` (sum of linear falloffs).
#[must_use]
pub fn land_value_bonus_at(world: &World, p: Vec2) -> f32 {
    let Some(ids) = world.get_resource::<Ids>() else {
        return 0.0;
    };
    ids.props
        .iter()
        .filter_map(|(_, e)| world.get::<PlacedProp>(e))
        .map(|pr| {
            let s = spec(pr.kind);
            let d = Vec2::new(pr.pos.x, pr.pos.z).distance(p);
            s.lv_bonus * (1.0 - d / s.radius_m).max(0.0)
        })
        .sum()
}

/// Validates a decoration at `pos`: on dry land, off roads and buildings.
///
/// # Errors
/// `prop-invalid-blocked`, `prop-invalid-water` or `money-insufficient`.
pub fn preview_place_prop(
    world: &World,
    kind: DecorKind,
    pos: Vec2,
) -> Result<Money, ValidationError> {
    let s = spec(kind);
    let ids = world
        .get_resource::<Ids>()
        .ok_or(ValidationError::new("internal-no-catalog"))?;
    let r = s.footprint_r_m;
    let blocked = |poly: &[Vec2]| {
        cb_geometry::polygon::contains(poly, pos)
            || cb_geometry::polygon::boundary_distance(poly, pos) < r
    };
    for (_, e) in ids.segments.iter() {
        if let Some(seg) = world.get::<crate::roads::RoadSegment>(e)
            && blocked(&seg.geom().corridor)
        {
            return Err(ValidationError::at("prop-invalid-blocked", pos));
        }
    }
    for (_, e) in ids.buildings.iter() {
        if let Some(b) = world.get::<crate::buildings::Building>(e)
            && b.outlines().iter().any(|o| blocked(o))
        {
            return Err(ValidationError::at("prop-invalid-blocked", pos));
        }
    }
    if let Some(t) = world.get_resource::<crate::terrain::TerrainRes>()
        && t.terrain().water_depth_at(pos) > 0.05
    {
        return Err(ValidationError::at("prop-invalid-water", pos));
    }
    let cost = Money(s.cost_cents);
    if !crate::economy::can_afford(world, cost) {
        return Err(ValidationError::new("money-insufficient"));
    }
    Ok(cost)
}

/// `PlaceProp`: validate, charge, place.
pub(crate) fn place_prop(
    world: &mut World,
    kind: DecorKind,
    pos: Vec2,
    yaw: f32,
) -> Result<Money, ValidationError> {
    let cost = preview_place_prop(world, kind, pos)?;
    crate::economy::charge(world, cost)?;
    let id = world
        .resource_mut::<Ids>()
        .props
        .allocate()
        .map_err(|_| ValidationError::new("internal-id-space"))?;
    let g = crate::terrain::ground(world);
    let y = cb_network::validation::Ground::height(&g, pos);
    let world_seed = world
        .get_resource::<crate::config::WorldSeed>()
        .map_or(0, |s| s.0);
    let prop = PlacedProp {
        id,
        kind,
        pos: Vec3::new(pos.x, y, pos.y),
        yaw,
        seed: derive_seed(world_seed, Domain::Prop, u64::from(id.get()), 0),
    };
    restore_put_prop(world, prop);
    Ok(cost)
}

/// `RemoveProp`.
pub(crate) fn remove_prop(world: &mut World, id: PropId) -> Result<Money, ValidationError> {
    if world
        .get_resource::<Ids>()
        .and_then(|i| i.props.get(id))
        .is_none()
    {
        return Err(ValidationError::new("prop-invalid-missing"));
    }
    restore_remove_prop(world, id);
    Ok(Money::ZERO)
}

/// Undo/redo and placement: puts a prop with its id.
pub(crate) fn restore_put_prop(world: &mut World, p: PlacedProp) {
    let id = p.id;
    let existing = world.get_resource::<Ids>().and_then(|i| i.props.get(id));
    match existing {
        Some(e) => {
            if let Some(mut x) = world.get_mut::<PlacedProp>(e) {
                *x = p;
            }
        }
        None => {
            let e = world.spawn(p).id();
            world.resource_mut::<Ids>().props.insert(id, e);
        }
    }
    world.write_message(PropChanged::Placed(id));
}

/// Undo/redo: removes a prop.
pub(crate) fn restore_remove_prop(world: &mut World, id: PropId) {
    if let Some(e) = world.resource_mut::<Ids>().props.remove(id) {
        world.despawn(e);
        world.write_message(PropChanged::Removed(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn props_data_parses_and_matches_game_design() {
        assert!(ron::from_str::<PropsFile>(PROPS_RON).is_ok());
        assert_eq!(spec(DecorKind::Fountain).cost_cents, 200_000);
        assert!((spec(DecorKind::Fountain).lv_bonus - 0.05).abs() < 1e-6);
        assert!((spec(DecorKind::Tree).lv_bonus - 0.01).abs() < 1e-6);
        for k in DecorKind::ALL {
            let s = spec(k);
            assert!((30.0..=60.0).contains(&s.radius_m), "{k:?}");
        }
    }
}
