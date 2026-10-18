use sim_model::config::PopulationConfig;
use sim_model::hashing::{
    canonical_m3p_event_hash, canonical_m3p_metrics_hash, canonical_m3p_state_hash,
    canonical_m3p_state_hash_from_storage,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pDailyMetrics, M3pEvent, M3pEventRecord,
    M3pIncrementalTopologyStorageRunContext, M3pReferenceState, M3pStorageRunContext,
    SegmentedAgentStorage, SimConfig, decode_m3p_snapshot, initialize_world, run_m3p_reference_day,
    run_m3p_storage_days, run_m3p_storage_days_incremental_topology,
};
use std::collections::BTreeSet;

const GOLDEN_CONFIG: &str = r#"
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

fn golden_config() -> SimConfig {
    SimConfig::parse_and_validate(GOLDEN_CONFIG).expect("golden config validates")
}

fn run_context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn initial_state(config: &SimConfig) -> M3pReferenceState {
    let world = initialize_world(config).expect("world initializes");
    M3pReferenceState::new(world, config, &run_context(config))
        .expect("reference state initializes")
}

fn hybrid_from_state(state: &M3pReferenceState, reverse_slots: bool) -> HybridWorldState {
    let mut hybrid = HybridWorldState::hybrid(state.world().clone());
    if reverse_slots {
        let mut agents = state.world().agents.clone();
        agents.reverse();
        hybrid.segmented_storage = Some(SegmentedAgentStorage::from_agents(&agents));
    }
    hybrid
}

fn assert_storage_state_hash(
    reference: &M3pReferenceState,
    hybrid: &HybridWorldState,
    graph: &sim_model::RelationshipGraph,
    next_agent_id: u64,
    label: &str,
) {
    assert_eq!(
        canonical_m3p_state_hash(reference).expect("reference state digest"),
        canonical_m3p_state_hash_from_storage(
            hybrid.storage().expect("storage exists"),
            hybrid.world.current_day,
            &hybrid.world.settlements,
            graph,
            next_agent_id,
        )
        .expect("storage state digest"),
        "{label}"
    );
}

#[derive(Debug)]
struct CompareSummary {
    reference_metrics: Vec<M3pDailyMetrics>,
    days_with_multiple_birth_parents: usize,
}

