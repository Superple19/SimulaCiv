use sim_model::hashing::{
    canonical_m3p_event_hash, canonical_m3p_metrics_hash, canonical_m3p_state_hash,
    canonical_m3p_state_hash_from_storage,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pIncrementalTopologyStorageRunContext, M3pReferenceState,
    M3pStorageRunContext, SimConfig, initialize_world, run_m3p_reference_day, run_m3p_storage_days,
    run_m3p_storage_days_incremental_topology,
};

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

#[test]
fn incremental_topology_csr_append_matches_m3p_oracles_and_goldens() {
    let config = SimConfig::parse_and_validate(GOLDEN_CONFIG).expect("golden config validates");
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("world initializes");
    let mut reference =
        M3pReferenceState::new(world, &config, &run_context).expect("reference state initializes");
    let mut storage_world = HybridWorldState::hybrid(reference.world().clone());
    let mut incremental_world = HybridWorldState::hybrid(reference.world().clone());
    let mut storage_context = M3pStorageRunContext::new(&storage_world, &config, &run_context)
        .expect("storage context initializes");
    let mut incremental_context =
        M3pIncrementalTopologyStorageRunContext::new(&incremental_world, &config, &run_context)
            .expect("incremental context initializes");
    let mut metrics = Vec::new();
    let mut events = Vec::new();

    for day in 1..=500 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: [100, 250, 500].contains(&day),
        };
        let reference_outcome =
            run_m3p_reference_day(&mut reference, &config, &run_context, &options)
                .expect("reference day executes");
        let storage_outcome = run_m3p_storage_days(
            &mut storage_world,
            &config,
            &mut storage_context,
            1,
            &options,
        )
        .expect("storage day executes")
        .remove(0);
        let incremental_outcome = run_m3p_storage_days_incremental_topology(
            &mut incremental_world,
            &config,
            &mut incremental_context,
            1,
            &options,
        )
        .expect("incremental day executes")
        .remove(0);

        assert_eq!(storage_outcome, reference_outcome, "storage day {day}");
        assert_eq!(
            incremental_outcome, reference_outcome,
            "incremental day {day}"
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
        assert_eq!(
            canonical_m3p_state_hash(&reference).expect("reference state digest"),
            canonical_m3p_state_hash_from_storage(
                storage_world.storage().expect("storage exists"),
                storage_world.world.current_day,
                &storage_world.world.settlements,
                storage_context.relationship_graph(),
                storage_context.next_agent_id(),
            )
            .expect("storage state digest")
        );
        assert_eq!(
            canonical_m3p_state_hash_from_storage(
                storage_world.storage().expect("storage exists"),
                storage_world.world.current_day,
                &storage_world.world.settlements,
                storage_context.relationship_graph(),
                storage_context.next_agent_id(),
            )
            .expect("storage state digest"),
            canonical_m3p_state_hash_from_storage(
                incremental_world
                    .storage()
                    .expect("incremental storage exists"),
                incremental_world.world.current_day,
                &incremental_world.world.settlements,
                incremental_context.relationship_graph(),
                incremental_context.next_agent_id(),
            )
            .expect("incremental state digest")
        );
        metrics.push(reference_outcome.metrics.expect("metrics enabled"));
        events.extend(reference_outcome.events);

        if let Some((_, state, metrics_digest, events_digest)) =
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
                canonical_m3p_metrics_hash(&metrics)
                    .expect("metrics digest")
                    .to_hex(),
                *metrics_digest,
                "Metrics golden day {day}"
            );
            assert_eq!(
                canonical_m3p_event_hash(&events)
                    .expect("events digest")
                    .to_hex(),
                *events_digest,
                "Events golden day {day}"
            );
        }
    }
}
