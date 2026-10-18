#![allow(clippy::disallowed_methods)]

use sim_core::{AgentId, SimulationDay};
use sim_model::decision::{PrimaryActionChoice, phase4_primary_action_selection_storage_into};
use sim_model::features::{
    AgentFeatures, Phase3ScarcityScratch, phase3_observation_and_features_storage_with_scratch,
};
use sim_model::hashing::{
    canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash,
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
    M3Phase6BStorageScratch, Phase8WelfareScratch, TargetedActionKind, compare_keyed_interactions,
    compute_m3_resolution_key, phase6a_work_resolution_storage,
    phase6b_targeted_resolution_storage_m3_with_scratch,
    phase7_market_clearance_storage_with_config,
    phase8_welfare_distribution_storage_with_config_and_scratch,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, M3StorageRunContext,
    run_m3_reference_day, run_m3_storage_days,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{SimConfig, SimulationProfile, initialize_world};
use std::collections::HashMap;
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
const SAMPLES: usize = 7;
const DAYS_PER_SAMPLE: u32 = 3;

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
    phase6b: M3Phase6BStorageScratch,
    phase8: Phase8WelfareScratch,
}

fn config_for(cell: Cell) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 config parses");
    config.profile = SimulationProfile::M3;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.world.master_seed = MASTER_SEED;
    config.world.replicate_id = REPLICATE_ID;
    config.interaction.relationship_degree_k = Some(cell.degree as i64);
    config.interaction.p_local = Some(cell.p_local);
    config.validate().expect("M3 benchmark config validates");
    config
}

impl ManualRun {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).expect("world initializes");
        let storage = SegmentedAgentStorage::from_agents(&world.agents);
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
            phase6b: M3Phase6BStorageScratch::with_capacity(n),
            phase8: Phase8WelfareScratch::with_capacity(k),
        }
    }

    fn day(&mut self, config: &SimConfig) -> DailyVolume {
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
        let keyed_entries =
            keyed_stream_from_partitions(&partitions, self.world.current_day, self.run_context);
        volume.keyed = keyed_entries.len() as u64;
        let conflict = conflict_stats(&keyed_entries);
        if let Some(value) = conflict.fraction {
            volume.conflict_fraction_sum += value;
            volume.conflict_fraction_days += 1;
        }
        if let Some(value) = conflict.mean_contention {
            volume.contention_sum += value;
            volume.contention_days += 1;
        }

        let _work_resolutions = phase6a_work_resolution_storage(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
        )
        .expect("Phase 6A succeeds");

        let _targeted_resolutions = phase6b_targeted_resolution_storage_m3_with_scratch(
            &mut self.storage,
            &self.world.settlements,
            self.world.current_day,
            &effective,
            &partitions,
            &mut self.phase6b,
        )
        .expect("M3 Phase 6B succeeds");

        let _market_resolutions = phase7_market_clearance_storage_with_config(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
            &effective.economy,
        )
        .expect("phase 7 succeeds");

        let _welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
            &mut self.storage,
            &mut self.world.settlements,
            &effective,
            &mut self.phase8,
        )
        .expect("phase 8 succeeds");

        let _mortality =
            phase9_mortality_commitment_storage(&mut self.storage).expect("phase 9 succeeds");

        self.world.current_day = SimulationDay(next_day);
        volume
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
    let mut cells = Vec::with_capacity(45);
    for (population, settlements) in [(10_000, 50), (50_000, 250), (100_000, 500)] {
        for p_local in [0.0, 0.25, 0.5, 0.75, 1.0] {
            for degree in [4, 8, 16] {
                cells.push(Cell {
                    population,
                    settlements,
                    degree,
                    p_local,
                });
            }
        }
    }
    cells
}

fn time_graph_and_projection(cell: Cell, config: &SimConfig) {
    let world = initialize_world(config).expect("world initializes");
    let storage = SegmentedAgentStorage::from_agents(&world.agents);
    let memberships: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect();
    let mut graph_times = Vec::with_capacity(SAMPLES);
    let mut projection_times = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let start = Instant::now();
        let graph = RelationshipGraph::generate(
            &memberships,
            MASTER_SEED,
            REPLICATE_ID,
            cell.degree,
            cell.p_local,
        )
        .expect("relationship graph builds");
        let graph_elapsed = elapsed_us(start.elapsed());
        let start = Instant::now();
        let _projection = M3Phase4SlotProjection::new(&graph, &storage).expect("projection builds");
        let projection_elapsed = elapsed_us(start.elapsed());
        if sample >= WARMUPS {
            graph_times.push(graph_elapsed);
            projection_times.push(projection_elapsed);
        }
    }
    let (graph_median, graph_mad) = summarize(&mut graph_times);
    let (projection_median, projection_mad) = summarize(&mut projection_times);
    println!(
        "ONETIME|{}|{}|{}|{:.2}|{:.3}|{:.3}|{:.3}|{:.3}",
        cell.population,
        cell.settlements,
        cell.degree,
        cell.p_local,
        graph_median,
        graph_mad,
        projection_median,
        projection_mad
    );
}

