//! Phase 10: Metrics & Observation Hook for SimulaCiv.
//!
//! Provides deterministic daily macro metrics aggregation:
//! - population (count of living agents)
//! - wealth Gini coefficient (reference integer formulation)
//! - total food reserves (sequential f64 accumulation of living agents' food)
//! - total settlement treasury (checked Money sum in canonical GroupId order)
//!
//! All operations are strictly read-only on `WorldState`, guaranteeing observer independence.

use crate::state::{
    AgentDynamicSoAScratch, AgentDynamicState, AgentState, SettlementState, WorldState,
};
use crate::storage::SegmentedAgentStorage;
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

/// Executes Phase 10: Macroscopic Metrics Observation natively on [`SegmentedAgentStorage`].
///
/// Directly reads contiguous column slices (`alive`, `food`, `wealth`, `agent_ids`) without
/// any intermediate AoS/SoA buffer conversions or heap allocations.
pub fn phase10_observe_storage(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    day: u32,
) -> Result<DailyMetrics, Phase10Error> {
    let mut indices = Vec::with_capacity(storage.len());
    let mut settlement_indices = Vec::with_capacity(settlements.len());
    phase10_observe_storage_with_scratch(
        storage,
        settlements,
        day,
        &mut indices,
        &mut settlement_indices,
    )
}

/// Convenience alias for [`phase10_observe_storage`].
pub use phase10_observe_storage as phase10_observe_segmented;

