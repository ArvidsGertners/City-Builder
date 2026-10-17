//! Save file format v0 (ADR-0044): `MAGIC | format version (u32 LE) | header length (u32 LE) |
//! header | body | checksum (u64 LE)`. Header and body are MessagePack with named fields
//! (`with_struct_map`), so additive `#[serde(default)]` fields need no migration; the checksum
//! is the stable hash of everything before it. Compression is the caller's (the game wraps the
//! bytes in LZ4).
//!
//! **Completeness**: everything that influences future results is saved — authored objects,
//! base terrain, clock, config, ID allocators and every sim resource including the in-progress
//! sliced passes (fields, commute assignment). Derived state (junction geometry, segment geometry,
//! terrain stamps and composited heights, lane and link graphs, access caches, population
//! indices) is rebuilt on load.
//!
//! Loading validates referential integrity before anything is spawned and returns typed errors;
//! malformed input never panics (`save_fuzz_no_panic`).

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hasher;

use bevy::ecs::world::World;
use cb_core::hash::StableHasher;
use cb_core::ids::{BuildingId, IdAllocator, LotId, NodeId, PlotId, PropId, SegmentId};
use cb_geometry::CubicBezier2;
use cb_network::bridge::Pier;
use cb_network::control::{ApproachControl, ControlMode};
use cb_network::style::SegmentStyle;
use cb_network::{CrossSection, Profile, RoadKind, SegmentGeom, Variant};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use super::migrate;
use super::snapshot::{self, Delta};
use crate::buildings::Building;
use crate::citizens::happiness::HappinessStats;
use crate::citizens::{Capacities, Population, PopulationStats};
use crate::demand::Demand;
use crate::economy::goods::Goods;
use crate::economy::{Finances, Ledger, Treasury};
use crate::fields::Fields;
use crate::ids::Ids;
use crate::props::PlacedProp;
use crate::roads::{RoadNode, RoadSegment};
use crate::terrain::TerrainRes;
use crate::traffic::{Traffic, TrafficSave};
use crate::utilities::Utilities;
use crate::zoning::{DirtyPlots, Lot, Plot};
use crate::{SimClock, SimConfig, WorldSeed};

/// File magic.
pub const MAGIC: [u8; 8] = *b"CBSAVE\0\0";
/// Current format version (0 = development format until v1.0, ADR-0044).
pub const FORMAT_VERSION: u32 = 0;

/// Why a save could not be written or read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SaveError {
    /// Shorter than its framing says.
    #[error("save file truncated")]
    Truncated,
    /// Not a save file.
    #[error("not a City Builder save")]
    BadMagic,
    /// Written by a newer game.
    #[error("unsupported save format version {0}")]
    UnsupportedVersion(u32),
    /// Contents do not match the checksum.
    #[error("save file corrupt (checksum mismatch)")]
    Checksum,
    /// Encoding failed.
    #[error("encoding failed: {0}")]
    Encode(String),
    /// Decoding failed.
    #[error("decoding failed: {0}")]
    Decode(String),
    /// Decoded but inconsistent (dangling references and the like).
    #[error("invalid save: {0}")]
    Invalid(String),
    /// A migration step failed.
    #[error("migration from version {0} failed: {1}")]
    Migration(u32, String),
}

/// Summary shown before loading (load dialog, crash reports).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Format version of the body.
    pub format_version: u32,
    /// Game version that wrote it.
    pub game_version: String,
    /// Hash of the catalog data (balance changes since are warned about).
    pub catalog_hash: u64,
    /// World seed.
    pub seed: u64,
    /// Completed ticks.
    pub tick: u64,
    /// State hash at save time (`save_roundtrip_hash_equal`).
    pub state_hash: u64,
    /// Citizens.
    pub population: u32,
    /// Treasury balance (cents).
    pub money_cents: i64,
}

/// A road node's authored state (junction geometry and controls are rebuilt).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeSave {
    id: NodeId,
    pos: Vec3,
    segments: SmallVec<[SegmentId; 4]>,
    control: ControlMode,
    custom: SmallVec<[(SegmentId, bool, ApproachControl); 4]>,
}

/// A road segment's authored state (geometry, cut-backs and stamps are rebuilt).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SegmentSave {
    id: SegmentId,
    start: NodeId,
    end: NodeId,
    kind: RoadKind,
    variant: Variant,
    style: SegmentStyle,
    curve: CubicBezier2,
    profile: Profile,
    built_tick: u64,
    spans: Vec<(f32, f32)>,
    piers: Vec<Pier>,
}

