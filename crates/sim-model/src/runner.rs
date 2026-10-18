//! Full-Day M0 Reference Runner (Milestone M0-16A).
//!
//! Deterministically composes M0 Phases 1 through 11 in immutable sequential order:
//! - Phase 1: Environment Regrowth
//! - Phase 2: Biological Degradation
//! - Phase 3: Observation & Normalized Feature Extraction
//! - Phase 4: Intent Generation (Action Selection & Intent Formulation)
//! - Phase 5: Locality Partitioning
//! - Phase 6A: Work Resolution
//! - Phase 6B: Targeted Interaction Resolution
//! - Phase 7: Settlement Market Clearance
//! - Phase 8: Institutional Welfare Distribution
//! - Phase 9: Mortality Status Commitment
//! - Phase 10: Macroscopic Metrics & Observation Hook
//! - Phase 11: Canonical Snapshot & Event Stream Flush

use crate::config::{
    SimConfig, SimulationProfile, SimulationRunManifest, SimulationRunnerSelection,
};
use crate::decision::{
    DecisionError, PrimaryActionChoice, phase4_primary_action_selection_into,
    phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_rayon,
};
use crate::events::{
    Event, EventBuffer, EventError, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    event_from_daily_metrics, events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    phase11_flush_events,
};
use crate::features::{
    AgentFeatures, Phase3Error, Phase3ScarcityScratch, phase3_observation_and_features_into,
    phase3_observation_and_features_storage_with_scratch,
};
use crate::intents::{
    CrossPartitionCountError, Intent, IntentError, M3Phase4GlobalEligibility,
    M3Phase4SlotProjection, Phase4CandidateIndexScratch,
    count_cross_partition_interactions_per_day, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_into, generate_intents_m3_storage_into,
    generate_intents_m3_storage_with_global_eligibility_into,
    generate_intents_storage_with_candidate_index,
    generate_intents_storage_with_candidate_index_rayon, generate_intents_storage_with_scratch,
    phase4_generate_intents_into,
};
use crate::m3p::{
    M3pDailyMetrics, M3pDayOutcome, M3pEvent, M3pEventRecord, M3pIncrementalTopologyScratch,
    M3pReferenceState, Phase9bResult, Phase9bRunParameters, apply_phase9b_storage,
    apply_phase9b_storage_incremental, encode_m3p_snapshot,
};
use crate::metrics::{
    DailyMetrics, M3DailyMetrics, Phase10Error, phase10_observe_storage_with_scratch,
    phase10_observe_with_scratch,
};
use crate::partitioning::{
    Phase5Error, Phase5PartitionScratch, phase5_partition_intents_baseline,
    phase5_partition_intents_from_vec_with_scratch,
};
use crate::phases::{
    Phase9Error, phase1_resource_regrowth, phase2_biological_degradation,
    phase9_mortality_commitment,
};
use crate::relationship_graph::{RelationshipGraph, RelationshipGraphError};
use crate::resolution::{
    M3Phase6BParallelScratch, M3Phase6BStageTimings, M3Phase6BStorageScratch, Phase6AError,
    Phase6BError, Phase6BResolutionScratch, Phase7Error, Phase8Error, Phase8WelfareScratch,
    phase6a_work_resolution, phase6a_work_resolution_storage, phase6b_targeted_resolution,
    phase6b_targeted_resolution_m3, phase6b_targeted_resolution_storage_baseline,
    phase6b_targeted_resolution_storage_m3_parallel_with_scratch,
    phase6b_targeted_resolution_storage_m3_parallel_with_timing,
    phase6b_targeted_resolution_storage_m3_with_scratch,
    phase6b_targeted_resolution_storage_m3_with_timing,
    phase6b_targeted_resolution_storage_ordered_with_scratch,
    phase7_market_clearance_storage_with_config, phase7_market_clearance_with_config,
    phase8_welfare_distribution_storage_full_scan,
    phase8_welfare_distribution_storage_with_config_and_scratch,
    phase8_welfare_distribution_with_config,
};
use crate::snapshot::{
    CanonicalSnapshot, SNAPSHOT_SCHEMA_VERSION, SnapshotError, SnapshotMetadata, encode_snapshot,
};
use crate::state::{AgentDynamicSoAScratch, HybridWorldState, WorldState};
use crate::storage::SegmentedAgentStorage;
use rayon::ThreadPool;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, SimulationDay};
use std::collections::HashMap;
use std::num::NonZeroUsize;

fn record_phase6_event_count(
    counts: &mut Vec<(u16, u64)>,
    slots: &mut HashMap<GroupId, usize>,
    group_id: GroupId,
    count: u64,
) -> u64 {
    if let Some(&slot) = slots.get(&group_id) {
        let previous = counts[slot].1;
        counts[slot].1 += count;
        previous
    } else {
        let slot = counts.len();
        counts.push((group_id.0, count));
        slots.insert(group_id, slot);
        0
    }
}

#[cfg(test)]
mod phase6_event_count_tests {
    use super::record_phase6_event_count;
    use sim_core::GroupId;
    use std::collections::HashMap;

    #[test]
    fn indexed_offsets_match_first_seen_vector_order() {
        let mut counts = Vec::new();
        let mut slots = HashMap::new();

        assert_eq!(
            record_phase6_event_count(&mut counts, &mut slots, GroupId(7), 3),
            0
        );
        assert_eq!(
            record_phase6_event_count(&mut counts, &mut slots, GroupId(2), 5),
            0
        );
        assert_eq!(
            record_phase6_event_count(&mut counts, &mut slots, GroupId(7), 4),
            3
        );
        assert_eq!(
            record_phase6_event_count(&mut counts, &mut slots, GroupId(2), 6),
            5
        );
        assert_eq!(counts, [(7, 7), (2, 11)]);
    }
}

/// Default model version string used for canonical snapshot metadata.
pub const DEFAULT_MODEL_VERSION: &str = "0.1.0";

/// Default configuration version string used for canonical snapshot metadata.
pub const DEFAULT_CONFIG_VERSION: &str = "1.0";

/// Compact immutable context defining deterministic simulation coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct M0RunContext {
    pub master_seed: u64,
    pub replicate_id: u32,
}

/// Context selected by the explicit feature-set dispatcher.
pub enum ConfiguredStorageRunContext {
    M2(M0RunContext),
    M3(M3StorageRunContext),
    M3p(M3pStorageRunContext),
}

/// Typed result from one of the existing legacy storage/reference runner families.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfiguredDayOutcome {
    M2(DayOutcome),
    M3(M3DayOutcome),
    M3p(M3pDayOutcome),
}

/// Effective feature manifest and the outcomes produced by the selected existing runner.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfiguredRunOutcome {
    pub manifest: SimulationRunManifest,
    pub days: Vec<ConfiguredDayOutcome>,
}

impl M0RunContext {
    #[inline]
    pub const fn new(master_seed: u64, replicate_id: u32) -> Self {
        Self {
            master_seed,
            replicate_id,
        }
    }
}

/// Per-day telemetry and observation switches.
///
/// These switches control only telemetry emission and NEVER alter the authoritative
/// simulation state trajectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DayExecutionOptions {
    pub metrics_enabled: bool,
    pub events_enabled: bool,
    pub snapshot_boundary: bool,
}

impl Default for DayExecutionOptions {
    fn default() -> Self {
        Self {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        }
    }
}

/// Structured observation output of a single completed simulation day.
#[derive(Debug, Clone, PartialEq)]
pub struct DayOutcome {
    pub executed_day: u32,
    pub metrics: Option<DailyMetrics>,
    pub events: Vec<EventRecord>,
    pub snapshot: Option<CanonicalSnapshot>,
}

/// Runtime-only graph and run coordinates for the M3 reference profile.
#[derive(Debug, Clone)]
pub struct M3ReferenceContext {
    run_context: M0RunContext,
    relationship_graph: RelationshipGraph,
}

/// Runtime graph, slot projection, and reusable scratch for M3 storage authority.
#[derive(Debug)]
pub struct M3StorageRunContext {
    run_context: M0RunContext,
    relationship_graph: RelationshipGraph,
    phase4_projection: M3Phase4SlotProjection,
    storage_memberships: Vec<(AgentId, GroupId)>,
    relationship_degree_k: u32,
    p_local_bits: u32,
    features_scratch: Vec<AgentFeatures>,
    choices_scratch: Vec<PrimaryActionChoice>,
    intents_scratch: Vec<Intent>,
    candidate_scratch: Vec<AgentId>,
    phase3_scratch: Phase3ScarcityScratch,
    phase5_scratch: Phase5PartitionScratch,
    phase6b_scratch: M3Phase6BStorageScratch,
    phase8_scratch: Phase8WelfareScratch,
}

impl M3StorageRunContext {
    /// Builds the relationship graph and slot projection for the current hybrid storage instance.
    pub fn new(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        ensure_m3_storage_profile(config)?;
        config.validate().map_err(|error| {
            M0RunError::InvariantViolation(format!("invalid M3 configuration: {error}"))
        })?;
        if !hybrid_world.is_hybrid() {
            return Err(M0RunError::InvariantViolation(
                "M3 storage runner requires Hybrid Storage Authority".into(),
            ));
        }
        let storage = hybrid_world.segmented_storage.as_ref().ok_or_else(|| {
            M0RunError::InvariantViolation("hybrid mode without segmented storage".into())
        })?;
        let relationship_degree_k = config.interaction.relationship_degree_k.ok_or_else(|| {
            M0RunError::InvariantViolation("M3 profile requires relationship_degree_k".into())
        })?;
        let relationship_degree_k = u32::try_from(relationship_degree_k).map_err(|_| {
            M0RunError::InvariantViolation("M3 relationship_degree_k is outside u32 range".into())
        })?;
        let p_local = config
            .interaction
            .p_local
            .ok_or_else(|| M0RunError::InvariantViolation("M3 profile requires p_local".into()))?;
        let storage_memberships: Vec<_> = storage
            .agent_ids()
            .iter()
            .copied()
            .zip(storage.group_ids().iter().copied())
            .collect();
        if storage_memberships.len() != storage.len() {
            return Err(M0RunError::InvariantViolation(
                "segmented storage agent and group columns have different lengths".into(),
            ));
        }
        let relationship_graph = RelationshipGraph::generate(
            &storage_memberships,
            run_context.master_seed,
            run_context.replicate_id,
            relationship_degree_k,
            p_local,
        )?;
        let phase4_projection = M3Phase4SlotProjection::new(&relationship_graph, storage)?;
        let agent_count = storage.len();
        let settlement_count = hybrid_world.world.settlements.len();

        Ok(Self {
            run_context: *run_context,
            relationship_graph,
            phase4_projection,
            storage_memberships,
            relationship_degree_k,
            p_local_bits: p_local.to_bits(),
            features_scratch: Vec::with_capacity(agent_count),
            choices_scratch: Vec::with_capacity(agent_count),
            intents_scratch: Vec::with_capacity(agent_count),
            candidate_scratch: Vec::with_capacity(relationship_degree_k as usize),
            phase3_scratch: Phase3ScarcityScratch::with_capacity(settlement_count),
            phase5_scratch: Phase5PartitionScratch::with_capacity(settlement_count),
            phase6b_scratch: M3Phase6BStorageScratch::with_capacity(agent_count),
            phase8_scratch: Phase8WelfareScratch::with_capacity(settlement_count),
        })
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        &self.relationship_graph
    }

