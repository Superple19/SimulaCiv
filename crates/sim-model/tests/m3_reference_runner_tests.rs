use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash,
};
use sim_model::runner::run_hybrid_authority_days;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3DayOutcome, M3ReferenceContext, run_m0_day,
    run_m3_reference_day, run_native_soa_days,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::HybridWorldState;
use sim_model::{M3DailyMetrics, SimConfig, SimulationProfile, initialize_world};

const M3_GOLDEN_CONFIG: &str = r#"
profile = "m3"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 30
settlement_count = 3
initial_health = 1.0
initial_food = 10.0
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
base_metabolic_cost = 2.0
health_decay_rate = 0.15

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
relationship_degree_k = 8
p_local = 0.5

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 100.0, 0.0]
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

const EXPECTED_M2_STATE: &str = "5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9";
const EXPECTED_M2_METRICS: &str =
    "ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b";
const EXPECTED_M2_EVENTS: &str = "2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac";

const GOLDENS: [(u32, &str, &str, &str); 3] = [
    (
        100,
        "6507138d34f495ddf2f148dccc01c84a95bf7e22322a142dc9227197184d279a",
        "1b7dd9df9b441e5163aaf7393eb4ba010b3544b8d62c625e3a9fd614a38bac2b",
        "7003abb86b6c5598a8a362651f34b111493b33ac2900cd6ac533a0466d843769",
    ),
    (
        250,
        "43b75cf172e0f424631e1bf25ad19c8343e8a1e0993282696cf1a7d55d5b24ee",
        "783fc6ecd8a88cc967fcd6a217a8cddabce83f1cd6344184a7ff5fadeacef28c",
        "17fa7f486afc8ed2f73f03c8043384f944f8df04a48241f57b9cee34d036f881",
    ),
    (
        500,
        "41b85bf652cab71670404ba84bae8af3e0800a54e7d719274cef0e324863c9a0",
        "e7f0fba0680849fc910b072cab60db8327d9a471e747309fb56c8f0e5bd9a1ac",
        "621be56f6b6bf663ad14ed9b950e2add436e7973acbe6908ca69b5cdb9736c9c",
    ),
];

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
    snapshot: Vec<u8>,
}

struct RunSoFar {
    world: sim_model::state::WorldState,
    context: M3ReferenceContext,
    metrics: Vec<M3DailyMetrics>,
    events: Vec<sim_model::EventRecord>,
    checkpoints: Vec<Checkpoint>,
    had_cross_partition_interaction: bool,
    initial_alive: usize,
}

fn config() -> SimConfig {
    SimConfig::parse_and_validate(M3_GOLDEN_CONFIG).unwrap()
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn run_until(mut run: RunSoFar, stop_day: u32) -> RunSoFar {
    let config = config();
    while run.world.current_day.as_u32() < stop_day {
        let day = run.world.current_day.as_u32() + 1;
        let boundary = [100, 250, 500].contains(&day);
        let outcome =
            run_m3_reference_day(&mut run.world, &config, &run.context, &options(boundary))
                .unwrap();
        record_outcome(&mut run, outcome);
        if day == stop_day {
            break;
        }
    }
    run
}

fn record_outcome(run: &mut RunSoFar, outcome: M3DayOutcome) {
    let metric = outcome.metrics.unwrap();
    assert_eq!(
        (
            metric.cross_partition_edge_count,
            metric.total_directed_edge_count
        ),
        run.context.relationship_graph().edge_ratio_counts()
    );
    run.had_cross_partition_interaction |= metric.cross_partition_interaction_count > 0;
    run.metrics.push(metric);
    run.events.extend(outcome.events);
    if let Some(snapshot) = outcome.snapshot {
        let day = outcome.executed_day + 1;
        run.checkpoints.push(Checkpoint {
            day,
            state: canonical_m3_state_hash(&run.world, run.context.relationship_graph()).unwrap(),
            metrics: canonical_m3_metrics_hash(&run.metrics).unwrap(),
            events: canonical_event_hash(&run.events).unwrap(),
            snapshot: snapshot.bytes,
        });
    }
}

fn fresh_run() -> RunSoFar {
    let config = config();
    let world = initialize_world(&config).unwrap();
    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let graph_context = M3ReferenceContext::new(&world, &config, &context).unwrap();
    RunSoFar {
        initial_alive: world.agents.iter().filter(|agent| agent.alive).count(),
        world,
        context: graph_context,
        metrics: Vec::new(),
        events: Vec::new(),
        checkpoints: Vec::new(),
        had_cross_partition_interaction: false,
    }
}

#[test]
fn m3_golden_replay_and_snapshot_resume_match() {
    let uninterrupted = run_until(fresh_run(), 500);
    let mut paused = run_until(fresh_run(), 250);
    let snapshot_250 = paused.checkpoints.last().unwrap().snapshot.clone();
    let restored = restore_snapshot(&snapshot_250).unwrap();
    assert_eq!(restored.metadata.day, 250);
    let seed_context = M0RunContext::new(
        restored.metadata.master_seed,
        restored.metadata.replicate_id,
    );
    let regenerated = M3ReferenceContext::new(&restored.world, &config(), &seed_context).unwrap();
    assert_eq!(
        regenerated.relationship_graph().csr_hash(),
        paused.context.relationship_graph().csr_hash()
    );
    paused.world = restored.world;
    paused.context = regenerated;
    paused = run_until(paused, 500);

    assert_eq!(paused.checkpoints, uninterrupted.checkpoints);
    assert_eq!(paused.metrics, uninterrupted.metrics);
    assert_eq!(paused.events, uninterrupted.events);
    assert!(uninterrupted.had_cross_partition_interaction);
    assert!(
        uninterrupted
            .world
            .agents
            .iter()
            .filter(|agent| agent.alive)
            .count()
            < uninterrupted.initial_alive
    );

    for ((day, expected_state, expected_metrics, expected_events), actual) in
        GOLDENS.iter().zip(&uninterrupted.checkpoints)
    {
        assert_eq!(*day, actual.day);
        assert_eq!(*expected_state, actual.state.to_hex(), "state day {day}");
        assert_eq!(
            *expected_metrics,
            actual.metrics.to_hex(),
            "metrics day {day}"
        );
        assert_eq!(*expected_events, actual.events.to_hex(), "events day {day}");
    }
    let final_checkpoint = uninterrupted.checkpoints.last().unwrap();
    assert_ne!(final_checkpoint.state.to_hex(), EXPECTED_M2_STATE);
    assert_ne!(final_checkpoint.metrics.to_hex(), EXPECTED_M2_METRICS);
    assert_ne!(final_checkpoint.events.to_hex(), EXPECTED_M2_EVENTS);
}

#[test]
fn m2_runners_reject_m3_profile_and_m3_runner_rejects_m2() {
    let m3_config = config();
    let run_context = M0RunContext::new(m3_config.world.master_seed, m3_config.world.replicate_id);
    let mut world = initialize_world(&m3_config).unwrap();
    assert!(run_m0_day(&mut world, &m3_config, &run_context, &options(false)).is_err());
    assert!(run_native_soa_days(&mut world, &m3_config, &run_context, 0, &options(false)).is_err());
    let mut hybrid = HybridWorldState::legacy(initialize_world(&m3_config).unwrap());
    assert!(
        run_hybrid_authority_days(&mut hybrid, &m3_config, &run_context, 0, &options(false))
            .is_err()
    );

    let mut m2_config = m3_config;
    m2_config.profile = SimulationProfile::M2;
    assert!(M3ReferenceContext::new(&world, &m2_config, &run_context).is_err());
}