fn compare_all_paths(
    config: &SimConfig,
    days: u32,
    reverse_initial_slots: bool,
    snapshot_days: &[u32],
    check_golden: bool,
) -> CompareSummary {
    let run_context = run_context(config);
    let mut reference = initial_state(config);
    let mut storage_world = hybrid_from_state(&reference, reverse_initial_slots);
    let mut incremental_world = hybrid_from_state(&reference, reverse_initial_slots);
    let mut storage_context = M3pStorageRunContext::new(&storage_world, config, &run_context)
        .expect("storage context initializes");
    let mut incremental_context =
        M3pIncrementalTopologyStorageRunContext::new(&incremental_world, config, &run_context)
            .expect("incremental context initializes");
    let mut metrics: [Vec<M3pDailyMetrics>; 3] = std::array::from_fn(|_| Vec::new());
    let mut events: [Vec<M3pEventRecord>; 3] = std::array::from_fn(|_| Vec::new());
    let mut days_with_multiple_birth_parents = 0;

    for day in 1..=days {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: snapshot_days.contains(&day),
        };
        let outcomes = [
            run_m3p_reference_day(&mut reference, config, &run_context, &options)
                .expect("reference day executes"),
            run_m3p_storage_days(
                &mut storage_world,
                config,
                &mut storage_context,
                1,
                &options,
            )
            .expect("storage day executes")
            .remove(0),
            run_m3p_storage_days_incremental_topology(
                &mut incremental_world,
                config,
                &mut incremental_context,
                1,
                &options,
            )
            .expect("incremental day executes")
            .remove(0),
        ];
        assert_eq!(
            outcomes[1], outcomes[0],
            "storage daily record on day {day}"
        );
        assert_eq!(
            outcomes[2], outcomes[0],
            "incremental daily record on day {day}"
        );
        for (index, outcome) in outcomes.iter().enumerate() {
            metrics[index].push(outcome.metrics.as_ref().expect("metrics enabled").clone());
            events[index].extend(outcome.events.iter().cloned());
        }
        assert_storage_state_hash(
            &reference,
            &storage_world,
            storage_context.relationship_graph(),
            storage_context.next_agent_id(),
            &format!("storage State day {day}"),
        );
        assert_storage_state_hash(
            &reference,
            &incremental_world,
            incremental_context.relationship_graph(),
            incremental_context.next_agent_id(),
            &format!("incremental State day {day}"),
        );
        assert_eq!(
            storage_context.relationship_graph().source_ids(),
            incremental_context.relationship_graph().source_ids(),
            "CSR sources on day {day}"
        );
        assert_eq!(
            storage_context.relationship_graph().row_offsets(),
            incremental_context.relationship_graph().row_offsets(),
            "CSR offsets on day {day}"
        );
        assert_eq!(
            storage_context.relationship_graph().neighbors(),
            incremental_context.relationship_graph().neighbors(),
            "CSR neighbors on day {day}"
        );

        let parents: BTreeSet<_> = outcomes[0]
            .events
            .iter()
            .filter_map(|record| match &record.event {
                M3pEvent::Birth { parent, .. } => Some(*parent),
                _ => None,
            })
            .collect();
        days_with_multiple_birth_parents += usize::from(parents.len() > 1);

        if day % 50 == 0 || day == days {
            let metric_hashes: Vec<_> = metrics
                .iter()
                .map(|records| canonical_m3p_metrics_hash(records).expect("metrics hash"))
                .collect();
            let event_hashes: Vec<_> = events
                .iter()
                .map(|records| canonical_m3p_event_hash(records).expect("events hash"))
                .collect();
            assert_eq!(metric_hashes[1], metric_hashes[0], "Metrics day {day}");
            assert_eq!(metric_hashes[2], metric_hashes[0], "Metrics day {day}");
            assert_eq!(event_hashes[1], event_hashes[0], "Events day {day}");
            assert_eq!(event_hashes[2], event_hashes[0], "Events day {day}");
        }

        if check_golden
            && let Some((_, state, metrics_digest, events_digest)) =
                GOLDENS.iter().find(|(golden_day, ..)| *golden_day == day)
        {
            assert_eq!(
                canonical_m3p_state_hash(&reference)
                    .expect("state digest")
                    .to_hex(),
                *state,
                "State golden day {day}"
            );
            assert_eq!(
                canonical_m3p_metrics_hash(&metrics[0])
                    .expect("metrics digest")
                    .to_hex(),
                *metrics_digest,
                "Metrics golden day {day}"
            );
            assert_eq!(
                canonical_m3p_event_hash(&events[0])
                    .expect("events digest")
                    .to_hex(),
                *events_digest,
                "Events golden day {day}"
            );
        }
    }

    CompareSummary {
        reference_metrics: metrics[0].clone(),
        days_with_multiple_birth_parents,
    }
}

fn matrix_config(seed: u64, settlements: u32, degree: i64, p_local: f32) -> SimConfig {
    let mut config = golden_config();
    config.world.master_seed = seed;
    config.world.settlement_count = settlements;
    config.interaction.relationship_degree_k = Some(degree);
    config.interaction.p_local = Some(p_local);
    config.validate().expect("M3P matrix config validates");
    config
}

fn sustained_config() -> SimConfig {
    let mut config = golden_config();
    config.world.initial_population = 1_000;
    config.world.settlement_count = 5;
    config.world.initial_food = 25.0;
    config.world.initial_wealth = 10_000;
    config.world.initial_settlement_resource = 1_000_000.0;
    config.world.initial_treasury = 5_000;
    config.environment.carrying_capacity = 10_000_000.0;
    config.environment.health_decay_rate = 0.2;
    config.economy.base_work_yield = 1.9;
    config.population = Some(PopulationConfig {
        birth_min_health: 0.5,
        birth_food_threshold: 0.5,
        birth_probability: 0.0001,
        birth_food_cost: 0.5,
        births_max_per_day: 0,
        compaction_interval_days: 50,
    });
    config.decision.action_biases = [2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    config.decision.base_weight_matrix = [
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0; 5],
        [0.0; 5],
    ];
    config.decision.trait_weight_cooperation = 1.0;
    config.decision.trait_weight_aggression = 1.0;
    config.decision.trait_weight_risk_tolerance = 1.0;
    config.validate().expect("sustained M3P config validates");
    config
}

