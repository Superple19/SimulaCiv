use sim_core::SimulationDay;
use sim_model::config::SimConfig;
use sim_model::hashing::{canonical_m3p_state_hash, canonical_m3p_state_hash_from_storage};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pIncrementalTopologyStorageRunContext, M3pReferenceState,
    M3pStorageRunContext, SegmentedAgentStorage, initialize_world, run_m3p_reference_day,
    run_m3p_storage_days, run_m3p_storage_days_incremental_topology,
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
birth_probability = 1.0
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

fn assert_storage_matches_reference(
    reference: &M3pReferenceState,
    hybrid: &HybridWorldState,
    context: &M3pStorageRunContext,
) {
    assert_eq!(
        canonical_m3p_state_hash(reference).expect("reference state hash"),
        canonical_m3p_state_hash_from_storage(
            hybrid.storage().expect("hybrid storage"),
            hybrid.world.current_day,
            &hybrid.world.settlements,
            context.relationship_graph(),
            context.next_agent_id(),
        )
        .expect("storage state hash")
    );
}

fn assert_incremental_matches_reference(
    reference: &M3pReferenceState,
    hybrid: &HybridWorldState,
    context: &M3pIncrementalTopologyStorageRunContext,
) {
    assert_eq!(
        canonical_m3p_state_hash(reference).expect("reference state hash"),
        canonical_m3p_state_hash_from_storage(
            hybrid.storage().expect("hybrid storage"),
            hybrid.world.current_day,
            &hybrid.world.settlements,
            context.relationship_graph(),
            context.next_agent_id(),
        )
        .expect("incremental state hash")
    );
}

#[test]
fn non_compaction_birth_error_keeps_incremental_path_equal_to_both_oracles() {
    let config = SimConfig::parse_and_validate(CONFIG).expect("config validates");
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut world = initialize_world(&config).expect("world initializes");
    world.current_day = SimulationDay(1);
    let generated =
        M3pReferenceState::new(world, &config, &run_context).expect("reference graph initializes");
    let state = M3pReferenceState::from_parts(
        generated.world().clone(),
        generated.relationship_graph().clone(),
        u32::MAX as u64 + 1,
        &config,
        &run_context,
    )
    .expect("exhausted allocator cursor is representable");

    let mut reference = state.clone();
    let storage_reference = state.clone();
    let mut storage_world = HybridWorldState::hybrid(state.world().clone());
    storage_world.segmented_storage =
        Some(SegmentedAgentStorage::from_agents(&state.world().agents));
    let mut incremental_world = storage_world.clone();
    let mut storage_context = M3pStorageRunContext::from_reference_state(
        &storage_world,
        &config,
        &run_context,
        &storage_reference,
    )
    .expect("storage context initializes");
    let mut incremental_context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
        &incremental_world,
        &config,
        &run_context,
        &state,
    )
    .expect("incremental context initializes");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    let reference_error = run_m3p_reference_day(&mut reference, &config, &run_context, &options)
        .expect_err("reference rejects the unbounded birth batch");
    let storage_error = run_m3p_storage_days(
        &mut storage_world,
        &config,
        &mut storage_context,
        1,
        &options,
    )
    .expect_err("serial storage rejects the unbounded birth batch");
    let incremental_error = run_m3p_storage_days_incremental_topology(
        &mut incremental_world,
        &config,
        &mut incremental_context,
        1,
        &options,
    )
    .expect_err("incremental storage rejects the unbounded birth batch");

    assert_eq!(storage_error, reference_error);
    assert_eq!(incremental_error, reference_error);
    assert_storage_matches_reference(&reference, &storage_world, &storage_context);
    assert_incremental_matches_reference(&reference, &incremental_world, &incremental_context);
}
