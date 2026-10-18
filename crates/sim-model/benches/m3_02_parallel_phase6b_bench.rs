#![allow(clippy::disallowed_methods)]

use sim_core::{AgentId, SimulationDay};
use sim_model::decision::{PrimaryActionChoice, phase4_primary_action_selection_storage_into};
use sim_model::features::{
    AgentFeatures, Phase3ScarcityScratch, phase3_observation_and_features_storage_with_scratch,
};
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::intents::{
    Intent, M3Phase4SlotProjection, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_storage_into,
};
use sim_model::metrics::M3DailyMetrics;
use sim_model::partitioning::{
    Phase5PartitionScratch, SettlementIntentPartition,
    phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::{phase1_resource_regrowth, phase9_mortality_commitment_storage};
use sim_model::relationship_graph::RelationshipGraph;
use sim_model::resolution::{
    M3Phase6BLevelStats, M3Phase6BParallelScratch, M3Phase6BStageTimings, Phase8WelfareScratch,
    TargetedActionKind, compare_keyed_interactions, compute_m3_resolution_key,
    phase6a_work_resolution_storage, phase6b_targeted_resolution_storage_m3_parallel_with_timing,
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
    run_context: M0RunContext,
    projection: M3Phase4SlotProjection,
    features: Vec<AgentFeatures>,
    choices: Vec<PrimaryActionChoice>,
    intents: Vec<Intent>,
    candidates: Vec<AgentId>,
    phase3: Phase3ScarcityScratch,
    phase5: Phase5PartitionScratch,
    phase6b_parallel: M3Phase6BParallelScratch,
    phase8: Phase8WelfareScratch,
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
            run_context,
            projection,
            features: Vec::with_capacity(n),
            choices: Vec::with_capacity(n),
            intents: Vec::with_capacity(n),
            candidates: Vec::with_capacity(
                config.interaction.relationship_degree_k.unwrap() as usize
            ),
            phase3: Phase3ScarcityScratch::with_capacity(k),
            phase5: Phase5PartitionScratch::with_capacity(k),
            phase6b_parallel: M3Phase6BParallelScratch::with_capacity(n, k, n),
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
