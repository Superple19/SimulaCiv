use rayon::ThreadPoolBuilder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sim_core::{AgentId, GroupId};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchIntervention, ConfigParameter, ConfigValue,
};
use sim_model::crn_efficiency::{
    CrnEfficiencyRequest, run_crn_efficiency, run_crn_efficiency_parallel,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::minimal_cause::{
    EffectComparison, MinimalCausePredicate, MinimalCauseRequest, build_agent_field_candidates,
    search_minimal_cause, search_minimal_cause_parallel,
};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{M0RunContext, SimConfig, SimulationProfile, initialize_world};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "common/process_controls.rs"]
mod process_controls;

const FIXTURE: &str = include_str!("../tests/fixtures/m3p_propagation_sustained.toml");
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const HORIZONS: [u32; 4] = [7, 30, 90, 365];
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const SAMPLE_TARGET: Duration = Duration::from_millis(1250);
const SAMPLE_FLOOR: Duration = Duration::from_secs(1);
const CHUNK: NonZeroUsize = NonZeroUsize::new(1).unwrap();
const THREADS: usize = 6;

const CELLS: [&str; 4] = [
    "case-a-exhaustive-R10-H90",
    "food-zero-full-R100-H7-30-90-365",
    "health-0.01-full-R100-H7-30-90-365",
    "global-cost-plus-1pct-full-R100-H7-30-90-365",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let smoke = args.iter().any(|arg| arg == "--smoke");
    let resume = args.iter().any(|arg| arg == "--resume");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let selected = args.iter().find_map(|arg| arg.strip_prefix("--cell="));
    let output = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--output="))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/m5_06_m4_parallel_a5.jsonl"));
    if dry_run {
        for cell in CELLS {
            println!("{cell} expected_sample_floor=41x1.25s plus setup");
        }
        return Ok(());
    }
    if !smoke && selected.is_some_and(|id| !CELLS.contains(&id)) {
        return Err(format!("unknown M4 cell: {}", selected.unwrap()).into());
    }
    if smoke && !resume {
        let _ = fs::remove_file(&output);
    }
    ensure_header(&output)?;
    let pool = ThreadPoolBuilder::new().num_threads(THREADS).build()?;
    let cells: Vec<&str> = if smoke {
        vec!["smoke-crn-food-zero", "smoke-minimal-cause"]
    } else {
        CELLS
            .iter()
            .copied()
            .filter(|cell| selected.is_none_or(|id| id == *cell))
            .collect()
    };
    for (index, cell) in cells.iter().enumerate() {
        let series_started_ms = timestamp_ms();
        let hash = config_hash(cell);
        if completed(&output, cell, &hash)? {
            eprintln!(
                "skip {}/{} {cell} (matching config hash already complete)",
                index + 1,
                cells.len()
            );
            continue;
        }
        if smoke {
            let exact = if cell.starts_with("smoke-crn") {
                crn_pair("food_0", 100, 1, vec![1], 1, &pool, 0)?.2
            } else {
                search_pair(true, &pool, Path::new("target/m5_06_m4_smoke"), 0)?.2
            };
            assert!(exact);
            append(
                &output,
                json!({"kind":"series","cell_id":cell,"config_hash":hash,"series_started_unix_ms":series_started_ms,"series_completed_unix_ms":timestamp_ms(),"smoke":true,"parity":true,"complete":true}),
            )?;
            eprintln!("cell {}/{} {cell} smoke complete", index + 1, cells.len());
            continue;
        }
        let applied = process_controls::apply_high_priority_affinity();
        #[cfg(windows)]
        {
            let current =
                process_controls::current_settings().ok_or("failed to read process controls")?;
            if !applied.priority_set
                || !applied.affinity_set
                || current.priority_class != 0x80
                || current.process_mask != 0x555
            {
                return Err("A-5 priority/affinity controls were not applied".into());
            }
        }
        let started = Instant::now();
        let series = if *cell == CELLS[0] {
            measure_search(&pool)?
        } else {
            measure_crn(cell, &pool)?
        };
        append(
            &output,
            json!({"kind":"series","cell_id":cell,"config_hash":hash,"series_started_unix_ms":series_started_ms,"series_completed_unix_ms":timestamp_ms(),"pool_threads":THREADS,"chunk_size":CHUNK.get(),"process_controls":{"priority_set":applied.priority_set,"affinity_set":applied.affinity_set},"series":series,"complete":true}),
        )?;
        eprintln!(
            "cell {}/{} {} elapsed={:.1}s",
            index + 1,
            cells.len(),
            cell,
            started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

fn config_for(population: u64, settlements: u32) -> Result<SimConfig, Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(FIXTURE)?;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.validate()?;
    Ok(config)
}

fn snapshot_for(
    config: &SimConfig,
    context: &M0RunContext,
) -> Result<sim_model::CanonicalSnapshot, Box<dyn std::error::Error>> {
    let world = initialize_world(config)?;
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, config, context)?;
    Ok(encode_m3p_snapshot(&state, &metadata)?)
}

