use crate::config::SimConfig;
use crate::state::WorldState;

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
