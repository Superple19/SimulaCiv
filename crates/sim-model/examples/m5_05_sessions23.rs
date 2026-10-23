use rayon::{ThreadPool, ThreadPoolBuilder};
use serde_json::{Value, json};
use sha2::Digest;
use sim_core::{AgentId, SimulationDay};
use sim_model::decision::{PrimaryActionChoice, phase4_primary_action_selection_storage_into};
use sim_model::features::{AgentFeatures, phase3_observation_and_features_storage};
use sim_model::hashing::{
    canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash_from_storage,
    canonical_m3p_event_hash, canonical_m3p_metrics_hash, canonical_m3p_state_hash_from_storage,
    canonical_m3prop_event_hash, canonical_m3prop_metrics_hash,
    canonical_m3prop_state_hash_from_storage,
};
use sim_model::propagation::{
    PropagationAgentState, apply_phase9c_storage, apply_phase9c_storage_parallel,
};
use sim_model::relationship_graph::RelationshipGraph;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3PropagationStorageRunContext, M3StorageRunContext,
    M3pIncrementalTopologyStorageRunContext, M3pStorageRunContext, run_m3_storage_days,
    run_m3p_storage_days, run_m3p_storage_days_incremental_topology, run_m3prop_storage_days,
    run_m3prop_storage_days_with_parallel_spread,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{M3pReferenceState, SimConfig, decode_m3p_snapshot, initialize_world};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const M3_EQUILIBRIUM: &str = "../../docs/reproducibility/m5_05_configs/m3_equilibrium.toml";
const M3_PROPAGATION: &str =
    "../../docs/reproducibility/m5_05_configs/m3_equilibrium_propagation.toml";
const M3P_SUSTAINED: &str = "tests/fixtures/m3p_propagation_sustained.toml";
const SAMPLES: usize = 41;
const WARMUPS: usize = 2;
const CALIBRATION_PROBES: usize = 3;
const TARGET_SAMPLE_SECONDS: f64 = 1.5;
const MIN_SAMPLE_SECONDS: f64 = 1.0;
const DAYS: u32 = 3;
const CHUNK: NonZeroUsize = NonZeroUsize::new(256).expect("nonzero chunk");
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;

