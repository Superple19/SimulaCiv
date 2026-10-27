use sha2::Digest;
use sim_model::hashing::{
    canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash_from_storage,
    canonical_m3p_event_hash, canonical_m3p_metrics_hash, canonical_m3p_state_hash_from_storage,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3StorageRunContext,
    M3pIncrementalTopologyStorageRunContext, M3pStorageRunContext, run_m3_storage_days,
    run_m3p_storage_days, run_m3p_storage_days_incremental_topology,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::{M3pReferenceState, SimConfig, decode_m3p_snapshot, initialize_world};
use std::io::{self, BufRead, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

#[path = "common/a5.rs"]
mod a5;

const N: i64 = 100_000;
const K: i64 = 500;
const DEGREE: i64 = 8;
const LOCALITY: f64 = 0.5;
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const DAYS: u32 = 3;

#[path = "common/process_controls.rs"]
mod process_controls;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct Digests {
    state: String,
    metrics: String,
    events: String,
}

struct Prepared {
    m3p_config: SimConfig,
    m3_config: SimConfig,
    reference: M3pReferenceState,
    m3_world: WorldState,
    expected_m3p: Digests,
    expected_m3: Digests,
    options: String,
    live_population: u64,
    config_sha256: String,
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn windows_controls() -> String {
    let status = process_controls::apply_high_priority_affinity();
    if cfg!(windows) {
        assert!(
            status.priority_set && status.affinity_set,
            "M4 A-5 process controls failed"
        );
        "priority=high;affinity_mask=0x555".to_owned()
    } else {
        "priority=not-applied;affinity_mask=not-applied".to_owned()
    }
}

fn fixture_path() -> PathBuf {
    if let Some(path) = std::env::var_os("SIMCIV_M5_05_CONFIG") {
        return PathBuf::from(path);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("m3p_propagation_sustained.toml")
}

fn parse_config(profile: &str, initial_population: i64, settlement_count: i64) -> SimConfig {
    let source =
        std::fs::read_to_string(fixture_path()).expect("read frozen M3P sustained fixture");
    let mut value: toml::Value = toml::from_str(&source).expect("M3P fixture parses");
    value["profile"] = toml::Value::String(profile.to_owned());
    value["world"]["initial_population"] = toml::Value::Integer(initial_population);
    value["world"]["settlement_count"] = toml::Value::Integer(settlement_count);
    value["world"]["master_seed"] = toml::Value::Integer(SEED as i64);
    value["world"]["replicate_id"] = toml::Value::Integer(i64::from(REPLICATE));
    value["interaction"]["relationship_degree_k"] = toml::Value::Integer(DEGREE);
    value["interaction"]["p_local"] = toml::Value::Float(LOCALITY);
    value
        .as_table_mut()
        .expect("root config table")
        .remove("simulation");
    value
        .as_table_mut()
        .expect("root config table")
        .remove("propagation");
    if profile == "m3" {
        value
            .as_table_mut()
            .expect("root config table")
            .remove("population");
    }
    let serialized = toml::to_string(&value).expect("config serializes");
    SimConfig::parse_and_validate(&serialized).expect("frozen diagnosis config validates")
}

fn make_checkpoint(config: &SimConfig, checkpoint_cursor: u32) -> M3pReferenceState {
    let world = initialize_world(config).expect("M3P world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3pStorageRunContext::new(&hybrid, config, &run_context)
        .expect("M3P storage context initializes");
    let warm_days = checkpoint_cursor
        .checked_sub(1)
        .expect("checkpoint cursor must be positive");
    let _ = run_m3p_storage_days(
        &mut hybrid,
        config,
        &mut context,
        warm_days,
        &options(false),
    )
    .expect("M3P checkpoint warm-up");
    let outcome = run_m3p_storage_days(&mut hybrid, config, &mut context, 1, &options(true))
        .expect("M3P checkpoint boundary");
    let snapshot = outcome
        .last()
        .and_then(|day| day.snapshot.as_ref())
        .expect("checkpoint snapshot");
    let state = decode_m3p_snapshot(&snapshot.bytes, config)
        .expect("M3P checkpoint restores")
        .state;
    assert_eq!(
        state.world().current_day.as_u32(),
        checkpoint_cursor,
        "snapshot cursor must match the requested next day"
    );
    state
}

fn prepare_for(initial_population: i64, settlement_count: i64, checkpoint_cursor: u32) -> Prepared {
    let m3p_config = parse_config("m3p", initial_population, settlement_count);
    let reference = make_checkpoint(&m3p_config, checkpoint_cursor);
    let mut m3_world = reference.world().clone();
    m3_world.agents.retain(|agent| agent.alive);
    let live_population = m3_world.agents.len() as u64;
    let m3_config = parse_config("m3", live_population as i64, settlement_count);

    let checkpoint_scope = if checkpoint_cursor == 400 && initial_population == N {
        format!("checkpoint_day=400;days={DAYS}")
    } else {
        let timed_days = (checkpoint_cursor..checkpoint_cursor + DAYS)
            .map(|day| day.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!("checkpoint_cursor={checkpoint_cursor};timed_days={timed_days}")
    };
    let options_record = format!(
        "initial_population={initial_population};settlements={settlement_count};k={DEGREE};p_local={LOCALITY};seed={SEED};replicate={REPLICATE};{checkpoint_scope};metrics=true;events=true;snapshots=false;policy=serial;pool=0;chunk=256;propagation=false;live_population={live_population}"
    );

    let (_, expected_m3p) = run_m3p(&m3p_config, &reference, false);
    let (_, expected_incremental) = run_m3p(&m3p_config, &reference, true);
    assert_eq!(
        expected_m3p, expected_incremental,
        "M3P serial/incremental parity"
    );
    let (_, expected_m3) = run_m3(&m3_config, &m3_world);
    let fixture = std::fs::read(fixture_path()).expect("read fixture for config hash");
    let config_sha256 = hex(&sha2::Sha256::digest(fixture));

    Prepared {
        m3p_config,
        m3_config,
        reference,
        m3_world,
        expected_m3p,
        expected_m3,
        options: options_record,
        live_population,
        config_sha256,
    }
}

fn prepare() -> Prepared {
    let initial_population = std::env::var("SIMCIV_M5_05_N")
        .ok()
        .map(|value| value.parse::<i64>().expect("population is an i64"))
        .unwrap_or(N);
    let settlement_count = std::env::var("SIMCIV_M5_05_K")
        .ok()
        .map(|value| value.parse::<i64>().expect("settlement count is an i64"))
        .unwrap_or(K);
    let checkpoint_cursor = std::env::var("SIMCIV_M5_05_CHECKPOINT_CURSOR")
        .ok()
        .map(|value| value.parse::<u32>().expect("checkpoint cursor is a u32"))
        .unwrap_or(400);
    prepare_for(initial_population, settlement_count, checkpoint_cursor)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn m3p_digests(
    hybrid: &HybridWorldState,
    outcomes: &[sim_model::M3pDayOutcome],
    graph: &sim_model::RelationshipGraph,
    next_agent_id: u64,
) -> Digests {
    let storage = hybrid.storage().expect("M3P storage");
    Digests {
        state: canonical_m3p_state_hash_from_storage(
            storage,
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
            next_agent_id,
        )
        .expect("M3P state hash")
        .to_hex(),
        metrics: canonical_m3p_metrics_hash(
            &outcomes
                .iter()
                .map(|day| day.metrics.clone().expect("M3P metrics"))
                .collect::<Vec<_>>(),
        )
        .expect("M3P metrics hash")
        .to_hex(),
        events: canonical_m3p_event_hash(
            &outcomes
                .iter()
                .flat_map(|day| day.events.iter().cloned())
                .collect::<Vec<_>>(),
        )
        .expect("M3P event hash")
        .to_hex(),
    }
}

fn run_m3p(config: &SimConfig, reference: &M3pReferenceState, incremental: bool) -> (f64, Digests) {
    let mut hybrid = HybridWorldState::hybrid(reference.world().clone());
    let run_context = M0RunContext::new(SEED, REPLICATE);
    if incremental {
        let mut context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
            &hybrid,
            config,
            &run_context,
            reference,
        )
        .expect("incremental M3P context");
        let started = Instant::now();
        let outcomes = run_m3p_storage_days_incremental_topology(
            &mut hybrid,
            config,
            &mut context,
            DAYS,
            &options(false),
        )
        .expect("incremental M3P runner");
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        let hashes = m3p_digests(
            &hybrid,
            &outcomes,
            context.relationship_graph(),
            context.next_agent_id(),
        );
        (elapsed, hashes)
    } else {
        let mut context =
            M3pStorageRunContext::from_reference_state(&hybrid, config, &run_context, reference)
                .expect("serial M3P context");
        let started = Instant::now();
        let outcomes =
            run_m3p_storage_days(&mut hybrid, config, &mut context, DAYS, &options(false))
                .expect("serial M3P runner");
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        let hashes = m3p_digests(
            &hybrid,
            &outcomes,
            context.relationship_graph(),
            context.next_agent_id(),
        );
        (elapsed, hashes)
    }
}

fn m3_digests(
    hybrid: &HybridWorldState,
    outcomes: &[sim_model::M3DayOutcome],
    graph: &sim_model::RelationshipGraph,
) -> Digests {
    let storage = hybrid.storage().expect("M3 storage");
    Digests {
        state: canonical_m3_state_hash_from_storage(
            storage,
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
        )
        .expect("M3 state hash")
        .to_hex(),
        metrics: canonical_m3_metrics_hash(
            &outcomes
                .iter()
                .map(|day| day.metrics.clone().expect("M3 metrics"))
                .collect::<Vec<_>>(),
        )
        .expect("M3 metrics hash")
        .to_hex(),
        events: canonical_event_hash(
            &outcomes
                .iter()
                .flat_map(|day| day.events.iter().cloned())
                .collect::<Vec<_>>(),
        )
        .expect("M3 event hash")
        .to_hex(),
    }
}

fn run_m3(config: &SimConfig, world: &WorldState) -> (f64, Digests) {
    let mut hybrid = HybridWorldState::hybrid(world.clone());
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3StorageRunContext::new(&hybrid, config, &run_context).expect("M3 context");
    let graph = context.relationship_graph().clone();
    let started = Instant::now();
    let outcomes = run_m3_storage_days(&mut hybrid, config, &mut context, DAYS, &options(false))
        .expect("M3 runner");
    let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
    let hashes = m3_digests(&hybrid, &outcomes, &graph);
    (elapsed, hashes)
}

fn validate_m3p_call(prepared: &Prepared) -> f64 {
    let mut hybrid = HybridWorldState::hybrid(prepared.reference.world().clone());
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3pStorageRunContext::from_reference_state(
        &hybrid,
        &prepared.m3p_config,
        &run_context,
        &prepared.reference,
    )
    .expect("M3P validation context");
    let started = Instant::now();
    let _ = run_m3p_storage_days(
        &mut hybrid,
        &prepared.m3p_config,
        &mut context,
        0,
        &options(false),
    )
    .expect("zero-day M3P validation call");
    started.elapsed().as_secs_f64() * 1_000_000.0
}

fn run_path(prepared: &Prepared, path: usize) -> (f64, Digests) {
    let (elapsed, digests) = match path {
        0 => run_m3p(&prepared.m3p_config, &prepared.reference, false),
        1 => run_m3p(&prepared.m3p_config, &prepared.reference, true),
        2 => run_m3(&prepared.m3_config, &prepared.m3_world),
        _ => panic!("unknown equal-live path {path}"),
    };
    let expected = if path < 2 {
        &prepared.expected_m3p
    } else {
        &prepared.expected_m3
    };
    assert_eq!(digests, *expected, "equal-live runner repeat digest");
    (elapsed, digests)
}

fn pair_once(prepared: &Prepared, rotation: usize) -> ([f64; 3], f64) {
    let started = Instant::now();
    let order = match rotation % 3 {
        0 => [0, 1, 2],
        1 => [1, 2, 0],
        _ => [2, 0, 1],
    };
    let mut timings = [0.0; 3];
    for path in order {
        let (elapsed, _) = run_path(prepared, path);
        timings[path] = elapsed;
    }
    (timings, started.elapsed().as_secs_f64() * 1_000_000.0)
}

fn m3_09_cells() -> [&'static str; 4] {
    [
        "N10k-paired",
        "N10k-noninterleaved",
        "N100k-paired",
        "N100k-noninterleaved",
    ]
}

fn cell_config(cell: &str, smoke: bool) -> (i64, i64, bool, u32) {
    if smoke {
        return (300, 3, cell.contains("paired"), 4);
    }
    let population = if cell.starts_with("N10k-") {
        10_000
    } else if cell.starts_with("N100k-") {
        100_000
    } else {
        panic!("unknown M3-09 cell {cell}")
    };
    let paired = cell.ends_with("-paired");
    let noninterleaved = cell.ends_with("-noninterleaved");
    assert!(paired ^ noninterleaved, "cell schedule is explicit");
    (population, population / 200, paired, 400)
}

fn m3_09_config_hash(
    cell: &str,
    population: i64,
    settlement_count: i64,
    paired: bool,
    cursor: u32,
) -> String {
    a5::config_hash(&format!(
        "{cell}|N={population}|K={settlement_count}|k={DEGREE}|p_local={LOCALITY}|seed={SEED}|replicate={REPLICATE}|checkpoint_cursor={cursor}|days={DAYS}|metrics=true|events=true|snapshots=false|schedule={}",
        if paired {
            "paired-rotation-m3p-serial-incremental-m3"
        } else {
            "noninterleaved-m3p-serial-then-incremental-then-m3"
        }
    ))
}

fn a5_controls() -> (bool, bool) {
    let status = process_controls::apply_high_priority_affinity();
    if cfg!(windows) {
        assert!(
            status.priority_set && status.affinity_set,
            "A-5 priority/affinity setup failed"
        );
    }
    (status.priority_set, status.affinity_set)
}

fn a5_record(
    cell: &str,
    config_hash: &str,
    paired: bool,
    series: &a5::Series,
    controls: (bool, bool),
) -> String {
    let samples = (0..a5::SAMPLES).map(|sample| {
        format!(
            "{{\"serial_us_per_call\":{:.9},\"rayon_us_per_call\":{:.9},\"m3p_serial_us_per_call\":{:.9},\"timed_pair_elapsed_ns\":{:.0}}}",
            series.path_samples_us[1][sample],
            series.path_samples_us[2][sample],
            series.path_samples_us[0][sample],
            series.sample_seconds[sample] * 1_000_000_000.0,
        )
    }).collect::<Vec<_>>().join(",");
    let repetitions = series.repetitions_per_path.first().copied().unwrap_or(1);
    let recalibrations = a5::recalibrations_json(series);
    let series_json = format!(
        "{{\"warmups\":{},\"samples\":{},\"repetitions_per_sample\":{repetitions},\"sample_records\":[{samples}],\"sample_seconds\":{},\"path_mad_over_median\":{{\"serial\":{:.9},\"rayon\":{:.9}}},\"recalibrations\":{recalibrations}}}",
        a5::WARMUPS,
        a5::SAMPLES,
        a5::json_numbers(&series.sample_seconds),
        a5::mad_over_median(&series.path_samples_us[1]),
        a5::mad_over_median(&series.path_samples_us[2]),
    );
    format!(
        "{{\"kind\":\"series\",\"cell_id\":\"{cell}\",\"complete\":true,\"smoke\":false,\"config_hash\":\"{config_hash}\",\"schedule\":\"{}\",\"paths\":[\"m3p_serial\",\"m3p_incremental\",\"m3\"],\"ratio_mapping\":\"m3p_incremental/m3\",\"parity_asserted\":true,\"process_controls\":{{\"priority_set\":{},\"affinity_set\":{}}},\"series\":{series_json}}}",
        if paired {
            "paired-rotation"
        } else {
            "noninterleaved"
        },
        controls.0,
        controls.1,
    )
}

fn append_a5(path: &std::path::Path, record: &str) {
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open A-5 raw JSONL");
    writeln!(output, "{record}").expect("append A-5 cell series");
    output.flush().expect("flush A-5 cell series");
}

fn run_a5_cell(cell: &str, output: &std::path::Path, smoke: bool) {
    let (population, settlements, paired, cursor) = cell_config(cell, smoke);
    let config_hash = m3_09_config_hash(cell, population, settlements, paired, cursor);
    if !smoke {
        let existing = std::fs::read_to_string(output).unwrap_or_default();
        if let Some(line) = existing
            .lines()
            .find(|line| line.contains(&format!("\"cell_id\":\"{cell}\"")))
        {
            assert!(
                line.contains(&format!("\"config_hash\":\"{config_hash}\"")),
                "existing M3-09 cell has a stale config hash: {cell}"
            );
            println!("A5_SKIP|cell={cell}");
            return;
        }
    }
    let controls = a5_controls();
    let prepared = prepare_for(population, settlements, cursor);
    if smoke {
        let expected = if paired {
            let (times, _) = pair_once(&prepared, 0);
            times
        } else {
            [
                run_path(&prepared, 0).0,
                run_path(&prepared, 1).0,
                run_path(&prepared, 2).0,
            ]
        };
        assert!(expected.iter().all(|value| *value > 0.0));
        println!("A5_SMOKE_OK|cell={cell}|parity=true|timing_recorded=false");
        return;
    }
    let series = if paired {
        a5::paired_series(3, |sample, repeat| {
            let (values, _) = pair_once(&prepared, sample + repeat);
            let paths = values.to_vec();
            (paths.clone(), paths.iter().sum::<f64>() / 1_000_000.0)
        })
    } else {
        a5::noninterleaved_series(3, |path, _, _| {
            let (elapsed, _) = run_path(&prepared, path);
            (elapsed, elapsed / 1_000_000.0)
        })
    };
    let record = a5_record(cell, &config_hash, paired, &series, controls);
    append_a5(output, &record);
    println!("A5_CELL_COMPLETE|cell={cell}");
}

fn a5_cli(args: &[String]) -> bool {
    match args.get(1).map(String::as_str) {
        Some("--a5-cell") => {
            let cell = args.get(2).expect("--a5-cell needs a cell id");
            let output = PathBuf::from(args.get(3).expect("--a5-cell needs a JSONL path"));
            run_a5_cell(cell, &output, false);
            true
        }
        Some("--a5-all") => {
            let output = PathBuf::from(args.get(2).expect("--a5-all needs a JSONL path"));
            for cell in m3_09_cells() {
                run_a5_cell(cell, &output, false);
            }
            true
        }
        Some("--a5-smoke") => {
            let cell = args.get(2).expect("--a5-smoke needs a cell id");
            run_a5_cell(cell, std::path::Path::new(""), true);
            true
        }
        _ => false,
    }
}

fn reply(writer: &mut BufWriter<io::StdoutLock<'_>>, value: &str) {
    writeln!(writer, "{value}").expect("write worker response");
    writer.flush().expect("flush worker response");
}

fn server() {
    let controls = windows_controls();
    let prepared = prepare();
    let revision_id = std::env::var("SIMCIV_M5_05_REVISION_ID").unwrap_or_default();
    let commit = std::env::var("SIMCIV_M5_05_REVISION_COMMIT").unwrap_or_default();
    let mut writer = BufWriter::new(io::stdout().lock());
    reply(
        &mut writer,
        &format!(
            "READY|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            revision_id,
            commit,
            prepared.config_sha256,
            prepared.live_population,
            prepared.options,
            prepared.expected_m3p.state,
            prepared.expected_m3p.metrics,
            prepared.expected_m3p.events,
            prepared.expected_m3.state,
            prepared.expected_m3.metrics,
            prepared.expected_m3.events,
            controls
        ),
    );

    for line in io::stdin().lock().lines() {
        let line = line.expect("read server command");
        if line.is_empty() {
            continue;
        }
        let fields = line.split('|').collect::<Vec<_>>();
        let op = fields.first().copied().expect("command operation");
        let sample = fields
            .get(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let repetitions = fields
            .get(2)
            .and_then(|value| value.parse().ok())
            .unwrap_or(1);
        if op == "stop" {
            reply(&mut writer, "STOPPED");
            break;
        }
        if op == "probe" {
            let (times, wall) = pair_once(&prepared, sample as usize);
            reply(
                &mut writer,
                &format!(
                    "PROBE|{sample}|{:.6}|{:.6}|{:.6}|{:.6}",
                    times[0],
                    times[1],
                    times[2],
                    wall / 1_000_000.0
                ),
            );
            continue;
        }
        if op == "validate" {
            eprintln!("M5_VALIDATE_REQUEST={sample}");
            let elapsed = validate_m3p_call(&prepared);
            reply(&mut writer, &format!("VALIDATE|{sample}|{elapsed:.6}"));
            continue;
        }
        assert!(op == "warmup" || op == "sample", "unknown operation {op}");
        let started = Instant::now();
        let mut path_sums = [0.0; 3];
        for repeat in 0..repetitions {
            let (times, _) = pair_once(&prepared, (sample + repeat) as usize);
            for (sum, time) in path_sums.iter_mut().zip(times) {
                *sum += time;
            }
        }
        let wall_seconds = started.elapsed().as_secs_f64();
        assert!(
            wall_seconds >= 1.0,
            "A-5 {op} pair duration below one second"
        );
        reply(
            &mut writer,
            &format!(
                "{}|{}|{}|{:.6}|{:.6}|{:.6}|{:.6}",
                op,
                sample,
                repetitions,
                path_sums[0] / repetitions as f64 / f64::from(DAYS),
                path_sums[1] / repetitions as f64 / f64::from(DAYS),
                path_sums[2] / repetitions as f64 / f64::from(DAYS),
                wall_seconds
            ),
        );
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if a5_cli(&args) {
        return;
    }
    assert_eq!(args.get(1).map(String::as_str), Some("--server"));
    server();
}
