use sim_model::hashing::{
    CanonicalHash, DOMAIN_M3P_EVENTS, DOMAIN_M3P_METRICS, DOMAIN_M3P_STATE,
    canonical_m3p_event_bytes, canonical_m3p_event_hash, canonical_m3p_metrics_bytes,
    canonical_m3p_metrics_hash, canonical_m3p_state_bytes, canonical_m3p_state_hash,
};
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pDailyMetrics, M3pEvent, M3pReferenceState, SimConfig,
    initialize_world, run_m3p_reference_day,
};

const M3P_GOLDEN_CONFIG: &str = r#"
profile = "m3p"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 300
settlement_count = 3
initial_health = 1.0
initial_food = 20.0
initial_wealth = 100
initial_settlement_resource = 1.0
initial_treasury = 100

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
carrying_capacity = 100.0
regrowth_rate = 0.1
base_metabolic_cost = 0.2
health_decay_rate = 0.2

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

[population]
birth_min_health = 0.5
birth_food_threshold = 0.1
birth_probability = 0.05
birth_food_cost = 0.1
births_max_per_day = 2
compaction_interval_days = 50

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 1000.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 0.0
trait_weight_aggression = 0.0
trait_weight_risk_tolerance = 0.0
"#;

const GOLDEN_DAYS: [u32; 3] = [100, 250, 500];
const GOLDENS: [(u32, &str, &str, &str); 3] = [
    (
        100,
        "37c0bcf54b6d252f93a63d62fcdd1e4208bf269b62246a1830ff67f3e7c897e8",
        "4df1228faadeb61cc587a820bbd36a12ae1710ff48144ba45fa3568f6c1e229c",
        "d550a6e3a23372584d99e6e61bd8035cd2c14a717301396876b74fa48644d7c9",
    ),
    (
        250,
        "611746807403297bb72d2ebd33097376a2d0cc7eb2560cef1b17722c826eeda8",
        "b0b756c1026334cd8e78dbad22eaf5c55c7eca9a8b4a484c49c5052f48e2afb8",
        "ec6ecdba8b4cfe69d0a5dd66e98b1276515676c6a878252a1697089d5eff4cfc",
    ),
    (
        500,
        "dd82d3c8e960a953e479766180a26f55c0a14e4b5269fa8360a22085df0bc574",
        "7ea57fc999d3d59f6426e8b55e89e1265831fde8226510791eb3a2a0d340eafb",
        "c20930a60b36da0bf860d643909e7861bbf5a71c2dacdca683a7f4a62137830b",
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

#[derive(Debug, Default, PartialEq, Eq)]
struct PopulationCounts {
    births: u64,
    removed: u64,
    replaced_edges: u64,
    dropped_edges: u64,
    mortality_events: u64,
    compactions: u64,
    compactions_within_200_days: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Trace {
    checkpoints: Vec<Checkpoint>,
    counts: PopulationCounts,
}

fn config() -> SimConfig {
    SimConfig::parse_and_validate(M3P_GOLDEN_CONFIG).expect("M3P golden config validates")
}

fn run_context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn fresh_state(config: &SimConfig) -> M3pReferenceState {
    let world = initialize_world(config).expect("golden world initializes");
    M3pReferenceState::new(world, config, &run_context(config)).expect("M3P state initializes")
}

fn run_golden() -> Trace {
    let config = config();
    let run_context = run_context(&config);
    let mut state = fresh_state(&config);
    let mut metrics = Vec::<M3pDailyMetrics>::new();
    let mut events = Vec::new();
    let mut checkpoints = Vec::new();
    let mut counts = PopulationCounts::default();

    for _ in 0..500 {
        let next_day = state.world().current_day.as_u32() + 1;
        let snapshot_boundary = GOLDEN_DAYS.contains(&next_day);
        let outcome = run_m3p_reference_day(
            &mut state,
            &config,
            &run_context,
            &options(snapshot_boundary),
        )
        .expect("M3P golden day executes");
        let day_metrics = outcome.metrics.expect("metrics enabled");
        counts.births += day_metrics.births;
        counts.removed += day_metrics.removed;
        counts.replaced_edges += day_metrics.replaced_edges;
        let mut daily_birth_events = 0;
        let mut cleanup_event = None;

        for event in &outcome.events {
            match &event.event {
                M3pEvent::Birth { .. } => daily_birth_events += 1,
                M3pEvent::Cleanup {
                    removed,
                    replaced_edges,
                    dropped_edges,
                } => {
                    cleanup_event = Some((*removed, *replaced_edges, *dropped_edges));
                    counts.compactions += 1;
                    if outcome.executed_day < 200 {
                        counts.compactions_within_200_days += 1;
                    }
                }
                M3pEvent::Existing(sim_model::Event::StateTransition(
                    sim_model::StateTransitionEvent::MortalityCommitted { .. },
                )) => counts.mortality_events += 1,
                M3pEvent::Existing(_) => {}
            }
        }
        assert_eq!(daily_birth_events, day_metrics.births);
        if let Some((removed, replaced_edges, dropped_edges)) = cleanup_event {
            assert_eq!(removed, day_metrics.removed);
            assert_eq!(replaced_edges, day_metrics.replaced_edges);
            counts.dropped_edges += dropped_edges;
        }
        metrics.push(day_metrics);
        events.extend(outcome.events);

        if snapshot_boundary {
            checkpoints.push(Checkpoint {
                day: next_day,
                state: canonical_m3p_state_hash(&state).expect("M3P State digest encodes"),
                metrics: canonical_m3p_metrics_hash(&metrics).expect("M3P Metrics digest encodes"),
                events: canonical_m3p_event_hash(&events).expect("M3P Events digest encodes"),
                snapshot: outcome
                    .snapshot
                    .expect("snapshot emitted at golden boundary")
                    .bytes,
            });
        }
    }

    Trace {
        checkpoints,
        counts,
    }
}

#[test]
fn m3p_golden_replay_has_births_deaths_and_compaction() {
    let first = run_golden();
    let replay = run_golden();
    assert_eq!(first, replay, "M3P replay must be exact");
    println!("M3P golden population counts: {:?}", first.counts);
    assert_eq!(
        first.checkpoints.iter().map(|c| c.day).collect::<Vec<_>>(),
        GOLDEN_DAYS
    );
    assert!(first.counts.births > 0, "golden run exercises births");
    assert!(
        first.counts.removed > 0,
        "golden run exercises deaths and cleanup"
    );
    assert!(
        first.counts.mortality_events > 0,
        "golden run emits mortality events"
    );
    assert!(
        first.counts.compactions_within_200_days >= 3,
        "golden run has at least three compactions within its first 200 days"
    );

    for actual in &first.checkpoints {
        println!(
            "M3P golden day {}: state={} metrics={} events={}",
            actual.day, actual.state, actual.metrics, actual.events
        );
    }
    for (actual, (day, state, metrics, events)) in first.checkpoints.iter().zip(GOLDENS) {
        assert_eq!(actual.day, day);
        assert_eq!(actual.state.to_hex(), state, "State digest day {day}");
        assert_eq!(actual.metrics.to_hex(), metrics, "Metrics digest day {day}");
        assert_eq!(actual.events.to_hex(), events, "Events digest day {day}");
    }
}

#[test]
fn m3p_digest_preimages_follow_frozen_field_order_and_event_key_order() {
    let config = config();
    let run_context = run_context(&config);
    let mut state = fresh_state(&config);
    let state_bytes = canonical_m3p_state_bytes(&state).unwrap();
    assert!(state_bytes.starts_with(DOMAIN_M3P_STATE));
    assert_eq!(
        &state_bytes[state_bytes.len() - 40..state_bytes.len() - 8],
        state.relationship_graph().csr_hash()
    );
    assert_eq!(
        &state_bytes[state_bytes.len() - 8..],
        &state.next_agent_id().to_le_bytes()
    );

    let outcome =
        run_m3p_reference_day(&mut state, &config, &run_context, &options(false)).unwrap();
    let metric = outcome.metrics.unwrap();
    let metric_bytes = canonical_m3p_metrics_bytes(std::slice::from_ref(&metric)).unwrap();
    assert!(metric_bytes.starts_with(DOMAIN_M3P_METRICS));
    let population_suffix = &metric_bytes[DOMAIN_M3P_METRICS.len() + 4 + 60..];
    let mut expected_suffix = Vec::new();
    for value in [
        metric.births,
        metric.removed,
        metric.replaced_edges,
        metric.live_population,
        metric.next_agent_id,
    ] {
        expected_suffix.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(population_suffix, expected_suffix);

    let event_bytes = canonical_m3p_event_bytes(&outcome.events).unwrap();
    assert!(event_bytes.starts_with(DOMAIN_M3P_EVENTS));
    let first_key = outcome
        .events
        .iter()
        .min_by_key(|record| record.key)
        .unwrap()
        .key;
    let mut expected_key = Vec::new();
    expected_key.extend_from_slice(&first_key.day.to_le_bytes());
    expected_key.push(first_key.phase);
    expected_key.extend_from_slice(&first_key.partition_key.to_le_bytes());
    expected_key.extend_from_slice(&first_key.local_sequence.to_le_bytes());
    assert_eq!(
        &event_bytes[DOMAIN_M3P_EVENTS.len() + 4..DOMAIN_M3P_EVENTS.len() + 4 + 21],
        expected_key
    );
    let reversed: Vec<_> = outcome.events.iter().cloned().rev().collect();
    assert_eq!(event_bytes, canonical_m3p_event_bytes(&reversed).unwrap());
    assert_eq!(
        canonical_m3p_event_hash(&outcome.events).unwrap(),
        canonical_m3p_event_hash(&reversed).unwrap()
    );
    assert!(
        outcome
            .events
            .iter()
            .any(|record| matches!(&record.event, M3pEvent::Cleanup { .. }))
    );
}
