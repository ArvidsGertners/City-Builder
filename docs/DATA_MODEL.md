# Data Model

Status: **Accepted (Phase 1 plan)**. Field lists are normative for the first implementation;
additions are fine, removals/renames need a save migration (ADR-0020).

Conventions: world units are meters, Y-up; 2D types use `Vec2(x, z)` (ADR-0033). Struct
sketches below list fields; in code, sim component/resource fields are `pub(crate)` with
`pub` read accessors (ADR-0005). `f32`
everywhere for world coordinates (±2048 m map → ~0.25 mm precision); `f64` only inside
robust geometric predicates. Money is `Money(i64)` in cents. Time is `GameTime(i64)` in
game milliseconds; sim ticks are `u64`.

---

## 1. Identity

```rust
// cb_core::ids — u32 newtypes, allocated monotonically, NEVER reused (undo restores originals)
NodeId, SegmentId, PlotId, LotId, BuildingId, StopId, LineId, HouseholdId, CitizenId,
OutsideConnectionId, IncidentId
// Generational (dense SoA stores, not persisted across loads as identity):
LaneId, ConnectorId, LinkId (graph-local, rebuilt), VehicleId(gen+index), PedestrianId

#[derive(Component)] struct ObjectId<K>(K);          // on every sim entity with a stable ID
#[derive(Resource)]  struct IdMap<K> { by_id: HashMap<K, Entity> }  // lookup only, never iterated for logic
#[derive(Resource)]  struct IdAllocators { next: [u32; N_KINDS] }   // persisted
enum StableRef { Node(NodeId), Segment(SegmentId), Plot(PlotId), Lot(LotId), Building(BuildingId),
                 Stop(StopId), Line(LineId), Vehicle(VehicleId), Household(HouseholdId) }
```

Why entities for objects but stable IDs in data: `Entity` values change across save/load;
commands, saves, undo snapshots and cross-references always use stable IDs (ADR-0015).

---

## 2. Terrain (resource, no entities)

```rust
#[derive(Resource)] struct Terrain(cb_terrain::Terrain);
struct Terrain {
    size_m: f32,                   // 4096
    spacing_m: f32,                // 2.0  → 2049 × 2049 samples
    base: HeightGrid,              // procedural heights from seed (regenerable, not saved)
    stamps: StampSet,              // deformation stamps (saved): road corridors, pads, bridge abutments
    height: HeightGrid,            // final = composite(base, stamps); recomputed per dirty rect
    water: WaterField,             // water surface height per sample (NaN = dry), body ids (lake/river)
    materials: MaterialWeights,    // per sample [grass, dirt, rock, sand, forest] u8 weights (derived)
    forest_mask: ScalarGrid<u8>,   // procedural forest density (derived from seed)
    minmax: MinMaxQuadtree,        // for ray marching (derived)
}
struct Stamp { id: StampId, owner: StableRef, kind: StampKind, aabb: Aabb2, priority: u8 }
enum StampKind {
    Corridor { centerline: Polyline3, half_width_m: f32, shoulder_m: f32, max_side_slope: f32 },
    Pad { polygon: Polygon2, target_h: f32, falloff_m: f32 },
    Clear { polygon: Polygon2 },          // removes forest/props (no height change)
    Paint { polygon: Polygon2, material: TerrainMaterial, falloff_m: f32 },
}
```

Height query API: `height_at(Vec2) -> f32` uses the same triangle split as the render mesh
(consistent: vehicles and buildings sit exactly on the rendered surface at LOD0);
`normal_at`, `slope_at`, `water_depth_at`, `is_water(Vec2)`, `raycast(ray) -> Option<Hit>`.

## 3. Natural decoration (resource)

```rust
#[derive(Resource)] struct PropLayer {
    instances: Vec<PropInstance>,        // generated deterministically per scatter cell
    removed: BitSet,                      // excluded by stamps/plots/buildings/roads
    index: SpatialHash<u32>,              // instance index lookups
}
struct PropInstance { pos: Vec3, kind: PropKind /* Tree(Conifer|Deciduous|Birch|Bush), Rock, GrassClump */,
                      variant: u8, scale: f32, yaw: f32, seed: u32 }
```
Not saved: regenerated from seed; exclusion recomputed from saved roads/plots/buildings.