/// Base terrain: heights (1/256 m steps, u16 when lossless), water surfaces, forest mask, and
/// the outside connections and rivers the sim keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerrainSave {
    n: u32,
    spacing_m: f32,
    heights: Heights,
    /// `(sample index, level, body id)` of wet samples.
    water: Vec<(u32, f32, u8)>,
    /// Body kinds by id − 1 (0 river, 1 lake).
    bodies: Vec<u8>,
    forest_n: u32,
    forest_spacing_m: f32,
    forest: Vec<u8>,
    material_seed: u64,
    outside: Vec<Vec2>,
    rivers: Vec<Vec<Vec2>>,
}

/// Height samples.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Heights {
    /// `min + q / 256` for every sample.
    Quantized {
        /// Lowest height (m).
        min: f32,
        /// Steps above `min`.
        q: Vec<u16>,
    },
    /// Raw heights (used when quantizing would lose precision).
    Raw(Vec<f32>),
}

/// ID allocators.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IdsSave {
    nodes: IdAllocator<NodeId>,
    segments: IdAllocator<SegmentId>,
    plots: IdAllocator<PlotId>,
    lots: IdAllocator<LotId>,
    buildings: IdAllocator<BuildingId>,
    props: IdAllocator<PropId>,
    next_stamp: u32,
}

/// Everything saved.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Body {
    seed: u64,
    tick: u64,
    config: SimConfig,
    ids: IdsSave,
    terrain: Option<TerrainSave>,
    nodes: Vec<NodeSave>,
    segments: Vec<SegmentSave>,
    plots: Vec<Plot>,
    lots: Vec<Lot>,
    buildings: Vec<Building>,
    props: Vec<PlacedProp>,
    population: Population,
    capacities: Capacities,
    population_stats: PopulationStats,
    demand: Demand,
    treasury: Treasury,
    ledger: Ledger,
    finances: Finances,
    goods: Goods,
    utilities: Utilities,
    fields: Fields,
    happiness: HappinessStats,
    traffic: TrafficSave,
    #[serde(default)]
    jobs: crate::citizens::jobs::JobMap,
}

/// Stable hash of a byte slice (the file checksum).
#[must_use]
pub fn checksum(bytes: &[u8]) -> u64 {
    let mut h = StableHasher::default();
    h.write(bytes);
    h.finish()
}

fn encode_value<T: Serialize>(v: &T) -> Result<Vec<u8>, SaveError> {
    let mut out = Vec::new();
    v.serialize(&mut rmp_serde::Serializer::new(&mut out).with_struct_map())
        .map_err(|e| SaveError::Encode(e.to_string()))?;
    Ok(out)
}

fn quantize(h: &[f32]) -> Heights {
    let min = h.iter().copied().fold(f32::INFINITY, f32::min);
    if !min.is_finite() {
        return Heights::Raw(h.to_vec());
    }
    let mut q = Vec::with_capacity(h.len());
    for v in h {
        let steps = ((v - min) * 256.0).round();
        if !(0.0..=65_535.0).contains(&steps) || min + steps / 256.0 != *v {
            return Heights::Raw(h.to_vec());
        }
        q.push(steps as u16);
    }
    Heights::Quantized { min, q }
}

fn capture_terrain(t: &TerrainRes) -> TerrainSave {
    let terr = t.terrain();
    let base = terr.base();
    let water = terr.water();
    let lv = water.levels();
    let mut wet = Vec::new();
    for j in 0..lv.n() {
        for i in 0..lv.n() {
            if let Some(level) = water.level(i, j) {
                wet.push(((j * lv.n() + i) as u32, level, water.body(i, j)));
            }
        }
    }
    let forest = terr.forest();
    TerrainSave {
        n: base.n() as u32,
        spacing_m: base.spacing_m(),
        heights: quantize(base.data()),
        water: wet,
        bodies: water
            .bodies()
            .iter()
            .map(|b| u8::from(*b == cb_terrain::WaterBodyKind::Lake))
            .collect(),
        forest_n: forest.n() as u32,
        forest_spacing_m: forest.spacing_m(),
        forest: forest.data().to_vec(),
        material_seed: terr.material_seed(),
        outside: t.outside_ends().to_vec(),
        rivers: t.rivers().to_vec(),
    }
}

