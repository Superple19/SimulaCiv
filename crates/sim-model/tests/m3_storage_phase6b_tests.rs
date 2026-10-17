use sim_core::{AgentId, GroupId};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, run_m3_reference_day,
};
use sim_model::state::WorldState;
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Intent, M3Phase4SlotProjection, M3Phase6BStorageScratch, PrimaryActionChoice,
    RelationshipGraph, SettlementIntentPartition, SimConfig, TargetedOutcome,
    count_cross_partition_interactions_storage_per_day, generate_intents_m3_into,
    generate_intents_m3_storage_into, initialize_world, phase1_resource_regrowth,
    phase2_biological_degradation, phase3_observation_and_features,
    phase4_primary_action_selection_into, phase4_primary_action_selection_storage_into,
    phase5_partition_intents_baseline, phase6b_targeted_resolution_m3,
    phase6b_targeted_resolution_storage_m3_with_scratch,
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

fn phase4_inputs(
    world: &WorldState,
    config: &SimConfig,
    storage: &mut SegmentedAgentStorage,
    graph: &RelationshipGraph,
    projection: &M3Phase4SlotProjection,
    intent_scratch: &mut Vec<AgentId>,
) -> (WorldState, Vec<SettlementIntentPartition>) {
    let mut phase4_world = world.clone();
    phase1_resource_regrowth(&mut phase4_world, config);
    phase2_biological_degradation(&mut phase4_world, config);
    storage.sync_dynamic_from_agents(&phase4_world.agents);
    let features = phase3_observation_and_features(&phase4_world, config).unwrap();

    let mut reference_choices = Vec::<PrimaryActionChoice>::new();
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

    let mut reference_intents = Vec::new();
    generate_intents_m3_into(
        &phase4_world,
        config,
        &reference_choices,
        graph,
        &mut reference_intents,
    )
    .unwrap();
    let mut storage_intents = Vec::new();
    generate_intents_m3_storage_into(
        storage,
        phase4_world.current_day,
        config,
        &storage_choices,
        projection,
        intent_scratch,
        &mut storage_intents,
    )
    .unwrap();
    assert_eq!(storage_intents, reference_intents);
    assert_eq!(
        count_cross_partition_interactions_storage_per_day(&storage_intents, storage).unwrap(),
        sim_model::count_cross_partition_interactions_per_day(&reference_intents, &phase4_world)
            .unwrap()
    );

    let partitions = phase5_partition_intents_baseline(&reference_intents).unwrap();
    (phase4_world, partitions)
}

fn assert_food_parity(world: &WorldState, storage: &SegmentedAgentStorage) {
    for agent in &world.agents {
        assert_eq!(
            storage.get_agent(agent.agent_id).unwrap().food,
            agent.food,
            "food mismatch for AgentId {}",
            agent.agent_id
        );
    }
}

fn compare_reference_and_storage_error(
    world: &WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
) {
    let mut reference_world = world.clone();
    let reference_error =
        phase6b_targeted_resolution_m3(&mut reference_world, config, partitions).unwrap_err();
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let storage_error = phase6b_targeted_resolution_storage_m3_with_scratch(
        &mut storage,
        &world.settlements,
        world.current_day,
        config,
        partitions,
        &mut M3Phase6BStorageScratch::default(),
    )
    .unwrap_err();
    assert_eq!(format!("{storage_error:?}"), format!("{reference_error:?}"));
    assert_eq!(
        storage.food(),
        world
            .agents
            .iter()
            .map(|agent| agent.food)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        reference_world
            .agents
            .iter()
            .map(|agent| agent.food)
            .collect::<Vec<_>>(),
        world
            .agents
            .iter()
            .map(|agent| agent.food)
            .collect::<Vec<_>>()
    );
}