fn crn_pair(
    name: &str,
    population: u64,
    settlements: u32,
    horizons: Vec<u32>,
    replicates: usize,
    pool: &rayon::ThreadPool,
    order: usize,
) -> Result<(u128, u128, bool), Box<dyn std::error::Error>> {
    let config = config_for(population, settlements)?;
    let context = M0RunContext::new(SEED, REPLICATE);
    let snapshot = snapshot_for(&config, &context)?;
    let intervention = match name {
        "food_0" => BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        "health_0.01" => BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Health,
            value: AgentFieldValue::Health(0.01),
        },
        "base_metabolic_cost_plus_1pct" => BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(1.01),
        },
        _ => return Err(format!("unknown intervention {name}").into()),
    };
    let request = CrnEfficiencyRequest {
        config,
        branch_point: snapshot,
        intervention,
        target_group_id: GroupId(0),
        horizons,
        replicate_count: replicates,
    };
    let first_parallel = order % 2 == 1;
    let (serial_ns, serial, parallel_ns, parallel) = if first_parallel {
        let start = Instant::now();
        let parallel = run_crn_efficiency_parallel(&request, pool, CHUNK)?;
        let pn = start.elapsed().as_nanos();
        let start = Instant::now();
        let serial = run_crn_efficiency(&request)?;
        let sn = start.elapsed().as_nanos();
        (sn, serial, pn, parallel)
    } else {
        let start = Instant::now();
        let serial = run_crn_efficiency(&request)?;
        let sn = start.elapsed().as_nanos();
        let start = Instant::now();
        let parallel = run_crn_efficiency_parallel(&request, pool, CHUNK)?;
        let pn = start.elapsed().as_nanos();
        (sn, serial, pn, parallel)
    };
    let exact = serial == parallel;
    assert!(exact, "M4 serial/parallel CRN result mismatch: {name}");
    Ok((serial_ns, parallel_ns, exact))
}

