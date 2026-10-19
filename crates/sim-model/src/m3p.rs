//! M3-08 population dynamics reference path.

use crate::config::{SimConfig, SimulationProfile};
use crate::decision::phase4_primary_action_selection_into;
use crate::events::{
    Event, EventBuffer, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    event_from_daily_metrics, events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    phase11_flush_events,
};
use crate::features::phase3_observation_and_features_into;
use crate::intents::{
    Intent, M3Phase4SlotProjection, count_cross_partition_interactions_per_day,
    generate_intents_m3_into,
};
use crate::metrics::{M3DailyMetrics, phase10_observe_with_scratch};
use crate::partitioning::phase5_partition_intents_baseline;
use crate::phases::{
    phase1_resource_regrowth, phase2_biological_degradation, phase9_mortality_commitment,
};
use crate::relationship_graph::{
    RelationshipGraph, RelationshipGraphAppendPlan, adjust_rank_for_exclusions,
    complement_rank_to_global_rank,
};
use crate::resolution::{
    phase6a_work_resolution, phase6b_targeted_resolution_m3, phase7_market_clearance_with_config,
    phase8_welfare_distribution_with_config,
};
use crate::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext, M0RunError,
    RegisteredCorePhase, run_registered_world_day_loop,
};
use crate::snapshot::{
    CanonicalSnapshot, SNAPSHOT_MAGIC, SnapshotError, SnapshotMetadata, decode_snapshot,
    encode_snapshot,
};
use crate::state::{AgentDynamicSoAScratch, AgentState, WorldState};
use crate::storage::SegmentedAgentStorage;
use crate::system_registry::{
    PhaseSlot, RegisteredHook, RegisteredSystemRunOptions, SystemExecutionOptions, SystemId,
};
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, DenseSlot, GroupId, RngCoordinate, SimulationDay, coordinate_prng_f32};
use std::collections::{BTreeMap, BTreeSet};

const M3P_MAX_LIVE_POPULATION: u64 = 1 << 24;
const AGENT_ID_EXHAUSTED: u64 = u32::MAX as u64 + 1;
pub const M3P_SNAPSHOT_SCHEMA_VERSION: u32 = 2;

/// M3P-only stateless RNG subsystem; the frozen `Subsystem` mapping is unchanged.
pub const M3P_BIRTH_SUBSYSTEM_ID: u16 = 7;

/// M3P event envelope; legacy event payloads retain the existing event schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct M3pEventRecord {
    pub key: EventKey,
    pub event: M3pEvent,
}

/// Existing phase events plus population-dynamics events for the M3P digest domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum M3pEvent {
    Existing(Event),
    Birth {
        parent: AgentId,
        child: AgentId,
        group: GroupId,
    },
    Cleanup {
        removed: u64,
        replaced_edges: u64,
        dropped_edges: u64,
    },
}

/// M3P daily metrics append population and allocator observations to M3 metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct M3pDailyMetrics {
    pub m3_metrics: M3DailyMetrics,
    pub births: u64,
    pub removed: u64,
    pub replaced_edges: u64,
    pub live_population: u64,
    pub next_agent_id: u64,
}

/// Completed M3P reference day. Snapshot v2 support is added in the next stage.
#[derive(Debug, Clone, PartialEq)]
pub struct M3pDayOutcome {
    pub executed_day: u32,
    pub metrics: Option<M3pDailyMetrics>,
    pub events: Vec<M3pEventRecord>,
    pub snapshot: Option<CanonicalSnapshot>,
}

/// Restored population-dynamics state and its canonical snapshot metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredM3pSnapshot {
    pub metadata: SnapshotMetadata,
    pub state: M3pReferenceState,
}

/// Authoritative M3P reference state, including mutable graph and ID allocator state.
#[derive(Debug, Clone, PartialEq)]
pub struct M3pReferenceState {
    world: WorldState,
    relationship_graph: RelationshipGraph,
    next_agent_id: u64,
    run_context: M0RunContext,
    relationship_degree_k: u32,
    p_local_bits: u32,
}

impl M3pReferenceState {
    /// Builds the initial static M3 graph and the M3P allocator state for an initialized world.
    pub fn new(
        world: WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        ensure_m3p_config(config)?;
        let degree_k = config
            .interaction
            .relationship_degree_k
            .ok_or_else(|| invariant("M3P profile requires relationship_degree_k"))?;
        let degree_k = u32::try_from(degree_k)
            .map_err(|_| invariant("M3P relationship_degree_k is outside u32 range"))?;
        let p_local = config
            .interaction
            .p_local
            .ok_or_else(|| invariant("M3P profile requires p_local"))?;
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
        let next_agent_id = u64::try_from(world.agents.len())
            .map_err(|_| invariant("M3P initial AgentId cursor overflow"))?;
        Self::from_parts(
            world,
            relationship_graph,
            next_agent_id,
            config,
            run_context,
        )
    }