fn restore_terrain(s: &TerrainSave) -> Result<TerrainRes, SaveError> {
    let n = s.n as usize;
    let heights = match &s.heights {
        Heights::Quantized { min, q } => q.iter().map(|q| min + f32::from(*q) / 256.0).collect(),
        Heights::Raw(v) => v.clone(),
    };
    let bad = || SaveError::Invalid("terrain grid size".into());
    let base = cb_terrain::HeightGrid::from_vec(n, s.spacing_m, heights).ok_or_else(bad)?;
    let mut water = cb_terrain::WaterField::dry_like(&base);
    for kind in &s.bodies {
        water.add_body(if *kind == 1 {
            cb_terrain::WaterBodyKind::Lake
        } else {
            cb_terrain::WaterBodyKind::River
        });
    }
    for &(idx, level, body) in &s.water {
        let idx = idx as usize;
        if idx >= n * n || usize::from(body) > s.bodies.len() {
            return Err(SaveError::Invalid("water sample out of range".into()));
        }
        water.set(idx % n, idx / n, level, body);
    }
    let forest =
        cb_terrain::Grid::from_vec(s.forest_n as usize, s.forest_spacing_m, s.forest.clone())
            .ok_or_else(bad)?;
    let (wdist, wlevel) = cb_terrain::materials::water_distance(&water, 64.0);
    let materials =
        cb_terrain::materials::compute(&base, &water, &wdist, &wlevel, &forest, s.material_seed);
    let world = cb_terrain::World {
        base,
        water,
        rivers: Vec::new(),
        lakes: Vec::new(),
        outside: Vec::new(),
        forest,
        materials,
        material_seed: s.material_seed,
        report: cb_terrain::WorldReport {
            buildable_km2: 0.0,
            shore_distance_m: f32::INFINITY,
            rerolls: (0, 0, 0),
            relaxed: Vec::new(),
        },
    };
    let mut t = TerrainRes::from_world(world);
    t.outside.clone_from(&s.outside);
    t.rivers.clone_from(&s.rivers);
    Ok(t)
}

fn res<T: bevy::ecs::resource::Resource + Clone + Default>(world: &World) -> T {
    world.get_resource::<T>().cloned().unwrap_or_default()
}

/// Captures the body of the current world.
#[must_use]
pub fn capture(world: &World) -> Body {
    let objects = snapshot::capture(world);
    let ids = world.get_resource::<Ids>();
    let ids_save = IdsSave {
        nodes: ids.map(|i| i.nodes.allocator().clone()).unwrap_or_default(),
        segments: ids
            .map(|i| i.segments.allocator().clone())
            .unwrap_or_default(),
        plots: ids.map(|i| i.plots.allocator().clone()).unwrap_or_default(),
        lots: ids.map(|i| i.lots.allocator().clone()).unwrap_or_default(),
        buildings: ids
            .map(|i| i.buildings.allocator().clone())
            .unwrap_or_default(),
        props: ids.map(|i| i.props.allocator().clone()).unwrap_or_default(),
        next_stamp: ids.map_or(0, |i| i.next_stamp),
    };
    Body {
        seed: world.get_resource::<WorldSeed>().map_or(0, |s| s.0),
        tick: world.get_resource::<SimClock>().map_or(0, |c| c.tick().0),
        config: res::<SimConfig>(world),
        ids: ids_save,
        terrain: world.get_resource::<TerrainRes>().map(capture_terrain),
        nodes: objects
            .nodes
            .values()
            .map(|n| NodeSave {
                id: n.id,
                pos: n.pos,
                segments: n.segments.clone(),
                control: n.control,
                custom: n.custom.clone(),
            })
            .collect(),
        segments: objects
            .segments
            .values()
            .map(|s| SegmentSave {
                id: s.id,
                start: s.start,
                end: s.end,
                kind: s.kind,
                variant: s.variant,
                style: s.style,
                curve: s.curve,
                profile: s.profile.clone(),
                built_tick: s.built_tick,
                spans: s.spans.clone(),
                piers: s.piers.clone(),
            })
            .collect(),
        plots: objects.plots.into_values().collect(),
        lots: objects.lots.into_values().collect(),
        buildings: objects.buildings.into_values().collect(),
        props: objects.props.into_values().collect(),
        population: res::<Population>(world),
        capacities: res::<Capacities>(world),
        population_stats: res::<PopulationStats>(world),
        demand: res::<Demand>(world),
        treasury: world
            .get_resource::<Treasury>()
            .copied()
            .unwrap_or_else(|| Treasury::new(cb_core::Money::ZERO)),
        ledger: res::<Ledger>(world),
        finances: world
            .get_resource::<Finances>()
            .cloned()
            .unwrap_or_default(),
        goods: res::<Goods>(world),
        utilities: res::<Utilities>(world),
        fields: res::<Fields>(world),
        happiness: res::<HappinessStats>(world),
        jobs: res::<crate::citizens::jobs::JobMap>(world),
        traffic: world
            .get_resource::<Traffic>()
            .map(Traffic::to_save)
            .unwrap_or_default(),
    }
}

