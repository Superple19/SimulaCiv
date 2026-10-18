#![allow(clippy::disallowed_methods)]

use sim_core::{AgentId, SimulationDay};
use sim_model::decision::{
    Action, PrimaryActionChoice, phase4_primary_action_selection_storage_into,
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
const BENCH_CHUNKS: [usize; 5] = [64, 128, 256, 512, 1024];

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
    phase4_intent_timing: sim_model::M3Phase4IntentTimings,
    phase4_hook_timed_wall: Duration,
    phase4_hook_plain_wall: Duration,
    phase6b_timing: M3Phase6BStageTimings,
    phase6b_hook_timed_wall: Duration,
    phase6b_hook_plain_wall: Duration,
}

#[derive(Default, Clone, Copy)]
struct SearchSample {
    phase4_target_us: f64,
    phase4_other_us: f64,
    phase4_hook_overhead_us: f64,
    phase4_hook_overhead_percent: f64,
    phase6b_validation_us: f64,
    phase6b_key_us: f64,
    phase6b_sort_us: f64,
    applied_us: f64,
    other_keyed_us: f64,
    output_us: f64,
    search_setup_us: f64,
    work_us: f64,
    search_per_applied_us: f64,
    work_per_applied_us: f64,
    search_to_work_ratio: f64,
    phase6b_hook_overhead_us: f64,
    phase6b_hook_overhead_percent: f64,
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
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 config parses");
    config.profile = SimulationProfile::M3;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.world.master_seed = seed;
    config.world.replicate_id = REPLICATE_ID;
    config.interaction.relationship_degree_k = Some(cell.degree as i64);
    config.interaction.p_local = Some(cell.p_local);
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

    phase1_resource_regrowth(&mut run.world, &effective);
    run.storage.phase2_degradation_with_config(&effective);
    run.features.clear();
    phase3_observation_and_features_storage_with_scratch(
        &run.storage,
        &run.world.settlements,
        &effective,
        &mut run.features,
        &mut run.phase3,
    )
    .expect("phase 3 succeeds");
    run.choices.clear();
    phase4_primary_action_selection_storage_into(
        &run.storage,
        current_day,
        &effective,
        &run.features,
        &mut run.choices,
    )
    .expect("phase 4 action selection succeeds");

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
                Duration::ZERO,
                Duration::ZERO,
            )
        };

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
    let partitions =
        phase5_partition_intents_from_vec_with_scratch(&mut run.intents, &mut run.phase5)
            .expect("phase 5 succeeds");
    let keyed = keyed_stream_from_partitions(&partitions, current_day, run.run_context);
    let keyed_count = keyed.len() as u64;
    phase6a_work_resolution_storage(&mut run.storage, &mut run.world.settlements, &partitions)
        .expect("phase 6A succeeds");

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

    phase7_market_clearance_storage_with_config(
        &mut run.storage,
        &mut run.world.settlements,
        &partitions,
        &effective.economy,
    )
    .expect("phase 7 succeeds");
    phase8_welfare_distribution_storage_with_config_and_scratch(
        &mut run.storage,
        &mut run.world.settlements,
        &effective,
        &mut run.phase8,
    )
    .expect("phase 8 succeeds");
    phase9_mortality_commitment_storage(&mut run.storage).expect("phase 9 succeeds");

    let base_metrics = run
        .storage
        .phase10_metrics(&run.world.settlements, day)
        .expect("phase 10 succeeds");
    let (cross_partition_edge_count, total_directed_edge_count) =
        run.relationship_graph.edge_ratio_counts();
    let metrics = M3DailyMetrics {
        metrics: base_metrics,
        cross_partition_edge_count,
        total_directed_edge_count,
        cross_partition_interaction_count: cross_partition,
    };
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
        phase4_intent_timing,
        phase4_hook_timed_wall,
        phase4_hook_plain_wall,
        phase6b_timing,
        phase6b_hook_timed_wall,
        phase6b_hook_plain_wall,
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

