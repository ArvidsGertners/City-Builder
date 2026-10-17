//! Player commands (ADR-0055): the only way presentation code changes the world. Commands queue
//! in [`CommandQueue`], are applied in `EditSet::Commands` in queue order, record their
//! `apply_tick`, and answer with a [`CommandResult`] message. Validation failures are values with
//! localization keys, never log errors.

use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::ids::{BuildingId, NodeId, PlotId, SegmentId};
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use glam::Vec2;
use serde::{Deserialize, Serialize};

use crate::catalog::ZoneKind;
use crate::clock::SimClock;
use crate::config::SimConfig;

/// Where a new road starts or ends.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Endpoint {
    /// A new dead end at the curve end.
    Free,
    /// An existing node (the curve end must lie on it).
    Node(NodeId),
    /// Split an existing segment at the point nearest to the curve end.
    Segment(SegmentId),
}

/// Build a road along a curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BuildRoad {
    /// Road type.
    pub kind: RoadKind,
    /// Horizontal alignment.
    pub curve: CubicBezier2,
    /// Start connection.
    pub start: Endpoint,
    /// End connection.
    pub end: Endpoint,
}

/// What `EditRoadGeometry` changes (ADR-0055).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum GeometryEdit {
    /// Move a node; its segments follow with unchanged tangents.
    MoveNode {
        /// The node.
        node: NodeId,
        /// New plan position.
        to: Vec2,
    },
    /// Move a segment's two inner Bézier control points.
    Reshape {
        /// The segment.
        segment: SegmentId,
        /// New first control point.
        p1: Vec2,
        /// New second control point.
        p2: Vec2,
    },
}

/// One side of a segment between two arc lengths.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FrontageSpan {
    /// Segment.
    pub segment: SegmentId,
    /// Left side in the segment's direction (else right).
    pub left: bool,
    /// Start arc length on the segment (m).
    pub s0: f32,
    /// End arc length (m); `s1 < s0` walks against the segment direction.
    pub s1: f32,
}

/// Draw a plot along road frontage.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatePlot {
    /// Zone.
    pub zone: ZoneKind,
    /// Frontage, possibly around one junction corner (two spans joined at a corner).
    pub frontage: Vec<FrontageSpan>,
    /// Plot depth (m).
    pub depth_m: f32,
    /// Lot count offset from automatic (sub-plot ± control).
    pub dn: i8,
}

/// Debug tooling commands (ADR-0031): go through the queue and the command log like player
/// commands so sim invariants hold and replays stay deterministic; not undoable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DebugCommand {
    /// Rebuild a zoned building at another level (clamped to the lot's cap).
    SetBuildingLevel {
        /// The building.
        building: BuildingId,
        /// Target level.
        level: u8,
    },
    /// Set a building's condition.
    SetCondition {
        /// The building.
        building: BuildingId,
        /// New condition.
        condition: crate::buildings::Condition,
    },
}

