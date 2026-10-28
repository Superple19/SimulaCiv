use rayon::ThreadPoolBuilder;
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
    timed_day_ids: Vec<u32>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let smoke = args.iter().any(|arg| arg == "--smoke");
    let resume = args.iter().any(|arg| arg == "--resume");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let selected = args.iter().find_map(|arg| arg.strip_prefix("--cell="));
    let config_dir = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--config-dir="))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("SIMCIV_M5_08_M2_CONFIG_DIR").map(PathBuf::from));
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
    let cells: Vec<Cell> = CELLS
        .iter()
        .copied()
        .filter(|cell| selected.is_none_or(|id| id == cell.id))
        .collect();
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
        let (config, config_hash) = config_for(
            *cell,
            config_dir
                .as_deref()
                .ok_or("historical M2 config directory is required")?,
        )?;
        let protocol_hash = protocol_hash(&config, cell);
        if completed(&output, cell.id, &config_hash, &protocol_hash)? {
            eprintln!(
                "skip {}/{} {} (matching config hash already complete)",
                index + 1,
                cells.len(),
                cell.id
            );
            continue;
        }
        let initialized_world = initialize_world(&config)?;
        if initialized_world.current_day.as_u32() != 0 {
            return Err("M2 source configuration must initialize at day 0".into());
        }
        let initial = HybridWorldState::hybrid(initialized_world);
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let setup_ns = setup_started.elapsed().as_nanos();
        if smoke {
            let pair = pair_once(&initial, &config, &context, &pool, 1)?;
            append(
                &output,
                &format!(
                    "{{\"kind\":\"series\",\"cell_id\":\"{}\",\"config_hash\":\"{}\",\"protocol_hash\":\"{}\",\"checkpoint_path\":\"initialize_world at day 0\",\"setup_ns\":{},\"series_started_unix_ms\":{},\"series_completed_unix_ms\":{},\"timed_day_ids\":{},\"smoke\":true,\"serial_us\":{},\"rayon_us\":{},\"parity\":true,\"complete\":true}}",
                    cell.id,
                    config_hash,
                    protocol_hash,
                    setup_ns,
                    series_started_ms,
                    timestamp_ms(),
                    format_day_ids(&pair.timed_day_ids),
                    pair.serial_us,
                    pair.rayon_us
                ),
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
            &format!(
                "{{\"kind\":\"series\",\"cell_id\":\"{}\",\"workload\":\"{}\",\"N\":{},\"K\":{},\"config_hash\":\"{}\",\"protocol_hash\":\"{}\",\"checkpoint_path\":\"initialize_world at day 0\",\"setup_ns\":{},\"series_started_unix_ms\":{},\"series_completed_unix_ms\":{},\"timed_day_ids\":[0,1,2],\"execution_options\":{{\"days_per_call\":{},\"metrics\":true,\"events\":true,\"snapshots\":false,\"pool_threads\":6,\"chunk_size\":{}}},\"process_controls\":{{\"priority_set\":{},\"affinity_set\":{}}},\"series\":{},\"complete\":true}}",
                cell.id,
                cell.workload,
                cell.population,
                cell.settlements,
                config_hash,
                protocol_hash,
                setup_ns,
                series_started_ms,
                timestamp_ms(),
                DAYS,
                CHUNK.get(),
                settings.priority_set,
                settings.affinity_set,
                series
            ),
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

fn config_for(
    cell: Cell,
    config_dir: &Path,
) -> Result<(SimConfig, String), Box<dyn std::error::Error>> {
    let path = config_dir.join(format!("{}.toml", cell.id));
    let text = fs::read_to_string(&path)?;
    let hash = format!("sha256:{}", hex(&Sha256::digest(text.as_bytes())));
    let expected = fs::read_to_string(config_dir.join(format!("{}.sha256", cell.id)))?;
    if expected.trim() != hash {
        return Err(format!("historical M2 config hash mismatch for {}", cell.id).into());
    }
    let config = SimConfig::parse_and_validate(&text)?;
    if config.world.initial_population != cell.population
        || config.world.settlement_count != cell.settlements
    {
        return Err(format!("historical M2 config dimensions mismatch for {}", cell.id).into());
    }
    Ok((config, hash))
}

fn protocol_hash(config: &SimConfig, cell: &Cell) -> String {
    let bytes = format!(
        "M2-40-source-config|{}|{}|days={DAYS}|metrics=1|events=1|snapshot=0|pool=6|chunk={}",
        cell.id,
        config.world.initial_population,
        CHUNK.get()
    );
    format!("sha256:{}", hex(&Sha256::digest(bytes.as_bytes())))
}

fn format_day_ids(days: &[u32]) -> String {
    format!(
        "[{}]",
        days.iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn pair_once(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: &rayon::ThreadPool,
    order: usize,
) -> Result<Pair, Box<dyn std::error::Error>> {
    let (serial_us, serial_digest, serial_days, rayon_us, rayon_digest, rayon_days) =
        if order.is_multiple_of(2) {
            let serial = run_one(initial, config, context, None)?;
            let rayon = run_one(initial, config, context, Some(pool))?;
            (serial.0, serial.1, serial.2, rayon.0, rayon.1, rayon.2)
        } else {
            let rayon = run_one(initial, config, context, Some(pool))?;
            let serial = run_one(initial, config, context, None)?;
            (serial.0, serial.1, serial.2, rayon.0, rayon.1, rayon.2)
        };
    assert_eq!(serial_digest, rayon_digest, "M2 serial/Rayon digest parity");
    assert_eq!(serial_days, rayon_days, "M2 serial/Rayon timed day ids");
    let expected_days = (0..DAYS).collect::<Vec<_>>();
    if serial_days != expected_days {
        return Err(format!(
            "M2 timed day ids differ: expected {expected_days:?}, got {serial_days:?}"
        )
        .into());
    }
    Ok(Pair {
        serial_us,
        rayon_us,
        timed_day_ids: serial_days,
    })
}

fn run_one(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: Option<&rayon::ThreadPool>,
) -> Result<(f64, Digests, Vec<u32>), Box<dyn std::error::Error>> {
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
    let timed_day_ids = outcomes
        .iter()
        .map(|outcome| outcome.executed_day)
        .collect::<Vec<_>>();
    if timed_day_ids != (0..DAYS).collect::<Vec<_>>() {
        return Err(format!("M2 runner returned unexpected timed days: {timed_day_ids:?}").into());
    }
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
        timed_day_ids,
    ))
}

fn measure_cell(
    initial: &HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    pool: &rayon::ThreadPool,
) -> Result<String, Box<dyn std::error::Error>> {
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
            values.push(format!("{{\"sample\":{},\"started_unix_ms\":{},\"timed_pair_elapsed_ns\":{},\"wall_elapsed_ns\":{},\"timed_day_ids\":[0,1,2],\"serial_us_per_call\":{},\"rayon_us_per_call\":{}}}",sample,sample_started_ms,timed_ns,elapsed.as_nanos(),serial_us/repetitions as f64,rayon_us/repetitions as f64));
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
        recalibrations.push(format!("{{\"old_r\":{},\"new_r\":{},\"reason\":\"recorded paired sample below one-second floor; discarded and restarted series\"}}",old,repetitions));
    };
    Ok(format!(
        "{{\"warmups\":{},\"warmup_elapsed_ns\":[{}],\"samples\":{},\"target_sample_ns\":{},\"floor_sample_ns\":{},\"calibration_batches_elapsed_ns\":[{}],\"repetitions_per_sample\":{},\"recalibrations\":[{}],\"sample_records\":[{}]}}",
        WARMUPS,
        warmup_ns
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(","),
        SAMPLES,
        SAMPLE_TARGET.as_nanos(),
        SAMPLE_FLOOR.as_nanos(),
        calibration_ns
            .iter()
            .map(u128::to_string)
            .collect::<Vec<_>>()
            .join(","),
        repetitions,
        recalibrations.join(","),
        samples.join(",")
    ))
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
        &format!(
            "{{\"kind\":\"header\",\"stage\":\"M5-06\",\"adapter\":\"m2-explicit-rayon\",\"host_os\":\"{}\",\"host_arch\":\"{}\"}}",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
    )
}

fn completed(
    path: &Path,
    cell_id: &str,
    config_hash: &str,
    protocol_hash: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    if !path.exists() {
        return Ok(false);
    }
    let file = fs::File::open(path)?;
    for line in BufReader::new(file).lines() {
        let value = line?;
        if json_field(&value, "kind") == Some("series")
            && json_field(&value, "cell_id") == Some(cell_id)
        {
            if json_field(&value, "config_hash") != Some(config_hash)
                || json_field(&value, "protocol_hash") != Some(protocol_hash)
            {
                return Err(format!("resume config hash differs for {cell_id}").into());
            }
            return Ok(value.contains("\"complete\":true"));
        }
    }
    Ok(false)
}

fn append(path: &Path, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let mut writer = BufWriter::new(file);
    writer.write_all(value.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn json_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("\"{key}\":\"");
    let value = line.get(line.find(&marker)? + marker.len()..)?;
    value.split_once('"').map(|(field, _)| field)
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