---

## 4. Road network (entities + derived resources)

### 4.1 Entities

```rust
#[derive(Component)] struct RoadNode {
    pos: Vec3,                            // y = road surface height at node
    segments: SmallVec<[SegmentId; 4]>,   // incident segments (sorted by angle in JunctionGeometry)
    kind: NodeKind,                       // Junction | Continuation | DeadEnd | Outside(OutsideConnectionId)
    control: IntersectionControl,         // Auto (priority) | AllWayStop | Lights(LightPlanOverride) | Yield overrides
    elevation_offset_m: f32,              // player-set (PgUp/PgDn) over graded terrain
}
#[derive(Component)] struct JunctionGeometry {   // derived, rebuilt when dirty
    approaches: SmallVec<[Approach; 4]>,  // per incident segment: cut distance, edge points, priority class
    polygon: Polygon2, surface_h: f32,    // junction surface
    crosswalks: SmallVec<[Crosswalk; 4]>, stop_lines: SmallVec<[StopLine; 8]>,
    connectors: SmallVec<[ConnectorId; 16]>, conflict_zones: SmallVec<[ConflictZoneId; 16]>,
}

#[derive(Component)] struct RoadSegment {
    start: NodeId, end: NodeId,
    curve: CubicBezier2,                  // horizontal alignment in XZ (ADR-0007)
    profile: VerticalProfile,             // heights at fixed arc-length spacing (4 m), smooth interp
    road_type: RoadTypeId,                // catalog: cross-section, speed, capacity, costs
    direction: Direction,                 // Both | Forward | Backward (one-way relative to curve)
    style: SegmentStyle,
    structure: SmallVec<[StructureSpan; 2]>, // Ground | Embankment | Bridge{span params} per arc-length range
    built_tick: u64,
}
struct SegmentStyle {
    surface: Surface,                     // DarkAsphalt | LightAsphalt | Cobblestone | Dirt
    sidewalks: SideFlags, curbs: bool,
    grass_strip: SideFlags, tree_rows: Option<Spacing>, lamps: Option<Spacing>,
    bus_lanes: bool, tram_tracks: bool,   // tram tracks only on types that allow them
}
#[derive(Component)] struct SegmentGeometry {   // derived
    t_range: (f32, f32),                  // trimmed by junction cut-backs at both ends
    length_m: f32, arc: ArcLengthLut,
    left_edge: Polyline3, right_edge: Polyline3, corridor: Polygon2,
    lanes: SmallVec<[LaneId; 8]>,
}
#[derive(Component)] struct SegmentTrafficStats { volume_vph: f32, speed_ratio: f32, congestion: f32 /*0..1*/ }
#[derive(Component)] struct SegmentDependents { plots: SmallVec<[PlotId; 4]>, stops: SmallVec<[StopId; 2]> }

#[derive(Component)] struct OutsideConnection { node: NodeId, direction_out: Vec2, kinds: TradeFlags }
```

### 4.2 Lanes and graphs (resources, derived)

