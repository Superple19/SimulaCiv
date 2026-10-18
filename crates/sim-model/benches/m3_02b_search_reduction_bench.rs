// The benchmark calls the crate-private Phase 4 eligibility entry point so its timing
// measures the same implementation as the explicit production runner.
#![allow(hidden_glob_reexports)]
extern crate sim_model as runtime;

#[allow(dead_code, unused_imports)]
#[path = "../src/lib.rs"]
mod sim_model;
pub use sim_model::*;

use sim_model::decision::{
    Action, PrimaryActionChoice, phase4_primary_action_selection_storage_into,
};
use sim_model::features::{
    AgentFeatures, Phase3ScarcityScratch, phase3_observation_and_features_storage_with_scratch,
};
use sim_model::intents::{
    Intent, M3Phase4GlobalEligibility, M3Phase4SlotProjection, generate_intents_m3_storage_into,
    generate_intents_m3_storage_with_global_eligibility_into,
};
use sim_model::partitioning::{
    Phase5PartitionScratch, phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::phase1_resource_regrowth;
use sim_model::resolution::{
    M3Phase6BStorageScratch, TargetedOutcome, phase6a_work_resolution_storage,
    phase6b_targeted_resolution_storage_m3_with_timing,
};
use sim_model::state::HybridWorldState as SourceHybridWorldState;
use sim_model::storage::SegmentedAgentStorage as SourceStorage;
use sim_model::{
    RelationshipGraph as SourceGraph, SimConfig as SourceConfig,
    SimulationProfile as SourceProfile, initialize_world as source_initialize_world,
};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

const CONFIG: &str = r#"
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

const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const WARMUPS: usize = 2;
const SAMPLES: usize = 11;
const DAYS_PER_SAMPLE: u32 = 3;

#[derive(Clone, Copy)]
struct Cell {
    population: u64,
    settlements: u32,
    p_local: f32,
    horizon: u32,
}

#[derive(Debug, PartialEq)]
struct Trace {
    state: runtime::hashing::CanonicalHash,
    metrics: runtime::hashing::CanonicalHash,
    events: runtime::hashing::CanonicalHash,
    daily_metrics: Vec<runtime::M3DailyMetrics>,
    daily_events: Vec<Vec<runtime::EventRecord>>,
}

fn config_for(cell: Cell) -> runtime::SimConfig {
    let mut config = runtime::SimConfig::parse_and_validate(CONFIG).expect("M2-40 config parses");
    config.profile = runtime::SimulationProfile::M3;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(cell.p_local);
    if cell.horizon == 150 {
        config.environment.base_metabolic_cost = 0.48;
    }
    config.validate().expect("benchmark config validates");
    config
}

fn source_config_for(cell: Cell) -> SourceConfig {
    let mut config = SourceConfig::parse_and_validate(CONFIG).expect("M2-40 source config parses");
    config.profile = SourceProfile::M3;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(cell.p_local);
    if cell.horizon == 150 {
        config.environment.base_metabolic_cost = 0.48;
    }
    config
        .validate()
        .expect("source benchmark config validates");
    config
}

fn options() -> runtime::runner::DayExecutionOptions {
    runtime::runner::DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

fn source_options() -> sim_model::runner::DayExecutionOptions {
    sim_model::runner::DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

fn graph_for(
    config: &runtime::SimConfig,
    storage: &runtime::storage::SegmentedAgentStorage,
) -> runtime::RelationshipGraph {
    let memberships: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect();
    runtime::RelationshipGraph::generate(
        &memberships,
        config.world.master_seed,
        config.world.replicate_id,
        8,
        config.interaction.p_local.unwrap(),
    )
    .expect("production relationship graph builds")
}

fn source_graph_for(config: &SourceConfig, storage: &SourceStorage) -> SourceGraph {
    let memberships: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect();
    SourceGraph::generate(
        &memberships,
        config.world.master_seed,
        config.world.replicate_id,
        8,
        config.interaction.p_local.unwrap(),
    )
    .expect("relationship graph builds")
}

fn build_horizon(config: &runtime::SimConfig, horizon: u32) -> runtime::state::HybridWorldState {
    let mut world = runtime::state::HybridWorldState::hybrid(
        runtime::initialize_world(config).expect("world initializes"),
    );
    if horizon > 0 {
        let context_coords = runtime::runner::M0RunContext::new(SEED, REPLICATE);
        let mut context =
            runtime::runner::M3StorageRunContext::new(&world, config, &context_coords)
                .expect("horizon context builds");
        for _ in 0..horizon {
            let _ = runtime::runner::run_m3_storage_days(
                &mut world,
                config,
                &mut context,
                1,
                &options(),
            )
            .expect("checkpoint run succeeds");
        }
    }
    world
}

fn build_source_horizon(config: &SourceConfig, horizon: u32) -> SourceHybridWorldState {
    let mut world = SourceHybridWorldState::hybrid(
        source_initialize_world(config).expect("source world initializes"),
    );
    if horizon > 0 {
        let coordinates = sim_model::runner::M0RunContext::new(SEED, REPLICATE);
        let mut context = sim_model::runner::M3StorageRunContext::new(&world, config, &coordinates)
            .expect("source horizon context builds");
        for _ in 0..horizon {
            let _ = sim_model::runner::run_m3_storage_days(
                &mut world,
                config,
                &mut context,
                1,
                &source_options(),
            )
            .expect("source checkpoint run succeeds");
        }
    }
    world
}

fn trace(
    world: &runtime::state::HybridWorldState,
    graph: &runtime::RelationshipGraph,
    outcomes: &[runtime::M3DayOutcome],
) -> Trace {
    let storage = world.storage().expect("hybrid storage exists");
    let daily_metrics: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.metrics.as_ref().expect("metrics enabled").clone())
        .collect();
    let daily_events: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.events.clone())
        .collect();
    let events: Vec<_> = daily_events.iter().flatten().cloned().collect();
    Trace {
        state: runtime::hashing::canonical_m3_state_hash_from_storage(
            storage,
            world.world.current_day,
            &world.world.settlements,
            graph,
        )
        .expect("State digest succeeds"),
        metrics: runtime::hashing::canonical_m3_metrics_hash(&daily_metrics)
            .expect("Metrics digest succeeds"),
        events: runtime::hashing::canonical_event_hash(&events).expect("Events digest succeeds"),
        daily_metrics,
        daily_events,
    }
}

struct TimedRun {
    micros_per_day: f64,
    trace: Trace,
}

fn run_sample(
    base: &runtime::state::HybridWorldState,
    config: &runtime::SimConfig,
    graph: &runtime::RelationshipGraph,
    optimized: bool,
) -> TimedRun {
    let mut world = base.clone();
    let context_coords = runtime::runner::M0RunContext::new(SEED, REPLICATE);
    let outcomes = if optimized {
        let mut context =
            runtime::runner::M3SearchReductionRunContext::new(&world, config, &context_coords)
                .expect("search-reduction context builds");
        let started = Instant::now();
        let outcomes = runtime::runner::run_m3_storage_days_with_search_reduction(
            &mut world,
            config,
            &mut context,
            DAYS_PER_SAMPLE,
            &options(),
        )
        .expect("search-reduction run succeeds");
        let elapsed = started.elapsed();
        (outcomes, elapsed)
    } else {
        let mut context =
            runtime::runner::M3StorageRunContext::new(&world, config, &context_coords)
                .expect("serial context builds");
        let started = Instant::now();
        let outcomes = runtime::runner::run_m3_storage_days(
            &mut world,
            config,
            &mut context,
            DAYS_PER_SAMPLE,
            &options(),
        )
        .expect("serial run succeeds");
        let elapsed = started.elapsed();
        (outcomes, elapsed)
    };
    let (outcomes, elapsed) = outcomes;
    let micros_per_day = elapsed.as_nanos() as f64 / 1_000.0 / DAYS_PER_SAMPLE as f64;
    TimedRun {
        micros_per_day,
        trace: trace(&world, graph, &outcomes),
    }
}

fn median_mad(values: &mut [f64]) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let mut deviations: Vec<_> = values.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    (median, deviations[deviations.len() / 2])
}

