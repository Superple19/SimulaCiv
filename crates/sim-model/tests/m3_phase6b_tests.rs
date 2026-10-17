use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::{
    AgentState, Intent, Phase6BError, SettlementIntentPartition, SettlementState, SimConfig,
    TargetedActionKind, TargetedOutcome, TargetedResolution, WorldState,
    compare_keyed_interactions, compute_m3_resolution_key, compute_resolution_key,
    phase6b_targeted_resolution, phase6b_targeted_resolution_m3,
};

const M3_CONFIG: &str = r#"
profile = "m3"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 4
settlement_count = 2
initial_health = 0.8
initial_food = 10.0
initial_wealth = 10000
initial_settlement_resource = 25.0
initial_treasury = 2500

[traits]
prod_min = 0.5
prod_max = 2.5
coop_min = 0.0
coop_max = 1.0
aggr_min = 0.0
aggr_max = 1.0
risk_min = 0.0
risk_max = 1.0

[environment]
carrying_capacity = 100.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.25

[economy]
base_work_yield = 4.0
food_price = 100
target_food = 20.0
target_reserve = 10000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 6.0
theft_amount = 6.0
theft_success_probability = 1.0
starvation_threshold = 10.0
relationship_degree_k = 1
p_local = 0.5

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 0.5
trait_weight_aggression = 0.5
trait_weight_risk_tolerance = 0.5
"#;

fn config() -> SimConfig {
    SimConfig::parse_and_validate(M3_CONFIG).expect("M3 test config is valid")
}

fn agent(id: u32, group: u16, food: f32, alive: bool, health: f32) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        group_id: GroupId(group),
        dense_slot: DenseSlot(id),
        alive,
        birth_day: SimulationDay(0),
        health,
        food,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
    }
}

fn world(agents: Vec<AgentState>) -> WorldState {
    let mut group_ids: Vec<_> = agents.iter().map(|agent| agent.group_id).collect();
    group_ids.sort_unstable();
    group_ids.dedup();
    WorldState {
        current_day: SimulationDay(23),
        agents,
        settlements: group_ids
            .into_iter()
            .map(|group_id| SettlementState {
                group_id,
                resource: 50.0,
                treasury: 1000,
            })
            .collect(),
        initial_money_supply: 2000,
    }
}

fn partition(group_id: u16, intents: Vec<Intent>) -> SettlementIntentPartition {
    SettlementIntentPartition {
        group_id: GroupId(group_id),
        intents,
    }
}

fn give(agent_id: u32, group_id: u16, target_id: Option<u32>, amount: f32) -> Intent {
    Intent::GiveFood {
        agent_id: AgentId(agent_id),
        group_id: GroupId(group_id),
        target_agent_id: target_id.map(AgentId),
        requested_amount: amount,
    }
}

fn steal(agent_id: u32, group_id: u16, target_id: Option<u32>, amount: f32) -> Intent {
    Intent::StealFood {
        agent_id: AgentId(agent_id),
        group_id: GroupId(group_id),
        target_agent_id: target_id.map(AgentId),
        requested_amount: amount,
    }
}

fn total_food(world: &WorldState) -> f32 {
    world.agents.iter().map(|agent| agent.food).sum()
}

#[test]
fn m3_key_omits_group_bits_and_matches_a_frozen_golden() {
    let key = compute_m3_resolution_key(0x0123_4567_89ab_cdef, 7, 0, 7, 1, 3);
    assert_eq!(
        key,
        compute_resolution_key(0x0123_4567_89ab_cdef, 7, 0, 0, 7, 1, 3)
    );
    assert_ne!(
        key,
        compute_resolution_key(0x0123_4567_89ab_cdef, 7, 0, 7, 7, 1, 3)
    );
    assert_eq!(key, 0xbb56_2e6f_e607_6d7f);
}

