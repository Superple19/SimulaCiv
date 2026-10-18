use sim_model::hashing::{canonical_m3p_state_hash, canonical_m3p_state_hash_from_storage};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pEvent, M3pReferenceState, M3pStorageRunContext,
    SimConfig, initialize_world, run_m3p_reference_day, run_m3p_storage_days,
};
use std::collections::BTreeSet;

const CONFIG: &str = r#"
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
birth_probability = 0.005
birth_food_cost = 0.5
births_max_per_day = 0
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

#[test]
fn unlimited_births_match_storage_and_reference_for_300_days() {
    let config = SimConfig::parse_and_validate(CONFIG).expect("M3P test config validates");
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("world initializes");
    let mut reference = M3pReferenceState::new(world, &config, &run_context)
        .expect("M3P reference state initializes");
    let mut hybrid_world = HybridWorldState::hybrid(reference.world().clone());
    let mut storage_context = M3pStorageRunContext::new(&hybrid_world, &config, &run_context)
        .expect("M3P storage context initializes");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut days_with_multiple_parents = 0;

    for _ in 0..300 {
        let reference_outcome =
            run_m3p_reference_day(&mut reference, &config, &run_context, &options)
                .expect("reference day executes");
        let storage_outcome = run_m3p_storage_days(
            &mut hybrid_world,
            &config,
            &mut storage_context,
            1,
            &options,
        )
        .expect("storage day executes")
        .remove(0);

        assert_eq!(reference_outcome.metrics, storage_outcome.metrics);
        assert_eq!(reference_outcome.events, storage_outcome.events);
        assert_eq!(
            canonical_m3p_state_hash(&reference).expect("reference state hashes"),
            canonical_m3p_state_hash_from_storage(
                hybrid_world.storage().expect("storage exists"),
                hybrid_world.world.current_day,
                &hybrid_world.world.settlements,
                storage_context.relationship_graph(),
                storage_context.next_agent_id(),
            )
            .expect("storage state hashes")
        );

        let birth_parents: BTreeSet<_> = storage_outcome
            .events
            .iter()
            .filter_map(|record| match record.event {
                M3pEvent::Birth { parent, .. } => Some(parent),
                _ => None,
            })
            .collect();
        days_with_multiple_parents += usize::from(birth_parents.len() > 1);
    }

    assert!(
        days_with_multiple_parents > 0,
        "unlimited births should admit distinct parents on some days"
    );
}