    fn validate_for(
        &self,
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
    ) -> Result<(), M0RunError> {
        ensure_m3_storage_profile(config)?;
        config.validate().map_err(|error| {
            M0RunError::InvariantViolation(format!("invalid M3 configuration: {error}"))
        })?;
        if !hybrid_world.is_hybrid() {
            return Err(M0RunError::InvariantViolation(
                "M3 storage runner requires Hybrid Storage Authority".into(),
            ));
        }
        let storage = hybrid_world.segmented_storage.as_ref().ok_or_else(|| {
            M0RunError::InvariantViolation("hybrid mode without segmented storage".into())
        })?;
        let relationship_degree_k = config
            .interaction
            .relationship_degree_k
            .and_then(|degree| u32::try_from(degree).ok());
        let p_local_bits = config.interaction.p_local.map(f32::to_bits);
        if relationship_degree_k != Some(self.relationship_degree_k)
            || p_local_bits != Some(self.p_local_bits)
        {
            return Err(M0RunError::InvariantViolation(
                "M3 storage context does not match relationship graph configuration".into(),
            ));
        }
        let memberships_match = storage.len() == self.storage_memberships.len()
            && storage
                .agent_ids()
                .iter()
                .copied()
                .zip(storage.group_ids().iter().copied())
                .eq(self.storage_memberships.iter().copied());
        if !memberships_match {
            return Err(M0RunError::InvariantViolation(
                "M3 storage context must be rebuilt after storage construction or replacement"
                    .into(),
            ));
        }
        Ok(())
    }
}

/// Runtime graph projection and reusable scratch for M3P storage authority.
#[derive(Debug)]
pub struct M3pStorageRunContext {
    run_context: M0RunContext,
    relationship_graph: RelationshipGraph,
    phase4_projection: M3Phase4SlotProjection,
    storage_memberships: Vec<(AgentId, GroupId)>,
    next_agent_id: u64,
    relationship_degree_k: u32,
    p_local_bits: u32,
    features_scratch: Vec<AgentFeatures>,
    choices_scratch: Vec<PrimaryActionChoice>,
    intents_scratch: Vec<Intent>,
    candidate_scratch: Vec<AgentId>,
    phase3_scratch: Phase3ScarcityScratch,
    phase5_scratch: Phase5PartitionScratch,
    phase6b_scratch: M3Phase6BStorageScratch,
    phase8_scratch: Phase8WelfareScratch,
    phase10_indices: Vec<usize>,
    phase10_settlement_indices: Vec<usize>,
}

impl M3pStorageRunContext {
    /// Creates an M3P graph and projection for an initial hybrid storage instance.
    pub fn new(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        let storage = m3p_storage_from_hybrid(hybrid_world)?;
        let world = m3p_world_from_storage(&hybrid_world.world, storage);
        let reference = M3pReferenceState::new(world, config, run_context)?;
        Self::from_graph_and_cursor(
            hybrid_world,
            config,
            run_context,
            reference.relationship_graph().clone(),
            reference.next_agent_id(),
        )
    }

    /// Rebuilds transient execution projection from a restored M3P reference state.
    pub fn from_reference_state(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
        reference: &M3pReferenceState,
    ) -> Result<Self, M0RunError> {
        if reference.run_context() != *run_context {
            return Err(M0RunError::InvariantViolation(
                "restored M3P storage state does not match run coordinates".into(),
            ));
        }
        let storage = m3p_storage_from_hybrid(hybrid_world)?;
        let world = m3p_world_from_storage(&hybrid_world.world, storage);
        M3pReferenceState::from_parts(
            world,
            reference.relationship_graph().clone(),
            reference.next_agent_id(),
            config,
            run_context,
        )?;
        Self::from_graph_and_cursor(
            hybrid_world,
            config,
            run_context,
            reference.relationship_graph().clone(),
            reference.next_agent_id(),
        )
    }

    fn from_graph_and_cursor(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
        relationship_graph: RelationshipGraph,
        next_agent_id: u64,
    ) -> Result<Self, M0RunError> {
        ensure_m3p_storage_profile(config)?;
        config.validate().map_err(|error| {
            M0RunError::InvariantViolation(format!("invalid M3P configuration: {error}"))
        })?;
        let storage = m3p_storage_from_hybrid(hybrid_world)?;
        let relationship_degree_k = config
            .interaction
            .relationship_degree_k
            .and_then(|degree| u32::try_from(degree).ok())
            .ok_or_else(|| {
                M0RunError::InvariantViolation(
                    "M3P profile requires a valid relationship_degree_k".into(),
                )
            })?;
        let p_local = config
            .interaction
            .p_local
            .ok_or_else(|| M0RunError::InvariantViolation("M3P profile requires p_local".into()))?;
        let memberships = m3p_storage_memberships(storage);
        let world = m3p_world_from_storage(&hybrid_world.world, storage);
        M3pReferenceState::from_parts(
            world,
            relationship_graph.clone(),
            next_agent_id,
            config,
            run_context,
        )?;
        let phase4_projection = M3Phase4SlotProjection::new(&relationship_graph, storage)?;
        let agent_count = storage.len();
        let settlement_count = hybrid_world.world.settlements.len();
        Ok(Self {
            run_context: *run_context,
            relationship_graph,
            phase4_projection,
            storage_memberships: memberships,
            next_agent_id,
            relationship_degree_k,
            p_local_bits: p_local.to_bits(),
            features_scratch: Vec::with_capacity(agent_count),
            choices_scratch: Vec::with_capacity(agent_count),
            intents_scratch: Vec::with_capacity(agent_count),
            candidate_scratch: Vec::with_capacity(relationship_degree_k as usize),
            phase3_scratch: Phase3ScarcityScratch::with_capacity(settlement_count),
            phase5_scratch: Phase5PartitionScratch::with_capacity(settlement_count),
            phase6b_scratch: M3Phase6BStorageScratch::with_capacity(agent_count),
            phase8_scratch: Phase8WelfareScratch::with_capacity(settlement_count),
            phase10_indices: Vec::with_capacity(agent_count),
            phase10_settlement_indices: Vec::with_capacity(settlement_count),
        })
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        &self.relationship_graph
    }

    pub const fn next_agent_id(&self) -> u64 {
        self.next_agent_id
    }

    fn refresh_after_phase9b(
        &mut self,
        storage: &SegmentedAgentStorage,
        phase9b: &Phase9bResult,
    ) -> Result<(), M0RunError> {
        if !phase9b.births.is_empty() || phase9b.compacted {
            self.phase4_projection
                .rebuild(&self.relationship_graph, storage)?;
            self.storage_memberships = m3p_storage_memberships(storage);
        }
        Ok(())
    }

    fn refresh_incremental_after_phase9b(
        &mut self,
        storage: &SegmentedAgentStorage,
        phase9b: &Phase9bResult,
    ) -> Result<(), M0RunError> {
        if phase9b.compacted {
            self.phase4_projection
                .rebuild(&self.relationship_graph, storage)?;
            self.storage_memberships = m3p_storage_memberships(storage);
        } else {
            self.storage_memberships.extend(
                phase9b
                    .births
                    .iter()
                    .map(|birth| (birth.child, birth.group)),
            );
        }
        Ok(())
    }

    fn validate_for(
        &self,
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
    ) -> Result<(), M0RunError> {
        ensure_m3p_storage_profile(config)?;
        config.validate().map_err(|error| {
            M0RunError::InvariantViolation(format!("invalid M3P configuration: {error}"))
        })?;
        let storage = m3p_storage_from_hybrid(hybrid_world)?;
        if config
            .interaction
            .relationship_degree_k
            .and_then(|k| u32::try_from(k).ok())
            != Some(self.relationship_degree_k)
            || config.interaction.p_local.map(f32::to_bits) != Some(self.p_local_bits)
        {
            return Err(M0RunError::InvariantViolation(
                "M3P storage context does not match relationship graph configuration".into(),
            ));
        }
        if m3p_storage_memberships(storage) != self.storage_memberships {
            return Err(M0RunError::InvariantViolation(
                "M3P storage context is stale after storage construction or replacement".into(),
            ));
        }
        let world = m3p_world_from_storage(&hybrid_world.world, storage);
        M3pReferenceState::from_parts(
            world,
            self.relationship_graph.clone(),
            self.next_agent_id,
            config,
            &self.run_context,
        )?;
        Ok(())
    }
}

/// Explicit opt-in storage context for incremental M3P topology maintenance.
#[derive(Debug)]
pub struct M3pIncrementalTopologyStorageRunContext {
    storage_context: M3pStorageRunContext,
    phase9b_scratch: M3pIncrementalTopologyScratch,
}

impl M3pIncrementalTopologyStorageRunContext {
    pub fn new(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        Ok(Self {
            storage_context: M3pStorageRunContext::new(hybrid_world, config, run_context)?,
            phase9b_scratch: M3pIncrementalTopologyScratch::default(),
        })
    }

    pub fn from_reference_state(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
        reference: &M3pReferenceState,
    ) -> Result<Self, M0RunError> {
        Ok(Self {
            storage_context: M3pStorageRunContext::from_reference_state(
                hybrid_world,
                config,
                run_context,
                reference,
            )?,
            phase9b_scratch: M3pIncrementalTopologyScratch::default(),
        })
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        self.storage_context.relationship_graph()
    }

    pub const fn next_agent_id(&self) -> u64 {
        self.storage_context.next_agent_id()
    }
}

fn ensure_m3p_storage_profile(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile == SimulationProfile::M3p {
        if config.effective_systems().propagation {
            Err(M0RunError::InvariantViolation(
                "propagation-enabled configuration requires an M3-11 runner".into(),
            ))
        } else {
            Ok(())
        }
    } else {
        Err(M0RunError::InvariantViolation(
            "M3P storage runner requires profile = \"m3p\"".into(),
        ))
    }
}

fn m3p_storage_from_hybrid(
    hybrid_world: &HybridWorldState,
) -> Result<&SegmentedAgentStorage, M0RunError> {
    if !hybrid_world.is_hybrid() {
        return Err(M0RunError::InvariantViolation(
            "M3P storage runner requires Hybrid Storage Authority".into(),
        ));
    }
    hybrid_world.segmented_storage.as_ref().ok_or_else(|| {
        M0RunError::InvariantViolation("hybrid mode without segmented storage".into())
    })
}

fn m3p_storage_memberships(storage: &SegmentedAgentStorage) -> Vec<(AgentId, GroupId)> {
    storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect()
}

#[cfg(debug_assertions)]
fn assert_m3p_incremental_topology_matches_full_rebuild(
    storage: &SegmentedAgentStorage,
    context: &M3pStorageRunContext,
) {
    let graph = &context.relationship_graph;
    let rows = graph
        .source_ids()
        .iter()
        .map(|&source_id| {
            (
                source_id,
                graph
                    .neighbors_of(source_id)
                    .expect("canonical graph source has a row")
                    .to_vec(),
            )
        })
        .collect();
    let rebuilt_graph =
        RelationshipGraph::from_canonical_rows(rows, &m3p_storage_memberships(storage))
            .expect("incremental M3P graph rebuild validates");
    assert_eq!(
        graph, &rebuilt_graph,
        "incremental CSR differs from rebuild"
    );
    assert_eq!(
        graph.csr_hash(),
        rebuilt_graph.csr_hash(),
        "incremental CSR hash differs from rebuild"
    );
    let rebuilt_projection = M3Phase4SlotProjection::new(&rebuilt_graph, storage)
        .expect("incremental M3P projection rebuild validates");
    assert_eq!(
        context.phase4_projection, rebuilt_projection,
        "incremental Phase 4 projection differs from rebuild"
    );
}

fn m3p_world_from_storage(world: &WorldState, storage: &SegmentedAgentStorage) -> WorldState {
    let mut copy = world.clone();
    copy.agents = storage.to_agents();
    copy
}

#[derive(Debug, Default)]
struct M3SearchReductionScratch {
    phase4_global_eligibility: M3Phase4GlobalEligibility,
}

impl M3SearchReductionScratch {
    fn update_phase4_global_eligibility(
        &mut self,
        storage: &SegmentedAgentStorage,
        starvation_threshold: f32,
    ) {
        let alive = storage.alive();
        let health = storage.health();
        let food = storage.food();
        let mut eligibility = M3Phase4GlobalEligibility::default();
        for slot in 0..storage.len() {
            if alive[slot] && health[slot] > 0.0 {
                eligibility.give_food |= food[slot] < starvation_threshold;
                eligibility.steal_food |= food[slot] > 0.0;
                if eligibility.give_food && eligibility.steal_food {
                    break;
                }
            }
        }
        self.phase4_global_eligibility = eligibility;
    }
}

/// Caller-owned state for the explicit serial search-reduction M3 runner.
#[derive(Debug)]
pub struct M3SearchReductionRunContext {
    storage_context: M3StorageRunContext,
    scratch: M3SearchReductionScratch,
}

