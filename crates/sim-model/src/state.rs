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

/// Structure-of-Arrays (SoA) scratch view of hot dynamic agent state fields
/// (`health`, `food`, `wealth`, `alive`, `agent_ids`) used for cache-line streaming,
/// zero-allocation aggregation, and vectorizable observation passes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentDynamicSoAScratch {
    pub health: Vec<f32>,
    pub food: Vec<f32>,
    pub wealth: Vec<Money>,
    pub alive: Vec<bool>,
    pub agent_ids: Vec<AgentId>,
    pub group_ids: Vec<GroupId>,
    pub indices: Vec<usize>,
    pub settlement_indices: Vec<usize>,
}

impl AgentDynamicSoAScratch {
    /// Constructs an empty [`AgentDynamicSoAScratch`] without initial allocation.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an [`AgentDynamicSoAScratch`] with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            health: Vec::with_capacity(capacity),
            food: Vec::with_capacity(capacity),
            wealth: Vec::with_capacity(capacity),
            alive: Vec::with_capacity(capacity),
            agent_ids: Vec::with_capacity(capacity),
            group_ids: Vec::with_capacity(capacity),
            indices: Vec::with_capacity(capacity),
            settlement_indices: Vec::new(),
        }
    }

    /// Clears all vectors while retaining allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.health.clear();
        self.food.clear();
        self.wealth.clear();
        self.alive.clear();
        self.agent_ids.clear();
        self.group_ids.clear();
        self.indices.clear();
        self.settlement_indices.clear();
    }

    /// Reserves capacity for at least `n` elements in all vectors.
    #[inline]
    pub fn reserve(&mut self, n: usize) {
        self.health.reserve(n);
        self.food.reserve(n);
        self.wealth.reserve(n);
        self.alive.reserve(n);
        self.agent_ids.reserve(n);
        self.group_ids.reserve(n);
        self.indices.reserve(n);
    }

    /// Populates the SoA scratch buffer with hot dynamic state from an authoritative [`AgentState`] slice.
    #[inline]
    pub fn collect_from_agents(&mut self, agents: &[AgentState]) {
        self.clear();
        self.reserve(agents.len());
        for agent in agents {
            self.health.push(agent.health);
            self.food.push(agent.food);
            self.wealth.push(agent.wealth);
            self.alive.push(agent.alive);
            self.agent_ids.push(agent.agent_id);
            self.group_ids.push(agent.group_id);
        }
    }

    /// Updates dynamic state fields under Phase 2 biological degradation rules.
    #[inline]
    pub fn update_biological_degradation(&mut self, f_metabolic: f32, decay_rate: f32) {
        let n = self.health.len();
        for i in 0..n {
            if !self.alive[i] {
                continue;
            }
            let f_consumed = self.food[i].min(f_metabolic);
            let f_deficit = f_metabolic - f_consumed;
            let health_delta = -decay_rate * f_deficit;

            self.food[i] = (self.food[i] - f_consumed).max(0.0);
            self.health[i] = (self.health[i] + health_delta).clamp(0.0, 1.0);
        }
    }

    /// Writes back modified Phase 2 dynamic fields (`health` and `food`) to an authoritative [`AgentState`] slice.
    #[inline]
    pub fn write_back_phase2(&self, agents: &mut [AgentState]) {
        for (agent, (h, f)) in agents
            .iter_mut()
            .zip(self.health.iter().zip(self.food.iter()))
        {
            agent.health = *h;
            agent.food = *f;
        }
    }
}

/// Authoritative Day 0 world state container.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    pub current_day: SimulationDay,
    pub agents: Vec<AgentState>,
    pub settlements: Vec<SettlementState>,
    pub initial_money_supply: Money,
}

impl WorldState {
    /// Returns a [`WorldStorage`](crate::storage::WorldStorage) adapter view over the authoritative agents.
    #[inline]
    pub fn storage(&self) -> crate::storage::AosStorageView<'_> {
        crate::storage::AosStorageView::new(&self.agents)
    }

    /// Wraps this [`WorldState`] into a [`HybridWorldState`] with the chosen authority mode.
    #[inline]
    pub fn into_hybrid(self, mode: AuthorityMode) -> HybridWorldState {
        HybridWorldState::new(self, mode)
    }
}

