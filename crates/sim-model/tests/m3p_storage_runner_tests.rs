use sim_model::hashing::{
    canonical_m3p_state_bytes, canonical_m3p_state_bytes_from_storage, canonical_m3p_state_hash,
    canonical_m3p_state_hash_from_storage,
};
use sim_model::state::WorldState;
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pReferenceState, SimConfig, initialize_world,
    run_m3p_reference_day,
};

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

fn config() -> SimConfig {
    SimConfig::parse_and_validate(CONFIG).expect("M3P storage config validates")
}

fn context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn state(config: &SimConfig) -> M3pReferenceState {
    let world = initialize_world(config).expect("M3P world initializes");
    M3pReferenceState::new(world, config, &context(config)).expect("M3P reference initializes")
}

fn snapshot_world(world: &WorldState, storage: &SegmentedAgentStorage) -> WorldState {
    let mut snapshot_world = world.clone();
    snapshot_world.agents = storage.to_agents();
    snapshot_world
}

#[test]
fn m3p_storage_state_digest_matches_reference_for_shuffled_slots_and_mutated_graph() {
    let config = config();
    let run_context = context(&config);
    let mut reference = state(&config);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    for _ in 0..60 {
        run_m3p_reference_day(&mut reference, &config, &run_context, &options)
            .expect("reference day executes");
    }

    let world = reference.world();
    let mut shuffled_agents = world.agents.clone();
    shuffled_agents.reverse();
    let storage = SegmentedAgentStorage::from_agents(&shuffled_agents);
    let storage_world = snapshot_world(world, &storage);
    let reference_bytes = canonical_m3p_state_bytes(&reference).unwrap();
    let storage_bytes = canonical_m3p_state_bytes_from_storage(
        &storage,
        storage_world.current_day,
        &storage_world.settlements,
        reference.relationship_graph(),
        reference.next_agent_id(),
    )
    .unwrap();
    assert_eq!(reference_bytes, storage_bytes);
    assert_eq!(
        canonical_m3p_state_hash(&reference).unwrap(),
        canonical_m3p_state_hash_from_storage(
            &storage,
            storage_world.current_day,
            &storage_world.settlements,
            reference.relationship_graph(),
            reference.next_agent_id(),
        )
        .unwrap()
    );
}
