use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sim_core::{AgentId, GroupId};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, run_m3_reference_day,
};
use sim_model::state::WorldState;
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Intent, M3Phase4SlotProjection, M3Phase6BParallelScratch, M3Phase6BStorageScratch,
    PrimaryActionChoice, RelationshipGraph, SettlementIntentPartition, SimConfig,
    TargetedActionKind, compare_keyed_interactions, compute_m3_resolution_key,
    generate_intents_m3_into, generate_intents_m3_storage_into, initialize_world,
    phase1_resource_regrowth, phase2_biological_degradation, phase3_observation_and_features,
    phase4_primary_action_selection_into, phase4_primary_action_selection_storage_into,
    phase5_partition_intents_baseline,
    phase6b_targeted_resolution_storage_m3_parallel_with_scratch,
    phase6b_targeted_resolution_storage_m3_with_scratch,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroUsize;

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

fn config_for(population: u64, degree: i64, p_local: f32) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(CONFIG_TOML).unwrap();
    config.world.initial_population = population;
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

    let partitions = phase5_partition_intents_baseline(&reference_intents).unwrap();
    (phase4_world, partitions)
}

fn make_pools() -> Vec<(usize, ThreadPool)> {
    [1, 2, 4]
        .into_iter()
        .map(|count| {
            (
                count,
                ThreadPoolBuilder::new().num_threads(count).build().unwrap(),
            )
        })
        .collect()
}

fn food_by_agent(storage: &SegmentedAgentStorage) -> BTreeMap<AgentId, f32> {
    storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.food().iter().copied())
        .collect()
}

fn run_parallel(
    storage: &mut SegmentedAgentStorage,
    world: &WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
    scratch: &mut M3Phase6BParallelScratch,
    pool: &ThreadPool,
    chunk_size: usize,
) -> Vec<sim_model::SettlementTargetedResolution> {
    phase6b_targeted_resolution_storage_m3_parallel_with_scratch(
        storage,
        &world.settlements,
        world.current_day,
        config,
        partitions,
        scratch,
        pool,
        NonZeroUsize::new(chunk_size).unwrap(),
    )
    .unwrap()
}

#[test]
fn parallel_m3_storage_phase6b_matches_serial_over_days_pool_and_chunk_matrix() {
    let pools = make_pools();
    for degree in [4, 16] {
        for p_local in [0.0, 0.5, 1.0] {
            let config = config_for(300, degree, p_local);
            let mut driver_world = initialize_world(&config).unwrap();
            let graph = graph_for(&driver_world, &config);
            let mut storage = SegmentedAgentStorage::from_agents(&driver_world.agents);
            let projection = M3Phase4SlotProjection::new(&graph, &storage).unwrap();
            let mut intent_scratch = Vec::with_capacity(degree as usize);
            let run_context =
                M0RunContext::new(config.world.master_seed, config.world.replicate_id);
            let reference_context =
                M3ReferenceContext::new(&driver_world, &config, &run_context).unwrap();

            let mut cases = Vec::new();
            for _ in 0..3 {
                let (phase4_world, partitions) = phase4_inputs(
                    &driver_world,
                    &config,
                    &mut storage,
                    &graph,
                    &projection,
                    &mut intent_scratch,
                );
                cases.push((phase4_world, partitions, storage.clone()));
                run_m3_reference_day(
                    &mut driver_world,
                    &config,
                    &reference_context,
                    &DayExecutionOptions {
                        metrics_enabled: false,
                        events_enabled: false,
                        snapshot_boundary: false,
                    },
                )
                .unwrap();
            }

            for (_, pool) in &pools {
                for chunk_size in [64, 256, 1024] {
                    let mut scratch = M3Phase6BParallelScratch::with_capacity(300, 3, 300);
                    let mut serial_scratch = M3Phase6BStorageScratch::with_capacity(300);
                    for (phase4_world, partitions, storage_before) in &cases {
                        let mut serial_storage = storage_before.clone();
                        let expected = phase6b_targeted_resolution_storage_m3_with_scratch(
                            &mut serial_storage,
                            &phase4_world.settlements,
                            phase4_world.current_day,
                            &config,
                            partitions,
                            &mut serial_scratch,
                        )
                        .unwrap();
                        let mut parallel_storage = storage_before.clone();
                        let actual = run_parallel(
                            &mut parallel_storage,
                            phase4_world,
                            &config,
                            partitions,
                            &mut scratch,
                            pool,
                            chunk_size,
                        );
                        assert_eq!(actual, expected);
                        assert_eq!(parallel_storage.food(), serial_storage.food());
                        assert_eq!(
                            food_by_agent(&parallel_storage),
                            food_by_agent(&serial_storage)
                        );
                    }
                }
            }
        }
    }
}

