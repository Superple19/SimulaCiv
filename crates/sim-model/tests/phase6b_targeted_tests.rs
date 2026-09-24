use sim_core::{
    AgentId, DenseSlot, GroupId, RngCoordinate, SimulationDay, coordinate_prng_f32,
    coordinate_prng_u64,
};
use sim_model::{
    AgentState, Command, CommandExecutionError, Intent, Phase6BError, SettlementIntentPartition,
    SettlementState, SimConfig, TargetedActionKind, TargetedOutcome, WorldState,
    compare_keyed_interactions, compute_resolution_key, execute_phases_1_and_2, generate_intents,
    initialize_world, phase3_observation_and_features, phase4_primary_action_selection,
    phase5_partition_intents, phase6a_work_resolution, phase6b_targeted_resolution,
};

const BASE_TOML: &str = r#"
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
theft_success_probability = 0.7
starvation_threshold = 10.0

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

fn make_test_agent(id: u32, group: u16, food: f32, health: f32, alive: bool) -> AgentState {
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

fn make_test_world(agents: Vec<AgentState>) -> WorldState {
    WorldState {
        current_day: SimulationDay(0),
        agents,
        settlements: vec![
            SettlementState {
                group_id: GroupId(0),
                resource: 50.0,
                treasury: 1000,
            },
            SettlementState {
                group_id: GroupId(1),
                resource: 50.0,
                treasury: 1000,
            },
        ],
        initial_money_supply: 2000,
    }
}

// 1. ResolutionKey GiveFood golden
#[test]
fn test_resolution_key_give_food_golden() {
    let key = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 1, 3);
    assert_eq!(key, 0xbb562e6fe6076d7f);
}

// 2. ResolutionKey StealFood golden
#[test]
fn test_resolution_key_steal_food_golden() {
    let key = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 1, 4);
    assert_eq!(key, 0x4949b49a3be6f25c);
}

// 3. action_kind changes key and second steal food golden
#[test]
fn test_resolution_key_action_kind_differs_and_second_steal() {
    let key_give = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 1, 3);
    let key_steal = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 1, 4);
    assert_ne!(key_give, key_steal);

    let key_steal_2 = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 2, 4);
    assert_eq!(key_steal_2, 0xd5f6df5aada109b8);
}

// 4. Exact keyed-stream ordering
#[test]
fn test_exact_keyed_stream_ordering() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
        make_test_agent(7, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::GiveFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 1.0,
            },
            Intent::StealFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 1.0,
            },
            Intent::StealFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 1.0,
            },
        ],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res.len(), 1);
    let keyed = &res[0].keyed_stream;
    assert_eq!(keyed.len(), 3);

    // 0x4949b49a3be6f25c < 0xbb562e6fe6076d7f < 0xd5f6df5aada109b8
    assert_eq!(keyed[0].resolution_key, Some(0x4949b49a3be6f25c));
    assert_eq!(keyed[0].initiator_agent_id, AgentId(1));
    assert_eq!(keyed[0].action_kind, TargetedActionKind::StealFood);

    assert_eq!(keyed[1].resolution_key, Some(0xbb562e6fe6076d7f));
    assert_eq!(keyed[1].initiator_agent_id, AgentId(1));
    assert_eq!(keyed[1].action_kind, TargetedActionKind::GiveFood);

    assert_eq!(keyed[2].resolution_key, Some(0xd5f6df5aada109b8));
    assert_eq!(keyed[2].initiator_agent_id, AgentId(2));
    assert_eq!(keyed[2].action_kind, TargetedActionKind::StealFood);
}

// 5. Lexical tie-break comparator
#[test]
fn test_lexical_tie_break_comparator() {
    let synthetic_key = 0x1234_5678_9abc_def0;

    // target_agent_id comparison
    let ord1 = compare_keyed_interactions(
        synthetic_key,
        AgentId(1),
        AgentId(5),
        TargetedActionKind::StealFood,
        synthetic_key,
        AgentId(2),
        AgentId(5),
        TargetedActionKind::StealFood,
    );
    assert_eq!(ord1, std::cmp::Ordering::Less);

    // initiator_agent_id comparison
    let ord2 = compare_keyed_interactions(
        synthetic_key,
        AgentId(1),
        AgentId(3),
        TargetedActionKind::StealFood,
        synthetic_key,
        AgentId(1),
        AgentId(5),
        TargetedActionKind::StealFood,
    );
    assert_eq!(ord2, std::cmp::Ordering::Less);

    // action_kind comparison: GiveFood (3) < StealFood (4)
    let ord3 = compare_keyed_interactions(
        synthetic_key,
        AgentId(1),
        AgentId(3),
        TargetedActionKind::GiveFood,
        synthetic_key,
        AgentId(1),
        AgentId(3),
        TargetedActionKind::StealFood,
    );
    assert_eq!(ord3, std::cmp::Ordering::Less);

    // Identical
    let ord4 = compare_keyed_interactions(
        synthetic_key,
        AgentId(1),
        AgentId(3),
        TargetedActionKind::GiveFood,
        synthetic_key,
        AgentId(1),
        AgentId(3),
        TargetedActionKind::GiveFood,
    );
    assert_eq!(ord4, std::cmp::Ordering::Equal);
}

