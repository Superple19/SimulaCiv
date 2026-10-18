#![allow(clippy::disallowed_methods)]

use sim_core::{AgentId, SimulationDay};
use sim_model::decision::{
    Action, PrimaryActionChoice, phase4_primary_action_selection_storage_into,
};
use sim_model::events::{
    EventBuffer, event_from_daily_metrics, events_from_market_resolution,
    events_from_mortality_resolution, events_from_targeted_resolution,
    events_from_welfare_resolution, events_from_work_resolution, phase11_flush_events,
};
use sim_model::features::{
    AgentFeatures, Phase3ScarcityScratch, phase3_observation_and_features_storage_with_scratch,
};
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::intents::{
    Intent, M3Phase4SlotProjection, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_storage_into, generate_intents_m3_storage_into_with_timing,
};
use sim_model::metrics::M3DailyMetrics;
use sim_model::partitioning::{
    Phase5PartitionScratch, SettlementIntentPartition,
    phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::{phase1_resource_regrowth, phase9_mortality_commitment_storage};
use sim_model::relationship_graph::RelationshipGraph;
use sim_model::resolution::{
    M3Phase6BLevelStats, M3Phase6BParallelScratch, M3Phase6BStageTimings, M3Phase6BStorageScratch,
    Phase8WelfareScratch, TargetedActionKind, TargetedOutcome, compare_keyed_interactions,
    compute_m3_resolution_key, phase6a_work_resolution_storage,
    phase6b_targeted_resolution_storage_m3_parallel_with_timing,
    phase6b_targeted_resolution_storage_m3_with_scratch,
    phase6b_targeted_resolution_storage_m3_with_timing,
    phase7_market_clearance_storage_with_config,
    phase8_welfare_distribution_storage_with_config_and_scratch,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ParallelStorageRunContext, M3StorageRunContext,
    run_m3_storage_days, run_m3_storage_days_with_parallel_phase6b,
    run_m3_storage_days_with_parallel_phase6b_timing, run_m3_storage_days_with_phase6b_timing,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{SimConfig, SimulationProfile, initialize_world};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

const M2_40_CONFIG: &str = r#"
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

const MASTER_SEED: u64 = 81_985_529_216_486_895;
const REPLICATE_ID: u32 = 7;
const WARMUPS: usize = 2;
const SAMPLES: usize = 11;
const DAYS_PER_SAMPLE: u32 = 3;

#[derive(Clone, Copy)]
struct FoodConfig {
    initial_settlement_resource: f32,
    carrying_capacity: f32,
    regrowth_rate: f32,
    base_metabolic_cost: f32,
    health_decay_rate: f32,
    base_work_yield: f32,
}

const M2_40_FOOD: FoodConfig = FoodConfig {
    initial_settlement_resource: 2_000.0,
    carrying_capacity: 10_000.0,
    regrowth_rate: 0.1,
    base_metabolic_cost: 1.0,
    health_decay_rate: 0.05,
    base_work_yield: 2.0,
};

// The grid's least-change accepted candidate modifies only metabolic cost.
const SUSTAINED_FOOD: FoodConfig = FoodConfig {
    base_metabolic_cost: 0.48,
    ..M2_40_FOOD
};

#[derive(Clone, Copy)]
struct Cell {
    population: u64,
    settlements: u32,
    degree: u32,
    p_local: f32,
}

#[derive(Clone, Copy)]
struct KeyedEntry {
    key: u64,
    target: AgentId,
    initiator: AgentId,
    action: TargetedActionKind,
}

#[derive(Clone, Copy)]
struct ConflictStats {
    fraction: Option<f64>,
    mean_contention: Option<f64>,
}

#[derive(Default)]
struct DailyVolume {
    keyed: u64,
    cross_partition: u64,
    conflict_fraction_sum: f64,
    conflict_fraction_days: u32,
    contention_sum: f64,
    contention_days: u32,
    level_count_sum: u64,
    max_level_size_sum: u64,
    max_level_size_days: u32,
}

struct ManualRun {
    world: WorldState,
    storage: SegmentedAgentStorage,
    relationship_graph: RelationshipGraph,
    run_context: M0RunContext,
    projection: M3Phase4SlotProjection,
    features: Vec<AgentFeatures>,
    choices: Vec<PrimaryActionChoice>,
    intents: Vec<Intent>,
    candidates: Vec<AgentId>,
    probe_intents: Vec<Intent>,
    probe_candidates: Vec<AgentId>,
    phase3: Phase3ScarcityScratch,
    phase5: Phase5PartitionScratch,
    phase6b_parallel: M3Phase6BParallelScratch,
    phase6b_serial: M3Phase6BStorageScratch,
    phase6b_probe: M3Phase6BStorageScratch,
    phase8: Phase8WelfareScratch,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct OutcomeCounts {
    zero_target: u64,
    initiator_ineligible: u64,
    target_ineligible: u64,
    theft_failed: u64,
    applied_positive: u64,
    applied_zero: u64,
}

struct InteractionDayProfile {
    day: u32,
    alive: usize,
    mean_alive_food: Option<f64>,
    action_counts: [u64; Action::COUNT],
    give_some: u64,
    give_none: u64,
    steal_some: u64,
    steal_none: u64,
    keyed: u64,
    cross_partition: u64,
    outcomes: OutcomeCounts,
    metrics: M3DailyMetrics,
    events: Vec<sim_model::EventRecord>,
    phase4_intent_timing: sim_model::M3Phase4IntentTimings,
    phase4_hook_timed_wall: Duration,
    phase4_hook_plain_wall: Duration,
    phase6b_timing: M3Phase6BStageTimings,
    phase6b_hook_timed_wall: Duration,
    phase6b_hook_plain_wall: Duration,
    phase_times_us: [f64; 17],
}

#[derive(Default, Clone, Copy)]
struct SearchSample {
    phase4_hook_overhead_us: f64,
    phase4_hook_overhead_percent: f64,
    search_setup_us: f64,
    work_us: f64,
    search_per_applied_us: f64,
    work_per_applied_us: f64,
    search_to_work_ratio: f64,
    phase6b_hook_overhead_us: f64,
    phase6b_hook_overhead_percent: f64,
    phase_times_us: [f64; 17],
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SearchCounts {
    actions: [u64; Action::COUNT],
    give_some: u64,
    give_none: u64,
    steal_some: u64,
    steal_none: u64,
    keyed: u64,
    cross_partition: u64,
    outcomes: OutcomeCounts,
}

fn config_for(cell: Cell) -> SimConfig {
    config_for_seed(cell, MASTER_SEED)
}

fn config_for_seed(cell: Cell, seed: u64) -> SimConfig {
    config_for_food(cell, seed, SUSTAINED_FOOD)
}

fn config_for_food(cell: Cell, seed: u64, food: FoodConfig) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 config parses");
    config.profile = SimulationProfile::M3;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.world.master_seed = seed;
    config.world.replicate_id = REPLICATE_ID;
    config.interaction.relationship_degree_k = Some(cell.degree as i64);
    config.interaction.p_local = Some(cell.p_local);
    config.world.initial_settlement_resource = food.initial_settlement_resource;
    config.environment.carrying_capacity = food.carrying_capacity;
    config.environment.regrowth_rate = food.regrowth_rate;
    config.environment.base_metabolic_cost = food.base_metabolic_cost;
    config.environment.health_decay_rate = food.health_decay_rate;
    config.economy.base_work_yield = food.base_work_yield;
    config.validate().expect("M3 benchmark config validates");
    config
}

impl ManualRun {
    fn from_hybrid_world(config: &SimConfig, hybrid_world: HybridWorldState) -> Self {
        let world = hybrid_world.world;
        let storage = hybrid_world
            .segmented_storage
            .expect("hybrid checkpoint has segmented storage");
        let memberships: Vec<_> = storage
            .agent_ids()
            .iter()
            .copied()
            .zip(storage.group_ids().iter().copied())
            .collect();
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let graph = RelationshipGraph::generate(
            &memberships,
            MASTER_SEED,
            REPLICATE_ID,
            config.interaction.relationship_degree_k.unwrap() as u32,
            config.interaction.p_local.unwrap(),
        )
        .expect("relationship graph builds");
        let projection =
            M3Phase4SlotProjection::new(&graph, &storage).expect("slot projection builds");
        let n = storage.len();
        let k = world.settlements.len();
        Self {
            world,
            storage,
            relationship_graph: graph,
            run_context,
            projection,
            features: Vec::with_capacity(n),
            choices: Vec::with_capacity(n),
            intents: Vec::with_capacity(n),
            candidates: Vec::with_capacity(
                config.interaction.relationship_degree_k.unwrap() as usize
            ),
            probe_intents: Vec::with_capacity(n),
            probe_candidates: Vec::with_capacity(
                config.interaction.relationship_degree_k.unwrap() as usize
            ),
            phase3: Phase3ScarcityScratch::with_capacity(k),
            phase5: Phase5PartitionScratch::with_capacity(k),
            phase6b_parallel: M3Phase6BParallelScratch::with_capacity(n, k, n),
            phase6b_serial: M3Phase6BStorageScratch::with_capacity(n),
            phase6b_probe: M3Phase6BStorageScratch::with_capacity(n),
            phase8: Phase8WelfareScratch::with_capacity(k),
        }
    }

    fn day_parallel_timed(
        &mut self,
        config: &SimConfig,
        pool: &rayon::ThreadPool,
        chunk_size: NonZeroUsize,
    ) -> (DailyVolume, M3Phase6BStageTimings, M3Phase6BLevelStats) {
        let mut volume = DailyVolume::default();
        let day = self.world.current_day.as_u32();
        let next_day = day.checked_add(1).expect("day does not overflow");
        let mut effective = config.clone();
        effective.world.master_seed = self.run_context.master_seed;
        effective.world.replicate_id = self.run_context.replicate_id;

        phase1_resource_regrowth(&mut self.world, &effective);
        self.storage.phase2_degradation_with_config(&effective);
        self.features.clear();
        phase3_observation_and_features_storage_with_scratch(
            &self.storage,
            &self.world.settlements,
            &effective,
            &mut self.features,
            &mut self.phase3,
        )
        .expect("phase 3 succeeds");
        self.choices.clear();
        phase4_primary_action_selection_storage_into(
            &self.storage,
            self.world.current_day,
            &effective,
            &self.features,
            &mut self.choices,
        )
        .expect("Phase 4 selection succeeds");
        self.intents.clear();
        generate_intents_m3_storage_into(
            &self.storage,
            self.world.current_day,
            &effective,
            &self.choices,
            &self.projection,
            &mut self.candidates,
            &mut self.intents,
        )
        .expect("M3 Phase 4 intent generation succeeds");
        volume.cross_partition =
            count_cross_partition_interactions_storage_per_day(&self.intents, &self.storage)
                .expect("cross-partition count succeeds");

        let partitions =
            phase5_partition_intents_from_vec_with_scratch(&mut self.intents, &mut self.phase5)
                .expect("phase 5 partitioning succeeds");
        let keyed =
            keyed_stream_from_partitions(&partitions, self.world.current_day, self.run_context);
        volume.keyed = keyed.len() as u64;
        let conflict = conflict_stats(&keyed);
        if let Some(value) = conflict.fraction {
            volume.conflict_fraction_sum = value;
            volume.conflict_fraction_days = 1;
        }
        if let Some(value) = conflict.mean_contention {
            volume.contention_sum = value;
            volume.contention_days = 1;
        }

        phase6a_work_resolution_storage(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
        )
        .expect("Phase 6A succeeds");
        let (_, stage_timings) = phase6b_targeted_resolution_storage_m3_parallel_with_timing(
            &mut self.storage,
            &self.world.settlements,
            self.world.current_day,
            &effective,
            &partitions,
            &mut self.phase6b_parallel,
            pool,
            chunk_size,
        )
        .expect("parallel M3 Phase 6B succeeds");
        let level_stats = self.phase6b_parallel.level_stats();
        volume.level_count_sum = level_stats.non_empty_level_count as u64;
        volume.max_level_size_sum = level_stats.max_level_size as u64;
        volume.max_level_size_days = 1;

        phase7_market_clearance_storage_with_config(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
            &effective.economy,
        )
        .expect("phase 7 succeeds");
        phase8_welfare_distribution_storage_with_config_and_scratch(
            &mut self.storage,
            &mut self.world.settlements,
            &effective,
            &mut self.phase8,
        )
        .expect("phase 8 succeeds");
        phase9_mortality_commitment_storage(&mut self.storage).expect("phase 9 succeeds");
        self.world.current_day = SimulationDay(next_day);
        (volume, stage_timings, level_stats)
    }
}

fn assert_profile_matches_runner(
    profile: &InteractionDayProfile,
    measured: &ManualRun,
    oracle: &HybridWorldState,
    oracle_context: &M3StorageRunContext,
    outcome: &sim_model::M3DayOutcome,
) {
    assert_eq!(profile.day, outcome.executed_day);
    assert_eq!(Some(&profile.metrics), outcome.metrics.as_ref());
    assert_eq!(profile.events, outcome.events);
    let measured_hash = canonical_m3_state_hash_from_storage(
        &measured.storage,
        measured.world.current_day,
        &measured.world.settlements,
        &measured.relationship_graph,
    )
    .expect("measured State digest succeeds");
    let oracle_hash = canonical_m3_state_hash_from_storage(
        oracle.storage().expect("oracle storage exists"),
        oracle.world.current_day,
        &oracle.world.settlements,
        oracle_context.relationship_graph(),
    )
    .expect("production runner State digest succeeds");
    assert_eq!(measured_hash, oracle_hash);
}

#[allow(clippy::too_many_arguments)]
fn run_profile_phase4(
    storage: &SegmentedAgentStorage,
    day: SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    projection: &M3Phase4SlotProjection,
    candidate_scratch: &mut Vec<AgentId>,
    out: &mut Vec<Intent>,
    timed: bool,
) -> (Duration, sim_model::M3Phase4IntentTimings) {
    let started = Instant::now();
    let timings = if timed {
        generate_intents_m3_storage_into_with_timing(
            storage,
            day,
            config,
            choices,
            projection,
            candidate_scratch,
            out,
        )
        .expect("timed phase 4 succeeds")
    } else {
        generate_intents_m3_storage_into(
            storage,
            day,
            config,
            choices,
            projection,
            candidate_scratch,
            out,
        )
        .expect("phase 4 succeeds");
        sim_model::M3Phase4IntentTimings::default()
    };
    (started.elapsed(), timings)
}

fn run_profile_phase6b(
    storage: &mut SegmentedAgentStorage,
    settlements: &[sim_model::state::SettlementState],
    day: SimulationDay,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
    scratch: &mut M3Phase6BStorageScratch,
    timed: bool,
) -> (
    Duration,
    Vec<sim_model::SettlementTargetedResolution>,
    M3Phase6BStageTimings,
) {
    let started = Instant::now();
    if timed {
        let (output, timings) = phase6b_targeted_resolution_storage_m3_with_timing(
            storage,
            settlements,
            day,
            config,
            partitions,
            scratch,
        )
        .expect("timed phase 6B succeeds");
        (started.elapsed(), output, timings)
    } else {
        let output = phase6b_targeted_resolution_storage_m3_with_scratch(
            storage,
            settlements,
            day,
            config,
            partitions,
            scratch,
        )
        .expect("phase 6B succeeds");
        (started.elapsed(), output, M3Phase6BStageTimings::default())
    }
}

fn advance_interaction_profile(
    run: &mut ManualRun,
    config: &SimConfig,
    compare_timing_hooks: bool,
    reverse_hook_order: bool,
) -> InteractionDayProfile {
    let day = run.world.current_day.as_u32();
    let next_day = day.checked_add(1).expect("day does not overflow");
    let current_day = run.world.current_day;
    let mut effective = config.clone();
    effective.world.master_seed = run.run_context.master_seed;
    effective.world.replicate_id = run.run_context.replicate_id;

    let mut phase_times_us = [0.0; 17];
    let phase_started = Instant::now();
    phase1_resource_regrowth(&mut run.world, &effective);
    phase_times_us[0] = elapsed_us(phase_started.elapsed());
    let phase_started = Instant::now();
    run.storage.phase2_degradation_with_config(&effective);
    phase_times_us[1] = elapsed_us(phase_started.elapsed());
    let phase_started = Instant::now();
    run.features.clear();
    phase3_observation_and_features_storage_with_scratch(
        &run.storage,
        &run.world.settlements,
        &effective,
        &mut run.features,
        &mut run.phase3,
    )
    .expect("phase 3 succeeds");
    phase_times_us[2] = elapsed_us(phase_started.elapsed());
    let phase_started = Instant::now();
    run.choices.clear();
    phase4_primary_action_selection_storage_into(
        &run.storage,
        current_day,
        &effective,
        &run.features,
        &mut run.choices,
    )
    .expect("phase 4 action selection succeeds");
    phase_times_us[3] = elapsed_us(phase_started.elapsed());

    let (phase4_intent_timing, phase4_hook_timed_wall, phase4_hook_plain_wall) =
        if compare_timing_hooks {
            let (timed_wall, timing, plain_wall) = if reverse_hook_order {
                let (plain_wall, _) = run_profile_phase4(
                    &run.storage,
                    current_day,
                    &effective,
                    &run.choices,
                    &run.projection,
                    &mut run.probe_candidates,
                    &mut run.probe_intents,
                    false,
                );
                let (timed_wall, timing) = run_profile_phase4(
                    &run.storage,
                    current_day,
                    &effective,
                    &run.choices,
                    &run.projection,
                    &mut run.candidates,
                    &mut run.intents,
                    true,
                );
                (timed_wall, timing, plain_wall)
            } else {
                let (timed_wall, timing) = run_profile_phase4(
                    &run.storage,
                    current_day,
                    &effective,
                    &run.choices,
                    &run.projection,
                    &mut run.candidates,
                    &mut run.intents,
                    true,
                );
                let (plain_wall, _) = run_profile_phase4(
                    &run.storage,
                    current_day,
                    &effective,
                    &run.choices,
                    &run.projection,
                    &mut run.probe_candidates,
                    &mut run.probe_intents,
                    false,
                );
                (timed_wall, timing, plain_wall)
            };
            assert_eq!(run.intents, run.probe_intents);
            (timing, timed_wall, plain_wall)
        } else {
            let started = Instant::now();
            generate_intents_m3_storage_into(
                &run.storage,
                current_day,
                &effective,
                &run.choices,
                &run.projection,
                &mut run.candidates,
                &mut run.intents,
            )
            .expect("phase 4 intent generation succeeds");
            (
                sim_model::M3Phase4IntentTimings::default(),
                started.elapsed(),
                Duration::ZERO,
            )
        };
    phase_times_us[4] = elapsed_us(phase4_hook_timed_wall);

    let mut action_counts = [0u64; Action::COUNT];
    for choice in &run.choices {
        action_counts[choice.action.index()] += 1;
    }
    let mut give_some = 0;
    let mut give_none = 0;
    let mut steal_some = 0;
    let mut steal_none = 0;
    for intent in &run.intents {
        match intent {
            Intent::GiveFood {
                target_agent_id: Some(_),
                ..
            } => give_some += 1,
            Intent::GiveFood {
                target_agent_id: None,
                ..
            } => give_none += 1,
            Intent::StealFood {
                target_agent_id: Some(_),
                ..
            } => steal_some += 1,
            Intent::StealFood {
                target_agent_id: None,
                ..
            } => steal_none += 1,
            _ => {}
        }
    }
    let cross_partition =
        count_cross_partition_interactions_storage_per_day(&run.intents, &run.storage)
            .expect("cross-partition counting succeeds");
    let phase_started = Instant::now();
    let partitions =
        phase5_partition_intents_from_vec_with_scratch(&mut run.intents, &mut run.phase5)
            .expect("phase 5 succeeds");
    phase_times_us[5] = elapsed_us(phase_started.elapsed());
    let keyed = keyed_stream_from_partitions(&partitions, current_day, run.run_context);
    let keyed_count = keyed.len() as u64;
    let phase_started = Instant::now();
    let work_resolutions =
        phase6a_work_resolution_storage(&mut run.storage, &mut run.world.settlements, &partitions)
            .expect("phase 6A succeeds");
    phase_times_us[6] = elapsed_us(phase_started.elapsed());

    let (resolutions, phase6b_timing, phase6b_hook_timed_wall, phase6b_hook_plain_wall) =
        if compare_timing_hooks {
            let mut probe_storage = run.storage.clone();
            let (timed_output, timing, plain_output, timed_wall, plain_wall) = if reverse_hook_order
            {
                let (plain_wall, plain_output, _) = run_profile_phase6b(
                    &mut run.storage,
                    &run.world.settlements,
                    current_day,
                    &effective,
                    &partitions,
                    &mut run.phase6b_serial,
                    false,
                );
                let (timed_wall, timed_output, timing) = run_profile_phase6b(
                    &mut probe_storage,
                    &run.world.settlements,
                    current_day,
                    &effective,
                    &partitions,
                    &mut run.phase6b_probe,
                    true,
                );
                (timed_output, timing, plain_output, timed_wall, plain_wall)
            } else {
                let (timed_wall, timed_output, timing) = run_profile_phase6b(
                    &mut probe_storage,
                    &run.world.settlements,
                    current_day,
                    &effective,
                    &partitions,
                    &mut run.phase6b_probe,
                    true,
                );
                let (plain_wall, plain_output, _) = run_profile_phase6b(
                    &mut run.storage,
                    &run.world.settlements,
                    current_day,
                    &effective,
                    &partitions,
                    &mut run.phase6b_serial,
                    false,
                );
                (timed_output, timing, plain_output, timed_wall, plain_wall)
            };
            assert_eq!(timed_output, plain_output);
            assert_eq!(probe_storage.food(), run.storage.food());
            assert_eq!(
                canonical_m3_state_hash_from_storage(
                    &probe_storage,
                    current_day,
                    &run.world.settlements,
                    &run.relationship_graph,
                )
                .expect("timed Phase 6B state digest succeeds"),
                canonical_m3_state_hash_from_storage(
                    &run.storage,
                    current_day,
                    &run.world.settlements,
                    &run.relationship_graph,
                )
                .expect("untimed Phase 6B state digest succeeds")
            );
            (plain_output, timing, timed_wall, plain_wall)
        } else {
            let (plain_wall, output, _) = run_profile_phase6b(
                &mut run.storage,
                &run.world.settlements,
                current_day,
                &effective,
                &partitions,
                &mut run.phase6b_serial,
                false,
            );
            let _ = plain_wall;
            (
                output,
                M3Phase6BStageTimings::default(),
                Duration::ZERO,
                Duration::ZERO,
            )
        };
    let mut outcomes = OutcomeCounts::default();
    for resolution in resolutions.iter().flat_map(|group| &group.resolutions) {
        match resolution.outcome {
            TargetedOutcome::ZeroTarget => outcomes.zero_target += 1,
            TargetedOutcome::InitiatorIneligible => outcomes.initiator_ineligible += 1,
            TargetedOutcome::TargetIneligible => outcomes.target_ineligible += 1,
            TargetedOutcome::TheftFailed => outcomes.theft_failed += 1,
            TargetedOutcome::Applied { amount } if amount > 0.0 => outcomes.applied_positive += 1,
            TargetedOutcome::Applied { .. } => outcomes.applied_zero += 1,
        }
    }

    phase_times_us[7] = elapsed_us(phase6b_timing.validation_slot_lookup);
    phase_times_us[8] = elapsed_us(phase6b_timing.key_generation);
    phase_times_us[9] = elapsed_us(phase6b_timing.global_sort);
    phase_times_us[10] = elapsed_us(phase6b_timing.evaluation_writeback_output);
    let phase_started = Instant::now();
    let market_resolutions = phase7_market_clearance_storage_with_config(
        &mut run.storage,
        &mut run.world.settlements,
        &partitions,
        &effective.economy,
    )
    .expect("phase 7 succeeds");
    phase_times_us[11] = elapsed_us(phase_started.elapsed());
    let phase_started = Instant::now();
    let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
        &mut run.storage,
        &mut run.world.settlements,
        &effective,
        &mut run.phase8,
    )
    .expect("phase 8 succeeds");
    phase_times_us[12] = elapsed_us(phase_started.elapsed());
    let phase_started = Instant::now();
    let mortality_resolution =
        phase9_mortality_commitment_storage(&mut run.storage).expect("phase 9 succeeds");
    phase_times_us[13] = elapsed_us(phase_started.elapsed());

    let phase_started = Instant::now();
    let mut event_buffer = EventBuffer::with_capacity(
        run.storage.len().saturating_mul(2) + run.world.settlements.len() + 4,
    );
    let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
    for work in &work_resolutions {
        let work_events = events_from_work_resolution(day, work);
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
    for targeted in &resolutions {
        let mut targeted_events = events_from_targeted_resolution(day, targeted);
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
        event_buffer.push_all(events_from_market_resolution(day, market));
    }
    for welfare in &welfare_resolutions {
        event_buffer.push_all(events_from_welfare_resolution(day, welfare));
    }
    event_buffer.push_all(events_from_mortality_resolution(day, &mortality_resolution));
    phase_times_us[14] = elapsed_us(phase_started.elapsed());

    let phase_started = Instant::now();
    let base_metrics = run
        .storage
        .phase10_metrics(&run.world.settlements, day)
        .expect("phase 10 succeeds");
    event_buffer.push(event_from_daily_metrics(&base_metrics));
    phase_times_us[15] = elapsed_us(phase_started.elapsed());
    let (cross_partition_edge_count, total_directed_edge_count) =
        run.relationship_graph.edge_ratio_counts();
    let metrics = M3DailyMetrics {
        metrics: base_metrics,
        cross_partition_edge_count,
        total_directed_edge_count,
        cross_partition_interaction_count: cross_partition,
    };
    let phase_started = Instant::now();
    let events = phase11_flush_events(&mut event_buffer).expect("phase 11 event flush succeeds");
    phase_times_us[16] = elapsed_us(phase_started.elapsed());
    let alive = run.storage.alive().iter().filter(|&&alive| alive).count();
    let mean_alive_food = (alive > 0).then(|| {
        run.storage
            .alive()
            .iter()
            .zip(run.storage.food())
            .filter(|(alive, _)| **alive)
            .map(|(_, &food)| food as f64)
            .sum::<f64>()
            / alive as f64
    });
    run.world.current_day = SimulationDay(next_day);

    InteractionDayProfile {
        day,
        alive,
        mean_alive_food,
        action_counts,
        give_some,
        give_none,
        steal_some,
        steal_none,
        keyed: keyed_count,
        cross_partition,
        outcomes,
        metrics,
        events,
        phase4_intent_timing,
        phase4_hook_timed_wall,
        phase4_hook_plain_wall,
        phase6b_timing,
        phase6b_hook_timed_wall,
        phase6b_hook_plain_wall,
        phase_times_us,
    }
}

fn keyed_stream_from_partitions(
    partitions: &[SettlementIntentPartition],
    day: SimulationDay,
    context: M0RunContext,
) -> Vec<KeyedEntry> {
    let mut keyed = Vec::new();
    for partition in partitions {
        for intent in &partition.intents {
            let (initiator, target, action) = match *intent {
                Intent::GiveFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => (agent_id, target, TargetedActionKind::GiveFood),
                Intent::StealFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => (agent_id, target, TargetedActionKind::StealFood),
                _ => continue,
            };
            keyed.push(KeyedEntry {
                key: compute_m3_resolution_key(
                    context.master_seed,
                    context.replicate_id,
                    day.as_u32(),
                    target.as_u32(),
                    initiator.as_u32(),
                    action.action_kind(),
                ),
                target,
                initiator,
                action,
            });
        }
    }
    keyed.sort_by(|a, b| {
        compare_keyed_interactions(
            a.key,
            a.target,
            a.initiator,
            a.action,
            b.key,
            b.target,
            b.initiator,
            b.action,
        )
    });
    keyed
}

fn conflict_stats(entries: &[KeyedEntry]) -> ConflictStats {
    if entries.is_empty() {
        return ConflictStats {
            fraction: None,
            mean_contention: None,
        };
    }
    let mut touches = HashMap::<AgentId, u64>::with_capacity(entries.len() * 2);
    for entry in entries {
        *touches.entry(entry.initiator).or_default() += 1;
        *touches.entry(entry.target).or_default() += 1;
    }
    let conflicting = entries
        .iter()
        .filter(|entry| touches[&entry.initiator] > 1 || touches[&entry.target] > 1)
        .count();
    let total_touches: u64 = touches.values().sum();
    ConflictStats {
        fraction: Some(conflicting as f64 / entries.len() as f64),
        mean_contention: Some(total_touches as f64 / touches.len() as f64),
    }
}

fn summarize(values: &mut [f64]) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let mut deviations: Vec<_> = values.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    (median, deviations[deviations.len() / 2])
}

fn elapsed_us(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1_000.0
}

const STAGE_NAMES: [&str; 5] = [
    "validation_slot_lookup",
    "key_generation",
    "global_sort",
    "dependency_level_build",
    "evaluation_writeback_output",
];

#[derive(Debug, Clone, PartialEq)]
struct TickDigests {
    state: CanonicalHash,
    daily_metrics: Vec<M3DailyMetrics>,
    daily_events: Vec<Vec<sim_model::EventRecord>>,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

struct TimedSample {
    tick_us: f64,
    stages_us: [f64; 5],
    digests: TickDigests,
}

fn stage_values(timings: &[M3Phase6BStageTimings]) -> [f64; 5] {
    let divisor = timings.len() as f64 * 1_000.0;
    let mut totals_ns = [0u128; 5];
    for timing in timings {
        totals_ns[0] += timing.validation_slot_lookup.as_nanos();
        totals_ns[1] += timing.key_generation.as_nanos();
        totals_ns[2] += timing.global_sort.as_nanos();
        totals_ns[3] += timing.dependency_level_build.as_nanos();
        totals_ns[4] += timing.evaluation_writeback_output.as_nanos();
    }
    totals_ns.map(|nanos| nanos as f64 / divisor)
}

fn tick_digests(
    hybrid: &HybridWorldState,
    graph: &RelationshipGraph,
    outcomes: &[sim_model::M3DayOutcome],
) -> TickDigests {
    let daily_metrics: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
        .collect();
    let daily_events: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.events.clone())
        .collect();
    let all_events: Vec<_> = daily_events.iter().flatten().cloned().collect();
    let storage = hybrid.storage().expect("hybrid storage exists");
    TickDigests {
        state: canonical_m3_state_hash_from_storage(
            storage,
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
        )
        .expect("M3 state digest succeeds"),
        metrics: canonical_m3_metrics_hash(&daily_metrics).expect("M3 metrics digest succeeds"),
        events: canonical_event_hash(&all_events).expect("event digest succeeds"),
        daily_metrics,
        daily_events,
    }
}

fn timed_serial_sample(base: &HybridWorldState, config: &SimConfig) -> TimedSample {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context =
        M3StorageRunContext::new(&hybrid, config, &run_context).expect("serial context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let started = Instant::now();
    let (outcomes, timings) = run_m3_storage_days_with_phase6b_timing(
        &mut hybrid,
        config,
        &mut context,
        DAYS_PER_SAMPLE,
        &options,
    )
    .expect("timed serial M3 run succeeds");
    let tick_us = elapsed_us(started.elapsed()) / DAYS_PER_SAMPLE as f64;
    let stages_us = stage_values(&timings);
    let digests = tick_digests(&hybrid, context.relationship_graph(), &outcomes);
    TimedSample {
        tick_us,
        stages_us,
        digests,
    }
}

fn timed_parallel_sample(
    base: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    chunk_size: usize,
) -> TimedSample {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context = M3ParallelStorageRunContext::new(&hybrid, config, &run_context)
        .expect("parallel context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let started = Instant::now();
    let (outcomes, timings) = run_m3_storage_days_with_parallel_phase6b_timing(
        &mut hybrid,
        config,
        &mut context,
        DAYS_PER_SAMPLE,
        &options,
        pool,
        NonZeroUsize::new(chunk_size).unwrap(),
    )
    .expect("timed parallel M3 run succeeds");
    let tick_us = elapsed_us(started.elapsed()) / DAYS_PER_SAMPLE as f64;
    let stages_us = stage_values(&timings);
    let digests = tick_digests(&hybrid, context.relationship_graph(), &outcomes);
    TimedSample {
        tick_us,
        stages_us,
        digests,
    }
}

fn untimed_serial_sample(base: &HybridWorldState, config: &SimConfig) -> TimedSample {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context =
        M3StorageRunContext::new(&hybrid, config, &run_context).expect("serial context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let started = Instant::now();
    let outcomes =
        run_m3_storage_days(&mut hybrid, config, &mut context, DAYS_PER_SAMPLE, &options)
            .expect("untimed serial M3 run succeeds");
    let tick_us = elapsed_us(started.elapsed()) / DAYS_PER_SAMPLE as f64;
    TimedSample {
        tick_us,
        stages_us: [0.0; 5],
        digests: tick_digests(&hybrid, context.relationship_graph(), &outcomes),
    }
}

fn untimed_parallel_sample(
    base: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    chunk_size: usize,
) -> TimedSample {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context = M3ParallelStorageRunContext::new(&hybrid, config, &run_context)
        .expect("parallel context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let started = Instant::now();
    let outcomes = run_m3_storage_days_with_parallel_phase6b(
        &mut hybrid,
        config,
        &mut context,
        DAYS_PER_SAMPLE,
        &options,
        pool,
        NonZeroUsize::new(chunk_size).unwrap(),
    )
    .expect("untimed parallel M3 run succeeds");
    let tick_us = elapsed_us(started.elapsed()) / DAYS_PER_SAMPLE as f64;
    TimedSample {
        tick_us,
        stages_us: [0.0; 5],
        digests: tick_digests(&hybrid, context.relationship_graph(), &outcomes),
    }
}

fn measure_pair(
    horizon: u32,
    cell: Cell,
    config: &SimConfig,
    base: &HybridWorldState,
    threads: usize,
    chunk_size: usize,
    pool: &rayon::ThreadPool,
) {
    println!(
        "START|horizon={horizon}|N={}|K={}|p={:.2}|threads={threads}|chunk={chunk_size}",
        cell.population, cell.settlements, cell.p_local
    );
    let mut serial_ticks = Vec::with_capacity(SAMPLES);
    let mut parallel_ticks = Vec::with_capacity(SAMPLES);
    let mut serial_stages: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    let mut parallel_stages: [Vec<f64>; 5] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));

    for sample in 0..(WARMUPS + SAMPLES) {
        // Each sample starts from an identical checkpoint; the serial and parallel runs are interleaved.
        let serial_hooks = timed_serial_sample(base, config);
        let parallel_hooks = timed_parallel_sample(base, config, pool, chunk_size);
        let serial = untimed_serial_sample(base, config);
        let parallel = untimed_parallel_sample(base, config, pool, chunk_size);
        assert_eq!(serial_hooks.digests, serial.digests);
        assert_eq!(parallel_hooks.digests, parallel.digests);
        assert_eq!(parallel.digests, serial.digests);
        if sample >= WARMUPS {
            serial_ticks.push(serial.tick_us);
            parallel_ticks.push(parallel.tick_us);
            for stage in 0..5 {
                serial_stages[stage].push(serial_hooks.stages_us[stage]);
                parallel_stages[stage].push(parallel_hooks.stages_us[stage]);
            }
        }
    }

    let (serial_median, serial_mad) = summarize(&mut serial_ticks);
    let (parallel_median, parallel_mad) = summarize(&mut parallel_ticks);
    println!(
        "TICK|{horizon}|{}|{}|{:.2}|{threads}|{chunk_size}|{serial_median:.3}|{serial_mad:.3}|{parallel_median:.3}|{parallel_mad:.3}|{:.5}",
        cell.population,
        cell.settlements,
        cell.p_local,
        serial_median / parallel_median
    );
    for stage in 0..5 {
        let (serial_stage, serial_stage_mad) = summarize(&mut serial_stages[stage]);
        let (parallel_stage, parallel_stage_mad) = summarize(&mut parallel_stages[stage]);
        let speedup = if serial_stage > 0.0 && parallel_stage > 0.0 {
            format!("{:.5}", serial_stage / parallel_stage)
        } else {
            "NA".to_string()
        };
        println!(
            "STAGE|{horizon}|{}|{}|{:.2}|{threads}|{chunk_size}|{}|{serial_stage:.3}|{serial_stage_mad:.3}|{parallel_stage:.3}|{parallel_stage_mad:.3}|{speedup}",
            cell.population, cell.settlements, cell.p_local, STAGE_NAMES[stage]
        );
    }
}

struct WorkloadSummary {
    alive: usize,
    keyed_by_day: [u64; 3],
    cross_partition_mean: f64,
    conflict_fraction: Option<f64>,
    mean_contention: Option<f64>,
    level_count_mean: f64,
    max_level_size_mean: f64,
    max_level_size: usize,
}

fn measure_workload(
    config: &SimConfig,
    base: &HybridWorldState,
    pool: &rayon::ThreadPool,
) -> WorkloadSummary {
    let storage = base.storage().expect("hybrid storage exists");
    let alive = storage.alive().iter().filter(|&&is_alive| is_alive).count();
    let mut run = ManualRun::from_hybrid_world(config, base.clone());
    let chunk = NonZeroUsize::new(256).unwrap();
    let mut keyed_by_day = [0; 3];
    let mut cross_partition_total = 0u64;
    let mut conflict_sum = 0.0;
    let mut conflict_days = 0u32;
    let mut contention_sum = 0.0;
    let mut contention_days = 0u32;
    let mut level_count_sum = 0u64;
    let mut max_level_size_sum = 0u64;
    let mut max_level_size = 0usize;
    for (day, keyed_day) in keyed_by_day.iter_mut().enumerate() {
        let (volume, _, level_stats) = run.day_parallel_timed(config, pool, chunk);
        *keyed_day = volume.keyed;
        cross_partition_total += volume.cross_partition;
        conflict_sum += volume.conflict_fraction_sum;
        conflict_days += volume.conflict_fraction_days;
        contention_sum += volume.contention_sum;
        contention_days += volume.contention_days;
        level_count_sum += level_stats.non_empty_level_count as u64;
        max_level_size_sum += level_stats.max_level_size as u64;
        max_level_size = max_level_size.max(level_stats.max_level_size);
        let _ = day;
    }
    let mean = |sum: u64| sum as f64 / DAYS_PER_SAMPLE as f64;
    WorkloadSummary {
        alive,
        keyed_by_day,
        cross_partition_mean: mean(cross_partition_total),
        conflict_fraction: (conflict_days > 0).then_some(conflict_sum / conflict_days as f64),
        mean_contention: (contention_days > 0).then_some(contention_sum / contention_days as f64),
        level_count_mean: mean(level_count_sum),
        max_level_size_mean: mean(max_level_size_sum),
        max_level_size,
    }
}

fn build_horizon_state(config: &SimConfig, horizon: u32) -> HybridWorldState {
    let mut hybrid = HybridWorldState::hybrid(initialize_world(config).expect("world initializes"));
    if horizon > 0 {
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let mut context = M3StorageRunContext::new(&hybrid, config, &run_context)
            .expect("day-100 context builds");
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        for _ in 0..horizon {
            let _ = run_m3_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .expect("serial run to day 100 succeeds");
        }
    }
    hybrid
}

fn search_counts(profiles: &[InteractionDayProfile]) -> SearchCounts {
    let mut counts = SearchCounts::default();
    for profile in profiles {
        for (total, count) in counts.actions.iter_mut().zip(profile.action_counts) {
            *total += count;
        }
        counts.give_some += profile.give_some;
        counts.give_none += profile.give_none;
        counts.steal_some += profile.steal_some;
        counts.steal_none += profile.steal_none;
        counts.keyed += profile.keyed;
        counts.cross_partition += profile.cross_partition;
        counts.outcomes.zero_target += profile.outcomes.zero_target;
        counts.outcomes.initiator_ineligible += profile.outcomes.initiator_ineligible;
        counts.outcomes.target_ineligible += profile.outcomes.target_ineligible;
        counts.outcomes.theft_failed += profile.outcomes.theft_failed;
        counts.outcomes.applied_positive += profile.outcomes.applied_positive;
        counts.outcomes.applied_zero += profile.outcomes.applied_zero;
    }
    counts
}

fn signed_duration_us(left: Duration, right: Duration) -> f64 {
    (left.as_nanos() as f64 - right.as_nanos() as f64) / 1_000.0
}

fn summarize_search_sample(
    profiles: &[InteractionDayProfile],
    counts: SearchCounts,
) -> SearchSample {
    let days = profiles.len() as f64;
    let total_ns = |select: fn(&InteractionDayProfile) -> Duration| {
        profiles
            .iter()
            .map(select)
            .map(|duration| duration.as_nanos())
            .sum::<u128>()
    };
    let us_per_tick = |nanos: u128| nanos as f64 / 1_000.0 / days;
    let p4_candidate_ns = profiles
        .iter()
        .map(|profile| {
            profile
                .phase4_intent_timing
                .candidate_filter_and_draw
                .as_nanos()
        })
        .sum::<u128>();
    let p4_plain_wall_ns = profiles
        .iter()
        .map(|profile| profile.phase4_hook_plain_wall.as_nanos())
        .sum::<u128>();
    let p4_hook_delta_us = profiles
        .iter()
        .map(|profile| {
            signed_duration_us(
                profile.phase4_hook_timed_wall,
                profile.phase4_hook_plain_wall,
            )
        })
        .sum::<f64>()
        / days;
    let p6_plain_wall_ns = profiles
        .iter()
        .map(|profile| profile.phase6b_hook_plain_wall.as_nanos())
        .sum::<u128>();
    let p6_hook_delta_us = profiles
        .iter()
        .map(|profile| {
            signed_duration_us(
                profile.phase6b_hook_timed_wall,
                profile.phase6b_hook_plain_wall,
            )
        })
        .sum::<f64>()
        / days;
    let phase4_target_us = us_per_tick(p4_candidate_ns);
    let phase6b_validation_us = us_per_tick(total_ns(|p| p.phase6b_timing.validation_slot_lookup));
    let phase6b_key_us = us_per_tick(total_ns(|p| p.phase6b_timing.key_generation));
    let phase6b_sort_us = us_per_tick(total_ns(|p| p.phase6b_timing.global_sort));
    let applied_us = us_per_tick(total_ns(|p| p.phase6b_timing.applied_positive_entries));
    let search_setup_us =
        phase4_target_us + phase6b_validation_us + phase6b_key_us + phase6b_sort_us;
    let applied_per_day = counts.outcomes.applied_positive as f64 / days;
    let phase_times_us = std::array::from_fn(|index| {
        profiles
            .iter()
            .map(|profile| profile.phase_times_us[index])
            .sum::<f64>()
            / days
    });
    let p4_hook_overhead_us = p4_hook_delta_us;
    let p6_hook_overhead_us = p6_hook_delta_us;
    SearchSample {
        phase4_hook_overhead_us: p4_hook_overhead_us,
        phase4_hook_overhead_percent: p4_hook_delta_us / (p4_plain_wall_ns as f64 / 1_000.0 / days)
            * 100.0,
        search_setup_us,
        work_us: applied_us,
        search_per_applied_us: if applied_per_day > 0.0 {
            search_setup_us / applied_per_day
        } else {
            f64::NAN
        },
        work_per_applied_us: if applied_per_day > 0.0 {
            applied_us / applied_per_day
        } else {
            f64::NAN
        },
        search_to_work_ratio: if applied_us > 0.0 {
            search_setup_us / applied_us
        } else {
            f64::NAN
        },
        phase6b_hook_overhead_us: p6_hook_overhead_us,
        phase6b_hook_overhead_percent: p6_hook_delta_us
            / (p6_plain_wall_ns as f64 / 1_000.0 / days)
            * 100.0,
        phase_times_us,
    }
}

fn search_stat(samples: &[SearchSample], select: fn(&SearchSample) -> f64) -> (f64, f64) {
    let mut values: Vec<_> = samples.iter().map(select).collect();
    if values.iter().all(|value| !value.is_finite()) {
        return (f64::NAN, f64::NAN);
    }
    summarize(&mut values)
}

const SUSTAINED_PHASE_NAMES: [&str; 17] = [
    "phase1_regrowth",
    "phase2_degradation",
    "phase3_features",
    "phase4_selection",
    "phase4_intent_projection_hook_wall",
    "phase5_partition",
    "phase6a_work",
    "phase6b_validation_slot",
    "phase6b_key_generation",
    "phase6b_sort",
    "phase6b_commit_output",
    "phase7_market",
    "phase8_welfare",
    "phase9_mortality",
    "event_staging",
    "phase10_metrics",
    "phase11_event_flush",
];

fn profile_sustained_cell(cell: Cell, horizon: u32, config: &SimConfig, base: &HybridWorldState) {
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut samples = Vec::with_capacity(SAMPLES);
    let mut expected_trace: Option<TickDigests> = None;
    let mut expected_counts: Option<SearchCounts> = None;
    for sample in 0..(WARMUPS + SAMPLES) {
        let mut measured = ManualRun::from_hybrid_world(config, base.clone());
        let mut oracle = base.clone();
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let mut oracle_context = M3StorageRunContext::new(&oracle, config, &run_context)
            .expect("sustained profile oracle context builds");
        let mut profiles = Vec::with_capacity(DAYS_PER_SAMPLE as usize);
        let mut oracle_outcomes = Vec::with_capacity(DAYS_PER_SAMPLE as usize);
        for day_index in 0..DAYS_PER_SAMPLE {
            let profile = advance_interaction_profile(
                &mut measured,
                config,
                true,
                (sample + day_index as usize) % 2 == 1,
            );
            if sample == 0 {
                println!(
                    "SUSTAINED_DAY|horizon={horizon}|N={}|K={}|p={:.1}|day={}|alive={}|mean_alive_food={}|work={}|buy={}|sell={}|give={}|steal={}|idle={}|give_some={}|give_none={}|steal_some={}|steal_none={}|keyed={}|cross={}|applied_positive={}|applied_zero={}|zero_target={}|initiator_ineligible={}|target_ineligible={}|theft_failed={}",
                    cell.population,
                    cell.settlements,
                    cell.p_local,
                    profile.day,
                    profile.alive,
                    profile
                        .mean_alive_food
                        .map_or_else(|| "NA".to_string(), |food| format!("{food:.4}")),
                    profile.action_counts[Action::Work.index()],
                    profile.action_counts[Action::BuyFood.index()],
                    profile.action_counts[Action::SellFood.index()],
                    profile.action_counts[Action::GiveFood.index()],
                    profile.action_counts[Action::StealFood.index()],
                    profile.action_counts[Action::Idle.index()],
                    profile.give_some,
                    profile.give_none,
                    profile.steal_some,
                    profile.steal_none,
                    profile.keyed,
                    profile.cross_partition,
                    profile.outcomes.applied_positive,
                    profile.outcomes.applied_zero,
                    profile.outcomes.zero_target,
                    profile.outcomes.initiator_ineligible,
                    profile.outcomes.target_ineligible,
                    profile.outcomes.theft_failed,
                );
            }
            let mut day_outcomes =
                run_m3_storage_days(&mut oracle, config, &mut oracle_context, 1, &options)
                    .expect("sustained profile oracle day succeeds");
            let outcome = day_outcomes.pop().expect("one sustained oracle outcome");
            assert_eq!(Some(&profile.metrics), outcome.metrics.as_ref());
            assert_eq!(profile.events, outcome.events);
            let measured_hash = canonical_m3_state_hash_from_storage(
                &measured.storage,
                measured.world.current_day,
                &measured.world.settlements,
                &measured.relationship_graph,
            )
            .expect("profile state hash succeeds");
            let oracle_hash = canonical_m3_state_hash_from_storage(
                oracle.storage().expect("oracle storage exists"),
                oracle.world.current_day,
                &oracle.world.settlements,
                oracle_context.relationship_graph(),
            )
            .expect("oracle state hash succeeds");
            assert_eq!(measured_hash, oracle_hash);
            profiles.push(profile);
            oracle_outcomes.push(outcome);
        }
        let trace = tick_digests(
            &oracle,
            oracle_context.relationship_graph(),
            &oracle_outcomes,
        );
        if let Some(expected) = &expected_trace {
            assert_eq!(&trace, expected);
        } else {
            expected_trace = Some(trace);
        }
        let counts = search_counts(&profiles);
        if let Some(expected) = expected_counts {
            assert_eq!(counts, expected);
        } else {
            expected_counts = Some(counts);
        }
        if sample >= WARMUPS {
            samples.push(summarize_search_sample(&profiles, counts));
        }
    }

    let counts = expected_counts.expect("sustained profile counts recorded");
    println!(
        "SUSTAINED_COUNTS|horizon={horizon}|N={}|K={}|p={:.1}|work={}|buy={}|sell={}|give={}|steal={}|idle={}|give_some={}|give_none={}|steal_some={}|steal_none={}|keyed={}|cross={}|applied_positive={}|zero_target={}|initiator_ineligible={}|target_ineligible={}|theft_failed={}|applied_zero={}",
        cell.population,
        cell.settlements,
        cell.p_local,
        counts.actions[Action::Work.index()],
        counts.actions[Action::BuyFood.index()],
        counts.actions[Action::SellFood.index()],
        counts.actions[Action::GiveFood.index()],
        counts.actions[Action::StealFood.index()],
        counts.actions[Action::Idle.index()],
        counts.give_some,
        counts.give_none,
        counts.steal_some,
        counts.steal_none,
        counts.keyed,
        counts.cross_partition,
        counts.outcomes.applied_positive,
        counts.outcomes.zero_target,
        counts.outcomes.initiator_ineligible,
        counts.outcomes.target_ineligible,
        counts.outcomes.theft_failed,
        counts.outcomes.applied_zero,
    );
    type SearchMetric = (&'static str, fn(&SearchSample) -> f64);
    let metric_names: [SearchMetric; 9] = [
        ("search_setup_us_tick", |s: &SearchSample| s.search_setup_us),
        ("applied_work_us_tick", |s: &SearchSample| s.work_us),
        ("search_to_work_ratio", |s: &SearchSample| {
            s.search_to_work_ratio
        }),
        ("search_per_applied_us", |s: &SearchSample| {
            s.search_per_applied_us
        }),
        ("work_per_applied_us", |s: &SearchSample| {
            s.work_per_applied_us
        }),
        ("phase4_hook_overhead_us", |s: &SearchSample| {
            s.phase4_hook_overhead_us
        }),
        ("phase4_hook_overhead_pct", |s: &SearchSample| {
            s.phase4_hook_overhead_percent
        }),
        ("phase6b_hook_overhead_us", |s: &SearchSample| {
            s.phase6b_hook_overhead_us
        }),
        ("phase6b_hook_overhead_pct", |s: &SearchSample| {
            s.phase6b_hook_overhead_percent
        }),
    ];
    for (name, select) in metric_names {
        let (median, mad) = search_stat(&samples, select);
        println!(
            "SUSTAINED_SEARCH|horizon={horizon}|N={}|K={}|p={:.1}|{name}|{median:.3}|{mad:.3}",
            cell.population, cell.settlements, cell.p_local
        );
    }
    for (index, name) in SUSTAINED_PHASE_NAMES.iter().enumerate() {
        let mut values: Vec<_> = samples
            .iter()
            .map(|sample| sample.phase_times_us[index])
            .collect();
        let (median, mad) = summarize(&mut values);
        println!(
            "SUSTAINED_PHASE|horizon={horizon}|N={}|K={}|p={:.1}|{name}|{median:.3}|{mad:.3}",
            cell.population, cell.settlements, cell.p_local
        );
    }
}

fn run_sustained_grid() {
    let mut candidates = Vec::<(String, FoodConfig)>::new();
    candidates.push(("M2-40 unchanged".to_string(), M2_40_FOOD));
    for value in [
        0.75, 0.5, 0.49, 0.48, 0.47, 0.46, 0.455, 0.451, 0.45, 0.4, 0.35, 0.3, 0.275, 0.25,
    ] {
        candidates.push((
            format!("base_metabolic_cost={value}"),
            FoodConfig {
                base_metabolic_cost: value,
                ..M2_40_FOOD
            },
        ));
    }
    for value in [0.025, 0.01, 0.0075, 0.005] {
        candidates.push((
            format!("health_decay_rate={value}"),
            FoodConfig {
                health_decay_rate: value,
                ..M2_40_FOOD
            },
        ));
    }
    candidates.extend([
        (
            "base_work_yield=3.0".to_string(),
            FoodConfig {
                base_work_yield: 3.0,
                ..M2_40_FOOD
            },
        ),
        (
            "base_work_yield=4.0".to_string(),
            FoodConfig {
                base_work_yield: 4.0,
                ..M2_40_FOOD
            },
        ),
        (
            "initial_settlement_resource=4000.0".to_string(),
            FoodConfig {
                initial_settlement_resource: 4_000.0,
                ..M2_40_FOOD
            },
        ),
        (
            "regrowth_rate=0.2".to_string(),
            FoodConfig {
                regrowth_rate: 0.2,
                ..M2_40_FOOD
            },
        ),
        (
            "carrying_capacity=20000.0".to_string(),
            FoodConfig {
                carrying_capacity: 20_000.0,
                ..M2_40_FOOD
            },
        ),
    ]);

    println!("GRID|N=10000|K=50|k=8|p_local=0.5|seed={MASTER_SEED}|replicate=7|days=200");
    println!(
        "GRID_HEADER|candidate|alive_min_1_200|alive_day200|keyed_mean_100_200|alive_mean_100_200|keyed_zero_days_100_200|first_keyed_zero_day|give_some_days_100_200|accepted"
    );
    for (name, food) in candidates {
        let cell = Cell {
            population: 10_000,
            settlements: 50,
            degree: 8,
            p_local: 0.5,
        };
        let config = config_for_food(cell, MASTER_SEED, food);
        let base = HybridWorldState::hybrid(initialize_world(&config).expect("world initializes"));
        let mut run = ManualRun::from_hybrid_world(&config, base.clone());
        let mut oracle = base;
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut oracle_context = M3StorageRunContext::new(&oracle, &config, &run_context)
            .expect("grid production context builds");
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let mut alive_min = usize::MAX;
        let mut keyed_total = 0u64;
        let mut alive_total = 0u64;
        let mut range_days = 0u64;
        let mut keyed_zero = 0u32;
        let mut first_zero = None;
        let mut give_some_days = 0u32;
        let mut accepted = true;
        for day in 1..=200 {
            let profile = advance_interaction_profile(&mut run, &config, false, false);
            let mut outcomes =
                run_m3_storage_days(&mut oracle, &config, &mut oracle_context, 1, &options)
                    .expect("grid production day succeeds");
            let outcome = outcomes.pop().expect("one grid production outcome");
            assert_profile_matches_runner(&profile, &run, &oracle, &oracle_context, &outcome);
            alive_min = alive_min.min(profile.alive);
            if profile.alive < 5_000 {
                accepted = false;
            }
            if profile.keyed == 0 {
                first_zero.get_or_insert(day);
                if (100..=200).contains(&day) {
                    keyed_zero += 1;
                }
            }
            if (100..=200).contains(&day) {
                keyed_total += profile.keyed;
                alive_total += profile.alive as u64;
                range_days += 1;
                give_some_days += u32::from(profile.give_some > 0);
            }
        }
        let keyed_mean = keyed_total as f64 / range_days as f64;
        let alive_mean = alive_total as f64 / range_days as f64;
        if keyed_mean < 0.1 * alive_mean || keyed_zero != 0 || give_some_days == 0 {
            accepted = false;
        }
        let alive_final = run.storage.alive().iter().filter(|&&alive| alive).count();
        println!(
            "GRID_ROW|{name}|{alive_min}|{alive_final}|{keyed_mean:.2}|{alive_mean:.2}|{keyed_zero}|{}|{give_some_days}|{accepted}",
            first_zero.map_or_else(|| "none".to_string(), |day| day.to_string())
        );
    }
}

fn run_sustained_stability() {
    println!("STABILITY|N=100000|K=500|k=8|seed={MASTER_SEED}|replicate=7|days=200");
    println!(
        "STABILITY_HEADER|p_local|alive_min_1_200|alive_day200|keyed_mean_100_200|alive_mean_100_200|keyed_zero_days_100_200|give_some_days_100_200|accepted"
    );
    for p_local in [0.0, 0.5, 1.0] {
        let cell = Cell {
            population: 100_000,
            settlements: 500,
            degree: 8,
            p_local,
        };
        let config = config_for(cell);
        let base = HybridWorldState::hybrid(initialize_world(&config).expect("world initializes"));
        let mut run = ManualRun::from_hybrid_world(&config, base.clone());
        let mut oracle = base;
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut oracle_context = M3StorageRunContext::new(&oracle, &config, &run_context)
            .expect("stability production context builds");
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let mut alive_min = usize::MAX;
        let mut keyed_total = 0u64;
        let mut alive_total = 0u64;
        let mut late_days = 0u64;
        let mut keyed_zero = 0u32;
        let mut give_some_days = 0u32;
        let mut accepted = true;
        for day in 1..=200 {
            let profile = advance_interaction_profile(&mut run, &config, false, false);
            let mut outcomes =
                run_m3_storage_days(&mut oracle, &config, &mut oracle_context, 1, &options)
                    .expect("stability production day succeeds");
            let outcome = outcomes.pop().expect("one stability production outcome");
            assert_profile_matches_runner(&profile, &run, &oracle, &oracle_context, &outcome);
            alive_min = alive_min.min(profile.alive);
            if profile.alive < 50_000 {
                accepted = false;
            }
            if (100..=200).contains(&day) {
                keyed_total += profile.keyed;
                alive_total += profile.alive as u64;
                late_days += 1;
                keyed_zero += u32::from(profile.keyed == 0);
                give_some_days += u32::from(profile.give_some > 0);
            }
        }
        let keyed_mean = keyed_total as f64 / late_days as f64;
        let alive_mean = alive_total as f64 / late_days as f64;
        if keyed_zero > 0 || keyed_mean < 0.1 * alive_mean || give_some_days == 0 {
            accepted = false;
        }
        let alive_final = run.storage.alive().iter().filter(|&&alive| alive).count();
        println!(
            "STABILITY_ROW|{p_local:.1}|{alive_min}|{alive_final}|{keyed_mean:.2}|{alive_mean:.2}|{keyed_zero}|{give_some_days}|{accepted}"
        );
    }
}

fn run_sustained_measurements() {
    let started = Instant::now();
    print_host_info();
    println!(
        "SUSTAINED_CONFIG|base_metabolic_cost=0.48|other M2-40 food-supply knobs unchanged|N=10000,100000|K=N/200|k=8|p_local=0,0.5,1|horizons=0,150"
    );
    println!(
        "SUSTAINED_METHOD|whole_tick=untimed production runner|phase_splits=opt-in hooks + manual M3-01 phase driver|warmups=2|samples=11|days_per_sample=3|serial/parallel paired interleaved|pools=1,4,6|chunk=256|metrics=on|events=on|snapshots=off"
    );
    let pools: Vec<_> = [1, 4, 6]
        .into_iter()
        .map(|threads| {
            (
                threads,
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("fixed benchmark pool builds"),
            )
        })
        .collect();

    for (population, settlements) in [(10_000, 50), (100_000, 500)] {
        for p_local in [0.0, 0.5, 1.0] {
            let cell = Cell {
                population,
                settlements,
                degree: 8,
                p_local,
            };
            let config = config_for(cell);
            for horizon in [0, 150] {
                println!(
                    "SUSTAINED_CELL|horizon={horizon}|N={population}|K={settlements}|p={p_local:.1}"
                );
                let base = build_horizon_state(&config, horizon);
                let volume = measure_workload(&config, &base, &pools[0].1);
                println!(
                    "SUSTAINED_WORKLOAD|horizon={horizon}|N={population}|K={settlements}|p={p_local:.1}|alive_at_checkpoint={}|keyed_by_day={},{},{}|cross_partition_mean={:.2}|conflict_fraction={}|mean_contention={}|levels_per_day={:.2}|max_level_size_mean={:.2}|max_level_size={}",
                    volume.alive,
                    volume.keyed_by_day[0],
                    volume.keyed_by_day[1],
                    volume.keyed_by_day[2],
                    volume.cross_partition_mean,
                    volume
                        .conflict_fraction
                        .map_or_else(|| "NA".to_string(), |value| format!("{value:.6}")),
                    volume
                        .mean_contention
                        .map_or_else(|| "NA".to_string(), |value| format!("{value:.6}")),
                    volume.level_count_mean,
                    volume.max_level_size_mean,
                    volume.max_level_size
                );
                profile_sustained_cell(cell, horizon, &config, &base);
                for (threads, pool) in &pools {
                    measure_pair(horizon, cell, &config, &base, *threads, 256, pool);
                }
            }
        }
    }
    println!(
        "SUSTAINED_BENCH_SECONDS|{:.3}",
        started.elapsed().as_secs_f64()
    );
}

fn print_host_info() {
    let logical = std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1);
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    println!(
        "HOST|os={}|arch={}|logical_processors={logical}|processor_identifier={}|rustc={rustc}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "record-separately".into())
    );
}

fn main() {
    if std::env::args().any(|arg| arg == "--sustained-grid") {
        run_sustained_grid();
        return;
    }
    if std::env::args().any(|arg| arg == "--sustained-stability") {
        run_sustained_stability();
        return;
    }
    if std::env::args().any(|arg| arg == "--sustained-measure") {
        run_sustained_measurements();
        return;
    }
    let total_started = Instant::now();
    run_sustained_grid();
    run_sustained_stability();
    run_sustained_measurements();
    println!(
        "BENCH_TOTAL_SECONDS|{:.3}",
        total_started.elapsed().as_secs_f64()
    );
}
