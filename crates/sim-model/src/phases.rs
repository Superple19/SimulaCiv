use crate::commands::{Command, CommandExecutionError};
use crate::config::SimConfig;
use crate::state::WorldState;
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
        if !agent.alive {
            continue;
        }

        let f_consumed = agent.food.min(f_metabolic);
        let f_deficit = f_metabolic - f_consumed;
        let health_delta = -decay_rate * f_deficit;

        agent.food = (agent.food - f_consumed).max(0.0);
        agent.health = (agent.health + health_delta).clamp(0.0, 1.0);
    }
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
    let mut seen_agents = HashSet::new();
    let mut already_dead_count = 0;
    let mut survivors_count = 0;
    let mut newly_deceased = Vec::new();

    for agent in &world.agents {
        if !seen_agents.insert(agent.agent_id) {
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
