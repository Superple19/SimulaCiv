use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::{
    AgentState, Intent, Phase6AError, SettlementIntentPartition, SettlementState, SimConfig,
    WorldState, execute_phases_1_and_2, generate_intents, initialize_world,
    phase3_observation_and_features, phase4_primary_action_selection, phase5_partition_intents,
    phase6a_work_resolution,
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

fn make_test_agent(id: u32, gid: u16, food: f32, alive: bool, health: f32) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
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
        group_id: GroupId(gid),
    }
}

#[test]
fn test_sufficient_resource_golden_section_16() {
    // §16 External sufficient-resource golden
    // Settlement GroupId(0): resource = 10.0
    // AgentId(1): food = 4.0, request = 2.0
    // AgentId(3): food = 7.0, request = 6.0
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 4.0, true, 1.0),
            make_test_agent(3, 0, 7.0, true, 1.0),
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 10.0,
            treasury: 1000,
        }],
        initial_money_supply: 3000,
    };

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 2.0,
            },
            Intent::Work {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                requested_harvest: 6.0,
            },
        ],
    }];

    let resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();
    assert_eq!(resolutions.len(), 1);
    let r = &resolutions[0];
    assert_eq!(r.group_id, GroupId(0));
    assert_eq!(r.resource_before, 10.0);
    assert_eq!(r.total_requested, 8.0);
    assert_eq!(r.total_allocated, 8.0);
    assert_eq!(r.resource_after, 2.0);

    // Expected allocations
    assert_eq!(r.allocations[0].allocated_harvest, 2.0);
    assert_eq!(r.allocations[1].allocated_harvest, 6.0);

    // Expected final worker food
    let a1 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(1))
        .unwrap();
    assert_eq!(a1.food, 6.0);
    assert_eq!(a1.food.to_bits(), 0x40c00000);

    let a3 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(3))
        .unwrap();
    assert_eq!(a3.food, 13.0);
    assert_eq!(a3.food.to_bits(), 0x41500000);

    // Expected final resource
    let s0 = world
        .settlements
        .iter()
        .find(|s| s.group_id == GroupId(0))
        .unwrap();
    assert_eq!(s0.resource, 2.0);
    assert_eq!(s0.resource.to_bits(), 0x40000000);
}

#[test]
fn test_insufficient_resource_golden_section_11_and_17() {
    // §11 & §17 External insufficient-resource golden
    // Settlement GroupId(0): resource = 5.0
    // AgentId(1): food = 4.0, request = 2.0
    // AgentId(3): food = 7.0, request = 6.0
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 4.0, true, 1.0),
            make_test_agent(3, 0, 7.0, true, 1.0),
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 5.0,
            treasury: 1000,
        }],
        initial_money_supply: 3000,
    };

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 2.0,
            },
            Intent::Work {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                requested_harvest: 6.0,
            },
        ],
    }];

    let resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();
    assert_eq!(resolutions.len(), 1);
    let r = &resolutions[0];
    assert_eq!(r.group_id, GroupId(0));
    assert_eq!(r.resource_before, 5.0);
    assert_eq!(r.total_requested, 8.0);
    assert_eq!(r.total_allocated, 5.0);
    assert_eq!(r.total_allocated.to_bits(), 0x40a00000);
    assert_eq!(r.resource_after, 0.0);
    assert_eq!(r.resource_after.to_bits(), 0x00000000);

    // Expected allocations
    assert_eq!(r.allocations[0].allocated_harvest, 1.25);
    assert_eq!(r.allocations[0].allocated_harvest.to_bits(), 0x3fa00000);
    assert_eq!(r.allocations[1].allocated_harvest, 3.75);
    assert_eq!(r.allocations[1].allocated_harvest.to_bits(), 0x40700000);

    // Expected final worker food
    let a1 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(1))
        .unwrap();
    assert_eq!(a1.food, 5.25);
    assert_eq!(a1.food.to_bits(), 0x40a80000);

    let a3 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(3))
        .unwrap();
    assert_eq!(a3.food, 10.75);
    assert_eq!(a3.food.to_bits(), 0x412c0000);

    // Expected final settlement resource
    let s0 = world
        .settlements
        .iter()
        .find(|s| s.group_id == GroupId(0))
        .unwrap();
    assert_eq!(s0.resource, 0.0);
    assert_eq!(s0.resource.to_bits(), 0x00000000);
}