#[test]
fn cross_settlement_give_and_steal_use_the_existing_food_command_and_conserve_food() {
    let config = config();
    let give_world = world(vec![
        agent(1, 0, 8.0, true, 1.0),
        agent(2, 1, 2.0, true, 1.0),
    ]);
    let give_before = total_food(&give_world);
    let mut give_world = give_world;
    let give_result = phase6b_targeted_resolution_m3(
        &mut give_world,
        &config,
        &[partition(0, vec![give(1, 0, Some(2), 3.0)])],
    )
    .unwrap();
    assert_eq!(give_world.agents[0].food, 5.0);
    assert_eq!(give_world.agents[1].food, 5.0);
    assert_eq!(total_food(&give_world), give_before);
    assert_eq!(
        give_result[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 3.0 }
    );

    let steal_world = world(vec![
        agent(1, 0, 1.0, true, 1.0),
        agent(2, 1, 6.0, true, 1.0),
    ]);
    let steal_before = total_food(&steal_world);
    let mut steal_world = steal_world;
    let steal_result = phase6b_targeted_resolution_m3(
        &mut steal_world,
        &config,
        &[partition(0, vec![steal(1, 0, Some(2), 2.0)])],
    )
    .unwrap();
    assert_eq!(steal_world.agents[0].food, 3.0);
    assert_eq!(steal_world.agents[1].food, 4.0);
    assert_eq!(total_food(&steal_world), steal_before);
    assert_eq!(
        steal_result[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 2.0 }
    );
    assert!(steal_result[0].keyed_stream[0].theft_success_draw.is_some());
}

#[test]
fn global_keyed_stream_commits_across_groups_and_later_entries_observe_live_state() {
    let mut config = config();
    config.world.master_seed = 0;
    let mut world = world(vec![
        agent(2, 1, 5.0, true, 1.0),
        agent(3, 2, 8.0, true, 1.0),
        agent(1, 0, 5.0, true, 1.0),
    ]);
    let before = total_food(&world);
    let partitions = [
        partition(1, vec![give(2, 1, Some(3), 2.0)]),
        partition(0, vec![give(1, 0, Some(3), 2.0)]),
    ];
    let mut expected_order = [AgentId(1), AgentId(2)];
    expected_order.sort_by(|a, b| {
        let key_a = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            world.current_day.as_u32(),
            3,
            a.as_u32(),
            TargetedActionKind::GiveFood.action_kind(),
        );
        let key_b = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            world.current_day.as_u32(),
            3,
            b.as_u32(),
            TargetedActionKind::GiveFood.action_kind(),
        );
        key_a.cmp(&key_b).then_with(|| a.cmp(b))
    });
    assert_eq!(expected_order, [AgentId(2), AgentId(1)]);

    let result = phase6b_targeted_resolution_m3(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        result.iter().map(|row| row.group_id).collect::<Vec<_>>(),
        [GroupId(0), GroupId(1)]
    );
    assert_eq!(result[0].keyed_stream[0].initiator_agent_id, AgentId(1));
    assert_eq!(result[1].keyed_stream[0].initiator_agent_id, AgentId(2));
    for (index, initiator_id) in expected_order.iter().enumerate() {
        let record = result
            .iter()
            .flat_map(|row| &row.keyed_stream)
            .find(|record| record.initiator_agent_id == *initiator_id)
            .unwrap();
        let expected = if index == 0 {
            TargetedOutcome::Applied { amount: 2.0 }
        } else {
            TargetedOutcome::TargetIneligible
        };
        assert_eq!(record.outcome, expected);
    }
    assert_eq!(world.agents[1].food, 10.0);
    assert_eq!(total_food(&world), before);
}

