//! Phase 10: Metrics & Observation Hook for SimulaCiv.
//!
//! Provides deterministic daily macro metrics aggregation:
//! - population (count of living agents)
//! - wealth Gini coefficient (reference integer formulation)
//! - total food reserves (sequential f64 accumulation of living agents' food)
//! - total settlement treasury (checked Money sum in canonical GroupId order)
//!
//! All operations are strictly read-only on `WorldState`, guaranteeing observer independence.

use crate::state::{AgentState, SettlementState, WorldState};
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, Money};
use std::collections::HashSet;

/// Authoritative deterministic daily macro metrics record produced by Phase 10.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DailyMetrics {
    pub day: u32,
    pub population: u64,
    pub wealth_gini: f64,
    pub total_food_reserves: f64,
    pub total_treasury: Money,
}

/// Explicit errors returned during Phase 10 Metrics Observation.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase10Error {
    DuplicateAgent(AgentId),
    DuplicateSettlement(GroupId),
    NonFiniteFood { agent_id: AgentId, food: f32 },
    NegativeFood { agent_id: AgentId, food: f32 },
    NegativeWealth { agent_id: AgentId, wealth: Money },
    NegativeTreasury { group_id: GroupId, treasury: Money },
    ArithmeticOverflow,
    InvalidGini(f64),
    InvariantViolation(String),
}

impl std::fmt::Display for Phase10Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateAgent(aid) => write!(f, "duplicate agent ID in world state: {}", aid),
            Self::DuplicateSettlement(gid) => {
                write!(f, "duplicate settlement group ID in world state: {}", gid)
            }
            Self::NonFiniteFood { agent_id, food } => {
                write!(f, "non-finite food for agent {}: {}", agent_id, food)
            }
            Self::NegativeFood { agent_id, food } => {
                write!(f, "negative food for agent {}: {}", agent_id, food)
            }
            Self::NegativeWealth { agent_id, wealth } => {
                write!(f, "negative wealth for agent {}: {}", agent_id, wealth)
            }
            Self::NegativeTreasury { group_id, treasury } => {
                write!(f, "negative treasury for group {}: {}", group_id, treasury)
            }
            Self::ArithmeticOverflow => write!(f, "arithmetic overflow in Phase 10 metrics"),
            Self::InvalidGini(val) => write!(f, "invalid wealth Gini value: {}", val),
            Self::InvariantViolation(msg) => write!(f, "phase 10 invariant violation: {}", msg),
        }
    }
}

impl std::error::Error for Phase10Error {}