/// Writes a save of the current world.
///
/// # Errors
/// [`SaveError::Encode`] if serialization fails.
pub fn save(world: &World, game_version: &str) -> Result<Vec<u8>, SaveError> {
    let _span = tracing::info_span!("save_encode").entered();
    let body = capture(world);
    let header = Header {
        format_version: FORMAT_VERSION,
        game_version: game_version.to_owned(),
        catalog_hash: world
            .get_resource::<crate::catalog::Catalog>()
            .map_or(0, crate::catalog::Catalog::content_hash),
        seed: body.seed,
        tick: body.tick,
        state_hash: crate::state_hash(world),
        population: u32::try_from(body.population.population()).unwrap_or(u32::MAX),
        money_cents: body.treasury.balance().0,
    };
    let h = encode_value(&header)?;
    let b = encode_value(&body)?;
    Ok(assemble(FORMAT_VERSION, &h, &b))
}

/// Frames encoded header and body bytes into a file (magic, version, lengths, checksum).
#[must_use]
pub fn assemble(version: u32, header: &[u8], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(16 + header.len() + body.len() + 8);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(
        &u32::try_from(header.len())
            .unwrap_or(u32::MAX)
            .to_le_bytes(),
    );
    out.extend_from_slice(header);
    out.extend_from_slice(body);
    let sum = checksum(&out);
    out.extend_from_slice(&sum.to_le_bytes());
    out
}

/// Splits a file into `(version, header bytes, body bytes)` after checking magic and checksum.
///
/// # Errors
/// [`SaveError::BadMagic`], [`SaveError::Truncated`] or [`SaveError::Checksum`].
pub fn split(bytes: &[u8]) -> Result<(u32, &[u8], &[u8]), SaveError> {
    if bytes.len() < 8 + 4 + 4 + 8 {
        return Err(if bytes.starts_with(&MAGIC) || MAGIC.starts_with(bytes) {
            SaveError::Truncated
        } else {
            SaveError::BadMagic
        });
    }
    if bytes[..8] != MAGIC {
        return Err(SaveError::BadMagic);
    }
    let (payload, sum) = bytes.split_at(bytes.len() - 8);
    let mut s = [0u8; 8];
    s.copy_from_slice(sum);
    if checksum(payload) != u64::from_le_bytes(s) {
        return Err(SaveError::Checksum);
    }
    let word = |at: usize| {
        let mut w = [0u8; 4];
        w.copy_from_slice(&payload[at..at + 4]);
        u32::from_le_bytes(w)
    };
    let version = word(8);
    let hlen = word(12) as usize;
    let rest = &payload[16..];
    if hlen > rest.len() {
        return Err(SaveError::Truncated);
    }
    let (h, b) = rest.split_at(hlen);
    Ok((version, h, b))
}

/// Reads only the header (load dialog).
///
/// # Errors
/// Framing, checksum and decoding errors.
pub fn read_header(bytes: &[u8]) -> Result<Header, SaveError> {
    let (_, h, _) = split(bytes)?;
    rmp_serde::from_slice(h).map_err(|e| SaveError::Decode(e.to_string()))
}

/// Decodes and migrates a file (no world access).
///
/// # Errors
/// Framing, checksum, version, decoding, migration and validation errors.
pub fn decode(bytes: &[u8]) -> Result<(Header, Body), SaveError> {
    let (version, h, b) = split(bytes)?;
    if version > FORMAT_VERSION {
        return Err(SaveError::UnsupportedVersion(version));
    }
    let header: Header = rmp_serde::from_slice(h).map_err(|e| SaveError::Decode(e.to_string()))?;
    let body: Body = if version == FORMAT_VERSION {
        rmp_serde::from_slice(b).map_err(|e| SaveError::Decode(e.to_string()))?
    } else {
        let value: rmpv::Value =
            rmp_serde::from_slice(b).map_err(|e| SaveError::Decode(e.to_string()))?;
        let value = migrate::migrate(value, version, FORMAT_VERSION, migrate::MIGRATIONS)?;
        rmpv::ext::from_value(value).map_err(|e| SaveError::Decode(e.to_string()))?
    };
    validate(&body)?;
    Ok((header, body))
}