#[derive(Default)]
struct OutcomeCounts {
    zero_target: u64,
    initiator_ineligible: u64,
    target_ineligible: u64,
    theft_failed: u64,
    applied_positive: u64,
    applied_zero: u64,
}

fn count_outcomes(groups: &[sim_model::SettlementTargetedResolution]) -> OutcomeCounts {
    let mut counts = OutcomeCounts::default();
    for resolution in groups.iter().flat_map(|group| &group.resolutions) {
        match resolution.outcome {
            TargetedOutcome::ZeroTarget => counts.zero_target += 1,
            TargetedOutcome::InitiatorIneligible => counts.initiator_ineligible += 1,
            TargetedOutcome::TargetIneligible => counts.target_ineligible += 1,
            TargetedOutcome::TheftFailed => counts.theft_failed += 1,
            TargetedOutcome::Applied { amount } if amount > 0.0 => counts.applied_positive += 1,
            TargetedOutcome::Applied { .. } => counts.applied_zero += 1,
        }
    }
    counts
}

struct DayProfile {
    day: u32,
    alive: usize,
    actions: [u64; Action::COUNT],
    give_some: u64,
    give_none: u64,
    steal_some: u64,
    steal_none: u64,
    cross: u64,
    keyed: u64,
    outcomes: OutcomeCounts,
    p1_us: f64,
    p2_us: f64,
    p3_us: f64,
    p4_select_us: f64,
    p4_reference_us: f64,
    p4_reference_mad: f64,
    p4_reduced_us: f64,
    p4_reduced_mad: f64,
    p4_saving_us: f64,
    p4_saving_mad: f64,
    p4_precheck_us: f64,
    p4_precheck_mad: f64,
    p5_us: f64,
    p6a_us: f64,
    p6b_us: [f64; 4],
    p6b_mad: [f64; 4],
}