/// Executes Phase 10: Metrics & Observation Hook on authoritative world state.
///
/// Behavior:
/// 1. Validate world state integrity without mutating any state:
///    - Unique `AgentId`s.
///    - Finite, non-negative food for all agents.
///    - Non-negative wealth for all agents.
///    - Unique `GroupId`s for all settlements.
///    - Non-negative treasury for all settlements.
/// 2. Canonical observation population:
///    - Filter living agents (`alive == true`).
///    - Sort strictly by ascending stable `AgentId`.
/// 3. Compute population:
///    - `population = count of living agents as u64`.
/// 4. Compute total food reserves:
///    - Sequentially accumulate `alive_agent.food as f64` in ascending `AgentId` order.
/// 5. Compute total treasury:
///    - Traverse settlements in strictly ascending `GroupId` order.
///    - Accumulate each settlement's treasury using checked `Money` addition.
/// 6. Compute wealth Gini:
///    - Using the exact M0 reference integer formula on alive agents' wealth.
/// 7. Return `DailyMetrics`.
pub fn phase10_observe(world: &WorldState, day: u32) -> Result<DailyMetrics, Phase10Error> {
    // 1. Structural validation
    let mut seen_agents = HashSet::with_capacity(world.agents.len());
    for agent in &world.agents {
        if !seen_agents.insert(agent.agent_id) {
            return Err(Phase10Error::DuplicateAgent(agent.agent_id));
        }
        if !agent.food.is_finite() {
            return Err(Phase10Error::NonFiniteFood {
                agent_id: agent.agent_id,
                food: agent.food,
            });
        }
        if agent.food < 0.0 {
            return Err(Phase10Error::NegativeFood {
                agent_id: agent.agent_id,
                food: agent.food,
            });
        }
        if agent.wealth < 0 {
            return Err(Phase10Error::NegativeWealth {
                agent_id: agent.agent_id,
                wealth: agent.wealth,
            });
        }
    }

    let mut seen_groups = HashSet::with_capacity(world.settlements.len());
    for settlement in &world.settlements {
        if !seen_groups.insert(settlement.group_id) {
            return Err(Phase10Error::DuplicateSettlement(settlement.group_id));
        }
        if settlement.treasury < 0 {
            return Err(Phase10Error::NegativeTreasury {
                group_id: settlement.group_id,
                treasury: settlement.treasury,
            });
        }
    }

    // 2. Canonical living agents: ascending AgentId
    let mut alive_agents: Vec<&AgentState> = Vec::with_capacity(world.agents.len());
    for a in &world.agents {
        if a.alive {
            alive_agents.push(a);
        }
    }
    alive_agents.sort_by_key(|a| a.agent_id);

    // 3. Population: u64 count of alive agents
    let population = alive_agents.len() as u64;

    // 4. Total food reserves: sequential f64 sum in ascending AgentId order
    let mut total_food_reserves = 0.0_f64;
    for a in &alive_agents {
        total_food_reserves += a.food as f64;
    }
    if !total_food_reserves.is_finite() {
        return Err(Phase10Error::InvariantViolation(
            "total food reserves non-finite".into(),
        ));
    }

    // 5. Total treasury: ascending GroupId checked Money addition
    let mut sorted_settlements: Vec<&SettlementState> = Vec::with_capacity(world.settlements.len());
    sorted_settlements.extend(world.settlements.iter());
    sorted_settlements.sort_by_key(|s| s.group_id);

    let mut total_treasury: Money = 0;
    for s in &sorted_settlements {
        total_treasury = total_treasury
            .checked_add(s.treasury)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
    }

    // 6. Wealth Gini
    let wealth_gini = compute_wealth_gini(&mut alive_agents)?;

    Ok(DailyMetrics {
        day,
        population,
        wealth_gini,
        total_food_reserves,
        total_treasury,
    })
}

/// Convenience alias for [`phase10_observe`].
pub use phase10_observe as phase10_metrics_observation;

/// Convenience alias for [`phase10_observe`].
pub use phase10_observe as phase10_metrics;

/// Convenience wrapper executing Phase 10 observation with [`crate::config::SimConfig`].
pub fn phase10_observe_with_config(
    world: &WorldState,
    _config: &crate::config::SimConfig,
) -> Result<DailyMetrics, Phase10Error> {
    phase10_observe(world, world.current_day.0)
}

/// Computes the wealth Gini coefficient for alive agents according to the exact M0 reference semantics.
fn compute_wealth_gini(alive_agents: &mut [&AgentState]) -> Result<f64, Phase10Error> {
    let n = alive_agents.len() as u128;
    if n == 0 {
        return Ok(0.0);
    }

    let mut sum_x: u128 = 0;
    for a in alive_agents.iter() {
        sum_x = sum_x
            .checked_add(a.wealth as u128)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
    }

    if sum_x == 0 {
        return Ok(0.0);
    }

    // Sort by (wealth ascending, AgentId ascending) in place without allocation
    alive_agents.sort_by_key(|a| (a.wealth, a.agent_id));

    let mut weighted_sum: u128 = 0;
    for (idx, a) in alive_agents.iter().enumerate() {
        let rank = (idx as u128) + 1; // 1-indexed: 1..=n
        let x_i = a.wealth as u128;
        let term = rank
            .checked_mul(x_i)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
        weighted_sum = weighted_sum
            .checked_add(term)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
    }

    let two_w = weighted_sum
        .checked_mul(2)
        .ok_or(Phase10Error::ArithmeticOverflow)?;
    let n_plus_one_s = (n + 1)
        .checked_mul(sum_x)
        .ok_or(Phase10Error::ArithmeticOverflow)?;

    if two_w < n_plus_one_s {
        return Err(Phase10Error::InvariantViolation(
            "2*W < (n+1)*S in Gini calculation".into(),
        ));
    }

    let numerator = two_w - n_plus_one_s;
    let denominator = n
        .checked_mul(sum_x)
        .ok_or(Phase10Error::ArithmeticOverflow)?;

    let gini = (numerator as f64) / (denominator as f64);
    if !gini.is_finite() || !(0.0..=1.0).contains(&gini) {
        return Err(Phase10Error::InvalidGini(gini));
    }

    Ok(gini)
}
