//! Phase 10: Metrics & Observation Hook for SimulaCiv.
//!
//! Provides deterministic daily macro metrics aggregation:
//! - population (count of living agents)
//! - wealth Gini coefficient (reference integer formulation)
//! - total food reserves (sequential f64 accumulation of living agents' food)
//! - total settlement treasury (checked Money sum in canonical GroupId order)
//!
//! All operations are strictly read-only on `WorldState`, guaranteeing observer independence.

use crate::state::{AgentDynamicSoAScratch, AgentDynamicState, AgentState, WorldState};
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

/// Executes Phase 10: Macroscopic Metrics Observation reusing an external [`AgentDynamicSoAScratch`] buffer.
///
/// Behavior:
/// 1. Validate world state integrity without mutating any state:
///    - Unique `AgentId`s.
///    - Finite, non-negative food for all agents.
///    - Non-negative wealth for all agents.
///    - Unique `GroupId`s for all settlements.
///    - Non-negative treasury for all settlements.
/// 2. Populate SoA scratch buffer (`health`, `food`, `wealth`, `alive`, `agent_ids`).
/// 3. Compute population: count living agents in contiguous `scratch.alive`.
/// 4. Canonical living agent indices: populate `scratch.indices` and ensure ascending `AgentId` order.
/// 5. Compute total food reserves:
///    - Fast-path contiguous streaming over `scratch.food` when all agents are alive and sorted.
///    - Canonical index traversal over `scratch.indices` otherwise.
/// 6. Compute total treasury: ascending `GroupId` checked `Money` addition.
/// 7. Compute wealth Gini: zero-allocation in-place sorting on `scratch.indices` via [`compute_wealth_gini_soa`].
/// 8. Return `DailyMetrics`.
pub fn phase10_observe_with_scratch(
    world: &WorldState,
    day: u32,
    scratch: &mut AgentDynamicSoAScratch,
) -> Result<DailyMetrics, Phase10Error> {
    // 1. Structural validation of agents
    let n = world.agents.len();
    let mut seen_set = if n > 32 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    for (i, agent) in world.agents.iter().enumerate() {
        let duplicate = if let Some(ref mut set) = seen_set {
            !set.insert(agent.agent_id)
        } else {
            world.agents[..i]
                .iter()
                .any(|a| a.agent_id == agent.agent_id)
        };
        if duplicate {
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

    // 2. Structural validation of settlements
    for (i, settlement) in world.settlements.iter().enumerate() {
        if world.settlements[..i]
            .iter()
            .any(|s| s.group_id == settlement.group_id)
        {
            return Err(Phase10Error::DuplicateSettlement(settlement.group_id));
        }
        if settlement.treasury < 0 {
            return Err(Phase10Error::NegativeTreasury {
                group_id: settlement.group_id,
                treasury: settlement.treasury,
            });
        }
    }

    // 3. Populate SoA scratch buffer with hot dynamic state
    scratch.collect_from_agents(&world.agents);

    // 4. Population: count living agents via contiguous bool vector
    let population = scratch.alive.iter().filter(|&&a| a).count() as u64;

    // 5. Canonical living agent indices
    scratch.indices.clear();
    for (i, &is_alive) in scratch.alive.iter().enumerate() {
        if is_alive {
            scratch.indices.push(i);
        }
    }

    // Ensure ascending AgentId canonical order for living agents
    let is_sorted = scratch
        .indices
        .windows(2)
        .all(|w| scratch.agent_ids[w[0]] <= scratch.agent_ids[w[1]]);
    if !is_sorted {
        scratch
            .indices
            .sort_unstable_by_key(|&i| scratch.agent_ids[i]);
    }

    // 6. Total food reserves: sequential f64 accumulation in ascending AgentId order
    let mut total_food_reserves = 0.0_f64;
    if is_sorted && population as usize == scratch.food.len() {
        // Fast path: all agents living and strictly sorted - contiguous streaming
        for &f in &scratch.food {
            total_food_reserves += f as f64;
        }
    } else {
        // Canonical sorted index traversal
        for &idx in &scratch.indices {
            total_food_reserves += scratch.food[idx] as f64;
        }
    }
    if !total_food_reserves.is_finite() {
        return Err(Phase10Error::InvariantViolation(
            "total food reserves non-finite".into(),
        ));
    }

    // 7. Total treasury: ascending GroupId checked Money addition
    let settlements_sorted = world
        .settlements
        .windows(2)
        .all(|w| w[0].group_id <= w[1].group_id);

    let mut total_treasury: Money = 0;
    if settlements_sorted {
        for s in &world.settlements {
            total_treasury = total_treasury
                .checked_add(s.treasury)
                .ok_or(Phase10Error::ArithmeticOverflow)?;
        }
    } else {
        scratch.settlement_indices.clear();
        scratch
            .settlement_indices
            .extend(0..world.settlements.len());
        scratch
            .settlement_indices
            .sort_unstable_by_key(|&i| world.settlements[i].group_id);
        for &idx in &scratch.settlement_indices {
            total_treasury = total_treasury
                .checked_add(world.settlements[idx].treasury)
                .ok_or(Phase10Error::ArithmeticOverflow)?;
        }
    }

    // 8. Wealth Gini via SoA index-based sorting
    let wealth_gini = compute_wealth_gini_soa(scratch)?;

    Ok(DailyMetrics {
        day,
        population,
        wealth_gini,
        total_food_reserves,
        total_treasury,
    })
}

/// Executes Phase 10: Metrics & Observation Hook on authoritative world state.
///
/// Allocates a local [`AgentDynamicSoAScratch`] buffer and delegates to [`phase10_observe_with_scratch`].
pub fn phase10_observe(world: &WorldState, day: u32) -> Result<DailyMetrics, Phase10Error> {
    let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
    phase10_observe_with_scratch(world, day, &mut scratch)
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
pub fn compute_wealth_gini(alive_agents: &mut [&AgentState]) -> Result<f64, Phase10Error> {
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

/// Computes the wealth Gini coefficient for alive agents from their compact dynamic state slice
/// according to the exact M0 reference semantics.
pub fn compute_wealth_gini_dynamic(
    alive_agents: &mut [AgentDynamicState],
) -> Result<f64, Phase10Error> {
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

    // Sort by (wealth ascending, AgentId ascending) in place on contiguous dynamic structs
    alive_agents.sort_unstable_by_key(|a| (a.wealth, a.agent_id));

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

/// Computes the wealth Gini coefficient for alive agents from an [`AgentDynamicSoAScratch`] buffer
/// according to the exact M0 reference semantics.
///
/// Uses `scratch.indices` (or populates from `scratch.alive` if empty)
/// and sorts indices in place by `(wealth ascending, AgentId ascending)` without heap allocations.
pub fn compute_wealth_gini_soa(scratch: &mut AgentDynamicSoAScratch) -> Result<f64, Phase10Error> {
    if scratch.indices.is_empty() {
        for (i, &is_alive) in scratch.alive.iter().enumerate() {
            if is_alive {
                scratch.indices.push(i);
            }
        }
    }

    let n = scratch.indices.len() as u128;
    if n == 0 {
        return Ok(0.0);
    }

    let mut sum_x: u128 = 0;
    for &idx in &scratch.indices {
        sum_x = sum_x
            .checked_add(scratch.wealth[idx] as u128)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
    }

    if sum_x == 0 {
        return Ok(0.0);
    }

    // Sort living indices by (wealth ascending, AgentId ascending)
    scratch
        .indices
        .sort_unstable_by_key(|&i| (scratch.wealth[i], scratch.agent_ids[i]));

    let mut weighted_sum: u128 = 0;
    for (idx, &i) in scratch.indices.iter().enumerate() {
        let rank = (idx as u128) + 1; // 1-indexed: 1..=n
        let x_i = scratch.wealth[i] as u128;
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
