//! Economy (GAME_DESIGN §9, ALGORITHMS §21, §27): the treasury, a ledger of every money flow
//! by account, hourly accrual at tick 0 of the game hour (household and business taxes, road
//! and building upkeep, loan interest), the daily statement, emergency loans and bankruptcy.
//!
//! Accruals are computed in millicents and settled in whole cents; the remainder is carried
//! per account, so hourly amounts add up exactly over a day. Every change of the balance goes
//! through [`post`], which is why the ledger always sums to the balance delta.

pub mod goods;

use std::collections::VecDeque;

use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use cb_core::Money;
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use serde::{Deserialize, Serialize};

use crate::buildings::{Building, BuildingKind};
use crate::catalog::{Catalog, ZoneKind};
use crate::commands::ValidationError;
use crate::config::SimConfig;
use crate::ids::Ids;
use crate::roads::RoadSegment;

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

/// Ledger accounts (positive = income).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Account {
    /// Household taxes.
    TaxResidential,
    /// Commercial profit taxes.
    TaxCommercial,
    /// Industrial profit taxes.
    TaxIndustrial,
    /// Road upkeep.
    RoadUpkeep,
    /// Civic building upkeep.
    BuildingUpkeep,
    /// Construction charges.
    Construction,
    /// Undo and demolition refunds.
    Refunds,
    /// Loan interest.
    LoanInterest,
    /// Loan principal received.
    LoanProceeds,
    /// Loan principal repaid.
    LoanRepayment,
}

impl Account {
    /// Every account, in ledger order.
    pub const ALL: [Self; 10] = [
        Self::TaxResidential,
        Self::TaxCommercial,
        Self::TaxIndustrial,
        Self::RoadUpkeep,
        Self::BuildingUpkeep,
        Self::Construction,
        Self::Refunds,
        Self::LoanInterest,
        Self::LoanProceeds,
        Self::LoanRepayment,
    ];
    const N: usize = Self::ALL.len();

    const fn index(self) -> usize {
        self as usize
    }
}

/// One game day of the ledger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// Game day (0-based) the statement covers.
    pub day: u64,
    /// Cents per account.
    pub by_account: [i64; Account::N],
    /// Balance at the end of the day (cents).
    pub closing_balance: i64,
}

impl Statement {
    /// Net flow of the day.
    #[must_use]
    pub fn net(&self) -> i64 {
        self.by_account.iter().sum()
    }
}

/// Days of statements kept.
pub const HISTORY_DAYS: usize = 30;

/// Every money flow by account (saved).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    pub(crate) today: [i64; Account::N],
    pub(crate) total: [i64; Account::N],
    /// Millicents accrued but not yet settled, per account.
    pub(crate) residual_mc: [i64; Account::N],
    pub(crate) history: VecDeque<Statement>,
}

impl Ledger {
    /// Cents posted to `account` today.
    #[must_use]
    pub fn today(&self, account: Account) -> i64 {
        self.today[account.index()]
    }

    /// Cents posted to `account` since the start.
    #[must_use]
    pub fn total(&self, account: Account) -> i64 {
        self.total[account.index()]
    }

    /// Sum of every account since the start.
    #[must_use]
    pub fn total_net(&self) -> i64 {
        self.total.iter().sum()
    }

    /// Past daily statements, oldest first.
    pub fn history(&self) -> impl Iterator<Item = &Statement> {
        self.history.iter()
    }
}

/// An outstanding loan.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loan {
    /// Tier index (0–2).
    pub tier: u8,
    /// Principal (cents).
    pub principal_cents: i64,
    /// Simple interest per game day.
    pub rate_per_day: f32,
    /// Day taken.
    pub day: u64,
}

/// Taxes, loans and the bankruptcy state (saved).
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Finances {
    /// Tax rates R, C, I (basis points).
    pub(crate) tax_bp: [u16; 3],
    pub(crate) loans: Vec<Loan>,
    /// Consecutive days that ended below zero.
    pub(crate) days_negative: u32,
    /// Forced 50 % budgets and construction freeze.
    pub(crate) bankrupt: bool,
    /// The balance is projected below zero within 3 days.
    pub(crate) warning: bool,
}

