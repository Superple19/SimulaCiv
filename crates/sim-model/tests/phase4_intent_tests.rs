use sim_core::{
    AgentId, DenseSlot, GroupId, RngCoordinate, SimulationDay, coordinate_prng_f32,
    coordinate_prng_u64,
};
use sim_model::{
    Action, AgentState, Intent, IntentError, PrimaryActionChoice, SettlementState, SimConfig,
    Subsystem, WorldState, execute_phases_1_and_2, generate_intents, get_give_food_candidates,
    get_steal_food_candidates, initialize_world, phase3_observation_and_features,
    phase4_primary_action_selection,
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
fn test_intent_schema_and_action_mapping() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 10.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: (0..6).map(make_agent).collect(),
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 6000,
    };

    let choices = vec![
        PrimaryActionChoice {
            agent_id: AgentId(0),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::BuyFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(2),
            action: Action::SellFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(3),
            action: Action::GiveFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::StealFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(5),
            action: Action::Idle,
        },
    ];

    let intents = generate_intents(&world, &cfg, &choices).unwrap();
    assert_eq!(intents.len(), 6);

    assert!(matches!(intents[0], Intent::Work { .. }));
    assert!(matches!(intents[1], Intent::BuyFood { .. }));
    assert!(matches!(intents[2], Intent::SellFood { .. }));
    assert!(matches!(intents[3], Intent::GiveFood { .. }));
    assert!(matches!(intents[4], Intent::StealFood { .. }));
    assert!(matches!(intents[5], Intent::Idle { .. }));

    for (i, intent) in intents.iter().enumerate() {
        assert_eq!(intent.agent_id(), AgentId(i as u32));
        assert_eq!(intent.group_id(), GroupId(0));
    }
}