/// Referential integrity of a decoded body.
///
/// # Errors
/// [`SaveError::Invalid`] naming the first problem found.
pub fn validate(b: &Body) -> Result<(), SaveError> {
    let bad = |what: String| Err(SaveError::Invalid(what));
    let nodes: BTreeSet<NodeId> = b.nodes.iter().map(|n| n.id).collect();
    let segs: BTreeSet<SegmentId> = b.segments.iter().map(|s| s.id).collect();
    let plots: BTreeSet<PlotId> = b.plots.iter().map(|p| p.id).collect();
    let lots: BTreeSet<LotId> = b.lots.iter().map(|l| l.id).collect();
    let buildings: BTreeSet<BuildingId> = b.buildings.iter().map(|x| x.id).collect();
    if nodes.len() != b.nodes.len() || segs.len() != b.segments.len() {
        return bad("duplicate road ids".into());
    }
    if plots.len() != b.plots.len()
        || lots.len() != b.lots.len()
        || buildings.len() != b.buildings.len()
    {
        return bad("duplicate object ids".into());
    }
    for s in &b.segments {
        if !nodes.contains(&s.start) || !nodes.contains(&s.end) || s.start == s.end {
            return bad(format!("{} has missing or equal end nodes", s.id));
        }
        if !s.curve.p.iter().all(|p| p.is_finite()) || s.profile.s.len() != s.profile.h.len() {
            return bad(format!("{} has a malformed curve or profile", s.id));
        }
    }
    for n in &b.nodes {
        if !n.pos.is_finite() {
            return bad(format!("{} position not finite", n.id));
        }
        if let Some(s) = n.segments.iter().find(|s| !segs.contains(s)) {
            return bad(format!("{} lists missing {s}", n.id));
        }
    }
    for p in &b.plots {
        if let Some(s) = p.segments.iter().find(|s| !segs.contains(s)) {
            return bad(format!("{} fronts missing {s}", p.id));
        }
        if let Some(l) = p.lots.iter().find(|l| !lots.contains(l)) {
            return bad(format!("{} lists missing {l}", p.id));
        }
    }
    for l in &b.lots {
        if !plots.contains(&l.plot) {
            return bad(format!("{} outside any plot", l.id));
        }
        if let Some(x) = l.building.filter(|x| !buildings.contains(x)) {
            return bad(format!("{} holds missing {x}", l.id));
        }
    }
    for x in &b.buildings {
        if let Some(l) = x.lot.filter(|l| !lots.contains(l)) {
            return bad(format!("{} stands on missing {l}", x.id));
        }
    }
    let pop = &b.population;
    for (hid, h) in &pop.households {
        if !buildings.contains(&h.home) {
            return bad(format!("{hid} lives in missing {}", h.home));
        }
        if let Some(c) = h.members.iter().find(|c| !pop.citizens.contains_key(c)) {
            return bad(format!("{hid} lists missing {c}"));
        }
    }
    for (cid, c) in &pop.citizens {
        if !pop.households.contains_key(&c.household) {
            return bad(format!("{cid} belongs to missing {}", c.household));
        }
        if let Some((w, _)) = c.job.filter(|(w, _)| !buildings.contains(w)) {
            return bad(format!("{cid} works at missing {w}"));
        }
    }
    let max_ok = |next: u32, max: Option<u32>| max.is_none_or(|m| next > m);
    let ok = max_ok(b.ids.nodes.peek_next(), nodes.last().map(|x| x.get()))
        && max_ok(b.ids.segments.peek_next(), segs.last().map(|x| x.get()))
        && max_ok(b.ids.plots.peek_next(), plots.last().map(|x| x.get()))
        && max_ok(b.ids.lots.peek_next(), lots.last().map(|x| x.get()))
        && max_ok(
            b.ids.buildings.peek_next(),
            buildings.last().map(|x| x.get()),
        )
        && max_ok(
            pop.household_ids.peek_next(),
            pop.households.keys().last().map(|x| x.get()),
        )
        && max_ok(
            pop.citizen_ids.peek_next(),
            pop.citizens.keys().last().map(|x| x.get()),
        );
    if !ok {
        return bad("an id allocator is behind its objects".into());
    }
    if let Some(t) = &b.terrain {
        let n = t.n as usize;
        let len = match &t.heights {
            Heights::Quantized { q, .. } => q.len(),
            Heights::Raw(v) => v.len(),
        };
        if n < 2 || len != n * n || t.forest.len() != (t.forest_n as usize).pow(2) {
            return bad("terrain grid size".into());
        }
    }
    Ok(())
}