impl M3SearchReductionRunContext {
    /// Builds a fresh graph, slot projection, and reusable search-reduction scratch.
    pub fn new(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        Ok(Self {
            storage_context: M3StorageRunContext::new(hybrid_world, config, run_context)?,
            scratch: M3SearchReductionScratch::default(),
        })
    }

    fn validate_for(
        &self,
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
    ) -> Result<(), M0RunError> {
        self.storage_context.validate_for(hybrid_world, config)
    }
}

/// Runtime wrapper for explicitly opted-in parallel M3 storage Phase 6B.
#[derive(Debug)]
pub struct M3ParallelStorageRunContext {
    storage_context: M3StorageRunContext,
    phase6b_scratch: M3Phase6BParallelScratch,
}

impl M3ParallelStorageRunContext {
    /// Builds a fresh M3 graph, slot projection, and reusable parallel Phase 6B scratch.
    pub fn new(
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        let storage_context = M3StorageRunContext::new(hybrid_world, config, run_context)?;
        let agent_count = storage_context.storage_memberships.len();
        let partition_count = hybrid_world.world.settlements.len();
        let phase6b_scratch =
            M3Phase6BParallelScratch::with_capacity(agent_count, partition_count, agent_count);
        Ok(Self {
            storage_context,
            phase6b_scratch,
        })
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        self.storage_context.relationship_graph()
    }

    fn validate_for(
        &self,
        hybrid_world: &HybridWorldState,
        config: &SimConfig,
    ) -> Result<(), M0RunError> {
        self.storage_context.validate_for(hybrid_world, config)
    }
}

impl M3ReferenceContext {
    /// Builds the static relationship graph for the initialized population.
    pub fn new(
        world: &WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        ensure_m3_profile(config)?;
        config.validate().map_err(|error| {
            M0RunError::InvariantViolation(format!("invalid M3 configuration: {error}"))
        })?;
        let degree_k = config.interaction.relationship_degree_k.ok_or_else(|| {
            M0RunError::InvariantViolation("M3 profile requires relationship_degree_k".into())
        })?;
        let degree_k = u32::try_from(degree_k).map_err(|_| {
            M0RunError::InvariantViolation("M3 relationship_degree_k is outside u32 range".into())
        })?;
        let p_local = config
            .interaction
            .p_local
            .ok_or_else(|| M0RunError::InvariantViolation("M3 profile requires p_local".into()))?;
        let memberships: Vec<_> = world
            .agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id))
            .collect();
        let relationship_graph = RelationshipGraph::generate(
            &memberships,
            run_context.master_seed,
            run_context.replicate_id,
            degree_k,
            p_local,
        )?;
        Ok(Self {
            run_context: *run_context,
            relationship_graph,
        })
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        &self.relationship_graph
    }
}

/// Observation output of one completed M3 reference day.
#[derive(Debug, Clone, PartialEq)]
pub struct M3DayOutcome {
    pub executed_day: u32,
    pub metrics: Option<M3DailyMetrics>,
    pub events: Vec<EventRecord>,
    pub snapshot: Option<CanonicalSnapshot>,
}

/// Unified error enum for M0 daily orchestration failures.
#[derive(Debug, Clone, PartialEq)]
pub enum M0RunError {
    Phase3(Phase3Error),
    Phase4Decision(DecisionError),
    Phase4Intent(IntentError),
    Phase5(Phase5Error),
    Phase6A(Phase6AError),
    Phase6B(Phase6BError),
    Phase7(Phase7Error),
    Phase8(Phase8Error),
    Phase9(Phase9Error),
    Phase10(Phase10Error),
    Phase11Snapshot(SnapshotError),
    Phase11Event(EventError),
    DayOverflow,
    InvariantViolation(String),
}

impl std::fmt::Display for M0RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Phase3(err) => write!(f, "Phase 3 feature extraction error: {}", err),
            Self::Phase4Decision(err) => write!(f, "Phase 4 decision error: {}", err),
            Self::Phase4Intent(err) => write!(f, "Phase 4 intent error: {}", err),
            Self::Phase5(err) => write!(f, "Phase 5 partitioning error: {}", err),
            Self::Phase6A(err) => write!(f, "Phase 6A work resolution error: {}", err),
            Self::Phase6B(err) => write!(f, "Phase 6B targeted resolution error: {}", err),
            Self::Phase7(err) => write!(f, "Phase 7 market clearance error: {}", err),
            Self::Phase8(err) => write!(f, "Phase 8 welfare distribution error: {}", err),
            Self::Phase9(err) => write!(f, "Phase 9 mortality commitment error: {}", err),
            Self::Phase10(err) => write!(f, "Phase 10 metrics observation error: {}", err),
            Self::Phase11Snapshot(err) => write!(f, "Phase 11 snapshot error: {}", err),
            Self::Phase11Event(err) => write!(f, "Phase 11 event flush error: {}", err),
            Self::DayOverflow => write!(f, "simulation day overflowed u32 representation"),
            Self::InvariantViolation(msg) => write!(f, "runner invariant violation: {}", msg),
        }
    }
}

impl std::error::Error for M0RunError {}

fn ensure_m2_profile(config: &SimConfig) -> Result<(), M0RunError> {
    if config.effective_systems().propagation {
        return Err(M0RunError::InvariantViolation(
            "propagation-enabled configuration requires an M3-11 runner".into(),
        ));
    }
    match config.profile {
        SimulationProfile::M2 => Ok(()),
        SimulationProfile::M3 => Err(M0RunError::InvariantViolation(
            "profile = \"m3\" requires the M3 reference runner".into(),
        )),
        SimulationProfile::M3p => Err(M0RunError::InvariantViolation(
            "profile = \"m3p\" requires the M3P reference runner".into(),
        )),
    }
}

fn ensure_m3_profile(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile == SimulationProfile::M3 && !config.effective_systems().propagation {
        Ok(())
    } else {
        Err(M0RunError::InvariantViolation(
            "M3 reference runner requires profile = \"m3\"".into(),
        ))
    }
}

fn ensure_m3_storage_profile(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile == SimulationProfile::M3 && !config.effective_systems().propagation {
        Ok(())
    } else {
        Err(M0RunError::InvariantViolation(
            "M3 storage runner requires profile = \"m3\"".into(),
        ))
    }
}

impl From<CrossPartitionCountError> for M0RunError {
    fn from(error: CrossPartitionCountError) -> Self {
        Self::InvariantViolation(error.to_string())
    }
}

impl From<RelationshipGraphError> for M0RunError {
    fn from(error: RelationshipGraphError) -> Self {
        Self::InvariantViolation(error.to_string())
    }
}

impl From<Phase3Error> for M0RunError {
    fn from(err: Phase3Error) -> Self {
        Self::Phase3(err)
    }
}

impl From<DecisionError> for M0RunError {
    fn from(err: DecisionError) -> Self {
        Self::Phase4Decision(err)
    }
}

impl From<IntentError> for M0RunError {
    fn from(err: IntentError) -> Self {
        Self::Phase4Intent(err)
    }
}

impl From<Phase5Error> for M0RunError {
    fn from(err: Phase5Error) -> Self {
        Self::Phase5(err)
    }
}

impl From<Phase6AError> for M0RunError {
    fn from(err: Phase6AError) -> Self {
        Self::Phase6A(err)
    }
}

impl From<Phase6BError> for M0RunError {
    fn from(err: Phase6BError) -> Self {
        Self::Phase6B(err)
    }
}

impl From<Phase7Error> for M0RunError {
    fn from(err: Phase7Error) -> Self {
        Self::Phase7(err)
    }
}

impl From<Phase8Error> for M0RunError {
    fn from(err: Phase8Error) -> Self {
        Self::Phase8(err)
    }
}

impl From<Phase9Error> for M0RunError {
    fn from(err: Phase9Error) -> Self {
        Self::Phase9(err)
    }
}

impl From<Phase10Error> for M0RunError {
    fn from(err: Phase10Error) -> Self {
        Self::Phase10(err)
    }
}

impl From<SnapshotError> for M0RunError {
    fn from(err: SnapshotError) -> Self {
        Self::Phase11Snapshot(err)
    }
}

impl From<EventError> for M0RunError {
    fn from(err: EventError) -> Self {
        Self::Phase11Event(err)
    }
}

/// Executes exactly one full deterministic simulation day (Phases 1 through 11).
///
/// Allocates local scratch buffers and delegates to [`run_m0_day_with_scratch`].
pub fn run_m0_day(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<DayOutcome, M0RunError> {
    let mut features_scratch = Vec::with_capacity(world.agents.len());
    let mut choices_scratch = Vec::with_capacity(world.agents.len());
    let mut intents_scratch = Vec::with_capacity(world.agents.len());
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
    run_m0_day_with_scratch(
        world,
        config,
        context,
        options,
        &mut features_scratch,
        &mut choices_scratch,
        &mut intents_scratch,
        &mut metrics_scratch,
    )
}

/// Executes exactly one full deterministic simulation day (Phases 1 through 11)
/// reusing external scratch buffers for Phase 3 features, Phase 4 action choices, Phase 4 intents,
/// and Phase 10 dynamic metrics aggregation.
///
/// Execution rules:
/// 1. Captures `executed_day = world.current_day`.
/// 2. Validates day arithmetic (`next_day = executed_day + 1`).
/// 3. Propagates `context.master_seed` and `context.replicate_id` through `SimConfig`.
/// 4. Executes Phases 1..=9 sequentially against authoritative live state.
/// 5. Collects committed Phase 6..=9 events if `options.events_enabled`.
/// 6. Executes Phase 10 metrics if `options.metrics_enabled`.
/// 7. Executes Phase 11 snapshot if `options.snapshot_boundary` (with resume day `D + 1`).
/// 8. Emits Phase 11 SnapshotEmitted observation event if snapshot created and events enabled.
/// 9. Flushes pending event buffer canonically if `options.events_enabled`.
/// 10. Advances `world.current_day` to `next_day` strictly upon successful Day Complete.
#[allow(clippy::too_many_arguments)]
pub fn run_m0_day_with_scratch(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    metrics_scratch: &mut AgentDynamicSoAScratch,
) -> Result<DayOutcome, M0RunError> {
    ensure_m2_profile(config)?;
    // 1. Capture logical day coordinate
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;

    // 2. Prepare effective config with deterministic context coordinates
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    // 3. Phase 1: Environment Regrowth
    phase1_resource_regrowth(world, &effective_config);

    // 4. Phase 2: Biological Degradation
    phase2_biological_degradation(world, &effective_config);

    // 5. Phase 3: Observation & Normalized Feature Extraction
    phase3_observation_and_features_into(world, &effective_config, features_scratch)?;

    // 6. Phase 4: Intent Generation (Primary Action Selection & Intent Formulation)
    phase4_primary_action_selection_into(
        world,
        &effective_config,
        features_scratch,
        choices_scratch,
    )?;
    phase4_generate_intents_into(world, &effective_config, choices_scratch, intents_scratch)?;

    // 7. Phase 5: Locality Partitioning
    let partitions = phase5_partition_intents_baseline(intents_scratch)?;

    // 8. Phase 6A: Work Resolution
    let work_resolutions = phase6a_work_resolution(world, &partitions)?;

    // 9. Phase 6B: Targeted Interaction Resolution
    let targeted_resolutions = phase6b_targeted_resolution(world, &effective_config, &partitions)?;

    // 10. Phase 7: Settlement Market Clearance
    let market_resolutions =
        phase7_market_clearance_with_config(world, &partitions, &effective_config.economy)?;

    // 11. Phase 8: Institutional Welfare Distribution
    let welfare_resolutions = phase8_welfare_distribution_with_config(world, &effective_config)?;

    // 12. Phase 9: Mortality Status Commitment
    let mortality_resolution = phase9_mortality_commitment(world)?;

    // 13. Staged Event Buffer Assembly
    let mut event_buffer = if options.events_enabled {
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        EventBuffer::with_capacity(estimated_cap)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());

        for w in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                phase6_counts.push((t.group_id.0, targeted_events.len() as u64));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    // 14. Phase 10: Macroscopic Metrics Observation
    let metrics_opt = if options.metrics_enabled {
        let metrics = phase10_observe_with_scratch(world, executed_day, metrics_scratch)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&metrics));
        }
        Some(metrics)
    } else {
        None
    };

    // 15. Phase 11: Canonical Snapshot
    let snapshot_opt = if options.snapshot_boundary {
        let snapshot_metadata = SnapshotMetadata::new(
            next_day, // D + 1 resume cursor: restore resumes at the next unexecuted day
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &snapshot_metadata)?;

        if options.events_enabled {
            // Snapshot event occurrence day is D (the day that produced the emission)
            let snapshot_event = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: snapshot_metadata.day, // carries resume cursor D + 1 in metadata payload
                    master_seed: snapshot_metadata.master_seed,
                    replicate_id: snapshot_metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snapshot_event);
        }

        Some(snapshot)
    } else {
        None
    };

    // 16. Phase 11: Canonical Event Flush
    let flushed_events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };

    // 17. Day Complete: advance authoritative scheduler day cursor
    world.current_day = SimulationDay(next_day);

    Ok(DayOutcome {
        executed_day,
        metrics: metrics_opt,
        events: flushed_events,
        snapshot: snapshot_opt,
    })
}