#[test]
fn test_quantity_goldens() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // 1. Work: base_work_yield = 4.0, productivity = 1.5, health = 0.5 -> harvest = 3.0 (0x40400000)
    let agent_work = AgentState {
        agent_id: AgentId(1),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 0.5,
        food: 10.0,
        wealth: 1000,
        productivity: 1.5,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // 2. BuyFood: target_food = 20.0, agent.food = 7.5 -> demand = 12.5 (0x41480000)
    let agent_buy = AgentState {
        agent_id: AgentId(2),
        dense_slot: DenseSlot(1),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 7.5,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // 3. SellFood: target_food = 20.0, agent.food = 27.5 -> supply = 7.5 (0x40f00000)
    let agent_sell = AgentState {
        agent_id: AgentId(3),
        dense_slot: DenseSlot(2),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 27.5,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // 4. GiveFood: gift_amount = 6.0, giver.food = 4.0 -> amount = 4.0 (0x40800000)
    let agent_give = AgentState {
        agent_id: AgentId(4),
        dense_slot: DenseSlot(3),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 4.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // 5. StealFood: theft_amount = 6.0 -> amount = 6.0 (0x40c00000)
    let agent_steal = AgentState {
        agent_id: AgentId(5),
        dense_slot: DenseSlot(4),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 2.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![agent_work, agent_buy, agent_sell, agent_give, agent_steal],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 5000,
    };

    let choices = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(2),
            action: Action::BuyFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(3),
            action: Action::SellFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::GiveFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(5),
            action: Action::StealFood,
        },
    ];

    let intents = generate_intents(&world, &cfg, &choices).unwrap();

    // Check Work
    match intents[0] {
        Intent::Work {
            requested_harvest, ..
        } => {
            assert_eq!(requested_harvest, 3.0);
            assert_eq!(requested_harvest.to_bits(), 0x40400000);
        }
        _ => panic!("expected Work intent"),
    }

    // Check BuyFood
    match intents[1] {
        Intent::BuyFood {
            requested_demand, ..
        } => {
            assert_eq!(requested_demand, 12.5);
            assert_eq!(requested_demand.to_bits(), 0x41480000);
        }
        _ => panic!("expected BuyFood intent"),
    }

    // Check SellFood
    match intents[2] {
        Intent::SellFood {
            submitted_supply, ..
        } => {
            assert_eq!(submitted_supply, 7.5);
            assert_eq!(submitted_supply.to_bits(), 0x40f00000);
        }
        _ => panic!("expected SellFood intent"),
    }

    // Check GiveFood
    match intents[3] {
        Intent::GiveFood {
            requested_amount, ..
        } => {
            assert_eq!(requested_amount, 4.0);
            assert_eq!(requested_amount.to_bits(), 0x40800000);
        }
        _ => panic!("expected GiveFood intent"),
    }

    // Check StealFood
    match intents[4] {
        Intent::StealFood {
            requested_amount, ..
        } => {
            assert_eq!(requested_amount, 6.0);
            assert_eq!(requested_amount.to_bits(), 0x40c00000);
        }
        _ => panic!("expected StealFood intent"),
    }
}

#[test]
fn test_external_target_selection_golden_fixture() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32, food: f32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // Construct actor (id 4) and candidates (1, 3, 7) physically in noncanonical order: [7, 4, 3, 1]
    let agents = vec![
        make_agent(7, 5.0),
        make_agent(4, 10.0),
        make_agent(3, 5.0),
        make_agent(1, 5.0),
    ];

    let world = WorldState {
        current_day: SimulationDay(0),
        agents,
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 4000,
    };

    // 1. Verify candidate lists are sorted strictly: [1, 3, 7]
    let actor = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(4))
        .unwrap();
    let steal_cands = get_steal_food_candidates(actor, &world);
    assert_eq!(steal_cands, vec![AgentId(1), AgentId(3), AgentId(7)]);
    let give_cands = get_give_food_candidates(actor, &world, &cfg);
    assert_eq!(give_cands, vec![AgentId(1), AgentId(3), AgentId(7)]);

    // 2. Direct PRNG coordinate checks per §18
    let master_seed = 0x0123_4567_89ab_cdef_u64;
    let replicate_id = 7u32;
    let day = 0u32;
    let phase = 4u8;

    // TheftTarget: SubsystemId = 2, AgentId = 4, DrawIndex = 1
    let coord_theft = RngCoordinate::new(
        master_seed,
        replicate_id,
        day,
        phase,
        Subsystem::TheftTarget.id(),
        4,
        1,
    );
    assert_eq!(coordinate_prng_u64(&coord_theft), 0x3500b3ade25e410a);
    let u_theft = coordinate_prng_f32(&coord_theft);
    assert_eq!(u_theft.to_bits(), 0x3e5402cc);
    assert_eq!((u_theft * 3.0).floor() as usize, 0);

    // MutualAidTarget: SubsystemId = 3, AgentId = 4, DrawIndex = 1
    let coord_give = RngCoordinate::new(
        master_seed,
        replicate_id,
        day,
        phase,
        Subsystem::MutualAidTarget.id(),
        4,
        1,
    );
    assert_eq!(coordinate_prng_u64(&coord_give), 0x83693b4259d57255);
    let u_give = coordinate_prng_f32(&coord_give);
    assert_eq!(u_give.to_bits(), 0x3f03693b);
    assert_eq!((u_give * 3.0).floor() as usize, 1);

    // 3. Test through generate_intents for StealFood
    let choices_steal = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(3),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::StealFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(7),
            action: Action::Idle,
        },
    ];

    let intents_steal = generate_intents(&world, &cfg, &choices_steal).unwrap();
    let actor_intent = intents_steal
        .iter()
        .find(|i| i.agent_id() == AgentId(4))
        .unwrap();
    match actor_intent {
        Intent::StealFood {
            target_agent_id,
            requested_amount,
            ..
        } => {
            assert_eq!(*target_agent_id, Some(AgentId(1)));
            assert_eq!(*requested_amount, 6.0);
        }
        _ => panic!("expected StealFood intent"),
    }

    // 4. Test through generate_intents for GiveFood
    let choices_give = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(3),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::GiveFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(7),
            action: Action::Idle,
        },
    ];

    let intents_give = generate_intents(&world, &cfg, &choices_give).unwrap();
    let actor_intent_give = intents_give
        .iter()
        .find(|i| i.agent_id() == AgentId(4))
        .unwrap();
    match actor_intent_give {
        Intent::GiveFood {
            target_agent_id,
            requested_amount,
            ..
        } => {
            assert_eq!(*target_agent_id, Some(AgentId(3)));
            assert_eq!(*requested_amount, 6.0);
        }
        _ => panic!("expected GiveFood intent"),
    }
}