impl Default for Finances {
    fn default() -> Self {
        Self {
            tax_bp: [900; 3],
            loans: Vec::new(),
            days_negative: 0,
            bankrupt: false,
            warning: false,
        }
    }
}

impl Finances {
    /// Tax rate of a zone (basis points).
    #[must_use]
    pub fn tax_bp(&self, zone: ZoneKind) -> u16 {
        self.tax_bp[zone_index(zone)]
    }

    /// Outstanding loans.
    #[must_use]
    pub fn loans(&self) -> &[Loan] {
        &self.loans
    }

    /// Consecutive days below zero.
    #[must_use]
    pub fn days_negative(&self) -> u32 {
        self.days_negative
    }

    /// Bankrupt (budgets forced to 50 %, construction frozen).
    #[must_use]
    pub fn bankrupt(&self) -> bool {
        self.bankrupt
    }

    /// The balance is projected below zero within 3 days.
    #[must_use]
    pub fn warning(&self) -> bool {
        self.warning
    }
}

fn zone_index(z: ZoneKind) -> usize {
    match z {
        ZoneKind::Residential => 0,
        ZoneKind::Commercial => 1,
        ZoneKind::Industrial => 2,
    }
}

fn sandbox(world: &World) -> bool {
    world.get_resource::<SimConfig>().is_some_and(|c| c.sandbox)
}

/// Books `cents` to `account` and moves the balance (outside sandbox).
pub(crate) fn post(world: &mut World, account: Account, cents: i64) {
    if cents == 0 {
        return;
    }
    let sb = sandbox(world);
    if let Some(mut l) = world.get_resource_mut::<Ledger>() {
        l.today[account.index()] += cents;
        l.total[account.index()] += cents;
    }
    if !sb && let Some(mut t) = world.get_resource_mut::<Treasury>() {
        t.balance = Money(t.balance.0 + cents);
    }
}

/// Adds accrued millicents to an account and posts the whole cents (remainder carried).
pub(crate) fn accrue(world: &mut World, account: Account, millicents: i64) {
    let cents = {
        let Some(mut l) = world.get_resource_mut::<Ledger>() else {
            return;
        };
        let r = &mut l.residual_mc[account.index()];
        *r += millicents;
        let whole = *r / 1000;
        *r -= whole * 1000;
        whole
    };
    post(world, account, cents);
}

/// Whether `cost` is affordable (always in sandbox).
#[must_use]
pub fn can_afford(world: &World, cost: Money) -> bool {
    sandbox(world)
        || world
            .get_resource::<Treasury>()
            .is_some_and(|t| t.balance.0 >= cost.0)
}

/// Charges a construction cost.
///
/// # Errors
/// `money-insufficient` when the balance does not cover it (outside sandbox) and
/// `money-bankrupt` while construction is frozen.
pub(crate) fn charge(world: &mut World, cost: Money) -> Result<(), ValidationError> {
    if cost.0 > 0 && !sandbox(world) && world.get_resource::<Finances>().is_some_and(|f| f.bankrupt)
    {
        return Err(ValidationError::new("money-bankrupt"));
    }
    if !can_afford(world, cost) {
        return Err(ValidationError::new("money-insufficient"));
    }
    post(world, Account::Construction, -cost.0);
    if let Some(mut t) = world.get_resource_mut::<Treasury>() {
        t.spent_construction = t.spent_construction.saturating_add(cost);
    }
    Ok(())
}

/// Credits a refund (undo, ADR-0055).
pub(crate) fn refund(world: &mut World, amount: Money) {
    post(world, Account::Refunds, amount.0);
    if let Some(mut t) = world.get_resource_mut::<Treasury>() {
        t.spent_construction = Money(t.spent_construction.0 - amount.0);
    }
}