#[test]
fn test_zero_work_intents() {
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![make_test_agent(1, 0, 4.0, true, 1.0)],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 10.0,
            treasury: 1000,
        }],
        initial_money_supply: 2000,
    };
    let initial_world = world.clone();

    // Partition with no Work intents
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Idle {
            agent_id: AgentId(1),
            group_id: GroupId(0),
        }],
    }];

    let resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();
    assert_eq!(resolutions.len(), 1);
    assert_eq!(resolutions[0].allocations.len(), 0);
    assert_eq!(resolutions[0].total_allocated, 0.0);
    assert_eq!(world, initial_world);
}

#[test]
fn test_all_zero_work_requests() {
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![make_test_agent(1, 0, 4.0, true, 1.0)],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 10.0,
            treasury: 1000,
        }],
        initial_money_supply: 2000,
    };
    let initial_world = world.clone();

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_harvest: 0.0,
        }],
    }];

    let resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();
    assert_eq!(resolutions.len(), 1);
    assert_eq!(resolutions[0].allocations[0].allocated_harvest, 0.0);
    assert_eq!(resolutions[0].total_allocated, 0.0);
    assert_eq!(world, initial_world);
}

#[test]
fn test_multi_settlement_isolation_section_18() {
    // §18 Multi-settlement isolation fixture
    // Group 0 resource = 5.0, worker 1 request = 5.0 -> gets 5.0, res = 0.0
    // Group 1 resource = 20.0, worker 2 request = 5.0 -> gets 5.0, res = 15.0
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 0.0, true, 1.0),
            make_test_agent(2, 1, 0.0, true, 1.0),
        ],
        settlements: vec![
            SettlementState {
                group_id: GroupId(0),
                resource: 5.0,
                treasury: 1000,
            },
            SettlementState {
                group_id: GroupId(1),
                resource: 20.0,
                treasury: 1000,
            },
        ],
        initial_money_supply: 4000,
    };

    let partitions = vec![
        SettlementIntentPartition {
            group_id: GroupId(0),
            intents: vec![Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 5.0,
            }],
        },
        SettlementIntentPartition {
            group_id: GroupId(1),
            intents: vec![Intent::Work {
                agent_id: AgentId(2),
                group_id: GroupId(1),
                requested_harvest: 5.0,
            }],
        },
    ];

    let _resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();

    let s0 = world
        .settlements
        .iter()
        .find(|s| s.group_id == GroupId(0))
        .unwrap();
    let s1 = world
        .settlements
        .iter()
        .find(|s| s.group_id == GroupId(1))
        .unwrap();
    assert_eq!(s0.resource, 0.0);
    assert_eq!(s1.resource, 15.0);

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
}

#[test]
fn test_non_work_intents_ignored_by_phase_6a_section_19() {
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 10.0, true, 1.0),
            make_test_agent(2, 0, 10.0, true, 1.0),
            make_test_agent(3, 0, 10.0, true, 1.0),
            make_test_agent(4, 0, 10.0, true, 1.0),
            make_test_agent(5, 0, 10.0, true, 1.0),
            make_test_agent(6, 0, 10.0, true, 1.0),
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 7000,
    };

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 5.0,
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
                target_agent_id: Some(AgentId(2)),
                requested_amount: 4.0,
            },
            Intent::StealFood {
                agent_id: AgentId(5),
                group_id: GroupId(0),
                target_agent_id: Some(AgentId(3)),
                requested_amount: 6.0,
            },
            Intent::Idle {
                agent_id: AgentId(6),
                group_id: GroupId(0),
            },
        ],
    }];

    let _resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();

    // Only Agent 1 got food added
    for agent in &world.agents {
        if agent.agent_id == AgentId(1) {
            assert_eq!(agent.food, 15.0);
        } else {
            assert_eq!(
                agent.food, 10.0,
                "agent {} must be unmutated",
                agent.agent_id
            );
        }
        assert_eq!(agent.wealth, 1000);
        assert_eq!(agent.health, 1.0);
    }
    // Treasury unchanged
    assert_eq!(world.settlements[0].treasury, 1000);
    assert_eq!(world.settlements[0].resource, 45.0);
    assert_eq!(world.current_day, SimulationDay(0));
}