/// Executes one full M3 reference day, replacing only Phase 4 targeting and Phase 6B ordering.
pub fn run_m3_reference_day(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M3ReferenceContext,
    options: &DayExecutionOptions,
) -> Result<M3DayOutcome, M0RunError> {
    ensure_m3_profile(config)?;
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.run_context.master_seed;
    effective_config.world.replicate_id = context.run_context.replicate_id;

    phase1_resource_regrowth(world, &effective_config);
    phase2_biological_degradation(world, &effective_config);
    let mut features = Vec::with_capacity(world.agents.len());
    phase3_observation_and_features_into(world, &effective_config, &mut features)?;

    let mut choices = Vec::with_capacity(world.agents.len());
    phase4_primary_action_selection_into(world, &effective_config, &features, &mut choices)?;
    let mut intents = Vec::with_capacity(world.agents.len());
    generate_intents_m3_into(
        world,
        &effective_config,
        &choices,
        &context.relationship_graph,
        &mut intents,
    )?;
    let cross_partition_interaction_count =
        count_cross_partition_interactions_per_day(&intents, world)?;

    let partitions = phase5_partition_intents_baseline(&intents)?;
    let work_resolutions = phase6a_work_resolution(world, &partitions)?;
    let targeted_resolutions =
        phase6b_targeted_resolution_m3(world, &effective_config, &partitions)?;
    let market_resolutions =
        phase7_market_clearance_with_config(world, &partitions, &effective_config.economy)?;
    let welfare_resolutions = phase8_welfare_distribution_with_config(world, &effective_config)?;
    let mortality_resolution = phase9_mortality_commitment(world)?;

    let mut event_buffer = if options.events_enabled {
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        EventBuffer::with_capacity(estimated_cap)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, work);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(group_id, _)| *group_id == work.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((work.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, targeted);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(group_id, _)| *group_id == targeted.group_id.0)
            {
                let previous = entry.1;
                entry.1 += targeted_events.len() as u64;
                previous
            } else {
                phase6_counts.push((targeted.group_id.0, targeted_events.len() as u64));
                0
            };
            if offset > 0 {
                for event in &mut targeted_events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for market in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, market));
        }
        for welfare in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, welfare));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    let metrics = if options.metrics_enabled {
        let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let base = phase10_observe_with_scratch(world, executed_day, &mut metrics_scratch)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&base));
        }
        let (cross_partition_edge_count, total_directed_edge_count) =
            context.relationship_graph.edge_ratio_counts();
        Some(M3DailyMetrics {
            metrics: base,
            cross_partition_edge_count,
            total_directed_edge_count,
            cross_partition_interaction_count,
        })
    } else {
        None
    };

    let snapshot = if options.snapshot_boundary {
        let metadata = SnapshotMetadata::new(
            next_day,
            context.run_context.master_seed,
            context.run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &metadata)?;
        if options.events_enabled {
            event_buffer.push(EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            ));
        }
        Some(snapshot)
    } else {
        None
    };

    let events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };
    world.current_day = SimulationDay(next_day);
    Ok(M3DayOutcome {
        executed_day,
        metrics,
        events,
        snapshot,
    })
}

/// Executes consecutive M3 days serially with Hybrid / SegmentedAgentStorage authority.
pub fn run_m3_storage_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<M3DayOutcome>, M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3_storage_day(hybrid_world, config, context, options)?);
    }
    Ok(outcomes)
}

/// Executes consecutive M3P days serially with Hybrid / SegmentedAgentStorage authority.
pub fn run_m3p_storage_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3pStorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<M3pDayOutcome>, M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3p_storage_day(hybrid_world, config, context, options)?);
    }
    Ok(outcomes)
}

/// Executes M3P storage days with the explicit incremental-topology path.
pub fn run_m3p_storage_days_incremental_topology(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3pIncrementalTopologyStorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<M3pDayOutcome>, M0RunError> {
    context.storage_context.validate_for(hybrid_world, config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3p_storage_day_inner(
            hybrid_world,
            config,
            &mut context.storage_context,
            options,
            Some(&mut context.phase9b_scratch),
        )?);
    }
    Ok(outcomes)
}

/// Selects an existing serial storage runner from the validated feature set.
/// Propagation is rejected here until its dedicated M3-11 path is available.
pub fn run_configured_storage_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut ConfiguredStorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<ConfiguredRunOutcome, M0RunError> {
    config.validate().map_err(|error| {
        M0RunError::InvariantViolation(format!("invalid configuration: {error}"))
    })?;
    let manifest = config.run_manifest();
    let outcomes = match (config.runner_selection(), context) {
        (SimulationRunnerSelection::M2, ConfiguredStorageRunContext::M2(run_context)) => {
            run_hybrid_authority_days(hybrid_world, config, run_context, days, options)?
                .into_iter()
                .map(ConfiguredDayOutcome::M2)
                .collect()
        }
        (SimulationRunnerSelection::M3, ConfiguredStorageRunContext::M3(run_context)) => {
            run_m3_storage_days(hybrid_world, config, run_context, days, options)?
                .into_iter()
                .map(ConfiguredDayOutcome::M3)
                .collect()
        }
        (SimulationRunnerSelection::M3p, ConfiguredStorageRunContext::M3p(run_context)) => {
            run_m3p_storage_days(hybrid_world, config, run_context, days, options)?
                .into_iter()
                .map(ConfiguredDayOutcome::M3p)
                .collect()
        }
        (
            SimulationRunnerSelection::M3Propagation | SimulationRunnerSelection::M3pPropagation,
            _,
        ) => {
            return Err(M0RunError::InvariantViolation(
                "propagation-enabled configuration requires an M3-11 runner".into(),
            ));
        }
        _ => {
            return Err(M0RunError::InvariantViolation(
                "configured runner context does not match the effective feature set".into(),
            ));
        }
    };
    Ok(ConfiguredRunOutcome {
        manifest,
        days: outcomes,
    })
}

/// Executes serial M3 storage days with opt-in target-search reduction.
pub fn run_m3_storage_days_with_search_reduction(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3SearchReductionRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<M3DayOutcome>, M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3_storage_day_with_search_reduction(
            hybrid_world,
            config,
            context,
            options,
        )?);
    }
    Ok(outcomes)
}

/// Timing-only wrapper for the serial M3 runner, returning per-day Phase 6B stage durations.
/// It follows the same phase path and produces the same outcomes as `run_m3_storage_days`.
pub fn run_m3_storage_days_with_phase6b_timing(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<(Vec<M3DayOutcome>, Vec<M3Phase6BStageTimings>), M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut phase6b_timings = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3_storage_day_with_phase6b(
            hybrid_world,
            config,
            context,
            options,
            M3StoragePhase6BMode::SerialTimed {
                timings: &mut phase6b_timings,
            },
        )?);
    }
    Ok((outcomes, phase6b_timings))
}

/// Executes consecutive M3 storage days with explicit caller-owned Rayon Phase 6B execution.
pub fn run_m3_storage_days_with_parallel_phase6b(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3ParallelStorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<Vec<M3DayOutcome>, M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let M3ParallelStorageRunContext {
        storage_context,
        phase6b_scratch,
    } = context;
    let mut outcomes = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3_storage_day_with_phase6b(
            hybrid_world,
            config,
            storage_context,
            options,
            M3StoragePhase6BMode::Parallel {
                scratch: phase6b_scratch,
                pool,
                chunk_size,
            },
        )?);
    }
    Ok(outcomes)
}