/// Tick of the accrual in the game hour (phase table).
pub const OFFSET: u64 = 0;

/// Household tax per game hour in millicents: `income(level, wealth) × rate / 24`.
#[must_use]
pub fn household_tax_mc(
    spec: &crate::catalog::EconomySpec,
    level: u8,
    wealth: crate::citizens::Wealth,
    tax_bp: u16,
) -> i64 {
    let income = spec.household_income_cents_per_day[usize::from(level.clamp(1, 5)) - 1] as f32
        * spec.wealth_income_factor[wealth.index()];
    // cents/day × bp / 10 000 / 24 × 1000 = cents × bp / 240 millicents per hour.
    (income * f32::from(tax_bp) / 240.0).round() as i64
}

/// The hourly accrual.
fn accrue_hour(world: &mut World) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.economy()) else {
        return;
    };
    let fin = world
        .get_resource::<Finances>()
        .cloned()
        .unwrap_or_default();
    // Household taxes.
    let mut tax_r = 0i64;
    if let (Some(pop), Some(cap)) = (
        world.get_resource::<crate::citizens::Population>(),
        world.get_resource::<crate::citizens::Capacities>(),
    ) {
        for (_, h) in pop.households() {
            let level = cap.level(h.home()).unwrap_or(1);
            tax_r += household_tax_mc(&spec, level, h.wealth(), fin.tax_bp[0]);
        }
    }
    // Business profit taxes (from the goods model's last hour).
    let (profit_c, profit_i) = world
        .get_resource::<goods::Goods>()
        .map_or((0, 0), goods::Goods::taxable_profit_cents);
    let tax_c = profit_c * i64::from(fin.tax_bp[1]) / 10;
    let tax_i = profit_i * i64::from(fin.tax_bp[2]) / 10;
    // Road and building upkeep.
    let mut roads_mc = 0.0f64;
    let mut buildings_mc = 0i64;
    if let (Some(ids), Some(cat)) = (world.get_resource::<Ids>(), world.get_resource::<Catalog>()) {
        for (_, e) in ids.segments.iter() {
            if let Some(s) = world.get::<RoadSegment>(e) {
                let per_day_units =
                    f64::from(s.geom().length()) * f64::from(cat.road(s.kind()).upkeep_per_m_day);
                roads_mc += per_day_units * 100.0 * 1000.0 / 24.0;
            }
        }
        for (_, e) in ids.buildings.iter() {
            if let Some(b) = world.get::<Building>(e)
                && let BuildingKind::Civic(k) = b.kind()
            {
                buildings_mc += cat.civic(k).upkeep_cents_per_day * 1000 / 24;
            }
        }
    }
    let budget = if fin.bankrupt { 0.5 } else { 1.0 };
    // Loan interest.
    let interest_mc: f64 = fin
        .loans
        .iter()
        .map(|l| l.principal_cents as f64 * f64::from(l.rate_per_day) * 1000.0 / 24.0)
        .sum();
    accrue(world, Account::TaxResidential, tax_r);
    accrue(world, Account::TaxCommercial, tax_c);
    accrue(world, Account::TaxIndustrial, tax_i);
    accrue(world, Account::RoadUpkeep, -(roads_mc.round() as i64));
    accrue(
        world,
        Account::BuildingUpkeep,
        -((buildings_mc as f64 * budget).round() as i64),
    );
    accrue(world, Account::LoanInterest, -(interest_mc.round() as i64));
}