#[test]
fn test_canonical_agent_id_evaluation_ordering() {
    let mut world1 = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(5, 0, 0.0, true, 1.0),
            make_test_agent(2, 0, 0.0, true, 1.0),
            make_test_agent(8, 0, 0.0, true, 1.0),
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 5.0,
            treasury: 1000,
        }],
        initial_money_supply: 4000,
    };
    let mut world2 = world1.clone();

    // Order A: 8, 2, 5
    let partitions_a = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(8),
                group_id: GroupId(0),
                requested_harvest: 3.0,
            },
            Intent::Work {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                requested_harvest: 1.0,
            },
            Intent::Work {
                agent_id: AgentId(5),
                group_id: GroupId(0),
                requested_harvest: 2.0,
            },
        ],
    }];

    // Order B: 2, 5, 8
    let partitions_b = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                requested_harvest: 1.0,
            },
            Intent::Work {
                agent_id: AgentId(5),
                group_id: GroupId(0),
                requested_harvest: 2.0,
            },
            Intent::Work {
                agent_id: AgentId(8),
                group_id: GroupId(0),
                requested_harvest: 3.0,
            },
        ],
    }];

    let res_a = phase6a_work_resolution(&mut world1, &partitions_a).unwrap();
    let res_b = phase6a_work_resolution(&mut world2, &partitions_b).unwrap();

    assert_eq!(res_a, res_b);
    assert_eq!(world1, world2);
}

#[test]
fn test_explicit_validation_errors() {
    let make_base_world = || WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 10.0, true, 1.0),
            make_test_agent(2, 0, 10.0, false, 1.0), // dead
            make_test_agent(3, 1, 10.0, true, 1.0),  // group 1
        ],
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
        initial_money_supply: 5000,
    };

    // 1. Missing settlement
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(99),
        intents: vec![],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::MissingSettlement(GroupId(99))
    );

    // 2. Missing worker
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(99),
            group_id: GroupId(0),
            requested_harvest: 2.0,
        }],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::MissingAgent(AgentId(99))
    );

    // 3. Group mismatch (agent belongs to group 1, intent says group 0)
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(3),
            group_id: GroupId(0),
            requested_harvest: 2.0,
        }],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::GroupMismatch {
            agent_id: AgentId(3),
            agent_group_id: GroupId(1),
            intent_group_id: GroupId(0),
        }
    );

    // 4. Partition group mismatch
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(3),
            group_id: GroupId(1),
            requested_harvest: 2.0,
        }],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::PartitionGroupMismatch {
            agent_id: AgentId(3),
            intent_group_id: GroupId(1),
            partition_group_id: GroupId(0),
        }
    );

    // 5. Ineligible worker (dead)
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            requested_harvest: 2.0,
        }],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::IneligibleWorker(AgentId(2))
    );

    // 6. Negative requested harvest
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_harvest: -1.0,
        }],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::InvalidRequestedHarvest {
            agent_id: AgentId(1),
            requested_harvest: -1.0,
        }
    );

    // 7. Non-finite requested harvest
    let mut w = make_base_world();
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::Work {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_harvest: f32::NAN,
        }],
    }];
    assert!(matches!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::InvalidRequestedHarvest { .. }
    ));

    // 8. Negative settlement resource
    let mut w = make_base_world();
    w.settlements[0].resource = -5.0;
    let p = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![],
    }];
    assert_eq!(
        phase6a_work_resolution(&mut w, &p).unwrap_err(),
        Phase6AError::InvalidSettlementResource {
            group_id: GroupId(0),
            resource: -5.0,
        }
    );
}