// 6. Zero-target preclassification
#[test]
fn test_zero_target_preclassification() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
        make_test_agent(3, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::StealFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: None,
                requested_amount: 5.0,
            },
            Intent::GiveFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: None,
                requested_amount: 5.0,
            },
        ],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res.len(), 1);
    assert!(res[0].keyed_stream.is_empty());
    assert_eq!(res[0].zero_target.len(), 2);

    // Ordered by ascending initiator AgentId
    assert_eq!(res[0].zero_target[0].initiator_agent_id, AgentId(1));
    assert_eq!(
        res[0].zero_target[0].action_kind,
        TargetedActionKind::GiveFood
    );
    assert_eq!(res[0].zero_target[0].resolution_key, None);
    assert_eq!(res[0].zero_target[0].theft_success_draw, None);
    assert_eq!(res[0].zero_target[0].outcome, TargetedOutcome::ZeroTarget);

    assert_eq!(res[0].zero_target[1].initiator_agent_id, AgentId(2));
    assert_eq!(
        res[0].zero_target[1].action_kind,
        TargetedActionKind::StealFood
    );
    assert_eq!(res[0].zero_target[1].resolution_key, None);
    assert_eq!(res[0].zero_target[1].theft_success_draw, None);
    assert_eq!(res[0].zero_target[1].outcome, TargetedOutcome::ZeroTarget);

    // Food unmodified
    assert_eq!(world.agents[0].food, 10.0);
    assert_eq!(world.agents[1].food, 10.0);
}

// 7. TheftSuccess external golden
#[test]
fn test_theft_success_external_golden() {
    let coord = RngCoordinate::new(0x0123456789abcdef, 7, 0, 6, 4, 4, 0);
    let u64_val = coordinate_prng_u64(&coord);
    assert_eq!(u64_val, 0x48fd6bc0943aebf0);

    let u = coordinate_prng_f32(&coord);
    assert_eq!(u.to_bits(), 0x3e91fad6);
    assert_eq!(u, 0.28511685);
}

// 8. u < p success
#[test]
fn test_theft_threshold_u_less_than_p_success() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    // Agent 4 with seed 0x0123456789abcdef, rep 7, day 0 -> u = 0.28511685
    config.interaction.theft_success_probability = 0.30; // 0.28511685 < 0.30

    let mut world = make_test_world(vec![
        make_test_agent(4, 0, 5.0, 1.0, true),
        make_test_agent(7, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(4),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(7)),
            requested_amount: 3.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res[0].keyed_stream.len(), 1);
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 3.0 }
    );
    assert_eq!(world.agents[0].food, 8.0);
    assert_eq!(world.agents[1].food, 7.0);
}

// 9. u == p failure
#[test]
fn test_theft_threshold_u_equals_p_failure() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    // Set p equal to exact u value
    let coord = RngCoordinate::new(0x0123456789abcdef, 7, 0, 6, 4, 4, 0);
    let u = coordinate_prng_f32(&coord);
    config.interaction.theft_success_probability = u;

    let mut world = make_test_world(vec![
        make_test_agent(4, 0, 5.0, 1.0, true),
        make_test_agent(7, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(4),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(7)),
            requested_amount: 3.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res[0].keyed_stream.len(), 1);
    assert_eq!(res[0].keyed_stream[0].outcome, TargetedOutcome::TheftFailed);
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, Some(u));
    assert_eq!(world.agents[0].food, 5.0);
    assert_eq!(world.agents[1].food, 10.0);
}