/// All player commands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlayerCommand {
    /// Build a road.
    BuildRoad(BuildRoad),
    /// Bulldoze a road segment.
    RemoveSegment {
        /// The segment.
        segment: SegmentId,
        /// Also delete the plots fronting it, with their lots and buildings (bulldoze dialog,
        /// ALGORITHMS §14).
        #[serde(default)]
        remove_plots: bool,
    },
    /// Roundabout preset: a one-way street ring of 15–60 m radius; crossing roads become entries.
    BuildRoundabout {
        /// Ring centre.
        center: Vec2,
        /// Ring radius (m).
        radius_m: f32,
    },
    /// Move a node or reshape a segment.
    EditRoadGeometry(GeometryEdit),
    /// Upgrade/downgrade: change a segment's road type and variant (bus lanes, tram tracks).
    SetRoadType {
        /// The segment.
        segment: SegmentId,
        /// New road type.
        kind: RoadKind,
        /// New variant.
        variant: cb_network::Variant,
    },
    /// Surface and decorations of a segment.
    SetSegmentStyle {
        /// The segment.
        segment: SegmentId,
        /// New style.
        style: cb_network::style::SegmentStyle,
    },
    /// Junction control: mode plus per-approach classes for `ControlMode::Custom`.
    SetApproachControl {
        /// The junction node.
        node: NodeId,
        /// Control mode.
        mode: cb_network::control::ControlMode,
        /// (segment, node is its start, class) for custom control.
        custom: Vec<(SegmentId, bool, cb_network::control::ApproachControl)>,
    },
    /// Place a civic building with its front on the road edge nearest to `pos` (≤ 2 m).
    PlaceBuilding {
        /// What to build.
        kind: cb_procgen::building::civic::CivicKind,
        /// Front-centre position (snapped to the road edge).
        pos: Vec2,
    },
    /// Place a free decoration.
    PlaceProp {
        /// What.
        kind: crate::props::DecorKind,
        /// Where (plan position).
        pos: Vec2,
        /// Rotation about Y (radians).
        yaw: f32,
    },
    /// Remove a placed decoration.
    RemoveProp {
        /// The decoration.
        prop: cb_core::ids::PropId,
    },
    /// Draw a plot.
    CreatePlot(CreatePlot),
    /// Draw a plot by its outline (polygon mode): vertices within 1.5 m of a road edge snap onto
    /// it and the road-edge edges become frontage.
    CreatePlotPolygon {
        /// Zone.
        zone: ZoneKind,
        /// Outline vertices (either winding).
        vertices: Vec<Vec2>,
        /// Lot count offset from automatic.
        dn: i8,
    },
    /// Replace a plot's outline (vertex or edge drag); lots keep their IDs where possible.
    EditPlotPolygon {
        /// The plot.
        plot: PlotId,
        /// New outline vertices.
        vertices: Vec<Vec2>,
    },
    /// Change a plot's frontage and depth.
    EditPlot {
        /// The plot.
        plot: PlotId,
        /// New frontage.
        frontage: Vec<FrontageSpan>,
        /// New depth (m).
        depth_m: f32,
    },
    /// Delete a plot with its lots and buildings.
    DeletePlot {
        /// The plot.
        plot: PlotId,
    },
    /// Change the lot count of a plot by ±1 (or set the offset).
    SetSubdivision {
        /// The plot.
        plot: PlotId,
        /// New lot count offset from automatic.
        dn: i8,
    },
    /// Replace the simulation options (not undoable).
    SetSimConfig(SimConfig),
    /// Set a zone's tax rate in basis points (clamped to 1–25 %; not undoable).
    SetTaxRate {
        /// Zone.
        zone: ZoneKind,
        /// Rate (basis points).
        bp: u16,
    },
    /// Take the loan of a tier (≤ 1 per tier; not undoable).
    TakeLoan {
        /// Tier 0–2.
        tier: u8,
    },
    /// Repay the loan of a tier in full (not undoable).
    RepayLoan {
        /// Tier 0–2.
        tier: u8,
    },
    /// Debug tooling (not undoable).
    Debug(DebugCommand),
    /// Undo the most recent undoable action (ADR-0055).
    Undo,
    /// Redo the most recently undone action.
    Redo,
}

impl PlayerCommand {
    /// Localization key of the action label for the undo history; `None` for commands that
    /// cannot be undone (configuration, undo/redo themselves).
    #[must_use]
    pub fn undo_label(&self) -> Option<&'static str> {
        Some(match self {
            Self::BuildRoad(_) => "undo-build-road",
            Self::BuildRoundabout { .. } => "undo-build-roundabout",
            Self::RemoveSegment { .. } => "undo-remove-segment",
            Self::SetRoadType { .. } => "undo-set-road-type",
            Self::EditRoadGeometry(_) => "undo-edit-road",
            Self::SetSegmentStyle { .. } => "undo-set-segment-style",
            Self::SetApproachControl { .. } => "undo-set-control",
            Self::PlaceBuilding { .. } => "undo-place-building",
            Self::PlaceProp { .. } => "undo-place-prop",
            Self::RemoveProp { .. } => "undo-remove-prop",
            Self::CreatePlot(_) | Self::CreatePlotPolygon { .. } => "undo-create-plot",
            Self::EditPlotPolygon { .. } => "undo-edit-plot",
            Self::EditPlot { .. } => "undo-edit-plot",
            Self::DeletePlot { .. } => "undo-delete-plot",
            Self::SetSubdivision { .. } => "undo-set-subdivision",
            Self::SetSimConfig(_)
            | Self::SetTaxRate { .. }
            | Self::TakeLoan { .. }
            | Self::RepayLoan { .. }
            | Self::Debug(_)
            | Self::Undo
            | Self::Redo => return None,
        })
    }
}

/// Sequence number of a queued command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CommandSeq(pub u64);

/// Queued commands, applied in order in `EditSet::Commands`.
#[derive(Resource, Default, Debug)]
pub struct CommandQueue {
    pending: Vec<(CommandSeq, PlayerCommand)>,
    next: u64,
}

impl CommandQueue {
    /// Queues a command.
    pub fn push(&mut self, cmd: PlayerCommand) -> CommandSeq {
        self.next += 1;
        let seq = CommandSeq(self.next);
        self.pending.push((seq, cmd));
        seq
    }