fn search_pair(
    smoke: bool,
    pool: &rayon::ThreadPool,
    log_root: &Path,
    order: usize,
) -> Result<(u128, u128, bool), Box<dyn std::error::Error>> {
    let (population, settlements, candidate_count, replicates, horizon, max_evaluations) = if smoke
    {
        (100, 1, 2, 1, 2, 32)
    } else {
        (2_000, 3, 8, 10, 90, 5_000)
    };
    let config = config_for(population, settlements)?;
    let context = M0RunContext::new(SEED, REPLICATE);
    let snapshot = snapshot_for(&config, &context)?;
    let world = initialize_world(&config)?;
    let mut ids = world
        .agents
        .iter()
        .filter(|a| a.group_id == GroupId(0))
        .map(|a| a.agent_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.truncate(candidate_count);
    let candidates = build_agent_field_candidates(
        &world,
        &ids,
        Some(GroupId(0)),
        AgentField::Health,
        &[AgentFieldValue::Health(0.01)],
    )?;
    let serial_path = log_root.with_extension("serial.jsonl");
    let parallel_path = log_root.with_extension("parallel.jsonl");
    let request = |evaluation_log_path| MinimalCauseRequest {
        batch: sim_model::BranchInterventionBatchRequest {
            snapshot: snapshot.clone(),
            config: config.clone(),
            run_context: context,
            interventions: Vec::new(),
            horizon_days: horizon,
            snapshot_days: Vec::new(),
        },
        candidates: candidates.clone(),
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::TargetSettlementAlivePopulation,
            target_group_id: Some(GroupId(0)),
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: (1..=replicates as u32).collect(),
        exploratory_single_replicate: false,
        max_branch_evaluations: max_evaluations,
        max_wall_time: Duration::from_secs(7200),
        evaluation_log_path,
    };
    let _ = fs::remove_file(&serial_path);
    let _ = fs::remove_file(&parallel_path);
    let (serial, serial_ns, parallel, parallel_ns) = if order.is_multiple_of(2) {
        let start = Instant::now();
        let serial = search_minimal_cause(&request(serial_path.clone()))?;
        let serial_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let parallel = search_minimal_cause_parallel(&request(parallel_path.clone()), pool, CHUNK)?;
        let parallel_ns = start.elapsed().as_nanos();
        (serial, serial_ns, parallel, parallel_ns)
    } else {
        let start = Instant::now();
        let parallel = search_minimal_cause_parallel(&request(parallel_path.clone()), pool, CHUNK)?;
        let parallel_ns = start.elapsed().as_nanos();
        let start = Instant::now();
        let serial = search_minimal_cause(&request(serial_path.clone()))?;
        let serial_ns = start.elapsed().as_nanos();
        (serial, serial_ns, parallel, parallel_ns)
    };
    let exact = serial.status == parallel.status
        && serial.cause_ids == parallel.cause_ids
        && serial.guarantee == parallel.guarantee
        && serial.evaluation_count == parallel.evaluation_count
        && serial.mean_effect == parallel.mean_effect
        && serial.predicate_value == parallel.predicate_value;
    assert!(exact, "M4 serial/parallel minimal-cause result mismatch");
    let serial_log = read_log(&serial_path)?;
    let parallel_log = read_log(&parallel_path)?;
    assert_eq!(
        serial_log, parallel_log,
        "M4 serial/parallel evaluation log parity"
    );
    let _ = fs::remove_file(serial_path);
    let _ = fs::remove_file(parallel_path);
    Ok((serial_ns, parallel_ns, exact))
}

fn read_log(path: &Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
    BufReader::new(file)
        .lines()
        .map(|line| -> Result<Value, Box<dyn std::error::Error>> {
            let mut value: Value = serde_json::from_str(&line?)?;
            if value["kind"] == "result" {
                value["result"]["evaluation_log_path"] = json!("<evaluation-log>");
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()
}

fn measure_crn(cell: &str, pool: &rayon::ThreadPool) -> Result<Value, Box<dyn std::error::Error>> {
    let (name, label) = if cell.starts_with("food-zero") {
        ("food_0", "food-zero")
    } else if cell.starts_with("health-0.01") {
        ("health_0.01", "health-0.01")
    } else {
        ("base_metabolic_cost_plus_1pct", "global-cost-plus-1pct")
    };
    measure_series(
        |order| crn_pair(name, 10_000, 50, HORIZONS.to_vec(), 100, pool, order),
        label,
    )
}

fn measure_search(pool: &rayon::ThreadPool) -> Result<Value, Box<dyn std::error::Error>> {
    measure_series(
        |order| search_pair(false, pool, Path::new("target/m5_06_m4_pair"), order),
        "case-a",
    )
}

fn measure_series<F>(mut operation: F, label: &str) -> Result<Value, Box<dyn std::error::Error>>
where
    F: FnMut(usize) -> Result<(u128, u128, bool), Box<dyn std::error::Error>>,
{
    let mut order = 0usize;
    let mut warmup_ns = Vec::with_capacity(WARMUPS);
    for _ in 0..WARMUPS {
        let started = Instant::now();
        let result = operation(order)?;
        order += 1;
        assert!(result.2);
        warmup_ns.push(started.elapsed().as_nanos());
    }
    let mut repetitions = 1usize;
    let mut calibration_wall_ns = Vec::new();
    loop {
        let start = Instant::now();
        let mut timed_ns = 0u128;
        for _ in 0..repetitions {
            let result = operation(order)?;
            order += 1;
            assert!(result.2);
            timed_ns += result.0 + result.1;
        }
        let elapsed = start.elapsed();
        calibration_wall_ns.push(elapsed.as_nanos());
        if timed_ns >= SAMPLE_TARGET.as_nanos() {
            break;
        }
        repetitions = ((repetitions as f64 * SAMPLE_TARGET.as_nanos() as f64 / timed_ns as f64)
            * 1.1)
            .ceil()
            .max((repetitions + 1) as f64) as usize;
    }
    let mut recalibrations = Vec::new();
    let sample_records = loop {
        let mut values = Vec::with_capacity(SAMPLES);
        let mut short = false;
        for sample in 0..SAMPLES {
            let sample_started_ms = timestamp_ms();
            let start = Instant::now();
            let mut serial = 0u128;
            let mut parallel = 0u128;
            for _ in 0..repetitions {
                let pair = operation(order)?;
                order += 1;
                assert!(pair.2);
                serial += pair.0;
                parallel += pair.1;
            }
            let elapsed = start.elapsed();
            let timed_ns = serial + parallel;
            if timed_ns < SAMPLE_FLOOR.as_nanos() {
                short = true;
                break;
            }
            values.push(json!({"sample":sample,"started_unix_ms":sample_started_ms,"timed_pair_elapsed_ns":timed_ns,"wall_elapsed_ns":elapsed.as_nanos(),"serial_ns_per_call":serial as f64/repetitions as f64,"parallel_ns_per_call":parallel as f64/repetitions as f64}));
        }
        if !short {
            break values;
        }
        if recalibrations.len() >= 3 {
            return Err(
                "A-5 sample remained below the one-second floor after three recalibrations".into(),
            );
        }
        let old = repetitions;
        repetitions = ((repetitions as f64) * 1.5).ceil() as usize;
        recalibrations.push(json!({"old_r":old,"new_r":repetitions,"reason":"sample below one-second floor; discarded and series restarted"}));
    };
    Ok(
        json!({"label":label,"warmups":WARMUPS,"warmup_wall_ns":warmup_ns,"samples":SAMPLES,"samples_target_ns":SAMPLE_TARGET.as_nanos(),"floor_sample_ns":SAMPLE_FLOOR.as_nanos(),"calibration_wall_ns":calibration_wall_ns,"repetitions_per_sample":repetitions,"recalibrations":recalibrations,"sample_records":sample_records}),
    )
}

fn config_hash(cell: &str) -> String {
    format!(
        "sha256:{}",
        hex(&Sha256::digest(
            format!(
                "{FIXTURE}|{cell}|pool={THREADS}|chunk={}|horizons={HORIZONS:?}|samples={SAMPLES}",
                CHUNK.get()
            )
            .as_bytes()
        ))
    )
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_millis()
}

fn ensure_header(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    append(
        path,
        json!({"kind":"header","stage":"M5-06","adapter":"m4-parallel","host_os":std::env::consts::OS,"host_arch":std::env::consts::ARCH}),
    )
}
fn completed(path: &Path, cell: &str, hash: &str) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(false);
    }
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let row: Value = serde_json::from_str(&line?)?;
        if row["kind"] == "series" && row["cell_id"] == cell {
            if row["config_hash"] != hash {
                return Err(format!("resume config hash differs for {cell}").into());
            }
            return Ok(row["complete"] == true);
        }
    }
    Ok(false)
}
fn append(path: &Path, value: Value) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &value)?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}
