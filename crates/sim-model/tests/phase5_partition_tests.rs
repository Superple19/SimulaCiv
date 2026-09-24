use sim_core::{AgentId, GroupId};
use sim_model::{
    Intent, Phase5Error, SimConfig, execute_phases_1_and_2, generate_intents, initialize_world,
    phase3_observation_and_features, phase4_primary_action_selection, phase5_partition_intents,
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
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

#[test]
fn test_canonical_partition_fixture_section_14() {
    // §14 External canonical partition fixture in deliberately shuffled input order
    let intents = vec![
        Intent::StealFood {
            agent_id: AgentId(8),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 6.0,
        },
        Intent::Work {
            agent_id: AgentId(5),
            group_id: GroupId(0),
            requested_harvest: 3.0,
        },
        Intent::GiveFood {
            agent_id: AgentId(3),
            group_id: GroupId(1),
            target_agent_id: None,
            requested_amount: 4.0,
        },
        Intent::BuyFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_demand: 12.5,
        },
        Intent::SellFood {
            agent_id: AgentId(9),
            group_id: GroupId(2),
            submitted_supply: 7.5,
        },
        Intent::Idle {
            agent_id: AgentId(4),
            group_id: GroupId(1),
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();

    // Expected partition sequence:
    // Partition 0: group_id = 0, AgentIds = [1, 5]
    // Partition 1: group_id = 1, AgentIds = [3, 4, 8]
    // Partition 2: group_id = 2, AgentIds = [9]
    assert_eq!(partitions.len(), 3);

    // Partition 0 (GroupId 0)
    assert_eq!(partitions[0].group_id, GroupId(0));
    assert_eq!(partitions[0].intents.len(), 2);
    assert_eq!(partitions[0].intents[0].agent_id(), AgentId(1));
    assert_eq!(partitions[0].intents[1].agent_id(), AgentId(5));
    assert_eq!(
        partitions[0].intents[0],
        Intent::BuyFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_demand: 12.5,
        }
    );
    assert_eq!(
        partitions[0].intents[1],
        Intent::Work {
            agent_id: AgentId(5),
            group_id: GroupId(0),
            requested_harvest: 3.0,
        }
    );

    // Partition 1 (GroupId 1)
    assert_eq!(partitions[1].group_id, GroupId(1));
    assert_eq!(partitions[1].intents.len(), 3);
    assert_eq!(partitions[1].intents[0].agent_id(), AgentId(3));
    assert_eq!(partitions[1].intents[1].agent_id(), AgentId(4));
    assert_eq!(partitions[1].intents[2].agent_id(), AgentId(8));
    assert_eq!(
        partitions[1].intents[0],
        Intent::GiveFood {
            agent_id: AgentId(3),
            group_id: GroupId(1),
            target_agent_id: None,
            requested_amount: 4.0,
        }
    );
    assert_eq!(
        partitions[1].intents[1],
        Intent::Idle {
            agent_id: AgentId(4),
            group_id: GroupId(1),
        }
    );
    assert_eq!(
        partitions[1].intents[2],
        Intent::StealFood {
            agent_id: AgentId(8),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(2)),
            requested_amount: 6.0,
        }
    );

    // Partition 2 (GroupId 2)
    assert_eq!(partitions[2].group_id, GroupId(2));
    assert_eq!(partitions[2].intents.len(), 1);
    assert_eq!(partitions[2].intents[0].agent_id(), AgentId(9));
    assert_eq!(
        partitions[2].intents[0],
        Intent::SellFood {
            agent_id: AgentId(9),
            group_id: GroupId(2),
            submitted_supply: 7.5,
        }
    );
}

#[test]
fn test_input_order_independence() {
    let i1 = Intent::Work {
        agent_id: AgentId(5),
        group_id: GroupId(0),
        requested_harvest: 3.0,
    };
    let i2 = Intent::BuyFood {
        agent_id: AgentId(1),
        group_id: GroupId(0),
        requested_demand: 12.5,
    };
    let i3 = Intent::GiveFood {
        agent_id: AgentId(3),
        group_id: GroupId(1),
        target_agent_id: None,
        requested_amount: 4.0,
    };
    let i4 = Intent::StealFood {
        agent_id: AgentId(8),
        group_id: GroupId(1),
        target_agent_id: Some(AgentId(2)),
        requested_amount: 6.0,
    };

    let order1 = vec![i1.clone(), i2.clone(), i3.clone(), i4.clone()];
    let order2 = vec![i4.clone(), i3.clone(), i2.clone(), i1.clone()];
    let order3 = vec![i2.clone(), i4.clone(), i1.clone(), i3.clone()];

    let p1 = phase5_partition_intents(&order1).unwrap();
    let p2 = phase5_partition_intents(&order2).unwrap();
    let p3 = phase5_partition_intents(&order3).unwrap();

    assert_eq!(p1, p2);
    assert_eq!(p2, p3);
}