fn time_full_tick(cell: Cell, config: &SimConfig) {
    let initial_world = initialize_world(config).expect("world initializes");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut samples = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let mut hybrid = HybridWorldState::hybrid(initial_world.clone());
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut context = M3StorageRunContext::new(&hybrid, config, &run_context)
            .expect("M3 storage context builds");
        let start = Instant::now();
        let _outcomes =
            run_m3_storage_days(&mut hybrid, config, &mut context, DAYS_PER_SAMPLE, &options)
                .expect("M3 serial storage days succeed");
        let sample_us = elapsed_us(start.elapsed()) / DAYS_PER_SAMPLE as f64;
        if sample >= WARMUPS {
            samples.push(sample_us);
        }
    }
    let (median, mad) = summarize(&mut samples);
    println!(
        "TICK|{}|{}|{}|{:.2}|{:.3}|{:.3}",
        cell.population, cell.settlements, cell.degree, cell.p_local, median, mad
    );
}

fn measure_volume(cell: Cell, config: &SimConfig) {
    let mut run = ManualRun::new(config);
    let mut keyed = 0u64;
    let mut cross = 0u64;
    let mut conflict = 0.0;
    let mut conflict_days = 0;
    let mut contention = 0.0;
    let mut contention_days = 0;
    for _ in 0..DAYS_PER_SAMPLE {
        let day = run.day(config);
        keyed += day.keyed;
        cross += day.cross_partition;
        conflict += day.conflict_fraction_sum;
        conflict_days += day.conflict_fraction_days;
        contention += day.contention_sum;
        contention_days += day.contention_days;
    }
    let days = DAYS_PER_SAMPLE as f64;
    let conflict = (conflict_days > 0).then_some(conflict / conflict_days as f64);
    let contention = (contention_days > 0).then_some(contention / contention_days as f64);
    println!(
        "VOLUME|{}|{}|{}|{:.2}|{:.3}|{:.3}|{:.6}|{:.6}",
        cell.population,
        cell.settlements,
        cell.degree,
        cell.p_local,
        keyed as f64 / days,
        cross as f64 / days,
        conflict.unwrap_or(f64::NAN),
        contention.unwrap_or(f64::NAN)
    );
}

fn run_full_matrix() {
    println!("M3-01 serial benchmark; M2-40 fixed-group-size config, M3 fields only");
    println!("ROW|N|K|k|p_local|median_us_tick|MAD_us_tick");
    for cell in make_cells() {
        let config = config_for(cell);
        time_full_tick(cell, &config);
        time_graph_and_projection(cell, &config);
        measure_volume(cell, &config);
    }
}

fn run_release_differential() {
    let cell = Cell {
        population: 10_000,
        settlements: 50,
        degree: 8,
        p_local: 0.5,
    };
    let config = config_for(cell);
    let initial_world = initialize_world(&config).expect("world initializes");
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut reference_world = initial_world.clone();
    let reference_context = M3ReferenceContext::new(&reference_world, &config, &run_context)
        .expect("M3 reference context builds");
    let mut hybrid = HybridWorldState::hybrid(initial_world);
    let mut storage_context = M3StorageRunContext::new(&hybrid, &config, &run_context)
        .expect("M3 storage context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    let start = Instant::now();
    let mut reference_metrics: Vec<M3DailyMetrics> = Vec::with_capacity(100);
    let mut reference_events = Vec::new();
    for _ in 0..100 {
        let outcome =
            run_m3_reference_day(&mut reference_world, &config, &reference_context, &options)
                .expect("reference differential run succeeds");
        reference_metrics.push(outcome.metrics.expect("metrics enabled"));
        reference_events.extend(outcome.events);
    }
    let reference_elapsed = start.elapsed();

    let start = Instant::now();
    let storage_outcomes =
        run_m3_storage_days(&mut hybrid, &config, &mut storage_context, 100, &options)
            .expect("storage differential run succeeds");
    let storage_elapsed = start.elapsed();
    let storage_metrics: Vec<_> = storage_outcomes
        .iter()
        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
        .collect();
    let storage_events: Vec<_> = storage_outcomes
        .into_iter()
        .flat_map(|outcome| outcome.events)
        .collect();

    let reference_state =
        canonical_m3_state_hash(&reference_world, reference_context.relationship_graph())
            .expect("reference state hashes");
    let storage = hybrid.segmented_storage.as_ref().unwrap();
    let storage_state = canonical_m3_state_hash_from_storage(
        storage,
        hybrid.world.current_day,
        &hybrid.world.settlements,
        storage_context.relationship_graph(),
    )
    .expect("storage state hashes");
    let reference_metrics_hash = canonical_m3_metrics_hash(&reference_metrics).unwrap();
    let storage_metrics_hash = canonical_m3_metrics_hash(&storage_metrics).unwrap();
    let reference_events_hash = canonical_event_hash(&reference_events).unwrap();
    let storage_events_hash = canonical_event_hash(&storage_events).unwrap();
    assert_eq!(reference_state, storage_state);
    assert_eq!(reference_metrics_hash, storage_metrics_hash);
    assert_eq!(reference_events_hash, storage_events_hash);
    println!(
        "RELEASE_DIFF|10000|50|8|0.500|100|{}|{}|{}|{}|{}|{}|{:.3}|{:.3}",
        reference_state,
        storage_state,
        reference_metrics_hash,
        storage_metrics_hash,
        reference_events_hash,
        storage_events_hash,
        reference_elapsed.as_secs_f64() * 1000.0,
        storage_elapsed.as_secs_f64() * 1000.0
    );
}

fn main() {
    run_full_matrix();
    run_release_differential();
}
