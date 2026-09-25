use crate::config::SimConfig;
use crate::state::{AgentDynamicSoAScratch, SettlementState, WorldState};
use crate::storage::SegmentedAgentStorage;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId};

/// Canonical 5-element normalized feature vector:
/// `[φ0: hunger_ratio, φ1: wealth_pressure, φ2: health_deficit, φ3: local_scarcity, φ4: food_surplus]`
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FeatureVector {
    pub values: [f32; 5],
}

impl FeatureVector {
    #[inline]
    pub const fn new(values: [f32; 5]) -> Self {
        Self { values }
    }

    #[inline]
    pub const fn as_slice(&self) -> &[f32; 5] {
        &self.values
    }

    #[inline]
    pub const fn hunger_ratio(&self) -> f32 {
        self.values[0]
    }

    #[inline]
    pub const fn wealth_pressure(&self) -> f32 {
        self.values[1]
    }

    #[inline]
    pub const fn health_deficit(&self) -> f32 {
        self.values[2]
    }

    #[inline]
    pub const fn local_scarcity(&self) -> f32 {
        self.values[3]
    }

    #[inline]
    pub const fn food_surplus(&self) -> f32 {
        self.values[4]
    }
}

/// Extracted observation features for a behaviorally eligible agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentFeatures {
    pub agent_id: AgentId,
    pub features: FeatureVector,
}

/// Convenience alias for a contiguous vector of extracted agent features.
pub type FeatureBuffer = Vec<AgentFeatures>;

/// Error returned during Phase 3 feature extraction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase3Error {
    /// An eligible agent references a settlement GroupId that does not exist in authoritative world state.
    MissingSettlement(GroupId),
}

impl std::fmt::Display for Phase3Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSettlement(g) => {
                write!(
                    f,
                    "eligible agent references non-existent settlement GroupId {}",
                    g
                )
            }
        }
    }
}

impl std::error::Error for Phase3Error {}

/// Executes Phase 3: Observation & Normalized Feature Extraction into a reusable buffer.
///
/// For each behaviorally eligible agent (`alive == true && health > 0.0`), computes the canonical 5-element
/// normalized feature vector in [0.0, 1.0]^5:
/// - φ0: hunger_ratio = clamp(1.0 - food / starvation_threshold, 0.0, 1.0)
/// - φ1: wealth_pressure = clamp(1.0 - (wealth as f32) / (target_reserve as f32), 0.0, 1.0)
/// - φ2: health_deficit = clamp(1.0 - health, 0.0, 1.0)
/// - φ3: local_scarcity = 1.0 - clamp(local_resource / carrying_capacity, 0.0, 1.0)
/// - φ4: food_surplus = clamp((food - starvation_threshold) / target_food, 0.0, 1.0)
///
/// Output is sorted strictly in ascending `AgentId` order.
/// Phase 3 is purely observational and does not mutate authoritative world state.
pub fn phase3_observation_and_features_into(
    world: &WorldState,
    config: &SimConfig,
    out: &mut Vec<AgentFeatures>,
) -> Result<(), Phase3Error> {
    out.clear();
    let needed = world.agents.len();
    if out.capacity() < needed {
        out.reserve(needed - out.capacity());
    }

    for agent in &world.agents {
        if !agent.is_behaviorally_eligible() {
            continue;
        }

        let settlement = world
            .settlements
            .iter()
            .find(|s| s.group_id == agent.group_id)
            .ok_or(Phase3Error::MissingSettlement(agent.group_id))?;

        let hunger_ratio =
            (1.0 - agent.food / config.interaction.starvation_threshold).clamp(0.0, 1.0);
        let wealth_pressure =
            (1.0 - (agent.wealth as f32) / (config.economy.target_reserve as f32)).clamp(0.0, 1.0);
        let health_deficit = (1.0 - agent.health).clamp(0.0, 1.0);
        let local_scarcity =
            1.0 - (settlement.resource / config.environment.carrying_capacity).clamp(0.0, 1.0);
        let food_surplus = ((agent.food - config.interaction.starvation_threshold)
            / config.economy.target_food)
            .clamp(0.0, 1.0);

        out.push(AgentFeatures {
            agent_id: agent.agent_id,
            features: FeatureVector::new([
                hunger_ratio,
                wealth_pressure,
                health_deficit,
                local_scarcity,
                food_surplus,
            ]),
        });
    }

    // Canonical output order: strictly ascending AgentId
    out.sort_by_key(|af| af.agent_id);
    Ok(())
}