// 10. p = 0.0 failure
#[test]
fn test_theft_threshold_p_zero() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 0.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 3.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res[0].keyed_stream[0].outcome, TargetedOutcome::TheftFailed);
    assert_eq!(world.agents[0].food, 5.0);
    assert_eq!(world.agents[1].food, 10.0);
}

// 11. p = 1.0 success
#[test]
fn test_theft_threshold_p_one() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 1.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 3.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 3.0 }
    );
    assert_eq!(world.agents[0].food, 8.0);
    assert_eq!(world.agents[1].food, 7.0);
}

// 12. GiveFood successful transfer
#[test]
fn test_give_food_successful_transfer() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 2.0, 1.0, true), // food < starvation_threshold (10.0)
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 4.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 4.0 }
    );
    assert_eq!(world.agents[0].food, 6.0);
    assert_eq!(world.agents[1].food, 6.0);
}

// 13. GiveFood actual amount clamped to live giver food
#[test]
fn test_give_food_amount_clamped_to_giver_food() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.5, 1.0, true), // only 2.5 food available
        make_test_agent(2, 0, 1.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 5.0, // requests 5.0
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 2.5 }
    );
    assert_eq!(world.agents[0].food, 0.0);
    assert_eq!(world.agents[1].food, 3.5);
}

// 14. GiveFood target live invalidation
#[test]
fn test_give_food_target_live_invalidation() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // Target dead
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 1.0, 1.0, false),
    ]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 2.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );

    // Target health <= 0
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 1.0, 0.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );

    // Target food >= starvation_threshold (10.0)
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 15.0, 1.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );

    // Target wrong group
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 1, 1.0, 1.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );

    // Self-target
    let mut world = make_test_world(vec![make_test_agent(1, 0, 10.0, 1.0, true)]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(1)),
            requested_amount: 2.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );

    // Giver food <= 0.0
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 0.0, 1.0, true),
        make_test_agent(2, 0, 1.0, 1.0, true),
    ]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 2.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
}

// 15. StealFood successful transfer
#[test]
fn test_steal_food_successful_transfer() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 1.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 4.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 4.0 }
    );
    assert_eq!(world.agents[0].food, 6.0);
    assert_eq!(world.agents[1].food, 6.0);
}

// 16. StealFood failed probability
#[test]
fn test_steal_food_failed_probability() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 0.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 4.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res[0].keyed_stream[0].outcome, TargetedOutcome::TheftFailed);
    assert!(res[0].keyed_stream[0].theft_success_draw.is_some());
    assert_eq!(world.agents[0].food, 2.0);
    assert_eq!(world.agents[1].food, 10.0);
}

// 17. StealFood amount clamped to live victim food
#[test]
fn test_steal_food_amount_clamped_to_victim_food() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 1.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.0, 1.0, true),
        make_test_agent(2, 0, 1.5, 1.0, true), // victim has only 1.5 food
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 5.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::Applied { amount: 1.5 }
    );
    assert_eq!(world.agents[0].food, 3.5);
    assert_eq!(world.agents[1].food, 0.0);
}

// 18. Invalid Steal target consumes no success draw
#[test]
fn test_invalid_steal_target_consumes_no_rng_draw() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // Victim food == 0.0
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 0.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 3.0,
        }],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Victim dead
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 5.0, 1.0, false),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Victim health <= 0
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 5.0, 0.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Wrong settlement
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 1, 5.0, 1.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Self-target
    let mut world = make_test_world(vec![make_test_agent(1, 0, 5.0, 1.0, true)]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(1)),
            requested_amount: 3.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::TargetIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);
}

// 19. Sequential-contention golden
#[test]
fn test_sequential_contention_golden_section_19() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.world.master_seed = 0x0123456789abcdef;
    config.world.replicate_id = 7;
    config.interaction.theft_success_probability = 1.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.0, 1.0, true),
        make_test_agent(2, 0, 2.0, 1.0, true),
        make_test_agent(7, 0, 5.0, 1.0, true),
    ]);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::StealFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 4.0,
            },
            Intent::StealFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 4.0,
            },
        ],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res.len(), 1);
    let keyed = &res[0].keyed_stream;
    assert_eq!(keyed.len(), 2);

    // Agent 1 -> 7 key is 0x4949b49a3be6f25c
    assert_eq!(keyed[0].resolution_key, Some(0x4949b49a3be6f25c));
    assert_eq!(keyed[0].initiator_agent_id, AgentId(1));
    assert_eq!(keyed[0].outcome, TargetedOutcome::Applied { amount: 4.0 });

    // Agent 2 -> 7 key is 0xd5f6df5aada109b8
    assert_eq!(keyed[1].resolution_key, Some(0xd5f6df5aada109b8));
    assert_eq!(keyed[1].initiator_agent_id, AgentId(2));
    assert_eq!(keyed[1].outcome, TargetedOutcome::Applied { amount: 1.0 });

    // Authoritative final food
    let agent1 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(1))
        .unwrap();
    let agent2 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(2))
        .unwrap();
    let victim = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(7))
        .unwrap();

    assert_eq!(agent1.food, 6.0); // 2.0 + 4.0
    assert_eq!(agent2.food, 3.0); // 2.0 + 1.0
    assert_eq!(victim.food, 0.0); // 5.0 - 4.0 - 1.0
}