```rust
#[derive(Resource)] struct LaneGraph {          // cb_network::LaneGraph
    lanes: DenseSlotMap<LaneId, Lane>,
    connectors: DenseSlotMap<ConnectorId, Connector>,
    conflict_zones: DenseSlotMap<ConflictZoneId, ConflictZone>,
    by_segment: HashMap<SegmentId, SmallVec<[LaneId; 8]>>,
    version: u64,
}
struct Lane {
    segment: SegmentId, index: u8, kind: LaneKind /* Car|Bus|Tram|CarTramShared|Sidewalk|Bike(reserved) */,
    dir: Direction, offset_m: f32, width_m: f32,
    polyline: Polyline3,                   // ~1 m spacing on curves, cumulative arc length
    length_m: f32, speed_limit_mps: f32,
    outgoing: SmallVec<[ConnectorId; 4]>, incoming: SmallVec<[ConnectorId; 4]>,
    left: Option<LaneId>, right: Option<LaneId>,     // same-direction neighbors for lane changes
}
struct Connector { from: LaneId, to: LaneId, node: NodeId, polyline: Polyline3, length_m: f32,
                   turn: Turn /*Straight|Left|Right|UTurn*/, conflicts: SmallVec<[(ConflictZoneId, f32 /*s on connector*/); 6]> }
struct ConflictZone { node: NodeId, connectors: SmallVec<[ConnectorId; 4]>, center: Vec3, radius_m: f32 }

#[derive(Resource)] struct LinkGraph {          // cb_traffic routing graph (segment-direction level)
    links: Vec<Link>,                      // one per (segment, travel direction, mode set)
    turns: Vec<TurnEdge>,                  // allowed link→link movements via nodes, with penalty
    landmarks: AltLandmarks,               // 8–16 landmarks, distance tables (free-flow)
    version: u64,
}
#[derive(Resource)] struct Connectivity { node_component: HashMap<NodeId, ComponentId>,
                                          components: Vec<ComponentInfo /* has_outside, node count */> }
```

---

## 5. Plots, lots, buildings

```rust
#[derive(Component)] struct Plot {
    zone: ZoneType,                        // Residential | Commercial | Industrial
    polygon: Polygon2,                     // CCW, simple, validated
    frontage: SmallVec<[FrontageRef; 2]>,  // road-facing edges: (segment, side, t0, t1)
    subdivision: SubdivisionParams { target_lots: u16, seed_salt: u32, depth_mode: DepthMode },
    lots: SmallVec<[LotId; 16]>,
    access: AccessState,                   // Connected | Disconnected{since_tick}
}
#[derive(Component)] struct Lot {
    plot: PlotId, polygon: Polygon2, area_m2: f32,
    frontage: FrontageRef, frontage_len_m: f32,
    access_point: AccessPoint { segment: SegmentId, t: f32, side: Side, pos: Vec3 },
    slope: LotSlope { min_h: f32, max_h: f32, mean_grade: f32 },
    building: Option<BuildingId>, yard_seed: u32,
}

#[derive(Component)] struct Building {
    kind: BuildingKind,                    // Zoned{lot, zone} | Service(ServiceTypeId) | Utility(UtilityTypeId)
                                           // | Transit(TransitBuildingId) | Special(SpecialId)
    level: u8, seed: u64,                  // seed = derive_seed(world, Domain::Building, id, level)
    placement: Placement { origin: Vec3, yaw: f32 },     // free placement for non-zoned; lot frame for zoned
    footprint: Footprint,                  // massing modules (OBBs) or polygon; from cb_procgen::massing
    height_m: f32,
    built_tick: u64,
}
#[derive(Component)] enum Condition { Constructing{progress: f32}, Active, Declining{since: GameTime},
                                      Abandoned{since: GameTime}, Burning{intensity: f32, since: GameTime}, Ruined }
#[derive(Component)] struct Residential { capacity_households: u16, households: SmallVec<[HouseholdId; 8]> }
#[derive(Component)] struct Workplace   { jobs: [u16; 3] /* by edu level */, filled: [u16; 3], sector: Sector }
#[derive(Component)] struct Commercial  { goods_stock: f32, goods_capacity: f32, daily_customers: u32 }
#[derive(Component)] struct Industrial  { output_per_hour: f32, goods_stock: f32, pollution: f32 }
#[derive(Component)] struct UtilityUse  { power_kw: f32, water_m3h: f32, sewage_m3h: f32, garbage_kg_day: f32 }
#[derive(Component)] struct UtilityStatus { power: Supply, water: Supply, sewage: Supply, garbage_kg: f32 }
#[derive(Component)] struct ServiceExposure { police: f32, fire: f32, health: f32, education: [f32; 3],
                                              transit: f32, parks: f32 }        // 0..1 sampled coverage
#[derive(Component)] struct Risks { crime: f32, fire_risk: f32, sickness: f32 }
#[derive(Component)] struct LevelProgress { score: f32, days_above: u16, days_below: u16, blocked_by: LevelBlockers }
#[derive(Component)] struct Problems(ProblemFlags);   // NoRoad, NoPower, NoWater, NoSewage, Garbage, Fire,
                                                      // Crime, Sick, NoWorkers, NoCustomers, NoGoods, Abandoned, Pollution, Noise
#[derive(Component)] struct RoadAccess { segment: SegmentId, t: f32, side: Side, pos: Vec3, component: ComponentId }

#[derive(Component)] struct ServiceBuilding { service: ServiceKind, capacity: u32, reach_m: f32,
                                              vehicles: u8, vehicles_out: u8, budget_factor: f32 }
#[derive(Component)] struct PowerPlant   { output_kw: f32, kind: PowerKind /*Coal|Gas|Wind|Solar*/ }
#[derive(Component)] struct WaterPump    { capacity_m3h: f32, source: WaterSource /*Surface|Ground*/ }
#[derive(Component)] struct SewagePlant  { capacity_m3h: f32, treatment: f32 }
#[derive(Component)] struct GarbageFacility { capacity_kg_day: f32, storage_kg: f32, stored_kg: f32, trucks: u8, kind: Landfill|Incinerator }
#[derive(Component)] struct Park { attractiveness: f32, radius_m: f32 }
```

