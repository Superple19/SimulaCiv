use sim_core::AgentId;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, run_m3_reference_day,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::WorldState;
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Intent, M3Phase4SlotProjection, PrimaryActionChoice, RelationshipGraph, SimConfig,
    count_cross_partition_interactions_per_day, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_into, generate_intents_m3_storage_into, initialize_world,
    phase1_resource_regrowth, phase2_biological_degradation, phase3_observation_and_features,
    phase4_primary_action_selection_into, phase4_primary_action_selection_storage_into,
};

const CONFIG_TOML: &str = r#"
profile = "m3"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 300
settlement_count = 3
initial_health = 1.0
initial_food = 4.0
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
base_metabolic_cost = 0.5
health_decay_rate = 0.05

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
action_biases = [0.0, 0.0, 0.0, 1.5, 1.5, 0.0]
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

const SEEDS: [u64; 3] = [0, 1, 0x0123_4567_89ab_cdef];

fn config_for(
    population: u64,
    settlements: u32,
    degree: i64,
    p_local: f32,
    seed: u64,
) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(CONFIG_TOML).unwrap();
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.master_seed = seed;
    config.interaction.relationship_degree_k = Some(degree);
    config.interaction.p_local = Some(p_local);
    config.validate().unwrap();
    config
}

fn graph_for(world: &WorldState, config: &SimConfig) -> RelationshipGraph {
    let memberships: Vec<_> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    RelationshipGraph::generate(
        &memberships,
        config.world.master_seed,
        config.world.replicate_id,
        config.interaction.relationship_degree_k.unwrap() as u32,
        config.interaction.p_local.unwrap(),
    )
    .unwrap()
}

fn phase4_state_and_choices(
    world: &WorldState,
    config: &SimConfig,
    storage: &mut SegmentedAgentStorage,
) -> (WorldState, Vec<PrimaryActionChoice>) {
    let mut phase4_world = world.clone();
    phase1_resource_regrowth(&mut phase4_world, config);
    phase2_biological_degradation(&mut phase4_world, config);
    storage.sync_dynamic_from_agents(&phase4_world.agents);

    let features = phase3_observation_and_features(&phase4_world, config).unwrap();
    let mut reference_choices = Vec::new();
    phase4_primary_action_selection_into(&phase4_world, config, &features, &mut reference_choices)
        .unwrap();
    let mut storage_choices = Vec::new();
    phase4_primary_action_selection_storage_into(
        storage,
        phase4_world.current_day,
        config,
        &features,
        &mut storage_choices,
    )
    .unwrap();
    assert_eq!(storage_choices, reference_choices);
    (phase4_world, storage_choices)
}

struct Phase4Comparison<'a> {
    config: &'a SimConfig,
    graph: &'a RelationshipGraph,
    storage: &'a SegmentedAgentStorage,
    projection: &'a M3Phase4SlotProjection,
}

fn assert_intent_parity(
    phase4_world: &WorldState,
    choices: &[PrimaryActionChoice],
    candidate_scratch: &mut Vec<AgentId>,
    reference_intents: &mut Vec<Intent>,
    storage_intents: &mut Vec<Intent>,
    comparison: &Phase4Comparison<'_>,
) {
    generate_intents_m3_into(
        phase4_world,
        comparison.config,
        choices,
        comparison.graph,
        reference_intents,
    )
    .unwrap();

    generate_intents_m3_storage_into(
        comparison.storage,
        phase4_world.current_day,
        comparison.config,
        choices,
        comparison.projection,
        candidate_scratch,
        storage_intents,
    )
    .unwrap();
    assert_eq!(*storage_intents, *reference_intents);
    assert_eq!(
        count_cross_partition_interactions_storage_per_day(storage_intents, comparison.storage)
            .unwrap(),
        count_cross_partition_interactions_per_day(reference_intents, phase4_world).unwrap()
    );
}

#[test]
fn m3_storage_intents_match_reference_across_seed_layout_and_day_matrix() {
    let mut saw_give_food = false;
    let mut saw_steal_food = false;

    for &seed in &SEEDS {
        for population in [300, 1_000] {
            for settlements in [3, 10] {
                for degree in [4, 8, 16] {
                    for p_local in [0.0, 0.5, 1.0] {
                        let config = config_for(population, settlements, degree, p_local, seed);
                        let mut world = initialize_world(&config).unwrap();
                        let run_context =
                            M0RunContext::new(config.world.master_seed, config.world.replicate_id);
                        let m3_context =
                            M3ReferenceContext::new(&world, &config, &run_context).unwrap();
                        let graph = m3_context.relationship_graph();
                        let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
                        let projection = M3Phase4SlotProjection::new(graph, &storage).unwrap();
                        let mut candidate_scratch = Vec::with_capacity(degree as usize);
                        let candidate_capacity = candidate_scratch.capacity();
                        let mut reference_intents = Vec::with_capacity(population as usize);
                        let mut storage_intents = Vec::with_capacity(population as usize);

                        for _ in 0..3 {
                            let (phase4_world, choices) =
                                phase4_state_and_choices(&world, &config, &mut storage);
                            let comparison = Phase4Comparison {
                                config: &config,
                                graph,
                                storage: &storage,
                                projection: &projection,
                            };
                            assert_intent_parity(
                                &phase4_world,
                                &choices,
                                &mut candidate_scratch,
                                &mut reference_intents,
                                &mut storage_intents,
                                &comparison,
                            );
                            assert_eq!(candidate_scratch.capacity(), candidate_capacity);
                            saw_give_food |= storage_intents.iter().any(|intent| {
                                matches!(
                                    intent,
                                    Intent::GiveFood {
                                        target_agent_id: Some(_),
                                        ..
                                    }
                                )
                            });
                            saw_steal_food |= storage_intents.iter().any(|intent| {
                                matches!(
                                    intent,
                                    Intent::StealFood {
                                        target_agent_id: Some(_),
                                        ..
                                    }
                                )
                            });

                            run_m3_reference_day(
                                &mut world,
                                &config,
                                &m3_context,
                                &DayExecutionOptions {
                                    metrics_enabled: false,
                                    events_enabled: false,
                                    snapshot_boundary: false,
                                },
                            )
                            .unwrap();
                        }
                    }
                }
            }
        }
    }

    assert!(saw_give_food);
    assert!(saw_steal_food);
}