#[test]
fn incremental_topology_matches_m3p_goldens_and_oracles() {
    compare_all_paths(&golden_config(), 500, false, &[100, 250, 500], true);
}

#[test]
fn incremental_topology_matches_all_36_m3p_matrix_cells() {
    for seed in [0, 81985529216486895] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for p_local in [0.0, 0.5, 1.0] {
                    compare_all_paths(
                        &matrix_config(seed, settlements, degree, p_local),
                        300,
                        false,
                        &[],
                        false,
                    );
                }
            }
        }
    }
}

#[test]
fn unlimited_births_match_both_m3p_oracles_and_admit_multiple_parents() {
    let mut config = golden_config();
    let population = config.population.as_mut().expect("population config");
    population.birth_probability = 0.005;
    population.births_max_per_day = 0;
    config.validate().expect("unlimited-birth config validates");
    let summary = compare_all_paths(&config, 300, false, &[], false);
    assert!(summary.days_with_multiple_birth_parents > 0);
}

#[test]
fn shuffled_initial_slots_match_both_m3p_oracles() {
    compare_all_paths(&golden_config(), 300, true, &[], false);
}

#[test]
fn incremental_topology_snapshot_resume_matches_across_compaction() {
    let config = golden_config();
    let run_context = run_context(&config);
    let mut uninterrupted = initial_state(&config);
    let mut paused = initial_state(&config);
    let mut storage_world = hybrid_from_state(&paused, false);
    let mut incremental_world = hybrid_from_state(&paused, false);
    let mut storage_context = M3pStorageRunContext::new(&storage_world, &config, &run_context)
        .expect("storage context initializes");
    let mut incremental_context =
        M3pIncrementalTopologyStorageRunContext::new(&incremental_world, &config, &run_context)
            .expect("incremental context initializes");
    let mut compaction_snapshot = None;

    for day in 0..=50 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: day == 50,
        };
        let expected = run_m3p_reference_day(&mut paused, &config, &run_context, &options)
            .expect("paused reference day executes");
        let base = run_m3p_storage_days(
            &mut storage_world,
            &config,
            &mut storage_context,
            1,
            &options,
        )
        .expect("storage day executes")
        .remove(0);
        let optimized = run_m3p_storage_days_incremental_topology(
            &mut incremental_world,
            &config,
            &mut incremental_context,
            1,
            &options,
        )
        .expect("incremental day executes")
        .remove(0);
        let uninterrupted_outcome =
            run_m3p_reference_day(&mut uninterrupted, &config, &run_context, &options)
                .expect("uninterrupted reference day executes");
        assert_eq!(expected, base, "base day {day}");
        assert_eq!(expected, optimized, "incremental day {day}");
        assert_eq!(expected, uninterrupted_outcome, "uninterrupted day {day}");
        if day == 50 {
            compaction_snapshot = expected
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.bytes.clone());
        }
    }

    let restored = decode_m3p_snapshot(
        compaction_snapshot.as_deref().expect("compaction snapshot"),
        &config,
    )
    .expect("snapshot restores");
    let mut resumed = restored.state;
    assert_eq!(
        canonical_m3p_state_hash(&resumed).expect("resumed State digest"),
        canonical_m3p_state_hash(&uninterrupted).expect("uninterrupted State digest")
    );
    let mut resumed_storage_world = hybrid_from_state(&resumed, false);
    let mut resumed_incremental_world = hybrid_from_state(&resumed, false);
    let mut resumed_storage_context = M3pStorageRunContext::from_reference_state(
        &resumed_storage_world,
        &config,
        &run_context,
        &resumed,
    )
    .expect("resumed storage context initializes");
    let mut resumed_incremental_context =
        M3pIncrementalTopologyStorageRunContext::from_reference_state(
            &resumed_incremental_world,
            &config,
            &run_context,
            &resumed,
        )
        .expect("resumed incremental context initializes");

    for day in 51..=100 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: day == 100,
        };
        let expected = run_m3p_reference_day(&mut uninterrupted, &config, &run_context, &options)
            .expect("uninterrupted reference day executes");
        let resumed_outcome = run_m3p_reference_day(&mut resumed, &config, &run_context, &options)
            .expect("resumed reference day executes");
        let base = run_m3p_storage_days(
            &mut resumed_storage_world,
            &config,
            &mut resumed_storage_context,
            1,
            &options,
        )
        .expect("resumed storage day executes")
        .remove(0);
        let optimized = run_m3p_storage_days_incremental_topology(
            &mut resumed_incremental_world,
            &config,
            &mut resumed_incremental_context,
            1,
            &options,
        )
        .expect("resumed incremental day executes")
        .remove(0);
        assert_eq!(resumed_outcome, expected, "resumed reference day {day}");
        assert_eq!(base, expected, "resumed storage day {day}");
        assert_eq!(optimized, expected, "resumed incremental day {day}");
    }
}

