use crate::config::SimConfig;
use crate::features::{AgentFeatures, FeatureVector};
use crate::state::{AgentState, WorldState};
use crate::subsystems::Subsystem;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, RngCoordinate, coordinate_prng_f32};

/// Canonical six primary actions available to an agent during Phase 4 action selection.
/// The mapping between enum variant and canonical index is explicit and fixed:
/// - 0 = Work
/// - 1 = BuyFood
/// - 2 = SellFood
/// - 3 = GiveFood
/// - 4 = StealFood
/// - 5 = Idle
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    Work,
    BuyFood,
    SellFood,
    GiveFood,
    StealFood,
    Idle,
}

impl Action {
    /// Total number of canonical actions.
    pub const COUNT: usize = 6;

    /// All canonical actions in strict canonical index order.
    pub const ALL: [Action; 6] = [
        Action::Work,
        Action::BuyFood,
        Action::SellFood,
        Action::GiveFood,
        Action::StealFood,
        Action::Idle,
    ];

    /// Returns the canonical action index in 0..6.
    #[inline]
    pub const fn index(self) -> usize {
        match self {
            Action::Work => 0,
            Action::BuyFood => 1,
            Action::SellFood => 2,
            Action::GiveFood => 3,
            Action::StealFood => 4,
            Action::Idle => 5,
        }
    }

    /// Reconstructs an Action from its canonical index.
    #[inline]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Action::Work),
            1 => Some(Action::BuyFood),
            2 => Some(Action::SellFood),
            3 => Some(Action::GiveFood),
            4 => Some(Action::StealFood),
            5 => Some(Action::Idle),
            _ => None,
        }
    }
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Work => write!(f, "Work"),
            Self::BuyFood => write!(f, "BuyFood"),
            Self::SellFood => write!(f, "SellFood"),
            Self::GiveFood => write!(f, "GiveFood"),
            Self::StealFood => write!(f, "StealFood"),
            Self::Idle => write!(f, "Idle"),
        }
    }
}

impl TryFrom<usize> for Action {
    type Error = usize;

    #[inline]
    fn try_from(index: usize) -> Result<Self, Self::Error> {
        Self::from_index(index).ok_or(index)
    }
}

/// Deterministic primary action decision result for an eligible agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimaryActionChoice {
    pub agent_id: AgentId,
    pub action: Action,
}

/// Explicit error returned during Phase 4 decision evaluation and action selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionError {
    /// A feature record references an AgentId that does not exist in authoritative world state.
    MissingAgent(AgentId),
    /// A feature record references an agent that is not behaviorally eligible (dead or health <= 0.0).
    IneligibleAgent(AgentId),
}

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAgent(id) => write!(f, "feature references non-existent agent: {}", id),
            Self::IneligibleAgent(id) => write!(f, "agent {} is not behaviorally eligible", id),
        }
    }
}

impl std::error::Error for DecisionError {}

/// Evaluates the final utilities U(a_m) for all six canonical actions:
///
/// U(a_m) = U_base(a_m) + M_trait(a_m)
///
/// U_base(a_m) = action_biases[m] + Σ(k=0..4) base_weight_matrix[m][k] * features[k]
///
/// Trait modifiers:
/// - Work: 0.0
/// - BuyFood: 0.0
/// - SellFood: 0.0
/// - GiveFood: trait_weight_cooperation * agent.cooperation
/// - StealFood: (trait_weight_aggression * agent.aggression) + (trait_weight_risk_tolerance * agent.risk_tolerance)
/// - Idle: 0.0
pub fn evaluate_utilities(
    agent: &AgentState,
    features: &FeatureVector,
    config: &SimConfig,
) -> [f32; 6] {
    let mut utilities = [0.0f32; 6];

    for (m, action) in Action::ALL.iter().enumerate() {
        let mut u_base = config.decision.action_biases[m];
        for k in 0..5 {
            let term = config.decision.base_weight_matrix[m][k] * features.values[k];
            u_base += term;
        }

        let trait_mod = match action {
            Action::Work => 0.0f32,
            Action::BuyFood => 0.0f32,
            Action::SellFood => 0.0f32,
            Action::GiveFood => config.decision.trait_weight_cooperation * agent.cooperation,
            Action::StealFood => {
                let aggression_term = config.decision.trait_weight_aggression * agent.aggression;
                let risk_term = config.decision.trait_weight_risk_tolerance * agent.risk_tolerance;
                aggression_term + risk_term
            }
            Action::Idle => 0.0f32,
        };

        utilities[m] = u_base + trait_mod;
    }

    utilities
}

