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
initial_food = 15.0
initial_wealth = 100
initial_settlement_resource = 1000000.0
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
carrying_capacity = 1000000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.2

[economy]
base_work_yield = 1.0
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
birth_food_threshold = 0.5
birth_probability = 0.05
birth_food_cost = 0.5
births_max_per_day = 1
compaction_interval_days = 50

[decision]
decision_temperature = 1.0
action_biases = [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]
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
        "cd0c6efbcbce40784f74223b63deb80201bcae6fbe94d490fa33081749e7880a",
        "dd2ae5195e9edc1cb8bc3b06fbf31cf46dbffea5361db259af2d12875c648baa",
        "19d269161ac73c46137d4649bfdad0c8c2751ffce15a02053d50c3d55f307a8c",
    ),
    (
        250,
        "962e5ccbf6e6a9c9ab438c60b1ee32c2b9634d6a64068d24d5ebba10e2c3b485",
        "20fab50584a46229f23d7baa7cd8f60a93e8fe0e70fbe0f9c619f999b0261ce1",
        "759aa0161ea73dfa3d27c7072690a19aeafdd53d32a9f2554e42d54618f6bf51",
    ),
    (
        500,
        "07af1f465fce87d62363437e9c2a8984843aaf1604431bb451ac485606957d51",
        "dfde72c4ac5681c417f99213e1d40235dc22ee886fd879aaa83f94cc9c76c204",
        "3c7484392b4bdf15699c23acf4ef2c8e6185b5601de332db3e4cb634890b2f0b",
    ),
];