#[path = "common/process_controls.rs"]
mod process_controls;
const SUPPORTED_ENTRIES: &[&str] = &[
    "m3_06_settlement_index",
    "m3_07_reversed_slots",
    "m3_08_full_rebuild_equal_live",
    "m3_09_incremental_equal_live",
    "m3_11_phase9c_speedup",
    "m3_11_whole_tick_speedup",
    "m3_11_frontier_crossover",
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bundle {
    state: String,
    metrics: String,
    events: String,
}

#[derive(Clone, Debug)]
struct Stats {
    median: f64,
    mad: f64,
    samples: Vec<f64>,
    repetitions: usize,
    min_sample_seconds: f64,
}

#[derive(Clone, Debug)]
struct PairStats {
    left: Stats,
    right: Stats,
    delta_median: f64,
    delta_mad: f64,
    delta_samples: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Settings {
    samples: usize,
    warmups: usize,
    target_seconds: f64,
    require_min_sample: bool,
}

const A5: Settings = Settings {
    samples: SAMPLES,
    warmups: WARMUPS,
    target_seconds: TARGET_SAMPLE_SECONDS,
    require_min_sample: true,
};
const SMOKE: Settings = Settings {
    samples: 1,
    warmups: 0,
    target_seconds: 0.0,
    require_min_sample: false,
};

fn path(relative: &str) -> PathBuf {
    Path::new(ROOT).join(relative)
}

fn load_config(relative: &str, population: u64, settlements: u32) -> SimConfig {
    let mut value: toml::Value =
        toml::from_str(&fs::read_to_string(path(relative)).expect("read registered config"))
            .expect("registered config TOML parses");
    value["world"]["initial_population"] = toml::Value::Integer(population as i64);
    value["world"]["settlement_count"] = toml::Value::Integer(i64::from(settlements));
    value["world"]["master_seed"] = toml::Value::Integer(SEED as i64);
    value["world"]["replicate_id"] = toml::Value::Integer(i64::from(REPLICATE));
    let encoded = toml::to_string(&value).expect("benchmark config serializes");
    SimConfig::parse_and_validate(&encoded).expect("benchmark config validates")
}

fn load_m3_config(relative: &str, population: u64, settlements: u32, degree: u32) -> SimConfig {
    let mut config = load_config(relative, population, settlements);
    config.interaction.relationship_degree_k = Some(i64::from(degree));
    config.interaction.p_local = Some(0.5);
    config.validate().expect("M3 benchmark config validates");
    config
}

fn load_m3prop_config(
    population: u64,
    settlements: u32,
    degree: u32,
    max_hops: u32,
    transmit: f32,
) -> SimConfig {
    let mut value: toml::Value =
        toml::from_str(&fs::read_to_string(path(M3_PROPAGATION)).expect("read propagation config"))
            .expect("propagation config parses");
    value["world"]["initial_population"] = toml::Value::Integer(population as i64);
    value["world"]["settlement_count"] = toml::Value::Integer(i64::from(settlements));
    value["interaction"]["relationship_degree_k"] = toml::Value::Integer(i64::from(degree));
    value["interaction"]["p_local"] = toml::Value::Float(0.5);
    value["propagation"]["max_hops"] = toml::Value::Integer(i64::from(max_hops));
    value["propagation"]["transmit_probability"] = toml::Value::Float(f64::from(transmit));
    let encoded = toml::to_string(&value).expect("propagation config serializes");
    SimConfig::parse_and_validate(&encoded).expect("propagation config validates")
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn apply_windows_controls() -> Value {
    let status = process_controls::apply_high_priority_affinity();
    if cfg!(windows) {
        assert!(
            status.priority_set && status.affinity_set,
            "A-5 priority/affinity setup failed"
        );
        json!({"priority":"high", "affinity_mask":"0x555"})
    } else {
        json!({"priority":"not-applied", "affinity_mask":"not-applied"})
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn summarize(values: Vec<f64>, repetitions: usize, min_sample_seconds: f64) -> Stats {
    let center = median(&values);
    let mad = median(
        &values
            .iter()
            .map(|value| (value - center).abs())
            .collect::<Vec<_>>(),
    );
    Stats {
        median: center,
        mad,
        samples: values,
        repetitions,
        min_sample_seconds,
    }
}

fn measure_single<F>(mut run: F, settings: Settings, per_operation_divisor: f64) -> Stats
where
    F: FnMut() -> f64,
{
    let repetitions = if settings.target_seconds == 0.0 {
        1
    } else {
        let probes = (0..CALIBRATION_PROBES)
            .map(|_| run() / 1_000_000.0)
            .collect::<Vec<_>>();
        (settings.target_seconds / median(&probes)).ceil().max(1.0) as usize
    };
    for _ in 0..settings.warmups {
        let elapsed = (0..repetitions).map(|_| run()).sum::<f64>() / 1_000_000.0;
        if settings.require_min_sample {
            assert!(elapsed >= MIN_SAMPLE_SECONDS, "A-5 warmup below one second");
        }
    }
    let mut samples = Vec::with_capacity(settings.samples);
    let mut minimum = f64::INFINITY;
    for _ in 0..settings.samples {
        let elapsed_us = (0..repetitions).map(|_| run()).sum::<f64>();
        let elapsed = elapsed_us / 1_000_000.0;
        minimum = minimum.min(elapsed);
        if settings.require_min_sample {
            assert!(elapsed >= MIN_SAMPLE_SECONDS, "A-5 sample below one second");
        }
        samples.push(elapsed_us / repetitions as f64 / per_operation_divisor);
    }
    summarize(samples, repetitions, minimum)
}

fn measure_pair<L, R>(
    mut left: L,
    mut right: R,
    settings: Settings,
    per_operation_divisor: f64,
) -> PairStats
where
    L: FnMut() -> f64,
    R: FnMut() -> f64,
{
    let mut one_pair = |right_first: bool| {
        if right_first {
            let r = right();
            let l = left();
            (l, r)
        } else {
            let l = left();
            let r = right();
            (l, r)
        }
    };
    let repetitions = if settings.target_seconds == 0.0 {
        1
    } else {
        let probes = (0..CALIBRATION_PROBES)
            .map(|index| {
                let (left_us, right_us) = one_pair(index % 2 == 1);
                (left_us + right_us) / 1_000_000.0
            })
            .collect::<Vec<_>>();
        (settings.target_seconds / median(&probes)).ceil().max(1.0) as usize
    };
    for warmup in 0..settings.warmups {
        let wall = (0..repetitions)
            .map(|repeat| {
                let (left_us, right_us) = one_pair((warmup + repeat) % 2 == 1);
                left_us + right_us
            })
            .sum::<f64>()
            / 1_000_000.0;
        if settings.require_min_sample {
            assert!(
                wall >= MIN_SAMPLE_SECONDS,
                "A-5 paired warmup below one second"
            );
        }
    }
    let mut left_samples = Vec::with_capacity(settings.samples);
    let mut right_samples = Vec::with_capacity(settings.samples);
    let mut delta_samples = Vec::with_capacity(settings.samples);
    let mut minimum = f64::INFINITY;
    for sample in 0..settings.samples {
        let mut left_sum = 0.0;
        let mut right_sum = 0.0;
        for repeat in 0..repetitions {
            let (l, r) = one_pair((sample + repeat) % 2 == 1);
            left_sum += l;
            right_sum += r;
        }
        let wall = (left_sum + right_sum) / 1_000_000.0;
        minimum = minimum.min(wall);
        if settings.require_min_sample {
            assert!(
                wall >= MIN_SAMPLE_SECONDS,
                "A-5 paired sample below one second"
            );
        }
        let divisor = repetitions as f64 * per_operation_divisor;
        left_samples.push(left_sum / divisor);
        right_samples.push(right_sum / divisor);
        delta_samples.push((left_sum - right_sum) / divisor);
    }
    let left = summarize(left_samples, repetitions, minimum);
    let right = summarize(right_samples, repetitions, minimum);
    let delta_median = median(&delta_samples);
    let delta_mad = median(
        &delta_samples
            .iter()
            .map(|value| (value - delta_median).abs())
            .collect::<Vec<_>>(),
    );
    PairStats {
        left,
        right,
        delta_median,
        delta_mad,
        delta_samples,
    }
}

fn measure_pair_sample_blocked<L, R>(
    mut left: L,
    mut right: R,
    settings: Settings,
    per_operation_divisor: f64,
) -> PairStats
where
    L: FnMut() -> f64,
    R: FnMut() -> f64,
{
    let probes = (0..CALIBRATION_PROBES)
        .map(|index| {
            let (l, r) = if index % 2 == 0 {
                (left(), right())
            } else {
                let r = right();
                let l = left();
                (l, r)
            };
            (l + r) / 1_000_000.0
        })
        .collect::<Vec<_>>();
    let repetitions = if settings.target_seconds == 0.0 {
        1
    } else {
        (settings.target_seconds / median(&probes)).ceil().max(1.0) as usize
    };
    for warmup in 0..settings.warmups {
        let (l, r) = if warmup % 2 == 0 {
            (
                (0..repetitions).map(|_| left()).sum::<f64>(),
                (0..repetitions).map(|_| right()).sum::<f64>(),
            )
        } else {
            let r = (0..repetitions).map(|_| right()).sum::<f64>();
            let l = (0..repetitions).map(|_| left()).sum::<f64>();
            (l, r)
        };
        if settings.require_min_sample {
            assert!(
                (l + r) / 1_000_000.0 >= MIN_SAMPLE_SECONDS,
                "A-5 blocked warmup below one second"
            );
        }
    }
    let mut left_samples = Vec::with_capacity(settings.samples);
    let mut right_samples = Vec::with_capacity(settings.samples);
    let mut delta_samples = Vec::with_capacity(settings.samples);
    let mut minimum = f64::INFINITY;
    for sample in 0..settings.samples {
        let (left_us, right_us) = if sample % 2 == 0 {
            (
                (0..repetitions).map(|_| left()).sum::<f64>(),
                (0..repetitions).map(|_| right()).sum::<f64>(),
            )
        } else {
            let right_us = (0..repetitions).map(|_| right()).sum::<f64>();
            let left_us = (0..repetitions).map(|_| left()).sum::<f64>();
            (left_us, right_us)
        };
        let wall = (left_us + right_us) / 1_000_000.0;
        minimum = minimum.min(wall);
        if settings.require_min_sample {
            assert!(
                wall >= MIN_SAMPLE_SECONDS,
                "A-5 blocked sample below one second"
            );
        }
        let divisor = repetitions as f64 * per_operation_divisor;
        left_samples.push(left_us / divisor);
        right_samples.push(right_us / divisor);
        delta_samples.push((left_us - right_us) / divisor);
    }
    let left = summarize(left_samples, repetitions, minimum);
    let right = summarize(right_samples, repetitions, minimum);
    let delta_median = median(&delta_samples);
    let delta_mad = median(
        &delta_samples
            .iter()
            .map(|value| (value - delta_median).abs())
            .collect::<Vec<_>>(),
    );
    PairStats {
        left,
        right,
        delta_median,
        delta_mad,
        delta_samples,
    }
}

fn stats_value(stats: &Stats) -> Value {
    json!({
        "median_us": stats.median,
        "mad_us": stats.mad,
        "samples_us": stats.samples,
        "repetitions_per_sample": stats.repetitions,
        "minimum_sample_seconds": stats.min_sample_seconds,
    })
}

fn pair_result(stats: &PairStats, left_mode: &str, right_mode: &str) -> Value {
    json!({
        "warmups": WARMUPS,
        "samples": SAMPLES,
        "repetitions_per_sample": stats.left.repetitions,
        "days_per_operation": DAYS,
        "time_unit": "us/tick",
        "left_mode": left_mode,
        "right_mode": right_mode,
        "left_median_us": stats.left.median,
        "left_mad_us": stats.left.mad,
        "right_median_us": stats.right.median,
        "right_mad_us": stats.right.mad,
        "speedup_left_over_right": stats.left.median / stats.right.median,
        "paired_left_minus_right_median_us": stats.delta_median,
        "paired_left_minus_right_mad_us": stats.delta_mad,
        "paired_delta_samples_us": stats.delta_samples,
        "left_samples_us": stats.left.samples,
        "right_samples_us": stats.right.samples,
        "minimum_sample_seconds": stats.left.min_sample_seconds,
        "execution_options": {"metrics":true,"events":true,"snapshots":false,"days":DAYS,"pool_owned_by_caller":true,"chunk_size":256}
    })
}

fn bundle_m3(
    hybrid: &HybridWorldState,
    outcomes: &[sim_model::M3DayOutcome],
    graph: &RelationshipGraph,
) -> Bundle {
    let storage = hybrid.storage().expect("hybrid storage");
    Bundle {
        state: canonical_m3_state_hash_from_storage(
            storage,
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
        )
        .expect("M3 State digest")
        .to_hex(),
        metrics: canonical_m3_metrics_hash(
            &outcomes
                .iter()
                .map(|day| day.metrics.clone().expect("metrics enabled"))
                .collect::<Vec<_>>(),
        )
        .expect("M3 Metrics digest")
        .to_hex(),
        events: canonical_event_hash(
            &outcomes
                .iter()
                .flat_map(|day| day.events.iter().cloned())
                .collect::<Vec<_>>(),
        )
        .expect("Events digest")
        .to_hex(),
    }
}

fn run_m3_ticks(config: &SimConfig) -> (f64, Bundle) {
    let world = initialize_world(config).expect("M3 world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3StorageRunContext::new(&hybrid, config, &run_context).expect("M3 context");
    let graph = context.relationship_graph().clone();
    let started = Instant::now();
    let outcomes = run_m3_storage_days(&mut hybrid, config, &mut context, DAYS, &options(false))
        .expect("M3 runner");
    let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
    (elapsed, bundle_m3(&hybrid, &outcomes, &graph))
}

fn m3_06(settings: Settings, smoke: bool) -> Value {
    let n = if smoke { 300 } else { 1_000_000 };
    let k = if smoke { 3 } else { 5_000 };
    let config = load_m3_config(M3_EQUILIBRIUM, n, k, 8);
    let mut expected_digest: Option<Bundle> = None;
    let stats = measure_single(
        || {
            let (elapsed, digest) = run_m3_ticks(&config);
            if let Some(expected) = &expected_digest {
                assert_eq!(expected, &digest, "repeated indexed M3 digest parity");
            } else {
                expected_digest = Some(digest);
            }
            elapsed
        },
        settings,
        f64::from(DAYS),
    );
    let current_digest = expected_digest.expect("indexed M3 digest captured");
    json!({
        "cell":"M3-indexed-N1000000-K5000-k8-p0.5-day0",
        "N":n,"K":k,"k":8,"p_local":0.5,
        "left_mode":"historical pre-index source path unavailable",
        "right_mode":"current indexed M3 serial storage",
        "result":{
            "left_median_us":Value::Null,"left_mad_us":Value::Null,
            "right_median_us":stats.median,"right_mad_us":stats.mad,
            "single_path":stats_value(&stats),"ratio":Value::Null,
            "current_digest":{"state":current_digest.state,"metrics":current_digest.metrics,"events":current_digest.events},
            "old_path_status":"unrecoverable: pre-index source implementation is absent",
            "time_unit":"us/tick","days_per_operation":DAYS,"execution_options":{"metrics":true,"events":true,"snapshots":false,"profile":"m3","runner":"serial storage"}
        }
    })
}

struct SelectionPath {
    config: SimConfig,
    storage: SegmentedAgentStorage,
    features: Vec<AgentFeatures>,
    choices: Vec<PrimaryActionChoice>,
}

fn selection_path(config: &SimConfig, reversed: bool) -> SelectionPath {
    let mut world = initialize_world(config).expect("selection world initializes");
    if reversed {
        world.agents.reverse();
    }
    let hybrid = HybridWorldState::hybrid(world);
    let storage = hybrid.storage().expect("selection storage").clone();
    let mut features = Vec::with_capacity(storage.len());
    phase3_observation_and_features_storage(
        &storage,
        &hybrid.world.settlements,
        config,
        &mut features,
    )
    .expect("Phase 3 features");
    let choice_capacity = storage.len();
    SelectionPath {
        config: config.clone(),
        storage,
        features,
        choices: Vec::with_capacity(choice_capacity),
    }
}

fn time_selection(path: &mut SelectionPath) -> f64 {
    let started = Instant::now();
    phase4_primary_action_selection_storage_into(
        &path.storage,
        SimulationDay(0),
        &path.config,
        &path.features,
        &mut path.choices,
    )
    .expect("Phase 4 selection");
    started.elapsed().as_secs_f64() * 1_000_000.0
}

fn m3_layout_digest(config: &SimConfig, reversed: bool) -> Bundle {
    let mut world = initialize_world(config).expect("M3 slot-layout world initializes");
    if reversed {
        world.agents.reverse();
    }
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context =
        M3StorageRunContext::new(&hybrid, config, &run_context).expect("M3 layout context");
    let graph = context.relationship_graph().clone();
    let outcomes = run_m3_storage_days(&mut hybrid, config, &mut context, 1, &options(false))
        .expect("M3 layout parity day");
    bundle_m3(&hybrid, &outcomes, &graph)
}

fn m3_07(settings: Settings, smoke: bool) -> Value {
    let n = if smoke { 300 } else { 1_000_000 };
    let k = if smoke { 3 } else { 5_000 };
    let config = load_m3_config(M3_EQUILIBRIUM, n, k, 8);
    let mut normal = selection_path(&config, false);
    let mut reversed = selection_path(&config, true);
    time_selection(&mut normal);
    time_selection(&mut reversed);
    assert_eq!(
        normal.features, reversed.features,
        "feature parity across slot layouts"
    );
    assert_eq!(
        normal.choices, reversed.choices,
        "Phase 4 choice parity across slot layouts"
    );
    let normal_digest = m3_layout_digest(&config, false);
    let reversed_digest = m3_layout_digest(&config, true);
    assert_eq!(
        normal_digest, reversed_digest,
        "M3 canonical digests across slot layouts"
    );
    let measured = measure_pair(
        || time_selection(&mut normal),
        || time_selection(&mut reversed),
        settings,
        1.0,
    );
    json!({
        "cell":"P4-selection-N1000000-K5000-k8-p0.5",
        "N":n,"K":k,"k":8,"p_local":0.5,
        "left_mode":"normal slots","right_mode":"reversed slots",
        "result":{
            "left_median_us":measured.left.median,"left_mad_us":measured.left.mad,
            "right_median_us":measured.right.median,"right_mad_us":measured.right.mad,
            "speedup_left_over_right":measured.left.median/measured.right.median,
            "reverse_over_normal":measured.right.median/measured.left.median,
            "paired_left_minus_right_median_us":measured.delta_median,
            "paired_left_minus_right_mad_us":measured.delta_mad,
            "left_samples_us":measured.left.samples,"right_samples_us":measured.right.samples,
            "repetitions_per_sample":measured.left.repetitions,
            "parity":{"features":true,"choices":true},
            "canonical_digests":{"state":normal_digest.state,"metrics":normal_digest.metrics,"events":normal_digest.events},
            "time_unit":"us/Phase4-selection","execution_options":{"profile":"m3","phase":"P4 action selection","input_state":"same logical day-0 world","metrics":false,"events":false,"snapshots":false}
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Trace {
    state: String,
    metrics: String,
    events: String,
}

fn m3p_sustained_config(population: u64, settlements: u32, smoke: bool) -> SimConfig {
    let mut value: toml::Value = toml::from_str(
        &fs::read_to_string(path(M3P_SUSTAINED)).expect("read frozen sustained config"),
    )
    .expect("M3P config parses");
    value["world"]["initial_population"] = toml::Value::Integer(population as i64);
    value["world"]["settlement_count"] = toml::Value::Integer(i64::from(settlements));
    // M3-08/09 equal-live comparisons predate the separate propagation overlay.
    value["simulation"]["systems"]["propagation"] = toml::Value::Boolean(false);
    value
        .as_table_mut()
        .expect("config table")
        .remove("propagation");
    if smoke {
        value["population"]["compaction_interval_days"] = toml::Value::Integer(5);
    }
    SimConfig::parse_and_validate(&toml::to_string(&value).expect("M3P config serializes"))
        .expect("M3P sustained config validates")
}

fn m3p_checkpoint(config: &SimConfig, smoke: bool) -> M3pReferenceState {
    let world = initialize_world(config).expect("M3P world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context =
        M3pStorageRunContext::new(&hybrid, config, &run_context).expect("M3P context");
    let warm_days = if smoke { 5 } else { 399 };
    let _ = run_m3p_storage_days(
        &mut hybrid,
        config,
        &mut context,
        warm_days,
        &options(false),
    )
    .expect("M3P checkpoint warm-up");
    let last = run_m3p_storage_days(&mut hybrid, config, &mut context, 1, &options(true))
        .expect("M3P checkpoint boundary");
    let snapshot = last
        .last()
        .and_then(|day| day.snapshot.as_ref())
        .expect("M3P checkpoint snapshot");
    decode_m3p_snapshot(&snapshot.bytes, config)
        .expect("M3P snapshot restores")
        .state
}

fn m3p_trace(
    hybrid: &HybridWorldState,
    outcomes: &[sim_model::M3pDayOutcome],
    graph: &RelationshipGraph,
    next_agent_id: u64,
) -> Trace {
    let storage = hybrid.storage().expect("M3P storage");
    let metrics = outcomes
        .iter()
        .map(|day| day.metrics.clone().expect("metrics enabled"))
        .collect::<Vec<_>>();
    let events = outcomes
        .iter()
        .flat_map(|day| day.events.iter().cloned())
        .collect::<Vec<_>>();
    Trace {
        state: canonical_m3p_state_hash_from_storage(
            storage,
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
            next_agent_id,
        )
        .expect("M3P State digest")
        .to_hex(),
        metrics: canonical_m3p_metrics_hash(&metrics)
            .expect("M3P Metrics digest")
            .to_hex(),
        events: canonical_m3p_event_hash(&events)
            .expect("M3P Events digest")
            .to_hex(),
    }
}

fn run_m3p_ticks(
    config: &SimConfig,
    reference: &M3pReferenceState,
    incremental: bool,
) -> (f64, Trace) {
    let mut hybrid = HybridWorldState::hybrid(reference.world().clone());
    let run_context = M0RunContext::new(SEED, REPLICATE);
    if incremental {
        let mut context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
            &hybrid,
            config,
            &run_context,
            reference,
        )
        .expect("incremental checkpoint context");
        let started = Instant::now();
        let outcomes = run_m3p_storage_days_incremental_topology(
            &mut hybrid,
            config,
            &mut context,
            DAYS,
            &options(false),
        )
        .expect("incremental M3P days");
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        let trace = m3p_trace(
            &hybrid,
            &outcomes,
            context.relationship_graph(),
            context.next_agent_id(),
        );
        (elapsed, trace)
    } else {
        let mut context =
            M3pStorageRunContext::from_reference_state(&hybrid, config, &run_context, reference)
                .expect("full checkpoint context");
        let started = Instant::now();
        let outcomes =
            run_m3p_storage_days(&mut hybrid, config, &mut context, DAYS, &options(false))
                .expect("full M3P days");
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        let trace = m3p_trace(
            &hybrid,
            &outcomes,
            context.relationship_graph(),
            context.next_agent_id(),
        );
        (elapsed, trace)
    }
}

fn m3_config_from_m3p(population: u64, settlements: u32) -> SimConfig {
    let mut value: toml::Value = toml::from_str(
        &fs::read_to_string(path(M3P_SUSTAINED)).expect("read frozen sustained config"),
    )
    .expect("M3P config parses");
    value["profile"] = toml::Value::String("m3".into());
    value["simulation"]["systems"]["relationship_graph"] = toml::Value::Boolean(true);
    value["simulation"]["systems"]["population_dynamics"] = toml::Value::Boolean(false);
    value["simulation"]["systems"]["propagation"] = toml::Value::Boolean(false);
    value["world"]["initial_population"] = toml::Value::Integer(population as i64);
    value["world"]["settlement_count"] = toml::Value::Integer(i64::from(settlements));
    value
        .as_table_mut()
        .expect("config table")
        .remove("population");
    value
        .as_table_mut()
        .expect("config table")
        .remove("propagation");
    SimConfig::parse_and_validate(&toml::to_string(&value).expect("M3 config serializes"))
        .expect("M3 equal-live config validates")
}

fn run_m3_ticks_from_world(config: &SimConfig, world: WorldState) -> Bundle {
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context =
        M3StorageRunContext::new(&hybrid, config, &run_context).expect("M3 equal-live context");
    let graph = context.relationship_graph().clone();
    let outcomes = run_m3_storage_days(&mut hybrid, config, &mut context, DAYS, &options(false))
        .expect("M3 equal-live days");
    bundle_m3(&hybrid, &outcomes, &graph)
}

fn m3_equal_live_stats(
    config: &SimConfig,
    reference: &M3pReferenceState,
    incremental: bool,
    settings: Settings,
) -> (PairStats, Trace, Bundle, u64) {
    let mut live_world = reference.world().clone();
    live_world.agents.retain(|agent| agent.alive);
    let live_count = live_world.agents.len() as u64;
    let m3_config = m3_config_from_m3p(live_count, config.world.settlement_count);
    let left_options = (DAYS, true, true, false, 0_usize, CHUNK.get());
    let right_options = (DAYS, true, true, false, 0_usize, CHUNK.get());
    assert_eq!(
        left_options, right_options,
        "equal-live paths must match days, metrics, events, snapshots, pool, and chunk"
    );
    assert_eq!(config.world.master_seed, m3_config.world.master_seed);
    assert_eq!(config.world.replicate_id, m3_config.world.replicate_id);
    assert_eq!(
        config.interaction.relationship_degree_k,
        m3_config.interaction.relationship_degree_k
    );
    assert_eq!(config.interaction.p_local, m3_config.interaction.p_local);
    let (_, full_trace) = run_m3p_ticks(config, reference, false);
    let (_, incremental_trace) = run_m3p_ticks(config, reference, true);
    assert_eq!(
        full_trace, incremental_trace,
        "M3P full/incremental State/Metrics/Events parity before timing"
    );
    let m3_digest = run_m3_ticks_from_world(&m3_config, live_world.clone());
    let left = || run_m3p_ticks(config, reference, incremental).0;
    let right_world = live_world.clone();
    let right = || {
        let mut hybrid = HybridWorldState::hybrid(right_world.clone());
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let mut context = M3StorageRunContext::new(&hybrid, &m3_config, &run_context)
            .expect("M3 equal-live context");
        let start = Instant::now();
        let _ = run_m3_storage_days(&mut hybrid, &m3_config, &mut context, DAYS, &options(false))
            .expect("M3 equal-live days");
        start.elapsed().as_secs_f64() * 1_000_000.0
    };
    (
        measure_pair(left, right, settings, f64::from(DAYS)),
        full_trace,
        m3_digest,
        live_count,
    )
}

fn m3_08_or_09(entry: &str, settings: Settings, smoke: bool) -> Value {
    let n = if smoke { 300 } else { 100_000 };
    let k = if smoke { 3 } else { 500 };
    let config = m3p_sustained_config(n, k, smoke);
    let reference = m3p_checkpoint(&config, smoke);
    let incremental = entry == "m3_09_incremental_equal_live";
    let (measured, parity_digest, m3_digest, live) =
        m3_equal_live_stats(&config, &reference, incremental, settings);
    let ratio = measured.left.median / measured.right.median;
    json!({
        "cell":format!("M3P-equal-live-day400-initialN{n}-live{live}-k8-p0.5"),
        "N":n,"K":k,"k":8,"p_local":0.5,"live_population":live,
        "left_mode":if incremental {"M3P incremental topology"} else {"M3P full rebuild"},
        "right_mode":"M3 serial storage equal-live",
        "result":{
            "left_median_us":measured.left.median,"left_mad_us":measured.left.mad,
            "right_median_us":measured.right.median,"right_mad_us":measured.right.mad,
            "speedup_left_over_right":ratio,"m3p_over_m3":ratio,
            "paired_left_minus_right_median_us":measured.delta_median,
            "paired_left_minus_right_mad_us":measured.delta_mad,
            "left_samples_us":measured.left.samples,"right_samples_us":measured.right.samples,
            "repetitions_per_sample":measured.left.repetitions,"parity_full_vs_incremental":true,
            "canonical_m3p_digest":{"state":parity_digest.state,"metrics":parity_digest.metrics,"events":parity_digest.events},
            "canonical_m3_digest":{"state":m3_digest.state,"metrics":m3_digest.metrics,"events":m3_digest.events},
            "config_override":"propagation disabled to match the M3-08/09 non-propagation equal-live workload",
            "time_unit":"us/tick","days_per_operation":DAYS,
            "execution_options":{"metrics":true,"events":true,"snapshots":false,"days":DAYS,"pool_threads":0,"chunk_size":256,"equal_live_checkpoint_day":if smoke {5} else {400},"same_physical_live_population":true}
        }
    })
}
fn initial_propagation(
    storage: &SegmentedAgentStorage,
) -> BTreeMap<AgentId, PropagationAgentState> {
    storage
        .agent_ids()
        .iter()
        .copied()
        .map(|agent_id| (agent_id, PropagationAgentState::default()))
        .collect()
}

struct Phase9cRun {
    storage: SegmentedAgentStorage,
    graph: RelationshipGraph,
    config: sim_model::PropagationConfig,
    initial_rows: BTreeMap<AgentId, PropagationAgentState>,
    rows: BTreeMap<AgentId, PropagationAgentState>,
}

impl Phase9cRun {
    fn new(config: &SimConfig) -> Self {
        let mut base_config = config.clone();
        base_config
            .simulation
            .as_mut()
            .expect("systems")
            .systems
            .propagation = false;
        base_config.propagation = None;
        base_config.validate().expect("M3 base config validates");
        let world = initialize_world(&base_config).expect("base world initializes");
        let mut hybrid = HybridWorldState::hybrid(world);
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let mut context =
            M3StorageRunContext::new(&hybrid, &base_config, &run_context).expect("M3 context");
        let _ = run_m3_storage_days(&mut hybrid, &base_config, &mut context, 1, &options(false))
            .expect("pre-propagation base day");
        let storage = hybrid.storage().expect("storage").clone();
        let rows = initial_propagation(&storage);
        Self {
            storage,
            graph: context.relationship_graph().clone(),
            config: config.propagation.expect("propagation config"),
            initial_rows: rows.clone(),
            rows,
        }
    }

    fn advance(
        &mut self,
        pool: Option<&ThreadPool>,
    ) -> (f64, sim_model::propagation::PropagationDailyCounts) {
        self.rows.clone_from(&self.initial_rows);
        let started = Instant::now();
        let counts = if let Some(pool) = pool {
            apply_phase9c_storage_parallel(
                &self.storage,
                &self.graph,
                &mut self.rows,
                self.config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK,
            )
            .expect("parallel Phase9c")
        } else {
            apply_phase9c_storage(
                &self.storage,
                &self.graph,
                &mut self.rows,
                self.config,
                SEED,
                REPLICATE,
                0,
            )
            .expect("serial Phase9c")
        };
        (started.elapsed().as_secs_f64() * 1_000_000.0, counts)
    }
}

fn phase9c_pair(config: &SimConfig, threads: usize, settings: Settings) -> Value {
    let pool = ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("caller-sized pool");
    let mut serial_run = Phase9cRun::new(config);
    let mut parallel_run = Phase9cRun::new(config);
    let (serial_time, serial_counts) = serial_run.advance(None);
    let serial_rows = serial_run.rows.clone();
    let (parallel_time, parallel_counts) = parallel_run.advance(Some(&pool));
    let parallel_rows = parallel_run.rows.clone();
    assert_eq!(serial_counts, parallel_counts, "Phase9c counts parity");
    assert_eq!(serial_rows, parallel_rows, "Phase9c state parity");
    let mut serial_measure = Phase9cRun::new(config);
    let mut parallel_measure = Phase9cRun::new(config);
    let mut serial = || {
        let (elapsed, counts) = serial_measure.advance(None);
        assert_eq!(counts, serial_counts, "serial Phase9c repeat parity");
        assert_eq!(
            serial_measure.rows, serial_rows,
            "serial Phase9c row parity"
        );
        elapsed
    };
    let mut parallel = || {
        let (elapsed, counts) = parallel_measure.advance(Some(&pool));
        assert_eq!(counts, serial_counts, "parallel Phase9c repeat parity");
        assert_eq!(
            parallel_measure.rows, serial_rows,
            "parallel Phase9c row parity"
        );
        elapsed
    };
    let measured = measure_pair(&mut serial, &mut parallel, settings, 1.0);
    let _ = (serial_time, parallel_time);
    json!({
        "left_mode":"M3PROP Phase9c serial","right_mode":format!("M3PROP Phase9c pool{threads}/chunk256"),
        "result":{
            "left_median_us":measured.left.median,"left_mad_us":measured.left.mad,
            "right_median_us":measured.right.median,"right_mad_us":measured.right.mad,
            "speedup_left_over_right":measured.left.median/measured.right.median,
            "paired_left_minus_right_median_us":measured.delta_median,"paired_left_minus_right_mad_us":measured.delta_mad,
            "left_samples_us":measured.left.samples,"right_samples_us":measured.right.samples,
            "repetitions_per_sample":measured.left.repetitions,"parity":true,
            "phase9c_levels":serial_counts.levels_used,"max_frontier_size":serial_counts.max_frontier_size,
            "informed_count":serial_counts.informed_count,"time_unit":"us/Phase9c",
            "execution_options":{"profile":"m3+propagation","day":0,"metrics":false,"events":false,"snapshots":false,"pool_threads":threads,"chunk_size":256}
        }
    })
}

fn m3_11_phase9c(settings: Settings, smoke: bool, crossover: bool) -> Vec<Value> {
    if crossover {
        let n = if smoke { 300 } else { 10_000 };
        [
            (8_u32, 2_usize),
            (8, 4),
            (16, 6),
        ].into_iter().map(|(hops, threads)| {
            let config = load_m3prop_config(n, if smoke { 3 } else { 50 }, 8, hops, 0.2);
            let row = phase9c_pair(&config, threads, settings);
            json!({"cell":format!("M3PROP-N{n}-k8-hops{hops}-pool{threads}-transmit0.2"),"N":n,"K":config.world.settlement_count,"k":8,"p_local":0.5,"max_hops":hops,"threads":threads,"result":row["result"],"left_mode":row["left_mode"],"right_mode":row["right_mode"]})
        }).collect()
    } else {
        let n = if smoke { 300 } else { 100_000 };
        let config = load_m3prop_config(n, if smoke { 3 } else { 500 }, 16, 16, 0.2);
        let row = phase9c_pair(&config, 6, settings);
        vec![
            json!({"cell":format!("M3PROP-P9c-N{n}-k16-hops16-pool6"),"N":n,"K":config.world.settlement_count,"k":16,"p_local":0.5,"max_hops":16,"threads":6,"result":row["result"],"left_mode":row["left_mode"],"right_mode":row["right_mode"]}),
        ]
    }
}

struct PropRun {
    hybrid: HybridWorldState,
    context: M3PropagationStorageRunContext,
}

impl PropRun {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).expect("M3PROP world initializes");
        let hybrid = HybridWorldState::hybrid(world);
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let context = M3PropagationStorageRunContext::new(&hybrid, config, &run_context)
            .expect("M3PROP context");
        Self { hybrid, context }
    }

    fn advance(&mut self, config: &SimConfig, pool: Option<&ThreadPool>) -> (f64, Trace) {
        let started = Instant::now();
        let outcomes = match pool {
            Some(pool) => run_m3prop_storage_days_with_parallel_spread(
                &mut self.hybrid,
                config,
                &mut self.context,
                DAYS,
                &options(false),
                pool,
                CHUNK,
            )
            .expect("parallel M3PROP days"),
            None => run_m3prop_storage_days(
                &mut self.hybrid,
                config,
                &mut self.context,
                DAYS,
                &options(false),
            )
            .expect("serial M3PROP days"),
        };
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        let storage = self.hybrid.storage().expect("M3PROP storage");
        let metrics = outcomes
            .iter()
            .map(|day| day.metrics.clone().expect("metrics"))
            .collect::<Vec<_>>();
        let events = outcomes
            .iter()
            .flat_map(|day| day.events.iter().cloned())
            .collect::<Vec<_>>();
        let trace = Trace {
            state: canonical_m3prop_state_hash_from_storage(
                storage,
                self.hybrid.world.current_day,
                &self.hybrid.world.settlements,
                self.context.relationship_graph(),
                self.context.propagation(),
            )
            .expect("M3PROP state hash")
            .to_hex(),
            metrics: canonical_m3prop_metrics_hash(&metrics)
                .expect("M3PROP metrics hash")
                .to_hex(),
            events: canonical_m3prop_event_hash(&events)
                .expect("M3PROP event hash")
                .to_hex(),
        };
        (elapsed, trace)
    }
}

fn m3_11_whole_tick(settings: Settings, smoke: bool) -> Value {
    let n = if smoke { 300 } else { 100_000 };
    let config = load_m3prop_config(n, if smoke { 3 } else { 500 }, 16, 16, 0.2);
    let pool = ThreadPoolBuilder::new()
        .num_threads(6)
        .build()
        .expect("six-thread caller pool");
    let mut parity_serial = PropRun::new(&config);
    let mut parity_parallel = PropRun::new(&config);
    let (_, serial_trace) = parity_serial.advance(&config, None);
    let (_, parallel_trace) = parity_parallel.advance(&config, Some(&pool));
    assert_eq!(
        serial_trace, parallel_trace,
        "M3PROP serial/parallel digest parity before timing"
    );
    let mut serial = || {
        let mut run = PropRun::new(&config);
        run.advance(&config, None).0
    };
    let mut parallel = || {
        let mut run = PropRun::new(&config);
        run.advance(&config, Some(&pool)).0
    };
    let interleaved = measure_pair(&mut serial, &mut parallel, settings, f64::from(DAYS));
    let mut serial_ni = || {
        let mut run = PropRun::new(&config);
        run.advance(&config, None).0
    };
    let mut parallel_ni = || {
        let mut run = PropRun::new(&config);
        run.advance(&config, Some(&pool)).0
    };
    let noninterleaved =
        measure_pair_sample_blocked(&mut serial_ni, &mut parallel_ni, settings, f64::from(DAYS));
    json!({
        "cell":format!("M3PROP-N{n}-k16-hops16-pool6"),"N":n,"K":config.world.settlement_count,"k":16,"p_local":0.5,"max_hops":16,"threads":6,
        "left_mode":"M3PROP serial storage","right_mode":"M3PROP pool6/chunk256",
        "result":{
            "interleaved":pair_result(&interleaved,"M3PROP serial","M3PROP parallel"),
            "non_interleaved":pair_result(&noninterleaved,"M3PROP serial","M3PROP parallel"),
            "left_median_us":interleaved.left.median,"left_mad_us":interleaved.left.mad,
            "right_median_us":interleaved.right.median,"right_mad_us":interleaved.right.mad,
            "speedup_left_over_right":interleaved.left.median/interleaved.right.median,
            "canonical_m3prop_digest":{"state":serial_trace.state,"metrics":serial_trace.metrics,"events":serial_trace.events},
            "parity":true,"time_unit":"us/tick","days_per_operation":DAYS,
            "execution_options":{"metrics":true,"events":true,"snapshots":false,"days":DAYS,"pool_threads":6,"chunk_size":256}
        }
    })
}

fn m3_11_entry(entry: &str, settings: Settings, smoke: bool) -> Vec<Value> {
    match entry {
        "m3_11_phase9c_speedup" => m3_11_phase9c(settings, smoke, false),
        "m3_11_frontier_crossover" => m3_11_phase9c(settings, smoke, true),
        "m3_11_whole_tick_speedup" => vec![m3_11_whole_tick(settings, smoke)],
        _ => unreachable!("entry not in M3-11"),
    }
}

fn run_entry(entry: &str, output: Option<&Path>, smoke: bool) -> Vec<Value> {
    assert!(
        SUPPORTED_ENTRIES.contains(&entry),
        "unknown Session 2/3 entry: {entry}"
    );
    let controls = if smoke {
        json!({"smoke":true,"priority":"not-applied","affinity_mask":"not-applied"})
    } else {
        apply_windows_controls()
    };
    let host = {
        let rustc = Command::new("rustc").arg("-V").output().expect("rustc");
        json!({"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"processor":std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),"rustc":String::from_utf8_lossy(&rustc.stdout).trim()})
    };
    let config_source = match entry {
        "m3_06_settlement_index" | "m3_07_reversed_slots" => M3_EQUILIBRIUM,
        "m3_08_full_rebuild_equal_live" | "m3_09_incremental_equal_live" => M3P_SUSTAINED,
        _ => M3_PROPAGATION,
    };
    let config_source_bytes = fs::read(path(config_source)).expect("read selected source config");
    let config_sha256 = sha2::Sha256::digest(&config_source_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let settings = if smoke { SMOKE } else { A5 };
    let cells = match entry {
        "m3_06_settlement_index" => vec![m3_06(settings, smoke)],
        "m3_07_reversed_slots" => vec![m3_07(settings, smoke)],
        "m3_08_full_rebuild_equal_live" | "m3_09_incremental_equal_live" => {
            vec![m3_08_or_09(entry, settings, smoke)]
        }
        "m3_11_phase9c_speedup" | "m3_11_whole_tick_speedup" | "m3_11_frontier_crossover" => {
            m3_11_entry(entry, settings, smoke)
        }
        _ => unreachable!(),
    };
    if let Some(output) = output {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).expect("create raw directory");
        }
        let mut writer = BufWriter::new(File::create(output).expect("create sample JSONL"));
        writeln!(writer, "{}", json!({"type":"header","entry":entry,"protocol":"A-5","host":host,"process_controls":controls,"config_path":config_source,"config_sha256":format!("sha256:{config_sha256}"),"source_digest":"finalized by provenance tooling; no git revision recorded"})).expect("write header");
        for mut cell in cells.clone() {
            cell["type"] = json!("cell");
            cell["entry"] = json!(entry);
            cell["config_sha256"] = json!(format!("sha256:{config_sha256}"));
            writeln!(writer, "{}", cell).expect("write cell");
        }
        writer.flush().expect("flush JSONL");
    }
    cells
}

fn smoke_all() {
    for entry in SUPPORTED_ENTRIES {
        let cells = run_entry(entry, None, true);
        assert!(!cells.is_empty(), "smoke must produce cells for {entry}");
        println!("SMOKE_OK|entry={entry}|cells={}", cells.len());
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() == ["--dry-run"] {
        for entry in SUPPORTED_ENTRIES {
            println!("IMPLEMENTED|{entry}");
        }
        return;
    }
    if args.as_slice() == ["--smoke"] {
        smoke_all();
        return;
    }
    let mut entry = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--entry" => {
                index += 1;
                entry = args.get(index).cloned();
            }
            "--output" => {
                index += 1;
                output = args.get(index).map(PathBuf::from);
            }
            arg => panic!("unknown argument: {arg}"),
        }
        index += 1;
    }
    let entry = entry.expect("--entry required");
    let output = output.expect("--output required");
    println!("TIMING RUN START entry={entry}");
    let _ = run_entry(&entry, Some(&output), false);
    println!("TIMING RUN END entry={entry}");
}
