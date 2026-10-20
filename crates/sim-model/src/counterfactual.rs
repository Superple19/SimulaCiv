//! Snapshot-backed paired runs and transient divergence analysis for M4.

use crate::config::{ConfigValidationError, SimConfig, SimulationRunnerSelection};
use crate::decision::Action;
use crate::events::EventRecord;
use crate::hashing::{
    CanonicalHash, CanonicalHashError, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage, canonical_m3p_event_hash, canonical_m3p_metrics_hash,
    canonical_m3p_state_hash_from_storage, canonical_m3pprop_event_hash,
    canonical_m3pprop_metrics_hash, canonical_m3pprop_state_hash_from_storage,
    canonical_m3prop_event_hash, canonical_m3prop_metrics_hash,
    canonical_m3prop_state_hash_from_storage,
};
use crate::m3p::{
    M3pDailyMetrics, M3pEventRecord, M3pReferenceState, decode_m3p_snapshot,
    run_m3p_reference_day_with_phase_recorder,
};
use crate::m3pprop::{
    M3pPropagationDailyMetrics, decode_m3pprop_snapshot,
    run_m3pprop_reference_day_with_phase_recorder,
};
use crate::m3prop::{
    M3PropDailyMetrics, M3PropagationReferenceState, decode_m3prop_snapshot,
    run_m3prop_reference_day_with_phase_recorder,
};
use crate::metrics::M3DailyMetrics;
use crate::propagation::PropagationAgentState;
use crate::relationship_graph::RelationshipGraph;
use crate::runner::{
    DayExecutionOptions, M0RunContext, M0RunError, M3DayOutcome, M3PropagationStorageRunContext,
    M3ReferenceContext, M3StorageRunContext, M3pIncrementalTopologyStorageRunContext,
    M3pPropagationIncrementalTopologyStorageRunContext, M3pPropagationStorageRunContext,
    M3pStorageRegisteredDay, M3pStorageRunContext, run_m3_reference_day_with_phase_recorder,
    run_m3_storage_day_for_branch, run_m3p_storage_day_for_branch,
    run_m3p_storage_day_incremental_for_branch, run_m3pprop_storage_day_for_branch,
    run_m3pprop_storage_day_incremental_for_branch, run_m3prop_storage_day_for_branch,
    run_registered_storage_day_pair_with_recorder,
};
use crate::snapshot::{CanonicalSnapshot, SnapshotError, SnapshotMetadata, decode_snapshot};
use crate::state::{AgentState, AuthorityMode, HybridWorldState, WorldState};
use crate::system_registry::{
    RegisteredSystemRunOptions, SYSTEM_REGISTRY, SystemExecutionOptions, SystemExecutionPolicy,
    SystemId,
};
use sha2::{Digest, Sha256};
use sim_core::{AgentId, Money};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentField {
    Food,
    Health,
    Wealth,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentFieldValue {
    Food(f32),
    Health(f32),
    Wealth(Money),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigParameter {
    CarryingCapacity,
    RegrowthRate,
    BaseMetabolicCost,
    HealthDecayRate,
    BaseWorkYield,
    FoodPrice,
    TargetFood,
    TargetReserve,
    TaxRate,
    WelfarePayment,
    GiftAmount,
    TheftAmount,
    TheftSuccessProbability,
    StarvationThreshold,
    DecisionTemperature,
    ActionBias { action_index: u8 },
    BaseWeight { action_index: u8, feature_index: u8 },
    TraitWeightCooperation,
    TraitWeightAggression,
    TraitWeightRiskTolerance,
    BirthMinHealth,
    BirthFoodThreshold,
    BirthProbability,
    BirthFoodCost,
    BirthsMaxPerDay,
    CompactionIntervalDays,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigValue {
    F32(f32),
    Money(Money),
    U32(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchIntervention {
    ConfigOverride {
        parameter: ConfigParameter,
        value: ConfigValue,
    },
    AgentFieldOverride {
        agent_id: AgentId,
        field: AgentField,
        value: AgentFieldValue,
    },
    ForcedAction {
        agent_id: AgentId,
        action: Action,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchExecutionOption {
    Serial,
    IncrementalTopology,
    ExplicitParallel,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BranchRequest {
    pub snapshot: CanonicalSnapshot,
    pub config: SimConfig,
    pub run_context: M0RunContext,
    pub intervention: BranchIntervention,
    pub horizon_days: u32,
    pub execution: BranchExecutionOption,
    pub snapshot_days: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForcedActionOutcome {
    Applied,
    Fallback,
}

pub(crate) struct ForcedActionDirective {
    pub agent_id: AgentId,
    pub action: Action,
    pub day: u32,
    found_choice: bool,
    pub outcome: Option<ForcedActionOutcome>,
}

impl ForcedActionDirective {
    pub(crate) fn new(agent_id: AgentId, action: Action, day: u32) -> Self {
        Self {
            agent_id,
            action,
            day,
            found_choice: false,
            outcome: None,
        }
    }

    pub(crate) fn apply_choices(
        &mut self,
        day: u32,
        choices: &mut [crate::decision::PrimaryActionChoice],
    ) {
        if day == self.day
            && let Some(choice) = choices
                .iter_mut()
                .find(|choice| choice.agent_id == self.agent_id)
        {
            choice.action = self.action;
            self.found_choice = true;
        }
    }

    pub(crate) fn observe_intents(&mut self, day: u32, intents: &[crate::intents::Intent]) {
        if day != self.day || self.outcome.is_some() {
            return;
        }
        if !self.found_choice {
            self.outcome = Some(ForcedActionOutcome::Fallback);
            return;
        }
        let has_target = intents
            .iter()
            .find(|intent| intent.agent_id() == self.agent_id)
            .is_some_and(|intent| match intent {
                crate::intents::Intent::GiveFood {
                    target_agent_id, ..
                }
                | crate::intents::Intent::StealFood {
                    target_agent_id, ..
                } => target_agent_id.is_some(),
                _ => true,
            });
        self.outcome = Some(if has_target {
            ForcedActionOutcome::Applied
        } else {
            ForcedActionOutcome::Fallback
        });
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchMetricsRecord {
    M3(M3DailyMetrics),
    M3p(M3pDailyMetrics),
    M3Propagation(M3PropDailyMetrics),
    M3pPropagation(M3pPropagationDailyMetrics),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchEventsRecord {
    Standard(Vec<EventRecord>),
    Population(Vec<M3pEventRecord>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BranchDayRecord {
    pub day: u32,
    pub state_digest: CanonicalHash,
    pub metrics: BranchMetricsRecord,
    pub metrics_digest: CanonicalHash,
    pub events: BranchEventsRecord,
    pub events_digest: CanonicalHash,
    pub snapshot: Option<CanonicalSnapshot>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub enum BranchAnalysisMode {
    Full,
    Disabled,
    RecorderComparisonsDisabled { expected: FirstDivergence },
}

#[doc(hidden)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchAnalysisCosts {
    pub digest_ns: u128,
    pub d_d_ns: u128,
    pub lineage_ns: u128,
    pub day_start_clone_ns: u128,
    pub recorder_replay_ns: u128,
    pub recorder_compare_ns: u128,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub struct BranchSimulationResult {
    pub base_days: Vec<BranchSimulationDay>,
    pub branch_days: Vec<BranchSimulationDay>,
    pub forced_action: Option<ForcedActionOutcome>,
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub enum BranchAnalysisOutput {
    Full {
        result: BranchResult,
        costs: BranchAnalysisCosts,
    },
    AnalysisDisabled(BranchSimulationResult),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AgentMatchKey {
    Existing(AgentId),
    Birth {
        ancestor: AgentId,
        ordinal_path: Vec<u32>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DailyDifference {
    pub day: u32,
    pub changed_agent_records: u64,
    pub graph_changed: bool,
    pub allocator_changed: bool,
    pub propagation_changed: bool,
    pub matched_births: Vec<(AgentId, AgentId)>,
    pub unmatched_base_births: Vec<AgentId>,
    pub unmatched_branch_births: Vec<AgentId>,
    pub newborn_crn_loss: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecordedSlot {
    Core(crate::runner::RegisteredCorePhase),
    Hook(crate::system_registry::RegisteredHook),
    InterventionBoundary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FirstDivergence {
    pub day: u32,
    pub slot: RecordedSlot,
    pub first_differing_agent: Option<AgentId>,
    pub differing_components: Vec<&'static str>,
    pub system_id: Option<crate::system_registry::SystemId>,
    pub rng_subsystem_ids: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhaseComponentHashes {
    pub(crate) boundary: crate::runner::RegisteredWorldBoundary,
    pub(crate) agent_rows: Vec<(AgentId, CanonicalHash)>,
    pub(crate) graph: CanonicalHash,
    pub(crate) allocator: Option<CanonicalHash>,
    pub(crate) propagation: Option<CanonicalHash>,
    pub(crate) metrics_so_far: Option<CanonicalHash>,
}

pub(crate) fn record_phase_components(
    boundary: crate::runner::RegisteredWorldBoundary,
    world: &WorldState,
    graph: &RelationshipGraph,
    allocator: Option<u64>,
    propagation: Option<&BTreeMap<AgentId, PropagationAgentState>>,
    metrics_so_far: Option<CanonicalHash>,
) -> Result<PhaseComponentHashes, M0RunError> {
    let mut sorted_agents: Vec<_> = world.agents.iter().collect();
    sorted_agents.sort_by_key(|agent| agent.agent_id);
    let agent_rows = sorted_agents
        .into_iter()
        .map(|agent| {
            (
                agent.agent_id,
                CanonicalHash(Sha256::digest(agent_record_bytes(agent)).into()),
            )
        })
        .collect();
    let allocator =
        allocator.map(|cursor| CanonicalHash(Sha256::digest(cursor.to_le_bytes()).into()));
    let propagation = propagation.map(hash_propagation_rows);
    Ok(PhaseComponentHashes {
        boundary,
        agent_rows,
        graph: CanonicalHash(*graph.csr_hash()),
        allocator,
        propagation,
        metrics_so_far,
    })
}

pub(crate) fn hash_propagation_rows(
    rows: &BTreeMap<AgentId, PropagationAgentState>,
) -> CanonicalHash {
    let mut bytes = Vec::with_capacity(rows.len().saturating_mul(9));
    for (id, row) in rows {
        bytes.extend_from_slice(&id.0.to_le_bytes());
        bytes.push(u8::from(row.informed));
        bytes.extend_from_slice(&row.informed_day.to_le_bytes());
    }
    CanonicalHash(Sha256::digest(bytes).into())
}

pub(crate) fn first_phase_difference(
    base: &[PhaseComponentHashes],
    branch: &[PhaseComponentHashes],
    day: u32,
) -> Option<FirstDivergence> {
    let count = base.len().max(branch.len());
    for index in 0..count {
        let (Some(base), Some(branch)) = (base.get(index), branch.get(index)) else {
            let record = base.get(index).or_else(|| branch.get(index))?;
            return Some(first_divergence_record(
                day,
                record,
                vec!["phase_trace"],
                None,
            ));
        };
        if base.boundary != branch.boundary {
            return Some(first_divergence_record(
                day,
                branch,
                vec!["phase_order"],
                None,
            ));
        }
        let mut differing = Vec::new();
        if base.agent_rows != branch.agent_rows {
            differing.push("agent_rows");
        }
        if base.graph != branch.graph {
            differing.push("graph");
        }
        if base.allocator != branch.allocator {
            differing.push("allocator");
        }
        if base.propagation != branch.propagation {
            differing.push("propagation");
        }
        if base.metrics_so_far != branch.metrics_so_far {
            differing.push("metrics_so_far");
        }
        if !differing.is_empty() {
            let first_agent = if base.agent_rows != branch.agent_rows {
                first_agent_row_difference(&base.agent_rows, &branch.agent_rows)
            } else {
                None
            };
            return Some(first_divergence_record(day, branch, differing, first_agent));
        }
    }
    None
}

fn first_agent_row_difference(
    base: &[(AgentId, CanonicalHash)],
    branch: &[(AgentId, CanonicalHash)],
) -> Option<AgentId> {
    let (mut left, mut right) = (0, 0);
    while left < base.len() && right < branch.len() {
        match base[left].0.cmp(&branch[right].0) {
            std::cmp::Ordering::Less => return Some(base[left].0),
            std::cmp::Ordering::Greater => return Some(branch[right].0),
            std::cmp::Ordering::Equal => {
                if base[left].1 != branch[right].1 {
                    return Some(base[left].0);
                }
                left += 1;
                right += 1;
            }
        }
    }
    base.get(left)
        .map(|(id, _)| *id)
        .or_else(|| branch.get(right).map(|(id, _)| *id))
}

fn first_divergence_record(
    day: u32,
    record: &PhaseComponentHashes,
    differing_components: Vec<&'static str>,
    first_differing_agent: Option<AgentId>,
) -> FirstDivergence {
    first_divergence_at(
        day,
        record.boundary,
        differing_components,
        first_differing_agent,
    )
}

fn first_divergence_at(
    day: u32,
    boundary: crate::runner::RegisteredWorldBoundary,
    differing_components: Vec<&'static str>,
    first_differing_agent: Option<AgentId>,
) -> FirstDivergence {
    let (slot, system_id) = match boundary {
        crate::runner::RegisteredWorldBoundary::Core(phase) => (RecordedSlot::Core(phase), None),
        crate::runner::RegisteredWorldBoundary::Hook(hook) => {
            (RecordedSlot::Hook(hook), Some(hook.system_id))
        }
    };
    let rng_subsystem_ids = system_id
        .and_then(|id| {
            crate::system_registry::SYSTEM_REGISTRY
                .iter()
                .find(|descriptor| descriptor.id == id)
        })
        .map_or_else(Vec::new, |descriptor| descriptor.rng_subsystem_ids.to_vec());
    FirstDivergence {
        day,
        slot,
        first_differing_agent,
        differing_components,
        system_id,
        rng_subsystem_ids,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BranchResult {
    pub base_days: Vec<BranchDayRecord>,
    pub branch_days: Vec<BranchDayRecord>,
    pub first_divergence: Option<FirstDivergence>,
    pub daily_differences: Vec<DailyDifference>,
    pub forced_action: Option<ForcedActionOutcome>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BranchError {
    UnsupportedProfile,
    UnsupportedExecutionOption,
    InvalidSnapshot(SnapshotError),
    SnapshotCursorMismatch,
    InvalidConfig(String),
    InvalidIntervention(String),
    RunIdentityMismatch,
    Runner(M0RunError),
    Hash(CanonicalHashError),
    DayOverflow,
}

impl fmt::Display for BranchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProfile => {
                write!(f, "counterfactual branches require an M3-family profile")
            }
            Self::UnsupportedExecutionOption => {
                write!(f, "parallel branch execution is not supported in M4-02")
            }
            Self::InvalidSnapshot(error) => write!(f, "invalid branch snapshot: {error}"),
            Self::SnapshotCursorMismatch => {
                write!(f, "base and branch snapshot resume cursors differ")
            }
            Self::InvalidConfig(error) => write!(f, "invalid branch configuration: {error}"),
            Self::InvalidIntervention(error) => write!(f, "invalid branch intervention: {error}"),
            Self::RunIdentityMismatch => write!(
                f,
                "branch config and snapshot run identity do not match M0RunContext"
            ),
            Self::Runner(error) => write!(f, "branch runner failed: {error}"),
            Self::Hash(error) => write!(f, "branch digest failed: {error}"),
            Self::DayOverflow => write!(f, "branch horizon overflows simulation day"),
        }
    }
}

impl std::error::Error for BranchError {}

impl From<SnapshotError> for BranchError {
    fn from(value: SnapshotError) -> Self {
        Self::InvalidSnapshot(value)
    }
}
impl From<M0RunError> for BranchError {
    fn from(value: M0RunError) -> Self {
        Self::Runner(value)
    }
}
impl From<CanonicalHashError> for BranchError {
    fn from(value: CanonicalHashError) -> Self {
        Self::Hash(value)
    }
}

impl From<ConfigValidationError> for BranchError {
    fn from(value: ConfigValidationError) -> Self {
        Self::InvalidConfig(value.to_string())
    }
}

fn ensure_snapshot_identity(
    metadata: &SnapshotMetadata,
    run_context: &M0RunContext,
) -> Result<(), BranchError> {
    if metadata.master_seed != run_context.master_seed
        || metadata.replicate_id != run_context.replicate_id
    {
        return Err(BranchError::RunIdentityMismatch);
    }
    Ok(())
}

enum StorageContext {
    M3(M3StorageRunContext),
    M3Propagation(M3PropagationStorageRunContext),
    M3p(M3pStorageRunContext),
    M3pIncremental(M3pIncrementalTopologyStorageRunContext),
    M3pPropagation(M3pPropagationStorageRunContext),
    M3pPropagationIncremental(M3pPropagationIncrementalTopologyStorageRunContext),
}

struct BranchRuntime {
    world: HybridWorldState,
    context: StorageContext,
    profile: SimulationRunnerSelection,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CanonicalAgentRecord {
    alive: bool,
    birth_day: u32,
    health: u32,
    food: u32,
    wealth: Money,
    productivity: u32,
    cooperation: u32,
    aggression: u32,
    risk_tolerance: u32,
    group_id: u16,
}

struct BranchStartState {
    world: HybridWorldState,
    graph: RelationshipGraph,
    allocator: Option<u64>,
    propagation: Option<BTreeMap<AgentId, PropagationAgentState>>,
    profile: SimulationRunnerSelection,
}

impl BranchRuntime {
    fn restore(
        snapshot: &CanonicalSnapshot,
        config: &SimConfig,
        run_context: &M0RunContext,
        execution: BranchExecutionOption,
    ) -> Result<Self, BranchError> {
        if config.world.master_seed != run_context.master_seed
            || config.world.replicate_id != run_context.replicate_id
        {
            return Err(BranchError::RunIdentityMismatch);
        }
        let runner = config.runner_selection();
        let runtime = match runner {
            SimulationRunnerSelection::M3 => {
                let restored = decode_snapshot(snapshot.as_ref())?;
                ensure_snapshot_identity(&restored.metadata, run_context)?;
                let world = HybridWorldState::new(restored.world, AuthorityMode::Hybrid);
                let context = M3StorageRunContext::new(&world, config, run_context)?;
                Self {
                    world,
                    context: StorageContext::M3(context),
                    profile: runner,
                }
            }
            SimulationRunnerSelection::M3Propagation => {
                let restored = decode_m3prop_snapshot(snapshot.as_ref(), config)?;
                ensure_snapshot_identity(&restored.metadata, run_context)?;
                let mut world =
                    HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
                let context = M3PropagationStorageRunContext::from_reference_state(
                    &world,
                    config,
                    run_context,
                    &restored.state,
                )?;
                world.sync_to_world_agents();
                Self {
                    world,
                    context: StorageContext::M3Propagation(context),
                    profile: runner,
                }
            }
            SimulationRunnerSelection::M3p => {
                let restored = decode_m3p_snapshot(snapshot.as_ref(), config)?;
                ensure_snapshot_identity(&restored.metadata, run_context)?;
                let mut world =
                    HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
                world.sync_to_world_agents();
                let context = if execution == BranchExecutionOption::IncrementalTopology {
                    StorageContext::M3pIncremental(
                        M3pIncrementalTopologyStorageRunContext::from_reference_state(
                            &world,
                            config,
                            run_context,
                            &restored.state,
                        )?,
                    )
                } else {
                    StorageContext::M3p(M3pStorageRunContext::from_reference_state(
                        &world,
                        config,
                        run_context,
                        &restored.state,
                    )?)
                };
                Self {
                    world,
                    context,
                    profile: runner,
                }
            }
            SimulationRunnerSelection::M3pPropagation => {
                let restored = decode_m3pprop_snapshot(snapshot.as_ref(), config)?;
                ensure_snapshot_identity(&restored.metadata, run_context)?;
                let mut world =
                    HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
                let context =
                    if execution == BranchExecutionOption::IncrementalTopology {
                        StorageContext::M3pPropagationIncremental(
                        M3pPropagationIncrementalTopologyStorageRunContext::from_reference_state(
                            &world, config, run_context, &restored.state,
                        )?,
                    )
                    } else {
                        StorageContext::M3pPropagation(
                            M3pPropagationStorageRunContext::from_reference_state(
                                &world,
                                config,
                                run_context,
                                &restored.state,
                            )?,
                        )
                    };
                world.sync_to_world_agents();
                Self {
                    world,
                    context,
                    profile: runner,
                }
            }
            SimulationRunnerSelection::M2 => return Err(BranchError::UnsupportedProfile),
        };
        Ok(runtime)
    }

    fn phase_storage_day<'a>(
        &'a mut self,
        config: &SimConfig,
        options: &DayExecutionOptions,
        forced_action: Option<&'a mut ForcedActionDirective>,
    ) -> Result<
        (
            &'a mut crate::storage::SegmentedAgentStorage,
            M3pStorageRegisteredDay<'a>,
        ),
        BranchError,
    > {
        let BranchRuntime { world, context, .. } = self;
        let HybridWorldState {
            world: logical_world,
            segmented_storage,
            authority_mode: _,
        } = world;
        let storage = segmented_storage.as_mut().ok_or_else(|| {
            BranchError::InvalidIntervention("branch requires segmented storage".into())
        })?;
        let state = match context {
            StorageContext::M3p(context) => M3pStorageRegisteredDay::new(
                logical_world,
                config,
                context,
                options,
                None,
                None,
                None,
                forced_action,
            ),
            StorageContext::M3pIncremental(context) => {
                let (storage_context, scratch) = context.phase_recorder_parts_mut();
                M3pStorageRegisteredDay::new(
                    logical_world,
                    config,
                    storage_context,
                    options,
                    Some(scratch),
                    None,
                    None,
                    forced_action,
                )
            }
            StorageContext::M3pPropagation(context) => {
                let base_config = crate::m3pprop::m3p_base_config(config);
                let (storage_context, propagation) = context.phase_recorder_parts_mut();
                M3pStorageRegisteredDay::new(
                    logical_world,
                    &base_config,
                    storage_context,
                    options,
                    None,
                    Some(propagation),
                    Some(config),
                    forced_action,
                )
            }
            StorageContext::M3pPropagationIncremental(context) => {
                let base_config = crate::m3pprop::m3p_base_config(config);
                let (storage_context, scratch, propagation) = context.phase_recorder_parts_mut();
                M3pStorageRegisteredDay::new(
                    logical_world,
                    &base_config,
                    storage_context,
                    options,
                    Some(scratch),
                    Some(propagation),
                    Some(config),
                    forced_action,
                )
            }
            StorageContext::M3(_) | StorageContext::M3Propagation(_) => {
                return Err(BranchError::UnsupportedProfile);
            }
        };
        Ok((storage, state))
    }

    fn day(
        &mut self,
        config: &SimConfig,
        run_context: &M0RunContext,
        snapshot: bool,
        forced_action: Option<&mut ForcedActionDirective>,
    ) -> Result<BranchDayRecord, BranchError> {
        let outcome = self.run_day_outcome(config, snapshot, forced_action)?;
        self.day_record(outcome, run_context, None)
    }

    fn day_with_analysis_costs(
        &mut self,
        config: &SimConfig,
        run_context: &M0RunContext,
        snapshot: bool,
        forced_action: Option<&mut ForcedActionDirective>,
        costs: &mut BranchAnalysisCosts,
    ) -> Result<BranchDayRecord, BranchError> {
        let outcome = self.run_day_outcome(config, snapshot, forced_action)?;
        self.day_record(outcome, run_context, Some(costs))
    }

    fn run_day_outcome(
        &mut self,
        config: &SimConfig,
        snapshot: bool,
        forced_action: Option<&mut ForcedActionDirective>,
    ) -> Result<BranchSimulationDay, BranchError> {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: snapshot,
        };
        let outcome = match &mut self.context {
            StorageContext::M3(context) => BranchSimulationDay::M3(run_m3_storage_day_for_branch(
                &mut self.world,
                config,
                context,
                &options,
                forced_action,
            )?),
            StorageContext::M3Propagation(context) => {
                BranchSimulationDay::M3Propagation(run_m3prop_storage_day_for_branch(
                    &mut self.world,
                    config,
                    context,
                    &options,
                    forced_action,
                )?)
            }
            StorageContext::M3p(context) => {
                BranchSimulationDay::M3p(run_m3p_storage_day_for_branch(
                    &mut self.world,
                    config,
                    context,
                    &options,
                    forced_action,
                )?)
            }
            StorageContext::M3pIncremental(context) => {
                BranchSimulationDay::M3p(run_m3p_storage_day_incremental_for_branch(
                    &mut self.world,
                    config,
                    context,
                    &options,
                    forced_action,
                )?)
            }
            StorageContext::M3pPropagation(context) => {
                BranchSimulationDay::M3pPropagation(run_m3pprop_storage_day_for_branch(
                    &mut self.world,
                    config,
                    context,
                    &options,
                    forced_action,
                )?)
            }
            StorageContext::M3pPropagationIncremental(context) => {
                BranchSimulationDay::M3pPropagation(run_m3pprop_storage_day_incremental_for_branch(
                    &mut self.world,
                    config,
                    context,
                    &options,
                    forced_action,
                )?)
            }
        };
        Ok(outcome)
    }

    fn day_record(
        &self,
        outcome: BranchSimulationDay,
        run_context: &M0RunContext,
        costs: Option<&mut BranchAnalysisCosts>,
    ) -> Result<BranchDayRecord, BranchError> {
        let digest_start = costs.as_ref().map(|_| Instant::now());
        let storage = self
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        let day = outcome.executed_day();
        let (state_digest, metrics, metrics_digest, events, events_digest, snapshot) = match outcome
        {
            BranchSimulationDay::M3(value) => {
                let context = match &self.context {
                    StorageContext::M3(c) => c,
                    _ => unreachable!(),
                };
                let state = canonical_m3_state_hash_from_storage(
                    storage,
                    self.world.world.current_day,
                    &self.world.world.settlements,
                    context.relationship_graph(),
                )?;
                let metrics = value.metrics.expect("branch metrics enabled");
                let metric_hash = canonical_m3_metrics_hash(std::slice::from_ref(&metrics))?;
                let event_hash = canonical_event_hash(&value.events)?;
                (
                    state,
                    BranchMetricsRecord::M3(metrics),
                    metric_hash,
                    BranchEventsRecord::Standard(value.events),
                    event_hash,
                    value.snapshot,
                )
            }
            BranchSimulationDay::M3Propagation(value) => {
                let context = match &self.context {
                    StorageContext::M3Propagation(c) => c,
                    _ => unreachable!(),
                };
                let state = canonical_m3prop_state_hash_from_storage(
                    storage,
                    self.world.world.current_day,
                    &self.world.world.settlements,
                    context.relationship_graph(),
                    context.propagation(),
                )?;
                let metrics = value.metrics.expect("branch metrics enabled");
                let metric_hash = canonical_m3prop_metrics_hash(std::slice::from_ref(&metrics))?;
                let event_hash = canonical_m3prop_event_hash(&value.events)?;
                (
                    state,
                    BranchMetricsRecord::M3Propagation(metrics),
                    metric_hash,
                    BranchEventsRecord::Standard(value.events),
                    event_hash,
                    value.snapshot,
                )
            }
            BranchSimulationDay::M3p(value) => {
                let (graph, cursor) = match &self.context {
                    StorageContext::M3p(c) => (c.relationship_graph(), c.next_agent_id()),
                    StorageContext::M3pIncremental(c) => {
                        (c.relationship_graph(), c.next_agent_id())
                    }
                    _ => unreachable!(),
                };
                let state = canonical_m3p_state_hash_from_storage(
                    storage,
                    self.world.world.current_day,
                    &self.world.world.settlements,
                    graph,
                    cursor,
                )?;
                let metrics = value.metrics.expect("branch metrics enabled");
                let metric_hash = canonical_m3p_metrics_hash(std::slice::from_ref(&metrics))?;
                let event_hash = canonical_m3p_event_hash(&value.events)?;
                (
                    state,
                    BranchMetricsRecord::M3p(metrics),
                    metric_hash,
                    BranchEventsRecord::Population(value.events),
                    event_hash,
                    value.snapshot,
                )
            }
            BranchSimulationDay::M3pPropagation(value) => {
                let (graph, cursor, propagation) = match &self.context {
                    StorageContext::M3pPropagation(c) => {
                        (c.relationship_graph(), c.next_agent_id(), c.propagation())
                    }
                    StorageContext::M3pPropagationIncremental(c) => {
                        (c.relationship_graph(), c.next_agent_id(), c.propagation())
                    }
                    _ => unreachable!(),
                };
                let state = canonical_m3pprop_state_hash_from_storage(
                    storage,
                    self.world.world.current_day,
                    &self.world.world.settlements,
                    graph,
                    cursor,
                    propagation,
                )?;
                let metrics = value.metrics.expect("branch metrics enabled");
                let metric_hash = canonical_m3pprop_metrics_hash(std::slice::from_ref(&metrics))?;
                let event_hash = canonical_m3pprop_event_hash(&value.events)?;
                (
                    state,
                    BranchMetricsRecord::M3pPropagation(metrics),
                    metric_hash,
                    BranchEventsRecord::Population(value.events),
                    event_hash,
                    value.snapshot,
                )
            }
        };
        if let (Some(costs), Some(start)) = (costs, digest_start) {
            costs.digest_ns += start.elapsed().as_nanos();
        }
        let _ = run_context;
        Ok(BranchDayRecord {
            day,
            state_digest,
            metrics,
            metrics_digest,
            events,
            events_digest,
            snapshot,
        })
    }

    fn graph(&self) -> &RelationshipGraph {
        match &self.context {
            StorageContext::M3(c) => c.relationship_graph(),
            StorageContext::M3Propagation(c) => c.relationship_graph(),
            StorageContext::M3p(c) => c.relationship_graph(),
            StorageContext::M3pIncremental(c) => c.relationship_graph(),
            StorageContext::M3pPropagation(c) => c.relationship_graph(),
            StorageContext::M3pPropagationIncremental(c) => c.relationship_graph(),
        }
    }

    fn start_state(&mut self) -> BranchStartState {
        self.world.sync_to_world_agents();
        BranchStartState {
            world: self.world.clone(),
            graph: self.graph().clone(),
            allocator: self.allocator_cursor(),
            propagation: self.propagation().cloned(),
            profile: self.profile,
        }
    }

    fn allocator_cursor(&self) -> Option<u64> {
        match &self.context {
            StorageContext::M3(_) | StorageContext::M3Propagation(_) => None,
            StorageContext::M3p(c) => Some(c.next_agent_id()),
            StorageContext::M3pIncremental(c) => Some(c.next_agent_id()),
            StorageContext::M3pPropagation(c) => Some(c.next_agent_id()),
            StorageContext::M3pPropagationIncremental(c) => Some(c.next_agent_id()),
        }
    }

    fn propagation(&self) -> Option<&BTreeMap<AgentId, PropagationAgentState>> {
        match &self.context {
            StorageContext::M3Propagation(c) => Some(c.propagation()),
            StorageContext::M3pPropagation(c) => Some(c.propagation()),
            StorageContext::M3pPropagationIncremental(c) => Some(c.propagation()),
            _ => None,
        }
    }

    fn apply_agent_field(
        &mut self,
        id: AgentId,
        field: AgentField,
        value: AgentFieldValue,
    ) -> Result<(), BranchError> {
        let storage = self.world.segmented_storage.as_mut().ok_or_else(|| {
            BranchError::InvalidIntervention("branch requires segmented storage".into())
        })?;
        let slot = storage
            .slot_of(id)
            .ok_or_else(|| BranchError::InvalidIntervention(format!("missing AgentId {id}")))?;
        if !storage.alive()[slot] {
            return Err(BranchError::InvalidIntervention(format!(
                "AgentId {id} is dead"
            )));
        }
        match (field, value) {
            (AgentField::Food, AgentFieldValue::Food(value))
                if value.is_finite() && value >= 0.0 =>
            {
                storage.food_mut()[slot] = value
            }
            (AgentField::Health, AgentFieldValue::Health(value)) if value.is_finite() => {
                storage.health_mut()[slot] = value
            }
            (AgentField::Wealth, AgentFieldValue::Wealth(value)) if value >= 0 => {
                storage.wealth_mut()[slot] = value
            }
            _ => {
                return Err(BranchError::InvalidIntervention(
                    "field value type or range is invalid".into(),
                ));
            }
        }
        self.world.sync_to_world_agents();
        Ok(())
    }

    fn validate_live_target(&self, id: AgentId) -> Result<(), BranchError> {
        let storage = self.world.segmented_storage.as_ref().ok_or_else(|| {
            BranchError::InvalidIntervention("branch requires segmented storage".into())
        })?;
        let slot = storage
            .slot_of(id)
            .ok_or_else(|| BranchError::InvalidIntervention(format!("missing AgentId {id}")))?;
        if !storage.alive()[slot] {
            return Err(BranchError::InvalidIntervention(format!(
                "AgentId {id} is dead"
            )));
        }
        Ok(())
    }

    fn initial_agent_ids(&self) -> BTreeSet<AgentId> {
        self.world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage")
            .agent_ids()
            .iter()
            .copied()
            .collect()
    }

    fn agent_record(&self, id: AgentId) -> Option<CanonicalAgentRecord> {
        let storage = self.world.segmented_storage.as_ref()?;
        let slot = storage.slot_of(id)?;
        Some(agent_record_at(storage, slot))
    }

    fn same_slot_mapping(&self, other: &Self) -> bool {
        let left = self
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        let right = other
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        left.agent_ids() == right.agent_ids() && left.group_ids() == right.group_ids()
    }
}

fn agent_record_at(
    storage: &crate::storage::SegmentedAgentStorage,
    slot: usize,
) -> CanonicalAgentRecord {
    CanonicalAgentRecord {
        alive: storage.alive()[slot],
        birth_day: storage.demography.birth_day[slot].0,
        health: storage.health()[slot].to_bits(),
        food: storage.food()[slot].to_bits(),
        wealth: storage.wealth()[slot],
        productivity: storage.personality.productivity[slot].to_bits(),
        cooperation: storage.personality.cooperation[slot].to_bits(),
        aggression: storage.personality.aggression[slot].to_bits(),
        risk_tolerance: storage.personality.risk_tolerance[slot].to_bits(),
        group_id: storage.group_ids()[slot].0,
    }
}

#[derive(Clone, Copy)]
struct StorageAgentSlotPair {
    id: AgentId,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Default)]
struct M3pStoragePhaseCompareScratch {
    same_ordered_mapping: bool,
    left_slots: Vec<(AgentId, usize)>,
    right_slots: Vec<(AgentId, usize)>,
    merged: Vec<StorageAgentSlotPair>,
}

impl M3pStoragePhaseCompareScratch {
    fn refresh(
        &mut self,
        left: &crate::storage::SegmentedAgentStorage,
        right: &crate::storage::SegmentedAgentStorage,
    ) {
        self.same_ordered_mapping = left.agent_ids().len() == right.agent_ids().len()
            && left
                .agent_ids()
                .iter()
                .zip(right.agent_ids())
                .all(|(left, right)| left == right)
            && left.agent_ids().windows(2).all(|pair| pair[0] < pair[1]);
        if self.same_ordered_mapping {
            self.left_slots.clear();
            self.right_slots.clear();
            self.merged.clear();
            return;
        }
        self.left_slots.clear();
        self.left_slots.extend(
            left.agent_ids()
                .iter()
                .copied()
                .enumerate()
                .map(|(slot, id)| (id, slot)),
        );
        let left_sorted = self.left_slots.windows(2).all(|pair| pair[0].0 < pair[1].0);
        if !left_sorted {
            self.left_slots.sort_unstable_by_key(|pair| pair.0);
        }
        self.right_slots.clear();
        self.right_slots.extend(
            right
                .agent_ids()
                .iter()
                .copied()
                .enumerate()
                .map(|(slot, id)| (id, slot)),
        );
        let right_sorted = self
            .right_slots
            .windows(2)
            .all(|pair| pair[0].0 < pair[1].0);
        if !right_sorted {
            self.right_slots.sort_unstable_by_key(|pair| pair.0);
        }
        self.merged.clear();
        let (mut left_index, mut right_index) = (0, 0);
        while left_index < self.left_slots.len() || right_index < self.right_slots.len() {
            match (
                self.left_slots.get(left_index),
                self.right_slots.get(right_index),
            ) {
                (Some(&(left_id, left_slot)), Some(&(right_id, right_slot))) => {
                    match left_id.cmp(&right_id) {
                        std::cmp::Ordering::Less => {
                            self.merged.push(StorageAgentSlotPair {
                                id: left_id,
                                left: Some(left_slot),
                                right: None,
                            });
                            left_index += 1;
                        }
                        std::cmp::Ordering::Greater => {
                            self.merged.push(StorageAgentSlotPair {
                                id: right_id,
                                left: None,
                                right: Some(right_slot),
                            });
                            right_index += 1;
                        }
                        std::cmp::Ordering::Equal => {
                            self.merged.push(StorageAgentSlotPair {
                                id: left_id,
                                left: Some(left_slot),
                                right: Some(right_slot),
                            });
                            left_index += 1;
                            right_index += 1;
                        }
                    }
                }
                (Some(&(left_id, left_slot)), None) => {
                    self.merged.push(StorageAgentSlotPair {
                        id: left_id,
                        left: Some(left_slot),
                        right: None,
                    });
                    left_index += 1;
                }
                (None, Some(&(right_id, right_slot))) => {
                    self.merged.push(StorageAgentSlotPair {
                        id: right_id,
                        left: None,
                        right: Some(right_slot),
                    });
                    right_index += 1;
                }
                (None, None) => break,
            }
        }
    }

    fn first_difference(
        &self,
        left: &crate::storage::SegmentedAgentStorage,
        right: &crate::storage::SegmentedAgentStorage,
    ) -> Option<AgentId> {
        if self.same_ordered_mapping {
            return left.agent_ids().iter().enumerate().find_map(|(slot, id)| {
                (agent_record_at(left, slot) != agent_record_at(right, slot)).then_some(*id)
            });
        }
        self.merged
            .iter()
            .find_map(|row| match (row.left, row.right) {
                (Some(left_slot), Some(right_slot))
                    if agent_record_at(left, left_slot) == agent_record_at(right, right_slot) =>
                {
                    None
                }
                _ => Some(row.id),
            })
    }
}

fn storage_recorder_metrics_hash(
    state: &M3pStorageRegisteredDay<'_>,
) -> Result<Option<CanonicalHash>, M0RunError> {
    let Some(metrics) = state.phase_recorder_metrics() else {
        return Ok(None);
    };
    if let Some(propagation) = state.phase_recorder_propagation_counts() {
        let metrics = crate::m3pprop::M3pPropagationDailyMetrics {
            m3p_metrics: metrics.clone(),
            propagation,
        };
        crate::hashing::canonical_m3pprop_metrics_hash(std::slice::from_ref(&metrics))
            .map(Some)
            .map_err(|error| {
                M0RunError::InvariantViolation(format!(
                    "phase recorder M3PPROP metrics hash failed: {error}"
                ))
            })
    } else {
        canonical_m3p_metrics_hash(std::slice::from_ref(metrics))
            .map(Some)
            .map_err(|error| {
                M0RunError::InvariantViolation(format!(
                    "phase recorder metrics hash failed: {error}"
                ))
            })
    }
}

#[derive(Clone, Copy)]
struct RecorderComponentMask {
    agent_rows: bool,
    graph: bool,
    allocator: bool,
    propagation: bool,
    metrics_so_far: bool,
}

impl RecorderComponentMask {
    const ALL: Self = Self {
        agent_rows: true,
        graph: true,
        allocator: true,
        propagation: true,
        metrics_so_far: true,
    };

    fn from_writes(writes: &[&str]) -> Self {
        Self {
            agent_rows: writes.iter().any(|write| write.starts_with("agent.")),
            graph: writes
                .iter()
                .any(|write| write.starts_with("relationship_graph.")),
            allocator: writes.contains(&"population.next_agent_id"),
            propagation: writes.iter().any(|write| write.starts_with("propagation.")),
            metrics_so_far: writes.contains(&"phase10.metrics"),
        }
    }
}

fn recorder_component_mask(
    boundary: crate::runner::RegisteredWorldBoundary,
) -> RecorderComponentMask {
    use crate::runner::RegisteredCorePhase as Core;

    match boundary {
        crate::runner::RegisteredWorldBoundary::Core(phase) => match phase {
            Core::Phase2 | Core::Phase6A | Core::Phase7 | Core::Phase8 | Core::Phase9 => {
                RecorderComponentMask {
                    agent_rows: true,
                    graph: false,
                    allocator: false,
                    propagation: false,
                    metrics_so_far: false,
                }
            }
            Core::Phase10 => RecorderComponentMask {
                agent_rows: false,
                graph: false,
                allocator: false,
                propagation: false,
                metrics_so_far: true,
            },
            Core::Phase1
            | Core::Phase3
            | Core::Phase4
            | Core::Phase5
            | Core::Phase6B
            | Core::Phase11 => RecorderComponentMask {
                agent_rows: false,
                graph: false,
                allocator: false,
                propagation: false,
                metrics_so_far: false,
            },
        },
        crate::runner::RegisteredWorldBoundary::Hook(hook) => SYSTEM_REGISTRY
            .iter()
            .find(|descriptor| descriptor.id == hook.system_id)
            .and_then(|descriptor| {
                descriptor
                    .access
                    .iter()
                    .find(|access| access.slot == hook.slot)
            })
            .map(|access| RecorderComponentMask::from_writes(access.writes))
            .unwrap_or(RecorderComponentMask::ALL),
    }
}

fn m3p_storage_phase_difference(
    day: u32,
    boundary: crate::runner::RegisteredWorldBoundary,
    scratch: &M3pStoragePhaseCompareScratch,
    left_storage: &crate::storage::SegmentedAgentStorage,
    left_state: &M3pStorageRegisteredDay<'_>,
    right_storage: &crate::storage::SegmentedAgentStorage,
    right_state: &M3pStorageRegisteredDay<'_>,
) -> Result<Option<FirstDivergence>, M0RunError> {
    let mask = recorder_component_mask(boundary);
    let difference = m3p_storage_component_difference(
        day,
        boundary,
        mask,
        scratch,
        left_storage,
        left_state,
        right_storage,
        right_state,
    )?;
    #[cfg(debug_assertions)]
    debug_assert_eq!(
        difference,
        m3p_storage_component_difference(
            day,
            boundary,
            RecorderComponentMask::ALL,
            scratch,
            left_storage,
            left_state,
            right_storage,
            right_state,
        )?,
        "write-set recorder comparison diverged from the full comparison at {boundary:?}"
    );
    Ok(difference)
}

#[allow(clippy::too_many_arguments)]
fn m3p_storage_component_difference(
    day: u32,
    boundary: crate::runner::RegisteredWorldBoundary,
    mask: RecorderComponentMask,
    scratch: &M3pStoragePhaseCompareScratch,
    left_storage: &crate::storage::SegmentedAgentStorage,
    left_state: &M3pStorageRegisteredDay<'_>,
    right_storage: &crate::storage::SegmentedAgentStorage,
    right_state: &M3pStorageRegisteredDay<'_>,
) -> Result<Option<FirstDivergence>, M0RunError> {
    let first_agent = if mask.agent_rows {
        scratch.first_difference(left_storage, right_storage)
    } else {
        None
    };
    let mut differing = Vec::new();
    if mask.agent_rows && first_agent.is_some() {
        differing.push("agent_rows");
    }
    if mask.graph
        && left_state.phase_recorder_graph().csr_hash()
            != right_state.phase_recorder_graph().csr_hash()
    {
        differing.push("graph");
    }
    if mask.allocator
        && left_state.phase_recorder_allocator() != right_state.phase_recorder_allocator()
    {
        differing.push("allocator");
    }
    let phase9b_population_hook = matches!(
        boundary,
        crate::runner::RegisteredWorldBoundary::Hook(hook)
            if hook.system_id == SystemId::PopulationDynamics
                && hook.slot == crate::system_registry::PhaseSlot::Phase9B
    );
    // The reference path materializes newborn propagation rows at Phase 9C.
    let propagation_differs = mask.propagation
        && match (
            left_state.phase_recorder_propagation(),
            right_state.phase_recorder_propagation(),
        ) {
            (Some(left), Some(right)) if phase9b_population_hook => left
                .iter()
                .filter(|(id, _)| {
                    left_state
                        .phase_recorder_newborn_ids()
                        .binary_search(id)
                        .is_err()
                })
                .ne(right.iter().filter(|(id, _)| {
                    right_state
                        .phase_recorder_newborn_ids()
                        .binary_search(id)
                        .is_err()
                })),
            (left, right) => left != right,
        };
    if propagation_differs {
        differing.push("propagation");
    }
    if mask.metrics_so_far
        && storage_recorder_metrics_hash(left_state)? != storage_recorder_metrics_hash(right_state)?
    {
        differing.push("metrics_so_far");
    }
    Ok((!differing.is_empty()).then(|| first_divergence_at(day, boundary, differing, first_agent)))
}

fn recorded_slot_for_boundary(boundary: crate::runner::RegisteredWorldBoundary) -> RecordedSlot {
    match boundary {
        crate::runner::RegisteredWorldBoundary::Core(phase) => RecordedSlot::Core(phase),
        crate::runner::RegisteredWorldBoundary::Hook(hook) => RecordedSlot::Hook(hook),
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn locate_m3p_storage_day_divergence(
    base_start: &BranchStartState,
    branch_start: &BranchStartState,
    config: &SimConfig,
    branch_config: &SimConfig,
    run_context: &M0RunContext,
    execution: BranchExecutionOption,
    day: u32,
    forced_action: Option<ForcedActionDirective>,
) -> Result<Option<FirstDivergence>, BranchError> {
    locate_m3p_storage_day_divergence_with_mode(
        base_start,
        branch_start,
        config,
        branch_config,
        run_context,
        execution,
        day,
        forced_action,
        RecorderComparisonMode::Enabled,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn locate_m3p_storage_day_divergence_with_mode(
    base_start: &BranchStartState,
    branch_start: &BranchStartState,
    config: &SimConfig,
    branch_config: &SimConfig,
    run_context: &M0RunContext,
    execution: BranchExecutionOption,
    day: u32,
    mut forced_action: Option<ForcedActionDirective>,
    recorder_mode: RecorderComparisonMode<'_>,
    analysis_costs: Option<&mut BranchAnalysisCosts>,
) -> Result<Option<FirstDivergence>, BranchError> {
    if let RecorderComparisonMode::DisabledUntil(expected) = &recorder_mode
        && expected.day != day
    {
        return Err(BranchError::InvalidIntervention(
            "measurement recorder boundary day does not match replay day".into(),
        ));
    }
    let mut base = base_start.restore_m3p_storage_runtime(config, run_context, execution)?;
    let mut branch =
        branch_start.restore_m3p_storage_runtime(branch_config, run_context, execution)?;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: false,
        snapshot_boundary: false,
    };
    let enabled_systems: &[SystemId] =
        if base_start.profile == SimulationRunnerSelection::M3pPropagation {
            &[
                SystemId::RelationshipGraph,
                SystemId::PopulationDynamics,
                SystemId::Propagation,
            ]
        } else {
            &[SystemId::RelationshipGraph, SystemId::PopulationDynamics]
        };
    let population_dynamics = if execution == BranchExecutionOption::IncrementalTopology {
        SystemExecutionPolicy::IncrementalTopology
    } else {
        SystemExecutionPolicy::Serial
    };
    let run_options = RegisteredSystemRunOptions {
        enabled_systems,
        execution: SystemExecutionOptions {
            population_dynamics,
            ..SystemExecutionOptions::default()
        },
    };
    let replay_started = analysis_costs.as_ref().map(|_| Instant::now());
    let (base_storage, mut base_day) = base.phase_storage_day(config, &options, None)?;
    let (branch_storage, mut branch_day) =
        branch.phase_storage_day(branch_config, &options, forced_action.as_mut())?;
    let mut compare = M3pStoragePhaseCompareScratch::default();
    if matches!(&recorder_mode, RecorderComparisonMode::Enabled) {
        compare.refresh(base_storage, branch_storage);
    }
    let mut comparison_ns = 0u128;
    let result = run_registered_storage_day_pair_with_recorder(
        base_storage,
        &mut base_day,
        branch_storage,
        &mut branch_day,
        &run_options,
        day,
        |day, phase, hooks, storage, state, _execution| {
            state.run_core_phase(day, phase, hooks, storage)
        },
        |day, hook, storage, state, execution| state.run_system_hook(day, hook, storage, execution),
        |boundary, left_storage, left_state, right_storage, right_state| {
            if let RecorderComparisonMode::DisabledUntil(expected) = &recorder_mode {
                return Ok((recorded_slot_for_boundary(boundary) == expected.slot)
                    .then(|| (*expected).clone()));
            }
            let compare_started = analysis_costs.as_ref().map(|_| Instant::now());
            if matches!(boundary, crate::runner::RegisteredWorldBoundary::Hook(hook)
                if hook.system_id == SystemId::PopulationDynamics
                    && hook.slot == crate::system_registry::PhaseSlot::Phase9B)
            {
                compare.refresh(left_storage, right_storage);
            }
            let difference = m3p_storage_phase_difference(
                day,
                boundary,
                &compare,
                left_storage,
                left_state,
                right_storage,
                right_state,
            );
            if let Some(started) = compare_started {
                comparison_ns += started.elapsed().as_nanos();
            }
            difference
        },
    )?;
    if let (Some(costs), Some(started)) = (analysis_costs, replay_started) {
        costs.recorder_replay_ns += started.elapsed().as_nanos();
        costs.recorder_compare_ns += comparison_ns;
    }
    if matches!(recorder_mode, RecorderComparisonMode::DisabledUntil(_)) && result.is_none() {
        return Err(BranchError::InvalidIntervention(
            "measurement recorder boundary was not reached".into(),
        ));
    }
    Ok(result)
}

impl BranchStartState {
    fn restore_m3p_storage_runtime(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
        execution: BranchExecutionOption,
    ) -> Result<BranchRuntime, BranchError> {
        let world = self.world.clone();
        match self.profile {
            SimulationRunnerSelection::M3p => {
                let reference = M3pReferenceState::from_parts(
                    self.world.world.clone(),
                    self.graph.clone(),
                    self.allocator.expect("M3P allocator"),
                    config,
                    run_context,
                )?;
                let context = if execution == BranchExecutionOption::IncrementalTopology {
                    StorageContext::M3pIncremental(
                        M3pIncrementalTopologyStorageRunContext::from_reference_state(
                            &world,
                            config,
                            run_context,
                            &reference,
                        )?,
                    )
                } else {
                    StorageContext::M3p(M3pStorageRunContext::from_reference_state(
                        &world,
                        config,
                        run_context,
                        &reference,
                    )?)
                };
                Ok(BranchRuntime {
                    world,
                    context,
                    profile: self.profile,
                })
            }
            SimulationRunnerSelection::M3pPropagation => {
                let base_config = crate::m3pprop::m3p_base_config(config);
                let base = M3pReferenceState::from_parts(
                    self.world.world.clone(),
                    self.graph.clone(),
                    self.allocator.expect("M3P allocator"),
                    &base_config,
                    run_context,
                )?;
                let reference = crate::m3pprop::M3pPropagationReferenceState::from_parts(
                    base,
                    self.propagation.clone().expect("M3P propagation rows"),
                    config,
                )?;
                let context = if execution == BranchExecutionOption::IncrementalTopology {
                    StorageContext::M3pPropagationIncremental(
                        M3pPropagationIncrementalTopologyStorageRunContext::from_reference_state(
                            &world,
                            config,
                            run_context,
                            &reference,
                        )?,
                    )
                } else {
                    StorageContext::M3pPropagation(
                        M3pPropagationStorageRunContext::from_reference_state(
                            &world,
                            config,
                            run_context,
                            &reference,
                        )?,
                    )
                };
                Ok(BranchRuntime {
                    world,
                    context,
                    profile: self.profile,
                })
            }
            SimulationRunnerSelection::M3 | SimulationRunnerSelection::M3Propagation => {
                Err(BranchError::UnsupportedProfile)
            }
            SimulationRunnerSelection::M2 => Err(BranchError::UnsupportedProfile),
        }
    }

    fn state_digest(&self) -> Result<CanonicalHash, BranchError> {
        let storage = self
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        let current_day = self.world.world.current_day;
        let settlements = &self.world.world.settlements;
        Ok(match self.profile {
            SimulationRunnerSelection::M3 => canonical_m3_state_hash_from_storage(
                storage,
                current_day,
                settlements,
                &self.graph,
            )?,
            SimulationRunnerSelection::M3Propagation => canonical_m3prop_state_hash_from_storage(
                storage,
                current_day,
                settlements,
                &self.graph,
                self.propagation.as_ref().expect("M3 propagation rows"),
            )?,
            SimulationRunnerSelection::M3p => canonical_m3p_state_hash_from_storage(
                storage,
                current_day,
                settlements,
                &self.graph,
                self.allocator.expect("M3P allocator"),
            )?,
            SimulationRunnerSelection::M3pPropagation => canonical_m3pprop_state_hash_from_storage(
                storage,
                current_day,
                settlements,
                &self.graph,
                self.allocator.expect("M3P allocator"),
                self.propagation.as_ref().expect("M3P propagation rows"),
            )?,
            SimulationRunnerSelection::M2 => return Err(BranchError::UnsupportedProfile),
        })
    }

    fn record_reference_day(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
        forced_action: Option<&mut ForcedActionDirective>,
        records: &mut Vec<PhaseComponentHashes>,
    ) -> Result<(), BranchError> {
        let world = self.world.world.clone();
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: false,
            snapshot_boundary: false,
        };
        match self.profile {
            SimulationRunnerSelection::M3 => {
                let context = M3ReferenceContext::new(&world, config, run_context)?;
                let mut world = world;
                run_m3_reference_day_with_phase_recorder(
                    &mut world,
                    config,
                    &context,
                    &options,
                    forced_action,
                    records,
                )?;
            }
            SimulationRunnerSelection::M3p => {
                let state = M3pReferenceState::from_parts(
                    world,
                    self.graph.clone(),
                    self.allocator.expect("M3P allocator"),
                    config,
                    run_context,
                )?;
                let mut state = state;
                run_m3p_reference_day_with_phase_recorder(
                    &mut state,
                    config,
                    run_context,
                    &options,
                    forced_action,
                    records,
                )?;
            }
            SimulationRunnerSelection::M3Propagation
            | SimulationRunnerSelection::M3pPropagation => {
                if self.profile == SimulationRunnerSelection::M3Propagation {
                    let state = M3PropagationReferenceState::from_parts(
                        world,
                        self.graph.clone(),
                        self.propagation.clone().expect("M3 propagation rows"),
                        config,
                        run_context,
                    )?;
                    let mut state = state;
                    run_m3prop_reference_day_with_phase_recorder(
                        &mut state,
                        config,
                        run_context,
                        &options,
                        forced_action,
                        records,
                    )?;
                } else {
                    let base = M3pReferenceState::from_parts(
                        world,
                        self.graph.clone(),
                        self.allocator.expect("M3P allocator"),
                        &crate::m3pprop::m3p_base_config(config),
                        run_context,
                    )?;
                    let mut state = crate::m3pprop::M3pPropagationReferenceState::from_parts(
                        base,
                        self.propagation.clone().expect("M3P propagation rows"),
                        config,
                    )?;
                    run_m3pprop_reference_day_with_phase_recorder(
                        &mut state,
                        config,
                        run_context,
                        &options,
                        forced_action,
                        records,
                    )?;
                }
            }
            SimulationRunnerSelection::M2 => return Err(BranchError::UnsupportedProfile),
        }
        Ok(())
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, PartialEq)]
pub enum BranchSimulationDay {
    M3(M3DayOutcome),
    M3Propagation(crate::m3prop::M3PropDayOutcome),
    M3p(crate::m3p::M3pDayOutcome),
    M3pPropagation(crate::m3pprop::M3pPropagationDayOutcome),
}

impl BranchSimulationDay {
    pub fn executed_day(&self) -> u32 {
        match self {
            Self::M3(value) => value.executed_day,
            Self::M3Propagation(value) => value.executed_day,
            Self::M3p(value) => value.executed_day,
            Self::M3pPropagation(value) => value.executed_day,
        }
    }
}

fn agent_record_bytes(agent: &AgentState) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(44);
    bytes.extend_from_slice(&agent.agent_id.0.to_le_bytes());
    bytes.push(u8::from(agent.alive));
    bytes.extend_from_slice(&agent.birth_day.0.to_le_bytes());
    bytes.extend_from_slice(&agent.health.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.food.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.wealth.to_le_bytes());
    bytes.extend_from_slice(&agent.productivity.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.cooperation.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.aggression.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.risk_tolerance.to_bits().to_le_bytes());
    bytes.extend_from_slice(&agent.group_id.0.to_le_bytes());
    bytes
}

fn apply_config_override(
    config: &mut SimConfig,
    parameter: ConfigParameter,
    value: ConfigValue,
) -> Result<(), BranchError> {
    let f32_value = match value {
        ConfigValue::F32(value) if value.is_finite() => Some(value),
        _ => None,
    };
    let money_value = match value {
        ConfigValue::Money(value) if value >= 0 => Some(value),
        _ => None,
    };
    let u32_value = match value {
        ConfigValue::U32(value) => Some(value),
        _ => None,
    };
    let invalid = || {
        BranchError::InvalidIntervention("config override value type or range is invalid".into())
    };
    match parameter {
        ConfigParameter::CarryingCapacity => {
            config.environment.carrying_capacity = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::RegrowthRate => {
            config.environment.regrowth_rate = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BaseMetabolicCost => {
            config.environment.base_metabolic_cost = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::HealthDecayRate => {
            config.environment.health_decay_rate = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BaseWorkYield => {
            config.economy.base_work_yield = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::FoodPrice => {
            config.economy.food_price = money_value.ok_or_else(invalid)?
        }
        ConfigParameter::TargetFood => {
            config.economy.target_food = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::TargetReserve => {
            config.economy.target_reserve = money_value.ok_or_else(invalid)?
        }
        ConfigParameter::TaxRate => config.economy.tax_rate = f32_value.ok_or_else(invalid)?,
        ConfigParameter::WelfarePayment => {
            config.economy.welfare_payment = money_value.ok_or_else(invalid)?
        }
        ConfigParameter::GiftAmount => {
            config.interaction.gift_amount = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::TheftAmount => {
            config.interaction.theft_amount = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::TheftSuccessProbability => {
            config.interaction.theft_success_probability = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::StarvationThreshold => {
            config.interaction.starvation_threshold = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::DecisionTemperature => {
            config.decision.decision_temperature = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::ActionBias { action_index } => {
            let slot = config
                .decision
                .action_biases
                .get_mut(action_index as usize)
                .ok_or_else(invalid)?;
            *slot = f32_value.ok_or_else(invalid)?;
        }
        ConfigParameter::BaseWeight {
            action_index,
            feature_index,
        } => {
            let slot = config
                .decision
                .base_weight_matrix
                .get_mut(action_index as usize)
                .and_then(|row| row.get_mut(feature_index as usize))
                .ok_or_else(invalid)?;
            *slot = f32_value.ok_or_else(invalid)?;
        }
        ConfigParameter::TraitWeightCooperation => {
            config.decision.trait_weight_cooperation = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::TraitWeightAggression => {
            config.decision.trait_weight_aggression = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::TraitWeightRiskTolerance => {
            config.decision.trait_weight_risk_tolerance = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BirthMinHealth => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .birth_min_health = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BirthFoodThreshold => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .birth_food_threshold = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BirthProbability => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .birth_probability = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BirthFoodCost => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .birth_food_cost = f32_value.ok_or_else(invalid)?
        }
        ConfigParameter::BirthsMaxPerDay => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .births_max_per_day = u32_value.ok_or_else(invalid)?
        }
        ConfigParameter::CompactionIntervalDays => {
            config
                .population
                .as_mut()
                .ok_or_else(invalid)?
                .compaction_interval_days = u32_value.ok_or_else(invalid)?
        }
    }
    config.validate().map_err(BranchError::from)
}

fn set_intervention_field(
    runtime: &mut BranchRuntime,
    intervention: &BranchIntervention,
) -> Result<(), BranchError> {
    match intervention {
        BranchIntervention::AgentFieldOverride {
            agent_id,
            field,
            value,
        } => runtime.apply_agent_field(*agent_id, *field, *value),
        _ => Ok(()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LineageState {
    root: AgentId,
    path: Vec<u32>,
}

#[derive(Default)]
struct LineageTracker {
    lineages: HashMap<AgentId, LineageState>,
    agents: HashMap<AgentMatchKey, AgentId>,
    ordinals: HashMap<AgentId, u32>,
}

fn record_births(
    events: &BranchEventsRecord,
    initial_ids: &BTreeSet<AgentId>,
    lineage: &mut LineageTracker,
) -> Vec<(AgentId, AgentId, AgentMatchKey)> {
    let BranchEventsRecord::Population(records) = events else {
        return Vec::new();
    };
    let mut births = Vec::new();
    for record in records {
        let crate::m3p::M3pEvent::Birth { parent, child, .. } = record.event else {
            continue;
        };
        let ordinal = lineage.ordinals.entry(parent).or_default();
        let current_ordinal = *ordinal;
        *ordinal = ordinal.saturating_add(1);
        let lineage_state = if initial_ids.contains(&parent) {
            LineageState {
                root: parent,
                path: vec![current_ordinal],
            }
        } else {
            let Some(mut parent_lineage) = lineage.lineages.get(&parent).cloned() else {
                continue;
            };
            parent_lineage.path.push(current_ordinal);
            parent_lineage
        };
        let key = AgentMatchKey::Birth {
            ancestor: lineage_state.root,
            ordinal_path: lineage_state.path.clone(),
        };
        lineage.lineages.insert(child, lineage_state);
        lineage.agents.insert(key.clone(), child);
        births.push((parent, child, key));
    }
    births
}

fn record_shared_births(
    events: &BranchEventsRecord,
    initial_ids: &BTreeSet<AgentId>,
    base: &mut LineageTracker,
    branch: &mut LineageTracker,
) -> Vec<(AgentId, AgentId, AgentMatchKey)> {
    let BranchEventsRecord::Population(records) = events else {
        return Vec::new();
    };
    let mut births = Vec::new();
    for record in records {
        let crate::m3p::M3pEvent::Birth { parent, child, .. } = record.event else {
            continue;
        };
        let ordinal = base.ordinals.entry(parent).or_default();
        let current_ordinal = *ordinal;
        *ordinal = ordinal.saturating_add(1);
        let branch_ordinal = branch.ordinals.entry(parent).or_default();
        debug_assert_eq!(*branch_ordinal, current_ordinal);
        *branch_ordinal = branch_ordinal.saturating_add(1);
        let lineage = if initial_ids.contains(&parent) {
            LineageState {
                root: parent,
                path: vec![current_ordinal],
            }
        } else {
            let Some(mut parent_lineage) = base.lineages.get(&parent).cloned() else {
                continue;
            };
            debug_assert_eq!(
                base.lineages
                    .get(&parent)
                    .map(|lineage| (&lineage.root, &lineage.path)),
                branch
                    .lineages
                    .get(&parent)
                    .map(|lineage| (&lineage.root, &lineage.path))
            );
            parent_lineage.path.push(current_ordinal);
            parent_lineage
        };
        let key = AgentMatchKey::Birth {
            ancestor: lineage.root,
            ordinal_path: lineage.path.clone(),
        };
        base.lineages.insert(child, lineage.clone());
        branch.lineages.insert(child, lineage);
        base.agents.insert(key.clone(), child);
        branch.agents.insert(key.clone(), child);
        births.push((parent, child, key));
    }
    births
}

#[allow(clippy::too_many_arguments)]
fn difference_for_day_canonical(
    day: u32,
    base: &BranchRuntime,
    branch: &BranchRuntime,
    base_initial: &BTreeSet<AgentId>,
    branch_initial: &BTreeSet<AgentId>,
    base_lineage_agents: &HashMap<AgentMatchKey, AgentId>,
    branch_lineage_agents: &HashMap<AgentMatchKey, AgentId>,
    base_births: &[(AgentId, AgentId, AgentMatchKey)],
    branch_births: &[(AgentId, AgentId, AgentMatchKey)],
) -> DailyDifference {
    let mut changed_agent_records = 0u64;
    for id in base_initial.union(branch_initial) {
        match (base.agent_record(*id), branch.agent_record(*id)) {
            (Some(left), Some(right)) if left != right => changed_agent_records += 1,
            (Some(_), None) | (None, Some(_)) => changed_agent_records += 1,
            _ => {}
        }
    }
    for (key, base_id) in base_lineage_agents {
        if let Some(branch_id) = branch_lineage_agents.get(key) {
            match (base.agent_record(*base_id), branch.agent_record(*branch_id)) {
                (Some(left), Some(right)) if left != right => changed_agent_records += 1,
                (Some(_), None) | (None, Some(_)) => changed_agent_records += 1,
                _ => {}
            }
        }
    }
    let base_birth_keys: BTreeSet<_> = base_births.iter().map(|(_, _, key)| key.clone()).collect();
    let branch_birth_keys: BTreeSet<_> = branch_births
        .iter()
        .map(|(_, _, key)| key.clone())
        .collect();
    let branch_ids_by_key: BTreeMap<_, _> = branch_births
        .iter()
        .map(|(_, child, key)| (key.clone(), *child))
        .collect();
    let base_ids_by_key: BTreeMap<_, _> = base_births
        .iter()
        .map(|(_, child, key)| (key.clone(), *child))
        .collect();
    let matched_births: Vec<_> = base_birth_keys
        .intersection(&branch_birth_keys)
        .map(|key| (base_ids_by_key[key], branch_ids_by_key[key]))
        .collect();
    let unmatched_base_births = base_births
        .iter()
        .filter(|(_, _, key)| !branch_birth_keys.contains(key))
        .map(|(_, child, _)| *child)
        .collect();
    let unmatched_branch_births = branch_births
        .iter()
        .filter(|(_, _, key)| !base_birth_keys.contains(key))
        .map(|(_, child, _)| *child)
        .collect();
    let newborn_crn_loss = matched_births
        .iter()
        .filter(|(left, right)| left != right)
        .count() as u64;
    let graph_changed = base.graph().csr_hash() != branch.graph().csr_hash();
    let allocator_changed = base.allocator_cursor() != branch.allocator_cursor();
    let propagation_changed = base.propagation() != branch.propagation();
    DailyDifference {
        day,
        changed_agent_records,
        graph_changed,
        allocator_changed,
        propagation_changed,
        matched_births,
        unmatched_base_births,
        unmatched_branch_births,
        newborn_crn_loss,
    }
}

struct BirthDifference {
    matched_births: Vec<(AgentId, AgentId)>,
    unmatched_base_births: Vec<AgentId>,
    unmatched_branch_births: Vec<AgentId>,
    newborn_crn_loss: u64,
}

fn canonical_birth_difference(
    base_births: &[(AgentId, AgentId, AgentMatchKey)],
    branch_births: &[(AgentId, AgentId, AgentMatchKey)],
) -> BirthDifference {
    let base_birth_keys: BTreeSet<_> = base_births.iter().map(|(_, _, key)| key.clone()).collect();
    let branch_birth_keys: BTreeSet<_> = branch_births
        .iter()
        .map(|(_, _, key)| key.clone())
        .collect();
    let branch_ids_by_key: BTreeMap<_, _> = branch_births
        .iter()
        .map(|(_, child, key)| (key.clone(), *child))
        .collect();
    let base_ids_by_key: BTreeMap<_, _> = base_births
        .iter()
        .map(|(_, child, key)| (key.clone(), *child))
        .collect();
    let matched_births: Vec<_> = base_birth_keys
        .intersection(&branch_birth_keys)
        .map(|key| (base_ids_by_key[key], branch_ids_by_key[key]))
        .collect();
    let unmatched_base_births = base_births
        .iter()
        .filter(|(_, _, key)| !branch_birth_keys.contains(key))
        .map(|(_, child, _)| *child)
        .collect();
    let unmatched_branch_births = branch_births
        .iter()
        .filter(|(_, _, key)| !base_birth_keys.contains(key))
        .map(|(_, child, _)| *child)
        .collect();
    let newborn_crn_loss = matched_births
        .iter()
        .filter(|(left, right)| left != right)
        .count() as u64;
    BirthDifference {
        matched_births,
        unmatched_base_births,
        unmatched_branch_births,
        newborn_crn_loss,
    }
}

fn equal_birth_difference(births: &[(AgentId, AgentId, AgentMatchKey)]) -> BirthDifference {
    let mut sorted: Vec<_> = births.iter().map(|(_, child, key)| (key, *child)).collect();
    sorted.sort_by(|left, right| left.0.cmp(right.0));
    let mut matched_births = Vec::with_capacity(sorted.len());
    let mut index = 0;
    while index < sorted.len() {
        let key = sorted[index].0;
        let mut child = sorted[index].1;
        index += 1;
        while index < sorted.len() && sorted[index].0 == key {
            child = sorted[index].1;
            index += 1;
        }
        matched_births.push((child, child));
    }
    BirthDifference {
        matched_births,
        unmatched_base_births: Vec::new(),
        unmatched_branch_births: Vec::new(),
        newborn_crn_loss: 0,
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn difference_for_day_fast(
    day: u32,
    base: &BranchRuntime,
    branch: &BranchRuntime,
    base_initial: &BTreeSet<AgentId>,
    branch_initial: &BTreeSet<AgentId>,
    base_lineage_agents: &HashMap<AgentMatchKey, AgentId>,
    branch_lineage_agents: &HashMap<AgentMatchKey, AgentId>,
    base_births: &[(AgentId, AgentId, AgentMatchKey)],
    branch_births: &[(AgentId, AgentId, AgentMatchKey)],
    state_digests_equal: bool,
    all_day_digests_equal: bool,
) -> DailyDifference {
    difference_for_day_fast_impl(
        DailyDifferenceInput {
            day,
            base,
            branch,
            base_initial,
            branch_initial,
            base_lineage_agents,
            branch_lineage_agents,
            base_births,
            branch_births: Some(branch_births),
            state_digests_equal,
            all_day_digests_equal,
        },
        None,
    )
}

struct DailyDifferenceInput<'a> {
    day: u32,
    base: &'a BranchRuntime,
    branch: &'a BranchRuntime,
    base_initial: &'a BTreeSet<AgentId>,
    branch_initial: &'a BTreeSet<AgentId>,
    base_lineage_agents: &'a HashMap<AgentMatchKey, AgentId>,
    branch_lineage_agents: &'a HashMap<AgentMatchKey, AgentId>,
    base_births: &'a [(AgentId, AgentId, AgentMatchKey)],
    branch_births: Option<&'a [(AgentId, AgentId, AgentMatchKey)]>,
    state_digests_equal: bool,
    all_day_digests_equal: bool,
}

fn difference_for_day_fast_measured(
    input: DailyDifferenceInput<'_>,
    costs: &mut BranchAnalysisCosts,
) -> DailyDifference {
    difference_for_day_fast_impl(input, Some(costs))
}

fn difference_for_day_fast_impl(
    input: DailyDifferenceInput<'_>,
    costs: Option<&mut BranchAnalysisCosts>,
) -> DailyDifference {
    let DailyDifferenceInput {
        day,
        base,
        branch,
        base_initial,
        branch_initial,
        base_lineage_agents,
        branch_lineage_agents,
        base_births,
        branch_births,
        state_digests_equal,
        all_day_digests_equal,
    } = input;
    let branch_births = branch_births.unwrap_or(base_births);
    let lineage_started = costs.as_ref().map(|_| Instant::now());
    let births = if all_day_digests_equal && base_births == branch_births {
        equal_birth_difference(base_births)
    } else {
        canonical_birth_difference(base_births, branch_births)
    };
    if let (Some(costs), Some(started)) = (costs, lineage_started) {
        costs.lineage_ns += started.elapsed().as_nanos();
    }
    if state_digests_equal {
        return DailyDifference {
            day,
            changed_agent_records: 0,
            graph_changed: false,
            allocator_changed: false,
            propagation_changed: false,
            matched_births: births.matched_births,
            unmatched_base_births: births.unmatched_base_births,
            unmatched_branch_births: births.unmatched_branch_births,
            newborn_crn_loss: births.newborn_crn_loss,
        };
    }

    let changed_agent_records = if base_initial == branch_initial
        && base_lineage_agents == branch_lineage_agents
        && base.same_slot_mapping(branch)
    {
        let left = base
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        let right = branch
            .world
            .segmented_storage
            .as_ref()
            .expect("hybrid authority has storage");
        left.agent_ids()
            .iter()
            .enumerate()
            .filter(|(slot, _)| agent_record_at(left, *slot) != agent_record_at(right, *slot))
            .count() as u64
    } else {
        return difference_for_day_canonical(
            day,
            base,
            branch,
            base_initial,
            branch_initial,
            base_lineage_agents,
            branch_lineage_agents,
            base_births,
            branch_births,
        );
    };

    DailyDifference {
        day,
        changed_agent_records,
        graph_changed: base.graph().csr_hash() != branch.graph().csr_hash(),
        allocator_changed: base.allocator_cursor() != branch.allocator_cursor(),
        propagation_changed: base.propagation() != branch.propagation(),
        matched_births: births.matched_births,
        unmatched_base_births: births.unmatched_base_births,
        unmatched_branch_births: births.unmatched_branch_births,
        newborn_crn_loss: births.newborn_crn_loss,
    }
}

fn intervention_boundary_divergence(
    day: u32,
    base: &BranchStartState,
    branch: &BranchStartState,
) -> Result<FirstDivergence, BranchError> {
    let boundary =
        crate::runner::RegisteredWorldBoundary::Core(crate::runner::RegisteredCorePhase::Phase1);
    let base_components = record_phase_components(
        boundary,
        &base.world.world,
        &base.graph,
        base.allocator,
        base.propagation.as_ref(),
        None,
    )?;
    let branch_components = record_phase_components(
        boundary,
        &branch.world.world,
        &branch.graph,
        branch.allocator,
        branch.propagation.as_ref(),
        None,
    )?;
    let differing = first_phase_difference(&[base_components], &[branch_components], day)
        .expect("different intervention-boundary digests have a differing component");
    Ok(FirstDivergence {
        slot: RecordedSlot::InterventionBoundary,
        ..differing
    })
}

/// Runs one snapshot-backed base/branch pair for the requested horizon.
pub fn run_counterfactual_branch(request: &BranchRequest) -> Result<BranchResult, BranchError> {
    run_counterfactual_branch_inner(request, RecorderComparisonMode::Enabled, None)
}

#[doc(hidden)]
pub fn run_counterfactual_branch_with_analysis_mode(
    request: &BranchRequest,
    mode: BranchAnalysisMode,
) -> Result<BranchAnalysisOutput, BranchError> {
    match mode {
        BranchAnalysisMode::Disabled => Ok(BranchAnalysisOutput::AnalysisDisabled(
            run_counterfactual_branch_simulation_only(request)?,
        )),
        BranchAnalysisMode::Full => {
            let mut costs = BranchAnalysisCosts::default();
            let result = run_counterfactual_branch_inner(
                request,
                RecorderComparisonMode::Enabled,
                Some(&mut costs),
            )?;
            Ok(BranchAnalysisOutput::Full { result, costs })
        }
        BranchAnalysisMode::RecorderComparisonsDisabled { expected } => {
            if !matches!(
                request.config.runner_selection(),
                SimulationRunnerSelection::M3p | SimulationRunnerSelection::M3pPropagation
            ) {
                return Err(BranchError::UnsupportedProfile);
            }
            let mut costs = BranchAnalysisCosts::default();
            let result = run_counterfactual_branch_inner(
                request,
                RecorderComparisonMode::DisabledUntil(&expected),
                Some(&mut costs),
            )?;
            Ok(BranchAnalysisOutput::Full { result, costs })
        }
    }
}

#[derive(Clone, Copy)]
enum RecorderComparisonMode<'a> {
    Enabled,
    DisabledUntil(&'a FirstDivergence),
}

struct PreparedBranch {
    branch_config: SimConfig,
    base: BranchRuntime,
    branch: BranchRuntime,
    start_day: u32,
    forced_directive: Option<ForcedActionDirective>,
}

fn prepare_branch(request: &BranchRequest) -> Result<PreparedBranch, BranchError> {
    if request.execution == BranchExecutionOption::ExplicitParallel {
        return Err(BranchError::UnsupportedExecutionOption);
    }
    if request.config.runner_selection() == SimulationRunnerSelection::M2 {
        return Err(BranchError::UnsupportedProfile);
    }
    request.config.validate().map_err(BranchError::from)?;
    let mut branch_config = request.config.clone();
    if let BranchIntervention::ConfigOverride { parameter, value } = request.intervention {
        apply_config_override(&mut branch_config, parameter, value)?;
    }
    let base = BranchRuntime::restore(
        &request.snapshot,
        &request.config,
        &request.run_context,
        request.execution,
    )?;
    let mut branch = BranchRuntime::restore(
        &request.snapshot,
        &branch_config,
        &request.run_context,
        request.execution,
    )?;
    let start_day = base.world.world.current_day.as_u32();
    let end_day = start_day
        .checked_add(request.horizon_days)
        .ok_or(BranchError::DayOverflow)?;
    if branch.world.world.current_day.as_u32() != start_day {
        return Err(BranchError::SnapshotCursorMismatch);
    }
    if request
        .snapshot_days
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || request
            .snapshot_days
            .iter()
            .any(|day| *day < start_day || *day >= end_day)
    {
        return Err(BranchError::InvalidIntervention(
            "snapshot_days must be unique, ascending, and inside the requested horizon".into(),
        ));
    }
    match &request.intervention {
        BranchIntervention::AgentFieldOverride { agent_id, .. }
        | BranchIntervention::ForcedAction { agent_id, .. } => {
            branch.validate_live_target(*agent_id)?
        }
        BranchIntervention::ConfigOverride { .. } => {}
    }
    set_intervention_field(&mut branch, &request.intervention)?;
    let forced_directive = match request.intervention {
        BranchIntervention::ForcedAction { agent_id, action } => {
            Some(ForcedActionDirective::new(agent_id, action, start_day))
        }
        _ => None,
    };
    Ok(PreparedBranch {
        branch_config,
        base,
        branch,
        start_day,
        forced_directive,
    })
}

fn run_counterfactual_branch_simulation_only(
    request: &BranchRequest,
) -> Result<BranchSimulationResult, BranchError> {
    let PreparedBranch {
        branch_config,
        mut base,
        mut branch,
        mut forced_directive,
        ..
    } = prepare_branch(request)?;
    let mut base_days = Vec::with_capacity(request.horizon_days as usize);
    let mut branch_days = Vec::with_capacity(request.horizon_days as usize);
    for _ in 0..request.horizon_days {
        let day = base.world.world.current_day.as_u32();
        if day != branch.world.world.current_day.as_u32() {
            return Err(BranchError::InvalidIntervention(
                "base and branch resume cursors diverged".into(),
            ));
        }
        let include_snapshot = request.snapshot_days.binary_search(&day).is_ok();
        base_days.push(base.run_day_outcome(&request.config, include_snapshot, None)?);
        branch_days.push(branch.run_day_outcome(
            &branch_config,
            include_snapshot,
            forced_directive.as_mut(),
        )?);
    }
    let forced_action = forced_directive.and_then(|directive| directive.outcome);
    Ok(BranchSimulationResult {
        base_days,
        branch_days,
        forced_action,
    })
}

fn run_counterfactual_branch_inner(
    request: &BranchRequest,
    recorder_mode: RecorderComparisonMode<'_>,
    mut analysis_costs: Option<&mut BranchAnalysisCosts>,
) -> Result<BranchResult, BranchError> {
    let PreparedBranch {
        branch_config,
        mut base,
        mut branch,
        start_day,
        mut forced_directive,
    } = prepare_branch(request)?;
    let base_initial = base.initial_agent_ids();
    let branch_initial = branch.initial_agent_ids();
    let mut base_lineage = LineageTracker::default();
    let mut branch_lineage = LineageTracker::default();
    let mut base_days = Vec::with_capacity(request.horizon_days as usize);
    let mut branch_days = Vec::with_capacity(request.horizon_days as usize);
    let mut daily_differences = Vec::with_capacity(request.horizon_days as usize);
    let mut first_divergence = None;
    let mut lineage_contexts_equal = true;
    for _ in 0..request.horizon_days {
        let day = base.world.world.current_day.as_u32();
        if day != branch.world.world.current_day.as_u32() {
            return Err(BranchError::InvalidIntervention(
                "base and branch resume cursors diverged".into(),
            ));
        }
        let include_snapshot = request.snapshot_days.binary_search(&day).is_ok();
        let clone_started = analysis_costs.as_ref().map(|_| Instant::now());
        let start_states = first_divergence
            .is_none()
            .then(|| (base.start_state(), branch.start_state()));
        if let (Some(costs), Some(started)) = (analysis_costs.as_deref_mut(), clone_started) {
            costs.day_start_clone_ns += started.elapsed().as_nanos();
        }
        let start_digest_started = analysis_costs.as_ref().map(|_| Instant::now());
        let start_states_differ = if day == start_day {
            match &start_states {
                Some((base_start, branch_start)) => {
                    base_start.state_digest()? != branch_start.state_digest()?
                }
                None => false,
            }
        } else {
            false
        };
        if let (Some(costs), Some(started)) = (analysis_costs.as_deref_mut(), start_digest_started)
        {
            costs.digest_ns += started.elapsed().as_nanos();
        }
        let base_record = match analysis_costs.as_deref_mut() {
            Some(costs) => base.day_with_analysis_costs(
                &request.config,
                &request.run_context,
                include_snapshot,
                None,
                costs,
            )?,
            None => base.day(
                &request.config,
                &request.run_context,
                include_snapshot,
                None,
            )?,
        };
        let branch_record = match analysis_costs.as_deref_mut() {
            Some(costs) => branch.day_with_analysis_costs(
                &branch_config,
                &request.run_context,
                include_snapshot,
                forced_directive.as_mut(),
                costs,
            )?,
            None => branch.day(
                &branch_config,
                &request.run_context,
                include_snapshot,
                forced_directive.as_mut(),
            )?,
        };
        let state_digests_equal = base_record.state_digest == branch_record.state_digest;
        let all_day_digests_equal = state_digests_equal
            && base_record.metrics_digest == branch_record.metrics_digest
            && base_record.events_digest == branch_record.events_digest
            && base_record.events == branch_record.events;
        let lineage_started = analysis_costs.as_ref().map(|_| Instant::now());
        let (base_birth_records, branch_birth_records) =
            if all_day_digests_equal && lineage_contexts_equal {
                let birth_records = record_shared_births(
                    &base_record.events,
                    &base_initial,
                    &mut base_lineage,
                    &mut branch_lineage,
                );
                (birth_records, None)
            } else {
                lineage_contexts_equal = false;
                let base_birth_records =
                    record_births(&base_record.events, &base_initial, &mut base_lineage);
                let branch_birth_records =
                    record_births(&branch_record.events, &branch_initial, &mut branch_lineage);
                (base_birth_records, Some(branch_birth_records))
            };
        if let (Some(costs), Some(started)) = (analysis_costs.as_deref_mut(), lineage_started) {
            costs.lineage_ns += started.elapsed().as_nanos();
        }
        let lineage_before_difference = analysis_costs.as_ref().map_or(0, |costs| costs.lineage_ns);
        let difference_started = analysis_costs.as_ref().map(|_| Instant::now());
        let difference_input = DailyDifferenceInput {
            day,
            base: &base,
            branch: &branch,
            base_initial: &base_initial,
            branch_initial: &branch_initial,
            base_lineage_agents: &base_lineage.agents,
            branch_lineage_agents: &branch_lineage.agents,
            base_births: &base_birth_records,
            branch_births: branch_birth_records.as_deref(),
            state_digests_equal,
            all_day_digests_equal,
        };
        let difference = match analysis_costs.as_deref_mut() {
            Some(costs) => difference_for_day_fast_measured(difference_input, costs),
            None => difference_for_day_fast_impl(difference_input, None),
        };
        if let (Some(costs), Some(started)) = (analysis_costs.as_deref_mut(), difference_started) {
            let total_ns = started.elapsed().as_nanos();
            let lineage_ns = costs.lineage_ns.saturating_sub(lineage_before_difference);
            costs.d_d_ns += total_ns.saturating_sub(lineage_ns);
        }
        if first_divergence.is_none() && base_record.state_digest != branch_record.state_digest {
            let (base_start, branch_start) = start_states
                .as_ref()
                .expect("day-start state retained until divergence analysis");
            first_divergence = if start_states_differ {
                match recorder_mode {
                    RecorderComparisonMode::DisabledUntil(expected) => Some(expected.clone()),
                    RecorderComparisonMode::Enabled => Some(intervention_boundary_divergence(
                        day,
                        base_start,
                        branch_start,
                    )?),
                }
            } else {
                let phase_force = match request.intervention {
                    BranchIntervention::ForcedAction { agent_id, action } => {
                        Some(ForcedActionDirective::new(agent_id, action, day))
                    }
                    _ => None,
                };
                let storage_result = if matches!(
                    base_start.profile,
                    SimulationRunnerSelection::M3p | SimulationRunnerSelection::M3pPropagation
                ) {
                    locate_m3p_storage_day_divergence_with_mode(
                        base_start,
                        branch_start,
                        &request.config,
                        &branch_config,
                        &request.run_context,
                        request.execution,
                        day,
                        phase_force,
                        recorder_mode,
                        analysis_costs.as_deref_mut(),
                    )?
                } else {
                    if matches!(recorder_mode, RecorderComparisonMode::DisabledUntil(_)) {
                        return Err(BranchError::UnsupportedProfile);
                    }
                    None
                };
                if storage_result.is_some() {
                    storage_result
                } else {
                    let mut base_trace = Vec::new();
                    let mut branch_trace = Vec::new();
                    base_start.record_reference_day(
                        &request.config,
                        &request.run_context,
                        None,
                        &mut base_trace,
                    )?;
                    let mut phase_force = match request.intervention {
                        BranchIntervention::ForcedAction { agent_id, action } => {
                            Some(ForcedActionDirective::new(agent_id, action, day))
                        }
                        _ => None,
                    };
                    branch_start.record_reference_day(
                        &branch_config,
                        &request.run_context,
                        phase_force.as_mut(),
                        &mut branch_trace,
                    )?;
                    first_phase_difference(&base_trace, &branch_trace, day)
                }
            };
        }
        daily_differences.push(difference);
        base_days.push(base_record);
        branch_days.push(branch_record);
    }
    let forced_action = forced_directive.and_then(|directive| directive.outcome);
    Ok(BranchResult {
        base_days,
        branch_days,
        first_divergence,
        daily_differences,
        forced_action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M3StorageRunContext};
    use crate::snapshot::{SnapshotMetadata, encode_snapshot};
    use crate::storage::SegmentedAgentStorage;
    use sim_core::GroupId;

    fn m3_config() -> SimConfig {
        let mut config =
            SimConfig::parse_and_validate(include_str!("../tests/fixtures/m3_propagation.toml"))
                .unwrap();
        config.simulation = None;
        config.propagation = None;
        config.profile = crate::config::SimulationProfile::M3;
        config.validate().unwrap();
        config
    }

    fn m3_runtime(reverse_slots: bool) -> (BranchRuntime, SimConfig, M0RunContext) {
        let config = m3_config();
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let world = crate::initialization::initialize_world(&config).unwrap();
        let metadata = SnapshotMetadata::new(
            world.current_day.as_u32(),
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_snapshot(&world, &metadata).unwrap();
        let mut runtime =
            BranchRuntime::restore(&snapshot, &config, &context, BranchExecutionOption::Serial)
                .unwrap();
        if reverse_slots {
            let storage = runtime.world.segmented_storage.as_ref().unwrap();
            let mut agents = storage.to_agents();
            agents.reverse();
            runtime.world.segmented_storage = Some(SegmentedAgentStorage::from_agents(&agents));
            runtime.context = StorageContext::M3(
                M3StorageRunContext::new(&runtime.world, &config, &context).unwrap(),
            );
        }
        (runtime, config, context)
    }

    fn recorder_test_config(population: bool, propagation: bool) -> SimConfig {
        let fixture = if population {
            include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
        } else {
            include_str!("../tests/fixtures/m3_propagation.toml")
        };
        let mut config = SimConfig::parse_and_validate(fixture).unwrap();
        config.simulation = None;
        config.propagation = propagation.then_some(config.propagation).flatten();
        if population {
            config.profile = crate::config::SimulationProfile::M3p;
        } else {
            config.profile = crate::config::SimulationProfile::M3;
            config.population = None;
        }
        if propagation {
            config.simulation = Some(crate::config::SimulationConfig {
                systems: crate::config::SimulationSystems {
                    relationship_graph: true,
                    population_dynamics: population,
                    propagation: true,
                },
            });
        }
        config.validate().unwrap();
        config
    }

    fn recorder_test_snapshot(config: &SimConfig, context: &M0RunContext) -> CanonicalSnapshot {
        let world = crate::initialization::initialize_world(config).unwrap();
        let metadata = SnapshotMetadata::new(
            world.current_day.as_u32(),
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        match config.runner_selection() {
            SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
            SimulationRunnerSelection::M3Propagation => {
                let state = M3PropagationReferenceState::new(world, config, context).unwrap();
                crate::m3prop::encode_m3prop_snapshot(&state, &metadata).unwrap()
            }
            SimulationRunnerSelection::M3p => {
                let state = M3pReferenceState::new(world, config, context).unwrap();
                crate::m3p::encode_m3p_snapshot(&state, &metadata).unwrap()
            }
            SimulationRunnerSelection::M3pPropagation => {
                let state =
                    crate::m3pprop::M3pPropagationReferenceState::new(world, config, context)
                        .unwrap();
                crate::m3pprop::encode_m3pprop_snapshot(&state, &metadata).unwrap()
            }
            SimulationRunnerSelection::M2 => unreachable!(),
        }
    }

    fn reverse_branch_start_slots(start: &mut BranchStartState) {
        let storage = start.world.segmented_storage.as_ref().unwrap();
        let mut agents = storage.to_agents();
        agents.reverse();
        start.world.segmented_storage = Some(SegmentedAgentStorage::from_agents(&agents));
    }

    fn birth_event(parent: u32, child: u32, sequence: u64) -> M3pEventRecord {
        M3pEventRecord {
            key: crate::events::EventKey::new(0, 9, crate::events::GLOBAL_PARTITION_KEY, sequence),
            event: crate::m3p::M3pEvent::Birth {
                parent: AgentId(parent),
                child: AgentId(child),
                group: GroupId(0),
            },
        }
    }

    #[test]
    fn shuffled_storage_slots_preserve_branch_day_records() {
        let (mut ordered, config, context) = m3_runtime(false);
        let (mut shuffled, _, _) = m3_runtime(true);
        let left = ordered.day(&config, &context, false, None).unwrap();
        let right = shuffled.day(&config, &context, false, None).unwrap();
        assert_eq!(left, right);
    }

    #[test]
    fn fast_daily_differences_match_canonical_birth_and_slot_paths() {
        let (base, _, _) = m3_runtime(false);
        let (shuffled, _, _) = m3_runtime(true);
        let base_initial = base.initial_agent_ids();
        let shuffled_initial = shuffled.initial_agent_ids();
        let empty_lineages = HashMap::new();
        let empty_births = Vec::new();
        let unchanged_canonical = difference_for_day_canonical(
            0,
            &base,
            &shuffled,
            &base_initial,
            &shuffled_initial,
            &empty_lineages,
            &empty_lineages,
            &empty_births,
            &empty_births,
        );
        let unchanged_fast = difference_for_day_fast(
            0,
            &base,
            &shuffled,
            &base_initial,
            &shuffled_initial,
            &empty_lineages,
            &empty_lineages,
            &empty_births,
            &empty_births,
            true,
            true,
        );
        assert_eq!(unchanged_fast, unchanged_canonical);

        let (base, _, _) = m3_runtime(false);
        let (mut changed, _, _) = m3_runtime(false);
        let storage = changed.world.segmented_storage.as_mut().unwrap();
        let slot = storage.slot_of(AgentId(0)).unwrap();
        storage.food_mut()[slot] += 1.0;
        let changed_initial = changed.initial_agent_ids();
        let canonical = difference_for_day_canonical(
            1,
            &base,
            &changed,
            &base_initial,
            &changed_initial,
            &empty_lineages,
            &empty_lineages,
            &empty_births,
            &empty_births,
        );
        let fast = difference_for_day_fast(
            1,
            &base,
            &changed,
            &base_initial,
            &changed_initial,
            &empty_lineages,
            &empty_lineages,
            &empty_births,
            &empty_births,
            false,
            false,
        );
        assert_eq!(fast, canonical);
        assert_eq!(fast.changed_agent_records, 1);

        let (base, _, _) = m3_runtime(false);
        let (branch, _, _) = m3_runtime(false);
        let key = AgentMatchKey::Birth {
            ancestor: AgentId(1),
            ordinal_path: vec![0],
        };
        let base_births = vec![(AgentId(1), AgentId(300), key.clone())];
        let branch_births = vec![(AgentId(1), AgentId(301), key.clone())];
        let base_birth_map = HashMap::from([(key.clone(), AgentId(300))]);
        let branch_birth_map = HashMap::from([(key, AgentId(301))]);
        let canonical = difference_for_day_canonical(
            2,
            &base,
            &branch,
            &base_initial,
            &base_initial,
            &base_birth_map,
            &branch_birth_map,
            &base_births,
            &branch_births,
        );
        let fast = difference_for_day_fast(
            2,
            &base,
            &branch,
            &base_initial,
            &base_initial,
            &base_birth_map,
            &branch_birth_map,
            &base_births,
            &branch_births,
            false,
            false,
        );
        assert_eq!(fast, canonical);
        assert_eq!(fast.matched_births, vec![(AgentId(300), AgentId(301))]);
        assert_eq!(fast.newborn_crn_loss, 1);

        let (branch, _, _) = m3_runtime(true);
        let canonical = difference_for_day_canonical(
            2,
            &base,
            &branch,
            &base_initial,
            &base_initial,
            &base_birth_map,
            &branch_birth_map,
            &base_births,
            &branch_births,
        );
        let fast = difference_for_day_fast(
            2,
            &base,
            &branch,
            &base_initial,
            &base_initial,
            &base_birth_map,
            &branch_birth_map,
            &base_births,
            &branch_births,
            false,
            false,
        );
        assert_eq!(fast, canonical);
    }

    #[test]
    fn lineage_paths_match_nested_births_after_agent_id_shifts() {
        let initial = BTreeSet::from([AgentId(1)]);
        let base_events = BranchEventsRecord::Population(vec![
            birth_event(1, 2, 0),
            birth_event(1, 3, 1),
            birth_event(2, 4, 2),
        ]);
        let branch_events = BranchEventsRecord::Population(vec![
            birth_event(1, 10, 0),
            birth_event(1, 11, 1),
            birth_event(10, 12, 2),
        ]);
        let mut base_lineage = LineageTracker::default();
        let mut branch_lineage = LineageTracker::default();
        let base = record_births(&base_events, &initial, &mut base_lineage);
        let branch = record_births(&branch_events, &initial, &mut branch_lineage);
        let base_keys: Vec<_> = base.iter().map(|(_, _, key)| key).collect();
        let branch_keys: Vec<_> = branch.iter().map(|(_, _, key)| key).collect();
        assert_eq!(base_keys, branch_keys);
        assert_eq!(
            base_keys[2],
            &AgentMatchKey::Birth {
                ancestor: AgentId(1),
                ordinal_path: vec![0, 0]
            }
        );
        assert_eq!(
            base.iter()
                .zip(branch.iter())
                .filter(|((_, left, _), (_, right, _))| left != right)
                .count(),
            3
        );
    }

    #[test]
    fn shared_lineage_update_matches_independent_birth_tracking() {
        let initial = BTreeSet::from([AgentId(1)]);
        let events =
            BranchEventsRecord::Population(vec![birth_event(1, 2, 0), birth_event(2, 3, 1)]);
        let mut base_lineage = LineageTracker::default();
        let mut branch_lineage = LineageTracker::default();
        let base_records = record_births(&events, &initial, &mut base_lineage);
        let branch_records = record_births(&events, &initial, &mut branch_lineage);
        let mut shared_base_lineage = LineageTracker::default();
        let mut shared_branch_lineage = LineageTracker::default();
        let shared_records = record_shared_births(
            &events,
            &initial,
            &mut shared_base_lineage,
            &mut shared_branch_lineage,
        );
        assert_eq!(base_records, branch_records);
        assert_eq!(shared_records, base_records);
        assert_eq!(shared_base_lineage.lineages, base_lineage.lineages);
        assert_eq!(shared_branch_lineage.lineages, branch_lineage.lineages);
        assert_eq!(shared_base_lineage.agents, base_lineage.agents);
        assert_eq!(shared_branch_lineage.agents, branch_lineage.agents);
        assert_eq!(shared_base_lineage.ordinals, base_lineage.ordinals);
        assert_eq!(shared_branch_lineage.ordinals, branch_lineage.ordinals);

        let (base, _, _) = m3_runtime(false);
        let (branch, _, _) = m3_runtime(false);
        let initial_ids = base.initial_agent_ids();
        let canonical = difference_for_day_canonical(
            0,
            &base,
            &branch,
            &initial_ids,
            &initial_ids,
            &base_lineage.agents,
            &branch_lineage.agents,
            &base_records,
            &branch_records,
        );
        let fast = difference_for_day_fast(
            0,
            &base,
            &branch,
            &initial_ids,
            &initial_ids,
            &shared_base_lineage.agents,
            &shared_branch_lineage.agents,
            &shared_records,
            &shared_records,
            true,
            true,
        );
        assert_eq!(fast, canonical);
    }

    #[test]
    fn storage_phase_locator_matches_reference_for_m3p_execution_options() {
        for (with_propagation, execution, shuffled_branch_slots) in [
            (false, BranchExecutionOption::Serial, false),
            (false, BranchExecutionOption::IncrementalTopology, false),
            (true, BranchExecutionOption::Serial, false),
            (true, BranchExecutionOption::IncrementalTopology, false),
            (false, BranchExecutionOption::Serial, true),
        ] {
            let mut config = crate::config::SimConfig::parse_and_validate(include_str!(
                "../tests/fixtures/m3p_propagation_sustained.toml"
            ))
            .unwrap();
            if !with_propagation {
                config.simulation = None;
                config.propagation = None;
            }
            config.validate().unwrap();
            let run_context =
                M0RunContext::new(config.world.master_seed, config.world.replicate_id);
            let world = crate::initialization::initialize_world(&config).unwrap();
            let metadata = SnapshotMetadata::new(
                world.current_day.as_u32(),
                run_context.master_seed,
                run_context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let snapshot = if with_propagation {
                let state =
                    crate::m3pprop::M3pPropagationReferenceState::new(world, &config, &run_context)
                        .unwrap();
                crate::m3pprop::encode_m3pprop_snapshot(&state, &metadata).unwrap()
            } else {
                let state = M3pReferenceState::new(world, &config, &run_context).unwrap();
                crate::m3p::encode_m3p_snapshot(&state, &metadata).unwrap()
            };
            let mut branch_config = config.clone();
            apply_config_override(
                &mut branch_config,
                ConfigParameter::BirthProbability,
                ConfigValue::F32(1.0),
            )
            .unwrap();
            let request = BranchRequest {
                snapshot: snapshot.clone(),
                config: config.clone(),
                run_context,
                intervention: BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BirthProbability,
                    value: ConfigValue::F32(1.0),
                },
                horizon_days: 1,
                execution,
                snapshot_days: Vec::new(),
            };
            let result = run_counterfactual_branch(&request).unwrap();

            let mut base_runtime =
                BranchRuntime::restore(&snapshot, &config, &run_context, execution).unwrap();
            let mut branch_runtime =
                BranchRuntime::restore(&snapshot, &branch_config, &run_context, execution).unwrap();
            let base_start = base_runtime.start_state();
            let mut branch_start = branch_runtime.start_state();
            if shuffled_branch_slots {
                let storage = branch_start.world.segmented_storage.as_ref().unwrap();
                let mut agents = storage.to_agents();
                agents.reverse();
                branch_start.world.segmented_storage =
                    Some(SegmentedAgentStorage::from_agents(&agents));
            }
            let mut base_trace = Vec::new();
            let mut branch_trace = Vec::new();
            base_start
                .record_reference_day(&config, &run_context, None, &mut base_trace)
                .unwrap();
            branch_start
                .record_reference_day(&branch_config, &run_context, None, &mut branch_trace)
                .unwrap();
            let reference_result = first_phase_difference(&base_trace, &branch_trace, 0);
            let storage_result = if shuffled_branch_slots {
                locate_m3p_storage_day_divergence(
                    &base_start,
                    &branch_start,
                    &config,
                    &branch_config,
                    &run_context,
                    execution,
                    0,
                    None,
                )
                .unwrap()
            } else {
                result.first_divergence.clone()
            };
            assert_eq!(
                storage_result, reference_result,
                "propagation={with_propagation}, execution={execution:?}, shuffled={shuffled_branch_slots}"
            );
            assert!(
                result
                    .first_divergence
                    .as_ref()
                    .is_some_and(|difference| difference.slot
                        == RecordedSlot::Hook(crate::system_registry::RegisteredHook {
                            system_id: SystemId::PopulationDynamics,
                            slot: crate::system_registry::PhaseSlot::Phase9B,
                        }))
            );
        }
    }

    #[test]
    fn selective_recorder_matches_full_comparison_for_all_profiles() {
        for (population, propagation) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            let config = recorder_test_config(population, propagation);
            let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
            let snapshot = recorder_test_snapshot(&config, &context);
            let intervention = if population {
                BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BirthProbability,
                    value: ConfigValue::F32(1.0),
                }
            } else {
                BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BaseMetabolicCost,
                    value: ConfigValue::F32(config.environment.base_metabolic_cost + 0.01),
                }
            };
            let mut branch_config = config.clone();
            if let BranchIntervention::ConfigOverride { parameter, value } = &intervention {
                apply_config_override(&mut branch_config, *parameter, *value).unwrap();
            }
            let request = BranchRequest {
                snapshot: snapshot.clone(),
                config: config.clone(),
                run_context: context,
                intervention,
                horizon_days: 1,
                execution: BranchExecutionOption::Serial,
                snapshot_days: Vec::new(),
            };
            let actual = run_counterfactual_branch(&request).unwrap();

            let mut base_runtime =
                BranchRuntime::restore(&snapshot, &config, &context, BranchExecutionOption::Serial)
                    .unwrap();
            let mut branch_runtime = BranchRuntime::restore(
                &snapshot,
                &branch_config,
                &context,
                BranchExecutionOption::Serial,
            )
            .unwrap();
            let base_start = base_runtime.start_state();
            let mut branch_start = branch_runtime.start_state();
            reverse_branch_start_slots(&mut branch_start);
            let mut base_trace = Vec::new();
            let mut branch_trace = Vec::new();
            base_start
                .record_reference_day(&config, &context, None, &mut base_trace)
                .unwrap();
            branch_start
                .record_reference_day(&branch_config, &context, None, &mut branch_trace)
                .unwrap();
            let reference = first_phase_difference(&base_trace, &branch_trace, 0);
            assert_eq!(actual.first_divergence, reference);

            if population {
                assert!(matches!(
                    &actual.branch_days[0].events,
                    BranchEventsRecord::Population(records)
                        if records.iter().any(|record| matches!(
                            &record.event,
                            crate::m3p::M3pEvent::Birth { .. }
                        ))
                ));
                let storage = locate_m3p_storage_day_divergence(
                    &base_start,
                    &branch_start,
                    &config,
                    &branch_config,
                    &context,
                    BranchExecutionOption::Serial,
                    0,
                    None,
                )
                .unwrap();
                assert_eq!(storage, reference);
                assert_eq!(
                    storage.as_ref().map(|divergence| &divergence.slot),
                    Some(&RecordedSlot::Hook(
                        crate::system_registry::RegisteredHook {
                            system_id: SystemId::PopulationDynamics,
                            slot: crate::system_registry::PhaseSlot::Phase9B,
                        }
                    ))
                );
            }
        }
    }
}
