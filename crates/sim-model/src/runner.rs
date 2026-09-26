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

use crate::config::SimConfig;
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
    Intent, IntentError, Phase4CandidateIndexScratch,
    generate_intents_storage_with_candidate_index,
    generate_intents_storage_with_candidate_index_rayon, generate_intents_storage_with_scratch,
    phase4_generate_intents_into,
};
use crate::metrics::{DailyMetrics, Phase10Error, phase10_observe_with_scratch};
use crate::partitioning::{
    Phase5Error, Phase5PartitionScratch, phase5_partition_intents_baseline,
    phase5_partition_intents_from_vec_with_scratch,
};
use crate::phases::{
    Phase9Error, phase1_resource_regrowth, phase2_biological_degradation,
    phase9_mortality_commitment,
};
use crate::resolution::{
    Phase6AError, Phase6BError, Phase6BResolutionScratch, Phase7Error, Phase8Error,
    Phase8WelfareScratch, phase6a_work_resolution, phase6a_work_resolution_storage,
    phase6b_targeted_resolution, phase6b_targeted_resolution_storage_baseline,
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
use sim_core::{AgentId, SimulationDay};
use std::num::NonZeroUsize;

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
        Phase4ExecutionMode::Serial,
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
        Phase4ExecutionMode::Serial,
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
        Phase4ExecutionMode::Serial,
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
    phase4_mode: Phase4ExecutionMode<'_>,
    phase8_scratch: Phase8RuntimeScratch<'_>,
    phase3_scratch: Phase3RuntimeScratch<'_>,
    phase5_mode: Phase5RuntimeMode,
    phase5_scratch: Option<&mut Phase5PartitionScratch>,
    phase6b_mode: Phase6BRuntimeMode,
    phase6b_scratch: Option<&mut Phase6BResolutionScratch>,
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
        Phase4ExecutionMode::Serial,
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
    )
}

/// Executes Hybrid days with an experimental, caller-pooled Rayon Phase4 path.
/// Existing runners remain serial; chunk merging preserves canonical input order.
pub fn run_hybrid_authority_days_with_rayon_phase4(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    options: &DayExecutionOptions,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<Vec<DayOutcome>, M0RunError> {
    if !hybrid_world.is_hybrid() {
        return Err(M0RunError::InvariantViolation(
            "Rayon Phase4 experiment requires Hybrid Storage Authority".to_string(),
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
        Phase4ExecutionMode::Rayon { pool, chunk_size },
        Phase5RuntimeMode::OwnedFast,
        Phase6BRuntimeMode::StorageFast,
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
        Phase4ExecutionMode::Serial,
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
        Phase4ExecutionMode::Serial,
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
        Phase4ExecutionMode::Serial,
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
        Phase4ExecutionMode::Serial,
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
    phase4_mode: Phase4ExecutionMode<'_>,
    phase5_mode: Phase5RuntimeMode,
    phase6b_mode: Phase6BRuntimeMode,
) -> Result<Vec<DayOutcome>, M0RunError> {
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
            phase4_mode,
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