const EDGE_DROP_GOLDENS: [(u32, &str, &str, &str); 3] = [
    (
        100,
        "440109eda03e5d6b0454c52cd6f1eefffcbf63245f790206a6c77deedbc7dd6b",
        "f4f22cf598e0455dcaa6687a75e36de78e387a160c65ec440914b95d36013936",
        "52f5b8b4553c78a7e3d70c26e5a98f561d16a8c92583264d89e8cb036f8bdbbe",
    ),
    (
        250,
        "f5dc9003025c03039819424fc649e8be88ab18ba10c1c77a3e31230794399c60",
        "34116875fd7f96dc9dfa32b4eefccb51b7f7f986c9926838625bd80f108c2a32",
        "0fc5ed1e55fe13a29002c0d9f74e4c209dbf533145aabfac4ad7de4f7d68bb8b",
    ),
    (
        500,
        "9572dafd3b74afb902e16a2c2b5d1f7d5a33baf5058c1a0b39ced6c5794823b5",
        "a0748cf4fc90ab8c6e9a2dc8380e66ee0740a7a0c02c82cf5163c1270c4ec79d",
        "10c40b5086948e052b5fffaea1578449826df5e0038c761ff8ba0c7991b35620",
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

#[derive(Debug, Default, PartialEq, Eq)]
struct WindowStats {
    start_day: u32,
    end_day: u32,
    alive_start: u64,
    alive_end: u64,
    alive_min: u64,
    alive_max: u64,
    births: u64,
    removed: u64,
    replaced_edges: u64,
    dropped_edges: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Trace {
    checkpoints: Vec<Checkpoint>,
    counts: PopulationCounts,
    windows: Vec<WindowStats>,
}

fn config() -> SimConfig {
    SimConfig::parse_and_validate(M3P_GOLDEN_CONFIG).expect("M3P golden config validates")
}

fn edge_drop_config() -> SimConfig {
    let mut config = config();
    config.world.settlement_count = 10;
    config.interaction.relationship_degree_k = Some(16);
    config.economy.base_work_yield = 0.7;
    config
        .validate()
        .expect("M3P edge-drop golden config validates");
    config
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

fn run_config(config: &SimConfig) -> Trace {
    let run_context = run_context(config);
    let mut state = fresh_state(config);
    let mut metrics = Vec::<M3pDailyMetrics>::new();
    let mut events = Vec::new();
    let mut checkpoints = Vec::new();
    let mut counts = PopulationCounts::default();
    let mut windows: Vec<_> = (0..10)
        .map(|index| WindowStats {
            start_day: index * 50 + 1,
            end_day: index * 50 + 50,
            alive_min: u64::MAX,
            ..WindowStats::default()
        })
        .collect();

    for _ in 0..500 {
        let next_day = state.world().current_day.as_u32() + 1;
        let snapshot_boundary = GOLDEN_DAYS.contains(&next_day);
        let outcome = run_m3p_reference_day(
            &mut state,
            config,
            &run_context,
            &options(snapshot_boundary),
        )
        .expect("M3P golden day executes");
        let day_metrics = outcome.metrics.expect("metrics enabled");
        let window = &mut windows[outcome.executed_day as usize / 50];
        let alive = day_metrics.m3_metrics.metrics.population;
        if outcome.executed_day.is_multiple_of(50) {
            window.alive_start = alive;
        }
        window.alive_end = alive;
        window.alive_min = window.alive_min.min(alive);
        window.alive_max = window.alive_max.max(alive);
        window.births += day_metrics.births;
        window.removed += day_metrics.removed;
        window.replaced_edges += day_metrics.replaced_edges;
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
                    window.dropped_edges += dropped_edges;
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
        windows,
    }
}

#[test]
fn m3p_golden_replay_has_births_deaths_and_compaction() {
    let main_config = config();
    assert_eq!(main_config.world.initial_population, 300);
    assert_eq!(main_config.world.settlement_count, 3);
    assert_eq!(main_config.interaction.relationship_degree_k, Some(8));
    assert_eq!(main_config.interaction.p_local, Some(0.5));
    let main = run_config(&main_config);
    assert_golden_trace(&main, &GOLDENS, false);
    assert_eq!(main, run_config(&main_config), "main M3P replay must match");

    // Ten small settlements and k=16 leave some surviving rows without enough
    // eligible same-settlement replacements, exercising RQ-7 dropped_edges.
    let edge_drop_config = edge_drop_config();
    assert_eq!(edge_drop_config.world.initial_population, 300);
    assert_eq!(edge_drop_config.world.settlement_count, 10);
    assert_eq!(edge_drop_config.interaction.relationship_degree_k, Some(16));
    assert_eq!(edge_drop_config.interaction.p_local, Some(0.5));
    let edge_drop = run_config(&edge_drop_config);
    assert_golden_trace(&edge_drop, &EDGE_DROP_GOLDENS, true);
    assert_eq!(
        edge_drop,
        run_config(&edge_drop_config),
        "edge-drop M3P replay must match"
    );
}

fn assert_golden_trace(trace: &Trace, goldens: &[(u32, &str, &str, &str)], has_drops: bool) {
    assert_eq!(
        trace.checkpoints.iter().map(|c| c.day).collect::<Vec<_>>(),
        GOLDEN_DAYS
    );
    assert_eq!(goldens.len(), GOLDEN_DAYS.len());
    assert!(trace.counts.compactions >= 5);
    assert!(trace.counts.replaced_edges > 0);
    if has_drops {
        assert!(trace.counts.dropped_edges > 0);
    }
    for window in &trace.windows {
        assert!(
            window.alive_min > 0,
            "extinct within days {}-{}",
            window.start_day,
            window.end_day
        );
        assert!(
            window.births > 0,
            "no births within days {}-{}",
            window.start_day,
            window.end_day
        );
        println!(
            "M3P golden days {}-{}: alive {}..{} (start {}, end {}), births {}, removals {}, replaced {}, dropped {}",
            window.start_day,
            window.end_day,
            window.alive_min,
            window.alive_max,
            window.alive_start,
            window.alive_end,
            window.births,
            window.removed,
            window.replaced_edges,
            window.dropped_edges,
        );
    }
    println!("M3P golden population totals: {:?}", trace.counts);
    for (actual, (day, state, metrics, events)) in trace.checkpoints.iter().zip(goldens) {
        assert_eq!(actual.day, *day);
        assert_eq!(actual.state.to_hex(), *state, "State digest day {day}");
        assert_eq!(
            actual.metrics.to_hex(),
            *metrics,
            "Metrics digest day {day}"
        );
        assert_eq!(actual.events.to_hex(), *events, "Events digest day {day}");
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
