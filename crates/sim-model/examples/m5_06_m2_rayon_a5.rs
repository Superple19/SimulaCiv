use rayon::ThreadPoolBuilder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, run_hybrid_authority_days,
    run_hybrid_authority_days_with_rayon_phase4,
};
use sim_model::state::HybridWorldState;
use sim_model::storage::canonical_state_hash_from_storage;
use sim_model::{SimConfig, initialize_world};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[path = "common/process_controls.rs"]
mod process_controls;

const BASE_CONFIG: &str = r#"
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
base_weight_matrix = [[0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0]]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

const DAYS: u32 = 3;
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const SAMPLE_TARGET: Duration = Duration::from_millis(1250);
const SAMPLE_FLOOR: Duration = Duration::from_secs(1);
const CHUNK: NonZeroUsize = NonZeroUsize::new(256).unwrap();

#[derive(Clone, Copy, Debug)]
struct Cell {
    id: &'static str,
    workload: &'static str,
    population: u64,
    settlements: u32,
}

const CELLS: [Cell; 8] = [
    Cell {
        id: "A-N10000",
        workload: "A",
        population: 10_000,
        settlements: 2,
    },
    Cell {
        id: "A-N20000",
        workload: "A",
        population: 20_000,
        settlements: 2,
    },
    Cell {
        id: "A-N50000",
        workload: "A",
        population: 50_000,
        settlements: 2,
    },
    Cell {
        id: "A-N100000",
        workload: "A",
        population: 100_000,
        settlements: 2,
    },
    Cell {
        id: "B-N10000",
        workload: "B",
        population: 10_000,
        settlements: 50,
    },
    Cell {
        id: "B-N20000",
        workload: "B",
        population: 20_000,
        settlements: 100,
    },
    Cell {
        id: "B-N50000",
        workload: "B",
        population: 50_000,
        settlements: 250,
    },
    Cell {
        id: "B-N100000",
        workload: "B",
        population: 100_000,
        settlements: 500,
    },
];

#[derive(Clone, Debug, PartialEq, Eq)]
struct Digests {
    state: String,
    metrics: String,
    events: String,
}

struct Pair {
    serial_us: f64,
    rayon_us: f64,
}

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
        .unwrap_or_else(|| PathBuf::from("target/m5_06_m2_rayon_a5.jsonl"));
    if dry_run {
        for cell in CELLS {
            println!(
                "{} N={} K={} expected_sample_floor=41x1.25s plus setup",
                cell.id, cell.population, cell.settlements
            );
        }
        return Ok(());
    }
    let cells: Vec<Cell> = if smoke {
        vec![Cell {
            id: "smoke-A-N100",
            workload: "A",
            population: 100,
            settlements: 2,
        }]
    } else {
        CELLS
            .iter()
            .copied()
            .filter(|cell| selected.is_none_or(|id| id == cell.id))
            .collect()
    };
    if cells.is_empty() {
        return Err(format!("unknown M2 cell: {}", selected.unwrap_or("<none>")).into());
    }
    if smoke && !resume {
        let _ = fs::remove_file(&output);
    }
    let pool = ThreadPoolBuilder::new().num_threads(6).build()?;
    ensure_header(&output)?;
    for (index, cell) in cells.iter().enumerate() {
        let series_started_ms = timestamp_ms();
        let setup_started = Instant::now();
        let config = config_for(*cell)?;
        let config_hash = config_hash(&config, cell);
        if completed(&output, cell.id, &config_hash)? {
            eprintln!(
                "skip {}/{} {} (matching config hash already complete)",
                index + 1,
                cells.len(),
                cell.id
            );
            continue;
        }
        let initial = HybridWorldState::hybrid(initialize_world(&config)?);
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let setup_ns = setup_started.elapsed().as_nanos();
        if smoke {
            let pair = pair_once(&initial, &config, &context, &pool, 1)?;
            append(
                &output,
                json!({"kind":"series","cell_id":cell.id,"config_hash":config_hash,"setup_ns":setup_ns,"series_started_unix_ms":series_started_ms,"series_completed_unix_ms":timestamp_ms(),"smoke":true,"serial_us":pair.serial_us,"rayon_us":pair.rayon_us,"parity":true,"complete":true}),
            )?;
            eprintln!(
                "cell {}/{} {} smoke complete",
                index + 1,
                cells.len(),
                cell.id
            );
            continue;
        }
        let settings = process_controls::apply_high_priority_affinity();
        #[cfg(windows)]
        {
            let current =
                process_controls::current_settings().ok_or("failed to read process controls")?;
            if !settings.priority_set
                || !settings.affinity_set
                || current.priority_class != 0x80
                || current.process_mask != 0x555
            {
                return Err("A-5 priority/affinity controls were not applied".into());
            }
        }
        let run_started = Instant::now();
        let series = measure_cell(&initial, &config, &context, &pool)?;
        append(
            &output,
            json!({"kind":"series","cell_id":cell.id,"workload":cell.workload,"N":cell.population,"K":cell.settlements,"config_hash":config_hash,"setup_ns":setup_ns,"series_started_unix_ms":series_started_ms,"series_completed_unix_ms":timestamp_ms(),"execution_options":{"days_per_call":DAYS,"metrics":true,"events":true,"snapshots":false,"pool_threads":6,"chunk_size":CHUNK.get()},"process_controls":{"priority_set":settings.priority_set,"affinity_set":settings.affinity_set},"series":series,"complete":true}),
        )?;
        eprintln!(
            "cell {}/{} {} elapsed={:.1}s",
            index + 1,
            cells.len(),
            cell.id,
            run_started.elapsed().as_secs_f64()
        );
    }
    Ok(())
}

