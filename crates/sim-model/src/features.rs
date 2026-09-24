use crate::config::SimConfig;
use crate::state::WorldState;
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

/// Executes Phase 3: Observation & Normalized Feature Extraction.
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
pub fn phase3_observation_and_features(
    world: &WorldState,
    config: &SimConfig,
) -> Result<Vec<AgentFeatures>, Phase3Error> {
    let mut features_list = Vec::with_capacity(world.agents.len());

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

        features_list.push(AgentFeatures {
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
    features_list.sort_by_key(|af| af.agent_id);
    Ok(features_list)
}