#[test]
fn test_canonical_candidate_ordering_independence_from_storage() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 5.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let perm1 = vec![make_agent(7), make_agent(4), make_agent(3), make_agent(1)];
    let perm2 = vec![make_agent(1), make_agent(3), make_agent(4), make_agent(7)];
    let perm3 = vec![make_agent(3), make_agent(7), make_agent(1), make_agent(4)];

    let world1 = WorldState {
        current_day: SimulationDay(0),
        agents: perm1,
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 4000,
    };
    let world2 = WorldState {
        agents: perm2,
        ..world1.clone()
    };
    let world3 = WorldState {
        agents: perm3,
        ..world1.clone()
    };

    let choices = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(3),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::StealFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(7),
            action: Action::Idle,
        },
    ];

    let intents1 = generate_intents(&world1, &cfg, &choices).unwrap();
    let intents2 = generate_intents(&world2, &cfg, &choices).unwrap();
    let intents3 = generate_intents(&world3, &cfg, &choices).unwrap();

    assert_eq!(intents1, intents2);
    assert_eq!(intents2, intents3);
}

#[test]
fn test_candidate_filtering_steal_food() {
    let thief = AgentState {
        agent_id: AgentId(10),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 5.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let make_cand = |id: u32, gid: u16, alive: bool, health: f32, food: f32| AgentState {
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
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            thief.clone(),
            make_cand(1, 0, true, 1.0, 5.0),  // valid
            make_cand(2, 0, false, 1.0, 5.0), // dead -> excluded
            make_cand(3, 0, true, 0.0, 5.0),  // zero health -> excluded
            make_cand(4, 1, true, 1.0, 5.0),  // other group -> excluded
            make_cand(5, 0, true, 1.0, 0.0),  // zero food -> excluded
            make_cand(10, 0, true, 1.0, 5.0), // self -> excluded
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
        initial_money_supply: 7000,
    };

    let cands = get_steal_food_candidates(&thief, &world);
    assert_eq!(cands, vec![AgentId(1)]);
}

#[test]
fn test_candidate_filtering_give_food() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    // starvation_threshold = 10.0

    let giver = AgentState {
        agent_id: AgentId(10),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 20.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let make_cand = |id: u32, gid: u16, alive: bool, health: f32, food: f32| AgentState {
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
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            giver.clone(),
            make_cand(1, 0, true, 1.0, 5.0),  // valid (< 10.0)
            make_cand(2, 0, false, 1.0, 5.0), // dead -> excluded
            make_cand(3, 0, true, 0.0, 5.0),  // zero health -> excluded
            make_cand(4, 1, true, 1.0, 5.0),  // other group -> excluded
            make_cand(5, 0, true, 1.0, 10.0), // food == threshold -> excluded
            make_cand(6, 0, true, 1.0, 15.0), // food > threshold -> excluded
            make_cand(10, 0, true, 1.0, 5.0), // self -> excluded
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
        initial_money_supply: 8000,
    };

    let cands = get_give_food_candidates(&giver, &world, &cfg);
    assert_eq!(cands, vec![AgentId(1)]);
}

#[test]
fn test_no_target_and_no_fallback_invariants() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // Isolated single agent: has no candidates for Give or Steal
    let agent = AgentState {
        agent_id: AgentId(1),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 0.0, // zero food
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![agent],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 1000,
    };

    // 1. GiveFood with zero candidates must emit GiveFood with target None (NOT Idle!)
    let choices_give = vec![PrimaryActionChoice {
        agent_id: AgentId(1),
        action: Action::GiveFood,
    }];
    let intents_give = generate_intents(&world, &cfg, &choices_give).unwrap();
    match intents_give[0] {
        Intent::GiveFood {
            target_agent_id,
            requested_amount,
            ..
        } => {
            assert_eq!(target_agent_id, None);
            assert_eq!(requested_amount, 0.0); // min(6.0, 0.0)
        }
        _ => panic!("expected GiveFood intent, got {:?}", intents_give[0]),
    }

    // 2. StealFood with zero candidates must emit StealFood with target None (NOT Idle!)
    let choices_steal = vec![PrimaryActionChoice {
        agent_id: AgentId(1),
        action: Action::StealFood,
    }];
    let intents_steal = generate_intents(&world, &cfg, &choices_steal).unwrap();
    match intents_steal[0] {
        Intent::StealFood {
            target_agent_id,
            requested_amount,
            ..
        } => {
            assert_eq!(target_agent_id, None);
            assert_eq!(requested_amount, 6.0); // theft_amount
        }
        _ => panic!("expected StealFood intent, got {:?}", intents_steal[0]),
    }

    // 3. BuyFood with zero demand (e.g. food >= target_food) must retain BuyFood
    let mut rich_food_world = world.clone();
    rich_food_world.agents[0].food = 30.0;
    let choices_buy = vec![PrimaryActionChoice {
        agent_id: AgentId(1),
        action: Action::BuyFood,
    }];
    let intents_buy = generate_intents(&rich_food_world, &cfg, &choices_buy).unwrap();
    match intents_buy[0] {
        Intent::BuyFood {
            requested_demand, ..
        } => {
            assert_eq!(requested_demand, 0.0);
        }
        _ => panic!("expected BuyFood intent"),
    }

    // 4. SellFood with zero supply (food <= target_food) must retain SellFood
    let choices_sell = vec![PrimaryActionChoice {
        agent_id: AgentId(1),
        action: Action::SellFood,
    }];
    let intents_sell = generate_intents(&world, &cfg, &choices_sell).unwrap();
    match intents_sell[0] {
        Intent::SellFood {
            submitted_supply, ..
        } => {
            assert_eq!(submitted_supply, 0.0);
        }
        _ => panic!("expected SellFood intent"),
    }
}