/// Computes the numerically stable softmax probability distribution across the six canonical actions.
///
/// 1. U_max = max(U[0], ..., U[5])
/// 2. shifted[m] = U[m] - U_max
/// 3. scaled[m] = shifted[m] / tau
/// 4. exp_value[m] = exp(scaled[m])
/// 5. denominator = Σ(m=0..5) exp_value[m]
/// 6. P[m] = exp_value[m] / denominator
pub fn stable_softmax(utilities: &[f32; 6], temperature: f32) -> [f32; 6] {
    let mut u_max = utilities[0];
    for &u in &utilities[1..] {
        if u > u_max {
            u_max = u;
        }
    }

    let mut exp_values = [0.0f32; 6];
    for m in 0..6 {
        let shifted = utilities[m] - u_max;
        let scaled = shifted / temperature;
        exp_values[m] = scaled.exp();
    }

    let mut denominator = 0.0f32;
    for exp_val in exp_values {
        denominator += exp_val;
    }

    let mut probabilities = [0.0f32; 6];
    for m in 0..6 {
        probabilities[m] = exp_values[m] / denominator;
    }

    probabilities
}

/// Selects an action using the cumulative threshold rule on [0.0, 1.0) random draw `u`.
///
/// Iterating m = 0..5 in canonical order:
/// next = cumulative + P[m]
/// if cumulative <= u && u < next -> select m
///
/// If u >= C5 due to floating-point accumulation, selects Idle.
pub fn select_action(probabilities: &[f32; 6], u: f32) -> Action {
    let mut cumulative = 0.0f32;
    for (m, &prob) in probabilities.iter().enumerate() {
        let next = cumulative + prob;
        if cumulative <= u && u < next {
            return Action::from_index(m).unwrap();
        }
        cumulative = next;
    }
    Action::Idle
}

/// Executes Phase 4 primary action selection for all agents present in `agent_features`.
///
/// For each behaviorally eligible agent:
/// 1. Validates that the agent exists in `world.agents` and satisfies `alive == true && health > 0.0`.
/// 2. Evaluates the 6 canonical utilities U(a_m) using the agent's traits, config biases, and Phase 3 features.
/// 3. Applies stable softmax with decision temperature `tau`.
/// 4. Draws exactly one coordinate PRNG float `u in [0.0, 1.0)` using:
///    - MasterSeed = config.world.master_seed
///    - ReplicateId = config.world.replicate_id
///    - Day = world.current_day
///    - Phase = 4
///    - SubsystemId = Decision (1)
///    - AgentId = agent.agent_id
///    - DrawIndex = 0
/// 5. Selects primary action via cumulative probability thresholds.
///
/// Output is sorted strictly in ascending `AgentId` order.
/// Phase 4 is purely read-only with respect to authoritative world state (no state mutation, no target selection).
pub fn phase4_primary_action_selection(
    world: &WorldState,
    config: &SimConfig,
    agent_features: &[AgentFeatures],
) -> Result<Vec<PrimaryActionChoice>, DecisionError> {
    let mut choices = Vec::with_capacity(agent_features.len());

    for af in agent_features {
        let agent = world
            .agents
            .iter()
            .find(|a| a.agent_id == af.agent_id)
            .ok_or(DecisionError::MissingAgent(af.agent_id))?;

        if !agent.is_behaviorally_eligible() {
            return Err(DecisionError::IneligibleAgent(af.agent_id));
        }

        let utilities = evaluate_utilities(agent, &af.features, config);
        let probabilities = stable_softmax(&utilities, config.decision.decision_temperature);

        let coord = RngCoordinate::new(
            config.world.master_seed,
            config.world.replicate_id,
            world.current_day.as_u32(),
            4,
            Subsystem::Decision.id(),
            agent.agent_id.as_u32(),
            0,
        );
        let u = coordinate_prng_f32(&coord);
        let action = select_action(&probabilities, u);

        choices.push(PrimaryActionChoice {
            agent_id: agent.agent_id,
            action,
        });
    }

    choices.sort_by_key(|c| c.agent_id);
    Ok(choices)
}

pub use phase4_primary_action_selection as phase4_action_selection;
