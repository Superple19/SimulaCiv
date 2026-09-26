//! Segmented Structure-of-Arrays (SoA) Storage Architecture POC.
//!
//! Provides authoritative column-oriented agent storage partitioned into cohesive,
//! cache-aligned segments (`DemographyStorage`, `EconomyStorage`, `PersonalityStorage`)
//! to eliminate ephemeral AoS/SoA conversion overhead while preserving the canonical
//! M0/M1 `AgentState` contracts and hash equivalence.

use crate::hashing::{CanonicalHash, CanonicalHashError, DOMAIN_STATE};
use crate::state::{AgentState, SettlementState, WorldState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use std::collections::{HashMap, HashSet};

/// Demographic and biological vitality columns.
///
/// Touched frequently during lifecycle phases (Phase 2 degradation, Phase 9 mortality,
/// Phase 10 census).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DemographyStorage {
    pub alive: Vec<bool>,
    pub birth_day: Vec<SimulationDay>,
    pub health: Vec<f32>,
}

impl DemographyStorage {
    /// Constructs an empty [`DemographyStorage`] without initial allocation.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an empty [`DemographyStorage`] with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            alive: Vec::with_capacity(capacity),
            birth_day: Vec::with_capacity(capacity),
            health: Vec::with_capacity(capacity),
        }
    }

    /// Number of agent records in storage.
    #[inline]
    pub fn len(&self) -> usize {
        self.alive.len()
    }

    /// Returns `true` if storage contains no agent records.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.alive.is_empty()
    }

    /// Clears all columns while retaining allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.alive.clear();
        self.birth_day.clear();
        self.health.clear();
    }

    /// Reserves capacity for at least `additional` more records.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.alive.reserve(additional);
        self.birth_day.reserve(additional);
        self.health.reserve(additional);
    }

    /// Appends demographic fields for an agent.
    #[inline]
    pub fn push(&mut self, alive: bool, birth_day: SimulationDay, health: f32) {
        self.alive.push(alive);
        self.birth_day.push(birth_day);
        self.health.push(health);
    }

    /// Returns a mutable slice over the living status column.
    #[inline]
    pub fn alive_mut(&mut self) -> &mut [bool] {
        &mut self.alive
    }

    /// Returns a mutable slice over the health column.
    #[inline]
    pub fn health_mut(&mut self) -> &mut [f32] {
        &mut self.health
    }
}

/// Economic holdings, trade reserves, and locality columns.
///
/// Touched during Phase 2 (consumption), Phase 3 (scarcity/wealth observation),
/// Phase 6-8 (work, trade, welfare transfers), and Phase 10 (Gini calculation).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EconomyStorage {
    pub food: Vec<f32>,
    pub wealth: Vec<Money>,
    pub group_id: Vec<GroupId>,
}

impl EconomyStorage {
    /// Constructs an empty [`EconomyStorage`] without initial allocation.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an empty [`EconomyStorage`] with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            food: Vec::with_capacity(capacity),
            wealth: Vec::with_capacity(capacity),
            group_id: Vec::with_capacity(capacity),
        }
    }

    /// Number of agent records in storage.
    #[inline]
    pub fn len(&self) -> usize {
        self.food.len()
    }

    /// Returns `true` if storage contains no agent records.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.food.is_empty()
    }

    /// Clears all columns while retaining allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.food.clear();
        self.wealth.clear();
        self.group_id.clear();
    }

    /// Reserves capacity for at least `additional` more records.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.food.reserve(additional);
        self.wealth.reserve(additional);
        self.group_id.reserve(additional);
    }

    /// Appends economic fields for an agent.
    #[inline]
    pub fn push(&mut self, food: f32, wealth: Money, group_id: GroupId) {
        self.food.push(food);
        self.wealth.push(wealth);
        self.group_id.push(group_id);
    }

    /// Returns a mutable slice over the food holdings column.
    #[inline]
    pub fn food_mut(&mut self) -> &mut [f32] {
        &mut self.food
    }

    /// Returns a mutable slice over the wealth column.
    #[inline]
    pub fn wealth_mut(&mut self) -> &mut [Money] {
        &mut self.wealth
    }

    /// Returns a contiguous slice over the group ID column.
    #[inline]
    pub fn group_ids(&self) -> &[GroupId] {
        &self.group_id
    }

    /// Returns a mutable slice over the group ID column.
    #[inline]
    pub fn group_ids_mut(&mut self) -> &mut [GroupId] {
        &mut self.group_id
    }
}

/// Personality traits and behavioral modifier columns.
///
/// Warm, read-only during normal execution; touched primarily during Phase 4 action evaluation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PersonalityStorage {
    pub productivity: Vec<f32>,
    pub cooperation: Vec<f32>,
    pub aggression: Vec<f32>,
    pub risk_tolerance: Vec<f32>,
}