fn config_for(cell: Cell) -> Result<SimConfig, Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(BASE_CONFIG)?;
    config.world.initial_population = cell.population;
    config.world.settlement_count = cell.settlements;
    config.validate()?;
    Ok(config)
}

fn config_hash(config: &SimConfig, cell: &Cell) -> String {
    let bytes = format!(
        "{BASE_CONFIG}|{}|{}|{}|days={DAYS}|metrics=1|events=1|snapshot=0|pool=6|chunk={}",
        cell.id,
        config.world.initial_population,
        config.world.settlement_count,
        CHUNK.get()
    );
    format!("sha256:{}", hex(&Sha256::digest(bytes.as_bytes())))
}

fn pair_once(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: &rayon::ThreadPool,
    order: usize,
) -> Result<Pair, Box<dyn std::error::Error>> {
    let (serial_us, serial_digest, rayon_us, rayon_digest) = if order.is_multiple_of(2) {
        let serial = run_one(initial, config, context, None)?;
        let rayon = run_one(initial, config, context, Some(pool))?;
        (serial.0, serial.1, rayon.0, rayon.1)
    } else {
        let rayon = run_one(initial, config, context, Some(pool))?;
        let serial = run_one(initial, config, context, None)?;
        (serial.0, serial.1, rayon.0, rayon.1)
    };
    assert_eq!(serial_digest, rayon_digest, "M2 serial/Rayon digest parity");
    Ok(Pair {
        serial_us,
        rayon_us,
    })
}

fn run_one(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: Option<&rayon::ThreadPool>,
) -> Result<(f64, Digests), Box<dyn std::error::Error>> {
    let mut hybrid = initial.clone();
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let started = Instant::now();
    let outcomes = match pool {
        Some(pool) => run_hybrid_authority_days_with_rayon_phase4(
            &mut hybrid,
            config,
            context,
            DAYS,
            &options,
            pool,
            CHUNK,
        )?,
        None => run_hybrid_authority_days(&mut hybrid, config, context, DAYS, &options)?,
    };
    let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
    let storage = hybrid.segmented_storage.as_ref().expect("M2 SoA storage");
    let metrics = outcomes
        .iter()
        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
        .collect::<Vec<_>>();
    let events = outcomes
        .iter()
        .flat_map(|outcome| outcome.events.iter().cloned())
        .collect::<Vec<_>>();
    Ok((
        elapsed_us,
        Digests {
            state: canonical_state_hash_from_storage(
                storage,
                hybrid.world.current_day,
                &hybrid.world.settlements,
            )?
            .to_hex(),
            metrics: canonical_metrics_hash(&metrics)?.to_hex(),
            events: canonical_event_hash(&events)?.to_hex(),
        },
    ))
}