/// Timing-only wrapper for the parallel M3 runner, returning per-day Phase 6B stage durations.
/// It follows the same phase path and produces the same outcomes as the untimed entry point.
pub fn run_m3_storage_days_with_parallel_phase6b_timing(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3ParallelStorageRunContext,
    days: u32,
    options: &DayExecutionOptions,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<(Vec<M3DayOutcome>, Vec<M3Phase6BStageTimings>), M0RunError> {
    context.validate_for(hybrid_world, config)?;
    let M3ParallelStorageRunContext {
        storage_context,
        phase6b_scratch,
    } = context;
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut phase6b_timings = Vec::with_capacity(days as usize);
    for _ in 0..days {
        outcomes.push(run_m3_storage_day_with_phase6b(
            hybrid_world,
            config,
            storage_context,
            options,
            M3StoragePhase6BMode::ParallelTimed {
                scratch: phase6b_scratch,
                pool,
                chunk_size,
                timings: &mut phase6b_timings,
            },
        )?);
    }
    Ok((outcomes, phase6b_timings))
}

fn run_m3_storage_day(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
    options: &DayExecutionOptions,
) -> Result<M3DayOutcome, M0RunError> {
    run_m3_storage_day_with_phase6b(
        hybrid_world,
        config,
        context,
        options,
        M3StoragePhase6BMode::Serial,
    )
}

fn run_m3p_storage_day(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3pStorageRunContext,
    options: &DayExecutionOptions,
) -> Result<M3pDayOutcome, M0RunError> {
    run_m3p_storage_day_inner(hybrid_world, config, context, options, None)
}

fn run_m3p_storage_day_inner(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3pStorageRunContext,
    options: &DayExecutionOptions,
    mut incremental_topology_scratch: Option<&mut M3pIncrementalTopologyScratch>,
) -> Result<M3pDayOutcome, M0RunError> {
    let HybridWorldState {
        world,
        segmented_storage,
        authority_mode: _,
    } = hybrid_world;
    let storage = segmented_storage
        .as_mut()
        .ok_or_else(|| M0RunError::InvariantViolation("hybrid mode without storage".into()))?;
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.run_context.master_seed;
    effective_config.world.replicate_id = context.run_context.replicate_id;

    phase1_resource_regrowth(world, &effective_config);
    storage.phase2_degradation_with_config(&effective_config);

    context.features_scratch.clear();
    phase3_observation_and_features_storage_with_scratch(
        storage,
        &world.settlements,
        &effective_config,
        &mut context.features_scratch,
        &mut context.phase3_scratch,
    )?;

    context.choices_scratch.clear();
    phase4_primary_action_selection_storage_into(
        storage,
        world.current_day,
        &effective_config,
        &context.features_scratch,
        &mut context.choices_scratch,
    )?;

    context.intents_scratch.clear();
    generate_intents_m3_storage_into(
        storage,
        world.current_day,
        &effective_config,
        &context.choices_scratch,
        &context.phase4_projection,
        &mut context.candidate_scratch,
        &mut context.intents_scratch,
    )?;
    let cross_partition_interaction_count =
        count_cross_partition_interactions_storage_per_day(&context.intents_scratch, storage)?;

    let partitions = phase5_partition_intents_from_vec_with_scratch(
        &mut context.intents_scratch,
        &mut context.phase5_scratch,
    )?;
    let work_resolutions =
        phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
    let targeted_resolutions = phase6b_targeted_resolution_storage_m3_with_scratch(
        storage,
        &world.settlements,
        world.current_day,
        &effective_config,
        &partitions,
        &mut context.phase6b_scratch,
    )?;
    let market_resolutions = phase7_market_clearance_storage_with_config(
        storage,
        &mut world.settlements,
        &partitions,
        &effective_config.economy,
    )?;
    let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
        storage,
        &mut world.settlements,
        &effective_config,
        &mut context.phase8_scratch,
    )?;
    let mortality_resolution = storage.phase9_mortality_commitment()?;
    let phase9b = if let Some(scratch) = incremental_topology_scratch.as_deref_mut() {
        apply_phase9b_storage_incremental(
            storage,
            world,
            &mut context.relationship_graph,
            &mut context.phase4_projection,
            &mut context.next_agent_id,
            Phase9bRunParameters {
                config: &effective_config,
                run_context: &context.run_context,
                day: executed_day,
                relationship_degree_k: context.relationship_degree_k,
                p_local_bits: context.p_local_bits,
            },
            scratch,
        )?
    } else {
        apply_phase9b_storage(
            storage,
            world,
            &mut context.relationship_graph,
            &mut context.next_agent_id,
            &effective_config,
            &context.run_context,
            executed_day,
        )?
    };
    if incremental_topology_scratch.is_some() {
        context.refresh_incremental_after_phase9b(storage, &phase9b)?;
        #[cfg(debug_assertions)]
        if phase9b.compacted || executed_day.is_multiple_of(16) {
            assert_m3p_incremental_topology_matches_full_rebuild(storage, context);
        }
    } else {
        context.refresh_after_phase9b(storage, &phase9b)?;
    }

    let mut event_buffer = if options.events_enabled {
        EventBuffer::with_capacity(storage.len().saturating_mul(2) + world.settlements.len() + 4)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );
        for work in &work_resolutions {
            let events = events_from_work_resolution(executed_day, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                events.len() as u64,
            );
            event_buffer.push_all(events);
        }
        for targeted in &targeted_resolutions {
            let mut events = events_from_targeted_resolution(executed_day, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                events.len() as u64,
            );
            if offset > 0 {
                for event in &mut events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(events);
        }
        for market in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, market));
        }
        for welfare in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, welfare));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    let metrics = if options.metrics_enabled {
        let base = phase10_observe_storage_with_scratch(
            storage,
            &world.settlements,
            executed_day,
            &mut context.phase10_indices,
            &mut context.phase10_settlement_indices,
        )?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&base));
        }
        let (cross_partition_edge_count, total_directed_edge_count) =
            context.relationship_graph.edge_ratio_counts();
        Some(M3pDailyMetrics {
            m3_metrics: M3DailyMetrics {
                metrics: base,
                cross_partition_edge_count,
                total_directed_edge_count,
                cross_partition_interaction_count,
            },
            births: phase9b.births.len() as u64,
            removed: phase9b.removed,
            replaced_edges: phase9b.replaced_edges,
            live_population: storage.alive().iter().filter(|&&alive| alive).count() as u64,
            next_agent_id: context.next_agent_id,
        })
    } else {
        None
    };

    let snapshot = if options.snapshot_boundary {
        let snapshot_world = m3p_world_from_storage(world, storage);
        let snapshot_state = M3pReferenceState::from_parts(
            snapshot_world,
            context.relationship_graph.clone(),
            context.next_agent_id,
            &effective_config,
            &context.run_context,
        )?;
        let metadata = SnapshotMetadata::new(
            next_day,
            context.run_context.master_seed,
            context.run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_m3p_snapshot(&snapshot_state, &metadata)?;
        if options.events_enabled {
            event_buffer.push(EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: crate::m3p::M3P_SNAPSHOT_SCHEMA_VERSION,
                }),
            ));
        }
        Some(snapshot)
    } else {
        None
    };

    let mut events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
            .into_iter()
            .map(|record| M3pEventRecord {
                key: record.key,
                event: M3pEvent::Existing(record.event),
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    if options.events_enabled {
        let mortality_event_count = mortality_resolution.newly_deceased.len() as u64;
        for (sequence, birth) in phase9b.births.iter().enumerate() {
            events.push(M3pEventRecord {
                key: EventKey::new(
                    executed_day,
                    9,
                    GLOBAL_PARTITION_KEY,
                    mortality_event_count + sequence as u64,
                ),
                event: M3pEvent::Birth {
                    parent: birth.parent,
                    child: birth.child,
                    group: birth.group,
                },
            });
        }
        if phase9b.compacted {
            events.push(M3pEventRecord {
                key: EventKey::new(
                    executed_day,
                    9,
                    GLOBAL_PARTITION_KEY,
                    mortality_event_count + phase9b.births.len() as u64,
                ),
                event: M3pEvent::Cleanup {
                    removed: phase9b.removed,
                    replaced_edges: phase9b.replaced_edges,
                    dropped_edges: phase9b.dropped_edges,
                },
            });
        }
        events.sort_by_key(|record| record.key);
    }

    world.current_day = SimulationDay(next_day);
    Ok(M3pDayOutcome {
        executed_day,
        metrics,
        events,
        snapshot,
    })
}

enum M3StoragePhase6BMode<'a> {
    Serial,
    SerialTimed {
        timings: &'a mut Vec<M3Phase6BStageTimings>,
    },
    Parallel {
        scratch: &'a mut M3Phase6BParallelScratch,
        pool: &'a ThreadPool,
        chunk_size: NonZeroUsize,
    },
    ParallelTimed {
        scratch: &'a mut M3Phase6BParallelScratch,
        pool: &'a ThreadPool,
        chunk_size: NonZeroUsize,
        timings: &'a mut Vec<M3Phase6BStageTimings>,
    },
}

fn run_m3_storage_day_with_phase6b(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
    options: &DayExecutionOptions,
    phase6b_mode: M3StoragePhase6BMode<'_>,
) -> Result<M3DayOutcome, M0RunError> {
    run_m3_storage_day_with_phase6b_and_search_reduction(
        hybrid_world,
        config,
        context,
        options,
        phase6b_mode,
        None,
    )
}

fn run_m3_storage_day_with_search_reduction(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3SearchReductionRunContext,
    options: &DayExecutionOptions,
) -> Result<M3DayOutcome, M0RunError> {
    run_m3_storage_day_with_phase6b_and_search_reduction(
        hybrid_world,
        config,
        &mut context.storage_context,
        options,
        M3StoragePhase6BMode::Serial,
        Some(&mut context.scratch),
    )
}

fn run_m3_storage_day_with_phase6b_and_search_reduction(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
    options: &DayExecutionOptions,
    phase6b_mode: M3StoragePhase6BMode<'_>,
    search_reduction_scratch: Option<&mut M3SearchReductionScratch>,
) -> Result<M3DayOutcome, M0RunError> {
    let HybridWorldState {
        world,
        segmented_storage,
        authority_mode: _,
    } = hybrid_world;
    let storage = segmented_storage
        .as_mut()
        .ok_or_else(|| M0RunError::InvariantViolation("hybrid mode without storage".into()))?;
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.run_context.master_seed;
    effective_config.world.replicate_id = context.run_context.replicate_id;

    phase1_resource_regrowth(world, &effective_config);
    storage.phase2_degradation_with_config(&effective_config);

    context.features_scratch.clear();
    phase3_observation_and_features_storage_with_scratch(
        storage,
        &world.settlements,
        &effective_config,
        &mut context.features_scratch,
        &mut context.phase3_scratch,
    )?;

    context.choices_scratch.clear();
    phase4_primary_action_selection_storage_into(
        storage,
        world.current_day,
        &effective_config,
        &context.features_scratch,
        &mut context.choices_scratch,
    )?;

    context.intents_scratch.clear();
    if let Some(search_scratch) = search_reduction_scratch {
        search_scratch.update_phase4_global_eligibility(
            storage,
            effective_config.interaction.starvation_threshold,
        );
        generate_intents_m3_storage_with_global_eligibility_into(
            storage,
            world.current_day,
            &effective_config,
            &context.choices_scratch,
            &context.phase4_projection,
            &mut context.candidate_scratch,
            search_scratch.phase4_global_eligibility,
            &mut context.intents_scratch,
        )?;
    } else {
        generate_intents_m3_storage_into(
            storage,
            world.current_day,
            &effective_config,
            &context.choices_scratch,
            &context.phase4_projection,
            &mut context.candidate_scratch,
            &mut context.intents_scratch,
        )?;
    }
    let cross_partition_interaction_count =
        count_cross_partition_interactions_storage_per_day(&context.intents_scratch, storage)?;

    let partitions = phase5_partition_intents_from_vec_with_scratch(
        &mut context.intents_scratch,
        &mut context.phase5_scratch,
    )?;
    let work_resolutions =
        phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
    let targeted_resolutions = match phase6b_mode {
        M3StoragePhase6BMode::Serial => phase6b_targeted_resolution_storage_m3_with_scratch(
            storage,
            &world.settlements,
            world.current_day,
            &effective_config,
            &partitions,
            &mut context.phase6b_scratch,
        )?,
        M3StoragePhase6BMode::SerialTimed { timings } => {
            let (resolutions, stage_timings) = phase6b_targeted_resolution_storage_m3_with_timing(
                storage,
                &world.settlements,
                world.current_day,
                &effective_config,
                &partitions,
                &mut context.phase6b_scratch,
            )?;
            timings.push(stage_timings);
            resolutions
        }
        M3StoragePhase6BMode::Parallel {
            scratch,
            pool,
            chunk_size,
        } => phase6b_targeted_resolution_storage_m3_parallel_with_scratch(
            storage,
            &world.settlements,
            world.current_day,
            &effective_config,
            &partitions,
            scratch,
            pool,
            chunk_size,
        )?,
        M3StoragePhase6BMode::ParallelTimed {
            scratch,
            pool,
            chunk_size,
            timings,
        } => {
            let (resolutions, stage_timings) =
                phase6b_targeted_resolution_storage_m3_parallel_with_timing(
                    storage,
                    &world.settlements,
                    world.current_day,
                    &effective_config,
                    &partitions,
                    scratch,
                    pool,
                    chunk_size,
                )?;
            timings.push(stage_timings);
            resolutions
        }
    };
    let market_resolutions = phase7_market_clearance_storage_with_config(
        storage,
        &mut world.settlements,
        &partitions,
        &effective_config.economy,
    )?;
    let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
        storage,
        &mut world.settlements,
        &effective_config,
        &mut context.phase8_scratch,
    )?;
    let mortality_resolution = storage.phase9_mortality_commitment()?;

    let mut event_buffer = if options.events_enabled {
        EventBuffer::with_capacity(storage.len().saturating_mul(2) + world.settlements.len() + 4)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );

        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                work_events.len() as u64,
            );
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                targeted_events.len() as u64,
            );
            if offset > 0 {
                for event in &mut targeted_events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for market in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, market));
        }
        for welfare in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, welfare));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    let metrics = if options.metrics_enabled {
        let base = storage.phase10_metrics(&world.settlements, executed_day)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&base));
        }
        let (cross_partition_edge_count, total_directed_edge_count) =
            context.relationship_graph.edge_ratio_counts();
        Some(M3DailyMetrics {
            metrics: base,
            cross_partition_edge_count,
            total_directed_edge_count,
            cross_partition_interaction_count,
        })
    } else {
        None
    };

    let snapshot = if options.snapshot_boundary {
        storage.write_back_to_agents(&mut world.agents);
        let metadata = SnapshotMetadata::new(
            next_day,
            context.run_context.master_seed,
            context.run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &metadata)?;
        if options.events_enabled {
            event_buffer.push(EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            ));
        }
        Some(snapshot)
    } else {
        None
    };

    let events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };
    world.current_day = SimulationDay(next_day);
    Ok(M3DayOutcome {
        executed_day,
        metrics,
        events,
        snapshot,
    })
}