impl PersonalityStorage {
    /// Constructs an empty [`PersonalityStorage`] without initial allocation.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an empty [`PersonalityStorage`] with pre-allocated capacity.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            productivity: Vec::with_capacity(capacity),
            cooperation: Vec::with_capacity(capacity),
            aggression: Vec::with_capacity(capacity),
            risk_tolerance: Vec::with_capacity(capacity),
        }
    }

    /// Number of agent records in storage.
    #[inline]
    pub fn len(&self) -> usize {
        self.productivity.len()
    }

    /// Returns `true` if storage contains no agent records.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.productivity.is_empty()
    }

    /// Clears all columns while retaining allocated capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.productivity.clear();
        self.cooperation.clear();
        self.aggression.clear();
        self.risk_tolerance.clear();
    }

    /// Reserves capacity for at least `additional` more records.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.productivity.reserve(additional);
        self.cooperation.reserve(additional);
        self.aggression.reserve(additional);
        self.risk_tolerance.reserve(additional);
    }

    /// Appends personality trait fields for an agent.
    #[inline]
    pub fn push(
        &mut self,
        productivity: f32,
        cooperation: f32,
        aggression: f32,
        risk_tolerance: f32,
    ) {
        self.productivity.push(productivity);
        self.cooperation.push(cooperation);
        self.aggression.push(aggression);
        self.risk_tolerance.push(risk_tolerance);
    }
}

/// Composite Segmented Structure-of-Arrays (SoA) agent storage container.
///
/// Holds partitioned columns across demography, economy, and personality domains,
/// alongside identity tracking and fast $O(1)$ `AgentId -> dense slot` resolution.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SegmentedAgentStorage {
    pub demography: DemographyStorage,
    pub economy: EconomyStorage,
    pub personality: PersonalityStorage,
    pub agent_ids: Vec<AgentId>,
    pub dense_slots: Vec<DenseSlot>,
    #[serde(skip)]
    slot_map: HashMap<AgentId, usize>,
}