fn profile_day(base: &SourceHybridWorldState, config: &SourceConfig) -> DayProfile {
    let mut world = base.world.clone();
    let mut storage = base
        .segmented_storage
        .as_ref()
        .expect("storage exists")
        .clone();
    let day = world.current_day;
    let mut effective = config.clone();
    effective.world.master_seed = SEED;
    effective.world.replicate_id = REPLICATE;

    let mut stage = Instant::now();
    phase1_resource_regrowth(&mut world, &effective);
    let p1_us = stage.elapsed().as_nanos() as f64 / 1_000.0;
    stage = Instant::now();
    storage.phase2_degradation_with_config(&effective);
    let p2_us = stage.elapsed().as_nanos() as f64 / 1_000.0;
    let mut features: Vec<AgentFeatures> = Vec::with_capacity(storage.len());
    let mut phase3 = Phase3ScarcityScratch::with_capacity(world.settlements.len());
    stage = Instant::now();
    phase3_observation_and_features_storage_with_scratch(
        &storage,
        &world.settlements,
        &effective,
        &mut features,
        &mut phase3,
    )
    .expect("Phase 3 succeeds");
    let p3_us = stage.elapsed().as_nanos() as f64 / 1_000.0;

    let mut choices: Vec<PrimaryActionChoice> = Vec::with_capacity(storage.len());
    stage = Instant::now();
    phase4_primary_action_selection_storage_into(
        &storage,
        day,
        &effective,
        &features,
        &mut choices,
    )
    .expect("Phase 4 action selection succeeds");
    let p4_select_us = stage.elapsed().as_nanos() as f64 / 1_000.0;
    let graph = source_graph_for(config, &storage);
    let projection = M3Phase4SlotProjection::new(&graph, &storage).expect("projection builds");
    let mut reference_intents = Vec::with_capacity(storage.len());
    let mut reduced_intents = Vec::with_capacity(storage.len());
    let mut candidates = Vec::with_capacity(8);
    let mut reduced_candidates = Vec::with_capacity(8);
    let mut reference_samples = Vec::with_capacity(SAMPLES);
    let mut reduced_samples = Vec::with_capacity(SAMPLES);
    let mut saving_samples = Vec::with_capacity(SAMPLES);
    let mut precheck_samples = Vec::with_capacity(SAMPLES);

    for sample in 0..(WARMUPS + SAMPLES) {
        let (ref_us, reduced_us, precheck_us) = if sample % 2 == 0 {
            let started = Instant::now();
            generate_intents_m3_storage_into(
                &storage,
                day,
                &effective,
                &choices,
                &projection,
                &mut candidates,
                &mut reference_intents,
            )
            .expect("reference Phase 4 intents succeed");
            let reference_us = started.elapsed().as_nanos() as f64 / 1_000.0;

            let reduced_started = Instant::now();
            let precheck_started = Instant::now();
            let eligibility =
                global_eligibility(&storage, effective.interaction.starvation_threshold);
            let precheck_us = precheck_started.elapsed().as_nanos() as f64 / 1_000.0;
            generate_intents_m3_storage_with_global_eligibility_into(
                &storage,
                day,
                &effective,
                &choices,
                &projection,
                &mut reduced_candidates,
                eligibility,
                &mut reduced_intents,
            )
            .expect("reduced Phase 4 intents succeed");
            (
                reference_us,
                reduced_started.elapsed().as_nanos() as f64 / 1_000.0,
                precheck_us,
            )
        } else {
            let reduced_started = Instant::now();
            let precheck_started = Instant::now();
            let eligibility =
                global_eligibility(&storage, effective.interaction.starvation_threshold);
            let precheck_us = precheck_started.elapsed().as_nanos() as f64 / 1_000.0;
            generate_intents_m3_storage_with_global_eligibility_into(
                &storage,
                day,
                &effective,
                &choices,
                &projection,
                &mut reduced_candidates,
                eligibility,
                &mut reduced_intents,
            )
            .expect("reduced Phase 4 intents succeed");
            let reduced_us = reduced_started.elapsed().as_nanos() as f64 / 1_000.0;
            let reference_started = Instant::now();
            generate_intents_m3_storage_into(
                &storage,
                day,
                &effective,
                &choices,
                &projection,
                &mut candidates,
                &mut reference_intents,
            )
            .expect("reference Phase 4 intents succeed");
            (
                reference_started.elapsed().as_nanos() as f64 / 1_000.0,
                reduced_us,
                precheck_us,
            )
        };
        assert_eq!(reference_intents, reduced_intents);
        if sample >= WARMUPS {
            reference_samples.push(ref_us);
            reduced_samples.push(reduced_us);
            saving_samples.push(ref_us - reduced_us);
            precheck_samples.push(precheck_us);
        }
    }
    let (p4_reference_us, p4_reference_mad) = median_mad(&mut reference_samples);
    let (p4_reduced_us, p4_reduced_mad) = median_mad(&mut reduced_samples);
    let (p4_saving_us, p4_saving_mad) = median_mad(&mut saving_samples);
    let (p4_precheck_us, p4_precheck_mad) = median_mad(&mut precheck_samples);

    let mut actions = [0; Action::COUNT];
    for choice in &choices {
        actions[choice.action.index()] += 1;
    }
    let mut give_some = 0;
    let mut give_none = 0;
    let mut steal_some = 0;
    let mut steal_none = 0;
    for intent in &reference_intents {
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
    let cross = sim_model::intents::count_cross_partition_interactions_storage_per_day(
        &reference_intents,
        &storage,
    )
    .expect("cross-partition count succeeds");

    let mut phase5 = Phase5PartitionScratch::with_capacity(world.settlements.len());
    let mut partition_intents = reference_intents;
    stage = Instant::now();
    let partitions =
        phase5_partition_intents_from_vec_with_scratch(&mut partition_intents, &mut phase5)
            .expect("Phase 5 succeeds");
    let p5_us = stage.elapsed().as_nanos() as f64 / 1_000.0;
    let mut settlements = world.settlements.clone();
    stage = Instant::now();
    let _work = phase6a_work_resolution_storage(&mut storage, &mut settlements, &partitions)
        .expect("Phase 6A succeeds");
    let p6a_us = stage.elapsed().as_nanos() as f64 / 1_000.0;
    let mut phase6b_scratch = M3Phase6BStorageScratch::with_capacity(storage.len());
    let mut keyed_count = 0;
    let mut outcomes = OutcomeCounts::default();
    let mut p6b_samples: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for sample in 0..(WARMUPS + SAMPLES) {
        let mut commit_storage = storage.clone();
        let (resolutions, timings) = phase6b_targeted_resolution_storage_m3_with_timing(
            &mut commit_storage,
            &settlements,
            day,
            &effective,
            &partitions,
            &mut phase6b_scratch,
        )
        .expect("Phase 6B timing succeeds");
        if sample == 0 {
            keyed_count = resolutions
                .iter()
                .map(|group| group.keyed_stream.len() as u64)
                .sum();
            outcomes = count_outcomes(&resolutions);
        }
        if sample >= WARMUPS {
            p6b_samples[0].push(timings.validation_slot_lookup.as_nanos() as f64 / 1_000.0);
            p6b_samples[1].push(timings.key_generation.as_nanos() as f64 / 1_000.0);
            p6b_samples[2].push(timings.global_sort.as_nanos() as f64 / 1_000.0);
            p6b_samples[3].push(timings.evaluation_writeback_output.as_nanos() as f64 / 1_000.0);
        }
    }
    let p6b_stats: [(f64, f64); 4] = std::array::from_fn(|i| median_mad(&mut p6b_samples[i]));
    let p6b_us = p6b_stats.map(|stats| stats.0);
    let p6b_mad = p6b_stats.map(|stats| stats.1);

    DayProfile {
        day: day.as_u32(),
        alive: storage.alive().iter().filter(|&&alive| alive).count(),
        actions,
        give_some,
        give_none,
        steal_some,
        steal_none,
        cross,
        keyed: keyed_count,
        outcomes,
        p1_us,
        p2_us,
        p3_us,
        p4_select_us,
        p4_reference_us,
        p4_reference_mad,
        p4_reduced_us,
        p4_reduced_mad,
        p4_saving_us,
        p4_saving_mad,
        p4_precheck_us,
        p4_precheck_mad,
        p5_us,
        p6a_us,
        p6b_us,
        p6b_mad,
    }
}

fn global_eligibility(
    storage: &SourceStorage,
    starvation_threshold: f32,
) -> M3Phase4GlobalEligibility {
    let mut result = M3Phase4GlobalEligibility::default();
    for slot in 0..storage.len() {
        if storage.alive()[slot] && storage.health()[slot] > 0.0 {
            result.give_food |= storage.food()[slot] < starvation_threshold;
            result.steal_food |= storage.food()[slot] > 0.0;
            if result.give_food && result.steal_food {
                break;
            }
        }
    }
    result
}

fn write_json(writer: &mut BufWriter<File>, cell: Cell, profile: &DayProfile) {
    let actions = profile
        .actions
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let stages = format!(
        "{{\"phase1\":{:.3},\"phase2\":{:.3},\"phase3\":{:.3},\"phase4_selection\":{:.3},\"phase4_reference_intents\":{:.3},\"phase4_reduced_intents\":{:.3},\"phase4_paired_saving\":{:.3},\"phase4_global_precheck\":{:.3},\"phase5\":{:.3},\"phase6a\":{:.3},\"phase6b_validation_slot\":{:.3},\"phase6b_key_generation\":{:.3},\"phase6b_sort\":{:.3},\"phase6b_commit_output\":{:.3},\"phase7_11\":null}}",
        profile.p1_us,
        profile.p2_us,
        profile.p3_us,
        profile.p4_select_us,
        profile.p4_reference_us,
        profile.p4_reduced_us,
        profile.p4_saving_us,
        profile.p4_precheck_us,
        profile.p5_us,
        profile.p6a_us,
        profile.p6b_us[0],
        profile.p6b_us[1],
        profile.p6b_us[2],
        profile.p6b_us[3],
    );
    let stage_mads = format!(
        "{{\"phase4_reference_intents\":{:.3},\"phase4_reduced_intents\":{:.3},\"phase4_paired_saving\":{:.3},\"phase4_global_precheck\":{:.3},\"phase6b_validation_slot\":{:.3},\"phase6b_key_generation\":{:.3},\"phase6b_sort\":{:.3},\"phase6b_commit_output\":{:.3}}}",
        profile.p4_reference_mad,
        profile.p4_reduced_mad,
        profile.p4_saving_mad,
        profile.p4_precheck_mad,
        profile.p6b_mad[0],
        profile.p6b_mad[1],
        profile.p6b_mad[2],
        profile.p6b_mad[3],
    );
    writeln!(
        writer,
        "{{\"N\":{},\"K\":{},\"k\":8,\"p_local\":{:.2},\"profile\":\"{}\",\"day\":{},\"alive\":{},\"action_counts\":[{}],\"give_some\":{},\"give_none\":{},\"steal_some\":{},\"steal_none\":{},\"outcomes\":{{\"zero_target\":{},\"initiator_ineligible\":{},\"target_ineligible\":{},\"theft_failed\":{},\"applied_positive\":{},\"applied_zero\":{}}},\"keyed\":{},\"cross_partition\":{},\"level_count\":null,\"max_level_size\":null,\"stage_times_us\":{},\"stage_mad_us\":{}}}",
        cell.population,
        cell.settlements,
        cell.p_local,
        if cell.horizon == 150 { "sustained" } else { "m2-40" },
        profile.day,
        profile.alive,
        actions,
        profile.give_some,
        profile.give_none,
        profile.steal_some,
        profile.steal_none,
        profile.outcomes.zero_target,
        profile.outcomes.initiator_ineligible,
        profile.outcomes.target_ineligible,
        profile.outcomes.theft_failed,
        profile.outcomes.applied_positive,
        profile.outcomes.applied_zero,
        profile.keyed,
        profile.cross,
        stages,
        stage_mads,
    )
    .expect("write JSONL record");
}

fn measure_cell(cell: Cell, config: &runtime::SimConfig, writer: &mut BufWriter<File>) {
    let base = build_horizon(config, cell.horizon);
    let graph = graph_for(config, base.storage().expect("storage exists"));
    let mut reference = Vec::with_capacity(SAMPLES);
    let mut reduced = Vec::with_capacity(SAMPLES);
    let mut savings = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let (left, right) = if sample % 2 == 0 {
            (
                run_sample(&base, config, &graph, false),
                run_sample(&base, config, &graph, true),
            )
        } else {
            let optimized = run_sample(&base, config, &graph, true);
            let serial = run_sample(&base, config, &graph, false);
            (serial, optimized)
        };
        assert_eq!(left.trace, right.trace);
        if sample >= WARMUPS {
            reference.push(left.micros_per_day);
            reduced.push(right.micros_per_day);
            savings.push(left.micros_per_day - right.micros_per_day);
        }
    }
    let (reference_us, reference_mad) = median_mad(&mut reference);
    let (reduced_us, reduced_mad) = median_mad(&mut reduced);
    let (saving_us, saving_mad) = median_mad(&mut savings);
    println!(
        "M3_02B_TICK|profile={}|horizon={}|N={}|K={}|p={:.1}|serial={reference_us:.3}|{reference_mad:.3}|search={reduced_us:.3}|{reduced_mad:.3}|paired_saving={saving_us:.3}|{saving_mad:.3}",
        if cell.horizon == 150 {
            "sustained"
        } else {
            "m2-40"
        },
        cell.horizon,
        cell.population,
        cell.settlements,
        cell.p_local
    );
    let profile_config = source_config_for(cell);
    let profile_base = build_source_horizon(&profile_config, cell.horizon);
    let profile = profile_day(&profile_base, &profile_config);
    println!(
        "M3_02B_PHASE|profile={}|N={}|p={:.1}|day={}|P4_reference={:.3}|{:.3}|P4_reduced={:.3}|{:.3}|P4_saving={:.3}|{:.3}|precheck={:.3}|{:.3}",
        if cell.horizon == 150 {
            "sustained"
        } else {
            "m2-40"
        },
        cell.population,
        cell.p_local,
        profile.day,
        profile.p4_reference_us,
        profile.p4_reference_mad,
        profile.p4_reduced_us,
        profile.p4_reduced_mad,
        profile.p4_saving_us,
        profile.p4_saving_mad,
        profile.p4_precheck_us,
        profile.p4_precheck_mad,
    );
    println!(
        "M3_02B_RESOLVER|profile={}|N={}|p={:.1}|day={}|validation={:.3}|{:.3}|key={:.3}|{:.3}|sort={:.3}|{:.3}|commit_output={:.3}|{:.3}",
        if cell.horizon == 150 {
            "sustained"
        } else {
            "m2-40"
        },
        cell.population,
        cell.p_local,
        profile.day,
        profile.p6b_us[0],
        profile.p6b_mad[0],
        profile.p6b_us[1],
        profile.p6b_mad[1],
        profile.p6b_us[2],
        profile.p6b_mad[2],
        profile.p6b_us[3],
        profile.p6b_mad[3],
    );
    println!(
        "M3_02B_WORK|profile={}|horizon={}|N={}|K={}|p={:.1}|day={}|alive={}|keyed={}|cross={}|give={}/{}|steal={}/{}|applied={}|theft_failed={}",
        if cell.horizon == 150 {
            "sustained"
        } else {
            "m2-40"
        },
        cell.horizon,
        cell.population,
        cell.settlements,
        cell.p_local,
        profile.day,
        profile.alive,
        profile.keyed,
        profile.cross,
        profile.give_some,
        profile.give_none,
        profile.steal_some,
        profile.steal_none,
        profile.outcomes.applied_positive,
        profile.outcomes.theft_failed
    );
    write_json(writer, cell, &profile);
}

