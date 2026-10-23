use rayon::ThreadPool;
use serde_json::{Value, json};
use sha2::Digest;
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage, canonical_metrics_hash,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ParallelStorageRunContext, M3SearchReductionRunContext,
    M3StorageRunContext, run_hybrid_authority_days, run_hybrid_authority_days_with_rayon_phase4,
    run_m3_storage_days, run_m3_storage_days_with_parallel_phase6b,
    run_m3_storage_days_with_search_reduction,
};
use sim_model::state::HybridWorldState;
use sim_model::storage::canonical_state_hash_from_storage;
use sim_model::{SimConfig, initialize_world};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const DAYS: u32 = 3;
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const TARGET_SAMPLE_SECONDS: f64 = 1.5;
const MIN_SAMPLE_SECONDS: f64 = 1.0;
const CALIBRATION_PROBES: usize = 3;
const CHUNK: NonZeroUsize = NonZeroUsize::new(256).expect("nonzero chunk");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const REGISTRY: &str = "../../docs/reproducibility/m5_05_conclusions.toml";
const M2_CONFIG: &str = "../../docs/reproducibility/m5_05_configs/m2_40.toml";
const M3_CONFIG: &str = "../../docs/reproducibility/m5_05_configs/m3_m2_40.toml";
const SESSION_ONE_ENTRIES: &[&str] = &[
    "m2_near_linear",
    "m2_final_tick",
    "m2_phase4_crossover",
    "m3_01_serial_baseline",
    "m3_02_parallel_phase6b",
    "m3_03_search_reduction",
];

#[path = "common/process_controls.rs"]
mod process_controls;