// 20. Sequential GiveFood target invalidation
#[test]
fn test_sequential_give_food_target_invalidation() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.world.master_seed = 0x0123456789abcdef;
    config.world.replicate_id = 7;
    config.interaction.starvation_threshold = 5.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
        make_test_agent(7, 0, 3.0, 1.0, true), // 3.0 < 5.0 -> eligible initially
    ]);

    // GiveFood keys:
    // Agent 1 -> 7, GiveFood = 3: 0xbb562e6fe6076d7f
    // Agent 2 -> 7, GiveFood = 3: compute_resolution_key(..., 7, 2, 3)
    let key1 = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 1, 3);
    let key2 = compute_resolution_key(0x0123456789abcdef, 7, 0, 0, 7, 2, 3);

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::GiveFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 3.0,
            },
            Intent::GiveFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 3.0,
            },
        ],
    }];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    let keyed = &res[0].keyed_stream;
    assert_eq!(keyed.len(), 2);

    let first_idx = if key1 < key2 { 0 } else { 1 };
    let second_idx = 1 - first_idx;

    assert_eq!(
        keyed[first_idx].outcome,
        TargetedOutcome::Applied { amount: 3.0 }
    );
    // Recipient food became 3.0 + 3.0 = 6.0 >= starvation_threshold (5.0)
    assert_eq!(keyed[second_idx].outcome, TargetedOutcome::TargetIneligible);

    let recipient = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(7))
        .unwrap();
    assert_eq!(recipient.food, 6.0);
}

// 21. Initiator live invalidation
#[test]
fn test_initiator_live_invalidation() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // Initiator dead
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, false),
        make_test_agent(2, 0, 5.0, 1.0, true),
    ]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 2.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::InitiatorIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Initiator health <= 0
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 0.0, true),
        make_test_agent(2, 0, 5.0, 1.0, true),
    ]);
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::InitiatorIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);

    // Initiator wrong group
    let mut world = make_test_world(vec![
        make_test_agent(1, 1, 10.0, 1.0, true),
        make_test_agent(2, 0, 5.0, 1.0, true),
    ]);
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::StealFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 2.0,
        }],
    }];
    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(
        res[0].keyed_stream[0].outcome,
        TargetedOutcome::InitiatorIneligible
    );
    assert_eq!(res[0].keyed_stream[0].theft_success_draw, None);
}

// 22. Multi-settlement isolation
#[test]
fn test_multi_settlement_isolation() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 1.0;

    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 2.0, 1.0, true),
        make_test_agent(2, 0, 8.0, 1.0, true),
        make_test_agent(3, 1, 10.0, 1.0, true),
        make_test_agent(4, 1, 2.0, 1.0, true),
    ]);

    let partitions = vec![
        SettlementIntentPartition {
            group_id: GroupId(0),
            intents: vec![Intent::StealFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(2)),
                requested_amount: 3.0,
            }],
        },
        SettlementIntentPartition {
            group_id: GroupId(1),
            intents: vec![Intent::GiveFood {
                agent_id: AgentId(3),
                group_id: GroupId(1),
                target_agent_id: Some(AgentId(4)),
                requested_amount: 4.0,
            }],
        },
    ];

    let res = phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].group_id, GroupId(0));
    assert_eq!(res[1].group_id, GroupId(1));

    // Settlement 0
    let a1 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(1))
        .unwrap();
    let a2 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(2))
        .unwrap();
    assert_eq!(a1.food, 5.0);
    assert_eq!(a2.food, 5.0);

    // Settlement 1
    let a3 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(3))
        .unwrap();
    let a4 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(4))
        .unwrap();
    assert_eq!(a3.food, 6.0);
    assert_eq!(a4.food, 6.0);
}