fn print_host() {
    let logical = std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1);
    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into());
    println!(
        "HOST|os={}|arch={}|logical_processors={logical}|processor_identifier={}|rustc={rustc}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "record-separately".into())
    );
}

fn main() {
    let total = Instant::now();
    print_host();
    println!(
        "METHOD|whole_tick=untimed public runners|warmups=2|samples=11|paired=interleaved|days_per_sample=3|metrics=on|events=on|snapshots=off|stage=opt-in Phase 4/6B hooks plus bench-local direct eligibility timing"
    );
    let mut output = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    output.push("../../target/m3_02b_search_reduction.jsonl");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create target directory");
    }
    let mut writer = BufWriter::new(File::create(&output).expect("create per-day JSONL"));
    for (population, settlements) in [(10_000, 50), (100_000, 500)] {
        for p_local in [0.0, 0.5, 1.0] {
            for horizon in [0, 150] {
                let cell = Cell {
                    population,
                    settlements,
                    p_local,
                    horizon,
                };
                let config = config_for(cell);
                measure_cell(cell, &config, &mut writer);
            }
        }
    }
    writer.flush().expect("flush JSONL");
    println!("JSONL|{}", output.display());
    println!("BENCH_SECONDS|{:.3}", total.elapsed().as_secs_f64());
}