#[derive(Clone, Debug)]
struct Cell {
    population: u64,
    settlements: u32,
    label: String,
    chunk_size: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum M2Mode {
    Serial,
    Rayon,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum M3Mode {
    Storage,
    Parallel,
    SearchReduction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Bundle {
    state: String,
    metrics: String,
    events: String,
}

struct Timed<T> {
    result: T,
    elapsed_us: f64,
}

#[derive(Default)]
struct PairTimes {
    left_us: f64,
    right_us: f64,
    delta_us: f64,
}

fn source(path: &str) -> String {
    Path::new(ROOT)
        .join(path)
        .canonicalize()
        .unwrap_or_else(|error| panic!("resolve {path}: {error}"))
        .to_string_lossy()
        .into_owned()
}

fn read_config(path: &str, population: u64, settlements: u32, is_m3: bool) -> (SimConfig, String) {
    let mut value: toml::Value =
        toml::from_str(&fs::read_to_string(source(path)).expect("read benchmark config"))
            .expect("parse benchmark config TOML");
    value["world"]["initial_population"] = toml::Value::Integer(population as i64);
    value["world"]["settlement_count"] = toml::Value::Integer(i64::from(settlements));
    if is_m3 {
        value["interaction"]["relationship_degree_k"] = toml::Value::Integer(8);
        value["interaction"]["p_local"] = toml::Value::Float(0.5);
    } else {
        value["environment"]["carrying_capacity"] = toml::Value::Float(1_000.0 * population as f64);
        value["world"]["initial_settlement_resource"] =
            toml::Value::Float(200.0 * population as f64);
    }
    let encoded = toml::to_string(&value).expect("serialize selected config");
    let digest = sha2::Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    (
        SimConfig::parse_and_validate(&encoded).expect("selected config validates"),
        format!("sha256:{digest}"),
    )
}

fn hash_text(hash: CanonicalHash) -> String {
    hash.to_hex()
}

fn digest_m2(hybrid: &HybridWorldState, outcomes: &[sim_model::runner::DayOutcome]) -> Bundle {
    let storage = hybrid
        .segmented_storage
        .as_ref()
        .expect("hybrid storage exists");
    let metrics: Vec<_> = outcomes
        .iter()
        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
        .collect();
    let events: Vec<_> = outcomes
        .iter()
        .flat_map(|outcome| outcome.events.iter().cloned())
        .collect();
    Bundle {
        state: hash_text(
            canonical_state_hash_from_storage(
                storage,
                hybrid.world.current_day,
                &hybrid.world.settlements,
            )
            .expect("canonical M2 storage state hash"),
        ),
        metrics: hash_text(canonical_metrics_hash(&metrics).expect("canonical M2 metrics hash")),
        events: hash_text(canonical_event_hash(&events).expect("canonical event hash")),
    }
}

struct M2Run {
    initial: HybridWorldState,
    hybrid: HybridWorldState,
    context: M0RunContext,
}

impl M2Run {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).expect("initialize M2 world");
        let initial = HybridWorldState::hybrid(world);
        Self {
            hybrid: initial.clone(),
            initial,
            context: M0RunContext::new(config.world.master_seed, config.world.replicate_id),
        }
    }

    fn reset(&mut self) {
        self.hybrid = self.initial.clone();
    }

    fn advance(
        &mut self,
        config: &SimConfig,
        mode: M2Mode,
        pool: Option<&ThreadPool>,
        chunk_size: usize,
    ) -> Timed<Bundle> {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let started = Instant::now();
        let outcomes = match mode {
            M2Mode::Serial => {
                run_hybrid_authority_days(&mut self.hybrid, config, &self.context, DAYS, &options)
            }
            M2Mode::Rayon => run_hybrid_authority_days_with_rayon_phase4(
                &mut self.hybrid,
                config,
                &self.context,
                DAYS,
                &options,
                pool.expect("Rayon mode has a caller-owned pool"),
                NonZeroUsize::new(chunk_size).expect("nonzero M2 chunk size"),
            ),
        }
        .expect("M2 runner succeeds");
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let bundle = digest_m2(&self.hybrid, &outcomes);
        Timed {
            result: bundle,
            elapsed_us,
        }
    }
}

enum M3Runtime {
    Storage {
        initial: HybridWorldState,
        hybrid: HybridWorldState,
        context: M3StorageRunContext,
        graph: sim_model::RelationshipGraph,
    },
    Parallel {
        initial: HybridWorldState,
        hybrid: HybridWorldState,
        context: Box<M3ParallelStorageRunContext>,
        graph: sim_model::RelationshipGraph,
    },
    Search {
        initial: HybridWorldState,
        hybrid: HybridWorldState,
        context: M3SearchReductionRunContext,
        graph: sim_model::RelationshipGraph,
    },
}

impl M3Runtime {
    fn new(config: &SimConfig, mode: M3Mode) -> Self {
        let world = initialize_world(config).expect("initialize M3 world");
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        match mode {
            M3Mode::Storage => {
                let hybrid = HybridWorldState::hybrid(world);
                let context = M3StorageRunContext::new(&hybrid, config, &run_context)
                    .expect("M3 storage context");
                let graph = context.relationship_graph().clone();
                let initial = hybrid.clone();
                Self::Storage {
                    initial,
                    hybrid,
                    context,
                    graph,
                }
            }
            M3Mode::Parallel => {
                let hybrid = HybridWorldState::hybrid(world);
                let context = M3ParallelStorageRunContext::new(&hybrid, config, &run_context)
                    .expect("M3 parallel context");
                let graph = context.relationship_graph().clone();
                let initial = hybrid.clone();
                Self::Parallel {
                    initial,
                    hybrid,
                    context: Box::new(context),
                    graph,
                }
            }
            M3Mode::SearchReduction => {
                let hybrid = HybridWorldState::hybrid(world);
                let graph_context = M3StorageRunContext::new(&hybrid, config, &run_context)
                    .expect("M3 graph context");
                let graph = graph_context.relationship_graph().clone();
                let context = M3SearchReductionRunContext::new(&hybrid, config, &run_context)
                    .expect("M3 search-reduction context");
                let initial = hybrid.clone();
                Self::Search {
                    initial,
                    hybrid,
                    context,
                    graph,
                }
            }
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Storage {
                initial, hybrid, ..
            }
            | Self::Parallel {
                initial, hybrid, ..
            }
            | Self::Search {
                initial, hybrid, ..
            } => *hybrid = initial.clone(),
        }
    }