The building **mesh** is never stored; it is regenerated from `(kind, level, seed, lot polygon
or placement, terrain heights)` (ADR-0029). `Footprint` and `height_m` are the cheap
deterministic massing used by the sim for picking, coverage sampling and overlap tests.

---

## 6. Citizens (resource tables, not entities) — ADR-0016

```rust
#[derive(Resource)] struct Population {
    households: DenseSlotMap<HouseholdId, Household>,
    citizens: DenseSlotMap<CitizenId, Citizen>,
    schedule: TimeWheel<Activity>,          // minute-resolution wheel of upcoming activities
    unemployed: [SortedVec<CitizenId>; 3],  // by education level, for job matching
    homeless_queue: VecDeque<ImmigrantHousehold>,
}
struct Household {
    home: BuildingId, members: SmallVec<[CitizenId; 5]>, wealth: Wealth /*Low|Mid|High*/,
    cars: u8, happiness: f32, happiness_factors: HappinessBreakdown,
    moved_in: GameTime, unhappy_days: u16,
}
struct Citizen {                             // 32 bytes target
    household: HouseholdId, age_group: AgeGroup /*Child|Student|Adult|Senior*/,
    education: u8 /*0..=3*/, workplace: Option<BuildingId>, school: Option<BuildingId>,
    shift: Shift { start_min: u16, len_min: u16 }, state: CitizenState /*AtHome|AtWork|AtSchool|Shopping|Travelling(TripId)*/,
    health: u8, commute_estimate_min: u16,
}
enum Activity { GoToWork(CitizenId), GoHome(CitizenId), GoToSchool(CitizenId), Shop(HouseholdId), Leisure(HouseholdId) }
```

Individual citizens are visible in the UI (building info lists residents/workers with
generated names from seed + ID) but are never ECS entities.

---

## 7. Trips and traffic (resources) — ADR-0010, ADR-0011

```rust
#[derive(Resource)] struct Trips { active: DenseSlotMap<TripId, Trip> }
struct Trip { purpose: Purpose, origin: TripEnd, dest: TripEnd, mode: Mode /*Car|Transit|Walk|Truck|Service*/,
              persons: u16, start: GameTime, route: Option<RouteHandle>, agent: Option<VehicleId>,
              weight: u16 /* trips represented when sampled-out */ }

#[derive(Resource)] struct TrafficSim(cb_traffic::TrafficWorld);
struct TrafficWorld {
    vehicles: VehicleStore,                  // SoA: kind, lane_or_connector, s_m, v_mps, length, route cursor,
                                             //      lane_change {from, t}, mode (Interacting|Ghost),
                                             //      wait_s, stuck_s, color_idx, variant, trip
    lane_occupancy: Vec<LaneQueue>,          // per lane/connector: vehicles ordered by s (leader lookup O(1))
    junctions: Vec<JunctionController>,      // per node: control state, light phase, reservations
    flow: FlowModel,                         // per link: volume EMA (agents + background), BPR time
    router: Router, cache: RouteCache,
    config: TrafficConfig { agent_cap, enabled, weather_speed_factor },
    diag: GridlockLog,
}
```