/// Executes multiple consecutive simulation days sequentially.
///
/// Invariants:
/// - Reuses runner-level scratch buffers across all days.
/// - Executes `run_m0_day_with_scratch` exactly `days` times in sequence.
/// - Returns sequential vector of `DayOutcome`s.
/// - Early returns on the first error without advancing further.
pub fn run_m0_days(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    ensure_m2_profile(config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut features_scratch = Vec::with_capacity(world.agents.len());
    let mut choices_scratch = Vec::with_capacity(world.agents.len());
    let mut intents_scratch = Vec::with_capacity(world.agents.len());
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
    for _ in 0..days {
        let outcome = run_m0_day_with_scratch(
            world,
            config,
            context,
            options,
            &mut features_scratch,
            &mut choices_scratch,
            &mut intents_scratch,
            &mut metrics_scratch,
        )?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Executes exactly one full deterministic simulation day using native Segmented SoA kernels
/// for Phase 2 (degradation), Phase 3 (observation & features), Phase 8 (welfare distribution),
/// Phase 9 (mortality commitment), and Phase 10 (metrics aggregation).
pub fn run_native_soa_day(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<DayOutcome, M0RunError> {
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let mut features_scratch = Vec::with_capacity(world.agents.len());
    let mut choices_scratch = Vec::with_capacity(world.agents.len());
    let mut intents_scratch = Vec::with_capacity(world.agents.len());
    run_native_soa_day_with_storage(
        world,
        config,
        context,
        options,
        &mut storage,
        &mut features_scratch,
        &mut choices_scratch,
        &mut intents_scratch,
    )
}

/// Executes exactly one full deterministic simulation day reusing external `SegmentedAgentStorage`
/// and scratch buffers across Native Segmented SoA phases.
#[allow(clippy::too_many_arguments)]
pub fn run_native_soa_day_with_storage(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    storage: &mut SegmentedAgentStorage,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
) -> Result<DayOutcome, M0RunError> {
    let mut phase3_scratch = Phase3ScarcityScratch::with_capacity(world.settlements.len());
    let mut phase8_scratch = Phase8WelfareScratch::with_capacity(world.settlements.len());
    run_native_soa_day_with_storage_and_phase_scratch(
        world,
        config,
        context,
        options,
        storage,
        features_scratch,
        choices_scratch,
        intents_scratch,
        &mut phase3_scratch,
        &mut phase8_scratch,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_native_soa_day_with_storage_and_phase_scratch(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    storage: &mut SegmentedAgentStorage,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    phase3_scratch: &mut Phase3ScarcityScratch,
    phase8_scratch: &mut Phase8WelfareScratch,
) -> Result<DayOutcome, M0RunError> {
    ensure_m2_profile(config)?;
    // 1. Capture logical day coordinate
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;

    // 2. Prepare effective config with deterministic context coordinates
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    // 3. Phase 1: Environment Regrowth
    phase1_resource_regrowth(world, &effective_config);

    // 4. Phase 2: Biological Degradation (Native Segmented SoA)
    storage.phase2_degradation_with_config(&effective_config);
    storage.write_back_to_agents(&mut world.agents);

    // 5. Phase 3: Observation & Normalized Feature Extraction (Native Segmented SoA)
    features_scratch.clear();
    phase3_observation_and_features_storage_with_scratch(
        storage,
        &world.settlements,
        &effective_config,
        features_scratch,
        phase3_scratch,
    )?;

    // 6. Phase 4: Intent Generation (Primary Action Selection & Intent Formulation)
    choices_scratch.clear();
    phase4_primary_action_selection_into(
        world,
        &effective_config,
        features_scratch,
        choices_scratch,
    )?;
    intents_scratch.clear();
    phase4_generate_intents_into(world, &effective_config, choices_scratch, intents_scratch)?;

    // 7. Phase 5: Locality Partitioning
    let partitions = phase5_partition_intents_baseline(intents_scratch)?;

    // 8. Phase 6A: Work Resolution
    let work_resolutions = phase6a_work_resolution(world, &partitions)?;

    // 9. Phase 6B: Targeted Interaction Resolution
    let targeted_resolutions = phase6b_targeted_resolution(world, &effective_config, &partitions)?;

    // 10. Phase 7: Settlement Market Clearance
    let market_resolutions =
        phase7_market_clearance_with_config(world, &partitions, &effective_config.economy)?;

    // Synchronize mutated fields (food, wealth) from phases 6A, 6B, 7 into segmented storage
    storage.sync_from_agents(&world.agents);

    // 11. Phase 8: Institutional Welfare Distribution (Native Segmented SoA)
    let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
        storage,
        &mut world.settlements,
        &effective_config,
        phase8_scratch,
    )?;
    storage.write_back_to_agents(&mut world.agents);

    // 12. Phase 9: Mortality Status Commitment (Native Segmented SoA)
    let mortality_resolution = storage.phase9_mortality_commitment()?;
    storage.write_back_to_agents(&mut world.agents);

    // 13. Staged Event Buffer Assembly
    let mut event_buffer = if options.events_enabled {
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        EventBuffer::with_capacity(estimated_cap)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );

        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                work_events.len() as u64,
            );
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                targeted_events.len() as u64,
            );
            if offset > 0 {
                for event in &mut targeted_events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    // 14. Phase 10: Macroscopic Metrics Observation (Native Segmented SoA)
    let metrics_opt = if options.metrics_enabled {
        let metrics = storage.phase10_metrics(&world.settlements, executed_day)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&metrics));
        }
        Some(metrics)
    } else {
        None
    };

    // 15. Phase 11: Canonical Snapshot
    let snapshot_opt = if options.snapshot_boundary {
        let snapshot_metadata = SnapshotMetadata::new(
            next_day,
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &snapshot_metadata)?;

        if options.events_enabled {
            let snapshot_event = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: snapshot_metadata.day,
                    master_seed: snapshot_metadata.master_seed,
                    replicate_id: snapshot_metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snapshot_event);
        }

        Some(snapshot)
    } else {
        None
    };

    // 16. Phase 11: Canonical Event Flush
    let flushed_events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };

    // 17. Day Complete: advance authoritative scheduler day cursor
    world.current_day = SimulationDay(next_day);

    Ok(DayOutcome {
        executed_day,
        metrics: metrics_opt,
        events: flushed_events,
        snapshot: snapshot_opt,
    })
}

/// Executes multiple consecutive simulation days sequentially using native Segmented SoA kernels.
pub fn run_native_soa_days(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    ensure_m2_profile(config)?;
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let mut features_scratch = Vec::with_capacity(world.agents.len());
    let mut choices_scratch = Vec::with_capacity(world.agents.len());
    let mut intents_scratch = Vec::with_capacity(world.agents.len());
    let mut phase3_scratch = Phase3ScarcityScratch::with_capacity(world.settlements.len());
    let mut phase8_scratch = Phase8WelfareScratch::with_capacity(world.settlements.len());
    for _ in 0..days {
        let outcome = run_native_soa_day_with_storage_and_phase_scratch(
            world,
            config,
            context,
            options,
            &mut storage,
            &mut features_scratch,
            &mut choices_scratch,
            &mut intents_scratch,
            &mut phase3_scratch,
            &mut phase8_scratch,
        )?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Executes exactly one full deterministic simulation day under Hybrid Storage Authority.
///
/// In [`AuthorityMode::Hybrid`], [`SegmentedAgentStorage`] is the authoritative runtime state.
/// Native SoA kernels are invoked directly across Phase 2, Phase 3, Phase 4, Phase 6A, Phase 6B,
/// Phase 7, Phase 8, Phase 9, and Phase 10 without any per-phase AoS synchronization or intermediate
/// write-backs.
///
/// In [`AuthorityMode::Legacy`], execution delegates to standard AoS reference logic.
pub fn run_hybrid_authority_day(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<DayOutcome, M0RunError> {
    if !hybrid_world.is_hybrid() {
        let agent_count = hybrid_world.world.agents.len();
        let mut features_scratch = Vec::with_capacity(agent_count);
        let mut choices_scratch = Vec::with_capacity(agent_count);
        let mut intents_scratch = Vec::with_capacity(agent_count);
        let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
        return run_hybrid_authority_day_with_scratch(
            hybrid_world,
            config,
            context,
            options,
            &mut features_scratch,
            &mut choices_scratch,
            &mut intents_scratch,
            &mut metrics_scratch,
        );
    }

    let mut candidate_index =
        Phase4CandidateIndexScratch::with_capacity(hybrid_world.world.settlements.len());
    run_hybrid_authority_day_with_candidate_index_scratch(
        hybrid_world,
        config,
        context,
        options,
        &mut candidate_index,
    )
}

/// Executes one Hybrid Authority day while reusing a caller-owned Phase 4 candidate index.
pub fn run_hybrid_authority_day_with_candidate_index_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    candidate_index: &mut Phase4CandidateIndexScratch,
) -> Result<DayOutcome, M0RunError> {
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase5_scratch =
        Phase5PartitionScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase6b_scratch = Phase6BResolutionScratch::with_capacity(agent_count);
    run_hybrid_authority_day_with_all_scratch(
        hybrid_world,
        config,
        context,
        options,
        &mut features_scratch,
        &mut choices_scratch,
        &mut intents_scratch,
        &mut metrics_scratch,
        Phase4RuntimeScratch::PreIndexed(candidate_index),
        Phase4ExecutionPolicy::Serial,
        Phase8RuntimeScratch::OnePass(&mut phase8_scratch),
        Phase3RuntimeScratch::Indexed(&mut phase3_scratch),
        Phase5RuntimeMode::OwnedFast,
        Some(&mut phase5_scratch),
        Phase6BRuntimeMode::StorageFast,
        Some(&mut phase6b_scratch),
    )
}

/// Executes one Hybrid Authority day with the reference full-storage candidate scan.
///
/// The caller-owned candidate buffer is cleared before each Phase 4 generation and retains capacity.
pub fn run_hybrid_authority_day_with_candidate_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    candidate_scratch: &mut Vec<AgentId>,
) -> Result<DayOutcome, M0RunError> {
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase5_scratch =
        Phase5PartitionScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase6b_scratch = Phase6BResolutionScratch::with_capacity(agent_count);
    run_hybrid_authority_day_with_all_scratch(
        hybrid_world,
        config,
        context,
        options,
        &mut features_scratch,
        &mut choices_scratch,
        &mut intents_scratch,
        &mut metrics_scratch,
        Phase4RuntimeScratch::FullScan(candidate_scratch),
        Phase4ExecutionPolicy::Serial,
        Phase8RuntimeScratch::OnePass(&mut phase8_scratch),
        Phase3RuntimeScratch::Indexed(&mut phase3_scratch),
        Phase5RuntimeMode::OwnedFast,
        Some(&mut phase5_scratch),
        Phase6BRuntimeMode::StorageFast,
        Some(&mut phase6b_scratch),
    )
}

/// Executes exactly one full deterministic simulation day under Hybrid Storage Authority
/// reusing caller-provided scratch buffers.
#[allow(clippy::too_many_arguments)]
pub fn run_hybrid_authority_day_with_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    metrics_scratch: &mut AgentDynamicSoAScratch,
) -> Result<DayOutcome, M0RunError> {
    if !hybrid_world.is_hybrid() {
        return run_m0_day_with_scratch(
            &mut hybrid_world.world,
            config,
            context,
            options,
            features_scratch,
            choices_scratch,
            intents_scratch,
            metrics_scratch,
        );
    }
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut candidate_scratch = Vec::with_capacity(agent_count);
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase5_scratch =
        Phase5PartitionScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase6b_scratch = Phase6BResolutionScratch::with_capacity(agent_count);
    run_hybrid_authority_day_with_all_scratch(
        hybrid_world,
        config,
        context,
        options,
        features_scratch,
        choices_scratch,
        intents_scratch,
        metrics_scratch,
        Phase4RuntimeScratch::FullScan(&mut candidate_scratch),
        Phase4ExecutionPolicy::Serial,
        Phase8RuntimeScratch::OnePass(&mut phase8_scratch),
        Phase3RuntimeScratch::Indexed(&mut phase3_scratch),
        Phase5RuntimeMode::OwnedFast,
        Some(&mut phase5_scratch),
        Phase6BRuntimeMode::StorageFast,
        Some(&mut phase6b_scratch),
    )
}