#[test]
fn test_atomic_commit_zero_partial_mutation_on_failure() {
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 10.0, true, 1.0),
            make_test_agent(2, 1, 10.0, true, 1.0),
        ],
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
        initial_money_supply: 4000,
    };
    let initial_world = world.clone();

    // Partition 0 is valid, but Partition 1 contains an invalid requested_harvest
    let partitions = vec![
        SettlementIntentPartition {
            group_id: GroupId(0),
            intents: vec![Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 5.0,
            }],
        },
        SettlementIntentPartition {
            group_id: GroupId(1),
            intents: vec![Intent::Work {
                agent_id: AgentId(2),
                group_id: GroupId(1),
                requested_harvest: -2.0, // Error in stage A!
            }],
        },
    ];

    let err = phase6a_work_resolution(&mut world, &partitions).unwrap_err();
    assert_eq!(
        err,
        Phase6AError::InvalidRequestedHarvest {
            agent_id: AgentId(2),
            requested_harvest: -2.0,
        }
    );

    // World must be COMPLETELY unmutated (zero partial commit to Partition 0)
    assert_eq!(world, initial_world);
}

#[test]
fn test_deterministic_replay() {
    let mut world1 = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_test_agent(1, 0, 5.0, true, 1.0),
            make_test_agent(2, 0, 5.0, true, 1.0),
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 7.0,
            treasury: 1000,
        }],
        initial_money_supply: 3000,
    };
    let mut world2 = world1.clone();

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::Work {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_harvest: 4.0,
            },
            Intent::Work {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                requested_harvest: 6.0,
            },
        ],
    }];

    let r1 = phase6a_work_resolution(&mut world1, &partitions).unwrap();
    let r2 = phase6a_work_resolution(&mut world2, &partitions).unwrap();

    assert_eq!(r1, r2);
    assert_eq!(world1, world2);
}

#[test]
fn test_integration_phases_1_to_6a() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let initial_day = world.current_day;
    let initial_agent_count = world.agents.len();

    // Phase 1 + 2
    execute_phases_1_and_2(&mut world, &cfg);

    let res_after_p1 = world.settlements[0].resource;
    let food_after_p2: Vec<f32> = world.agents.iter().map(|a| a.food).collect();

    // Phase 3
    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), initial_agent_count);

    // Phase 4
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    let intents = generate_intents(&world, &cfg, &choices).unwrap();

    // Phase 5
    let partitions = phase5_partition_intents(&intents).unwrap();

    // Phase 6A: Work resolution
    let resolutions = phase6a_work_resolution(&mut world, &partitions).unwrap();

    // Check invariants
    let mut total_allocated_all = 0.0f32;
    for res in &resolutions {
        assert_eq!(res.resource_before, res_after_p1);
        assert_eq!(res.resource_after, res_after_p1 - res.total_allocated);
        total_allocated_all += res.total_allocated;
    }
    assert!(total_allocated_all >= 0.0);

    // Workers with Work intent had their food increased by allocated_harvest
    for partition in &partitions {
        for intent in &partition.intents {
            if let Intent::Work {
                agent_id,
                requested_harvest,
                ..
            } = *intent
            {
                let agent = world
                    .agents
                    .iter()
                    .find(|a| a.agent_id == agent_id)
                    .unwrap();
                let prev_food = food_after_p2[agent_id.0 as usize];
                assert!(agent.food >= prev_food);
                // requested_harvest in Intent was resolved, not recomputed
                let alloc = resolutions
                    .iter()
                    .flat_map(|r| &r.allocations)
                    .find(|a| a.agent_id == agent_id)
                    .unwrap();
                assert_eq!(alloc.requested_harvest, requested_harvest);
            }
        }
    }

    // Treasury, Day, Wealth unchanged
    assert_eq!(world.settlements[0].treasury, 2500);
    assert_eq!(world.current_day, initial_day);
}