fn make_cells() -> Vec<Cell> {
    let mut cells = Vec::with_capacity(9);
    for (population, settlements) in [(10_000, 50), (50_000, 250), (100_000, 500)] {
        for p_local in [0.0, 0.5, 1.0] {
            cells.push(Cell {
                population,
                settlements,
                degree: 8,
                p_local,
            });
        }
    }
    cells
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

fn untimed_serial_digests(base: &HybridWorldState, config: &SimConfig) -> TickDigests {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context =
        M3StorageRunContext::new(&hybrid, config, &run_context).expect("serial context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let outcomes =
        run_m3_storage_days(&mut hybrid, config, &mut context, DAYS_PER_SAMPLE, &options)
            .expect("untimed serial M3 run succeeds");
    tick_digests(&hybrid, context.relationship_graph(), &outcomes)
}

fn untimed_parallel_digests(
    base: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    chunk_size: usize,
) -> TickDigests {
    let mut hybrid = base.clone();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context = M3ParallelStorageRunContext::new(&hybrid, config, &run_context)
        .expect("parallel context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
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
    tick_digests(&hybrid, context.relationship_graph(), &outcomes)
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
        let serial = timed_serial_sample(base, config);
        let parallel = timed_parallel_sample(base, config, pool, chunk_size);
        assert_eq!(parallel.digests, serial.digests);

        if sample == 0 {
            assert_eq!(untimed_serial_digests(base, config), serial.digests);
            assert_eq!(
                untimed_parallel_digests(base, config, pool, chunk_size),
                parallel.digests
            );
        }
        if sample >= WARMUPS {
            serial_ticks.push(serial.tick_us);
            parallel_ticks.push(parallel.tick_us);
            for stage in 0..5 {
                serial_stages[stage].push(serial.stages_us[stage]);
                parallel_stages[stage].push(parallel.stages_us[stage]);
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
    if horizon == 100 {
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let mut context = M3StorageRunContext::new(&hybrid, config, &run_context)
            .expect("day-100 context builds");
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        for _ in 0..100 {
            let _ = run_m3_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .expect("serial run to day 100 succeeds");
        }
    }
    hybrid
}

#[derive(Debug, Clone, PartialEq)]
struct ReplayTrace {
    state_days: Vec<CanonicalHash>,
    metric_days: Vec<M3DailyMetrics>,
    event_days: Vec<Vec<sim_model::EventRecord>>,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

fn serial_replay_trace(config: &SimConfig) -> ReplayTrace {
    let mut hybrid = HybridWorldState::hybrid(initialize_world(config).expect("world initializes"));
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context = M3StorageRunContext::new(&hybrid, config, &run_context)
        .expect("serial replay context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut state_days = Vec::with_capacity(100);
    let mut metric_days = Vec::with_capacity(100);
    let mut event_days = Vec::with_capacity(100);
    for _ in 0..100 {
        let mut outcomes = run_m3_storage_days(&mut hybrid, config, &mut context, 1, &options)
            .expect("serial extended replay succeeds");
        let outcome = outcomes.pop().unwrap();
        let storage = hybrid.storage().unwrap();
        state_days.push(
            canonical_m3_state_hash_from_storage(
                storage,
                hybrid.world.current_day,
                &hybrid.world.settlements,
                context.relationship_graph(),
            )
            .expect("serial state hashes"),
        );
        metric_days.push(outcome.metrics.unwrap());
        event_days.push(outcome.events);
    }
    let all_events: Vec<_> = event_days.iter().flatten().cloned().collect();
    ReplayTrace {
        state_days,
        metrics: canonical_m3_metrics_hash(&metric_days).unwrap(),
        events: canonical_event_hash(&all_events).unwrap(),
        metric_days,
        event_days,
    }
}

fn parallel_replay_trace(
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    chunk_size: usize,
) -> ReplayTrace {
    let mut hybrid = HybridWorldState::hybrid(initialize_world(config).expect("world initializes"));
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut context = M3ParallelStorageRunContext::new(&hybrid, config, &run_context)
        .expect("parallel replay context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut state_days = Vec::with_capacity(100);
    let mut metric_days = Vec::with_capacity(100);
    let mut event_days = Vec::with_capacity(100);
    for _ in 0..100 {
        let mut outcomes = run_m3_storage_days_with_parallel_phase6b(
            &mut hybrid,
            config,
            &mut context,
            1,
            &options,
            pool,
            NonZeroUsize::new(chunk_size).unwrap(),
        )
        .expect("parallel extended replay succeeds");
        let outcome = outcomes.pop().unwrap();
        state_days.push(
            canonical_m3_state_hash_from_storage(
                hybrid.storage().unwrap(),
                hybrid.world.current_day,
                &hybrid.world.settlements,
                context.relationship_graph(),
            )
            .expect("parallel state hashes"),
        );
        metric_days.push(outcome.metrics.unwrap());
        event_days.push(outcome.events);
    }
    let all_events: Vec<_> = event_days.iter().flatten().cloned().collect();
    ReplayTrace {
        state_days,
        metrics: canonical_m3_metrics_hash(&metric_days).unwrap(),
        events: canonical_event_hash(&all_events).unwrap(),
        metric_days,
        event_days,
    }
}

fn extended_cells() -> Vec<(Cell, u64)> {
    let mut cells = Vec::with_capacity(60);
    for seed in [0, 1, MASTER_SEED] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for p_local in [0.0, 0.5, 1.0] {
                    cells.push((
                        Cell {
                            population: 300,
                            settlements,
                            degree,
                            p_local,
                        },
                        seed,
                    ));
                }
            }
        }
    }
    for population in [1_000, 3_000] {
        for p_local in [0.0, 0.5, 1.0] {
            cells.push((
                Cell {
                    population,
                    settlements: (population / 17) as u32,
                    degree: 8,
                    p_local,
                },
                MASTER_SEED,
            ));
        }
    }
    cells
}

fn run_extended_release_parity() {
    let host_max = std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(host_max)
        .build()
        .expect("host-maximum pool builds");
    println!("EXTENDED_BEGIN|cells=60|days=100|host_max_pool={host_max}");
    let mut checked = 0;
    for (cell, seed) in extended_cells() {
        let config = config_for_seed(cell, seed);
        let expected = serial_replay_trace(&config);
        for chunk in BENCH_CHUNKS {
            let actual = parallel_replay_trace(&config, &pool, chunk);
            assert_eq!(actual, expected);
            checked += 1;
            println!(
                "EXTENDED_CELL|N={}|K={}|k={}|p={:.2}|seed={seed}|pool={host_max}|chunk={chunk}|PASS",
                cell.population, cell.settlements, cell.degree, cell.p_local
            );
        }
    }
    assert_eq!(checked, 300);
    println!("EXTENDED_PASS|cells=60|configurations=300|host_max_pool={host_max}|skipped=0");
}

fn interaction_followup_cells() -> Vec<Cell> {
    [10_000, 100_000]
        .into_iter()
        .flat_map(|population| {
            [0.0, 0.5, 1.0].into_iter().map(move |p_local| Cell {
                population,
                settlements: (population / 200) as u32,
                degree: 8,
                p_local,
            })
        })
        .collect()
}

fn print_decay_row(cell: Cell, profile: &InteractionDayProfile) {
    println!(
        "DECAY|{}|{}|{:.2}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        cell.population,
        cell.settlements,
        cell.p_local,
        profile.day + 1,
        profile.alive,
        profile
            .mean_alive_food
            .map_or_else(|| "NA".to_string(), |value| format!("{value:.6}")),
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
        profile.outcomes.zero_target,
        profile.outcomes.initiator_ineligible,
        profile.outcomes.target_ineligible,
        profile.outcomes.theft_failed,
        profile.outcomes.applied_zero,
    );
}

fn run_workload_decay() {
    println!(
        "DECAY_HEADER|N|K|p_local|day|alive|mean_alive_food|work|buy|sell|give|steal|idle|give_some|give_none|steal_some|steal_none|keyed|cross_partition|applied_positive|zero_target|initiator_ineligible|target_ineligible|theft_failed|applied_zero"
    );
    for cell in interaction_followup_cells() {
        let config = config_for(cell);
        let base =
            HybridWorldState::hybrid(initialize_world(&config).expect("decay world initializes"));
        let mut measured = ManualRun::from_hybrid_world(&config, base.clone());
        let mut oracle = base;
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let mut oracle_context = M3StorageRunContext::new(&oracle, &config, &run_context)
            .expect("decay oracle context builds");
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let mut measured_metrics = Vec::with_capacity(100);
        let mut oracle_metrics = Vec::with_capacity(100);
        let mut all_events = Vec::new();
        let mut first_zero = false;

        for _ in 0..100 {
            let profile = advance_interaction_profile(&mut measured, &config, false, false);
            let mut outcomes =
                run_m3_storage_days(&mut oracle, &config, &mut oracle_context, 1, &options)
                    .expect("decay oracle day succeeds");
            let outcome = outcomes.pop().expect("one decay outcome");
            assert_eq!(Some(&profile.metrics), outcome.metrics.as_ref());
            let measured_state = canonical_m3_state_hash_from_storage(
                &measured.storage,
                measured.world.current_day,
                &measured.world.settlements,
                &measured.relationship_graph,
            )
            .expect("measured state digest succeeds");
            let oracle_state = canonical_m3_state_hash_from_storage(
                oracle.storage().expect("oracle storage exists"),
                oracle.world.current_day,
                &oracle.world.settlements,
                oracle_context.relationship_graph(),
            )
            .expect("oracle state digest succeeds");
            assert_eq!(measured_state, oracle_state);
            measured_metrics.push(profile.metrics.clone());
            oracle_metrics.push(outcome.metrics.expect("metrics enabled"));
            all_events.extend(outcome.events);

            if !first_zero && profile.keyed == 0 {
                let targeted_actions = profile.action_counts[Action::GiveFood.index()]
                    + profile.action_counts[Action::StealFood.index()];
                let reason = if profile.alive == 0 {
                    "population_collapse"
                } else if targeted_actions == 0 {
                    "action_choice_shift"
                } else if profile.give_some + profile.steal_some == 0 {
                    "no_eligible_targets"
                } else {
                    "unexplained"
                };
                println!(
                    "DECAY_FIRST_ZERO|N={}|K={}|p={:.2}|day={}|reason={reason}|alive={}|give_actions={}|steal_actions={}|give_none={}|steal_none={}",
                    cell.population,
                    cell.settlements,
                    cell.p_local,
                    profile.day + 1,
                    profile.alive,
                    profile.action_counts[Action::GiveFood.index()],
                    profile.action_counts[Action::StealFood.index()],
                    profile.give_none,
                    profile.steal_none,
                );
                first_zero = true;
            }
            print_decay_row(cell, &profile);
        }

        assert_eq!(
            canonical_m3_metrics_hash(&measured_metrics).expect("measured metrics hash"),
            canonical_m3_metrics_hash(&oracle_metrics).expect("oracle metrics hash")
        );
        let events_hash = canonical_event_hash(&all_events).expect("decay events hash");
        let final_state = canonical_m3_state_hash_from_storage(
            &measured.storage,
            measured.world.current_day,
            &measured.world.settlements,
            &measured.relationship_graph,
        )
        .expect("decay final state hash");
        println!(
            "DECAY_PARITY|N={}|K={}|p={:.2}|state={final_state}|metrics={}|events={events_hash}",
            cell.population,
            cell.settlements,
            cell.p_local,
            canonical_m3_metrics_hash(&measured_metrics).unwrap(),
        );
    }
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
    let p4_total_ns = profiles
        .iter()
        .map(|profile| profile.phase4_intent_timing.total.as_nanos())
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
    let phase4_total_us = us_per_tick(p4_total_ns);
    let phase4_other_us = (phase4_total_us - phase4_target_us).max(0.0);
    let phase6b_validation_us = us_per_tick(total_ns(|p| p.phase6b_timing.validation_slot_lookup));
    let phase6b_key_us = us_per_tick(total_ns(|p| p.phase6b_timing.key_generation));
    let phase6b_sort_us = us_per_tick(total_ns(|p| p.phase6b_timing.global_sort));
    let applied_us = us_per_tick(total_ns(|p| p.phase6b_timing.applied_positive_entries));
    let other_keyed_us = us_per_tick(total_ns(|p| p.phase6b_timing.other_keyed_entries));
    let output_us = us_per_tick(total_ns(|p| p.phase6b_timing.output_assembly));
    let search_setup_us =
        phase4_target_us + phase6b_validation_us + phase6b_key_us + phase6b_sort_us;
    let applied_per_day = counts.outcomes.applied_positive as f64 / days;
    let p4_hook_overhead_us = p4_hook_delta_us;
    let p6_hook_overhead_us = p6_hook_delta_us;
    SearchSample {
        phase4_target_us,
        phase4_other_us,
        phase4_hook_overhead_us: p4_hook_overhead_us,
        phase4_hook_overhead_percent: p4_hook_delta_us / (p4_plain_wall_ns as f64 / 1_000.0 / days)
            * 100.0,
        phase6b_validation_us,
        phase6b_key_us,
        phase6b_sort_us,
        applied_us,
        other_keyed_us,
        output_us,
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
    }
}

fn search_stat(samples: &[SearchSample], select: fn(&SearchSample) -> f64) -> (f64, f64) {
    let mut values: Vec<_> = samples.iter().map(select).collect();
    if values.iter().all(|value| !value.is_finite()) {
        return (f64::NAN, f64::NAN);
    }
    summarize(&mut values)
}

fn print_search_counts(cell: Cell, counts: SearchCounts) {
    println!(
        "SEARCH_COUNTS|N={}|K={}|p={:.2}|days=3|work={}|buy={}|sell={}|give={}|steal={}|idle={}|give_some={}|give_none={}|steal_some={}|steal_none={}|keyed={}|cross_partition={}|applied_positive={}|zero_target={}|initiator_ineligible={}|target_ineligible={}|theft_failed={}|applied_zero={}",
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
}

fn run_search_vs_work() {
    type SearchMetric = (&'static str, fn(&SearchSample) -> f64);
    let metric_names: [SearchMetric; 17] = [
        ("phase4_neighbor_filter_draw", |s| s.phase4_target_us),
        ("phase4_other_intent_construction", |s| s.phase4_other_us),
        ("phase6b_validation_slot", |s| s.phase6b_validation_us),
        ("phase6b_key_generation", |s| s.phase6b_key_us),
        ("phase6b_sort", |s| s.phase6b_sort_us),
        ("phase6b_applied_positive", |s| s.applied_us),
        ("phase6b_other_keyed", |s| s.other_keyed_us),
        ("phase6b_output_assembly", |s| s.output_us),
        ("search_setup", |s| s.search_setup_us),
        ("applied_work", |s| s.work_us),
        ("search_per_applied", |s| s.search_per_applied_us),
        ("work_per_applied", |s| s.work_per_applied_us),
        ("search_to_work_ratio", |s| s.search_to_work_ratio),
        ("phase4_timing_hook_overhead", |s| s.phase4_hook_overhead_us),
        ("phase4_timing_hook_overhead_pct", |s| {
            s.phase4_hook_overhead_percent
        }),
        ("phase6b_timing_hook_overhead", |s| {
            s.phase6b_hook_overhead_us
        }),
        ("phase6b_timing_hook_overhead_pct", |s| {
            s.phase6b_hook_overhead_percent
        }),
    ];
    println!("SEARCH_HEADER|N|K|p_local|metric|median_us_or_ratio|MAD");
    for cell in interaction_followup_cells() {
        let config = config_for(cell);
        let base = HybridWorldState::hybrid(
            initialize_world(&config).expect("search/work world initializes"),
        );
        let mut samples = Vec::with_capacity(SAMPLES);
        let mut expected_trace: Option<TickDigests> = None;
        let mut expected_counts: Option<SearchCounts> = None;
        for sample in 0..(WARMUPS + SAMPLES) {
            let mut measured = ManualRun::from_hybrid_world(&config, base.clone());
            let mut oracle = base.clone();
            let run_context =
                M0RunContext::new(config.world.master_seed, config.world.replicate_id);
            let mut oracle_context = M3StorageRunContext::new(&oracle, &config, &run_context)
                .expect("search/work oracle context builds");
            let options = DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: false,
            };
            let mut profiles = Vec::with_capacity(3);
            let mut oracle_outcomes = Vec::with_capacity(3);
            for day_index in 0..3 {
                let profile = advance_interaction_profile(
                    &mut measured,
                    &config,
                    true,
                    (sample + day_index) % 2 == 1,
                );
                let mut day_outcomes =
                    run_m3_storage_days(&mut oracle, &config, &mut oracle_context, 1, &options)
                        .expect("search/work oracle day succeeds");
                let outcome = day_outcomes.pop().expect("one oracle outcome");
                assert_eq!(Some(&profile.metrics), outcome.metrics.as_ref());
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
                assert_eq!(trace, *expected);
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

        print_search_counts(cell, expected_counts.expect("search counts collected"));
        for (name, select) in metric_names {
            let (median, mad) = search_stat(&samples, select);
            if median.is_finite() && mad.is_finite() {
                println!(
                    "SEARCH|N={}|K={}|p={:.2}|{name}|{median:.4}|{mad:.4}",
                    cell.population, cell.settlements, cell.p_local
                );
            } else {
                println!(
                    "SEARCH|N={}|K={}|p={:.2}|{name}|NA|NA",
                    cell.population, cell.settlements, cell.p_local
                );
            }
        }
    }
}

fn run_interaction_followup() {
    let total_started = Instant::now();
    print_host_info();
    println!("FOLLOWUP_CONFIG|M2-40 unchanged|N=10000,100000|K=N/200|k=8|p_local=0,0.5,1");
    println!(
        "FOLLOWUP_METHOD|A=100 sequential days from day0|B=day0 through day3|warmups=2|timed_samples=11|serial M3 storage path|metrics=on|events=on|snapshots=off"
    );
    run_workload_decay();
    run_search_vs_work();
    println!(
        "FOLLOWUP_TOTAL_SECONDS|{:.3}",
        total_started.elapsed().as_secs_f64()
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
    if std::env::args().any(|arg| arg == "--interaction-followup") {
        run_interaction_followup();
        return;
    }
    let total_started = Instant::now();
    print_host_info();
    println!("CONFIG|M2-40 fixed-group-size parameters; only M3 profile, k=8, and p_local varied");
    println!("METHOD|warmups=2|samples=11|days_per_sample=3|metrics=on|events=on|snapshots=off");
    println!(
        "TICK|horizon|N|K|p|threads|chunk|serial_median_us|serial_MAD_us|parallel_median_us|parallel_MAD_us|speedup"
    );
    println!(
        "STAGE|horizon|N|K|p|threads|chunk|stage|serial_median_us|serial_MAD_us|parallel_median_us|parallel_MAD_us|speedup"
    );
    let pools: Vec<_> = [1, 2, 4, 6]
        .into_iter()
        .map(|threads| {
            (
                threads,
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("fixed-size benchmark pool builds"),
            )
        })
        .collect();

    for cell in make_cells() {
        let config = config_for(cell);
        for horizon in [0, 100] {
            let base = build_horizon_state(&config, horizon);
            let report_pool = &pools[0].1;
            let volume = measure_workload(&config, &base, report_pool);
            let no_interaction = volume.keyed_by_day.iter().all(|&count| count == 0);
            println!(
                "VOLUME|horizon={horizon}|N={}|K={}|p={:.2}|alive={}|keyed={},{},{}|keyed_mean={:.3}|cross_mean={:.3}|conflict={}|contention={}|levels_mean={:.3}|max_level_mean={:.3}|max_level={}|workload={}",
                cell.population,
                cell.settlements,
                cell.p_local,
                volume.alive,
                volume.keyed_by_day[0],
                volume.keyed_by_day[1],
                volume.keyed_by_day[2],
                volume.keyed_by_day.iter().sum::<u64>() as f64 / 3.0,
                volume.cross_partition_mean,
                volume
                    .conflict_fraction
                    .map_or_else(|| "NA".to_string(), |v| format!("{v:.6}")),
                volume
                    .mean_contention
                    .map_or_else(|| "NA".to_string(), |v| format!("{v:.6}")),
                volume.level_count_mean,
                volume.max_level_size_mean,
                volume.max_level_size,
                if no_interaction {
                    "no_interaction_workload"
                } else {
                    "interaction_workload"
                }
            );

            for (threads, pool) in &pools {
                for chunk in BENCH_CHUNKS {
                    measure_pair(horizon, cell, &config, &base, *threads, chunk, pool);
                }
            }
        }
    }

    run_extended_release_parity();
    println!(
        "BENCH_TOTAL_SECONDS|{:.3}",
        total_started.elapsed().as_secs_f64()
    );
}
