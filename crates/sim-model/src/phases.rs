use crate::commands::{Command, CommandExecutionError};
use crate::config::SimConfig;
use crate::state::{AgentDynamicSoAScratch, WorldState};
use crate::storage::SegmentedAgentStorage;
use serde::{Deserialize, Serialize};
use sim_core::AgentId;
use std::collections::HashSet;

/// Executes Phase 1: Environment Regrowth for each settlement.
///
/// For each settlement in ascending `GroupId` order:
/// `R_next = clamp(R + r * R * (1.0 - R / K), 0.0, K)`
pub fn phase1_resource_regrowth(world: &mut WorldState, config: &SimConfig) {
    let r = config.environment.regrowth_rate;
    let k = config.environment.carrying_capacity;

    for settlement in &mut world.settlements {
        let current_r = settlement.resource;
        let r_next = (current_r + r * current_r * (1.0 - current_r / k)).clamp(0.0, k);
        settlement.resource = r_next;
    }
}

/// Updates a single agent's dynamic state fields under Phase 2 biological degradation rules.
#[inline]
pub fn update_biological_degradation(
    health: &mut f32,
    food: &mut f32,
    alive: bool,
    f_metabolic: f32,
    decay_rate: f32,
) {
    if !alive {
        return;
    }

    let f_consumed = (*food).min(f_metabolic);
    let f_deficit = f_metabolic - f_consumed;
    let health_delta = -decay_rate * f_deficit;

    *food = (*food - f_consumed).max(0.0);
    *health = (*health + health_delta).clamp(0.0, 1.0);
}

/// Executes Phase 2: Biological Degradation on a contiguous slice of [`AgentDynamicState`].
pub fn phase2_biological_degradation_dynamic(
    dynamics: &mut [crate::state::AgentDynamicState],
    config: &SimConfig,
) {
    let f_metabolic = config.environment.base_metabolic_cost;
    let decay_rate = config.environment.health_decay_rate;

    for dynamic in dynamics {
        dynamic.degrade(f_metabolic, decay_rate);
    }
}

/// Executes Phase 2: Biological Degradation for each living agent.
///
/// For each agent with `alive == true`:
/// - `F_metabolic = base_metabolic_cost`
/// - `F_consumed = min(agent.food, F_metabolic)`
/// - `F_deficit = F_metabolic - F_consumed`
/// - `health_delta = -health_decay_rate * F_deficit`
/// - `agent.food = max(0.0, agent.food - F_consumed)`
/// - `agent.health = clamp(agent.health + health_delta, 0.0, 1.0)`
///
/// Note: Agents whose health reaches `0.0` remain `alive == true` in Phase 2;
/// death is formally committed only in Phase 9.
pub fn phase2_biological_degradation(world: &mut WorldState, config: &SimConfig) {
    let f_metabolic = config.environment.base_metabolic_cost;
    let decay_rate = config.environment.health_decay_rate;

    for agent in &mut world.agents {
        update_biological_degradation(
            &mut agent.health,
            &mut agent.food,
            agent.alive,
            f_metabolic,
            decay_rate,
        );
    }
}

/// Executes Phase 2: Biological Degradation directly on an [`AgentDynamicSoAScratch`] buffer.
pub fn phase2_biological_degradation_soa(scratch: &mut AgentDynamicSoAScratch, config: &SimConfig) {
    let f_metabolic = config.environment.base_metabolic_cost;
    let decay_rate = config.environment.health_decay_rate;
    scratch.update_biological_degradation(f_metabolic, decay_rate);
}

/// Executes Phase 2: Biological Degradation using an external [`AgentDynamicSoAScratch`] buffer,
/// collecting hot dynamic state, executing SoA degradation, and writing back to authoritative state.
pub fn phase2_biological_degradation_with_scratch(
    world: &mut WorldState,
    config: &SimConfig,
    scratch: &mut AgentDynamicSoAScratch,
) {
    scratch.collect_from_agents(&world.agents);
    phase2_biological_degradation_soa(scratch, config);
    scratch.write_back_phase2(&mut world.agents);
}

/// Executes Phase 2: Biological Degradation natively on authoritative [`SegmentedAgentStorage`].
pub fn phase2_biological_degradation_storage(
    storage: &mut SegmentedAgentStorage,
    config: &SimConfig,
) {
    storage.phase2_degradation_with_config(config);
}

/// Minimal M0-03 runner executing Phase 1 followed by Phase 2 in strict order.
///
/// Does not mutate `current_day`.
pub fn execute_phases_1_and_2(world: &mut WorldState, config: &SimConfig) {
    phase1_resource_regrowth(world, config);
    phase2_biological_degradation(world, config);
}