#[test]
fn shuffled_storage_slots_preserve_m3_intents() {
    let config = config_for(300, 3, 8, 0.5, SEEDS[0]);
    let world = initialize_world(&config).unwrap();
    let mut phase4_world = world.clone();
    phase1_resource_regrowth(&mut phase4_world, &config);
    phase2_biological_degradation(&mut phase4_world, &config);
    let graph = graph_for(&world, &config);
    let features = phase3_observation_and_features(&phase4_world, &config).unwrap();
    let mut choices = Vec::new();
    phase4_primary_action_selection_into(&phase4_world, &config, &features, &mut choices).unwrap();

    let ordered_storage = SegmentedAgentStorage::from_agents(&phase4_world.agents);
    let ordered_projection = M3Phase4SlotProjection::new(&graph, &ordered_storage).unwrap();
    let mut ordered_scratch = Vec::new();
    let mut ordered_reference = Vec::new();
    let mut ordered_intents = Vec::new();
    let ordered_comparison = Phase4Comparison {
        config: &config,
        graph: &graph,
        storage: &ordered_storage,
        projection: &ordered_projection,
    };
    assert_intent_parity(
        &phase4_world,
        &choices,
        &mut ordered_scratch,
        &mut ordered_reference,
        &mut ordered_intents,
        &ordered_comparison,
    );

    let mut reversed_agents = phase4_world.agents.clone();
    reversed_agents.reverse();
    let shuffled_storage = SegmentedAgentStorage::from_agents(&reversed_agents);
    let shuffled_projection = M3Phase4SlotProjection::new(&graph, &shuffled_storage).unwrap();
    let mut shuffled_choices = Vec::new();
    phase4_primary_action_selection_storage_into(
        &shuffled_storage,
        phase4_world.current_day,
        &config,
        &features,
        &mut shuffled_choices,
    )
    .unwrap();
    assert_eq!(shuffled_choices, choices);
    let mut shuffled_scratch = Vec::new();
    let mut shuffled_reference = Vec::new();
    let mut shuffled_intents = Vec::new();
    let shuffled_comparison = Phase4Comparison {
        config: &config,
        graph: &graph,
        storage: &shuffled_storage,
        projection: &shuffled_projection,
    };
    assert_intent_parity(
        &phase4_world,
        &shuffled_choices,
        &mut shuffled_scratch,
        &mut shuffled_reference,
        &mut shuffled_intents,
        &shuffled_comparison,
    );
    assert_eq!(shuffled_intents, ordered_intents);
}

#[test]
fn projection_rebuilt_after_snapshot_restore_matches_reference() {
    let config = config_for(300, 3, 8, 0.5, SEEDS[2]);
    let mut world = initialize_world(&config).unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let reference_context = M3ReferenceContext::new(&world, &config, &run_context).unwrap();
    let original_graph_hash = *reference_context.relationship_graph().csr_hash();
    let outcome = run_m3_reference_day(
        &mut world,
        &config,
        &reference_context,
        &DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: true,
        },
    )
    .unwrap();
    let snapshot = outcome.snapshot.unwrap();
    let restored = restore_snapshot(&snapshot.bytes).unwrap();
    assert_eq!(restored.metadata.day, 1);

    let restored_context = M3ReferenceContext::new(&restored.world, &config, &run_context).unwrap();
    assert_eq!(
        restored_context.relationship_graph().csr_hash(),
        &original_graph_hash
    );
    let (phase4_world, choices) = {
        let mut storage = SegmentedAgentStorage::from_agents(&restored.world.agents);
        let (phase4_world, choices) =
            phase4_state_and_choices(&restored.world, &config, &mut storage);
        let projection =
            M3Phase4SlotProjection::new(restored_context.relationship_graph(), &storage).unwrap();
        let mut candidate_scratch = Vec::new();
        let mut reference_intents = Vec::new();
        let mut storage_intents = Vec::new();
        let comparison = Phase4Comparison {
            config: &config,
            graph: restored_context.relationship_graph(),
            storage: &storage,
            projection: &projection,
        };
        assert_intent_parity(
            &phase4_world,
            &choices,
            &mut candidate_scratch,
            &mut reference_intents,
            &mut storage_intents,
            &comparison,
        );
        (phase4_world, choices)
    };
    assert_eq!(phase4_world.current_day.as_u32(), 1);
    assert!(!choices.is_empty());
}