    /// Number of queued commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Nothing queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Why a command was rejected (shown to the player through the localization key).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ValidationError {
    /// Localization key.
    pub key: &'static str,
    /// Where to highlight.
    pub at: Option<Vec2>,
}

impl ValidationError {
    /// An error without a location.
    #[must_use]
    pub const fn new(key: &'static str) -> Self {
        Self { key, at: None }
    }

    /// An error at a world position.
    #[must_use]
    pub const fn at(key: &'static str, at: Vec2) -> Self {
        Self { key, at: Some(at) }
    }
}

impl From<cb_network::Invalid> for ValidationError {
    fn from(v: cb_network::Invalid) -> Self {
        Self {
            key: v.key(),
            at: v.at(),
        }
    }
}

/// What an applied command did.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    /// Tick at which it was applied.
    pub apply_tick: u64,
    /// Money charged.
    pub cost: Money,
}

/// Outcome of a command.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct CommandResult {
    /// The queued command.
    pub seq: CommandSeq,
    /// Applied or rejected.
    pub outcome: Result<Applied, ValidationError>,
}

/// Applied commands with their tick (replay; versioned with the save format from M15).
#[derive(Resource, Default, Debug, Clone)]
pub struct CommandLog {
    entries: Vec<(u64, PlayerCommand)>,
}

impl CommandLog {
    /// Entries in application order.
    #[must_use]
    pub fn entries(&self) -> &[(u64, PlayerCommand)] {
        &self.entries
    }
}

/// Applies one command to the world.
fn apply(world: &mut World, cmd: &PlayerCommand, tick: u64) -> Result<Money, ValidationError> {
    match cmd {
        PlayerCommand::BuildRoad(b) => crate::roads::build_road(world, b, tick),
        PlayerCommand::BuildRoundabout { center, radius_m } => {
            crate::roads::build_roundabout(world, *center, *radius_m, tick)
        }
        PlayerCommand::RemoveSegment {
            segment,
            remove_plots,
        } => {
            if *remove_plots {
                for p in crate::roads::demolish_dependents(world, *segment).plots {
                    let _ = crate::zoning::delete_plot(world, p);
                }
            }
            crate::roads::remove_segment(world, *segment).map(|()| Money::ZERO)
        }
        PlayerCommand::SetRoadType {
            segment,
            kind,
            variant,
        } => crate::roads::set_road_type(world, *segment, *kind, *variant),
        PlayerCommand::EditRoadGeometry(e) => crate::roads::edit_road_geometry(world, e),
        PlayerCommand::SetSegmentStyle { segment, style } => {
            crate::roads::set_segment_style(world, *segment, *style)
        }
        PlayerCommand::SetApproachControl { node, mode, custom } => {
            crate::roads::set_node_control(world, *node, *mode, custom).map(|()| Money::ZERO)
        }
        PlayerCommand::PlaceBuilding { kind, pos } => {
            crate::buildings::place_civic(world, *kind, *pos)
        }
        PlayerCommand::PlaceProp { kind, pos, yaw } => {
            crate::props::place_prop(world, *kind, *pos, *yaw)
        }
        PlayerCommand::RemoveProp { prop } => crate::props::remove_prop(world, *prop),
        PlayerCommand::CreatePlot(c) => crate::zoning::create_plot(world, c).map(|_| Money::ZERO),
        PlayerCommand::CreatePlotPolygon { zone, vertices, dn } => {
            crate::zoning::create_plot_polygon(world, *zone, vertices, *dn).map(|_| Money::ZERO)
        }
        PlayerCommand::EditPlotPolygon { plot, vertices } => {
            crate::zoning::edit_plot_polygon(world, *plot, vertices).map(|()| Money::ZERO)
        }
        PlayerCommand::EditPlot {
            plot,
            frontage,
            depth_m,
        } => crate::zoning::edit_plot(world, *plot, frontage, *depth_m).map(|()| Money::ZERO),
        PlayerCommand::DeletePlot { plot } => {
            crate::zoning::delete_plot(world, *plot).map(|()| Money::ZERO)
        }
        PlayerCommand::SetSubdivision { plot, dn } => {
            crate::zoning::set_subdivision(world, *plot, *dn).map(|()| Money::ZERO)
        }
        PlayerCommand::SetSimConfig(c) => {
            world.insert_resource(c.clone());
            Ok(Money::ZERO)
        }
        PlayerCommand::SetTaxRate { zone, bp } => {
            crate::economy::set_tax_rate(world, *zone, *bp);
            Ok(Money::ZERO)
        }
        PlayerCommand::TakeLoan { tier } => {
            let day = world
                .get_resource::<SimClock>()
                .map_or(0, |c| c.tick().0 / cb_core::time::TICKS_PER_GAME_DAY);
            crate::economy::take_loan(world, *tier, day).map(|()| Money::ZERO)
        }
        PlayerCommand::RepayLoan { tier } => {
            crate::economy::repay_loan(world, *tier).map(|()| Money::ZERO)
        }
        PlayerCommand::Debug(d) => {
            let ok = match d {
                DebugCommand::SetBuildingLevel { building, level } => {
                    crate::buildings::set_level(world, *building, *level)
                }
                DebugCommand::SetCondition {
                    building,
                    condition,
                } => crate::buildings::set_condition(world, *building, *condition),
            };
            if ok {
                Ok(Money::ZERO)
            } else {
                Err(ValidationError::new("building-invalid-missing"))
            }
        }
        PlayerCommand::Undo => crate::history::undo(world, tick),
        PlayerCommand::Redo => crate::history::redo(world, tick),
    }
}