/// Canonical resolution record for Phase 9 Mortality Status Commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phase9MortalityResolution {
    pub newly_deceased: Vec<AgentId>,
    pub newly_deceased_count: usize,
    pub already_dead_count: usize,
    pub survivors_count: usize,
}

/// Convenience alias for [`Phase9MortalityResolution`].
pub type MortalityResolution = Phase9MortalityResolution;

/// Explicit errors returned during Phase 9 Mortality Status Commitment.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase9Error {
    NonFiniteHealth { agent_id: AgentId, health: f32 },
    DuplicateAgent(AgentId),
    CommandExecution(CommandExecutionError),
    InvariantViolation(String),
}

impl std::fmt::Display for Phase9Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteHealth { agent_id, health } => {
                write!(f, "non-finite health for agent {}: {}", agent_id, health)
            }
            Self::DuplicateAgent(aid) => {
                write!(f, "duplicate agent in world state: {}", aid)
            }
            Self::CommandExecution(err) => write!(f, "command execution failed: {}", err),
            Self::InvariantViolation(msg) => write!(f, "phase 9 invariant violation: {}", msg),
        }
    }
}

impl std::error::Error for Phase9Error {}

impl From<CommandExecutionError> for Phase9Error {
    fn from(err: CommandExecutionError) -> Self {
        Self::CommandExecution(err)
    }
}

/// Executes Phase 9: Mortality Status Commitment on authoritative world state.
///
/// In M0, the sole mortality condition is `health <= 0.0`.
///
/// Execution order:
/// 1. Stage A: Inspect and validate all agents:
///    - Validate unique `AgentId`s.
///    - Validate `health.is_finite()`; reject `NaN`, `+inf`, `-inf` before mutation.
///    - Categorize agents:
///      - `already_dead_count`: `alive == false`.
///      - `survivors_count`: `alive == true && health > 0.0`.
///      - `newly_deceased`: `alive == true && health <= 0.0`.
/// 2. Canonical ordering: Sort `newly_deceased` strictly by ascending `AgentId`.
/// 3. Stage B: Atomic commit:
///    - If `newly_deceased` is non-empty, execute `Command::MortalityStatusCommitment`.
///    - Mutates only `alive: true -> false`.
///    - Performs zero physical compaction; `AgentId`, `DenseSlot`, and physical order remain unchanged.
/// 4. Expose deterministic `Phase9MortalityResolution`.
pub fn phase9_mortality_commitment(
    world: &mut WorldState,
) -> Result<Phase9MortalityResolution, Phase9Error> {
    let n = world.agents.len();
    let mut seen_set = if n > 32 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    let newly_deceased_estimate = world
        .agents
        .iter()
        .filter(|a| a.alive && a.health <= 0.0)
        .count();
    let mut newly_deceased = Vec::with_capacity(newly_deceased_estimate);

    let mut already_dead_count = 0;
    let mut survivors_count = 0;

    for (i, agent) in world.agents.iter().enumerate() {
        let duplicate = if let Some(ref mut set) = seen_set {
            !set.insert(agent.agent_id)
        } else {
            world.agents[..i]
                .iter()
                .any(|a| a.agent_id == agent.agent_id)
        };
        if duplicate {
            return Err(Phase9Error::DuplicateAgent(agent.agent_id));
        }
        if !agent.health.is_finite() {
            return Err(Phase9Error::NonFiniteHealth {
                agent_id: agent.agent_id,
                health: agent.health,
            });
        }

        if !agent.alive {
            already_dead_count += 1;
        } else if agent.health <= 0.0 {
            newly_deceased.push(agent.agent_id);
        } else {
            survivors_count += 1;
        }
    }

    // Canonical ordering: strictly ascending AgentId
    newly_deceased.sort();

    if !newly_deceased.is_empty() {
        let cmd = Command::MortalityStatusCommitment {
            deceased_agents: newly_deceased.clone(),
        };
        cmd.execute(world)?;
    }

    Ok(Phase9MortalityResolution {
        newly_deceased_count: newly_deceased.len(),
        newly_deceased,
        already_dead_count,
        survivors_count,
    })
}

/// Convenience alias for [`phase9_mortality_commitment`].
pub use phase9_mortality_commitment as phase9_mortality_resolution;

/// Convenience wrapper executing Phase 9 mortality status commitment with [`crate::config::SimConfig`].
pub fn phase9_mortality_commitment_with_config(
    world: &mut WorldState,
    _config: &SimConfig,
) -> Result<Phase9MortalityResolution, Phase9Error> {
    phase9_mortality_commitment(world)
}