/// Closes the day: statement, projection warning, emergency loan and bankruptcy.
fn close_day(world: &mut World, day: u64) {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.economy()) else {
        return;
    };
    let balance = world.get_resource::<Treasury>().map_or(0, |t| t.balance.0);
    let statement = {
        let Some(mut l) = world.get_resource_mut::<Ledger>() else {
            return;
        };
        let st = Statement {
            day,
            by_account: l.today,
            closing_balance: balance,
        };
        l.today = [0; Account::N];
        l.history.push_back(st);
        while l.history.len() > HISTORY_DAYS {
            l.history.pop_front();
        }
        st
    };
    if sandbox(world) {
        return;
    }
    let mut take_loan: Option<(u8, i64, f32)> = None;
    if let Some(mut f) = world.get_resource_mut::<Finances>() {
        f.warning = balance + 3 * statement.net() < 0;
        if balance < 0 {
            f.days_negative += 1;
            if f.days_negative >= spec.bankruptcy_days {
                let free = (0..3u8).find(|t| f.loans.iter().all(|l| l.tier != *t));
                if let Some(t) = free {
                    let tier = spec.loans[usize::from(t)];
                    take_loan = Some((t, tier.principal_cents, tier.rate_per_day));
                    f.days_negative = 0;
                } else {
                    f.bankrupt = true;
                }
            }
        } else {
            f.days_negative = 0;
            f.bankrupt = false;
        }
        if let Some((tier, principal_cents, rate_per_day)) = take_loan {
            f.loans.push(Loan {
                tier,
                principal_cents,
                rate_per_day,
                day,
            });
        }
    }
    if let Some((_, principal, _)) = take_loan {
        post(world, Account::LoanProceeds, principal);
    }
}

/// Takes a loan of `tier` (≤ 1 per tier).
///
/// # Errors
/// `loan-taken` when the tier already has a loan.
pub(crate) fn take_loan(world: &mut World, tier: u8, day: u64) -> Result<(), ValidationError> {
    let Some(spec) = world.get_resource::<Catalog>().map(|c| *c.economy()) else {
        return Err(ValidationError::new("loan-taken"));
    };
    let t = spec.loans[usize::from(tier.min(2))];
    {
        let Some(mut f) = world.get_resource_mut::<Finances>() else {
            return Err(ValidationError::new("loan-taken"));
        };
        if f.loans.iter().any(|l| l.tier == tier) {
            return Err(ValidationError::new("loan-taken"));
        }
        f.loans.push(Loan {
            tier,
            principal_cents: t.principal_cents,
            rate_per_day: t.rate_per_day,
            day,
        });
    }
    post(world, Account::LoanProceeds, t.principal_cents);
    Ok(())
}

/// Repays the loan of `tier` in full.
///
/// # Errors
/// `loan-missing` without such a loan, `money-insufficient` when the balance does not cover it.
pub(crate) fn repay_loan(world: &mut World, tier: u8) -> Result<(), ValidationError> {
    let principal = world
        .get_resource::<Finances>()
        .and_then(|f| f.loans.iter().find(|l| l.tier == tier))
        .map(|l| l.principal_cents)
        .ok_or_else(|| ValidationError::new("loan-missing"))?;
    if !can_afford(world, Money(principal)) {
        return Err(ValidationError::new("money-insufficient"));
    }
    if let Some(mut f) = world.get_resource_mut::<Finances>() {
        f.loans.retain(|l| l.tier != tier);
    }
    post(world, Account::LoanRepayment, -principal);
    Ok(())
}

/// Sets a zone's tax rate (clamped to 100–2 500 basis points).
pub(crate) fn set_tax_rate(world: &mut World, zone: ZoneKind, bp: u16) {
    if let Some(mut f) = world.get_resource_mut::<Finances>() {
        f.tax_bp[zone_index(zone)] = bp.clamp(100, 2500);
    }
}

/// `TickSet::SubRate`: hourly accrual and the end-of-day close.
pub(crate) fn tick(world: &mut World) {
    let now = world
        .get_resource::<crate::SimClock>()
        .map_or(0, |c| c.tick().0);
    if now % TICKS_PER_GAME_HOUR != OFFSET {
        return;
    }
    goods::hour(world, now);
    accrue_hour(world);
    if now > 0 && now.is_multiple_of(TICKS_PER_GAME_DAY) {
        close_day(world, now / TICKS_PER_GAME_DAY - 1);
    }
}