impl SegmentedAgentStorage {
    /// Constructs an empty [`SegmentedAgentStorage`].
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs an empty [`SegmentedAgentStorage`] with pre-allocated capacity across all segments.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            demography: DemographyStorage::with_capacity(capacity),
            economy: EconomyStorage::with_capacity(capacity),
            personality: PersonalityStorage::with_capacity(capacity),
            agent_ids: Vec::with_capacity(capacity),
            dense_slots: Vec::with_capacity(capacity),
            slot_map: HashMap::with_capacity(capacity),
        }
    }

    /// Returns the number of agent entities stored.
    #[inline]
    pub fn len(&self) -> usize {
        self.agent_ids.len()
    }

    /// Returns `true` if the storage contains no agents.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.agent_ids.is_empty()
    }

    // =========================================================================
    // Column Slice Accessors
    // =========================================================================

    /// Returns a contiguous slice over the living status column (`demography.alive`).
    #[inline]
    pub fn alive(&self) -> &[bool] {
        &self.demography.alive
    }

    /// Returns a contiguous slice over the health column (`demography.health`).
    #[inline]
    pub fn health(&self) -> &[f32] {
        &self.demography.health
    }

    /// Returns a contiguous slice over the food holdings column (`economy.food`).
    #[inline]
    pub fn food(&self) -> &[f32] {
        &self.economy.food
    }

    /// Returns a contiguous slice over the wealth column (`economy.wealth`).
    #[inline]
    pub fn wealth(&self) -> &[Money] {
        &self.economy.wealth
    }

    /// Returns a contiguous slice over the permanent agent IDs column (`agent_ids`).
    #[inline]
    pub fn agent_ids(&self) -> &[AgentId] {
        &self.agent_ids
    }

    /// Returns a mutable slice over the living status column (`demography.alive`).
    #[inline]
    pub fn alive_mut(&mut self) -> &mut [bool] {
        &mut self.demography.alive
    }

    /// Returns a mutable slice over the health column (`demography.health`).
    #[inline]
    pub fn health_mut(&mut self) -> &mut [f32] {
        &mut self.demography.health
    }

    /// Returns a mutable slice over the food holdings column (`economy.food`).
    #[inline]
    pub fn food_mut(&mut self) -> &mut [f32] {
        &mut self.economy.food
    }

    /// Returns a mutable slice over the wealth column (`economy.wealth`).
    #[inline]
    pub fn wealth_mut(&mut self) -> &mut [Money] {
        &mut self.economy.wealth
    }

    /// Returns a contiguous slice over the settlement locality column (`economy.group_id`).
    #[inline]
    pub fn group_ids(&self) -> &[GroupId] {
        &self.economy.group_id
    }

    /// Returns a mutable slice over the settlement locality column (`economy.group_id`).
    #[inline]
    pub fn group_ids_mut(&mut self) -> &mut [GroupId] {
        &mut self.economy.group_id
    }

    /// Returns split disjoint views over columns mutated during Phase 2 (`alive`, `health`, `food`)
    /// without borrow conflicts.
    #[inline]
    pub fn phase2_columns_mut(&mut self) -> (&[bool], &mut [f32], &mut [f32]) {
        (
            &self.demography.alive,
            &mut self.demography.health,
            &mut self.economy.food,
        )
    }

    /// Clears all segments and indices while retaining capacity.
    #[inline]
    pub fn clear(&mut self) {
        self.demography.clear();
        self.economy.clear();
        self.personality.clear();
        self.agent_ids.clear();
        self.dense_slots.clear();
        self.slot_map.clear();
    }

    /// Reserves capacity across all columns for at least `additional` more agents.
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.demography.reserve(additional);
        self.economy.reserve(additional);
        self.personality.reserve(additional);
        self.agent_ids.reserve(additional);
        self.dense_slots.reserve(additional);
        self.slot_map.reserve(additional);
    }

    /// Appends an [`AgentState`] record across all storage segments and updates the slot mapping.
    pub fn push(&mut self, agent: &AgentState) {
        let slot = self.agent_ids.len();
        self.demography
            .push(agent.alive, agent.birth_day, agent.health);
        self.economy.push(agent.food, agent.wealth, agent.group_id);
        self.personality.push(
            agent.productivity,
            agent.cooperation,
            agent.aggression,
            agent.risk_tolerance,
        );
        self.agent_ids.push(agent.agent_id);
        self.dense_slots.push(agent.dense_slot);
        self.slot_map.insert(agent.agent_id, slot);
    }

    /// Rebuilds the internal `AgentId -> dense slot` lookup map.
    pub fn rebuild_slot_map(&mut self) {
        self.slot_map.clear();
        self.slot_map.reserve(self.agent_ids.len());
        for (slot, &id) in self.agent_ids.iter().enumerate() {
            self.slot_map.insert(id, slot);
        }
    }

    /// Resolves the internal dense slot index of an agent by permanent [`AgentId`].
    #[inline]
    pub fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.slot_map.get(&id).copied()
    }

    /// Resolves the [`DenseSlot`] identifier of an agent by permanent [`AgentId`].
    #[inline]
    pub fn dense_slot_of(&self, id: AgentId) -> Option<DenseSlot> {
        self.slot_of(id).map(|slot| self.dense_slots[slot])
    }

    /// Reconstructs an authoritative [`AgentState`] representation for a given dense slot index.
    pub fn agent_at(&self, slot: usize) -> Option<AgentState> {
        if slot >= self.len() {
            return None;
        }
        Some(AgentState {
            agent_id: self.agent_ids[slot],
            dense_slot: self.dense_slots[slot],
            alive: self.demography.alive[slot],
            birth_day: self.demography.birth_day[slot],
            health: self.demography.health[slot],
            food: self.economy.food[slot],
            wealth: self.economy.wealth[slot],
            productivity: self.personality.productivity[slot],
            cooperation: self.personality.cooperation[slot],
            aggression: self.personality.aggression[slot],
            risk_tolerance: self.personality.risk_tolerance[slot],
            group_id: self.economy.group_id[slot],
        })
    }

    /// Reconstructs an authoritative [`AgentState`] representation by permanent [`AgentId`].
    pub fn get_agent(&self, id: AgentId) -> Option<AgentState> {
        let slot = self.slot_of(id)?;
        self.agent_at(slot)
    }

    // =========================================================================
    // Adapter Interfaces
    // =========================================================================

    /// Ingests an authoritative slice of [`AgentState`] records into segmented storage.
    pub fn from_agents(agents: &[AgentState]) -> Self {
        let mut storage = Self::with_capacity(agents.len());
        for agent in agents {
            storage.push(agent);
        }
        storage
    }

    /// Reconstructs an authoritative [`Vec<AgentState>`] in dense slot order.
    pub fn to_agents(&self) -> Vec<AgentState> {
        let n = self.len();
        let mut out = Vec::with_capacity(n);
        for slot in 0..n {
            if let Some(agent) = self.agent_at(slot) {
                out.push(agent);
            }
        }
        out
    }

    /// Synchronizes dynamic and mutable fields back into an authoritative [`AgentState`] slice.
    pub fn write_back_to_agents(&self, agents: &mut [AgentState]) {
        for (i, agent) in agents.iter_mut().enumerate().take(self.len()) {
            agent.alive = self.demography.alive[i];
            agent.health = self.demography.health[i];
            agent.food = self.economy.food[i];
            agent.wealth = self.economy.wealth[i];
            agent.group_id = self.economy.group_id[i];
        }
    }

    /// Synchronizes mutable dynamic fields from an updated [`AgentState`] slice into existing segmented storage columns.
    pub fn sync_dynamic_from_agents(&mut self, agents: &[AgentState]) {
        for (i, agent) in agents.iter().enumerate().take(self.len()) {
            self.demography.alive[i] = agent.alive;
            self.demography.health[i] = agent.health;
            self.economy.food[i] = agent.food;
            self.economy.wealth[i] = agent.wealth;
            self.economy.group_id[i] = agent.group_id;
        }
    }

    /// Re-populates segmented storage from updated authoritative [`AgentState`] slice in-place.
    pub fn sync_from_agents(&mut self, agents: &[AgentState]) {
        self.clear();
        self.reserve(agents.len());
        for agent in agents {
            self.push(agent);
        }
    }

    /// Convenience adapter to construct a full [`WorldState`].
    pub fn to_world_state(
        &self,
        current_day: SimulationDay,
        settlements: Vec<SettlementState>,
        initial_money_supply: Money,
    ) -> WorldState {
        WorldState {
            current_day,
            agents: self.to_agents(),
            settlements,
            initial_money_supply,
        }
    }

    // =========================================================================
    // Native SoA Execution Kernels
    // =========================================================================

    /// Executes Phase 2: Biological Degradation natively on authoritative segmented storage columns.
    ///
    /// Directly updates `demography.health` and `economy.food` for all living agents in
    /// `demography.alive` in a single contiguous SIMD-friendly pass without any AoS/SoA
    /// conversion or heap allocation.
    #[inline]
    pub fn phase2_biological_degradation(&mut self, f_metabolic: f32, decay_rate: f32) {
        let alive = &self.demography.alive;
        let health = &mut self.demography.health;
        let food = &mut self.economy.food;

        for (is_alive, (h, f)) in alive.iter().zip(health.iter_mut().zip(food.iter_mut())) {
            if !*is_alive {
                continue;
            }
            let f_consumed = (*f).min(f_metabolic);
            let f_deficit = f_metabolic - f_consumed;
            let health_delta = -decay_rate * f_deficit;

            *f = (*f - f_consumed).max(0.0);
            *h = (*h + health_delta).clamp(0.0, 1.0);
        }
    }

    /// Executes Phase 2: Biological Degradation natively given [`SimConfig`](crate::SimConfig).
    #[inline]
    pub fn phase2_degradation_with_config(&mut self, config: &crate::SimConfig) {
        self.phase2_biological_degradation(
            config.environment.base_metabolic_cost,
            config.environment.health_decay_rate,
        );
    }

    /// Executes Phase 3: Observation & Normalized Feature Extraction natively on segmented storage columns.
    #[inline]
    pub fn phase3_features_into(
        &self,
        settlements: &[SettlementState],
        config: &crate::SimConfig,
        out: &mut Vec<crate::features::AgentFeatures>,
    ) -> Result<(), crate::features::Phase3Error> {
        crate::features::phase3_observation_and_features_storage_into(
            self,
            settlements,
            config,
            out,
        )
    }

    /// Executes Phase 3: Observation & Normalized Feature Extraction natively on segmented storage columns,
    /// returning a freshly allocated vector.
    #[inline]
    pub fn phase3_features(
        &self,
        settlements: &[SettlementState],
        config: &crate::SimConfig,
    ) -> Result<Vec<crate::features::AgentFeatures>, crate::features::Phase3Error> {
        let mut out = Vec::with_capacity(self.len());
        self.phase3_features_into(settlements, config, &mut out)?;
        Ok(out)
    }

    /// Executes Phase 8: Institutional Welfare Distribution natively on segmented storage columns.
    #[inline]
    pub fn phase8_welfare_distribution(
        &mut self,
        settlements: &mut [SettlementState],
        starvation_threshold: f32,
        welfare_payment: Money,
    ) -> Result<Vec<crate::resolution::SettlementWelfareResolution>, crate::resolution::Phase8Error>
    {
        crate::resolution::phase8_welfare_distribution_storage(
            self,
            settlements,
            starvation_threshold,
            welfare_payment,
        )
    }

    /// Executes Phase 8: Institutional Welfare Distribution natively given [`SimConfig`](crate::SimConfig).
    #[inline]
    pub fn phase8_welfare_distribution_with_config(
        &mut self,
        settlements: &mut [SettlementState],
        config: &crate::SimConfig,
    ) -> Result<Vec<crate::resolution::SettlementWelfareResolution>, crate::resolution::Phase8Error>
    {
        self.phase8_welfare_distribution(
            settlements,
            config.interaction.starvation_threshold,
            config.economy.welfare_payment,
        )
    }

    /// Executes Phase 9: Mortality Status Commitment natively on segmented storage columns.
    #[inline]
    pub fn phase9_mortality_commitment(
        &mut self,
    ) -> Result<crate::phases::Phase9MortalityResolution, crate::phases::Phase9Error> {
        crate::phases::phase9_mortality_commitment_storage(self)
    }

    /// Executes Phase 10: Macroscopic Metrics Observation natively on segmented storage columns.
    #[inline]
    pub fn phase10_metrics(
        &self,
        settlements: &[SettlementState],
        day: u32,
    ) -> Result<crate::metrics::DailyMetrics, crate::metrics::Phase10Error> {
        crate::metrics::phase10_observe_storage(self, settlements, day)
    }

    // =========================================================================
    // Canonical State Hashing
    // =========================================================================

    /// Encodes the segmented storage state into canonical binary preimage bytes,
    /// guaranteeing 100% bit-exact equivalence with `canonical_state_bytes(&world)`.
    pub fn canonical_state_bytes(
        &self,
        current_day: SimulationDay,
        settlements: &[SettlementState],
    ) -> Result<Vec<u8>, CanonicalHashError> {
        let agent_count_u32 =
            u32::try_from(self.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
        let settlement_count_u32 =
            u32::try_from(settlements.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;

        // Validate agent unique IDs & constraints
        let mut seen_agents = HashSet::with_capacity(self.len());
        for i in 0..self.len() {
            let aid = self.agent_ids[i];
            if !seen_agents.insert(aid) {
                return Err(CanonicalHashError::DuplicateAgent(aid));
            }
            if !self.demography.health[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("health"));
            }
            if !self.economy.food[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("food"));
            }
            if self.economy.food[i] < 0.0 {
                return Err(CanonicalHashError::NegativeValue("food"));
            }
            if self.economy.wealth[i] < 0 {
                return Err(CanonicalHashError::NegativeMoney(self.economy.wealth[i]));
            }
            if !self.personality.productivity[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("productivity"));
            }
            if !self.personality.cooperation[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("cooperation"));
            }
            if !self.personality.aggression[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("aggression"));
            }
            if !self.personality.risk_tolerance[i].is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("risk_tolerance"));
            }
        }

        // Validate settlements
        let mut seen_groups = HashSet::with_capacity(settlements.len());
        for s in settlements {
            if !seen_groups.insert(s.group_id) {
                return Err(CanonicalHashError::DuplicateSettlement(s.group_id));
            }
            if !s.resource.is_finite() {
                return Err(CanonicalHashError::NonFiniteFloat("resource"));
            }
            if s.resource < 0.0 {
                return Err(CanonicalHashError::NegativeValue("resource"));
            }
            if s.treasury < 0 {
                return Err(CanonicalHashError::NegativeMoney(s.treasury));
            }
        }

        let estimated_size = 15 + 4 + 4 + (self.len() * 43) + 4 + (settlements.len() * 14);
        let mut bytes = Vec::with_capacity(estimated_size);

        // Domain separator
        bytes.extend_from_slice(DOMAIN_STATE);

        // Logical coordinates
        bytes.extend_from_slice(&current_day.as_u32().to_le_bytes());
        bytes.extend_from_slice(&agent_count_u32.to_le_bytes());

        // Canonical sorted order: strictly ascending AgentId
        let mut sorted_indices: Vec<usize> = (0..self.len()).collect();
        sorted_indices.sort_by_key(|&idx| self.agent_ids[idx]);

        for &i in &sorted_indices {
            bytes.extend_from_slice(&self.agent_ids[i].as_u32().to_le_bytes());
            bytes.push(if self.demography.alive[i] { 0x01 } else { 0x00 });
            bytes.extend_from_slice(&self.demography.birth_day[i].as_u32().to_le_bytes());
            bytes.extend_from_slice(&self.demography.health[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.economy.food[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.economy.wealth[i].to_le_bytes());
            bytes.extend_from_slice(&self.personality.productivity[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.personality.cooperation[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.personality.aggression[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.personality.risk_tolerance[i].to_bits().to_le_bytes());
            bytes.extend_from_slice(&self.economy.group_id[i].0.to_le_bytes());
        }

        // Settlements
        bytes.extend_from_slice(&settlement_count_u32.to_le_bytes());
        let mut sorted_settlements: Vec<&SettlementState> = settlements.iter().collect();
        sorted_settlements.sort_by_key(|s| s.group_id);

        for s in sorted_settlements {
            bytes.extend_from_slice(&s.group_id.0.to_le_bytes());
            bytes.extend_from_slice(&s.resource.to_bits().to_le_bytes());
            bytes.extend_from_slice(&s.treasury.to_le_bytes());
        }

        Ok(bytes)
    }

    /// Computes the 32-byte canonical SHA-256 state hash directly from segmented storage.
    pub fn canonical_state_hash(
        &self,
        current_day: SimulationDay,
        settlements: &[SettlementState],
    ) -> Result<CanonicalHash, CanonicalHashError> {
        let bytes = self.canonical_state_bytes(current_day, settlements)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        Ok(CanonicalHash::from_bytes(hasher.finalize().into()))
    }
}

// =========================================================================
// WorldStorage Abstraction Layer
// =========================================================================

/// Abstraction trait for read access to agent entity storage.
///
/// Provides a unified, layout-agnostic interface across both legacy AoS
/// (`Vec<AgentState>`, `AosStorageView`) and modern columnar Segmented SoA
/// (`SegmentedAgentStorage`), allowing simulation systems to decouple from
/// concrete storage representations in preparation for M2 authoritative migration.
pub trait WorldStorage {
    /// Returns the total number of agent entities.
    fn agent_count(&self) -> usize;

    /// Returns `true` if the storage contains no agent records.
    #[inline]
    fn is_empty(&self) -> bool {
        self.agent_count() == 0
    }

    /// Reconstructs or retrieves the authoritative [`AgentState`] at the given dense slot index.
    fn agent_state(&self, slot: usize) -> AgentState;

    /// Resolves the permanent [`AgentId`] at the given dense slot index.
    fn agent_id(&self, slot: usize) -> AgentId;

    /// Safely retrieves an [`AgentState`] if the slot index is in-bounds.
    #[inline]
    fn get_agent_state(&self, slot: usize) -> Option<AgentState> {
        if slot < self.agent_count() {
            Some(self.agent_state(slot))
        } else {
            None
        }
    }

    /// Safely resolves the permanent [`AgentId`] if the slot index is in-bounds.
    #[inline]
    fn get_agent_id(&self, slot: usize) -> Option<AgentId> {
        if slot < self.agent_count() {
            Some(self.agent_id(slot))
        } else {
            None
        }
    }

    /// Resolves the dense slot index for a given permanent [`AgentId`].
    fn slot_of(&self, id: AgentId) -> Option<usize>;

    /// Reconstructs all agents into a contiguous [`Vec<AgentState>`].
    fn to_agents(&self) -> Vec<AgentState> {
        let n = self.agent_count();
        let mut out = Vec::with_capacity(n);
        for slot in 0..n {
            out.push(self.agent_state(slot));
        }
        out
    }
}

/// Borrowed compatibility view wrapping an authoritative AoS slice of [`AgentState`].
#[derive(Debug, Clone, Copy)]
pub struct AosStorageView<'a> {
    agents: &'a [AgentState],
}

impl<'a> AosStorageView<'a> {
    /// Constructs a new [`AosStorageView`] over the provided agent slice.
    #[inline]
    pub const fn new(agents: &'a [AgentState]) -> Self {
        Self { agents }
    }

    /// Returns the underlying raw slice of [`AgentState`].
    #[inline]
    pub const fn as_slice(&self) -> &'a [AgentState] {
        self.agents
    }
}

impl<'a> WorldStorage for AosStorageView<'a> {
    #[inline]
    fn agent_count(&self) -> usize {
        self.agents.len()
    }

    #[inline]
    fn agent_state(&self, slot: usize) -> AgentState {
        self.agents[slot].clone()
    }

    #[inline]
    fn agent_id(&self, slot: usize) -> AgentId {
        self.agents[slot].agent_id
    }

    #[inline]
    fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.agents.iter().position(|a| a.agent_id == id)
    }

    #[inline]
    fn to_agents(&self) -> Vec<AgentState> {
        self.agents.to_vec()
    }
}