    fn advance(
        &mut self,
        config: &SimConfig,
        mode: M3Mode,
        pool: Option<&ThreadPool>,
    ) -> Timed<Bundle> {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let (outcomes, graph, hybrid, elapsed_us) = match (mode, &mut *self) {
            (
                M3Mode::Storage,
                Self::Storage {
                    hybrid,
                    context,
                    graph,
                    ..
                },
            ) => {
                let started = Instant::now();
                let outcomes = run_m3_storage_days(hybrid, config, context, DAYS, &options)
                    .expect("M3 serial storage runner succeeds");
                let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
                (outcomes, graph.clone(), Some(hybrid), elapsed_us)
            }
            (
                M3Mode::Parallel,
                Self::Parallel {
                    hybrid,
                    context,
                    graph,
                    ..
                },
            ) => {
                let started = Instant::now();
                let outcomes = run_m3_storage_days_with_parallel_phase6b(
                    hybrid,
                    config,
                    context,
                    DAYS,
                    &options,
                    pool.expect("parallel mode has a caller-owned pool"),
                    CHUNK,
                )
                .expect("M3 parallel storage runner succeeds");
                let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
                (outcomes, graph.clone(), Some(hybrid), elapsed_us)
            }
            (
                M3Mode::SearchReduction,
                Self::Search {
                    hybrid,
                    context,
                    graph,
                    ..
                },
            ) => {
                let started = Instant::now();
                let outcomes = run_m3_storage_days_with_search_reduction(
                    hybrid, config, context, DAYS, &options,
                )
                .expect("M3 search-reduction runner succeeds");
                let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
                (outcomes, graph.clone(), Some(hybrid), elapsed_us)
            }
            _ => panic!("M3 mode/runtime mismatch"),
        };
        let hybrid = hybrid.expect("M3 storage runtime has a hybrid state");
        let storage = hybrid
            .segmented_storage
            .as_ref()
            .expect("hybrid storage exists");
        let bundle = Bundle {
            state: hash_text(
                canonical_m3_state_hash_from_storage(
                    storage,
                    hybrid.world.current_day,
                    &hybrid.world.settlements,
                    &graph,
                )
                .expect("M3 storage state hash"),
            ),
            metrics: hash_text(
                canonical_m3_metrics_hash(
                    &outcomes
                        .iter()
                        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
                        .collect::<Vec<_>>(),
                )
                .expect("M3 metrics hash"),
            ),
            events: hash_text(
                canonical_event_hash(
                    &outcomes
                        .iter()
                        .flat_map(|outcome| outcome.events.iter().cloned())
                        .collect::<Vec<_>>(),
                )
                .expect("M3 events hash"),
            ),
        };
        Timed {
            result: bundle,
            elapsed_us,
        }
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn median_mad(values: &[f64]) -> (f64, f64) {
    let center = median(values);
    let deviations: Vec<_> = values.iter().map(|value| (value - center).abs()).collect();
    (center, median(&deviations))
}

fn make_pool(threads: usize) -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("fixed-size caller-owned Rayon pool")
}

fn m2_pair(
    config: &SimConfig,
    pool: &ThreadPool,
    threads: usize,
    count: usize,
    order_offset: usize,
    chunk_size: usize,
) -> PairTimes {
    assert_eq!(pool.current_num_threads(), threads, "M2 pool-size parity");
    let mut serial_us = 0.0;
    let mut candidate_us = 0.0;
    let mut delta_us = 0.0;
    let mut serial = M2Run::new(config);
    let mut candidate = M2Run::new(config);
    for repeat in 0..count {
        serial.reset();
        candidate.reset();
        let candidate_first = (repeat + order_offset) % 2 == 1;
        let (first, second) = if candidate_first {
            let candidate_result = candidate.advance(config, M2Mode::Rayon, Some(pool), chunk_size);
            candidate_us += candidate_result.elapsed_us;
            let serial_result = serial.advance(config, M2Mode::Serial, None, chunk_size);
            serial_us += serial_result.elapsed_us;
            delta_us += serial_result.elapsed_us - candidate_result.elapsed_us;
            (candidate_result.result, serial_result.result)
        } else {
            let serial_result = serial.advance(config, M2Mode::Serial, None, chunk_size);
            serial_us += serial_result.elapsed_us;
            let candidate_result = candidate.advance(config, M2Mode::Rayon, Some(pool), chunk_size);
            candidate_us += candidate_result.elapsed_us;
            delta_us += serial_result.elapsed_us - candidate_result.elapsed_us;
            (serial_result.result, candidate_result.result)
        };
        assert_eq!(first, second, "M2 serial/Rayon digest parity");
    }
    PairTimes {
        left_us: serial_us,
        right_us: candidate_us,
        delta_us,
    }
}

fn m3_pair(
    config: &SimConfig,
    left: M3Mode,
    right: M3Mode,
    pool: Option<&ThreadPool>,
    threads: usize,
    count: usize,
    order_offset: usize,
) -> PairTimes {
    if let Some(pool) = pool {
        assert_eq!(pool.current_num_threads(), threads, "M3 pool-size parity");
    } else {
        assert_eq!(
            threads, 0,
            "pool-less M3 pair must use serial/reference modes"
        );
    }
    let mut left_us = 0.0;
    let mut right_us = 0.0;
    let mut delta_us = 0.0;
    let mut left_run = M3Runtime::new(config, left);
    let mut right_run = M3Runtime::new(config, right);
    for repeat in 0..count {
        left_run.reset();
        right_run.reset();
        let right_first = (repeat + order_offset) % 2 == 1;
        let (first, second) = if right_first {
            let right_result = right_run.advance(config, right, pool);
            right_us += right_result.elapsed_us;
            let left_result = left_run.advance(config, left, None);
            left_us += left_result.elapsed_us;
            delta_us += left_result.elapsed_us - right_result.elapsed_us;
            (right_result.result, left_result.result)
        } else {
            let left_result = left_run.advance(config, left, None);
            left_us += left_result.elapsed_us;
            let right_result = right_run.advance(config, right, pool);
            right_us += right_result.elapsed_us;
            delta_us += left_result.elapsed_us - right_result.elapsed_us;
            (left_result.result, right_result.result)
        };
        assert_eq!(first, second, "M3 paired runner digest parity");
    }
    PairTimes {
        left_us,
        right_us,
        delta_us,
    }
}

fn run_samples(
    m2: bool,
    config: &SimConfig,
    left: M3Mode,
    right: M3Mode,
    pool_threads: usize,
    chunk_size: usize,
) -> Value {
    let pool = (pool_threads > 0).then(|| make_pool(pool_threads));
    if !m2 {
        assert_eq!(chunk_size, CHUNK.get(), "M3 Session 1 uses fixed chunk 256");
    }
    let run_pair = |count, order_offset| {
        if m2 {
            m2_pair(
                config,
                pool.as_ref().expect("M2 candidate uses pool"),
                pool_threads,
                count,
                order_offset,
                chunk_size,
            )
        } else {
            m3_pair(
                config,
                left,
                right,
                pool.as_ref(),
                pool_threads,
                count,
                order_offset,
            )
        }
    };
    // Warm the runner's lazy scratch allocations before calibrating repetitions.
    let _ = run_pair(1, 0);
    let mut probe_seconds = Vec::with_capacity(CALIBRATION_PROBES);
    for probe in 0..CALIBRATION_PROBES {
        let pair = run_pair(1, probe);
        probe_seconds.push((pair.left_us + pair.right_us) / 1_000_000.0);
    }
    let repetitions = (TARGET_SAMPLE_SECONDS / median(&probe_seconds))
        .ceil()
        .max(1.0) as usize;
    for warmup in 0..WARMUPS {
        let pair = run_pair(repetitions, WARMUPS + warmup);
        let elapsed = (pair.left_us + pair.right_us) / 1_000_000.0;
        assert!(
            elapsed >= MIN_SAMPLE_SECONDS,
            "A-5 warmup was shorter than one second: {elapsed:.6}s (r={repetitions})"
        );
    }
    let mut left_samples = Vec::with_capacity(SAMPLES);
    let mut right_samples = Vec::with_capacity(SAMPLES);
    let mut delta_samples = Vec::with_capacity(SAMPLES);
    let mut sample_seconds = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let pair = run_pair(repetitions, sample);
        let elapsed = (pair.left_us + pair.right_us) / 1_000_000.0;
        if elapsed < 1.0 {
            panic!(
                "A-5 sample {sample} was shorter than one second: {elapsed:.6}s (r={repetitions})"
            );
        }
        left_samples.push(pair.left_us / repetitions as f64 / f64::from(DAYS));
        right_samples.push(pair.right_us / repetitions as f64 / f64::from(DAYS));
        delta_samples.push(pair.delta_us / repetitions as f64 / f64::from(DAYS));
        sample_seconds.push(elapsed);
    }
    let (left_median, left_mad) = median_mad(&left_samples);
    let (right_median, right_mad) = median_mad(&right_samples);
    let (delta_median, delta_mad) = median_mad(&delta_samples);
    json!({
        "warmups": WARMUPS,
        "calibration_probe_pairs": CALIBRATION_PROBES,
        "target_sample_seconds": TARGET_SAMPLE_SECONDS,
        "samples": SAMPLES,
        "repetitions_per_sample": repetitions,
        "days_per_operation": DAYS,
        "time_unit": "us/tick",
        "pool_threads": pool_threads,
        "chunk_size": chunk_size,
        "left_median_us": left_median,
        "left_mad_us": left_mad,
        "right_median_us": right_median,
        "right_mad_us": right_mad,
        "speedup_left_over_right": left_median / right_median,
        "paired_left_minus_right_median_us": delta_median,
        "paired_left_minus_right_mad_us": delta_mad,
        "sample_wall_seconds": sample_seconds,
        "left_samples_us": left_samples,
        "right_samples_us": right_samples,
        "paired_delta_samples_us": delta_samples,
        "minimum_sample_seconds": sample_seconds.iter().copied().fold(f64::INFINITY, f64::min),
        "execution_options": {
            "metrics": true,
            "events": true,
            "snapshots": false,
            "days": DAYS,
            "checkpoint": "fresh initialized state, deterministic seed/replicate"
        }
    })
}

fn session_one(entry: &str) -> Vec<(Cell, bool, M3Mode, M3Mode, usize)> {
    assert!(
        SESSION_ONE_ENTRIES.contains(&entry),
        "entry is outside Session 1"
    );
    match entry {
        "m2_near_linear" => [50_000, 100_000]
            .into_iter()
            .map(|n| {
                (
                    Cell {
                        population: n,
                        settlements: n.div_ceil(200) as u32,
                        label: format!("B-N{n}"),
                        chunk_size: CHUNK.get(),
                    },
                    true,
                    M3Mode::Storage,
                    M3Mode::Storage,
                    6,
                )
            })
            .collect(),
        "m2_final_tick" => [2_u32, 500_u32]
            .into_iter()
            .map(|k| {
                let n = 100_000_u64;
                (
                    Cell {
                        population: n,
                        settlements: k,
                        label: if k == 2 {
                            "A-N100000".into()
                        } else {
                            "B-N100000".into()
                        },
                        chunk_size: CHUNK.get(),
                    },
                    true,
                    M3Mode::Storage,
                    M3Mode::Storage,
                    6,
                )
            })
            .collect(),
        "m2_phase4_crossover" => [
            (1_000_u64, 5_u32, 6_usize, 256_usize),
            (5_000, 25, 6, 256),
            (10_000, 50, 1, 256),
        ]
        .into_iter()
        .map(|(n, k, threads, chunk_size)| {
            (
                Cell {
                    population: n,
                    settlements: k,
                    label: format!("B-N{n}-T{threads}"),
                    chunk_size,
                },
                true,
                M3Mode::Storage,
                M3Mode::Storage,
                threads,
            )
        })
        .collect(),
        "m3_01_serial_baseline" => [10_000_u64, 50_000, 100_000]
            .into_iter()
            .map(|n| {
                (
                    Cell {
                        population: n,
                        settlements: n.div_ceil(200) as u32,
                        label: format!("M3-storage-replay-N{n}"),
                        chunk_size: CHUNK.get(),
                    },
                    false,
                    M3Mode::Storage,
                    M3Mode::Storage,
                    0,
                )
            })
            .collect(),
        "m3_02_parallel_phase6b" => [10_000_u64, 50_000, 100_000]
            .into_iter()
            .map(|n| {
                (
                    Cell {
                        population: n,
                        settlements: n.div_ceil(200) as u32,
                        label: format!("M3-serial-parallel-N{n}"),
                        chunk_size: CHUNK.get(),
                    },
                    false,
                    M3Mode::Storage,
                    M3Mode::Parallel,
                    6,
                )
            })
            .collect(),
        "m3_03_search_reduction" => [10_000_u64, 100_000]
            .into_iter()
            .map(|n| {
                (
                    Cell {
                        population: n,
                        settlements: n.div_ceil(200) as u32,
                        label: format!("M3-serial-search-N{n}"),
                        chunk_size: CHUNK.get(),
                    },
                    false,
                    M3Mode::Storage,
                    M3Mode::SearchReduction,
                    0,
                )
            })
            .collect(),
        _ => panic!("entry is not part of authorized Session 1: {entry}"),
    }
}

fn apply_windows_controls() -> Value {
    let status = process_controls::apply_high_priority_affinity();
    if cfg!(windows) {
        if !status.priority_set || !status.affinity_set {
            panic!(
                "A-5 Windows priority/affinity setup failed: priority={}, affinity={}",
                status.priority_set, status.affinity_set
            );
        }
        json!({"priority":"high","affinity_mask":"0x555"})
    } else {
        json!({"priority":"not-applied","affinity_mask":"not-applied"})
    }
}

fn file_digest(path: &Path) -> String {
    let bytes = fs::read(path).expect("read cell config");
    format!(
        "sha256:{}",
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn config_for(entry: &str, cell: &Cell) -> (SimConfig, bool, String) {
    let m2 = entry.starts_with("m2_");
    let path = if m2 { M2_CONFIG } else { M3_CONFIG };
    let (config, digest) = read_config(path, cell.population, cell.settlements, !m2);
    (config, m2, digest)
}

fn run_entry(entry: &str, output: &Path) {
    let controls = apply_windows_controls();
    let registry_path = Path::new(ROOT).join(REGISTRY);
    let registry_text = fs::read_to_string(&registry_path).expect("read conclusion registry");
    let registry: toml::Value = toml::from_str(&registry_text).expect("parse conclusion registry");
    let registered = registry["conclusion"]
        .as_array()
        .expect("conclusion array")
        .iter()
        .any(|row| row["id"].as_str() == Some(entry));
    assert!(
        registered,
        "entry absent from frozen conclusion registry: {entry}"
    );
    let path = if entry.starts_with("m2_") {
        M2_CONFIG
    } else {
        M3_CONFIG
    };
    let config_path = Path::new(ROOT).join(path);
    let config_sha256 = file_digest(&config_path);
    let mut writer = BufWriter::new(File::create(output).expect("create raw output"));
    let rustc = Command::new("rustc")
        .arg("-V")
        .output()
        .expect("rustc version");
    writeln!(
        writer,
        "{}",
        json!({
            "type":"header",
            "entry":entry,
            "protocol":"A-5",
            "host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH,"processor":std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),"rustc":String::from_utf8_lossy(&rustc.stdout).trim()},
            "process_controls":controls,
            "config_path":path,
            "config_sha256":config_sha256,
            "source_digest":"finalized by provenance tooling; no git revision recorded"
        })
    )
    .expect("write raw header");

    for (cell, m2, left_mode, right_mode, threads) in session_one(entry) {
        let (config, is_m2, cell_config_digest) = config_for(entry, &cell);
        assert_eq!(m2, is_m2, "spec/profile parity");
        let (left_label, right_label) = if is_m2 {
            ("M2Serial".to_string(), "M2Phase4Rayon".to_string())
        } else {
            (format!("{left_mode:?}"), format!("{right_mode:?}"))
        };
        let result = if is_m2 {
            run_samples(
                true,
                &config,
                left_mode,
                right_mode,
                threads,
                cell.chunk_size,
            )
        } else {
            run_samples(
                false,
                &config,
                left_mode,
                right_mode,
                threads,
                cell.chunk_size,
            )
        };
        writeln!(
            writer,
            "{}",
            json!({
                "type":"cell",
                "entry":entry,
                "cell":cell.label,
                "N":cell.population,
                "K":cell.settlements,
                "k":if is_m2 { Value::Null } else { json!(8) },
                "p_local":if is_m2 { Value::Null } else { json!(0.5) },
                "config_sha256":cell_config_digest,
                "initial_settlement_resource":config.world.initial_settlement_resource,
                "carrying_capacity":config.environment.carrying_capacity,
                "left_mode":left_label,
                "right_mode":right_label,
                "chunk_size":cell.chunk_size,
                "result":result
            })
        )
        .expect("write cell record");
    }
    writer.flush().expect("flush raw output");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    let entry = entry.expect("--entry is required");
    let output = output.expect("--output is required");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).expect("create raw output directory");
    }
    println!("TIMING RUN START entry={entry}");
    run_entry(&entry, &output);
    println!("TIMING RUN END entry={entry}");
}
