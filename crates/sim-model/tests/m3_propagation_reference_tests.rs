use sim_model::hashing::{
    CanonicalHash, canonical_m3prop_event_hash, canonical_m3prop_metrics_hash,
};
use sim_model::m3prop::{
    M3PropagationReferenceState, decode_m3prop_snapshot, run_m3prop_reference_day,
};
use sim_model::{
    DayExecutionOptions, M0RunContext, M3PropDailyMetrics, PropagationDailyCounts, SimConfig,
    initialize_world,
};

const CONFIG: &str = include_str!("fixtures/m3_propagation.toml");

const GOLDEN_DAYS: [u32; 3] = [100, 250, 300];
const GOLDENS: [(&str, &str, &str); 3] = [
    (
        "200e00482f0fcd791125849ee34d6eb459d98415714a3d02084f609543e88045",
        "48d3ca44074bf64453dfe70dbffc69ad3bfbe4229668b00ea6668e7501dd0d8d",
        "1bd1f003f7ef9cc155bfdaecc9bfead15996431bbb0ca8a9fbeac98a326b58a9",
    ),
    (
        "b4b339a6a4ad9b21634a5249b2c6c9156d58a7bd5312c03482aa97d8afe2c5e4",
        "ec008b769ed388d62bb07d6a6221794a7fafcc2b14d669537272fda1a287ae9b",
        "e3dc469e29e2f9e92a160292f1e0756fb954f5a1201d6a3f923224077c78b6db",
    ),
    (
        "44ba2ca612e318399aeb2ad1e87e1730f88a1476e34d8d75d4c895e7faf9ba37",
        "da157af8052ac7decd1070fa2162fe4379e75de13be2592200f683f43562d5f6",
        "ae6243c62942f4b977f4b7b62696c01ccf4ef9b34329d171bcece17495acb1cf",
    ),
];

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

struct Run {
    state: M3PropagationReferenceState,
    metrics: Vec<M3PropDailyMetrics>,
    events: Vec<sim_model::EventRecord>,
    checkpoints: Vec<Checkpoint>,
    snapshots: Vec<(u32, Vec<u8>)>,
    daily: Vec<PropagationDailyCounts>,
}

fn config() -> SimConfig {
    SimConfig::parse_and_validate(CONFIG).expect("M3PROP config validates")
}

fn context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn options(snapshot: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: snapshot,
    }
}

fn fresh() -> Run {
    let config = config();
    let world = initialize_world(&config).expect("world initializes");
    Run {
        state: M3PropagationReferenceState::new(world, &config, &context(&config))
            .expect("M3 propagation state initializes"),
        metrics: Vec::new(),
        events: Vec::new(),
        checkpoints: Vec::new(),
        snapshots: Vec::new(),
        daily: Vec::new(),
    }
}

fn advance(run: &mut Run, end_day: u32) {
    let config = config();
    while run.state.world().current_day.as_u32() < end_day {
        let executed_day = run.state.world().current_day.as_u32();
        let next_day = executed_day + 1;
        let snapshot_boundary = [100, 250].contains(&next_day);
        let outcome = run_m3prop_reference_day(
            &mut run.state,
            &config,
            &context(&config),
            &options(snapshot_boundary),
        )
        .expect("M3 propagation day runs");
        let metric = outcome.metrics.expect("metrics enabled");
        run.daily.push(metric.propagation);
        run.metrics.push(metric);
        run.events.extend(outcome.events);
        if let Some(snapshot) = outcome.snapshot {
            run.snapshots.push((next_day, snapshot.bytes));
        }
        if GOLDEN_DAYS.contains(&next_day) {
            run.checkpoints.push(Checkpoint {
                day: next_day,
                state: run.state.state_hash().unwrap(),
                metrics: canonical_m3prop_metrics_hash(&run.metrics).unwrap(),
                events: canonical_m3prop_event_hash(&run.events).unwrap(),
            });
        }
    }
}

#[test]
fn propagation_reference_golden_and_snapshot_resume() {
    let uninterrupted = {
        let mut run = fresh();
        advance(&mut run, 300);
        run
    };
    let mut paused = fresh();
    advance(&mut paused, 100);
    let snapshot_100 = paused
        .snapshots
        .iter()
        .find(|(day, _)| *day == 100)
        .unwrap()
        .1
        .clone();
    let restored = decode_m3prop_snapshot(&snapshot_100, &config()).unwrap();
    assert_eq!(restored.metadata.day, 100);
    paused.state = restored.state;
    advance(&mut paused, 300);
    assert_eq!(paused.checkpoints, uninterrupted.checkpoints);
    assert_eq!(paused.metrics, uninterrupted.metrics);
    assert_eq!(paused.events, uninterrupted.events);
    assert_eq!(paused.snapshots, uninterrupted.snapshots);

    let actual: Vec<_> = uninterrupted
        .checkpoints
        .iter()
        .map(|checkpoint| {
            (
                checkpoint.state.to_hex(),
                checkpoint.metrics.to_hex(),
                checkpoint.events.to_hex(),
            )
        })
        .collect();
    let expected: Vec<_> = GOLDENS
        .iter()
        .map(|(state, metrics, events)| {
            (
                (*state).to_owned(),
                (*metrics).to_owned(),
                (*events).to_owned(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    let informed: Vec<_> = uninterrupted
        .daily
        .iter()
        .map(|row| row.informed_count)
        .collect();
    let (max_index, max_informed) = informed
        .iter()
        .copied()
        .enumerate()
        .max_by_key(|&(_, count)| count)
        .unwrap();
    assert!(max_informed > informed[0]);
    assert!(
        informed[max_index + 1..]
            .iter()
            .any(|&count| count < max_informed)
    );
    assert!(uninterrupted.daily.iter().any(|row| row.levels_used >= 3));
    assert!(
        uninterrupted
            .daily
            .iter()
            .any(|row| row.max_frontier_size > 1)
    );
    assert!(uninterrupted.daily.iter().any(|row| row.forgotten > 0));
}