fn measure_cell(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: &rayon::ThreadPool,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut warmup_ns = Vec::with_capacity(WARMUPS);
    for warmup in 0..WARMUPS {
        let started = Instant::now();
        let _ = pair_once(initial, config, context, pool, warmup)?;
        warmup_ns.push(started.elapsed().as_nanos());
    }
    let mut repetitions = 1usize;
    let mut calibration_ns = Vec::new();
    loop {
        let started = Instant::now();
        let mut timed_ns = 0u128;
        for index in 0..repetitions {
            let pair = pair_once(initial, config, context, pool, index)?;
            timed_ns += ((pair.serial_us + pair.rayon_us) * 1_000.0) as u128;
        }
        let elapsed = started.elapsed();
        calibration_ns.push(elapsed.as_nanos());
        if timed_ns >= SAMPLE_TARGET.as_nanos() {
            break;
        }
        repetitions = ((repetitions as f64 * SAMPLE_TARGET.as_nanos() as f64 / timed_ns as f64)
            * 1.1)
            .ceil()
            .max((repetitions + 1) as f64) as usize;
    }
    let mut recalibrations = Vec::new();
    let samples = loop {
        let mut values = Vec::with_capacity(SAMPLES);
        let mut short = false;
        for sample in 0..SAMPLES {
            let sample_started_ms = timestamp_ms();
            let started = Instant::now();
            let mut serial_us = 0.0;
            let mut rayon_us = 0.0;
            for repeat in 0..repetitions {
                let pair = pair_once(initial, config, context, pool, sample + repeat)?;
                serial_us += pair.serial_us;
                rayon_us += pair.rayon_us;
            }
            let elapsed = started.elapsed();
            let timed_ns = ((serial_us + rayon_us) * 1_000.0) as u128;
            if timed_ns < SAMPLE_FLOOR.as_nanos() {
                short = true;
                break;
            }
            values.push(json!({"sample":sample,"started_unix_ms":sample_started_ms,"timed_pair_elapsed_ns":timed_ns,"wall_elapsed_ns":elapsed.as_nanos(),"serial_us_per_call":serial_us/repetitions as f64,"rayon_us_per_call":rayon_us/repetitions as f64}));
        }
        if !short {
            break values;
        }
        if recalibrations.len() >= 3 {
            return Err(
                "sample remained below the one-second A-5 floor after three recalibrations".into(),
            );
        }
        let old = repetitions;
        repetitions = ((repetitions as f64) * 1.5).ceil() as usize;
        recalibrations.push(json!({"old_r":old,"new_r":repetitions,"reason":"recorded paired sample below one-second floor; discarded and restarted series"}));
    };
    Ok(
        json!({"warmups":WARMUPS,"warmup_elapsed_ns":warmup_ns,"samples":SAMPLES,"target_sample_ns":SAMPLE_TARGET.as_nanos(),"floor_sample_ns":SAMPLE_FLOOR.as_nanos(),"calibration_batches_elapsed_ns":calibration_ns,"repetitions_per_sample":repetitions,"recalibrations":recalibrations,"sample_records":samples}),
    )
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
        json!({"kind":"header","stage":"M5-06","adapter":"m2-explicit-rayon","host_os":std::env::consts::OS,"host_arch":std::env::consts::ARCH}),
    )
}

fn completed(
    path: &Path,
    cell_id: &str,
    config_hash: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(false);
    }
    let file = fs::File::open(path)?;
    for line in BufReader::new(file).lines() {
        let value: Value = serde_json::from_str(&line?)?;
        if value["kind"] == "series" && value["cell_id"] == cell_id {
            if value["config_hash"] != config_hash {
                return Err(format!("resume config hash differs for {cell_id}").into());
            }
            return Ok(value["complete"] == true);
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_millis()
}
