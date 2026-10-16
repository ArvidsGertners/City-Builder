//! Undo/redo history (ADR-0055, ARCHITECTURE §9). Every undoable command stores the object
//! deltas back to the state before it and forward to the state after it (stable ids, see
//! `save::snapshot`), its cost and its apply tick. Undo restores the before-state and refunds
//! 100 % within the first game hour, then linearly less down to 25 % at one game day, after
//! which the entry has expired. Redo restores the after-state and charges the full cost again.
//! Depth 100; a new command clears the redo stack.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};

use crate::commands::ValidationError;
use crate::save::snapshot::{self, Delta};

/// Maximum number of undo entries.
pub const UNDO_DEPTH: usize = 100;
/// Refund at the end of the undo window (the demolish refund).
pub const MIN_REFUND_PERMILLE: i64 = 250;

/// One undoable step.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Localization key of the action's label.
    pub label: &'static str,
    back: Delta,
    forward: Delta,
    /// What the action cost.
    pub cost: Money,
    /// Tick it was (re)applied.
    pub apply_tick: u64,
}

/// Undo and redo stacks (not saved; cleared on load, ADR-0055).
#[derive(Resource, Clone, Debug, Default)]
pub struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
}

impl History {
    /// Undo entries, oldest first.
    #[must_use]
    pub fn undo_entries(&self) -> &[Entry] {
        &self.undo
    }

    /// Redo entries, oldest first.
    #[must_use]
    pub fn redo_entries(&self) -> &[Entry] {
        &self.redo
    }

    /// Records an applied command (no entry when it changed no object).
    pub(crate) fn record(
        &mut self,
        label: &'static str,
        back: Delta,
        forward: Delta,
        cost: Money,
        tick: u64,
    ) {
        if back.is_empty() && forward.is_empty() {
            return;
        }
        self.redo.clear();
        self.undo.push(Entry {
            label,
            back,
            forward,
            cost,
            apply_tick: tick,
        });
        if self.undo.len() > UNDO_DEPTH {
            let extra = self.undo.len() - UNDO_DEPTH;
            self.undo.drain(..extra);
        }
    }
}

/// Refund of undoing an action that cost `cost`, `age_ticks` after it was applied (`None` once
/// the undo window has passed).
#[must_use]
pub fn refund_for(cost: Money, age_ticks: u64) -> Option<Money> {
    if age_ticks > TICKS_PER_GAME_DAY {
        return None;
    }
    let permille = if age_ticks <= TICKS_PER_GAME_HOUR {
        1000
    } else {
        let span = (TICKS_PER_GAME_DAY - TICKS_PER_GAME_HOUR) as i64;
        let into = (age_ticks - TICKS_PER_GAME_HOUR) as i64;
        1000 - (1000 - MIN_REFUND_PERMILLE) * into / span
    };
    Some(Money(cost.0 * permille / 1000))
}

/// `Undo`: restores the state before the most recent undoable action. Returns the refund as a
/// negative cost.
///
/// # Errors
/// `undo-empty` with nothing to undo; `undo-expired` when the action is older than a game day
/// (expired entries are dropped).
pub(crate) fn undo(world: &mut World, tick: u64) -> Result<Money, ValidationError> {
    let Some(entry) = world
        .get_resource_mut::<History>()
        .and_then(|mut h| h.undo.pop())
    else {
        return Err(ValidationError::new("undo-empty"));
    };
    let Some(refund) = refund_for(entry.cost, tick.saturating_sub(entry.apply_tick)) else {
        // This and every older entry are out of the window.
        if let Some(mut h) = world.get_resource_mut::<History>() {
            h.undo.clear();
        }
        return Err(ValidationError::new("undo-expired"));
    };
    snapshot::apply(world, &entry.back);
    // The restored objects are authoritative: no re-clipping of their plots.
    if let Some(mut d) = world.get_resource_mut::<crate::zoning::DirtyPlots>() {
        d.0.clear();
    }
    crate::economy::refund(world, refund);
    if let Some(mut h) = world.get_resource_mut::<History>() {
        h.redo.push(entry);
    }
    Ok(Money(-refund.0))
}

/// `Redo`: re-applies the most recently undone action with its original ids and full cost.
///
/// # Errors
/// `redo-empty` with nothing to redo; `money-insufficient` (the entry stays on the redo stack).
pub(crate) fn redo(world: &mut World, tick: u64) -> Result<Money, ValidationError> {
    let Some(mut entry) = world
        .get_resource_mut::<History>()
        .and_then(|mut h| h.redo.pop())
    else {
        return Err(ValidationError::new("redo-empty"));
    };
    if let Err(e) = crate::economy::charge(world, entry.cost) {
        if let Some(mut h) = world.get_resource_mut::<History>() {
            h.redo.push(entry);
        }
        return Err(e);
    }
    snapshot::apply(world, &entry.forward);
    // The restored objects are authoritative: no re-clipping of their plots.
    if let Some(mut d) = world.get_resource_mut::<crate::zoning::DirtyPlots>() {
        d.0.clear();
    }
    entry.apply_tick = tick;
    let cost = entry.cost;
    if let Some(mut h) = world.get_resource_mut::<History>() {
        h.undo.push(entry);
    }
    Ok(cost)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_refund_decays() {
        let c = Money(10_000);
        assert_eq!(refund_for(c, 0), Some(c));
        assert_eq!(refund_for(c, TICKS_PER_GAME_HOUR), Some(c));
        let mid = TICKS_PER_GAME_HOUR + (TICKS_PER_GAME_DAY - TICKS_PER_GAME_HOUR) / 2;
        assert_eq!(refund_for(c, mid), Some(Money(6_250)));
        assert_eq!(refund_for(c, TICKS_PER_GAME_DAY), Some(Money(2_500)));
        assert_eq!(refund_for(c, TICKS_PER_GAME_DAY + 1), None);
        // Monotone.
        let mut last = c.0;
        for t in (0..=TICKS_PER_GAME_DAY).step_by(500) {
            let r = refund_for(c, t).unwrap().0;
            assert!(r <= last);
            last = r;
        }
    }
}