/// `EditSet::Commands`: drains the queue in order.
pub(crate) fn apply_commands(world: &mut World) {
    let pending = match world.get_resource_mut::<CommandQueue>() {
        Some(mut q) if !q.pending.is_empty() => std::mem::take(&mut q.pending),
        _ => return,
    };
    let tick = world.get_resource::<SimClock>().map_or(0, |c| c.tick().0);
    for (seq, cmd) in pending {
        let label = cmd.undo_label();
        let before = label.map(|_| crate::save::snapshot::capture(world));
        let outcome = apply(world, &cmd, tick).map(|cost| Applied {
            apply_tick: tick,
            cost,
        });
        if let (Some(label), Some(before), Ok(applied)) = (label, before, &outcome) {
            // Plots re-clip in `EditSet::Zoning`; settle them now so the after-state is complete.
            crate::zoning::update_dirty_plots(world);
            let after = crate::save::snapshot::capture(world);
            let (back, forward) = crate::save::snapshot::diff(&before, &after);
            if let Some(mut h) = world.get_resource_mut::<crate::history::History>() {
                h.record(label, back, forward, applied.cost, tick);
            }
        }
        if outcome.is_ok()
            && let Some(mut log) = world.get_resource_mut::<CommandLog>()
        {
            log.entries.push((tick, cmd));
        }
        world.write_message(CommandResult { seq, outcome });
    }
}

/// Every localization key a command can return (checked against all locales by a test).
pub const ALL_KEYS: &[&str] = &[
    "building-invalid-missing",
    "building-invalid-no-road",
    "building-invalid-overlap",
    "building-invalid-shore",
    "building-invalid-steep",
    "building-invalid-water",
    "internal-id-space",
    "internal-no-catalog",
    "internal-no-result",
    "loan-missing",
    "loan-taken",
    "money-bankrupt",
    "money-insufficient",
    "plot-invalid-corner",
    "plot-invalid-frontage",
    "plot-invalid-missing",
    "plot-invalid-no-frontage",
    "plot-invalid-overlap",
    "plot-invalid-self-intersection",
    "plot-invalid-size",
    "prop-invalid-blocked",
    "prop-invalid-missing",
    "prop-invalid-water",
    "redo-empty",
    "road-invalid-bridge-pier-conflict",
    "road-invalid-bridge-too-high",
    "road-invalid-crossing-clearance",
    "road-invalid-edit-crossing",
    "road-invalid-junction-on-bridge",
    "road-invalid-junction-too-close",
    "road-invalid-overpass-unavailable",
    "road-invalid-roundabout-radius",
    "road-invalid-style-no-sidewalk",
    "road-invalid-style-spacing",
    "road-invalid-style-surface",
    "road-invalid-target-missing",
    "road-invalid-too-steep",
    "road-invalid-variant",
    "road-invalid-water-no-bridge",
    "undo-build-road",
    "undo-build-roundabout",
    "undo-create-plot",
    "undo-delete-plot",
    "undo-edit-plot",
    "undo-edit-road",
    "undo-empty",
    "undo-expired",
    "undo-place-building",
    "undo-place-prop",
    "undo-remove-prop",
    "undo-remove-segment",
    "undo-set-control",
    "undo-set-road-type",
    "undo-set-segment-style",
    "undo-set-subdivision",
];

#[cfg(test)]
mod tests {
    #[test]
    fn every_command_key_has_a_message_in_every_locale() {
        let l10n = cb_l10n::L10n::new(cb_l10n::Locale::En).expect("bundles load");
        for key in super::ALL_KEYS {
            for loc in cb_l10n::Locale::ALL {
                assert!(l10n.has(loc, cb_l10n::Key(key)), "{key} missing in {loc:?}");
            }
        }
    }
}