#[test]
fn test_steal_request_not_clamped_against_victim_food() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let thief = AgentState {
        agent_id: AgentId(1),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 5.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let victim = AgentState {
        agent_id: AgentId(2),
        dense_slot: DenseSlot(1),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 2.0, // less than theft_amount (6.0)
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![thief, victim],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 2000,
    };

    let choices = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::StealFood,
        },
        PrimaryActionChoice {
            agent_id: AgentId(2),
            action: Action::Idle,
        },
    ];

    let intents = generate_intents(&world, &cfg, &choices).unwrap();
    match intents[0] {
        Intent::StealFood {
            requested_amount,
            target_agent_id,
            ..
        } => {
            assert_eq!(target_agent_id, Some(AgentId(2)));
            // Requested amount must be exactly theft_amount (6.0), NOT victim food (2.0)
            assert_eq!(requested_amount, 6.0);
        }
        _ => panic!("expected StealFood intent"),
    }
}

#[test]
fn test_choice_validation_failures() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            AgentState {
                agent_id: AgentId(1),
                dense_slot: DenseSlot(0),
                alive: true,
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 10.0,
                wealth: 1000,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.5,
                risk_tolerance: 0.5,
                group_id: GroupId(0),
            },
            AgentState {
                agent_id: AgentId(2),
                dense_slot: DenseSlot(1),
                alive: false, // dead
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 10.0,
                wealth: 1000,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.5,
                risk_tolerance: 0.5,
                group_id: GroupId(0),
            },
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 2000,
    };

    // 1. Missing agent referenced
    let choices_missing = vec![PrimaryActionChoice {
        agent_id: AgentId(99),
        action: Action::Idle,
    }];
    assert_eq!(
        generate_intents(&world, &cfg, &choices_missing).unwrap_err(),
        IntentError::MissingAgent(AgentId(99))
    );

    // 2. Ineligible agent has a choice
    let choices_ineligible = vec![PrimaryActionChoice {
        agent_id: AgentId(2),
        action: Action::Idle,
    }];
    assert_eq!(
        generate_intents(&world, &cfg, &choices_ineligible).unwrap_err(),
        IntentError::IneligibleAgent(AgentId(2))
    );

    // 3. Duplicate choices for same agent
    let choices_dup = vec![
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(1),
            action: Action::Idle,
        },
    ];
    assert_eq!(
        generate_intents(&world, &cfg, &choices_dup).unwrap_err(),
        IntentError::DuplicateChoice(AgentId(1))
    );

    // 4. Missing choice for eligible agent
    let choices_empty = vec![];
    assert_eq!(
        generate_intents(&world, &cfg, &choices_empty).unwrap_err(),
        IntentError::MissingChoiceForEligibleAgent(AgentId(1))
    );
}