Agent state is 1D (lane, `s`, `v`). World-space poses are **not** sim state: the
presentation computes `pose(lane, s, lateral_offset)` only for agents inside the view
(frustum + margin) and interpolates between the previous and current tick. Camera position
therefore never influences simulation results (ADR-0010: presentation LOD vs. deterministic
aggregation beyond `agent_cap`).

---

## 8. Public transport — ADR-0035

```rust
#[derive(Component)] struct TransitStop { kind: TransitKind /*Bus|Tram*/, segment: SegmentId, t: f32, side: Side,
                                          pos: Vec3, waiting: SmallVec<[WaitingGroup; 8]> /* dest stop, count, since */,
                                          lines: SmallVec<[LineId; 4]> }
#[derive(Component)] struct TransitLine { kind: TransitKind, name_idx: u16, color: LineColor,
                                          stops: Vec<StopId>, circular: bool, vehicles_target: u8,
                                          depot: BuildingId, fare: Money, stats: LineStats }
struct LineStats { riders_today: u32, riders_history: RingBuffer<u32, 16>, income_today: Money, cost_today: Money,
                   avg_headway_s: f32, occupancy: f32 }
#[derive(Resource)] struct TransitNetwork {      // derived
    graph: TransitGraph,                         // nodes = (stop, line) + stop; edges = ride/transfer/wait
    stop_access: HashMap<BuildingId, SmallVec<[(StopId, f32 /*walk m*/); 4]>>,
    paths_between_stops: HashMap<(LineId, u16), RouteHandle>,
}
```
Transit vehicles are agents in `TrafficWorld` with `VehicleKind::Bus|Tram` and a `TransitRun
{ line, next_stop_idx, passengers: SmallVec<[(StopId, u16); 8]> }`.

---

## 9. Services, utilities, fields

```rust
#[derive(Resource)] struct Coverage {                  // per service kind, per segment, sampled along length
    per_kind: EnumMap<ServiceKind, SegmentCoverage>,   // SegmentCoverage: HashMap<SegmentId, CoverageSamples (up to 8 along t)>
    emitter_reach: HashMap<BuildingId, SmallVec<[SegmentId; 64]>>,  // for incremental invalidation
}
#[derive(Resource)] struct UtilityNetworks {
    power: Vec<NetworkBalance>, water: Vec<NetworkBalance>, sewage: Vec<NetworkBalance>,  // per connectivity component
}
struct NetworkBalance { component: ComponentId, supply: f32, demand: f32, served_ratio: f32 }
#[derive(Resource)] struct Incidents { active: DenseSlotMap<IncidentId, Incident> }  // Fire, Crime, Medical, GarbageOverflow
#[derive(Resource)] struct Fields {                    // internal rasters, 8 m cells, bilinear sampled — ADR-0028
    ground_pollution: ScalarField, noise: ScalarField, land_value: ScalarField, water_pollution: ScalarField,
}
```

---

## 10. City-wide resources