enum Phase4RuntimeScratch<'a> {
    FullScan(&'a mut Vec<AgentId>),
    PreIndexed(&'a mut Phase4CandidateIndexScratch),
}

/// Runtime Phase4 backend chosen for a simulation tick. It is not simulation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase4Backend {
    Serial,
    Rayon,
}

/// Caller-owned execution policy for Phase4.
///
/// Auto uses Serial when the pool has one worker or fewer. Otherwise it compares the eligible
/// feature count produced for the current tick with its configured threshold, selecting Rayon at
/// `count >= threshold`. Both numeric settings are nonzero, and the caller owns the pool for the
/// full run.
#[derive(Clone, Copy)]
pub enum Phase4ExecutionPolicy<'a> {
    /// Keep using the existing serial Phase4 implementation.
    Serial,
    /// Always use Rayon with the supplied, already-built pool.
    Rayon {
        pool: &'a ThreadPool,
        chunk_size: NonZeroUsize,
    },
    /// Use serial below `threshold`, and Rayon at or above it.
    Auto {
        pool: &'a ThreadPool,
        threshold: NonZeroUsize,
        chunk_size: NonZeroUsize,
    },
}

impl Phase4ExecutionPolicy<'_> {
    /// Exposes the deterministic policy decision for tests and benchmarks.
    pub fn backend_for(&self, item_count: usize) -> Phase4Backend {
        match self {
            Self::Serial => Phase4Backend::Serial,
            Self::Rayon { .. } => Phase4Backend::Rayon,
            Self::Auto {
                pool, threshold, ..
            } => {
                if pool.current_num_threads() > 1 && item_count >= threshold.get() {
                    Phase4Backend::Rayon
                } else {
                    Phase4Backend::Serial
                }
            }
        }
    }

    fn execution_mode(&self, item_count: usize) -> Phase4ExecutionMode<'_> {
        match (self.backend_for(item_count), self) {
            (Phase4Backend::Serial, _) => Phase4ExecutionMode::Serial,
            (Phase4Backend::Rayon, Self::Rayon { pool, chunk_size })
            | (
                Phase4Backend::Rayon,
                Self::Auto {
                    pool, chunk_size, ..
                },
            ) => Phase4ExecutionMode::Rayon {
                pool,
                chunk_size: *chunk_size,
            },
            (Phase4Backend::Rayon, Self::Serial) => {
                unreachable!("serial policy cannot select Rayon")
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Phase4ExecutionMode<'a> {
    Serial,
    Rayon {
        pool: &'a ThreadPool,
        chunk_size: NonZeroUsize,
    },
}

enum Phase8RuntimeScratch<'a> {
    FullScan,
    OnePass(&'a mut Phase8WelfareScratch),
}

enum Phase3RuntimeScratch<'a> {
    LinearScan,
    Indexed(&'a mut Phase3ScarcityScratch),
}

#[derive(Clone, Copy)]
enum Phase5RuntimeMode {
    CanonicalBaseline,
    OwnedFast,
}

#[derive(Clone, Copy)]
enum Phase6BRuntimeMode {
    CanonicalBaseline,
    StorageFast,
}

#[allow(clippy::too_many_arguments)]
fn run_hybrid_authority_day_with_all_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    metrics_scratch: &mut AgentDynamicSoAScratch,
    phase4_scratch: Phase4RuntimeScratch<'_>,
    phase4_policy: Phase4ExecutionPolicy<'_>,
    phase8_scratch: Phase8RuntimeScratch<'_>,
    phase3_scratch: Phase3RuntimeScratch<'_>,
    phase5_mode: Phase5RuntimeMode,
    phase5_scratch: Option<&mut Phase5PartitionScratch>,
    phase6b_mode: Phase6BRuntimeMode,
    phase6b_scratch: Option<&mut Phase6BResolutionScratch>,
) -> Result<DayOutcome, M0RunError> {
    ensure_m2_profile(config)?;
    if !hybrid_world.is_hybrid() {
        return run_m0_day_with_scratch(
            &mut hybrid_world.world,
            config,
            context,
            options,
            features_scratch,
            choices_scratch,
            intents_scratch,
            metrics_scratch,
        );
    }

    let HybridWorldState {
        world,
        segmented_storage,
        authority_mode: _,
    } = hybrid_world;

    let storage = segmented_storage
        .as_mut()
        .ok_or_else(|| M0RunError::InvariantViolation("hybrid mode without storage".to_string()))?;

    // 1. Capture logical day coordinate
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;

    // 2. Prepare effective config with deterministic context coordinates
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    // 3. Phase 1: Environment Regrowth (operates on settlement resources)
    phase1_resource_regrowth(world, &effective_config);

    // 4. Phase 2: Biological Degradation (Native Segmented SoA directly on storage)
    storage.phase2_degradation_with_config(&effective_config);

    // 5. Phase 3: Observation & Normalized Feature Extraction (Native Segmented SoA)
    features_scratch.clear();
    match phase3_scratch {
        Phase3RuntimeScratch::LinearScan => {
            storage.phase3_features_into(
                &world.settlements,
                &effective_config,
                features_scratch,
            )?;
        }
        Phase3RuntimeScratch::Indexed(scratch) => {
            phase3_observation_and_features_storage_with_scratch(
                storage,
                &world.settlements,
                &effective_config,
                features_scratch,
                scratch,
            )?;
        }
    }

    // Policy selection uses this tick's actual eligible Phase4 work, not total storage length.
    let phase4_mode = phase4_policy.execution_mode(features_scratch.len());

    // 6. Phase 4: Intent Generation (Primary Action Selection & Intent Formulation directly on storage)
    choices_scratch.clear();
    match phase4_mode {
        Phase4ExecutionMode::Serial => phase4_primary_action_selection_storage_into(
            storage,
            world.current_day,
            &effective_config,
            features_scratch,
            choices_scratch,
        )?,
        Phase4ExecutionMode::Rayon { pool, chunk_size } => {
            phase4_primary_action_selection_storage_into_rayon(
                storage,
                world.current_day,
                &effective_config,
                features_scratch,
                choices_scratch,
                pool,
                chunk_size,
            )?
        }
    }

    intents_scratch.clear();
    match phase4_scratch {
        Phase4RuntimeScratch::FullScan(candidate_scratch) => {
            generate_intents_storage_with_scratch(
                storage,
                world.current_day,
                &effective_config,
                choices_scratch,
                candidate_scratch,
                intents_scratch,
            )?;
        }
        Phase4RuntimeScratch::PreIndexed(candidate_index) => match phase4_mode {
            Phase4ExecutionMode::Serial => generate_intents_storage_with_candidate_index(
                storage,
                world.current_day,
                &effective_config,
                choices_scratch,
                candidate_index,
                intents_scratch,
            )?,
            Phase4ExecutionMode::Rayon { pool, chunk_size } => {
                generate_intents_storage_with_candidate_index_rayon(
                    storage,
                    world.current_day,
                    &effective_config,
                    choices_scratch,
                    candidate_index,
                    intents_scratch,
                    pool,
                    chunk_size,
                )?;
            }
        },
    }

    // 7. Phase 5: Locality Partitioning
    let partitions = match phase5_mode {
        Phase5RuntimeMode::CanonicalBaseline => phase5_partition_intents_baseline(intents_scratch)?,
        Phase5RuntimeMode::OwnedFast => {
            let scratch = phase5_scratch.ok_or_else(|| {
                M0RunError::InvariantViolation(
                    "owned Phase5 path requires its transient scratch".to_string(),
                )
            })?;
            phase5_partition_intents_from_vec_with_scratch(intents_scratch, scratch)?
        }
    };

    // 8. Phase 6A: Work Resolution (Directly on storage and settlements)
    let work_resolutions =
        phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;

    // 9. Phase 6B: Targeted Interaction Resolution (Directly on storage and settlements)
    let targeted_resolutions = match phase6b_mode {
        Phase6BRuntimeMode::CanonicalBaseline => phase6b_targeted_resolution_storage_baseline(
            storage,
            &world.settlements,
            world.current_day,
            &effective_config,
            &partitions,
        )?,
        Phase6BRuntimeMode::StorageFast => {
            let scratch = phase6b_scratch.ok_or_else(|| {
                M0RunError::InvariantViolation(
                    "fast Phase6B path requires its transient scratch".to_string(),
                )
            })?;
            phase6b_targeted_resolution_storage_ordered_with_scratch(
                storage,
                &world.settlements,
                world.current_day,
                &effective_config,
                &partitions,
                scratch,
            )?
        }
    };

    // 10. Phase 7: Settlement Market Clearance (Directly on storage and settlements)
    let market_resolutions = phase7_market_clearance_storage_with_config(
        storage,
        &mut world.settlements,
        &partitions,
        &effective_config.economy,
    )?;

    // ZERO SYNC: storage holds authoritative state mutated directly by phases 6A, 6B, 7.

    // 11. Phase 8: Institutional Welfare Distribution (Native Segmented SoA directly on storage)
    let welfare_resolutions = match phase8_scratch {
        Phase8RuntimeScratch::FullScan => phase8_welfare_distribution_storage_full_scan(
            storage,
            &mut world.settlements,
            effective_config.interaction.starvation_threshold,
            effective_config.economy.welfare_payment,
        )?,
        Phase8RuntimeScratch::OnePass(scratch) => {
            phase8_welfare_distribution_storage_with_config_and_scratch(
                storage,
                &mut world.settlements,
                &effective_config,
                scratch,
            )?
        }
    };

    // ZERO WRITE-BACK: storage remains the authority.

    // 12. Phase 9: Mortality Status Commitment (Native Segmented SoA directly on storage)
    let mortality_resolution = storage.phase9_mortality_commitment()?;

    // ZERO WRITE-BACK: storage remains the authority.

    // 13. Staged Event Buffer Assembly
    let mut event_buffer = if options.events_enabled {
        let estimated_cap = storage.len().saturating_mul(2) + world.settlements.len() + 4;
        EventBuffer::with_capacity(estimated_cap)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );

        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                work_events.len() as u64,
            );
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                targeted_events.len() as u64,
            );
            if offset > 0 {
                for event in &mut targeted_events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    // 14. Phase 10: Macroscopic Metrics Observation (Native Segmented SoA directly on storage)
    let metrics_opt = if options.metrics_enabled {
        let metrics = storage.phase10_metrics(&world.settlements, executed_day)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&metrics));
        }
        Some(metrics)
    } else {
        None
    };

    // 15. Phase 11: Canonical Snapshot
    let snapshot_opt = if options.snapshot_boundary {
        // Synchronizes authoritative storage back to world.agents ONLY when snapshotting
        storage.write_back_to_agents(&mut world.agents);

        let snapshot_metadata = SnapshotMetadata::new(
            next_day,
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &snapshot_metadata)?;

        if options.events_enabled {
            let snapshot_event = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: snapshot_metadata.day,
                    master_seed: snapshot_metadata.master_seed,
                    replicate_id: snapshot_metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snapshot_event);
        }

        Some(snapshot)
    } else {
        None
    };

    // 16. Phase 11: Canonical Event Flush
    let flushed_events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };

    // 17. Day Complete: advance authoritative scheduler day cursor
    world.current_day = SimulationDay(next_day);

    Ok(DayOutcome {
        executed_day,
        metrics: metrics_opt,
        events: flushed_events,
        snapshot: snapshot_opt,
    })
}

/// Executes multiple consecutive simulation days sequentially under Hybrid Storage Authority.
pub fn run_hybrid_authority_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        true,
        true,
        Phase4ExecutionPolicy::Serial,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
    )
}

/// Executes Hybrid days with an explicit caller-configured Phase4 policy.
/// Existing serial entry points are unchanged; non-serial policies require Hybrid authority.
pub fn run_hybrid_authority_days_with_phase4_policy(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
    policy: Phase4ExecutionPolicy<'_>,
) -> Result<Vec<DayOutcome>, M0RunError> {
    if !hybrid_world.is_hybrid() && !matches!(policy, Phase4ExecutionPolicy::Serial) {
        return Err(M0RunError::InvariantViolation(
            "non-serial Phase4 policy requires Hybrid Storage Authority".to_string(),
        ));
    }
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        true,
        true,
        policy,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
    )
}

