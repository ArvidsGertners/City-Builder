//! The terrain resource (DATA_MODEL §2): a `cb_terrain::Terrain` with deformation stamps owned by
//! roads and buildings. Every stamp change emits [`TerrainChanged`] with the dirty sample rect
//! so the game re-meshes only the affected chunks.

use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_network::validation::Ground;
use cb_terrain::{SampleRect, Stamp, Terrain, WorldGenParams};
use std::sync::Arc;

use glam::Vec2;

/// How the sim gets its terrain.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum TerrainSetup {
    /// No terrain resource: flat ground at 0 m over a 4 096 m map (fast headless tests).
    #[default]
    None,
    /// A flat, dry world of `samples²` at `spacing_m` and height `h`.
    Flat {
        /// Samples per side.
        samples: usize,
        /// Spacing (m).
        spacing_m: f32,
        /// Height (m).
        h: f32,
    },
    /// Procedural world generation from the world seed.
    Generate,
    /// A flat world with a straight valley along the z axis (bridge tests and scenarios).
    Valley {
        /// Samples per side.
        samples: usize,
        /// Spacing (m).
        spacing_m: f32,
        /// Plain height (m).
        h: f32,
        /// Valley depth (m).
        depth_m: f32,
        /// Valley width (m).
        width_m: f32,
        /// River depth above the valley floor (m; `None`: dry).
        water_m: Option<f32>,
    },
}

/// The terrain. Held behind an `Arc` so presentation can mesh chunks off the main thread from
/// a snapshot; edits copy on write (`Arc::make_mut`) only while such a snapshot is alive.
#[derive(Resource)]
pub struct TerrainRes {
    pub(crate) t: Arc<Terrain>,
    /// End points of the outside road connections inside the map (trade origin/destination).
    pub(crate) outside: Vec<Vec2>,
    /// River centrelines, source to mouth (water pollution advection).
    pub(crate) rivers: Vec<Vec<Vec2>>,
}

impl TerrainRes {
    /// Wraps a generated world, keeping its outside connection ends.
    #[must_use]
    pub fn from_world(w: cb_terrain::World) -> Self {
        let outside = w
            .outside
            .iter()
            .map(cb_terrain::OutsideConnection::end)
            .collect();
        let rivers = w.rivers.iter().map(|r| r.points.clone()).collect();
        Self {
            t: Arc::new(Terrain::from_world(w)),
            outside,
            rivers,
        }
    }

    /// End points of the outside road connections.
    #[must_use]
    pub fn outside_ends(&self) -> &[Vec2] {
        &self.outside
    }

    /// River centrelines, source to mouth.
    #[must_use]
    pub fn rivers(&self) -> &[Vec<Vec2>] {
        &self.rivers
    }

    /// The terrain (read-only; edits go through commands).
    #[must_use]
    pub fn terrain(&self) -> &Terrain {
        &self.t
    }

    /// A shared read-only snapshot of the current terrain (for background meshing).
    #[must_use]
    pub fn snapshot(&self) -> Arc<Terrain> {
        Arc::clone(&self.t)
    }
}

/// A region of the terrain changed.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainChanged {
    /// Dirty samples (inclusive).
    pub rect: SampleRect,
}

/// Builds the terrain for a setup (`None` for [`TerrainSetup::None`]).
///
/// # Errors
/// World generation errors.
pub fn build(
    setup: &TerrainSetup,
    seed: u64,
) -> Result<Option<TerrainRes>, cb_terrain::TerrainError> {
    Ok(match setup {
        TerrainSetup::None => None,
        TerrainSetup::Flat {
            samples,
            spacing_m,
            h,
        } => {
            let w = cb_terrain::World::flat(
                &WorldGenParams {
                    samples: *samples,
                    spacing_m: *spacing_m,
                },
                *h,
            );
            Some(TerrainRes::from_world(w))
        }
        TerrainSetup::Valley {
            samples,
            spacing_m,
            h,
            depth_m,
            width_m,
            water_m,
        } => {
            let w = cb_terrain::World::valley(
                &WorldGenParams {
                    samples: *samples,
                    spacing_m: *spacing_m,
                },
                *h,
                *depth_m,
                *width_m,
                *water_m,
            );
            Some(TerrainRes::from_world(w))
        }
        TerrainSetup::Generate => {
            let w = cb_terrain::generate(&WorldGenParams::default(), seed)?;
            Some(TerrainRes::from_world(w))
        }
    })
}

/// Ground queries over the optional terrain (flat 0 m, 4 096 m map without one).
pub struct GroundView<'a>(pub Option<&'a Terrain>);

impl Ground for GroundView<'_> {
    fn height(&self, p: Vec2) -> f32 {
        self.0.map_or(0.0, |t| t.height_at(p))
    }
    fn water_depth(&self, p: Vec2) -> f32 {
        self.0.map_or(0.0, |t| t.water_depth_at(p))
    }
    fn half_extent(&self) -> f32 {
        self.0.map_or(2048.0, |t| {
            0.5 * (t.height().n() - 1) as f32 * t.height().spacing_m()
        })
    }
}

/// Ground view of a world.
#[must_use]
pub fn ground(world: &World) -> GroundView<'_> {
    GroundView(world.get_resource::<TerrainRes>().map(|r| &*r.t))
}

/// Adds a stamp and reports the dirty rect.
pub(crate) fn add_stamp(world: &mut World, stamp: Stamp) {
    let rect = world
        .get_resource_mut::<TerrainRes>()
        .map(|mut r| Arc::make_mut(&mut r.t).add_stamp(stamp));
    if let Some(rect) = rect {
        world.write_message(TerrainChanged { rect });
    }
}

/// Removes a stamp and reports the dirty rect.
pub(crate) fn remove_stamp(world: &mut World, id: u32) {
    let rect = world
        .get_resource_mut::<TerrainRes>()
        .and_then(|mut r| Arc::make_mut(&mut r.t).remove_stamp(id));
    if let Some(rect) = rect {
        world.write_message(TerrainChanged { rect });
    }
}