/// Executes Phase 9: Mortality Status Commitment natively on authoritative [`SegmentedAgentStorage`].
///
/// Validates agent health and identifiers directly across contiguous `demography.alive`,
/// `demography.health`, and `agent_ids` columns, then atomically commits mortality by setting
/// `alive = false` in-place without heap allocations or linear command search overhead.
pub fn phase9_mortality_commitment_storage(
    storage: &mut SegmentedAgentStorage,
) -> Result<Phase9MortalityResolution, Phase9Error> {
    let n = storage.agent_ids.len();
    let mut seen_set = if n > 32 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    let mut newly_deceased_slots = Vec::new();
    let mut newly_deceased = Vec::new();
    let mut already_dead_count = 0;
    let mut survivors_count = 0;

    for i in 0..n {
        let aid = storage.agent_ids[i];
        let duplicate = if let Some(ref mut set) = seen_set {
            !set.insert(aid)
        } else {
            storage.agent_ids[..i].contains(&aid)
        };
        if duplicate {
            return Err(Phase9Error::DuplicateAgent(aid));
        }

        let h = storage.demography.health[i];
        if !h.is_finite() {
            return Err(Phase9Error::NonFiniteHealth {
                agent_id: aid,
                health: h,
            });
        }

        let is_alive = storage.demography.alive[i];
        if !is_alive {
            already_dead_count += 1;
        } else if h <= 0.0 {
            newly_deceased_slots.push(i);
            newly_deceased.push(aid);
        } else {
            survivors_count += 1;
        }
    }

    // Canonical ordering: strictly ascending AgentId
    newly_deceased.sort();

    // Stage B: Atomic in-place commit to demography.alive
    for &slot in &newly_deceased_slots {
        storage.demography.alive[slot] = false;
    }

    Ok(Phase9MortalityResolution {
        newly_deceased_count: newly_deceased.len(),
        newly_deceased,
        already_dead_count,
        survivors_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initialize_world;

    const TEST_CONFIG_TOML: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 0.8
initial_food = 5.0
initial_wealth = 1000
initial_settlement_resource = 500.0
initial_treasury = 1000

[traits]
prod_min = 0.8
prod_max = 1.2
coop_min = 0.3
coop_max = 0.7
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.2
risk_max = 0.6

[environment]
carrying_capacity = 1000.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 10.0
target_reserve = 1000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

    #[test]
    fn test_phase2_storage_parity() {
        let config = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();
        let mut world = initialize_world(&config).unwrap();

        // Mutate some agent values to test edge cases
        world.agents[0].alive = false;
        world.agents[1].food = 0.5; // less than metabolic cost -> deficit
        world.agents[2].food = 0.0; // zero food -> full deficit
        world.agents[3].food = 10.0; // plenty of food -> zero deficit

        let mut world_aos = world.clone();
        let mut storage = SegmentedAgentStorage::from_agents(&world.agents);

        // Run AoS Phase 2
        phase2_biological_degradation(&mut world_aos, &config);

        // Run Native SoA Phase 2
        phase2_biological_degradation_storage(&mut storage, &config);

        // Compare bit-for-bit
        for (i, agent) in world_aos.agents.iter().enumerate() {
            assert_eq!(agent.alive, storage.alive()[i]);
            assert_eq!(
                agent.health,
                storage.health()[i],
                "agent {} health mismatch",
                i
            );
            assert_eq!(agent.food, storage.food()[i], "agent {} food mismatch", i);
        }

        // Test reconstructing agents from storage matches world_aos
        let reconstructed = storage.to_agents();
        assert_eq!(world_aos.agents, reconstructed);
    }

    #[test]
    fn test_phase9_storage_parity() {
        let config = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();
        let mut world = initialize_world(&config).unwrap();

        // Mutate some agent health and alive values to test diverse conditions
        world.agents[0].alive = false; // already dead
        world.agents[1].health = 0.0; // newly deceased (exact boundary)
        world.agents[2].health = -0.5; // newly deceased (negative health)
        world.agents[3].health = 0.8; // survivor
        world.agents[4].health = 0.001; // survivor near boundary

        let mut world_aos = world.clone();
        let mut storage = SegmentedAgentStorage::from_agents(&world.agents);

        // Run AoS Phase 9
        let res_aos = phase9_mortality_commitment(&mut world_aos).unwrap();

        // Run Native SoA Phase 9
        let res_storage = phase9_mortality_commitment_storage(&mut storage).unwrap();

        // Check resolutions match bit-identically
        assert_eq!(res_aos, res_storage);

        // Check living status and health match exactly across all agents
        for (i, agent) in world_aos.agents.iter().enumerate() {
            assert_eq!(
                agent.alive,
                storage.alive()[i],
                "agent {} alive mismatch",
                i
            );
            assert_eq!(
                agent.health,
                storage.health()[i],
                "agent {} health mismatch",
                i
            );
        }

        // Test reconstructing agents from storage matches world_aos
        let reconstructed = storage.to_agents();
        assert_eq!(world_aos.agents, reconstructed);
    }
}