/// Executes Hybrid days with explicit Rayon Phase4 and a caller-owned pool.
/// Kept as a convenience wrapper over the common execution-policy runner.
pub fn run_hybrid_authority_days_with_rayon_phase4(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase4_policy(
        hybrid_world,
        config,
        context,
        days,
        options,
        Phase4ExecutionPolicy::Rayon { pool, chunk_size },
    )
}

/// Executes Hybrid days using the canonical Phase5 sort path for before/after benchmarks and
/// differential parity checks. Normal production execution uses the ordered owned fast path.
pub fn run_hybrid_authority_days_with_phase5_baseline(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        true,
        true,
        Phase4ExecutionPolicy::Serial,
        Phase5RuntimeMode::CanonicalBaseline,
        Phase6BRuntimeMode::StorageFast,
    )
}

/// Executes Hybrid days using the existing Phase6B storage resolver for before/after benchmarks.
pub fn run_hybrid_authority_days_with_phase6b_baseline(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        true,
        true,
        Phase4ExecutionPolicy::Serial,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::CanonicalBaseline,
    )
}

/// Executes multiple days using the reference repeated full-storage Phase 8 scan.
///
/// This path is retained for differential and benchmark comparisons with production execution.
pub fn run_hybrid_authority_days_with_phase8_full_scan(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        true,
        false,
        Phase4ExecutionPolicy::Serial,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
    )
}

/// Executes multiple days using the reference per-agent linear Phase 3 scarcity search.
///
/// This path is retained for differential and benchmark comparisons with production execution.
pub fn run_hybrid_authority_days_with_phase3_linear_scan(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    run_hybrid_authority_days_with_phase_modes(
        hybrid_world,
        config,
        context,
        days,
        options,
        false,
        true,
        Phase4ExecutionPolicy::Serial,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_hybrid_authority_days_with_phase_modes(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
    use_direct_phase3: bool,
    use_one_pass_phase8: bool,
    phase4_policy: Phase4ExecutionPolicy<'_>,
    phase5_mode: Phase5RuntimeMode,
    phase6b_mode: Phase6BRuntimeMode,
) -> Result<Vec<DayOutcome>, M0RunError> {
    ensure_m2_profile(config)?;
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
    let mut candidate_index = if hybrid_world.is_hybrid() {
        Phase4CandidateIndexScratch::with_capacity(hybrid_world.world.settlements.len())
    } else {
        Phase4CandidateIndexScratch::default()
    };
    let mut phase3_scratch = use_direct_phase3
        .then(|| Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len()));
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase5_scratch = match phase5_mode {
        Phase5RuntimeMode::CanonicalBaseline => None,
        Phase5RuntimeMode::OwnedFast => Some(Phase5PartitionScratch::with_capacity(
            hybrid_world.world.settlements.len(),
        )),
    };
    let mut phase6b_scratch = match phase6b_mode {
        Phase6BRuntimeMode::CanonicalBaseline => None,
        Phase6BRuntimeMode::StorageFast => {
            Some(Phase6BResolutionScratch::with_capacity(agent_count))
        }
    };

    for _ in 0..days {
        let outcome = run_hybrid_authority_day_with_all_scratch(
            hybrid_world,
            config,
            context,
            options,
            &mut features_scratch,
            &mut choices_scratch,
            &mut intents_scratch,
            &mut metrics_scratch,
            Phase4RuntimeScratch::PreIndexed(&mut candidate_index),
            phase4_policy,
            if use_one_pass_phase8 {
                Phase8RuntimeScratch::OnePass(&mut phase8_scratch)
            } else {
                Phase8RuntimeScratch::FullScan
            },
            match phase3_scratch.as_mut() {
                Some(scratch) => Phase3RuntimeScratch::Indexed(scratch),
                None => Phase3RuntimeScratch::LinearScan,
            },
            phase5_mode,
            phase5_scratch.as_mut(),
            phase6b_mode,
            phase6b_scratch.as_mut(),
        )?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}

/// Executes exactly one full deterministic simulation day under Scope-Isolated Hybrid Storage Authority (Pipeline C).
///
/// In this pipeline:
/// - [`HybridWorldState`] with [`SegmentedAgentStorage`] is authoritative.
/// - Phase 2, Phase 3, Phase 8, Phase 9, Phase 10 execute natively on `SegmentedAgentStorage`.
/// - Phase 4, Phase 5, Phase 6A, Phase 6B, Phase 7 execute via legacy AoS implementations on `world.agents`.
/// - Compatibility synchronization occurs strictly at two boundaries:
///   1. Before Phase 4: `storage.write_back_to_agents(&mut world.agents)` (SoA authority -> AoS materialization)
///   2. After Phase 7: `storage.sync_from_agents(&world.agents)` (AoS mutations -> SoA authority resync)
/// - Zero write-backs after Phase 8 or Phase 9.
pub fn run_hybrid_scope_isolated_day(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<DayOutcome, M0RunError> {
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
    run_hybrid_scope_isolated_day_with_scratch(
        hybrid_world,
        config,
        context,
        options,
        &mut features_scratch,
        &mut choices_scratch,
        &mut intents_scratch,
        &mut metrics_scratch,
    )
}

/// Executes exactly one full deterministic simulation day under Scope-Isolated Hybrid Storage Authority (Pipeline C)
/// reusing caller-provided scratch buffers.
#[allow(clippy::too_many_arguments)]
pub fn run_hybrid_scope_isolated_day_with_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    metrics_scratch: &mut AgentDynamicSoAScratch,
) -> Result<DayOutcome, M0RunError> {
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());
    run_hybrid_scope_isolated_day_with_phase8_scratch(
        hybrid_world,
        config,
        context,
        options,
        features_scratch,
        choices_scratch,
        intents_scratch,
        metrics_scratch,
        &mut phase3_scratch,
        &mut phase8_scratch,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_hybrid_scope_isolated_day_with_phase8_scratch(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
    intents_scratch: &mut Vec<Intent>,
    metrics_scratch: &mut AgentDynamicSoAScratch,
    phase3_scratch: &mut Phase3ScarcityScratch,
    phase8_scratch: &mut Phase8WelfareScratch,
) -> Result<DayOutcome, M0RunError> {
    ensure_m2_profile(config)?;
    if !hybrid_world.is_hybrid() {
        return run_m0_day_with_scratch(
            &mut hybrid_world.world,
            config,
            context,
            options,
            features_scratch,
            choices_scratch,
            intents_scratch,
            metrics_scratch,
        );
    }

    let HybridWorldState {
        world,
        segmented_storage,
        authority_mode: _,
    } = hybrid_world;

    let storage = segmented_storage
        .as_mut()
        .ok_or_else(|| M0RunError::InvariantViolation("hybrid mode without storage".to_string()))?;

    // 1. Capture logical day coordinate
    let executed_day = world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;

    // 2. Prepare effective config with deterministic context coordinates
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    // 3. Phase 1: Environment Regrowth (operates on settlement resources)
    phase1_resource_regrowth(world, &effective_config);

    // 4. Phase 2: Biological Degradation (Native Segmented SoA directly on storage)
    storage.phase2_degradation_with_config(&effective_config);

    // 5. Phase 3: Observation & Normalized Feature Extraction (Native Segmented SoA)
    features_scratch.clear();
    phase3_observation_and_features_storage_with_scratch(
        storage,
        &world.settlements,
        &effective_config,
        features_scratch,
        phase3_scratch,
    )?;

    // --- Compatibility Boundary 1: SoA authority -> AoS materialization ---
    storage.write_back_to_agents(&mut world.agents);

    // 6. Phase 4: Intent Generation (Legacy AoS on world)
    choices_scratch.clear();
    phase4_primary_action_selection_into(
        world,
        &effective_config,
        features_scratch,
        choices_scratch,
    )?;

    intents_scratch.clear();
    phase4_generate_intents_into(world, &effective_config, choices_scratch, intents_scratch)?;

    // 7. Phase 5: Locality Partitioning
    let partitions = phase5_partition_intents_baseline(intents_scratch)?;

    // 8. Phase 6A: Work Resolution (Legacy AoS on world)
    let work_resolutions = phase6a_work_resolution(world, &partitions)?;

    // 9. Phase 6B: Targeted Interaction Resolution (Legacy AoS on world)
    let targeted_resolutions = phase6b_targeted_resolution(world, &effective_config, &partitions)?;

    // 10. Phase 7: Settlement Market Clearance (Legacy AoS on world)
    let market_resolutions =
        phase7_market_clearance_with_config(world, &partitions, &effective_config.economy)?;

    // --- Compatibility Boundary 2: AoS mutations -> SoA authority resync ---
    storage.sync_from_agents(&world.agents);

    // 11. Phase 8: Institutional Welfare Distribution (Native Segmented SoA directly on storage)
    let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
        storage,
        &mut world.settlements,
        &effective_config,
        phase8_scratch,
    )?;

    // ZERO WRITE-BACK: storage is authority!

    // 12. Phase 9: Mortality Status Commitment (Native Segmented SoA directly on storage)
    let mortality_resolution = storage.phase9_mortality_commitment()?;

    // ZERO WRITE-BACK: storage is authority!

    // 13. Staged Event Buffer Assembly
    let mut event_buffer = if options.events_enabled {
        let estimated_cap = storage.len().saturating_mul(2) + world.settlements.len() + 4;
        EventBuffer::with_capacity(estimated_cap)
    } else {
        EventBuffer::new()
    };
    if options.events_enabled {
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );

        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                work_events.len() as u64,
            );
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events = events_from_targeted_resolution(executed_day, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                targeted_events.len() as u64,
            );
            if offset > 0 {
                for event in &mut targeted_events {
                    event.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_resolution,
        ));
    }

    // 14. Phase 10: Macroscopic Metrics Observation (Native Segmented SoA directly on storage)
    let metrics_opt = if options.metrics_enabled {
        let metrics = storage.phase10_metrics(&world.settlements, executed_day)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&metrics));
        }
        Some(metrics)
    } else {
        None
    };

    // 15. Phase 11: Canonical Snapshot
    let snapshot_opt = if options.snapshot_boundary {
        storage.write_back_to_agents(&mut world.agents);

        let snapshot_metadata = SnapshotMetadata::new(
            next_day,
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(world, &snapshot_metadata)?;

        if options.events_enabled {
            let snapshot_event = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: snapshot_metadata.day,
                    master_seed: snapshot_metadata.master_seed,
                    replicate_id: snapshot_metadata.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snapshot_event);
        }

        Some(snapshot)
    } else {
        None
    };

    // 16. Phase 11: Canonical Event Flush
    let flushed_events = if options.events_enabled {
        phase11_flush_events(&mut event_buffer)?
    } else {
        Vec::new()
    };

    // 17. Day Complete: advance authoritative scheduler day cursor
    world.current_day = SimulationDay(next_day);

    Ok(DayOutcome {
        executed_day,
        metrics: metrics_opt,
        events: flushed_events,
        snapshot: snapshot_opt,
    })
}

/// Executes multiple consecutive simulation days sequentially under Scope-Isolated Hybrid Storage Authority (Pipeline C).
pub fn run_hybrid_scope_isolated_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
) -> Result<Vec<DayOutcome>, M0RunError> {
    ensure_m2_profile(config)?;
    let agent_count = match &hybrid_world.segmented_storage {
        Some(s) => s.len(),
        None => hybrid_world.world.agents.len(),
    };
    let mut outcomes = Vec::with_capacity(days as usize);
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(agent_count);
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(hybrid_world.world.settlements.len());
    let mut phase8_scratch =
        Phase8WelfareScratch::with_capacity(hybrid_world.world.settlements.len());

    for _ in 0..days {
        let outcome = run_hybrid_scope_isolated_day_with_phase8_scratch(
            hybrid_world,
            config,
            context,
            options,
            &mut features_scratch,
            &mut choices_scratch,
            &mut intents_scratch,
            &mut metrics_scratch,
            &mut phase3_scratch,
            &mut phase8_scratch,
        )?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}