```rust
#[derive(Resource)] struct WorldSeed(u64);
#[derive(Resource)] struct SimConfig { traffic_enabled: bool, weather_enabled: bool, day_length_real_min: f32,
                                       season_length_days: u8, sandbox: bool, difficulty: Difficulty }
#[derive(Resource)] struct SimClock { tick: u64, game_time: GameTime, ms_per_tick: i64 }
#[derive(Resource)] struct SimSpeed(Speed);            // Paused | X1 | X2 | X4 (driver API)
#[derive(Resource)] struct Catalog { roads, zones, buildings, services, utilities, transit, unlocks, economy, weather }
#[derive(Resource)] struct Economy { money: Money, ledger: Ledger /* per category, hourly + daily history */,
                                     taxes: TaxRates /* per zone & wealth */, budgets: EnumMap<ServiceKind, f32>,
                                     loans: SmallVec<[Loan; 3]>, bankrupt_days: u16 }
#[derive(Resource)] struct Demand { residential: f32, commercial: f32, industrial: f32, breakdown: DemandBreakdown }
#[derive(Resource)] struct Progression { tier: u8, unlocked: BitSet, history: Vec<(u8, GameTime)> }
#[derive(Resource)] struct WeatherState { kind: WeatherKind, intensity: f32, transition: f32, wind: Vec2,
                                          temperature_c: f32, snow_cover: f32, wetness: f32,
                                          forecast: VecDeque<WeatherSpan>, lightning: Option<Strike> }
#[derive(Resource)] struct CityStats { history: StatsRingBuffers }
#[derive(Resource)] struct UndoHistory { … }           // not saved
#[derive(Resource)] struct SpatialIndices { roads: SpatialHash<SegmentChunkRef>, plots: SpatialHash<PlotId>,
                                            lots: SpatialHash<LotId>, buildings: SpatialHash<BuildingId>,
                                            stops: SpatialHash<StopId>, props: (in PropLayer) }
```

---

## 11. Messages (Bevy `Message`)

| Message | Writer → Reader | Payload |
|---|---|---|
| `PlayerCommand` | tools/UI → `EditSet::Commands` | command enum + `CommandSeq` |
| `CommandResult` | Commands → UI/audio/tutorial | seq, `Ok{cost, created}` / `Err(ValidationError)` |
| `SegmentChanged` / `NodeChanged` | road editor → lanes, zoning, views | ids, kind (Added/Modified/Removed), aabb |
| `TerrainChanged` | terrain → terrain view, props, building pads | dirty `Aabb2` |
| `LotsChanged` | zoning → buildings, views | plot id, added/removed/kept lot ids |
| `BuildingChanged` | buildings → views, coverage, utilities | id, what (Spawned/Level/Condition/Removed) |
| `GraphVersionBumped` | lanes → traffic, coverage, transit | new version, removed links |
| `HourElapsed` / `DayElapsed` / `SeasonChanged` | clock → many | calendar |
| `TripRequest` | citizens/economy → trips | trip spec |
| `IncidentStarted` / `IncidentResolved` | services → UI, audio, FX | incident |
| `SimNotification` | any sim → UI toasts, audio | kind, severity, subject, l10n args |
| `MilestoneReached` | progression → UI, audio, tutorial | tier, unlock list |
| `WeatherChanged` / `LightningStrike` | weather → FX, audio, fire | kind/intensity / position |

Observers: `On<Add/Remove, …>` for spatial index maintenance and `IdMap` upkeep;
UI-local `EntityEvent`s for menu widgets.

---

## 12. Spatial indexing — ADR-0013

- `cb_core::spatial::SpatialHash<K>`: loose uniform hash grid with 32 m cells over the map
  (128 × 128 cells, dense `Vec` of small cell vectors). Entries are AABBs; large objects
  are inserted into every cell they overlap, long segments are split into ≤ 32 m pieces so
  each entry stays tight. Removal by key via a reverse map (key → cells).
- Separate instances per category (roads, plots, lots, buildings, stops, props) so queries
  never filter unrelated objects. Vehicles use a per-tick rebuilt point grid inside
  `TrafficWorld` (only for picking/follow and junction proximity queries; car-following
  uses lane queues).
- Queries: `query_aabb`, `query_circle`, `query_segment`, `nearest(k)`, `raycast_cells`
  (3D DDA through cells for tall objects). Results are returned sorted by key for
  deterministic consumers.
- This grid is an **internal acceleration structure**; nothing in gameplay snaps to it.

## 13. What is saved

Saved: seed, map config, terrain stamps, all entities with stable IDs and their *authored* +
*state* components (not derived ones), population tables, trips in flight (without agents),
economy, demand smoothing state, progression, weather state + forecast, clock, ID
allocators, sim config, stats history. Not saved: meshes, lane/link graphs, spatial
indices, coverage, fields (recomputed; land value smoothing state *is* saved), agents
(regenerated), undo history.
