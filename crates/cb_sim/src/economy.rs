//! Economy stub for M2 (GAME_DESIGN §7): the treasury and construction charges. Accrual, taxes,
//! upkeep and loans arrive in M5.

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::Money;

use crate::commands::ValidationError;
use crate::config::SimConfig;

/// The city's money.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Treasury {
    pub(crate) balance: Money,
    pub(crate) spent_construction: Money,
}

impl Treasury {
    /// A treasury with a starting balance.
    #[must_use]
    pub fn new(balance: Money) -> Self {
        Self {
            balance,
            spent_construction: Money::ZERO,
        }
    }

    /// Current balance.
    #[must_use]
    pub fn balance(&self) -> Money {
        self.balance
    }

    /// Total construction spending.
    #[must_use]
    pub fn spent_construction(&self) -> Money {
        self.spent_construction
    }
}

/// Whether `cost` is affordable (always in sandbox).
#[must_use]
pub fn can_afford(world: &World, cost: Money) -> bool {
    let sandbox = world.get_resource::<SimConfig>().is_some_and(|c| c.sandbox);
    sandbox
        || world
            .get_resource::<Treasury>()
            .is_some_and(|t| t.balance.0 >= cost.0)
}

/// Charges a construction cost.
///
/// # Errors
/// `money-insufficient` when the balance does not cover it (outside sandbox).
pub(crate) fn charge(world: &mut World, cost: Money) -> Result<(), ValidationError> {
    if !can_afford(world, cost) {
        return Err(ValidationError::new("money-insufficient"));
    }
    let sandbox = world.get_resource::<SimConfig>().is_some_and(|c| c.sandbox);
    if let Some(mut t) = world.get_resource_mut::<Treasury>() {
        if !sandbox {
            t.balance = Money(t.balance.0 - cost.0);
        }
        t.spent_construction = t.spent_construction.saturating_add(cost);
    }
    Ok(())
}