#[test]
fn test_partition_key_is_group_id_only_section_15() {
    // §15 Same target does not create separate partitions
    let intents = vec![
        Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(7)),
            requested_amount: 4.0,
        },
        Intent::StealFood {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(7)),
            requested_amount: 6.0,
        },
        Intent::StealFood {
            agent_id: AgentId(3),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(9)),
            requested_amount: 6.0,
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();

    // Exactly ONE partition: GroupId(0) with AgentIds [1, 2, 3]
    assert_eq!(partitions.len(), 1);
    assert_eq!(partitions[0].group_id, GroupId(0));
    assert_eq!(
        partitions[0]
            .intents
            .iter()
            .map(|i| i.agent_id())
            .collect::<Vec<_>>(),
        vec![AgentId(1), AgentId(2), AgentId(3)]
    );
}

#[test]
fn test_all_six_variants_preserved() {
    let intents = vec![
        Intent::Work {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_harvest: 3.0,
        },
        Intent::BuyFood {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            requested_demand: 12.5,
        },
        Intent::SellFood {
            agent_id: AgentId(3),
            group_id: GroupId(0),
            submitted_supply: 7.5,
        },
        Intent::GiveFood {
            agent_id: AgentId(4),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(5)),
            requested_amount: 4.0,
        },
        Intent::StealFood {
            agent_id: AgentId(5),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(4)),
            requested_amount: 6.0,
        },
        Intent::Idle {
            agent_id: AgentId(6),
            group_id: GroupId(0),
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();
    assert_eq!(partitions.len(), 1);
    assert_eq!(partitions[0].intents.len(), 6);

    assert!(matches!(partitions[0].intents[0], Intent::Work { .. }));
    assert!(matches!(partitions[0].intents[1], Intent::BuyFood { .. }));
    assert!(matches!(partitions[0].intents[2], Intent::SellFood { .. }));
    assert!(matches!(partitions[0].intents[3], Intent::GiveFood { .. }));
    assert!(matches!(partitions[0].intents[4], Intent::StealFood { .. }));
    assert!(matches!(partitions[0].intents[5], Intent::Idle { .. }));
}