impl WorldStorage for [AgentState] {
    #[inline]
    fn agent_count(&self) -> usize {
        self.len()
    }

    #[inline]
    fn agent_state(&self, slot: usize) -> AgentState {
        self[slot].clone()
    }

    #[inline]
    fn agent_id(&self, slot: usize) -> AgentId {
        self[slot].agent_id
    }

    #[inline]
    fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.iter().position(|a| a.agent_id == id)
    }

    #[inline]
    fn to_agents(&self) -> Vec<AgentState> {
        self.to_vec()
    }
}

impl WorldStorage for Vec<AgentState> {
    #[inline]
    fn agent_count(&self) -> usize {
        self.len()
    }

    #[inline]
    fn agent_state(&self, slot: usize) -> AgentState {
        self[slot].clone()
    }

    #[inline]
    fn agent_id(&self, slot: usize) -> AgentId {
        self[slot].agent_id
    }

    #[inline]
    fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.iter().position(|a| a.agent_id == id)
    }

    #[inline]
    fn to_agents(&self) -> Vec<AgentState> {
        self.clone()
    }
}

impl WorldStorage for SegmentedAgentStorage {
    #[inline]
    fn agent_count(&self) -> usize {
        self.len()
    }

    #[inline]
    fn agent_state(&self, slot: usize) -> AgentState {
        self.agent_at(slot).expect("slot out of bounds")
    }