fn node_of(s: &NodeSave) -> RoadNode {
    RoadNode {
        id: s.id,
        pos: s.pos,
        segments: s.segments.clone(),
        junction: None,
        approaches: SmallVec::new(),
        junction_stamp: None,
        control: s.control,
        custom: s.custom.clone(),
        controls: SmallVec::new(),
        phases: Vec::new(),
        markings: cb_network::markings::Markings::default(),
        ring: false,
    }
}

fn segment_of(s: &SegmentSave) -> RoadSegment {
    RoadSegment {
        id: s.id,
        start: s.start,
        end: s.end,
        kind: s.kind,
        variant: s.variant,
        style: s.style,
        curve: s.curve,
        profile: s.profile.clone(),
        built_tick: s.built_tick,
        geom: SegmentGeom::new(s.curve, CrossSection::of(s.kind).half_width_m()),
        cut: (0.0, 0.0),
        stamps: SmallVec::new(),
        spans: s.spans.clone(),
        piers: s.piers.clone(),
    }
}

/// Loads a save into a fresh sim world (built with the sim plugins and no objects).
///
/// # Errors
/// Any [`decode`] error, or [`SaveError::Invalid`] if the world already holds objects.
pub fn load(world: &mut World, bytes: &[u8]) -> Result<Header, SaveError> {
    let _span = tracing::info_span!("save_load").entered();
    let (header, body) = decode(bytes)?;
    if world
        .get_resource::<Ids>()
        .is_some_and(|i| !i.nodes.is_empty() || !i.plots.is_empty() || !i.buildings.is_empty())
    {
        return Err(SaveError::Invalid("load needs an empty world".into()));
    }
    apply_body(world, body);
    Ok(header)
}

fn apply_body(world: &mut World, body: Body) {
    world.insert_resource(WorldSeed(body.seed));
    world.insert_resource(SimClock::at(body.tick));
    world.insert_resource(body.config);
    if let Some(t) = &body.terrain {
        match restore_terrain(t) {
            Ok(t) => {
                world.insert_resource(t);
            }
            Err(e) => tracing::error!("terrain restore failed after validation: {e}"),
        }
    } else {
        world.remove_resource::<TerrainRes>();
    }
    let delta = Delta {
        nodes: body
            .nodes
            .iter()
            .map(|n| (n.id, Some(node_of(n))))
            .collect::<BTreeMap<_, _>>(),
        segments: body
            .segments
            .iter()
            .map(|s| (s.id, Some(segment_of(s))))
            .collect(),
        plots: body.plots.into_iter().map(|p| (p.id, Some(p))).collect(),
        lots: body.lots.into_iter().map(|l| (l.id, Some(l))).collect(),
        buildings: body
            .buildings
            .into_iter()
            .map(|b| (b.id, Some(b)))
            .collect(),
        props: body.props.into_iter().map(|p| (p.id, Some(p))).collect(),
    };
    snapshot::apply(world, &delta);
    if let Some(mut ids) = world.get_resource_mut::<Ids>() {
        ids.nodes.set_allocator(body.ids.nodes);
        ids.segments.set_allocator(body.ids.segments);
        ids.plots.set_allocator(body.ids.plots);
        ids.lots.set_allocator(body.ids.lots);
        ids.buildings.set_allocator(body.ids.buildings);
        ids.props.set_allocator(body.ids.props);
        ids.next_stamp = ids.next_stamp.max(body.ids.next_stamp);
    }
    // The restored plots and lots already match the restored roads.
    world.insert_resource(DirtyPlots::default());
    let mut pop = body.population;
    pop.rebuild_indices();
    world.insert_resource(pop);
    world.insert_resource(body.capacities);
    world.insert_resource(body.population_stats);
    world.insert_resource(body.demand);
    world.insert_resource(body.treasury);
    world.insert_resource(body.ledger);
    world.insert_resource(body.finances);
    world.insert_resource(body.goods);
    world.insert_resource(body.utilities);
    world.insert_resource(body.fields);
    world.insert_resource(body.happiness);
    world.insert_resource(body.jobs);
    world.insert_resource(Traffic::from_save(&body.traffic));
    world.insert_resource(crate::history::History::default());
}