// 23. Input-order independence
#[test]
fn test_input_order_independence() {
    let mut config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    config.interaction.theft_success_probability = 1.0;

    let a1 = make_test_agent(1, 0, 2.0, 1.0, true);
    let a2 = make_test_agent(2, 0, 8.0, 1.0, true);
    let a3 = make_test_agent(3, 1, 10.0, 1.0, true);
    let a4 = make_test_agent(4, 1, 2.0, 1.0, true);

    let mut world_a = make_test_world(vec![a1.clone(), a2.clone(), a3.clone(), a4.clone()]);
    let mut world_b = make_test_world(vec![a4, a3, a2, a1]); // shuffled agents

    let part0 = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::StealFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(2)),
                requested_amount: 3.0,
            },
            Intent::GiveFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: None,
                requested_amount: 1.0,
            },
        ],
    };

    let part1 = SettlementIntentPartition {
        group_id: GroupId(1),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(3),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(4)),
            requested_amount: 4.0,
        }],
    };

    let partitions_a = vec![part0.clone(), part1.clone()];
    // Shuffled partitions and intents inside partition
    let mut part0_shuffled = part0;
    part0_shuffled.intents.reverse();
    let partitions_b = vec![part1, part0_shuffled];

    let res_a = phase6b_targeted_resolution(&mut world_a, &config, &partitions_a).unwrap();
    let res_b = phase6b_targeted_resolution(&mut world_b, &config, &partitions_b).unwrap();

    assert_eq!(res_a, res_b);

    // Food states are identical
    for agent_a in &world_a.agents {
        let agent_b = world_b
            .agents
            .iter()
            .find(|a| a.agent_id == agent_a.agent_id)
            .unwrap();
        assert_eq!(agent_a.food, agent_b.food);
    }
}

// 24. Structural validation failure causes zero Phase 6B mutation
#[test]
fn test_structural_validation_failure_zero_partial_mutation() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 10.0, 1.0, true),
    ]);
    let initial_world = world.clone();

    // 1. Missing settlement
    let bad_part = vec![SettlementIntentPartition {
        group_id: GroupId(99),
        intents: vec![],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &config, &bad_part),
        Err(Phase6BError::MissingSettlement(GroupId(99)))
    );
    assert_eq!(world, initial_world);

    // 2. Missing initiator
    let bad_part = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(999),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 1.0,
        }],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &config, &bad_part),
        Err(Phase6BError::MissingInitiator(AgentId(999)))
    );
    assert_eq!(world, initial_world);

    // 3. Missing target
    let bad_part = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(999)),
            requested_amount: 1.0,
        }],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &config, &bad_part),
        Err(Phase6BError::MissingTarget(AgentId(999)))
    );
    assert_eq!(world, initial_world);

    // 4. Partition group mismatch
    let bad_part = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 1.0,
        }],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &config, &bad_part),
        Err(Phase6BError::PartitionGroupMismatch {
            agent_id: AgentId(1),
            intent_group_id: GroupId(1),
            partition_group_id: GroupId(0),
        })
    );
    assert_eq!(world, initial_world);

    // 5. Invalid requested amount (negative or NaN)
    let bad_part = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: -5.0,
        }],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &config, &bad_part),
        Err(Phase6BError::InvalidRequestedAmount {
            agent_id: AgentId(1),
            requested_amount: -5.0,
        })
    );
    assert_eq!(world, initial_world);

    // 6. Invalid probability
    let mut bad_config = config.clone();
    bad_config.interaction.theft_success_probability = 1.5;
    let normal_part = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![],
    }];
    assert_eq!(
        phase6b_targeted_resolution(&mut world, &bad_config, &normal_part),
        Err(Phase6BError::InvalidTheftSuccessProbability(1.5))
    );
    assert_eq!(world, initial_world);
}

// 25. Deterministic replay
#[test]
fn test_deterministic_replay() {
    let config = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world1 = make_test_world(vec![
        make_test_agent(1, 0, 5.0, 1.0, true),
        make_test_agent(2, 0, 8.0, 1.0, true),
        make_test_agent(7, 0, 3.0, 1.0, true),
    ]);
    let mut world2 = world1.clone();

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::StealFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 2.0,
            },
            Intent::GiveFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(7)),
                requested_amount: 3.0,
            },
        ],
    }];

    let res1 = phase6b_targeted_resolution(&mut world1, &config, &partitions).unwrap();
    let res2 = phase6b_targeted_resolution(&mut world2, &config, &partitions).unwrap();

    assert_eq!(res1, res2);
    assert_eq!(world1, world2);
}