    #[inline]
    fn agent_id(&self, slot: usize) -> AgentId {
        self.agent_ids[slot]
    }

    #[inline]
    fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.slot_of(id)
    }

    #[inline]
    fn to_agents(&self) -> Vec<AgentState> {
        self.to_agents()
    }
}

impl WorldStorage for WorldState {
    #[inline]
    fn agent_count(&self) -> usize {
        self.agents.len()
    }

    #[inline]
    fn agent_state(&self, slot: usize) -> AgentState {
        self.agents[slot].clone()
    }

    #[inline]
    fn agent_id(&self, slot: usize) -> AgentId {
        self.agents[slot].agent_id
    }

    #[inline]
    fn slot_of(&self, id: AgentId) -> Option<usize> {
        self.agents.iter().position(|a| a.agent_id == id)
    }

    #[inline]
    fn to_agents(&self) -> Vec<AgentState> {
        self.agents.clone()
    }
}

/// Encodes any [`WorldStorage`] representation into canonical binary preimage bytes,
/// guaranteeing 100% bit-exact equivalence with `canonical_state_bytes(&world)`.
pub fn canonical_state_bytes_from_storage<S: WorldStorage + ?Sized>(
    storage: &S,
    current_day: SimulationDay,
    settlements: &[SettlementState],
) -> Result<Vec<u8>, CanonicalHashError> {
    let agent_count = storage.agent_count();
    let agent_count_u32 =
        u32::try_from(agent_count).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let settlement_count_u32 =
        u32::try_from(settlements.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;

    let mut agents = storage.to_agents();
    agents.sort_by_key(|a| a.agent_id);

    for window in agents.windows(2) {
        if window[0].agent_id == window[1].agent_id {
            return Err(CanonicalHashError::DuplicateAgent(window[0].agent_id));
        }
    }

    for a in &agents {
        if !a.health.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("health"));
        }
        if !a.food.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("food"));
        }
        if a.food < 0.0 {
            return Err(CanonicalHashError::NegativeValue("food"));
        }
        if a.wealth < 0 {
            return Err(CanonicalHashError::NegativeMoney(a.wealth));
        }
    }

    for s in settlements {
        if !s.resource.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("settlement_resource"));
        }
        if s.resource < 0.0 {
            return Err(CanonicalHashError::NegativeValue("settlement_resource"));
        }
        if s.treasury < 0 {
            return Err(CanonicalHashError::NegativeMoney(s.treasury));
        }
    }

    // Allocate exact canonical payload size
    let expected_len = DOMAIN_STATE.len()
        + 4 // current_day
        + 4 // agent_count
        + (agents.len() * 43)
        + 4 // settlement_count
        + (settlements.len() * 14);

    let mut bytes = Vec::with_capacity(expected_len);
    bytes.extend_from_slice(DOMAIN_STATE);
    bytes.extend_from_slice(&current_day.0.to_le_bytes());
    bytes.extend_from_slice(&agent_count_u32.to_le_bytes());

    for a in &agents {
        bytes.extend_from_slice(&a.agent_id.0.to_le_bytes());
        bytes.push(if a.alive { 0x01 } else { 0x00 });
        bytes.extend_from_slice(&a.birth_day.0.to_le_bytes());
        bytes.extend_from_slice(&a.health.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.food.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.wealth.to_le_bytes());
        bytes.extend_from_slice(&a.productivity.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.cooperation.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.aggression.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.risk_tolerance.to_bits().to_le_bytes());
        bytes.extend_from_slice(&a.group_id.0.to_le_bytes());
    }

    bytes.extend_from_slice(&settlement_count_u32.to_le_bytes());
    let mut sorted_settlements: Vec<&SettlementState> = settlements.iter().collect();
    sorted_settlements.sort_by_key(|s| s.group_id);

    for s in sorted_settlements {
        bytes.extend_from_slice(&s.group_id.0.to_le_bytes());
        bytes.extend_from_slice(&s.resource.to_bits().to_le_bytes());
        bytes.extend_from_slice(&s.treasury.to_le_bytes());
    }

    Ok(bytes)
}