#[test]
fn incremental_topology_matches_the_1000_day_sustained_gate() {
    let summary = compare_all_paths(&sustained_config(), 1_000, false, &[], false);
    for (index, metrics) in summary.reference_metrics.iter().enumerate() {
        let day = index as u32 + 1;
        if (100..=1_000).contains(&day) {
            assert!(
                (900..=1_100).contains(&metrics.live_population),
                "live population {} outside [900, 1100] on day {day}",
                metrics.live_population
            );
        }
    }
}

#[test]
fn incremental_topology_preserves_id_exhaustion_error_and_state() {
    let mut config = golden_config();
    config.population.as_mut().unwrap().birth_probability = 1.0;
    config.validate().expect("exhaustion config validates");
    let run_context = run_context(&config);
    let source = initial_state(&config);
    let exhausted = M3pReferenceState::from_parts(
        source.world().clone(),
        source.relationship_graph().clone(),
        u32::MAX as u64 + 1,
        &config,
        &run_context,
    )
    .expect("exhausted cursor is representable");
    let mut reference = exhausted.clone();
    let storage_reference = exhausted.clone();
    let mut hybrid = hybrid_from_state(&exhausted, false);
    let mut incremental_hybrid = hybrid_from_state(&exhausted, false);
    let mut storage_context = M3pStorageRunContext::from_reference_state(
        &hybrid,
        &config,
        &run_context,
        &storage_reference,
    )
    .expect("storage context accepts exhausted cursor");
    let mut incremental_context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
        &incremental_hybrid,
        &config,
        &run_context,
        &exhausted,
    )
    .expect("incremental context accepts exhausted cursor");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    let reference_error = run_m3p_reference_day(&mut reference, &config, &run_context, &options)
        .expect_err("reference rejects the birth batch");
    let storage_error =
        run_m3p_storage_days(&mut hybrid, &config, &mut storage_context, 1, &options)
            .expect_err("storage oracle rejects the birth batch");
    let incremental_error = run_m3p_storage_days_incremental_topology(
        &mut incremental_hybrid,
        &config,
        &mut incremental_context,
        1,
        &options,
    )
    .expect_err("incremental path rejects the birth batch");

    assert_eq!(storage_error, reference_error);
    assert_eq!(incremental_error, reference_error);
    assert_storage_state_hash(
        &reference,
        &hybrid,
        storage_context.relationship_graph(),
        storage_context.next_agent_id(),
        "failed base storage Phase 9b preserves the reference state",
    );
    assert_storage_state_hash(
        &reference,
        &incremental_hybrid,
        incremental_context.relationship_graph(),
        incremental_context.next_agent_id(),
        "failed incremental Phase 9b preserves the reference state",
    );
}