fn assert_error_parity_for_all_pool_chunks(
    world: &WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
    pools: &[(usize, ThreadPool)],
) -> sim_model::Phase6BError {
    let before = SegmentedAgentStorage::from_agents(&world.agents);
    let mut serial_storage = before.clone();
    let expected = phase6b_targeted_resolution_storage_m3_with_scratch(
        &mut serial_storage,
        &world.settlements,
        world.current_day,
        config,
        partitions,
        &mut M3Phase6BStorageScratch::default(),
    )
    .unwrap_err();
    assert_eq!(food_by_agent(&serial_storage), food_by_agent(&before));

    for (_, pool) in pools {
        for chunk_size in [64, 256, 1024] {
            let mut parallel_storage = before.clone();
            let error = phase6b_targeted_resolution_storage_m3_parallel_with_scratch(
                &mut parallel_storage,
                &world.settlements,
                world.current_day,
                config,
                partitions,
                &mut M3Phase6BParallelScratch::default(),
                pool,
                NonZeroUsize::new(chunk_size).unwrap(),
            )
            .unwrap_err();
            assert_eq!(format!("{error:?}"), format!("{expected:?}"));
            assert_eq!(food_by_agent(&parallel_storage), food_by_agent(&before));
        }
    }
    expected
}

#[test]
fn parallel_m3_storage_phase6b_preserves_error_precedence_and_atomicity() {
    let pools = make_pools();
    let config = config_for(300, 8, 0.5);
    let world = initialize_world(&config).unwrap();
    let group = world.settlements[0].group_id;
    let agent = world
        .agents
        .iter()
        .find(|agent| agent.group_id == group)
        .unwrap()
        .agent_id;

    let mut invalid_probability = config.clone();
    invalid_probability.interaction.theft_success_probability = -0.1;
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &invalid_probability, &[], &pools),
        sim_model::Phase6BError::InvalidTheftSuccessProbability(_)
    ));

    let mismatch = [SettlementIntentPartition {
        group_id: group,
        intents: vec![Intent::GiveFood {
            agent_id: agent,
            group_id: GroupId(group.0.wrapping_add(1)),
            target_agent_id: None,
            requested_amount: 1.0,
        }],
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &mismatch, &pools),
        sim_model::Phase6BError::PartitionGroupMismatch { .. }
    ));

    let bad_amount = [SettlementIntentPartition {
        group_id: group,
        intents: vec![Intent::GiveFood {
            agent_id: agent,
            group_id: group,
            target_agent_id: None,
            requested_amount: -1.0,
        }],
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &bad_amount, &pools),
        sim_model::Phase6BError::InvalidRequestedAmount { .. }
    ));

    let missing_initiator = [SettlementIntentPartition {
        group_id: group,
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(u32::MAX),
            group_id: group,
            target_agent_id: None,
            requested_amount: 1.0,
        }],
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &missing_initiator, &pools),
        sim_model::Phase6BError::MissingInitiator(_)
    ));

    let missing_target = [SettlementIntentPartition {
        group_id: group,
        intents: vec![Intent::StealFood {
            agent_id: agent,
            group_id: group,
            target_agent_id: Some(AgentId(u32::MAX)),
            requested_amount: 1.0,
        }],
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &missing_target, &pools),
        sim_model::Phase6BError::MissingTarget(_)
    ));

    let multiple_errors = [SettlementIntentPartition {
        group_id: group,
        intents: vec![
            Intent::GiveFood {
                agent_id: agent,
                group_id: GroupId(group.0.wrapping_add(1)),
                target_agent_id: None,
                requested_amount: -1.0,
            },
            Intent::GiveFood {
                agent_id: AgentId(u32::MAX),
                group_id: group,
                target_agent_id: None,
                requested_amount: 1.0,
            },
        ],
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &multiple_errors, &pools),
        sim_model::Phase6BError::PartitionGroupMismatch { .. }
    ));

    let duplicate_partitions = [
        SettlementIntentPartition {
            group_id: group,
            intents: Vec::new(),
        },
        SettlementIntentPartition {
            group_id: group,
            intents: vec![Intent::GiveFood {
                agent_id: agent,
                group_id: GroupId(group.0.wrapping_add(1)),
                target_agent_id: None,
                requested_amount: -1.0,
            }],
        },
    ];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &duplicate_partitions, &pools),
        sim_model::Phase6BError::DuplicatePartition(_)
    ));

    let missing_settlement = [SettlementIntentPartition {
        group_id: GroupId(u16::MAX),
        intents: Vec::new(),
    }];
    assert!(matches!(
        assert_error_parity_for_all_pool_chunks(&world, &config, &missing_settlement, &pools),
        sim_model::Phase6BError::MissingSettlement(_)
    ));
}