    /// Reconstructs validated M3P state from canonical world, graph, and allocator components.
    pub fn from_parts(
        world: WorldState,
        relationship_graph: RelationshipGraph,
        next_agent_id: u64,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        ensure_m3p_config(config)?;
        let degree_k = config
            .interaction
            .relationship_degree_k
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| invariant("M3P relationship_degree_k is outside u32 range"))?;
        let p_local = config
            .interaction
            .p_local
            .ok_or_else(|| invariant("M3P profile requires p_local"))?;
        validate_state_parts(&world.agents, &relationship_graph, next_agent_id)?;
        Ok(Self {
            world,
            relationship_graph,
            next_agent_id,
            run_context: *run_context,
            relationship_degree_k: degree_k,
            p_local_bits: p_local.to_bits(),
        })
    }

    pub fn world(&self) -> &WorldState {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut WorldState {
        &mut self.world
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        &self.relationship_graph
    }

    pub const fn next_agent_id(&self) -> u64 {
        self.next_agent_id
    }

    pub(crate) const fn run_context(&self) -> M0RunContext {
        self.run_context
    }

    fn validate_for(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<(), M0RunError> {
        ensure_m3p_config(config)?;
        config
            .validate()
            .map_err(|error| invariant(format!("invalid M3P configuration: {error}")))?;
        if &self.run_context != run_context {
            return Err(invariant("M3P run coordinates changed during a run"));
        }
        if config.interaction.relationship_degree_k != Some(self.relationship_degree_k as i64)
            || config.interaction.p_local.map(f32::to_bits) != Some(self.p_local_bits)
        {
            return Err(invariant(
                "M3P relationship graph parameters changed during a run",
            ));
        }
        validate_state_parts(
            &self.world.agents,
            &self.relationship_graph,
            self.next_agent_id,
        )
    }
}

/// Encodes an M3P schema-v2 snapshot while leaving the M2/M3 schema-v1 bytes untouched.
pub fn encode_m3p_snapshot(
    state: &M3pReferenceState,
    metadata: &SnapshotMetadata,
) -> Result<CanonicalSnapshot, SnapshotError> {
    validate_state_parts(
        &state.world.agents,
        &state.relationship_graph,
        state.next_agent_id,
    )
    .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    if metadata.master_seed != state.run_context.master_seed
        || metadata.replicate_id != state.run_context.replicate_id
    {
        return Err(SnapshotError::InvalidState(
            "M3P snapshot metadata does not match run coordinates".into(),
        ));
    }

    // Reuse the canonical v1 world encoding as the immutable state section in the v2 envelope.
    let world_snapshot = encode_snapshot(&state.world, metadata)?;
    let base_len =
        u64::try_from(world_snapshot.bytes.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let source_count = u64::try_from(state.relationship_graph.source_ids().len())
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SNAPSHOT_MAGIC);
    bytes.extend_from_slice(&M3P_SNAPSHOT_SCHEMA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&base_len.to_le_bytes());
    bytes.extend_from_slice(&world_snapshot.bytes);
    bytes.extend_from_slice(&state.next_agent_id.to_le_bytes());
    bytes.extend_from_slice(&state.world.initial_money_supply.to_le_bytes());
    bytes.extend_from_slice(&source_count.to_le_bytes());
    for &source_id in state.relationship_graph.source_ids() {
        let neighbors = state
            .relationship_graph
            .neighbors_of(source_id)
            .ok_or_else(|| SnapshotError::InvalidState("missing M3P CSR source row".into()))?;
        let degree = u32::try_from(neighbors.len()).map_err(|_| SnapshotError::LengthOverflow)?;
        bytes.extend_from_slice(&source_id.0.to_le_bytes());
        bytes.extend_from_slice(&degree.to_le_bytes());
        for neighbor in neighbors {
            bytes.extend_from_slice(&neighbor.0.to_le_bytes());
        }
    }
    bytes.extend_from_slice(state.relationship_graph.csr_hash());
    Ok(CanonicalSnapshot::new(metadata.clone(), bytes))
}

/// Decodes and validates an M3P v2 snapshot, including graph closure and CSR hash.
pub fn decode_m3p_snapshot(
    bytes: &[u8],
    config: &SimConfig,
) -> Result<RestoredM3pSnapshot, SnapshotError> {
    let mut cursor = 0usize;
    let magic: [u8; 8] = read_snapshot_bytes(bytes, &mut cursor, 8)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SnapshotError::InvalidMagic(magic));
    }
    let schema = read_snapshot_u32(bytes, &mut cursor)?;
    if schema != M3P_SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedVersion(schema));
    }
    let base_len = usize::try_from(read_snapshot_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let base_bytes = read_snapshot_bytes(bytes, &mut cursor, base_len)?;
    let mut restored = decode_snapshot(base_bytes)?;
    let next_agent_id = read_snapshot_u64(bytes, &mut cursor)?;
    let initial_money_supply = i64::from_le_bytes(
        read_snapshot_bytes(bytes, &mut cursor, 8)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    );
    if initial_money_supply < 0 {
        return Err(SnapshotError::InvalidState(
            "M3P initial money supply is negative".into(),
        ));
    }
    restored.world.initial_money_supply = initial_money_supply;
    let source_count = usize::try_from(read_snapshot_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    if source_count != restored.world.agents.len() {
        return Err(SnapshotError::InvalidState(
            "M3P CSR source count does not match canonical agents".into(),
        ));
    }

    let agent_ids: Vec<_> = restored
        .world
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    let mut rows = Vec::with_capacity(source_count);
    for &expected_source in &agent_ids {
        let source_id = AgentId(read_snapshot_u32(bytes, &mut cursor)?);
        if source_id != expected_source {
            return Err(SnapshotError::InvalidState(
                "M3P CSR sources are not in canonical AgentId order".into(),
            ));
        }
        let degree = read_snapshot_u32(bytes, &mut cursor)? as usize;
        if degree > 16 {
            return Err(SnapshotError::InvalidState(
                "M3P CSR row exceeds maximum degree".into(),
            ));
        }
        let mut neighbors = Vec::with_capacity(degree);
        for _ in 0..degree {
            let target_id = AgentId(read_snapshot_u32(bytes, &mut cursor)?);
            if target_id == source_id || agent_ids.binary_search(&target_id).is_err() {
                return Err(SnapshotError::InvalidState(
                    "M3P CSR contains a dangling or self edge".into(),
                ));
            }
            if neighbors
                .last()
                .is_some_and(|previous| *previous >= target_id)
            {
                return Err(SnapshotError::InvalidState(
                    "M3P CSR neighbors are not strictly AgentId-sorted".into(),
                ));
            }
            neighbors.push(target_id);
        }
        rows.push((source_id, neighbors));
    }
    let stored_hash: [u8; 32] = read_snapshot_bytes(bytes, &mut cursor, 32)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    if cursor < bytes.len() {
        return Err(SnapshotError::TrailingBytes(bytes.len() - cursor));
    }

    let memberships: Vec<_> = restored
        .world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    let graph = RelationshipGraph::from_canonical_rows(rows, &memberships)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    if graph.csr_hash() != &stored_hash {
        return Err(SnapshotError::InvalidState(
            "M3P CSR hash does not match canonical graph bytes".into(),
        ));
    }
    let run_context = M0RunContext::new(
        restored.metadata.master_seed,
        restored.metadata.replicate_id,
    );
    let state =
        M3pReferenceState::from_parts(restored.world, graph, next_agent_id, config, &run_context)
            .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    Ok(RestoredM3pSnapshot {
        metadata: restored.metadata,
        state,
    })
}

fn read_snapshot_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], SnapshotError> {
    let end = cursor
        .checked_add(len)
        .ok_or(SnapshotError::UnexpectedEof)?;
    if end > bytes.len() {
        return Err(SnapshotError::UnexpectedEof);
    }
    let value = &bytes[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn read_snapshot_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, SnapshotError> {
    Ok(u32::from_le_bytes(
        read_snapshot_bytes(bytes, cursor, 4)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

fn read_snapshot_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SnapshotError> {
    Ok(u64::from_le_bytes(
        read_snapshot_bytes(bytes, cursor, 8)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

/// Executes one M3P reference day, including schema-v2 snapshots at requested boundaries.
pub fn run_m3p_reference_day(
    state: &mut M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<M3pDayOutcome, M0RunError> {
    run_m3p_reference_day_registered(state, config, run_context, options, None)
}

pub(crate) fn run_m3p_reference_day_with_post_phase9c_registered(
    state: &mut M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    post_phase9c: &mut dyn FnMut(
        &WorldState,
        &RelationshipGraph,
        &[AgentId],
        u32,
    ) -> Result<(), M0RunError>,
) -> Result<M3pDayOutcome, M0RunError> {
    run_m3p_reference_day_registered(state, config, run_context, options, Some(post_phase9c))
}

fn run_m3p_reference_day_registered(
    state: &mut M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    mut post_phase9c: Option<
        &mut dyn FnMut(&WorldState, &RelationshipGraph, &[AgentId], u32) -> Result<(), M0RunError>,
    >,
) -> Result<M3pDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let M3pReferenceState {
        world,
        relationship_graph,
        next_agent_id,
        run_context: _,
        relationship_degree_k: _,
        p_local_bits: _,
    } = state;
    let mut day_state = M3pRegisteredReferenceDay::new(
        config,
        run_context,
        relationship_graph,
        next_agent_id,
        options,
    );
    let enabled_systems: &[SystemId] = if post_phase9c.is_some() {
        &[
            SystemId::RelationshipGraph,
            SystemId::PopulationDynamics,
            SystemId::Propagation,
        ]
    } else {
        &[SystemId::RelationshipGraph, SystemId::PopulationDynamics]
    };
    let run_options = RegisteredSystemRunOptions {
        enabled_systems: &enabled_systems,
        execution: SystemExecutionOptions::default(),
    };
    let mut outcomes = run_registered_world_day_loop(
        world,
        &mut day_state,
        &run_options,
        1,
        |day, phase, hooks, world, state, _| state.run_core_phase(day, phase, hooks, world),
        |day, hook, world, state, execution| {
            if hook.system_id == SystemId::Propagation && hook.slot == PhaseSlot::Phase9C {
                let phase9b = state
                    .phase9b
                    .as_ref()
                    .expect("Phase 9b hook runs before Phase 9c");
                let newborn_ids: Vec<_> = phase9b.births.iter().map(|birth| birth.child).collect();
                post_phase9c
                    .as_deref_mut()
                    .ok_or_else(|| invariant("registered Phase 9c hook has no callback"))?(
                    world,
                    state.relationship_graph,
                    &newborn_ids,
                    state.executed_day,
                )
            } else {
                state.run_system_hook(day, hook, world, execution)
            }
        },
        |_, world, state| state.finish_day(world),
    )?;
    Ok(outcomes.remove(0))
}

#[cfg(test)]
type M3pReferencePostPhase9b<'a> =
    dyn FnMut(&WorldState, &RelationshipGraph, &[AgentId], u32) -> Result<(), M0RunError> + 'a;

struct M3pRegisteredReferenceDay<'a> {
    effective_config: SimConfig,
    run_context: M0RunContext,
    relationship_graph: &'a mut RelationshipGraph,
    next_agent_id: &'a mut u64,
    options: DayExecutionOptions,
    executed_day: u32,
    next_day: u32,
    features: Vec<crate::features::AgentFeatures>,
    choices: Vec<crate::decision::PrimaryActionChoice>,
    intents: Vec<Intent>,
    cross_partition_interaction_count: u64,
    partitions: Vec<crate::partitioning::SettlementIntentPartition>,
    work: Vec<crate::resolution::SettlementWorkResolution>,
    targeted: Vec<crate::resolution::SettlementTargetedResolution>,
    markets: Vec<crate::resolution::SettlementMarketResolution>,
    welfare: Vec<crate::resolution::SettlementWelfareResolution>,
    mortality: Option<crate::phases::Phase9MortalityResolution>,
    phase9b: Option<Phase9bResult>,
    event_buffer: Option<EventBuffer>,
    metrics: Option<M3pDailyMetrics>,
    snapshot: Option<CanonicalSnapshot>,
    events: Vec<M3pEventRecord>,
}

impl<'a> M3pRegisteredReferenceDay<'a> {
    fn new(
        config: &SimConfig,
        run_context: &M0RunContext,
        relationship_graph: &'a mut RelationshipGraph,
        next_agent_id: &'a mut u64,
        options: &DayExecutionOptions,
    ) -> Self {
        let mut effective_config = config.clone();
        effective_config.world.master_seed = run_context.master_seed;
        effective_config.world.replicate_id = run_context.replicate_id;
        Self {
            effective_config,
            run_context: *run_context,
            relationship_graph,
            next_agent_id,
            options: *options,
            executed_day: 0,
            next_day: 0,
            features: Vec::new(),
            choices: Vec::new(),
            intents: Vec::new(),
            cross_partition_interaction_count: 0,
            partitions: Vec::new(),
            work: Vec::new(),
            targeted: Vec::new(),
            markets: Vec::new(),
            welfare: Vec::new(),
            mortality: None,
            phase9b: None,
            event_buffer: None,
            metrics: None,
            snapshot: None,
            events: Vec::new(),
        }
    }

    fn run_core_phase(
        &mut self,
        _day: u32,
        phase: RegisteredCorePhase,
        _hooks: &[RegisteredHook],
        world: &mut WorldState,
    ) -> Result<(), M0RunError> {
        match phase {
            RegisteredCorePhase::Phase1 => {
                self.executed_day = world.current_day.as_u32();
                self.next_day = self
                    .executed_day
                    .checked_add(1)
                    .ok_or(M0RunError::DayOverflow)?;
                phase1_resource_regrowth(world, &self.effective_config);
            }
            RegisteredCorePhase::Phase2 => {
                phase2_biological_degradation(world, &self.effective_config);
            }
            RegisteredCorePhase::Phase3 => {
                phase3_observation_and_features_into(
                    world,
                    &self.effective_config,
                    &mut self.features,
                )?;
            }
            RegisteredCorePhase::Phase4 => {
                phase4_primary_action_selection_into(
                    world,
                    &self.effective_config,
                    &self.features,
                    &mut self.choices,
                )?;
            }
            RegisteredCorePhase::Phase5 => {
                self.partitions = phase5_partition_intents_baseline(&self.intents)?;
            }
            RegisteredCorePhase::Phase6A => {
                self.work = phase6a_work_resolution(world, &self.partitions)?;
            }
            RegisteredCorePhase::Phase6B => {}
            RegisteredCorePhase::Phase7 => {
                self.markets = phase7_market_clearance_with_config(
                    world,
                    &self.partitions,
                    &self.effective_config.economy,
                )?;
            }
            RegisteredCorePhase::Phase8 => {
                self.welfare =
                    phase8_welfare_distribution_with_config(world, &self.effective_config)?;
            }
            RegisteredCorePhase::Phase9 => {
                self.mortality = Some(phase9_mortality_commitment(world)?);
            }
            RegisteredCorePhase::Phase10 => self.observe(world)?,
            RegisteredCorePhase::Phase11 => self.finish_events_and_snapshot(world)?,
        }
        Ok(())
    }

    fn run_system_hook(
        &mut self,
        _day: u32,
        hook: RegisteredHook,
        world: &mut WorldState,
        execution: &SystemExecutionOptions<'_>,
    ) -> Result<(), M0RunError> {
        match (hook.system_id, hook.slot) {
            (SystemId::RelationshipGraph, PhaseSlot::Initialization) => Ok(()),
            (SystemId::RelationshipGraph, PhaseSlot::Phase4) => {
                generate_intents_m3_into(
                    world,
                    &self.effective_config,
                    &self.choices,
                    self.relationship_graph,
                    &mut self.intents,
                )?;
                self.cross_partition_interaction_count =
                    count_cross_partition_interactions_per_day(&self.intents, world)?;
                Ok(())
            }
            (SystemId::RelationshipGraph, PhaseSlot::Phase6B) => {
                self.targeted = phase6b_targeted_resolution_m3(
                    world,
                    &self.effective_config,
                    &self.partitions,
                )?;
                Ok(())
            }
            (SystemId::PopulationDynamics, PhaseSlot::Phase9B) => {
                if !matches!(
                    execution.for_system(SystemId::PopulationDynamics),
                    crate::system_registry::SystemExecutionPolicy::Serial
                ) {
                    return Err(invariant(
                        "M3P reference supports serial population-dynamics execution only",
                    ));
                }
                let mut phase_state = M3pReferenceState::from_parts(
                    world.clone(),
                    self.relationship_graph.clone(),
                    *self.next_agent_id,
                    &self.effective_config,
                    &self.run_context,
                )?;
                let result = apply_phase9b(
                    &mut phase_state,
                    &self.effective_config,
                    &self.run_context,
                    self.executed_day,
                )?;
                *world = phase_state.world;
                *self.relationship_graph = phase_state.relationship_graph;
                *self.next_agent_id = phase_state.next_agent_id;
                self.phase9b = Some(result);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn observe(&mut self, world: &WorldState) -> Result<(), M0RunError> {
        let phase9b = self.phase9b.as_ref().expect("Phase 9b ran before Phase 10");
        let mut event_buffer = if self.options.events_enabled {
            let estimated_cap = world
                .agents
                .len()
                .saturating_mul(2)
                .saturating_add(world.settlements.len())
                .saturating_add(4);
            EventBuffer::with_capacity(estimated_cap)
        } else {
            EventBuffer::new()
        };
        if self.options.events_enabled {
            let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(self.work.len());
            for work in &self.work {
                let events = events_from_work_resolution(self.executed_day, work);
                let count = events.len() as u64;
                if let Some(entry) = phase6_counts
                    .iter_mut()
                    .find(|(group_id, _)| *group_id == work.group_id.0)
                {
                    entry.1 += count;
                } else {
                    phase6_counts.push((work.group_id.0, count));
                }
                event_buffer.push_all(events);
            }
            for targeted in &self.targeted {
                let mut events = events_from_targeted_resolution(self.executed_day, targeted);
                let offset = if let Some(entry) = phase6_counts
                    .iter_mut()
                    .find(|(group_id, _)| *group_id == targeted.group_id.0)
                {
                    let previous = entry.1;
                    entry.1 += events.len() as u64;
                    previous
                } else {
                    phase6_counts.push((targeted.group_id.0, events.len() as u64));
                    0
                };
                for event in &mut events {
                    event.key.local_sequence += offset;
                }
                event_buffer.push_all(events);
            }
            for market in &self.markets {
                event_buffer.push_all(events_from_market_resolution(self.executed_day, market));
            }
            for welfare in &self.welfare {
                event_buffer.push_all(events_from_welfare_resolution(self.executed_day, welfare));
            }
            event_buffer.push_all(events_from_mortality_resolution(
                self.executed_day,
                self.mortality
                    .as_ref()
                    .expect("Phase 9 ran before Phase 10"),
            ));
        }
        if self.options.metrics_enabled {
            let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
            let base = phase10_observe_with_scratch(world, self.executed_day, &mut scratch)?;
            if self.options.events_enabled {
                event_buffer.push(event_from_daily_metrics(&base));
            }
            let (cross_partition_edge_count, total_directed_edge_count) =
                self.relationship_graph.edge_ratio_counts();
            self.metrics = Some(M3pDailyMetrics {
                m3_metrics: M3DailyMetrics {
                    metrics: base,
                    cross_partition_edge_count,
                    total_directed_edge_count,
                    cross_partition_interaction_count: self.cross_partition_interaction_count,
                },
                births: phase9b.births.len() as u64,
                removed: phase9b.removed,
                replaced_edges: phase9b.replaced_edges,
                live_population: world.agents.iter().filter(|agent| agent.alive).count() as u64,
                next_agent_id: *self.next_agent_id,
            });
        }
        self.event_buffer = Some(event_buffer);
        Ok(())
    }

    fn finish_events_and_snapshot(&mut self, world: &mut WorldState) -> Result<(), M0RunError> {
        let phase9b = self.phase9b.as_ref().expect("Phase 9b ran before Phase 11");
        let mortality = self
            .mortality
            .as_ref()
            .expect("Phase 9 ran before Phase 11");
        let mut event_buffer = self.event_buffer.take().unwrap_or_default();
        if self.options.snapshot_boundary {
            let snapshot_state = M3pReferenceState::from_parts(
                world.clone(),
                self.relationship_graph.clone(),
                *self.next_agent_id,
                &self.effective_config,
                &self.run_context,
            )?;
            let metadata = SnapshotMetadata::new(
                self.next_day,
                self.run_context.master_seed,
                self.run_context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            self.snapshot = Some(encode_m3p_snapshot(&snapshot_state, &metadata)?);
            if self.options.events_enabled {
                event_buffer.push(EventRecord::new(
                    EventKey::new(self.executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                    Event::Observation(ObservationEvent::SnapshotEmitted {
                        day: metadata.day,
                        master_seed: metadata.master_seed,
                        replicate_id: metadata.replicate_id,
                        schema_version: M3P_SNAPSHOT_SCHEMA_VERSION,
                    }),
                ));
            }
        }
        let mut events = if self.options.events_enabled {
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
        if self.options.events_enabled {
            let mortality_count = mortality.newly_deceased.len() as u64;
            for (sequence, birth) in phase9b.births.iter().enumerate() {
                events.push(M3pEventRecord {
                    key: EventKey::new(
                        self.executed_day,
                        9,
                        GLOBAL_PARTITION_KEY,
                        mortality_count + sequence as u64,
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
                        self.executed_day,
                        9,
                        GLOBAL_PARTITION_KEY,
                        mortality_count + phase9b.births.len() as u64,
                    ),
                    event: M3pEvent::Cleanup {
                        removed: phase9b.removed,
                        replaced_edges: phase9b.replaced_edges,
                        dropped_edges: phase9b.dropped_edges,
                    },
                });
            }
            events.sort_by_key(|event| event.key);
        }
        self.events = events;
        world.current_day = SimulationDay(self.next_day);
        Ok(())
    }

    fn finish_day(&mut self, _world: &WorldState) -> Result<M3pDayOutcome, M0RunError> {
        Ok(M3pDayOutcome {
            executed_day: self.executed_day,
            metrics: self.metrics.take(),
            events: std::mem::take(&mut self.events),
            snapshot: self.snapshot.take(),
        })
    }
}

#[cfg(test)]
pub(crate) fn run_m3p_reference_day_with_post_phase9b(
    state: &mut M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    post_phase9b: Option<&mut M3pReferencePostPhase9b<'_>>,
) -> Result<M3pDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;

    let executed_day = state.world.current_day.as_u32();
    let next_day = executed_day.checked_add(1).ok_or(M0RunError::DayOverflow)?;
    let mut effective_config = config.clone();
    effective_config.world.master_seed = run_context.master_seed;
    effective_config.world.replicate_id = run_context.replicate_id;

    phase1_resource_regrowth(&mut state.world, &effective_config);
    phase2_biological_degradation(&mut state.world, &effective_config);
    let mut features = Vec::with_capacity(state.world.agents.len());
    phase3_observation_and_features_into(&state.world, &effective_config, &mut features)?;

    let mut choices = Vec::with_capacity(state.world.agents.len());
    phase4_primary_action_selection_into(&state.world, &effective_config, &features, &mut choices)?;
    let mut intents = Vec::with_capacity(state.world.agents.len());
    generate_intents_m3_into(
        &state.world,
        &effective_config,
        &choices,
        &state.relationship_graph,
        &mut intents,
    )?;
    let cross_partition_interaction_count =
        count_cross_partition_interactions_per_day(&intents, &state.world)?;

    let partitions = phase5_partition_intents_baseline(&intents)?;
    let work_resolutions = phase6a_work_resolution(&mut state.world, &partitions)?;
    let targeted_resolutions =
        phase6b_targeted_resolution_m3(&mut state.world, &effective_config, &partitions)?;
    let market_resolutions = phase7_market_clearance_with_config(
        &mut state.world,
        &partitions,
        &effective_config.economy,
    )?;
    let welfare_resolutions =
        phase8_welfare_distribution_with_config(&mut state.world, &effective_config)?;
    let mortality_resolution = phase9_mortality_commitment(&mut state.world)?;
    let phase9b = apply_phase9b(state, &effective_config, run_context, executed_day)?;
    if let Some(callback) = post_phase9b {
        let newborn_ids: Vec<_> = phase9b.births.iter().map(|birth| birth.child).collect();
        callback(
            &state.world,
            &state.relationship_graph,
            &newborn_ids,
            executed_day,
        )?;
    }

    let mut event_buffer = if options.events_enabled {
        let estimated_cap = state
            .world
            .agents
            .len()
            .saturating_mul(2)
            .saturating_add(state.world.settlements.len())
            .saturating_add(4);
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
            for event in &mut targeted_events {
                event.key.local_sequence += offset;
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
        let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(state.world.agents.len());
        let base = phase10_observe_with_scratch(&state.world, executed_day, &mut metrics_scratch)?;
        if options.events_enabled {
            event_buffer.push(event_from_daily_metrics(&base));
        }
        let (cross_partition_edge_count, total_directed_edge_count) =
            state.relationship_graph.edge_ratio_counts();
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
            live_population: state
                .world
                .agents
                .iter()
                .filter(|agent| agent.alive)
                .count() as u64,
            next_agent_id: state.next_agent_id,
        })
    } else {
        None
    };

    let snapshot = if options.snapshot_boundary {
        let metadata = SnapshotMetadata::new(
            next_day,
            run_context.master_seed,
            run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_m3p_snapshot(state, &metadata)?;
        if options.events_enabled {
            event_buffer.push(EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: M3P_SNAPSHOT_SCHEMA_VERSION,
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

    state.world.current_day = SimulationDay(next_day);
    Ok(M3pDayOutcome {
        executed_day,
        metrics,
        events,
        snapshot,
    })
}

#[derive(Debug)]
pub(crate) struct BirthRecord {
    pub(crate) parent: AgentId,
    pub(crate) child: AgentId,
    pub(crate) group: GroupId,
}

#[derive(Debug)]
pub(crate) struct Phase9bResult {
    pub(crate) births: Vec<BirthRecord>,
    pub(crate) removed: u64,
    pub(crate) replaced_edges: u64,
    pub(crate) dropped_edges: u64,
    pub(crate) compacted: bool,
}

#[derive(Debug)]
struct Phase9bPlan {
    agents: Vec<AgentState>,
    graph_update: Phase9bGraphUpdate,
    next_agent_id: u64,
    result: Phase9bResult,
}

#[derive(Clone, Copy)]
struct M3pPhase9bStateView<'a> {
    world: &'a WorldState,
    relationship_graph: &'a RelationshipGraph,
    next_agent_id: u64,
    relationship_degree_k: u32,
    p_local_bits: u32,
}

#[derive(Debug)]
enum Phase9bGraphUpdate {
    Replace(RelationshipGraph),
    Append(RelationshipGraphAppendPlan),
    Unchanged,
}

#[derive(Debug, Default)]
pub(crate) struct M3pIncrementalTopologyScratch {
    pub(crate) newborn_rows: Vec<(AgentId, GroupId, Vec<AgentId>)>,
    pub(crate) csr_row_ends: Vec<usize>,
    pub(crate) projection_row_ends: Vec<usize>,
    pub(crate) projection_neighbor_slots: Vec<usize>,
    sampling_index: M3pCandidateIndexScratch,
    eligible_parents: Vec<AgentId>,
    successful_parents: Vec<AgentId>,
    agent_slots: Vec<(AgentId, usize)>,
    parent_food_updates: Vec<(usize, f32)>,
    newborn_agents: Vec<AgentState>,
    planned_agents: Vec<AgentState>,
}

pub(crate) struct Phase9bRunParameters<'a> {
    pub(crate) config: &'a SimConfig,
    pub(crate) run_context: &'a M0RunContext,
    pub(crate) day: u32,
    pub(crate) relationship_degree_k: u32,
    pub(crate) p_local_bits: u32,
}

struct M3pPhase9bStorageAppendTarget<'a> {
    storage: &'a mut SegmentedAgentStorage,
    relationship_graph: &'a mut RelationshipGraph,
    phase4_projection: &'a mut M3Phase4SlotProjection,
    next_agent_id: &'a mut u64,
}

fn apply_phase9b(
    state: &mut M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    day: u32,
) -> Result<Phase9bResult, M0RunError> {
    let plan = plan_phase9b(state, config, run_context, day, None)?;
    let Phase9bPlan {
        agents,
        graph_update,
        next_agent_id,
        result,
    } = plan;
    let Phase9bGraphUpdate::Replace(replacement_graph) = graph_update else {
        return Err(invariant(
            "M3P reference Phase 9b did not produce a replacement graph",
        ));
    };
    state.world.agents = agents;
    state.relationship_graph = replacement_graph;
    state.next_agent_id = next_agent_id;
    Ok(result)
}

/// Plans M3P Phase 9b against storage-derived logical state, then commits to SoA columns.
pub(crate) fn apply_phase9b_storage(
    storage: &mut SegmentedAgentStorage,
    world: &WorldState,
    relationship_graph: &mut RelationshipGraph,
    next_agent_id: &mut u64,
    config: &SimConfig,
    run_context: &M0RunContext,
    day: u32,
) -> Result<Phase9bResult, M0RunError> {
    let source_len = storage.len();
    let planning_world = WorldState {
        current_day: world.current_day,
        agents: storage.to_agents(),
        settlements: world.settlements.clone(),
        initial_money_supply: world.initial_money_supply,
    };
    let planning_state = M3pReferenceState::from_parts(
        planning_world,
        relationship_graph.clone(),
        *next_agent_id,
        config,
        run_context,
    )?;
    let plan = plan_phase9b(&planning_state, config, run_context, day, None)?;
    if !plan.result.compacted && plan.agents.len() < source_len {
        return Err(invariant(
            "M3P storage Phase 9b removed agents without compaction",
        ));
    }

    let Phase9bPlan {
        agents,
        graph_update,
        next_agent_id: planned_next_agent_id,
        result,
    } = plan;
    let Phase9bGraphUpdate::Replace(replacement_graph) = graph_update else {
        return Err(invariant(
            "M3P storage reference Phase 9b did not produce a replacement graph",
        ));
    };
    if result.compacted {
        storage.clear();
        storage.reserve(agents.len());
        for (slot, planned_agent) in agents.iter().enumerate() {
            let mut agent = planned_agent.clone();
            agent.dense_slot = DenseSlot(slot as u32);
            storage.push(&agent);
        }
    } else {
        for (slot, planned_agent) in agents.iter().take(source_len).enumerate() {
            storage.economy.food[slot] = planned_agent.food;
        }
        for newborn in agents.iter().skip(source_len) {
            storage.push(newborn);
        }
    }
    *relationship_graph = replacement_graph;
    *next_agent_id = planned_next_agent_id;
    Ok(result)
}

pub(crate) fn apply_phase9b_storage_incremental(
    storage: &mut SegmentedAgentStorage,
    world: &WorldState,
    relationship_graph: &mut RelationshipGraph,
    phase4_projection: &mut M3Phase4SlotProjection,
    next_agent_id: &mut u64,
    parameters: Phase9bRunParameters<'_>,
    scratch: &mut M3pIncrementalTopologyScratch,
) -> Result<Phase9bResult, M0RunError> {
    let Phase9bRunParameters {
        config,
        run_context,
        day,
        relationship_degree_k,
        p_local_bits,
    } = parameters;
    let population_config = config
        .population
        .as_ref()
        .ok_or_else(|| invariant("M3P profile requires [population] settings"))?;
    if !day.is_multiple_of(population_config.compaction_interval_days) {
        return apply_phase9b_storage_incremental_append(
            M3pPhase9bStorageAppendTarget {
                storage,
                relationship_graph,
                phase4_projection,
                next_agent_id,
            },
            Phase9bRunParameters {
                config,
                run_context,
                day,
                relationship_degree_k,
                p_local_bits,
            },
            scratch,
        );
    }
    let source_len = storage.len();
    let planning_world = WorldState {
        current_day: world.current_day,
        agents: storage.to_agents(),
        settlements: world.settlements.clone(),
        initial_money_supply: world.initial_money_supply,
    };
    // The public incremental runner validates the complete state before its day loop; within
    // that loop structural changes are limited to this function's checked topology append.
    let state_view = M3pPhase9bStateView {
        world: &planning_world,
        relationship_graph,
        next_agent_id: *next_agent_id,
        relationship_degree_k,
        p_local_bits,
    };
    let plan = plan_phase9b_from_view(&state_view, config, run_context, day, Some(scratch))?;
    if !plan.result.compacted && plan.agents.len() < source_len {
        return Err(invariant(
            "M3P storage Phase 9b removed agents without compaction",
        ));
    }

    let Phase9bPlan {
        agents,
        graph_update,
        next_agent_id: planned_next_agent_id,
        result,
    } = plan;
    match graph_update {
        Phase9bGraphUpdate::Replace(next_graph) => {
            if result.compacted {
                storage.clear();
                storage.reserve(agents.len());
                for (slot, planned_agent) in agents.iter().enumerate() {
                    let mut agent = planned_agent.clone();
                    agent.dense_slot = DenseSlot(slot as u32);
                    storage.push(&agent);
                }
            } else {
                storage.reserve(agents.len().saturating_sub(source_len));
                for (slot, planned_agent) in agents.iter().take(source_len).enumerate() {
                    storage.economy.food[slot] = planned_agent.food;
                }
                for newborn in agents.iter().skip(source_len) {
                    storage.push(newborn);
                }
            }
            *relationship_graph = next_graph;
        }
        Phase9bGraphUpdate::Append(append_plan) => {
            let rows = &scratch.newborn_rows;
            let max_degree = phase4_projection.stage_appended_rows(
                rows,
                storage,
                &mut scratch.projection_row_ends,
                &mut scratch.projection_neighbor_slots,
            )?;
            storage.reserve(rows.len());
            relationship_graph.reserve_appended_rows(&append_plan);
            phase4_projection.reserve_appended_rows(
                scratch.projection_row_ends.len(),
                scratch.projection_neighbor_slots.len(),
            );
            relationship_graph.append_prevalidated_rows(rows, &scratch.csr_row_ends, append_plan);
            for (slot, planned_agent) in agents.iter().take(source_len).enumerate() {
                storage.economy.food[slot] = planned_agent.food;
            }
            for newborn in agents.iter().skip(source_len) {
                storage.push(newborn);
            }
            phase4_projection.append_staged_rows(
                &scratch.projection_row_ends,
                &scratch.projection_neighbor_slots,
                max_degree,
            );
        }
        Phase9bGraphUpdate::Unchanged => {
            for (slot, planned_agent) in agents.iter().take(source_len).enumerate() {
                storage.economy.food[slot] = planned_agent.food;
            }
        }
    }
    *next_agent_id = planned_next_agent_id;
    scratch.planned_agents = agents;
    Ok(result)
}

fn apply_phase9b_storage_incremental_append(
    target: M3pPhase9bStorageAppendTarget<'_>,
    parameters: Phase9bRunParameters<'_>,
    scratch: &mut M3pIncrementalTopologyScratch,
) -> Result<Phase9bResult, M0RunError> {
    let M3pPhase9bStorageAppendTarget {
        storage,
        relationship_graph,
        phase4_projection,
        next_agent_id,
    } = target;
    let Phase9bRunParameters {
        config,
        run_context,
        day,
        relationship_degree_k,
        p_local_bits,
    } = parameters;
    let population_config = config
        .population
        .as_ref()
        .ok_or_else(|| invariant("M3P profile requires [population] settings"))?;
    let alive = storage.alive();
    let health = storage.health();
    let food = storage.food();
    let agent_ids = storage.agent_ids();
    let group_ids = storage.group_ids();
    scratch.sampling_index.fill_members(
        (0..storage.len())
            .filter_map(|slot| alive[slot].then_some((agent_ids[slot], group_ids[slot]))),
    );
    let live_population = u64::try_from(scratch.sampling_index.members.len())
        .map_err(|_| invariant("M3P live population count overflow"))?;
    let candidate_index =
        scratch
            .sampling_index
            .build(f32::from_bits(p_local_bits), *run_context, day)?;

    scratch.eligible_parents.clear();
    scratch
        .eligible_parents
        .extend((0..storage.len()).filter_map(|slot| {
            (alive[slot]
                && health[slot] >= population_config.birth_min_health
                && food[slot] >= population_config.birth_food_threshold)
                .then_some(agent_ids[slot])
        }));
    scratch.eligible_parents.sort_unstable();

    scratch.successful_parents.clear();
    for parent_id in scratch.eligible_parents.iter().copied() {
        let coordinate = RngCoordinate::new(
            run_context.master_seed,
            run_context.replicate_id,
            day,
            9,
            M3P_BIRTH_SUBSYSTEM_ID,
            parent_id.0,
            0,
        );
        if coordinate_prng_f32(&coordinate) < population_config.birth_probability {
            scratch.successful_parents.push(parent_id);
        }
    }
    if population_config.births_max_per_day != 0 {
        scratch
            .successful_parents
            .truncate(population_config.births_max_per_day as usize);
    }

    let birth_count = u64::try_from(scratch.successful_parents.len())
        .map_err(|_| invariant("M3P daily birth count overflow"))?;
    let planned_next_agent_id = next_agent_id
        .checked_add(birth_count)
        .filter(|next| *next <= AGENT_ID_EXHAUSTED)
        .ok_or_else(|| invariant("M3P AgentId space exhausted"))?;
    if birth_count > 0 && *next_agent_id >= AGENT_ID_EXHAUSTED {
        return Err(invariant("M3P AgentId space exhausted"));
    }
    validate_live_population_after_births(live_population, birth_count)?;

    let cost = population_config.birth_food_cost;
    let degree_k = relationship_degree_k as usize;
    let mut births = Vec::with_capacity(scratch.successful_parents.len());
    scratch.parent_food_updates.clear();
    scratch.newborn_agents.clear();
    scratch.newborn_rows.clear();
    for (offset, parent_id) in scratch.successful_parents.iter().copied().enumerate() {
        let slot = storage
            .slot_of(parent_id)
            .ok_or_else(|| invariant("M3P birth parent is missing"))?;
        let parent = storage
            .agent_at(slot)
            .ok_or_else(|| invariant("M3P birth parent is missing"))?;
        if !parent.alive
            || parent.health < population_config.birth_min_health
            || parent.food < population_config.birth_food_threshold
            || !parent.food.is_finite()
            || parent.food < cost
        {
            return Err(invariant("M3P birth parent failed Phase 9b validation"));
        }
        let child_id_u64 = next_agent_id
            .checked_add(offset as u64)
            .ok_or_else(|| invariant("M3P AgentId space exhausted"))?;
        let child_id = AgentId(
            u32::try_from(child_id_u64).map_err(|_| invariant("M3P AgentId space exhausted"))?,
        );
        let mut parent_food = parent.food;
        parent_food -= cost;
        scratch.parent_food_updates.push((slot, parent_food));
        births.push(BirthRecord {
            parent: parent_id,
            child: child_id,
            group: parent.group_id,
        });
        scratch.newborn_agents.push(AgentState {
            agent_id: child_id,
            dense_slot: DenseSlot(child_id.0),
            alive: true,
            birth_day: SimulationDay(day),
            health: config.world.initial_health,
            food: cost,
            wealth: 0,
            productivity: parent.productivity,
            cooperation: parent.cooperation,
            aggression: parent.aggression,
            risk_tolerance: parent.risk_tolerance,
            group_id: parent.group_id,
        });
    }

    if births.is_empty() {
        return Ok(Phase9bResult {
            births,
            removed: 0,
            replaced_edges: 0,
            dropped_edges: 0,
            compacted: false,
        });
    }

    for birth in &births {
        let row = build_newborn_row(
            birth.parent,
            birth.child,
            birth.group,
            degree_k,
            &candidate_index,
        )?;
        scratch.newborn_rows.push((birth.child, birth.group, row));
    }
    let append_plan = relationship_graph.plan_append_rows(
        &scratch.newborn_rows,
        candidate_index.members,
        &mut scratch.csr_row_ends,
    )?;
    let max_degree = phase4_projection.stage_appended_rows(
        &scratch.newborn_rows,
        storage,
        &mut scratch.projection_row_ends,
        &mut scratch.projection_neighbor_slots,
    )?;

    storage.reserve(scratch.newborn_agents.len());
    relationship_graph.reserve_appended_rows(&append_plan);
    phase4_projection.reserve_appended_rows(
        scratch.projection_row_ends.len(),
        scratch.projection_neighbor_slots.len(),
    );
    for &(slot, food_after_birth_cost) in &scratch.parent_food_updates {
        storage.economy.food[slot] = food_after_birth_cost;
    }
    for newborn in &scratch.newborn_agents {
        storage.push(newborn);
    }
    relationship_graph.append_prevalidated_rows(
        &scratch.newborn_rows,
        &scratch.csr_row_ends,
        append_plan,
    );
    phase4_projection.append_staged_rows(
        &scratch.projection_row_ends,
        &scratch.projection_neighbor_slots,
        max_degree,
    );
    *next_agent_id = planned_next_agent_id;

    Ok(Phase9bResult {
        births,
        removed: 0,
        replaced_edges: 0,
        dropped_edges: 0,
        compacted: false,
    })
}

fn plan_phase9b(
    state: &M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    day: u32,
    incremental_scratch: Option<&mut M3pIncrementalTopologyScratch>,
) -> Result<Phase9bPlan, M0RunError> {
    let state_view = M3pPhase9bStateView {
        world: &state.world,
        relationship_graph: &state.relationship_graph,
        next_agent_id: state.next_agent_id,
        relationship_degree_k: state.relationship_degree_k,
        p_local_bits: state.p_local_bits,
    };
    plan_phase9b_from_view(&state_view, config, run_context, day, incremental_scratch)
}

fn plan_phase9b_from_view(
    state: &M3pPhase9bStateView<'_>,
    config: &SimConfig,
    run_context: &M0RunContext,
    day: u32,
    incremental_scratch: Option<&mut M3pIncrementalTopologyScratch>,
) -> Result<Phase9bPlan, M0RunError> {
    let M3pPhase9bStateView {
        world,
        relationship_graph,
        next_agent_id,
        relationship_degree_k,
        p_local_bits,
    } = *state;
    let population_config = config
        .population
        .as_ref()
        .ok_or_else(|| invariant("M3P profile requires [population] settings"))?;
    let mut local_sampling_index = M3pCandidateIndexScratch::default();
    let mut local_eligible_parents = Vec::new();
    let mut local_successful_parents = Vec::new();
    let (
        sampling_index,
        mut newborn_rows_scratch,
        mut csr_row_ends_scratch,
        eligible_parents,
        successful_parents,
        mut agent_slots_scratch,
        planned_agents_scratch,
    ) = match incremental_scratch {
        Some(scratch) => (
            &mut scratch.sampling_index,
            Some(&mut scratch.newborn_rows),
            Some(&mut scratch.csr_row_ends),
            &mut scratch.eligible_parents,
            &mut scratch.successful_parents,
            Some(&mut scratch.agent_slots),
            Some(&mut scratch.planned_agents),
        ),
        None => (
            &mut local_sampling_index,
            None,
            None,
            &mut local_eligible_parents,
            &mut local_successful_parents,
            None,
            None,
        ),
    };
    sampling_index.fill_members(
        world
            .agents
            .iter()
            .filter(|agent| agent.alive)
            .map(|agent| (agent.agent_id, agent.group_id)),
    );
    let live_population = u64::try_from(sampling_index.members.len())
        .map_err(|_| invariant("M3P live population count overflow"))?;
    let candidate_index = sampling_index.build(f32::from_bits(p_local_bits), *run_context, day)?;

    eligible_parents.clear();
    eligible_parents.extend(
        world
            .agents
            .iter()
            .filter(|agent| {
                agent.alive
                    && agent.health >= population_config.birth_min_health
                    && agent.food >= population_config.birth_food_threshold
            })
            .map(|agent| agent.agent_id),
    );
    eligible_parents.sort_unstable();

    successful_parents.clear();
    for parent_id in eligible_parents.iter().copied() {
        let coordinate = RngCoordinate::new(
            run_context.master_seed,
            run_context.replicate_id,
            day,
            9,
            M3P_BIRTH_SUBSYSTEM_ID,
            parent_id.0,
            0,
        );
        if coordinate_prng_f32(&coordinate) < population_config.birth_probability {
            successful_parents.push(parent_id);
        }
    }
    if population_config.births_max_per_day != 0 {
        successful_parents.truncate(population_config.births_max_per_day as usize);
    }

    let birth_count = u64::try_from(successful_parents.len())
        .map_err(|_| invariant("M3P daily birth count overflow"))?;
    let planned_next_agent_id = next_agent_id
        .checked_add(birth_count)
        .filter(|next| *next <= AGENT_ID_EXHAUSTED)
        .ok_or_else(|| invariant("M3P AgentId space exhausted"))?;
    if birth_count > 0 && next_agent_id >= AGENT_ID_EXHAUSTED {
        return Err(invariant("M3P AgentId space exhausted"));
    }
    validate_live_population_after_births(live_population, birth_count)?;

    let mut legacy_agent_slots = BTreeMap::new();
    if let Some(agent_slots) = agent_slots_scratch.as_mut() {
        agent_slots.clear();
        for (slot, agent) in world.agents.iter().enumerate() {
            agent_slots.push((agent.agent_id, slot));
        }
        agent_slots.sort_unstable_by_key(|&(agent_id, _)| agent_id);
        if agent_slots.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(invariant("duplicate AgentId during M3P Phase 9b planning"));
        }
    } else {
        for (slot, agent) in world.agents.iter().enumerate() {
            if legacy_agent_slots.insert(agent.agent_id, slot).is_some() {
                return Err(invariant("duplicate AgentId during M3P Phase 9b planning"));
            }
        }
    }
    let cost = population_config.birth_food_cost;
    let mut planned_agents = if let Some(planned_agents) = planned_agents_scratch {
        let mut reusable = std::mem::take(planned_agents);
        reusable.clear();
        reusable.extend_from_slice(&world.agents);
        reusable
    } else {
        world.agents.clone()
    };
    let mut births = Vec::with_capacity(successful_parents.len());
    let degree_k = relationship_degree_k as usize;

    for (offset, parent_id) in successful_parents.iter().copied().enumerate() {
        let slot = if let Some(agent_slots) = agent_slots_scratch.as_ref() {
            let parent_slot = agent_slots
                .binary_search_by_key(&parent_id, |&(agent_id, _)| agent_id)
                .map_err(|_| invariant("M3P birth parent is missing"))?;
            agent_slots[parent_slot].1
        } else {
            *legacy_agent_slots
                .get(&parent_id)
                .ok_or_else(|| invariant("M3P birth parent is missing"))?
        };
        let parent = &planned_agents[slot];
        if !parent.alive
            || parent.health < population_config.birth_min_health
            || parent.food < population_config.birth_food_threshold
            || !parent.food.is_finite()
            || parent.food < cost
        {
            return Err(invariant("M3P birth parent failed Phase 9b validation"));
        }
        let child_id_u64 = next_agent_id
            .checked_add(offset as u64)
            .ok_or_else(|| invariant("M3P AgentId space exhausted"))?;
        let child_id = AgentId(
            u32::try_from(child_id_u64).map_err(|_| invariant("M3P AgentId space exhausted"))?,
        );
        let parent_snapshot = parent.clone();
        planned_agents[slot].food -= cost;
        let child = AgentState {
            agent_id: child_id,
            dense_slot: DenseSlot(child_id.0),
            alive: true,
            birth_day: SimulationDay(day),
            health: config.world.initial_health,
            food: cost,
            wealth: 0,
            productivity: parent_snapshot.productivity,
            cooperation: parent_snapshot.cooperation,
            aggression: parent_snapshot.aggression,
            risk_tolerance: parent_snapshot.risk_tolerance,
            group_id: parent_snapshot.group_id,
        };
        births.push(BirthRecord {
            parent: parent_id,
            child: child_id,
            group: parent_snapshot.group_id,
        });
        planned_agents.push(child);
    }

    let compacted = day.is_multiple_of(population_config.compaction_interval_days);
    let use_incremental_append = newborn_rows_scratch.is_some() && !compacted;
    let mut rows = BTreeMap::new();
    if !use_incremental_append {
        for &source_id in relationship_graph.source_ids() {
            let neighbors = relationship_graph
                .neighbors_of(source_id)
                .ok_or_else(|| invariant("M3P graph source row is missing"))?;
            rows.insert(source_id, neighbors.to_vec());
        }
    }
    if let Some(rows) = newborn_rows_scratch.as_mut() {
        rows.clear();
    }
    for birth in &births {
        let row = build_newborn_row(
            birth.parent,
            birth.child,
            birth.group,
            degree_k,
            &candidate_index,
        )?;
        if use_incremental_append {
            newborn_rows_scratch
                .as_mut()
                .expect("incremental append requires scratch")
                .push((birth.child, birth.group, row));
        } else if rows.insert(birth.child, row).is_some() {
            return Err(invariant("duplicate M3P newborn graph source"));
        }
    }

    let mut removed = 0u64;
    let mut replaced_edges = 0u64;
    let mut dropped_edges = 0u64;
    if compacted {
        let dead_ids: BTreeSet<_> = planned_agents
            .iter()
            .filter(|agent| !agent.alive)
            .map(|agent| agent.agent_id)
            .collect();
        removed =
            u64::try_from(dead_ids.len()).map_err(|_| invariant("M3P cleanup count overflow"))?;
        let groups_by_id: BTreeMap<_, _> = planned_agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id))
            .collect();
        let mut surviving_rows = BTreeMap::new();
        for (source_id, original_row) in rows {
            if dead_ids.contains(&source_id) {
                continue;
            }
            let source_group = *groups_by_id
                .get(&source_id)
                .ok_or_else(|| invariant("M3P surviving graph source is missing"))?;
            let mut updated_row = Vec::with_capacity(original_row.len());
            for (position, &target_id) in original_row.iter().enumerate() {
                if !dead_ids.contains(&target_id) {
                    updated_row.push(target_id);
                    continue;
                }
                let mut excluded = original_row
                    .iter()
                    .enumerate()
                    .filter_map(|(other_position, &other)| {
                        (other_position != position).then_some(other)
                    })
                    .collect::<Vec<_>>();
                excluded.extend(updated_row.iter().copied());
                let draw_start = 1_000_000u32
                    .checked_add(2u32.saturating_mul(position as u32))
                    .ok_or_else(|| invariant("M3P replacement draw index overflow"))?;
                match candidate_index.sample_target(
                    source_id,
                    source_group,
                    &excluded,
                    draw_start,
                )? {
                    Some(replacement) => {
                        updated_row.push(replacement);
                        replaced_edges = replaced_edges
                            .checked_add(1)
                            .ok_or_else(|| invariant("M3P replaced-edge count overflow"))?;
                    }
                    None => {
                        dropped_edges = dropped_edges
                            .checked_add(1)
                            .ok_or_else(|| invariant("M3P dropped-edge count overflow"))?;
                    }
                }
            }
            updated_row.sort_unstable();
            if updated_row.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(invariant("M3P compaction produced a duplicate graph edge"));
            }
            surviving_rows.insert(source_id, updated_row);
        }
        rows = surviving_rows;
        planned_agents.retain(|agent| agent.alive);
        planned_agents.sort_by_key(|agent| agent.agent_id);
    }

    let graph_update = if compacted {
        let rows: Vec<_> = rows.into_iter().collect();
        let memberships: Vec<_> = planned_agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id))
            .collect();
        let next_graph = if newborn_rows_scratch.is_some() {
            RelationshipGraph::from_canonical_rows_dirty(rows, &memberships)?
        } else {
            RelationshipGraph::from_canonical_rows(rows, &memberships)?
        };
        Phase9bGraphUpdate::Replace(next_graph)
    } else if use_incremental_append {
        let rows = &**newborn_rows_scratch
            .as_ref()
            .expect("incremental append requires scratch");
        if births.is_empty() {
            Phase9bGraphUpdate::Unchanged
        } else {
            let row_ends: &mut Vec<usize> = csr_row_ends_scratch
                .as_mut()
                .expect("incremental append requires row-offset scratch");
            Phase9bGraphUpdate::Append(relationship_graph.plan_append_rows(
                rows,
                candidate_index.members,
                row_ends,
            )?)
        }
    } else if !births.is_empty() {
        let rows: Vec<_> = rows.into_iter().collect();
        let memberships: Vec<_> = planned_agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id))
            .collect();
        Phase9bGraphUpdate::Replace(RelationshipGraph::from_canonical_rows(rows, &memberships)?)
    } else {
        Phase9bGraphUpdate::Replace(relationship_graph.clone())
    };
    let next_agent_id = if births.is_empty() {
        next_agent_id
    } else {
        planned_next_agent_id
    };
    if let Phase9bGraphUpdate::Replace(graph) = &graph_update {
        validate_state_parts(&planned_agents, graph, next_agent_id)?;
    }

    Ok(Phase9bPlan {
        agents: planned_agents,
        graph_update,
        next_agent_id,
        result: Phase9bResult {
            births,
            removed,
            replaced_edges,
            dropped_edges,
            compacted,
        },
    })
}

fn build_newborn_row(
    parent_id: AgentId,
    child_id: AgentId,
    child_group: GroupId,
    degree_k: usize,
    candidate_index: &M3pCandidateIndex,
) -> Result<Vec<AgentId>, M0RunError> {
    if degree_k == 0 {
        return Ok(Vec::new());
    }
    let mut row = vec![parent_id];
    for edge_position in 1..degree_k {
        let draw_start = 2u32
            .checked_add(2u32.saturating_mul((edge_position - 1) as u32))
            .ok_or_else(|| invariant("M3P newborn draw index overflow"))?;
        let mut excluded = row.clone();
        excluded.push(child_id);
        if let Some(target) =
            candidate_index.sample_target(child_id, child_group, &excluded, draw_start)?
        {
            row.push(target);
        }
    }
    row.sort_unstable();
    Ok(row)
}

#[derive(Debug, Default)]
struct M3pCandidateIndexScratch {
    members: Vec<(AgentId, GroupId)>,
    group_ranks: BTreeMap<GroupId, Vec<usize>>,
}

impl M3pCandidateIndexScratch {
    fn fill_members(&mut self, members: impl Iterator<Item = (AgentId, GroupId)>) {
        self.members.clear();
        self.members.extend(members);
    }

    fn build(
        &mut self,
        p_local: f32,
        run_context: M0RunContext,
        day: u32,
    ) -> Result<M3pCandidateIndex<'_>, M0RunError> {
        self.members.sort_unstable_by_key(|&(agent_id, _)| agent_id);
        if self.members.len() as u64 >= M3P_MAX_LIVE_POPULATION {
            return Err(invariant("M3P live population must be below 2^24"));
        }
        if let Some(pair) = self.members.windows(2).find(|pair| pair[0].0 == pair[1].0) {
            return Err(invariant(format!(
                "duplicate M3P candidate AgentId {}",
                pair[0].0
            )));
        }
        for ranks in self.group_ranks.values_mut() {
            ranks.clear();
        }
        for (rank, &(_, group_id)) in self.members.iter().enumerate() {
            self.group_ranks.entry(group_id).or_default().push(rank);
        }
        Ok(M3pCandidateIndex {
            members: &self.members,
            group_ranks: &self.group_ranks,
            p_local,
            run_context,
            day,
        })
    }
}

struct M3pCandidateIndex<'a> {
    members: &'a [(AgentId, GroupId)],
    group_ranks: &'a BTreeMap<GroupId, Vec<usize>>,
    p_local: f32,
    run_context: M0RunContext,
    day: u32,
}

impl M3pCandidateIndex<'_> {
    fn sample_target(
        &self,
        source_id: AgentId,
        source_group: GroupId,
        excluded_ids: &[AgentId],
        draw_start: u32,
    ) -> Result<Option<AgentId>, M0RunError> {
        let source_group_ranks = self
            .group_ranks
            .get(&source_group)
            .map_or(&[][..], Vec::as_slice);
        let mut excluded_local = Vec::new();
        let mut excluded_cross = Vec::new();
        let mut ids = excluded_ids.to_vec();
        ids.push(source_id);
        ids.sort_unstable();
        ids.dedup();
        for excluded_id in ids {
            let Ok(global_rank) = self
                .members
                .binary_search_by_key(&excluded_id, |entry| entry.0)
            else {
                continue;
            };
            if self.members[global_rank].1 == source_group {
                let local_rank = source_group_ranks
                    .binary_search(&global_rank)
                    .map_err(|_| invariant("M3P local rank index is inconsistent"))?;
                excluded_local.push(local_rank);
            } else {
                let group_prefix = source_group_ranks.partition_point(|&rank| rank <= global_rank);
                excluded_cross.push(global_rank - group_prefix);
            }
        }
        excluded_local.sort_unstable();
        excluded_local.dedup();
        excluded_cross.sort_unstable();
        excluded_cross.dedup();

        let locality = RngCoordinate::new(
            self.run_context.master_seed,
            self.run_context.replicate_id,
            self.day,
            9,
            M3P_BIRTH_SUBSYSTEM_ID,
            source_id.0,
            draw_start,
        );
        let is_local = coordinate_prng_f32(&locality) < self.p_local;
        let excluded = if is_local {
            &excluded_local
        } else {
            &excluded_cross
        };
        let pool_size = if is_local {
            source_group_ranks.len()
        } else {
            self.members.len() - source_group_ranks.len()
        };
        let remaining = pool_size
            .checked_sub(excluded.len())
            .ok_or_else(|| invariant("M3P exclusion count exceeds its candidate pool"))?;
        if remaining == 0 {
            return Ok(None);
        }

        let target_coordinate = RngCoordinate::new(
            self.run_context.master_seed,
            self.run_context.replicate_id,
            self.day,
            9,
            M3P_BIRTH_SUBSYSTEM_ID,
            source_id.0,
            draw_start
                .checked_add(1)
                .ok_or_else(|| invariant("M3P target draw index overflow"))?,
        );
        let u = coordinate_prng_f32(&target_coordinate);
        let draw_rank = ((u * remaining as f32).floor() as usize).min(remaining - 1);
        let base_rank = adjust_rank_for_exclusions(draw_rank, excluded);
        let global_rank = if is_local {
            *source_group_ranks
                .get(base_rank)
                .ok_or_else(|| invariant("M3P local rank is outside its candidate pool"))?
        } else {
            complement_rank_to_global_rank(source_group_ranks, self.members.len(), base_rank)
        };
        self.members
            .get(global_rank)
            .map(|entry| Some(entry.0))
            .ok_or_else(|| invariant("M3P selected rank is outside its candidate index"))
    }
}

fn validate_live_population_after_births(live: u64, births: u64) -> Result<(), M0RunError> {
    let after_births = live
        .checked_add(births)
        .ok_or_else(|| invariant("M3P live population count overflow"))?;
    if after_births >= M3P_MAX_LIVE_POPULATION {
        return Err(invariant(
            "M3P live population after births must be below 2^24",
        ));
    }
    Ok(())
}

fn validate_state_parts(
    agents: &[AgentState],
    graph: &RelationshipGraph,
    next_agent_id: u64,
) -> Result<(), M0RunError> {
    if next_agent_id > AGENT_ID_EXHAUSTED {
        return Err(invariant(
            "M3P next_agent_id exceeds the exhausted sentinel",
        ));
    }
    let mut ids: Vec<_> = agents.iter().map(|agent| agent.agent_id).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invariant("duplicate AgentId in M3P world state"));
    }
    if graph.source_ids() != ids {
        return Err(invariant(
            "M3P graph source IDs do not match canonical state AgentIds",
        ));
    }
    if let Some(max_agent_id) = ids.last()
        && next_agent_id <= max_agent_id.0 as u64
    {
        return Err(invariant(
            "M3P next_agent_id is not above allocated AgentIds",
        ));
    }
    let live = agents.iter().filter(|agent| agent.alive).count() as u64;
    if live >= M3P_MAX_LIVE_POPULATION {
        return Err(invariant("M3P live population must be below 2^24"));
    }
    for &source_id in graph.source_ids() {
        let neighbors = graph
            .neighbors_of(source_id)
            .ok_or_else(|| invariant("M3P graph source row is missing"))?;
        if neighbors
            .iter()
            .any(|neighbor| ids.binary_search(neighbor).is_err())
        {
            return Err(invariant("M3P CSR contains a dangling edge target"));
        }
    }
    Ok(())
}

fn ensure_m3p_config(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile != SimulationProfile::M3p {
        return Err(invariant("M3P reference runner requires profile = \"m3p\""));
    }
    if config.effective_systems().propagation {
        return Err(invariant(
            "propagation-enabled configuration requires an M3-11 runner",
        ));
    }
    config
        .validate()
        .map_err(|error| invariant(format!("invalid M3P configuration: {error}")))
}

fn invariant(message: impl Into<String>) -> M0RunError {
    M0RunError::InvariantViolation(message.into())
}

#[cfg(test)]
mod tests {
    use super::{M3P_MAX_LIVE_POPULATION, validate_live_population_after_births};

    #[test]
    fn m3p_live_population_cap_is_strict_and_checked() {
        assert!(validate_live_population_after_births(M3P_MAX_LIVE_POPULATION - 1, 0).is_ok());
        assert!(validate_live_population_after_births(M3P_MAX_LIVE_POPULATION - 2, 1).is_ok());
        assert!(validate_live_population_after_births(M3P_MAX_LIVE_POPULATION - 2, 2).is_err());
        assert!(validate_live_population_after_births(M3P_MAX_LIVE_POPULATION, 0).is_err());
        assert!(validate_live_population_after_births(u64::MAX, 1).is_err());
    }
}
