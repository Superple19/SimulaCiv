use crate::config::SimConfig;
use crate::state::{AgentState, SettlementState, WorldState};
use crate::subsystems::Subsystem;
use sim_core::{
    AgentId, DenseSlot, GroupId, Money, RngCoordinate, SimulationDay, coordinate_prng_f32,
};

/// Errors that can occur during Day 0 world initialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitializationError {
    /// Initial money supply calculation overflowed 64-bit signed integer (`Money`).
    MoneyOverflow,
    /// Money conservation invariant violated at Day 0.
    MoneyConservationViolation { expected: Money, actual: Money },
}

impl std::fmt::Display for InitializationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MoneyOverflow => {
                write!(f, "initial money supply calculation overflowed Money (i64)")
            }
            Self::MoneyConservationViolation { expected, actual } => {
                write!(
                    f,
                    "initial money supply mismatch: calculated {}, but sum of wealth and treasury is {}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for InitializationError {}

/// Initializes a deterministic Day 0 world state from a validated `SimConfig`.
pub fn initialize_world(config: &SimConfig) -> Result<WorldState, InitializationError> {
    let n = config.world.initial_population;
    let g = config.world.settlement_count;

    // 1. Initial money supply calculation with checked integer arithmetic
    let population_wealth = (n as i64)
        .checked_mul(config.world.initial_wealth)
        .ok_or(InitializationError::MoneyOverflow)?;
    let total_treasury = (g as i64)
        .checked_mul(config.world.initial_treasury)
        .ok_or(InitializationError::MoneyOverflow)?;
    let initial_money_supply = population_wealth
        .checked_add(total_treasury)
        .ok_or(InitializationError::MoneyOverflow)?;

    // 2. Initialize settlements in strictly ascending GroupId order
    let mut settlements = Vec::with_capacity(g as usize);
    let mut sum_treasury: Money = 0;
    for group_idx in 0..g {
        sum_treasury = sum_treasury
            .checked_add(config.world.initial_treasury)
            .ok_or(InitializationError::MoneyOverflow)?;
        settlements.push(SettlementState {
            group_id: GroupId(group_idx as u16),
            resource: config.world.initial_settlement_resource,
            treasury: config.world.initial_treasury,
        });
    }

    // 3. Initialize agents
    let mut agents = Vec::with_capacity(n as usize);
    let mut sum_wealth: Money = 0;
    for i in 0..n {
        let agent_idx = i as u32;
        let group_id = GroupId((i % (g as u64)) as u16);

        // Deterministic trait initialization via Coordinate PRNG
        let base_coord = RngCoordinate::new(
            config.world.master_seed,
            config.world.replicate_id,
            0,                              // Day 0
            0,                              // Phase 0
            Subsystem::Initialization.id(), // Subsystem 0
            agent_idx,
            0, // DrawIndex placeholder
        );

        // DrawIndex 0 -> productivity
        let mut coord = base_coord;
        coord.draw_index = 0;
        let u_prod = coordinate_prng_f32(&coord);
        let productivity =
            config.traits.prod_min + u_prod * (config.traits.prod_max - config.traits.prod_min);

        // DrawIndex 1 -> cooperation
        coord.draw_index = 1;
        let u_coop = coordinate_prng_f32(&coord);
        let cooperation =
            config.traits.coop_min + u_coop * (config.traits.coop_max - config.traits.coop_min);

        // DrawIndex 2 -> aggression
        coord.draw_index = 2;
        let u_aggr = coordinate_prng_f32(&coord);
        let aggression =
            config.traits.aggr_min + u_aggr * (config.traits.aggr_max - config.traits.aggr_min);

        // DrawIndex 3 -> risk_tolerance
        coord.draw_index = 3;
        let u_risk = coordinate_prng_f32(&coord);
        let risk_tolerance =
            config.traits.risk_min + u_risk * (config.traits.risk_max - config.traits.risk_min);

        sum_wealth = sum_wealth
            .checked_add(config.world.initial_wealth)
            .ok_or(InitializationError::MoneyOverflow)?;

        agents.push(AgentState {
            agent_id: AgentId(agent_idx),
            dense_slot: DenseSlot(agent_idx),
            alive: true,
            birth_day: SimulationDay(0),
            health: config.world.initial_health,
            food: config.world.initial_food,
            wealth: config.world.initial_wealth,
            productivity,
            cooperation,
            aggression,
            risk_tolerance,
            group_id,
        });
    }

    // 4. Validate money conservation invariant at Day 0
    let actual_sum = sum_wealth
        .checked_add(sum_treasury)
        .ok_or(InitializationError::MoneyOverflow)?;
    if actual_sum != initial_money_supply {
        return Err(InitializationError::MoneyConservationViolation {
            expected: initial_money_supply,
            actual: actual_sum,
        });
    }

    Ok(WorldState {
        current_day: SimulationDay(0),
        agents,
        settlements,
        initial_money_supply,
    })
}