#[test]
fn parallel_m3_storage_phase6b_is_invariant_to_shuffled_storage_slots() {
    let pools = make_pools();
    let config = config_for(300, 8, 0.5);
    let world = initialize_world(&config).unwrap();
    let graph = graph_for(&world, &config);
    let mut ordered_storage = SegmentedAgentStorage::from_agents(&world.agents);
    let projection = M3Phase4SlotProjection::new(&graph, &ordered_storage).unwrap();
    let (phase4_world, partitions) = phase4_inputs(
        &world,
        &config,
        &mut ordered_storage,
        &graph,
        &projection,
        &mut Vec::new(),
    );
    let ordered_before = ordered_storage.clone();
    let mut serial_storage = ordered_before.clone();
    let expected = phase6b_targeted_resolution_storage_m3_with_scratch(
        &mut serial_storage,
        &phase4_world.settlements,
        phase4_world.current_day,
        &config,
        &partitions,
        &mut M3Phase6BStorageScratch::default(),
    )
    .unwrap();

    let mut shuffled_agents = phase4_world.agents.clone();
    shuffled_agents.reverse();
    let shuffled_storage = SegmentedAgentStorage::from_agents(&shuffled_agents);
    for (_, pool) in &pools {
        for chunk_size in [64, 256, 1024] {
            let mut parallel_storage = shuffled_storage.clone();
            let actual = run_parallel(
                &mut parallel_storage,
                &phase4_world,
                &config,
                &partitions,
                &mut M3Phase6BParallelScratch::default(),
                pool,
                chunk_size,
            );
            assert_eq!(actual, expected);
            assert_eq!(
                food_by_agent(&parallel_storage),
                food_by_agent(&serial_storage)
            );
        }
    }
}

#[derive(Clone, Copy)]
struct KeyedEndpoints {
    key: u64,
    target: AgentId,
    initiator: AgentId,
    action: TargetedActionKind,
}

fn keyed_endpoints(
    partitions: &[SettlementIntentPartition],
    config: &SimConfig,
    day: u32,
) -> Vec<KeyedEndpoints> {
    let mut keyed = Vec::new();
    for partition in partitions {
        for intent in &partition.intents {
            let entry = match *intent {
                Intent::GiveFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => Some((agent_id, target, TargetedActionKind::GiveFood)),
                Intent::StealFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => Some((agent_id, target, TargetedActionKind::StealFood)),
                _ => None,
            };
            if let Some((initiator, target, action)) = entry {
                keyed.push(KeyedEndpoints {
                    key: compute_m3_resolution_key(
                        config.world.master_seed,
                        config.world.replicate_id,
                        day,
                        target.as_u32(),
                        initiator.as_u32(),
                        action.action_kind(),
                    ),
                    target,
                    initiator,
                    action,
                });
            }
        }
    }
    keyed.sort_by(|a, b| {
        compare_keyed_interactions(
            a.key,
            a.target,
            a.initiator,
            a.action,
            b.key,
            b.target,
            b.initiator,
            b.action,
        )
    });
    keyed
}

#[test]
fn dependency_levels_have_disjoint_endpoints_and_preserve_each_endpoint_order() {
    let pools = make_pools();
    let config = config_for(300, 8, 0.5);
    let world = initialize_world(&config).unwrap();
    let graph = graph_for(&world, &config);
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let projection = M3Phase4SlotProjection::new(&graph, &storage).unwrap();
    let (phase4_world, partitions) = phase4_inputs(
        &world,
        &config,
        &mut storage,
        &graph,
        &projection,
        &mut Vec::new(),
    );
    let mut scratch = M3Phase6BParallelScratch::default();
    run_parallel(
        &mut storage,
        &phase4_world,
        &config,
        &partitions,
        &mut scratch,
        &pools[1].1,
        256,
    );

    let keyed = keyed_endpoints(&partitions, &config, phase4_world.current_day.as_u32());
    let assignments = scratch.keyed_level_assignments();
    assert_eq!(assignments.len(), keyed.len());
    let mut levels: HashMap<usize, HashSet<AgentId>> = HashMap::new();
    let mut last_level = HashMap::<AgentId, usize>::new();
    let mut level_sizes = HashMap::<usize, usize>::new();
    for (entry, &level) in keyed.iter().zip(assignments) {
        assert!(level > 0);
        let endpoints = levels.entry(level).or_default();
        assert!(
            endpoints.insert(entry.initiator),
            "shared initiator in level {level}"
        );
        assert!(
            endpoints.insert(entry.target),
            "shared target in level {level}"
        );
        for endpoint in [entry.initiator, entry.target] {
            if let Some(previous) = last_level.insert(endpoint, level) {
                assert!(previous < level, "endpoint level did not increase");
            }
        }
        *level_sizes.entry(level).or_default() += 1;
    }
    let stats = scratch.level_stats();
    assert_eq!(stats.non_empty_level_count, levels.len());
    assert_eq!(
        stats.max_level_size,
        level_sizes.values().copied().max().unwrap_or(0)
    );
}
