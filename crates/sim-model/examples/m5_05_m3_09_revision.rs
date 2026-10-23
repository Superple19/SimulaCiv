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
#[cfg(windows)]
use std::ffi::c_void;
use std::io::{self, BufRead, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

const N: i64 = 100_000;
const K: i64 = 500;
const DEGREE: i64 = 8;
const LOCALITY: f64 = 0.5;
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const DAYS: u32 = 3;

#[cfg(windows)]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut c_void;
    fn SetPriorityClass(process: *mut c_void, priority: u32) -> i32;
    fn SetProcessAffinityMask(process: *mut c_void, mask: usize) -> i32;
}

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
    #[cfg(windows)]
    {
        let process = unsafe { GetCurrentProcess() };
        let priority = unsafe { SetPriorityClass(process, 0x0000_0080) } != 0;
        let affinity = unsafe { SetProcessAffinityMask(process, 0x555) } != 0;
        assert!(priority && affinity, "M4 A-5 process controls failed");
        "priority=high;affinity_mask=0x555".to_owned()
    }
    #[cfg(not(windows))]
    {
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

fn parse_config(profile: &str, initial_population: i64) -> SimConfig {
    let source =
        std::fs::read_to_string(fixture_path()).expect("read frozen M3P sustained fixture");
    let mut value: toml::Value = toml::from_str(&source).expect("M3P fixture parses");
    value["profile"] = toml::Value::String(profile.to_owned());
    value["world"]["initial_population"] = toml::Value::Integer(initial_population);
    value["world"]["settlement_count"] = toml::Value::Integer(K);
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

fn make_checkpoint(config: &SimConfig) -> M3pReferenceState {
    let world = initialize_world(config).expect("M3P world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3pStorageRunContext::new(&hybrid, config, &run_context)
        .expect("M3P storage context initializes");
    let _ = run_m3p_storage_days(&mut hybrid, config, &mut context, 399, &options(false))
        .expect("M3P checkpoint warm-up");
    let outcome = run_m3p_storage_days(&mut hybrid, config, &mut context, 1, &options(true))
        .expect("M3P checkpoint boundary");
    let snapshot = outcome
        .last()
        .and_then(|day| day.snapshot.as_ref())
        .expect("day-400 snapshot");
    decode_m3p_snapshot(&snapshot.bytes, config)
        .expect("M3P checkpoint restores")
        .state
}

fn prepare() -> Prepared {
    let m3p_config = parse_config("m3p", N);
    let reference = make_checkpoint(&m3p_config);
    let mut m3_world = reference.world().clone();
    m3_world.agents.retain(|agent| agent.alive);
    let live_population = m3_world.agents.len() as u64;
    let m3_config = parse_config("m3", live_population as i64);

    let options_record = format!(
        "initial_population={N};settlements={K};k={DEGREE};p_local={LOCALITY};seed={SEED};replicate={REPLICATE};checkpoint_day=400;days={DAYS};metrics=true;events=true;snapshots=false;policy=serial;pool=0;chunk=256;propagation=false;live_population={live_population}"
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

fn pair_once(prepared: &Prepared, numerator_first: bool) -> (f64, f64, f64) {
    let started = Instant::now();
    let (numerator_us, numerator_hash, denominator_us, denominator_hash) = if numerator_first {
        let (num_us, num_hash) = run_m3p(&prepared.m3p_config, &prepared.reference, true);
        let (den_us, den_hash) = run_m3(&prepared.m3_config, &prepared.m3_world);
        (num_us, num_hash, den_us, den_hash)
    } else {
        let (den_us, den_hash) = run_m3(&prepared.m3_config, &prepared.m3_world);
        let (num_us, num_hash) = run_m3p(&prepared.m3p_config, &prepared.reference, true);
        (num_us, num_hash, den_us, den_hash)
    };
    assert_eq!(
        numerator_hash, prepared.expected_m3p,
        "M3P numerator repeat digest"
    );
    assert_eq!(
        denominator_hash, prepared.expected_m3,
        "M3 denominator repeat digest"
    );
    (
        numerator_us,
        denominator_us,
        started.elapsed().as_secs_f64() * 1_000_000.0,
    )
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
            let (num, den, wall) = pair_once(&prepared, sample % 2 == 0);
            reply(
                &mut writer,
                &format!("PROBE|{sample}|{num:.6}|{den:.6}|{:.6}", wall / 1_000_000.0),
            );
            continue;
        }
        assert!(op == "warmup" || op == "sample", "unknown operation {op}");
        let started = Instant::now();
        let mut numerator_sum = 0.0;
        let mut denominator_sum = 0.0;
        for repeat in 0..repetitions {
            let (numerator, denominator, _) = pair_once(&prepared, (sample + repeat) % 2 == 0);
            numerator_sum += numerator;
            denominator_sum += denominator;
        }
        let wall_seconds = started.elapsed().as_secs_f64();
        assert!(
            wall_seconds >= 1.0,
            "A-5 {op} pair duration below one second"
        );
        reply(
            &mut writer,
            &format!(
                "{}|{}|{}|{:.6}|{:.6}|{:.6}",
                op,
                sample,
                repetitions,
                numerator_sum / repetitions as f64 / f64::from(DAYS),
                denominator_sum / repetitions as f64 / f64::from(DAYS),
                wall_seconds
            ),
        );
    }
}

fn main() {
    assert_eq!(std::env::args().nth(1).as_deref(), Some("--server"));
    server();
}