#[test]
fn output_groups_are_sorted_and_each_group_lists_zero_targets_before_global_key_subsequence() {
    let mut config = config();
    config.interaction.theft_success_probability = 0.0;
    let mut world = world(vec![
        agent(11, 2, 5.0, true, 1.0),
        agent(20, 5, 1.0, true, 1.0),
        agent(3, 5, 5.0, true, 1.0),
        agent(9, 2, 5.0, true, 1.0),
        agent(6, 5, 5.0, true, 1.0),
        agent(4, 2, 5.0, true, 1.0),
        agent(8, 2, 5.0, true, 1.0),
    ]);
    let partitions = [
        partition(5, vec![give(6, 5, None, 0.0), give(3, 5, Some(20), 0.0)]),
        partition(
            2,
            vec![
                give(11, 2, None, 0.0),
                steal(4, 2, None, 0.0),
                give(8, 2, Some(20), 0.0),
                steal(9, 2, Some(20), 0.0),
            ],
        ),
    ];
    let food_before = total_food(&world);
    let result = phase6b_targeted_resolution_m3(&mut world, &config, &partitions).unwrap();

    assert_eq!(
        result.iter().map(|row| row.group_id).collect::<Vec<_>>(),
        [GroupId(2), GroupId(5)]
    );
    assert_eq!(
        result[0]
            .zero_target
            .iter()
            .map(|record| (record.initiator_agent_id, record.action_kind))
            .collect::<Vec<_>>(),
        [
            (AgentId(4), TargetedActionKind::StealFood),
            (AgentId(11), TargetedActionKind::GiveFood),
        ]
    );
    assert_eq!(result[1].zero_target[0].initiator_agent_id, AgentId(6));
    for zero_target in result.iter().flat_map(|row| &row.zero_target) {
        assert_eq!(zero_target.resolution_key, None);
        assert_eq!(zero_target.theft_success_draw, None);
        assert_eq!(zero_target.outcome, TargetedOutcome::ZeroTarget);
    }

    let mut expected_global = [
        (
            GroupId(5),
            AgentId(3),
            AgentId(20),
            TargetedActionKind::GiveFood,
        ),
        (
            GroupId(2),
            AgentId(8),
            AgentId(20),
            TargetedActionKind::GiveFood,
        ),
        (
            GroupId(2),
            AgentId(9),
            AgentId(20),
            TargetedActionKind::StealFood,
        ),
    ];
    expected_global.sort_by(|a, b| {
        let key_a = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            world.current_day.as_u32(),
            a.2.as_u32(),
            a.1.as_u32(),
            a.3.action_kind(),
        );
        let key_b = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            world.current_day.as_u32(),
            b.2.as_u32(),
            b.1.as_u32(),
            b.3.action_kind(),
        );
        compare_keyed_interactions(key_a, a.2, a.1, a.3, key_b, b.2, b.1, b.3)
    });
    for row in &result {
        let expected: Vec<_> = expected_global
            .iter()
            .filter(|entry| entry.0 == row.group_id)
            .map(|entry| entry.1)
            .collect();
        assert_eq!(
            row.keyed_stream
                .iter()
                .map(|record| record.initiator_agent_id)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            row.resolutions,
            row.zero_target
                .iter()
                .chain(&row.keyed_stream)
                .cloned()
                .collect::<Vec<_>>()
        );
    }
    assert_eq!(total_food(&world), food_before);
}