/// Simulation runtime authority mode determining the single source of truth for agent state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AuthorityMode {
    /// Legacy mode: `WorldState.agents` (AoS) is the authoritative storage layer.
    #[default]
    Legacy,
    /// Hybrid mode: `SegmentedAgentStorage` (Native SoA) is the runtime authoritative storage layer.
    /// AoS views are reconstructed lazily or on-demand only.
    Hybrid,
}

/// Hybrid world state container supporting runtime authority selection between
/// legacy AoS (`Vec<AgentState>`) and Native Segmented SoA (`SegmentedAgentStorage`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HybridWorldState {
    pub world: WorldState,
    pub segmented_storage: Option<crate::storage::SegmentedAgentStorage>,
    pub authority_mode: AuthorityMode,
}

impl HybridWorldState {
    /// Constructs a new [`HybridWorldState`] from an existing [`WorldState`] and authority mode.
    pub fn new(world: WorldState, authority_mode: AuthorityMode) -> Self {
        let segmented_storage = match authority_mode {
            AuthorityMode::Hybrid => Some(crate::storage::SegmentedAgentStorage::from_agents(
                &world.agents,
            )),
            AuthorityMode::Legacy => None,
        };
        Self {
            world,
            segmented_storage,
            authority_mode,
        }
    }

    /// Constructs a [`HybridWorldState`] in Legacy AoS authority mode.
    #[inline]
    pub fn legacy(world: WorldState) -> Self {
        Self::new(world, AuthorityMode::Legacy)
    }

    /// Constructs a [`HybridWorldState`] in Hybrid SoA authority mode.
    #[inline]
    pub fn hybrid(world: WorldState) -> Self {
        Self::new(world, AuthorityMode::Hybrid)
    }

    /// Returns the active authority mode.
    #[inline]
    pub const fn authority_mode(&self) -> AuthorityMode {
        self.authority_mode
    }

    /// Returns true if hybrid segmented SoA authority is active.
    #[inline]
    pub const fn is_hybrid(&self) -> bool {
        matches!(self.authority_mode, AuthorityMode::Hybrid)
    }

    /// Returns a reference to the segmented storage, if initialized.
    #[inline]
    pub fn storage(&self) -> Option<&crate::storage::SegmentedAgentStorage> {
        self.segmented_storage.as_ref()
    }

    /// Returns a mutable reference to the segmented storage, if initialized.
    #[inline]
    pub fn storage_mut(&mut self) -> Option<&mut crate::storage::SegmentedAgentStorage> {
        self.segmented_storage.as_mut()
    }

    /// Reconstructs or extracts an AoS compatibility view of all agents.
    ///
    /// In Hybrid mode, this reconstructs `Vec<AgentState>` on-demand from the authoritative
    /// segmented column storage. In Legacy mode, it clones `world.agents`.
    pub fn agents_view(&self) -> Vec<AgentState> {
        match &self.segmented_storage {
            Some(storage) => storage.to_agents(),
            None => self.world.agents.clone(),
        }
    }

    /// Synchronizes the authoritative segmented column storage back into `world.agents`.
    ///
    /// This adapter operation is intended exclusively for snapshot boundaries or legacy
    /// external export and is NOT executed during standard simulation ticks.
    pub fn sync_to_world_agents(&mut self) {
        if let Some(storage) = &self.segmented_storage {
            storage.write_back_to_agents(&mut self.world.agents);
        }
    }

    /// Computes the canonical state hash for this world state, delegating to native
    /// column storage in Hybrid mode or AoS preimage in Legacy mode.
    pub fn canonical_state_hash(
        &self,
    ) -> Result<crate::hashing::CanonicalHash, crate::hashing::CanonicalHashError> {
        match &self.segmented_storage {
            Some(storage) => {
                storage.canonical_state_hash(self.world.current_day, &self.world.settlements)
            }
            None => crate::hashing::canonical_state_hash(&self.world),
        }
    }

    /// Produces the canonical state bytes for this world state.
    pub fn canonical_state_bytes(&self) -> Result<Vec<u8>, crate::hashing::CanonicalHashError> {
        match &self.segmented_storage {
            Some(storage) => {
                storage.canonical_state_bytes(self.world.current_day, &self.world.settlements)
            }
            None => crate::hashing::canonical_state_bytes(&self.world),
        }
    }
}
