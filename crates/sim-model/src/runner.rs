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
use crate::decision::{DecisionError, PrimaryActionChoice, phase4_primary_action_selection_into};
use crate::events::{
    Event, EventBuffer, EventError, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    event_from_daily_metrics, events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    phase11_flush_events,
};
use crate::features::{AgentFeatures, Phase3Error, phase3_observation_and_features_into};
use crate::intents::{IntentError, phase4_generate_intents};
use crate::metrics::{DailyMetrics, Phase10Error, phase10_observe};
use crate::partitioning::{Phase5Error, phase5_partition_intents};
use crate::phases::{
    Phase9Error, phase1_resource_regrowth, phase2_biological_degradation,
    phase9_mortality_commitment,
};
use crate::resolution::{
    Phase6AError, Phase6BError, Phase7Error, Phase8Error, phase6a_work_resolution,
    phase6b_targeted_resolution, phase7_market_clearance_with_config,
    phase8_welfare_distribution_with_config,
};
use crate::snapshot::{
    CanonicalSnapshot, SNAPSHOT_SCHEMA_VERSION, SnapshotError, SnapshotMetadata, encode_snapshot,
};
use crate::state::WorldState;
use serde::{Deserialize, Serialize};
use sim_core::SimulationDay;

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
    run_m0_day_with_scratch(
        world,
        config,
        context,
        options,
        &mut features_scratch,
        &mut choices_scratch,
    )
}

/// Executes exactly one full deterministic simulation day (Phases 1 through 11)
/// reusing external scratch buffers for Phase 3 features and Phase 4 action choices.
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
pub fn run_m0_day_with_scratch(
    world: &mut WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    features_scratch: &mut Vec<AgentFeatures>,
    choices_scratch: &mut Vec<PrimaryActionChoice>,
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
    let intents = phase4_generate_intents(world, &effective_config, choices_scratch)?;

    // 7. Phase 5: Locality Partitioning
    let partitions = phase5_partition_intents(&intents)?;

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
        let metrics = phase10_observe(world, executed_day)?;
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
    for _ in 0..days {
        let outcome = run_m0_day_with_scratch(
            world,
            config,
            context,
            options,
            &mut features_scratch,
            &mut choices_scratch,
        )?;
        outcomes.push(outcome);
    }
    Ok(outcomes)
}