#[test]
fn test_canonical_intent_ordering_independent_of_input_order() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 10.0,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![make_agent(9), make_agent(2), make_agent(5)],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 3000,
    };

    // Passed out of order: 9, 2, 5
    let choices = vec![
        PrimaryActionChoice {
            agent_id: AgentId(9),
            action: Action::Idle,
        },
        PrimaryActionChoice {
            agent_id: AgentId(2),
            action: Action::Work,
        },
        PrimaryActionChoice {
            agent_id: AgentId(5),
            action: Action::BuyFood,
        },
    ];

    let intents = generate_intents(&world, &cfg, &choices).unwrap();
    assert_eq!(intents.len(), 3);
    assert_eq!(intents[0].agent_id(), AgentId(2));
    assert_eq!(intents[1].agent_id(), AgentId(5));
    assert_eq!(intents[2].agent_id(), AgentId(9));
}

#[test]
fn test_observer_independence_read_only() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();
    let cloned_world = world.clone();

    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();

    let _intents = generate_intents(&world, &cfg, &choices).unwrap();

    // WorldState must be completely unmodified
    assert_eq!(world, cloned_world);
    assert_eq!(world.current_day, SimulationDay(0));
}

#[test]
fn test_deterministic_replay() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();
    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();

    let run1 = generate_intents(&world, &cfg, &choices).unwrap();
    let run2 = generate_intents(&world, &cfg, &choices).unwrap();

    assert_eq!(run1, run2);
}

#[test]
fn test_integration_phases_1_to_4_full() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let initial_day = world.current_day;
    let initial_agent_count = world.agents.len();

    // Phase 1 + 2
    execute_phases_1_and_2(&mut world, &cfg);

    // Phase 3
    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), initial_agent_count);

    let world_before_phase4 = world.clone();

    // Phase 4: Primary action selection
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    assert_eq!(choices.len(), initial_agent_count);

    // Phase 4: Intent generation
    let intents = generate_intents(&world, &cfg, &choices).unwrap();
    assert_eq!(intents.len(), initial_agent_count);

    // Check invariants
    for (choice, intent) in choices.iter().zip(intents.iter()) {
        assert_eq!(choice.agent_id, intent.agent_id());
        match choice.action {
            Action::Work => assert!(matches!(intent, Intent::Work { .. })),
            Action::BuyFood => assert!(matches!(intent, Intent::BuyFood { .. })),
            Action::SellFood => assert!(matches!(intent, Intent::SellFood { .. })),
            Action::GiveFood => assert!(matches!(intent, Intent::GiveFood { .. })),
            Action::StealFood => assert!(matches!(intent, Intent::StealFood { .. })),
            Action::Idle => assert!(matches!(intent, Intent::Idle { .. })),
        }
    }

    // World state remains unchanged throughout Phase 3 and 4
    assert_eq!(world, world_before_phase4);
    assert_eq!(world.current_day, initial_day);
}