/// Executes Phase 3: Observation & Normalized Feature Extraction.
///
/// Allocates a new vector and delegates to [`phase3_observation_and_features_into`].
pub fn phase3_observation_and_features(
    world: &WorldState,
    config: &SimConfig,
) -> Result<Vec<AgentFeatures>, Phase3Error> {
    let mut features_list = Vec::with_capacity(world.agents.len());
    phase3_observation_and_features_into(world, config, &mut features_list)?;
    Ok(features_list)
}

/// Executes Phase 3: Observation & Normalized Feature Extraction directly from an [`AgentDynamicSoAScratch`] buffer.
///
/// For each behaviorally eligible agent (`alive == true && health > 0.0`), computes the canonical 5-element
/// normalized feature vector in [0.0, 1.0]^5 directly from contiguous SoA vectors:
/// - φ0: hunger_ratio = clamp(1.0 - food / starvation_threshold, 0.0, 1.0)
/// - φ1: wealth_pressure = clamp(1.0 - (wealth as f32) / (target_reserve as f32), 0.0, 1.0)
/// - φ2: health_deficit = clamp(1.0 - health, 0.0, 1.0)
/// - φ3: local_scarcity = 1.0 - clamp(local_resource / carrying_capacity, 0.0, 1.0)
/// - φ4: food_surplus = clamp((food - starvation_threshold) / target_food, 0.0, 1.0)
///
/// Output is sorted strictly in ascending `AgentId` order.
pub fn phase3_observation_and_features_soa_into(
    world: &WorldState,
    config: &SimConfig,
    scratch: &AgentDynamicSoAScratch,
    out: &mut Vec<AgentFeatures>,
) -> Result<(), Phase3Error> {
    out.clear();
    let n = scratch.agent_ids.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let k = config.environment.carrying_capacity;
    let starvation_threshold = config.interaction.starvation_threshold;
    let target_reserve = config.economy.target_reserve as f32;
    let target_food = config.economy.target_food;

    let mut stack_scarcity = [(GroupId(0), 0.0f32); 8];
    let num_settlements = world.settlements.len();
    let heap_scarcity;
    let scarcity_slice: &[(GroupId, f32)] = if num_settlements <= 8 {
        for (i, s) in world.settlements.iter().enumerate() {
            stack_scarcity[i] = (s.group_id, 1.0 - (s.resource / k).clamp(0.0, 1.0));
        }
        &stack_scarcity[..num_settlements]
    } else {
        heap_scarcity = world
            .settlements
            .iter()
            .map(|s| (s.group_id, 1.0 - (s.resource / k).clamp(0.0, 1.0)))
            .collect::<Vec<_>>();
        &heap_scarcity[..]
    };

    for i in 0..n {
        if !scratch.alive[i] || scratch.health[i] <= 0.0 {
            continue;
        }

        let group_id = scratch.group_ids[i];
        let local_scarcity = scarcity_slice
            .iter()
            .find(|(gid, _)| *gid == group_id)
            .map(|(_, sc)| *sc)
            .ok_or(Phase3Error::MissingSettlement(group_id))?;

        let hunger_ratio = (1.0 - scratch.food[i] / starvation_threshold).clamp(0.0, 1.0);
        let wealth_pressure = (1.0 - (scratch.wealth[i] as f32) / target_reserve).clamp(0.0, 1.0);
        let health_deficit = (1.0 - scratch.health[i]).clamp(0.0, 1.0);
        let food_surplus = ((scratch.food[i] - starvation_threshold) / target_food).clamp(0.0, 1.0);

        out.push(AgentFeatures {
            agent_id: scratch.agent_ids[i],
            features: FeatureVector::new([
                hunger_ratio,
                wealth_pressure,
                health_deficit,
                local_scarcity,
                food_surplus,
            ]),
        });
    }

    // Canonical output order: strictly ascending AgentId
    out.sort_by_key(|af| af.agent_id);
    Ok(())
}

/// Executes Phase 3: Observation & Normalized Feature Extraction using an external [`AgentDynamicSoAScratch`] buffer,
/// collecting hot dynamic state and executing SoA feature extraction into a reusable buffer.
pub fn phase3_observation_and_features_with_scratch(
    world: &WorldState,
    config: &SimConfig,
    scratch: &mut AgentDynamicSoAScratch,
    out: &mut Vec<AgentFeatures>,
) -> Result<(), Phase3Error> {
    scratch.collect_from_agents(&world.agents);
    phase3_observation_and_features_soa_into(world, config, scratch, out)
}