/// Executes Phase 10: Macroscopic Metrics Observation natively on [`SegmentedAgentStorage`]
/// reusing external index scratch buffers for zero-allocation performance.
pub fn phase10_observe_storage_with_scratch(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    day: u32,
    indices: &mut Vec<usize>,
    settlement_indices: &mut Vec<usize>,
) -> Result<DailyMetrics, Phase10Error> {
    let n = storage.len();

    let agent_ids = storage.agent_ids();
    let foods = storage.food();
    let wealths = storage.wealth();
    let alives = storage.alive();

    // 1. Structural validation of agents
    let mut seen_set = if n > 32 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    for (i, &aid) in agent_ids.iter().enumerate() {
        let duplicate = if let Some(ref mut set) = seen_set {
            !set.insert(aid)
        } else {
            agent_ids[..i].contains(&aid)
        };
        if duplicate {
            return Err(Phase10Error::DuplicateAgent(aid));
        }
        if !foods[i].is_finite() {
            return Err(Phase10Error::NonFiniteFood {
                agent_id: aid,
                food: foods[i],
            });
        }
        if foods[i] < 0.0 {
            return Err(Phase10Error::NegativeFood {
                agent_id: aid,
                food: foods[i],
            });
        }
        if wealths[i] < 0 {
            return Err(Phase10Error::NegativeWealth {
                agent_id: aid,
                wealth: wealths[i],
            });
        }
    }

    // 2. Structural validation of settlements
    for (i, settlement) in settlements.iter().enumerate() {
        if settlements[..i]
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

    // 3. Population: count living agents in contiguous bool vector
    let population = alives.iter().filter(|&&a| a).count() as u64;

    // 4. Canonical living agent indices
    indices.clear();
    for (i, &is_alive) in alives.iter().enumerate() {
        if is_alive {
            indices.push(i);
        }
    }

    // Ensure ascending AgentId canonical order for living agents
    let is_sorted = indices
        .windows(2)
        .all(|w| agent_ids[w[0]] <= agent_ids[w[1]]);
    if !is_sorted {
        indices.sort_unstable_by_key(|&i| agent_ids[i]);
    }

    // 5. Total food reserves: sequential f64 accumulation in ascending AgentId order
    let mut total_food_reserves = 0.0_f64;
    if is_sorted && population as usize == foods.len() {
        // Fast path: all agents living and strictly sorted - contiguous streaming
        for &f in foods {
            total_food_reserves += f as f64;
        }
    } else {
        // Canonical sorted index traversal
        for &idx in indices.iter() {
            total_food_reserves += foods[idx] as f64;
        }
    }
    if !total_food_reserves.is_finite() {
        return Err(Phase10Error::InvariantViolation(
            "total food reserves non-finite".into(),
        ));
    }

    // 6. Total treasury: ascending GroupId checked Money addition
    let settlements_sorted = settlements
        .windows(2)
        .all(|w| w[0].group_id <= w[1].group_id);

    let mut total_treasury: Money = 0;
    if settlements_sorted {
        for s in settlements {
            total_treasury = total_treasury
                .checked_add(s.treasury)
                .ok_or(Phase10Error::ArithmeticOverflow)?;
        }
    } else {
        settlement_indices.clear();
        settlement_indices.extend(0..settlements.len());
        settlement_indices.sort_unstable_by_key(|&i| settlements[i].group_id);
        for &idx in settlement_indices.iter() {
            total_treasury = total_treasury
                .checked_add(settlements[idx].treasury)
                .ok_or(Phase10Error::ArithmeticOverflow)?;
        }
    }

    // 7. Wealth Gini via native SoA column slices
    let wealth_gini = compute_wealth_gini_from_slices(wealths, agent_ids, indices)?;

    Ok(DailyMetrics {
        day,
        population,
        wealth_gini,
        total_food_reserves,
        total_treasury,
    })
}

/// Compact AoS Phase 10 execution (M2-15 baseline representation) for ablation benchmarking.
pub fn phase10_observe_compact_aos(
    world: &WorldState,
    day: u32,
) -> Result<DailyMetrics, Phase10Error> {
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

    // 2. Canonical living agents: ascending AgentId using compact dynamic state
    let mut alive_dynamics: Vec<AgentDynamicState> = Vec::with_capacity(world.agents.len());
    for a in &world.agents {
        if a.alive {
            alive_dynamics.push(AgentDynamicState::from_agent(a));
        }
    }
    alive_dynamics.sort_unstable_by_key(|d| d.agent_id);

    // 3. Population: u64 count of alive agents
    let population = alive_dynamics.len() as u64;

    // 4. Total food reserves: sequential f64 sum in ascending AgentId order
    let mut total_food_reserves = 0.0_f64;
    for d in &alive_dynamics {
        total_food_reserves += d.food as f64;
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
    let wealth_gini = compute_wealth_gini_dynamic(&mut alive_dynamics)?;

    Ok(DailyMetrics {
        day,
        population,
        wealth_gini,
        total_food_reserves,
        total_treasury,
    })
}

/// SoA Phase 10 execution with fresh allocation on every invocation for ablation benchmarking.
pub fn phase10_observe_soa_fresh(
    world: &WorldState,
    day: u32,
) -> Result<DailyMetrics, Phase10Error> {
    let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
    phase10_observe_with_scratch(world, day, &mut scratch)
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

/// Computes the wealth Gini coefficient for alive agents directly from column slices and an indices buffer
/// according to the exact M0 reference semantics.
pub fn compute_wealth_gini_from_slices(
    wealths: &[Money],
    agent_ids: &[AgentId],
    indices: &mut [usize],
) -> Result<f64, Phase10Error> {
    let n = indices.len() as u128;
    if n == 0 {
        return Ok(0.0);
    }

    let mut sum_x: u128 = 0;
    for &idx in indices.iter() {
        sum_x = sum_x
            .checked_add(wealths[idx] as u128)
            .ok_or(Phase10Error::ArithmeticOverflow)?;
    }

    if sum_x == 0 {
        return Ok(0.0);
    }

    // Sort living indices by (wealth ascending, AgentId ascending)
    indices.sort_unstable_by_key(|&i| (wealths[i], agent_ids[i]));

    let mut weighted_sum: u128 = 0;
    for (idx, &i) in indices.iter().enumerate() {
        let rank = (idx as u128) + 1; // 1-indexed: 1..=n
        let x_i = wealths[i] as u128;
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

    compute_wealth_gini_from_slices(&scratch.wealth, &scratch.agent_ids, &mut scratch.indices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::{DenseSlot, SimulationDay};

    fn make_test_world() -> WorldState {
        WorldState {
            current_day: SimulationDay(10),
            agents: vec![
                AgentState {
                    agent_id: AgentId(2),
                    dense_slot: DenseSlot(0),
                    alive: true,
                    birth_day: SimulationDay(0),
                    health: 0.9,
                    food: 12.5,
                    wealth: 250,
                    productivity: 1.0,
                    cooperation: 0.5,
                    aggression: 0.2,
                    risk_tolerance: 0.4,
                    group_id: GroupId(1),
                },
                AgentState {
                    agent_id: AgentId(1),
                    dense_slot: DenseSlot(1),
                    alive: true,
                    birth_day: SimulationDay(0),
                    health: 0.8,
                    food: 7.5,
                    wealth: 100,
                    productivity: 0.9,
                    cooperation: 0.6,
                    aggression: 0.1,
                    risk_tolerance: 0.3,
                    group_id: GroupId(0),
                },
                AgentState {
                    agent_id: AgentId(3),
                    dense_slot: DenseSlot(2),
                    alive: false,
                    birth_day: SimulationDay(0),
                    health: 0.0,
                    food: 0.0,
                    wealth: 50,
                    productivity: 1.2,
                    cooperation: 0.4,
                    aggression: 0.3,
                    risk_tolerance: 0.5,
                    group_id: GroupId(0),
                },
            ],
            settlements: vec![
                SettlementState {
                    group_id: GroupId(0),
                    resource: 1000.0,
                    treasury: 500,
                },
                SettlementState {
                    group_id: GroupId(1),
                    resource: 2000.0,
                    treasury: 1500,
                },
            ],
            initial_money_supply: 2400,
        }
    }

    #[test]
    fn test_phase10_observe_storage_parity() {
        let world = make_test_world();
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let m_aos = phase10_observe(&world, 10).unwrap();
        let m_soa = phase10_observe_storage(&segmented, &world.settlements, 10).unwrap();

        assert_eq!(
            m_aos, m_soa,
            "AoS and SoA Phase 10 metrics must match bit-for-bit"
        );
        assert_eq!(m_soa.population, 2);
        assert_eq!(m_soa.total_food_reserves, 20.0);
        assert_eq!(m_soa.total_treasury, 2000);
    }

    #[test]
    fn test_phase10_observe_storage_errors() {
        let mut world = make_test_world();
        world.agents[0].food = -1.0;
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let res = phase10_observe_storage(&segmented, &world.settlements, 10);
        assert!(matches!(res, Err(Phase10Error::NegativeFood { .. })));
    }
}