#[test]
fn single_settlement_matches_manual_resolution_in_m3_key_order() {
    let config = config();
    let mut initial = world(vec![
        agent(2, 7, 10.0, true, 1.0),
        agent(3, 7, 2.0, true, 1.0),
        agent(1, 7, 10.0, true, 1.0),
    ]);
    initial.settlements = vec![SettlementState {
        group_id: GroupId(7),
        resource: 0.0,
        treasury: 0,
    }];
    let mut manual_world = initial.clone();
    let partitions = [partition(
        7,
        vec![give(2, 7, Some(3), 3.0), give(1, 7, Some(3), 3.0)],
    )];
    let mut manual_order = [AgentId(1), AgentId(2)];
    manual_order.sort_by(|a, b| {
        let key_a = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            manual_world.current_day.as_u32(),
            3,
            a.as_u32(),
            TargetedActionKind::GiveFood.action_kind(),
        );
        let key_b = compute_m3_resolution_key(
            config.world.master_seed,
            config.world.replicate_id,
            manual_world.current_day.as_u32(),
            3,
            b.as_u32(),
            TargetedActionKind::GiveFood.action_kind(),
        );
        key_a.cmp(&key_b).then_with(|| a.cmp(b))
    });
    let manual_records: Vec<_> = manual_order
        .iter()
        .map(|initiator_id| {
            let key = compute_m3_resolution_key(
                config.world.master_seed,
                config.world.replicate_id,
                manual_world.current_day.as_u32(),
                3,
                initiator_id.as_u32(),
                TargetedActionKind::GiveFood.action_kind(),
            );
            let source = manual_world
                .agents
                .iter_mut()
                .find(|agent| agent.agent_id == *initiator_id)
                .unwrap();
            let amount = 3.0f32.min(source.food);
            source.food -= amount;
            let target = manual_world
                .agents
                .iter_mut()
                .find(|agent| agent.agent_id == AgentId(3))
                .unwrap();
            target.food += amount;
            TargetedResolution {
                group_id: GroupId(7),
                initiator_agent_id: *initiator_id,
                target_agent_id: Some(AgentId(3)),
                action_kind: TargetedActionKind::GiveFood,
                resolution_key: Some(key),
                theft_success_draw: None,
                outcome: TargetedOutcome::Applied { amount },
            }
        })
        .collect();

    let mut actual_world = initial;
    let actual = phase6b_targeted_resolution_m3(&mut actual_world, &config, &partitions).unwrap();
    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].keyed_stream, manual_records);
    assert_eq!(actual[0].resolutions, manual_records);
    assert_eq!(actual_world, manual_world);
}

#[test]
fn m3_preserves_every_existing_structural_error_and_precommit_atomicity() {
    let base_world = world(vec![
        agent(1, 0, 5.0, true, 1.0),
        agent(2, 1, 5.0, true, 1.0),
    ]);
    let base_config = config();
    let valid = [partition(0, vec![give(1, 0, Some(2), 1.0)])];

    let mut invalid_probability = base_config.clone();
    invalid_probability.interaction.theft_success_probability = 2.0;
    assert_same_error_and_state(
        &base_world,
        &invalid_probability,
        &valid,
        Phase6BError::InvalidTheftSuccessProbability(2.0),
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(0, vec![]), partition(0, vec![])],
        Phase6BError::DuplicatePartition(GroupId(0)),
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(9, vec![])],
        Phase6BError::MissingSettlement(GroupId(9)),
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(0, vec![give(1, 1, Some(2), 1.0)])],
        Phase6BError::PartitionGroupMismatch {
            agent_id: AgentId(1),
            intent_group_id: GroupId(1),
            partition_group_id: GroupId(0),
        },
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(0, vec![give(1, 0, None, -1.0)])],
        Phase6BError::InvalidRequestedAmount {
            agent_id: AgentId(1),
            requested_amount: -1.0,
        },
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(0, vec![give(99, 0, None, 1.0)])],
        Phase6BError::MissingInitiator(AgentId(99)),
    );
    assert_same_error_and_state(
        &base_world,
        &base_config,
        &[partition(0, vec![give(1, 0, Some(99), 1.0)])],
        Phase6BError::MissingTarget(AgentId(99)),
    );
}

fn assert_same_error_and_state(
    initial: &WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
    expected: Phase6BError,
) {
    let mut m2_world = initial.clone();
    let mut m3_world = initial.clone();
    let m2_error = phase6b_targeted_resolution(&mut m2_world, config, partitions).unwrap_err();
    let m3_error = phase6b_targeted_resolution_m3(&mut m3_world, config, partitions).unwrap_err();
    assert_eq!(m2_error, expected);
    assert_eq!(m3_error, expected);
    assert_eq!(m2_world, *initial);
    assert_eq!(m3_world, *initial);
}