#[test]
fn test_target_none_preservation() {
    let intents = vec![
        Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(2),
            target_agent_id: None,
            requested_amount: 4.0,
        },
        Intent::StealFood {
            agent_id: AgentId(2),
            group_id: GroupId(2),
            target_agent_id: None,
            requested_amount: 6.0,
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();
    assert_eq!(partitions.len(), 1);
    assert_eq!(partitions[0].group_id, GroupId(2));
    assert_eq!(partitions[0].intents.len(), 2);

    match &partitions[0].intents[0] {
        Intent::GiveFood {
            target_agent_id, ..
        } => assert_eq!(*target_agent_id, None),
        _ => panic!("expected GiveFood"),
    }

    match &partitions[0].intents[1] {
        Intent::StealFood {
            target_agent_id, ..
        } => assert_eq!(*target_agent_id, None),
        _ => panic!("expected StealFood"),
    }
}

#[test]
fn test_payload_bit_exact_preservation() {
    let work_harvest = 3.875f32;
    let buy_demand = 12.345678f32;
    let sell_supply = 7.891011f32;
    let give_amount = 4.56789f32;
    let steal_amount = 6.12345f32;

    let intents = vec![
        Intent::Work {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_harvest: work_harvest,
        },
        Intent::BuyFood {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            requested_demand: buy_demand,
        },
        Intent::SellFood {
            agent_id: AgentId(3),
            group_id: GroupId(0),
            submitted_supply: sell_supply,
        },
        Intent::GiveFood {
            agent_id: AgentId(4),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(1)),
            requested_amount: give_amount,
        },
        Intent::StealFood {
            agent_id: AgentId(5),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(2)),
            requested_amount: steal_amount,
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();
    let res = &partitions[0].intents;

    match res[0] {
        Intent::Work {
            requested_harvest, ..
        } => assert_eq!(requested_harvest.to_bits(), work_harvest.to_bits()),
        _ => panic!(),
    }
    match res[1] {
        Intent::BuyFood {
            requested_demand, ..
        } => assert_eq!(requested_demand.to_bits(), buy_demand.to_bits()),
        _ => panic!(),
    }
    match res[2] {
        Intent::SellFood {
            submitted_supply, ..
        } => assert_eq!(submitted_supply.to_bits(), sell_supply.to_bits()),
        _ => panic!(),
    }
    match res[3] {
        Intent::GiveFood {
            requested_amount, ..
        } => assert_eq!(requested_amount.to_bits(), give_amount.to_bits()),
        _ => panic!(),
    }
    match res[4] {
        Intent::StealFood {
            requested_amount, ..
        } => assert_eq!(requested_amount.to_bits(), steal_amount.to_bits()),
        _ => panic!(),
    }
}

#[test]
fn test_cardinality_invariant() {
    let intents = vec![
        Intent::Idle {
            agent_id: AgentId(10),
            group_id: GroupId(1),
        },
        Intent::Idle {
            agent_id: AgentId(20),
            group_id: GroupId(0),
        },
        Intent::Idle {
            agent_id: AgentId(30),
            group_id: GroupId(1),
        },
        Intent::Idle {
            agent_id: AgentId(40),
            group_id: GroupId(2),
        },
    ];

    let partitions = phase5_partition_intents(&intents).unwrap();
    let total_out: usize = partitions.iter().map(|p| p.intents.len()).sum();
    assert_eq!(total_out, intents.len());
}

#[test]
fn test_duplicate_initiator_fails_explicitly() {
    let intents = vec![
        Intent::Idle {
            agent_id: AgentId(42),
            group_id: GroupId(0),
        },
        Intent::Work {
            agent_id: AgentId(42), // duplicate!
            group_id: GroupId(0),
            requested_harvest: 2.0,
        },
    ];

    let err = phase5_partition_intents(&intents).unwrap_err();
    assert_eq!(err, Phase5Error::DuplicateInitiator(AgentId(42)));
}

#[test]
fn test_empty_input() {
    let empty: Vec<Intent> = vec![];
    let partitions = phase5_partition_intents(&empty).unwrap();
    assert!(partitions.is_empty());
}

#[test]
fn test_no_state_dependency_pure_function() {
    // Pure function: operates directly on &[Intent], requires no WorldState reference or PRNG calls
    let intents = vec![Intent::Idle {
        agent_id: AgentId(1),
        group_id: GroupId(0),
    }];
    let partitions = phase5_partition_intents(&intents).unwrap();
    assert_eq!(partitions.len(), 1);
}

#[test]
fn test_deterministic_replay() {
    let intents = vec![
        Intent::Work {
            agent_id: AgentId(7),
            group_id: GroupId(2),
            requested_harvest: 1.0,
        },
        Intent::Idle {
            agent_id: AgentId(3),
            group_id: GroupId(0),
        },
        Intent::BuyFood {
            agent_id: AgentId(1),
            group_id: GroupId(2),
            requested_demand: 5.0,
        },
    ];

    let run1 = phase5_partition_intents(&intents).unwrap();
    let run2 = phase5_partition_intents(&intents).unwrap();
    assert_eq!(run1, run2);
}

#[test]
fn test_integration_phases_1_to_5() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let initial_day = world.current_day;
    let initial_agent_count = world.agents.len();

    // Phase 1 + 2
    execute_phases_1_and_2(&mut world, &cfg);

    // Phase 3
    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), initial_agent_count);

    let world_before_decisions = world.clone();

    // Phase 4: Action Selection
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    assert_eq!(choices.len(), initial_agent_count);

    // Phase 4: Intent Generation
    let intents = generate_intents(&world, &cfg, &choices).unwrap();
    assert_eq!(intents.len(), initial_agent_count);

    // Phase 5: Locality Partitioning
    let partitions = phase5_partition_intents(&intents).unwrap();

    // 1. Total Intent cardinality unchanged
    let total_partition_intents: usize = partitions.iter().map(|p| p.intents.len()).sum();
    assert_eq!(total_partition_intents, initial_agent_count);

    // 2. Partitions are in strictly ascending GroupId order
    for i in 1..partitions.len() {
        assert!(partitions[i - 1].group_id < partitions[i].group_id);
    }

    // 3. Intents within each partition are in strictly ascending initiator AgentId order
    for partition in &partitions {
        for i in 1..partition.intents.len() {
            assert!(partition.intents[i - 1].agent_id() < partition.intents[i].agent_id());
        }
        // All intents in partition belong to partition.group_id
        for intent in &partition.intents {
            assert_eq!(intent.group_id(), partition.group_id);
        }
    }

    // 4. WorldState remains unchanged
    assert_eq!(world, world_before_decisions);
    assert_eq!(world.current_day, initial_day);
}
