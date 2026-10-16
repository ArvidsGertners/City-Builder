//! Player commands (ADR-0055): the only way presentation code changes the world. Commands queue
//! in [`CommandQueue`], are applied in `EditSet::Commands` in queue order, record their
//! `apply_tick`, and answer with a [`CommandResult`] message. Validation failures are values with
//! localization keys, never log errors.

use bevy::ecs::message::Message;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::ids::{NodeId, PlotId, SegmentId};
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

/// All player commands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlayerCommand {
    /// Build a road.
    BuildRoad(BuildRoad),
    /// Bulldoze a road segment.
    RemoveSegment {
        /// The segment.
        segment: SegmentId,
    },
    /// Upgrade/downgrade: change a segment's road type and variant (bus lanes, tram tracks).
    SetRoadType {
        /// The segment.
        segment: SegmentId,
        /// New road type.
        kind: RoadKind,
        /// New variant.
        variant: cb_network::Variant,
    },
    /// Draw a plot.
    CreatePlot(CreatePlot),
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
        PlayerCommand::RemoveSegment { segment } => {
            crate::roads::remove_segment(world, *segment).map(|()| Money::ZERO)
        }
        PlayerCommand::SetRoadType {
            segment,
            kind,
            variant,
        } => crate::roads::set_road_type(world, *segment, *kind, *variant),
        PlayerCommand::CreatePlot(c) => crate::zoning::create_plot(world, c).map(|_| Money::ZERO),
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
        let outcome = apply(world, &cmd, tick).map(|cost| Applied {
            apply_tick: tick,
            cost,
        });
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
    "internal-id-space",
    "internal-no-catalog",
    "internal-no-result",
    "money-insufficient",
    "plot-invalid-corner",
    "plot-invalid-frontage",
    "plot-invalid-missing",
    "plot-invalid-overlap",
    "plot-invalid-size",
    "road-invalid-junction-too-close",
    "road-invalid-overpass-unavailable",
    "road-invalid-target-missing",
    "road-invalid-too-steep",
    "road-invalid-variant",
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