// 26. Integration: Phase 1 -> Phase 6B
#[test]
fn test_integration_phases_1_to_6b() {
    let toml = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 6
settlement_count = 1
initial_health = 1.0
initial_food = 10.0
initial_wealth = 10000
initial_settlement_resource = 100.0
initial_treasury = 5000

[traits]
prod_min = 1.0
prod_max = 1.0
coop_min = 0.5
coop_max = 0.5
aggr_min = 0.5
aggr_max = 0.5
risk_min = 0.5
risk_max = 0.5

[environment]
carrying_capacity = 200.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.25

[economy]
base_work_yield = 5.0
food_price = 100
target_food = 20.0
target_reserve = 10000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 4.0
theft_amount = 3.0
theft_success_probability = 1.0
starvation_threshold = 12.0

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
trait_weight_cooperation = 0.0
trait_weight_aggression = 0.0
trait_weight_risk_tolerance = 0.0
"#;

    let config = SimConfig::parse_and_validate(toml).unwrap();
    let mut world = initialize_world(&config).unwrap();

    let initial_treasury = world.settlements[0].treasury;
    let initial_day = world.current_day;

    // Phase 1 and 2
    execute_phases_1_and_2(&mut world, &config);

    // Phase 3
    let agent_features = phase3_observation_and_features(&world, &config).unwrap();

    // Phase 4 action selection
    let choices = phase4_primary_action_selection(&world, &config, &agent_features).unwrap();

    // Phase 4 intent generation
    let intents = generate_intents(&world, &config, &choices).unwrap();

    // Phase 5 partitioning
    let partitions = phase5_partition_intents(&intents).unwrap();

    // Phase 6A Work resolution
    let work_resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();
    assert_eq!(work_resolutions.len(), 1);

    // Save food state post-Phase 6A
    let post_work_food: Vec<(AgentId, f32)> =
        world.agents.iter().map(|a| (a.agent_id, a.food)).collect();

    // Phase 6B Targeted Interaction resolution
    let targeted_resolutions =
        phase6b_targeted_resolution(&mut world, &config, &partitions).unwrap();
    assert_eq!(targeted_resolutions.len(), 1);

    // Verify invariants:
    // 1. Treasury unchanged
    assert_eq!(world.settlements[0].treasury, initial_treasury);
    // 2. Day unchanged
    assert_eq!(world.current_day, initial_day);

    // 3. Any applied transfers conserved total food across the settlement
    let total_food_before: f32 = post_work_food.iter().map(|(_, f)| *f).sum();
    let total_food_after: f32 = world.agents.iter().map(|a| a.food).sum();
    assert!((total_food_before - total_food_after).abs() < 1e-4);
}

// 27. Command unit test
#[test]
fn test_command_modify_food_execution() {
    let mut world = make_test_world(vec![
        make_test_agent(1, 0, 10.0, 1.0, true),
        make_test_agent(2, 0, 5.0, 1.0, true),
    ]);

    // Valid transfer
    let cmd = Command::ModifyFood {
        from: AgentId(1),
        to: AgentId(2),
        amount: 3.0,
    };
    cmd.execute(&mut world).unwrap();
    assert_eq!(world.agents[0].food, 7.0);
    assert_eq!(world.agents[1].food, 8.0);

    // Insufficient food
    let cmd_excess = Command::ModifyFood {
        from: AgentId(1),
        to: AgentId(2),
        amount: 100.0,
    };
    assert_eq!(
        cmd_excess.execute(&mut world),
        Err(CommandExecutionError::InsufficientSourceFood {
            source: AgentId(1),
            food: 7.0,
            amount: 100.0,
        })
    );

    // Self transfer
    let cmd_self = Command::ModifyFood {
        from: AgentId(1),
        to: AgentId(1),
        amount: 1.0,
    };
    assert_eq!(
        cmd_self.execute(&mut world),
        Err(CommandExecutionError::SelfTransfer(AgentId(1)))
    );

    // Invalid amount
    let cmd_nan = Command::ModifyFood {
        from: AgentId(1),
        to: AgentId(2),
        amount: f32::NAN,
    };
    assert!(matches!(
        cmd_nan.execute(&mut world),
        Err(CommandExecutionError::InvalidAmount(_))
    ));
}
