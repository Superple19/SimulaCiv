use serde::{Deserialize, Serialize};
use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};

/// Authoritative Day 0 state of an individual agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentState {
    pub agent_id: AgentId,
    pub dense_slot: DenseSlot,
    pub alive: bool,
    pub birth_day: SimulationDay,
    pub health: f32,
    pub food: f32,
    pub wealth: Money,
    pub productivity: f32,
    pub cooperation: f32,
    pub aggression: f32,
    pub risk_tolerance: f32,
    pub group_id: GroupId,
}

impl AgentState {
    /// Returns true if the agent is alive and has health strictly greater than 0.0.
    #[inline]
    pub fn is_behaviorally_eligible(&self) -> bool {
        self.alive && self.health > 0.0
    }

    /// Extracts the hot dynamic state view for this agent.
    #[inline]
    pub fn dynamic_state(&self) -> AgentDynamicState {
        AgentDynamicState::from_agent(self)
    }

    /// Updates this agent's dynamic state fields from an [`AgentDynamicState`].
    #[inline]
    pub fn update_from_dynamic(&mut self, dynamic: &AgentDynamicState) {
        dynamic.apply_to_agent(self);
    }
}

/// Compact representation of the hot, frequently updated dynamic state fields of an agent
/// (`health`, `food`, `wealth`, `alive`) for cache-efficient sequential processing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AgentDynamicState {
    pub agent_id: AgentId,
    pub health: f32,
    pub food: f32,
    pub wealth: Money,
    pub alive: bool,
}

impl AgentDynamicState {
    /// Constructs a new [`AgentDynamicState`].
    #[inline]
    pub const fn new(
        agent_id: AgentId,
        health: f32,
        food: f32,
        wealth: Money,
        alive: bool,
    ) -> Self {
        Self {
            agent_id,
            health,
            food,
            wealth,
            alive,
        }
    }

    /// Extracts hot dynamic state from an authoritative [`AgentState`].
    #[inline]
    pub fn from_agent(agent: &AgentState) -> Self {
        Self {
            agent_id: agent.agent_id,
            health: agent.health,
            food: agent.food,
            wealth: agent.wealth,
            alive: agent.alive,
        }
    }

    /// Writes dynamic fields back into an authoritative [`AgentState`].
    #[inline]
    pub fn apply_to_agent(&self, agent: &mut AgentState) {
        agent.health = self.health;
        agent.food = self.food;
        agent.wealth = self.wealth;
        agent.alive = self.alive;
    }

    /// Returns true if the agent is alive and has health strictly greater than 0.0.
    #[inline]
    pub fn is_behaviorally_eligible(&self) -> bool {
        self.alive && self.health > 0.0
    }

    /// Updates dynamic state under Phase 2 biological degradation rules.
    #[inline]
    pub fn degrade(&mut self, f_metabolic: f32, decay_rate: f32) {
        if !self.alive {
            return;
        }

        let f_consumed = self.food.min(f_metabolic);
        let f_deficit = f_metabolic - f_consumed;
        let health_delta = -decay_rate * f_deficit;

        self.food = (self.food - f_consumed).max(0.0);
        self.health = (self.health + health_delta).clamp(0.0, 1.0);
    }
}

/// Authoritative Day 0 state of a settlement / locality bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementState {
    pub group_id: GroupId,
    pub resource: f32,
    pub treasury: Money,
}

/// Authoritative Day 0 world state container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub current_day: SimulationDay,
    pub agents: Vec<AgentState>,
    pub settlements: Vec<SettlementState>,
    pub initial_money_supply: Money,
}