/// Computes the 32-byte canonical SHA-256 state hash from any [`WorldStorage`] implementation.
pub fn canonical_state_hash_from_storage<S: WorldStorage + ?Sized>(
    storage: &S,
    current_day: SimulationDay,
    settlements: &[SettlementState],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_state_bytes_from_storage(storage, current_day, settlements)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(CanonicalHash::from_bytes(hasher.finalize().into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hashing::{canonical_state_bytes, canonical_state_hash};

    fn make_test_world() -> WorldState {
        WorldState {
            current_day: SimulationDay(42),
            agents: vec![
                AgentState {
                    agent_id: AgentId(2),
                    dense_slot: DenseSlot(1),
                    alive: true,
                    birth_day: SimulationDay(0),
                    health: 0.95,
                    food: 18.5,
                    wealth: 250,
                    productivity: 1.1,
                    cooperation: 0.8,
                    aggression: 0.2,
                    risk_tolerance: 0.4,
                    group_id: GroupId(0),
                },
                AgentState {
                    agent_id: AgentId(1),
                    dense_slot: DenseSlot(0),
                    alive: false,
                    birth_day: SimulationDay(0),
                    health: 0.0,
                    food: 5.0,
                    wealth: 100,
                    productivity: 0.9,
                    cooperation: 0.5,
                    aggression: 0.3,
                    risk_tolerance: 0.2,
                    group_id: GroupId(1),
                },
            ],
            settlements: vec![
                SettlementState {
                    group_id: GroupId(1),
                    resource: 500.0,
                    treasury: 1000,
                },
                SettlementState {
                    group_id: GroupId(0),
                    resource: 1200.0,
                    treasury: 3500,
                },
            ],
            initial_money_supply: 4850,
        }
    }

    #[test]
    fn test_segmented_roundtrip_adapter() {
        let world = make_test_world();
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        assert_eq!(segmented.len(), 2);
        assert_eq!(segmented.slot_of(AgentId(1)), Some(1));
        assert_eq!(segmented.slot_of(AgentId(2)), Some(0));
        assert_eq!(segmented.dense_slot_of(AgentId(1)), Some(DenseSlot(0)));
        assert_eq!(segmented.dense_slot_of(AgentId(2)), Some(DenseSlot(1)));

        let reconstructed = segmented.to_agents();
        assert_eq!(reconstructed, world.agents);
    }

    #[test]
    fn test_canonical_state_hash_equivalence() {
        let world = make_test_world();
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let orig_bytes = canonical_state_bytes(&world).unwrap();
        let seg_bytes = segmented
            .canonical_state_bytes(world.current_day, &world.settlements)
            .unwrap();
        assert_eq!(
            orig_bytes, seg_bytes,
            "canonical bytes must match bit-for-bit"
        );

        let orig_hash = canonical_state_hash(&world).unwrap();
        let seg_hash = segmented
            .canonical_state_hash(world.current_day, &world.settlements)
            .unwrap();
        assert_eq!(orig_hash, seg_hash, "canonical hash must match bit-for-bit");
    }

    #[test]
    fn test_native_phase2_degradation_equivalence() {
        let mut world = make_test_world();
        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let f_metabolic = 1.0;
        let decay_rate = 0.05;

        // AoS update
        for agent in &mut world.agents {
            crate::phases::update_biological_degradation(
                &mut agent.health,
                &mut agent.food,
                agent.alive,
                f_metabolic,
                decay_rate,
            );
        }

        // Native SoA update on segmented storage
        segmented.phase2_biological_degradation(f_metabolic, decay_rate);

        let reconstructed = segmented.to_agents();
        assert_eq!(
            reconstructed, world.agents,
            "segmented Phase 2 update must match AoS Phase 2 exactly"
        );
    }

    #[test]
    fn test_sync_dynamic_from_agents() {
        let world = make_test_world();
        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let mut agents = segmented.to_agents();
        agents[0].health = 0.55;
        agents[0].food = 42.0;
        agents[0].alive = false;

        segmented.sync_dynamic_from_agents(&agents);
        assert_eq!(segmented.demography.health[0], 0.55);
        assert_eq!(segmented.economy.food[0], 42.0);
        assert!(!segmented.demography.alive[0]);
        // personality untouched
        assert_eq!(segmented.personality.productivity[0], 1.1);
    }

    #[test]
    fn test_world_storage_aos_view() {
        let world = make_test_world();
        let storage = world.storage();

        assert_eq!(storage.agent_count(), 2);
        assert!(!storage.is_empty());
        assert_eq!(storage.agent_id(0), AgentId(2));
        assert_eq!(storage.agent_id(1), AgentId(1));
        assert_eq!(storage.agent_state(0), world.agents[0]);
        assert_eq!(storage.agent_state(1), world.agents[1]);
        assert_eq!(storage.slot_of(AgentId(2)), Some(0));
        assert_eq!(storage.slot_of(AgentId(1)), Some(1));
        assert_eq!(storage.slot_of(AgentId(999)), None);
        assert_eq!(storage.to_agents(), world.agents);
    }

    #[test]
    fn test_world_storage_segmented_view() {
        let world = make_test_world();
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        assert_eq!(segmented.agent_count(), 2);
        assert!(!segmented.is_empty());
        assert_eq!(segmented.agent_id(0), AgentId(2));
        assert_eq!(segmented.agent_id(1), AgentId(1));
        assert_eq!(segmented.agent_state(0), world.agents[0]);
        assert_eq!(segmented.agent_state(1), world.agents[1]);
        assert_eq!(segmented.slot_of(AgentId(2)), Some(0));
        assert_eq!(segmented.slot_of(AgentId(1)), Some(1));
        assert_eq!(segmented.slot_of(AgentId(999)), None);
        assert_eq!(segmented.to_agents(), world.agents);
    }

    #[test]
    fn test_world_storage_canonical_hash_parity() {
        let world = make_test_world();
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let h_world = canonical_state_hash(&world).unwrap();
        let h_view = canonical_state_hash_from_storage(
            &world.storage(),
            world.current_day,
            &world.settlements,
        )
        .unwrap();
        let h_world_trait =
            canonical_state_hash_from_storage(&world, world.current_day, &world.settlements)
                .unwrap();
        let h_seg_trait =
            canonical_state_hash_from_storage(&segmented, world.current_day, &world.settlements)
                .unwrap();
        let h_seg_direct = segmented
            .canonical_state_hash(world.current_day, &world.settlements)
            .unwrap();

        assert_eq!(
            h_world, h_view,
            "WorldState vs AosStorageView hash must match"
        );
        assert_eq!(
            h_world, h_world_trait,
            "WorldState vs WorldStorage trait hash must match"
        );
        assert_eq!(
            h_world, h_seg_trait,
            "WorldState vs Segmented WorldStorage hash must match"
        );
        assert_eq!(
            h_world, h_seg_direct,
            "WorldState vs Segmented direct hash must match"
        );
    }

    #[test]
    fn test_world_storage_simulation_hashes_parity() {
        use crate::config::SimConfig;
        use crate::hashing::{canonical_event_hash, canonical_metrics_hash};
        use crate::initialization::initialize_world;
        use crate::runner::{DayExecutionOptions, M0RunContext, run_m0_day};

        let config_toml = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 1.0
initial_food = 25.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
initial_treasury = 5000

[traits]
prod_min = 0.8
prod_max = 1.5
coop_min = 0.2
coop_max = 0.8
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.1
risk_max = 0.5

[environment]
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 5000
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
        let config = SimConfig::parse_and_validate(config_toml).unwrap();
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);

        // Path 1: AoS simulation run for 5 days
        let mut world_aos = initialize_world(&config).unwrap();
        let mut metrics_aos = Vec::new();
        let mut events_aos = Vec::new();

        let opts = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };

        for _ in 0..5 {
            let outcome = run_m0_day(&mut world_aos, &config, &context, &opts).unwrap();
            metrics_aos.push(outcome.metrics.unwrap());
            events_aos.extend(outcome.events);
        }

        let state_hash_aos = canonical_state_hash(&world_aos).unwrap();
        let metrics_hash_aos = canonical_metrics_hash(&metrics_aos).unwrap();
        let event_hash_aos = canonical_event_hash(&events_aos).unwrap();

        // Path 2: Ingest into SegmentedAgentStorage, reconstruct world, advance 5 days
        let world_init = initialize_world(&config).unwrap();
        let segmented = SegmentedAgentStorage::from_agents(&world_init.agents);
        let mut world_seg = segmented.to_world_state(
            world_init.current_day,
            world_init.settlements,
            world_init.initial_money_supply,
        );

        let mut metrics_seg = Vec::new();
        let mut events_seg = Vec::new();

        for _ in 0..5 {
            let outcome = run_m0_day(&mut world_seg, &config, &context, &opts).unwrap();
            metrics_seg.push(outcome.metrics.unwrap());
            events_seg.extend(outcome.events);
        }

        let state_hash_seg = canonical_state_hash(&world_seg).unwrap();
        let metrics_hash_seg = canonical_metrics_hash(&metrics_seg).unwrap();
        let event_hash_seg = canonical_event_hash(&events_seg).unwrap();

        assert_eq!(
            state_hash_aos, state_hash_seg,
            "CanonicalStateHash must match bit-for-bit"
        );
        assert_eq!(
            metrics_hash_aos, metrics_hash_seg,
            "CanonicalMetricsHash must match bit-for-bit"
        );
        assert_eq!(
            event_hash_aos, event_hash_seg,
            "CanonicalEventHash must match bit-for-bit"
        );

        // Also verify storage hash on final world state matches
        let state_hash_from_storage = canonical_state_hash_from_storage(
            &world_seg.storage(),
            world_seg.current_day,
            &world_seg.settlements,
        )
        .unwrap();
        assert_eq!(
            state_hash_aos, state_hash_from_storage,
            "CanonicalStateHash from WorldStorage must match bit-for-bit"
        );
    }
}