/// Executes Phase 3: Observation & Normalized Feature Extraction natively from authoritative [`SegmentedAgentStorage`].
///
/// For each behaviorally eligible agent (`alive == true && health > 0.0`), computes the canonical 5-element
/// normalized feature vector in [0.0, 1.0]^5 directly from contiguous SoA vectors:
/// - φ0: hunger_ratio = clamp(1.0 - food / starvation_threshold, 0.0, 1.0)
/// - φ1: wealth_pressure = clamp(1.0 - (wealth as f32) / (target_reserve as f32), 0.0, 1.0)
/// - φ2: health_deficit = clamp(1.0 - health, 0.0, 1.0)
/// - φ3: local_scarcity = 1.0 - clamp(local_resource / carrying_capacity, 0.0, 1.0)
/// - φ4: food_surplus = clamp((food - starvation_threshold) / target_food, 0.0, 1.0)
///
/// Output is sorted strictly in ascending `AgentId` order.
pub fn phase3_observation_and_features_storage_into(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    out: &mut Vec<AgentFeatures>,
) -> Result<(), Phase3Error> {
    out.clear();
    let n = storage.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let k = config.environment.carrying_capacity;
    let starvation_threshold = config.interaction.starvation_threshold;
    let target_reserve = config.economy.target_reserve as f32;
    let target_food = config.economy.target_food;

    let mut stack_scarcity = [(GroupId(0), 0.0f32); 8];
    let num_settlements = settlements.len();
    let heap_scarcity;
    let scarcity_slice: &[(GroupId, f32)] = if num_settlements <= 8 {
        for (i, s) in settlements.iter().enumerate() {
            stack_scarcity[i] = (s.group_id, 1.0 - (s.resource / k).clamp(0.0, 1.0));
        }
        &stack_scarcity[..num_settlements]
    } else {
        heap_scarcity = settlements
            .iter()
            .map(|s| (s.group_id, 1.0 - (s.resource / k).clamp(0.0, 1.0)))
            .collect::<Vec<_>>();
        &heap_scarcity[..]
    };

    let alives = storage.alive();
    let healths = storage.health();
    let foods = storage.food();
    let wealths = storage.wealth();
    let group_ids = storage.group_ids();
    let agent_ids = storage.agent_ids();

    for i in 0..n {
        if !alives[i] || healths[i] <= 0.0 {
            continue;
        }

        let group_id = group_ids[i];
        let local_scarcity = scarcity_slice
            .iter()
            .find(|(gid, _)| *gid == group_id)
            .map(|(_, sc)| *sc)
            .ok_or(Phase3Error::MissingSettlement(group_id))?;

        let hunger_ratio = (1.0 - foods[i] / starvation_threshold).clamp(0.0, 1.0);
        let wealth_pressure = (1.0 - (wealths[i] as f32) / target_reserve).clamp(0.0, 1.0);
        let health_deficit = (1.0 - healths[i]).clamp(0.0, 1.0);
        let food_surplus = ((foods[i] - starvation_threshold) / target_food).clamp(0.0, 1.0);

        out.push(AgentFeatures {
            agent_id: agent_ids[i],
            features: FeatureVector::new([
                hunger_ratio,
                wealth_pressure,
                health_deficit,
                local_scarcity,
                food_surplus,
            ]),
        });
    }

    // Canonical output order: strictly ascending AgentId
    out.sort_by_key(|af| af.agent_id);
    Ok(())
}

/// Convenience wrapper for [`phase3_observation_and_features_storage_into`].
#[inline]
pub fn phase3_observation_and_features_storage(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    out: &mut Vec<AgentFeatures>,
) -> Result<(), Phase3Error> {
    phase3_observation_and_features_storage_into(storage, settlements, config, out)
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
    fn test_phase3_storage_parity() {
        let config = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();
        let mut world = initialize_world(&config).unwrap();

        // Mutate some agent values to test edge cases
        world.agents[0].alive = false; // dead agent -> excluded
        world.agents[1].health = 0.0; // zero health -> excluded
        world.agents[2].food = 0.0;
        world.agents[3].wealth = 0;

        let storage = SegmentedAgentStorage::from_agents(&world.agents);

        let mut out_aos = Vec::new();
        let mut out_storage = Vec::new();

        phase3_observation_and_features_into(&world, &config, &mut out_aos).unwrap();
        phase3_observation_and_features_storage_into(
            &storage,
            &world.settlements,
            &config,
            &mut out_storage,
        )
        .unwrap();

        assert_eq!(out_aos.len(), out_storage.len());
        for (a, b) in out_aos.iter().zip(out_storage.iter()) {
            assert_eq!(a.agent_id, b.agent_id);
            for k in 0..5 {
                assert_eq!(
                    a.features.values[k], b.features.values[k],
                    "feature {} mismatch for agent {}",
                    k, a.agent_id
                );
            }
        }
        assert_eq!(out_aos, out_storage);
    }
}
