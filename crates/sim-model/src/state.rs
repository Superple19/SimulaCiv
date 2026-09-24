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