#[test]
fn m3_storage_phase6b_matches_reference_across_seed_layout_and_day_matrix() {
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
                        let mut intent_scratch = Vec::with_capacity(degree as usize);
                        let mut reused_scratch =
                            M3Phase6BStorageScratch::with_capacity(population as usize);

                        for _ in 0..3 {
                            let (phase4_world, partitions) = phase4_inputs(
                                &world,
                                &config,
                                &mut storage,
                                graph,
                                &projection,
                                &mut intent_scratch,
                            );
                            let storage_before = storage.clone();
                            let mut reference_world = phase4_world.clone();
                            let reference_resolutions = phase6b_targeted_resolution_m3(
                                &mut reference_world,
                                &config,
                                &partitions,
                            )
                            .unwrap();

                            let mut reused_storage = storage_before.clone();
                            let storage_resolutions =
                                phase6b_targeted_resolution_storage_m3_with_scratch(
                                    &mut reused_storage,
                                    &phase4_world.settlements,
                                    phase4_world.current_day,
                                    &config,
                                    &partitions,
                                    &mut reused_scratch,
                                )
                                .unwrap();
                            assert_eq!(storage_resolutions, reference_resolutions);
                            assert_food_parity(&reference_world, &reused_storage);

                            let mut fresh_storage = storage_before;
                            let fresh_resolutions =
                                phase6b_targeted_resolution_storage_m3_with_scratch(
                                    &mut fresh_storage,
                                    &phase4_world.settlements,
                                    phase4_world.current_day,
                                    &config,
                                    &partitions,
                                    &mut M3Phase6BStorageScratch::default(),
                                )
                                .unwrap();
                            assert_eq!(fresh_resolutions, storage_resolutions);
                            assert_food_parity(&reference_world, &fresh_storage);

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
}

#[test]
fn shuffled_storage_and_noncanonical_partitions_match_reference() {
    let config = config_for(300, 3, 8, 0.5, SEEDS[0]);
    let world = initialize_world(&config).unwrap();
    let graph = graph_for(&world, &config);
    let mut ordered_storage = SegmentedAgentStorage::from_agents(&world.agents);
    let mut ordered_intent_scratch = Vec::new();
    let projection = M3Phase4SlotProjection::new(&graph, &ordered_storage).unwrap();
    let (phase4_world, partitions) = phase4_inputs(
        &world,
        &config,
        &mut ordered_storage,
        &graph,
        &projection,
        &mut ordered_intent_scratch,
    );

    let mut reference_world = phase4_world.clone();
    let expected =
        phase6b_targeted_resolution_m3(&mut reference_world, &config, &partitions).unwrap();
    let mut reversed_partitions = partitions.clone();
    reversed_partitions.reverse();
    let mut shuffled_agents = phase4_world.agents.clone();
    shuffled_agents.reverse();
    let mut shuffled_storage = SegmentedAgentStorage::from_agents(&shuffled_agents);
    let mut scratch = M3Phase6BStorageScratch::default();
    let actual = phase6b_targeted_resolution_storage_m3_with_scratch(
        &mut shuffled_storage,
        &phase4_world.settlements,
        phase4_world.current_day,
        &config,
        &reversed_partitions,
        &mut scratch,
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_food_parity(&reference_world, &shuffled_storage);
}

#[test]
fn m3_storage_phase6b_matches_every_reference_structural_error() {
    let config = config_for(300, 3, 8, 0.5, SEEDS[1]);
    let world = initialize_world(&config).unwrap();
    let group = world.settlements[0].group_id;
    let agent = world
        .agents
        .iter()
        .find(|agent| agent.group_id == group)
        .unwrap()
        .agent_id;

    let empty = SettlementIntentPartition {
        group_id: group,
        intents: Vec::new(),
    };
    compare_reference_and_storage_error(&world, &config, &[empty.clone(), empty]);
    compare_reference_and_storage_error(
        &world,
        &config,
        &[SettlementIntentPartition {
            group_id: GroupId(u16::MAX),
            intents: Vec::new(),
        }],
    );
    compare_reference_and_storage_error(
        &world,
        &config,
        &[SettlementIntentPartition {
            group_id: group,
            intents: vec![Intent::GiveFood {
                agent_id: agent,
                group_id: GroupId(group.0.wrapping_add(1)),
                target_agent_id: None,
                requested_amount: 1.0,
            }],
        }],
    );
    for requested_amount in [-1.0, f32::INFINITY, f32::NAN] {
        compare_reference_and_storage_error(
            &world,
            &config,
            &[SettlementIntentPartition {
                group_id: group,
                intents: vec![Intent::GiveFood {
                    agent_id: agent,
                    group_id: group,
                    target_agent_id: None,
                    requested_amount,
                }],
            }],
        );
    }
    compare_reference_and_storage_error(
        &world,
        &config,
        &[SettlementIntentPartition {
            group_id: group,
            intents: vec![Intent::GiveFood {
                agent_id: AgentId(u32::MAX),
                group_id: group,
                target_agent_id: None,
                requested_amount: 1.0,
            }],
        }],
    );
    compare_reference_and_storage_error(
        &world,
        &config,
        &[SettlementIntentPartition {
            group_id: group,
            intents: vec![Intent::StealFood {
                agent_id: agent,
                group_id: group,
                target_agent_id: Some(AgentId(u32::MAX)),
                requested_amount: 1.0,
            }],
        }],
    );

    for probability in [2.0, f32::NAN] {
        let mut invalid_probability = config.clone();
        invalid_probability.interaction.theft_success_probability = probability;
        compare_reference_and_storage_error(&world, &invalid_probability, &[]);
    }
}

#[test]
fn m3_storage_phase6b_preserves_keyed_output_order_and_cross_settlement_transfers() {
    let mut config = config_for(300, 3, 8, 0.5, SEEDS[2]);
    config.interaction.theft_success_probability = 1.0;
    let world = initialize_world(&config).unwrap();
    let graph = graph_for(&world, &config);
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let projection = M3Phase4SlotProjection::new(&graph, &storage).unwrap();
    let mut intent_scratch = Vec::new();
    let (phase4_world, partitions) = phase4_inputs(
        &world,
        &config,
        &mut storage,
        &graph,
        &projection,
        &mut intent_scratch,
    );
    let mut reference_world = phase4_world.clone();
    let expected =
        phase6b_targeted_resolution_m3(&mut reference_world, &config, &partitions).unwrap();
    let mut scratch = M3Phase6BStorageScratch::default();
    let actual = phase6b_targeted_resolution_storage_m3_with_scratch(
        &mut storage,
        &phase4_world.settlements,
        phase4_world.current_day,
        &config,
        &partitions,
        &mut scratch,
    )
    .unwrap();
    assert_eq!(actual, expected);
    assert_food_parity(&reference_world, &storage);
    for settlement in &actual {
        assert!(settlement.zero_target.windows(2).all(|pair| (
            pair[0].initiator_agent_id,
            pair[0].action_kind
        ) <= (
            pair[1].initiator_agent_id,
            pair[1].action_kind
        )));
        assert_eq!(
            settlement.resolutions,
            [
                settlement.zero_target.clone(),
                settlement.keyed_stream.clone()
            ]
            .concat()
        );
    }
    assert!(
        actual
            .windows(2)
            .all(|pair| pair[0].group_id < pair[1].group_id)
    );
    let groups_by_agent: std::collections::HashMap<_, _> = phase4_world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    assert!(actual.iter().any(|settlement| {
        settlement.keyed_stream.iter().any(|record| {
            let Some(target_id) = record.target_agent_id else {
                return false;
            };
            groups_by_agent[&target_id] != record.group_id
                && matches!(record.outcome, TargetedOutcome::Applied { amount } if amount > 0.0)
        })
    }));
}
