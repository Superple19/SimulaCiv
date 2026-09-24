use sim_core::{
    AgentId, DenseSlot, GroupId, RngCoordinate, SimulationDay, coordinate_prng_f32,
    coordinate_prng_u64,
};
use sim_model::{
    Action, AgentFeatures, AgentState, DecisionError, FeatureVector, PrimaryActionChoice,
    SettlementState, SimConfig, Subsystem, WorldState, evaluate_utilities, execute_phases_1_and_2,
    initialize_world, phase3_observation_and_features, phase4_primary_action_selection,
    select_action, stable_softmax,
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
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 10000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 4.0
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
fn test_canonical_action_order() {
    assert_eq!(Action::Work.index(), 0);
    assert_eq!(Action::BuyFood.index(), 1);
    assert_eq!(Action::SellFood.index(), 2);
    assert_eq!(Action::GiveFood.index(), 3);
    assert_eq!(Action::StealFood.index(), 4);
    assert_eq!(Action::Idle.index(), 5);

    assert_eq!(Action::from_index(0), Some(Action::Work));
    assert_eq!(Action::from_index(1), Some(Action::BuyFood));
    assert_eq!(Action::from_index(2), Some(Action::SellFood));
    assert_eq!(Action::from_index(3), Some(Action::GiveFood));
    assert_eq!(Action::from_index(4), Some(Action::StealFood));
    assert_eq!(Action::from_index(5), Some(Action::Idle));
    assert_eq!(Action::from_index(6), None);

    assert_eq!(
        Action::ALL,
        [
            Action::Work,
            Action::BuyFood,
            Action::SellFood,
            Action::GiveFood,
            Action::StealFood,
            Action::Idle,
        ]
    );

    assert_eq!(Action::COUNT, 6);

    for (i, &action) in Action::ALL.iter().enumerate() {
        assert_eq!(action.index(), i);
        assert_eq!(Action::try_from(i).unwrap(), action);
    }
    assert!(Action::try_from(6).is_err());
}

#[test]
fn test_exact_utility_golden() {
    let mut cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    // §6 Configuration
    cfg.decision.action_biases = [0.0, 1.0, -1.0, 0.5, -0.5, 2.0];
    cfg.decision.base_weight_matrix = [
        [1.0, 0.0, 0.0, 0.0, 0.0], // Work
        [0.0, 2.0, 0.0, 0.0, 0.0], // BuyFood
        [0.0, 0.0, 1.0, 0.0, 0.0], // SellFood
        [0.0, 0.0, 0.0, 0.0, 1.0], // GiveFood
        [1.0, 1.0, 0.0, 0.0, 0.0], // StealFood
        [0.0, 0.0, 0.0, 0.0, 0.0], // Idle
    ];
    cfg.decision.trait_weight_cooperation = 0.5;
    cfg.decision.trait_weight_aggression = 0.5;
    cfg.decision.trait_weight_risk_tolerance = 0.5;

    let agent = AgentState {
        agent_id: AgentId(42),
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
    };

    let features = FeatureVector::new([0.5, 0.25, 0.75, 0.0, 1.0]);

    let utilities = evaluate_utilities(&agent, &features, &cfg);

    // Expected final utilities in canonical order:
    // [0.5, 1.5, -0.25, 1.75, 0.75, 2.0]
    assert_eq!(utilities[0], 0.5);
    assert_eq!(utilities[1], 1.5);
    assert_eq!(utilities[2], -0.25);
    assert_eq!(utilities[3], 1.75);
    assert_eq!(utilities[4], 0.75);
    assert_eq!(utilities[5], 2.0);

    // Exact f32::to_bits() equality required by §6:
    assert_eq!(utilities[0].to_bits(), 0x3f000000); // 0.5
    assert_eq!(utilities[1].to_bits(), 0x3fc00000); // 1.5
    assert_eq!(utilities[2].to_bits(), 0xbe800000); // -0.25
    assert_eq!(utilities[3].to_bits(), 0x3fe00000); // 1.75
    assert_eq!(utilities[4].to_bits(), 0x3f400000); // 0.75
    assert_eq!(utilities[5].to_bits(), 0x40000000); // 2.0
}

#[test]
fn test_trait_modifier_isolation() {
    let mut cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    cfg.decision.action_biases = [0.0; 6];
    cfg.decision.base_weight_matrix = [[0.0; 5]; 6];
    cfg.decision.trait_weight_cooperation = 1.0;
    cfg.decision.trait_weight_aggression = 1.0;
    cfg.decision.trait_weight_risk_tolerance = 1.0;

    let base_agent = AgentState {
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
    };

    let features = FeatureVector::new([0.0; 5]);
    let base_utils = evaluate_utilities(&base_agent, &features, &cfg);

    // 1. Changing cooperation affects ONLY GiveFood (index 3)
    let mut coop_agent = base_agent.clone();
    coop_agent.cooperation = 0.9;
    let coop_utils = evaluate_utilities(&coop_agent, &features, &cfg);
    for m in 0..6 {
        if m == Action::GiveFood.index() {
            assert_ne!(coop_utils[m], base_utils[m]);
            assert_eq!(coop_utils[m], 0.9);
        } else {
            assert_eq!(coop_utils[m], base_utils[m]);
        }
    }

    // 2. Changing aggression affects ONLY StealFood (index 4)
    let mut aggr_agent = base_agent.clone();
    aggr_agent.aggression = 0.9;
    let aggr_utils = evaluate_utilities(&aggr_agent, &features, &cfg);
    for m in 0..6 {
        if m == Action::StealFood.index() {
            assert_ne!(aggr_utils[m], base_utils[m]);
            assert_eq!(aggr_utils[m], 0.9 + 0.5); // aggression(0.9) + risk(0.5)
        } else {
            assert_eq!(aggr_utils[m], base_utils[m]);
        }
    }

    // 3. Changing risk_tolerance affects ONLY StealFood (index 4)
    let mut risk_agent = base_agent.clone();
    risk_agent.risk_tolerance = 0.9;
    let risk_utils = evaluate_utilities(&risk_agent, &features, &cfg);
    for m in 0..6 {
        if m == Action::StealFood.index() {
            assert_ne!(risk_utils[m], base_utils[m]);
            assert_eq!(risk_utils[m], 0.5 + 0.9); // aggression(0.5) + risk(0.9)
        } else {
            assert_eq!(risk_utils[m], base_utils[m]);
        }
    }

    // 4. Changing productivity affects NO utilities
    let mut prod_agent = base_agent.clone();
    prod_agent.productivity = 2.5;
    let prod_utils = evaluate_utilities(&prod_agent, &features, &cfg);
    assert_eq!(prod_utils, base_utils);
}

#[test]
fn test_stable_softmax_uniform_golden() {
    let utilities = [0.0f32; 6];
    let temperature = 1.0f32;

    let probabilities = stable_softmax(&utilities, temperature);

    // Each probability is exactly 1.0 / 6.0
    for &prob in &probabilities {
        assert_eq!(prob.to_bits(), 0x3e2aaaab);
    }

    // Sequential cumulative thresholds check per §8
    let mut cumulative = 0.0f32;
    let expected_threshold_bits = [
        0x3e2aaaab, // C0: 0.16666667
        0x3eaaaaab, // C1: 0.33333334
        0x3f000000, // C2: 0.5
        0x3f2aaaab, // C3: 0.6666667
        0x3f555556, // C4: 0.8333334
        0x3f800000, // C5: 1.0
    ];

    for (m, &expected_bits) in expected_threshold_bits.iter().enumerate() {
        let next = cumulative + probabilities[m];
        assert_eq!(
            next.to_bits(),
            expected_bits,
            "threshold C{} bit mismatch: actual 0x{:08x} vs expected 0x{:08x}",
            m,
            next.to_bits(),
            expected_bits
        );
        cumulative = next;
    }
}

#[test]
fn test_stable_softmax_large_offset_numerical_stability() {
    // Large positive offset should not overflow into Inf / NaN due to max subtraction
    let large_utilities = [1000.0, 999.0, 998.0, 997.0, 996.0, 995.0];
    let probs = stable_softmax(&large_utilities, 1.0);

    for &p in &probs {
        assert!(p.is_finite(), "probability must be finite");
        assert!((0.0..=1.0).contains(&p), "probability out of range: {}", p);
    }
    let sum: f32 = probs.iter().sum();
    assert!((sum - 1.0).abs() < 1e-6);

    // Large negative offset
    let neg_utilities = [-1000.0, -999.0, -998.0, -997.0, -996.0, -995.0];
    let neg_probs = stable_softmax(&neg_utilities, 1.0);
    for &p in &neg_probs {
        assert!(p.is_finite(), "probability must be finite");
        assert!((0.0..=1.0).contains(&p), "probability out of range: {}", p);
    }
    let neg_sum: f32 = neg_probs.iter().sum();
    assert!((neg_sum - 1.0).abs() < 1e-6);
}

#[test]
fn test_prng_action_selection_golden_fixture() {
    // §11 External action-selection golden fixture
    // MasterSeed  = 0x0123456789abcdef
    // ReplicateId = 7
    // Day         = 0
    // Phase       = 4
    // SubsystemId = 1
    // DrawIndex   = 0

    let master_seed = 0x0123_4567_89ab_cdef_u64;
    let replicate_id = 7u32;
    let day = 0u32;
    let phase = 4u8;
    let subsystem_id = Subsystem::Decision.id(); // 1
    let draw_index = 0u32;

    let uniform_probs = [1.0f32 / 6.0f32; 6];

    // AgentId 0:
    let coord0 = RngCoordinate::new(
        master_seed,
        replicate_id,
        day,
        phase,
        subsystem_id,
        0,
        draw_index,
    );
    let u64_0 = coordinate_prng_u64(&coord0);
    assert_eq!(u64_0, 0x63864e0d23722b3a);
    let f32_0 = coordinate_prng_f32(&coord0);
    assert_eq!(f32_0.to_bits(), 0x3ec70c9c);
    let action0 = select_action(&uniform_probs, f32_0);
    assert_eq!(action0, Action::SellFood);

    // AgentId 4:
    let coord4 = RngCoordinate::new(
        master_seed,
        replicate_id,
        day,
        phase,
        subsystem_id,
        4,
        draw_index,
    );
    let u64_4 = coordinate_prng_u64(&coord4);
    assert_eq!(u64_4, 0x064eca6ad6affeee);
    let f32_4 = coordinate_prng_f32(&coord4);
    assert_eq!(f32_4.to_bits(), 0x3cc9d940);
    let action4 = select_action(&uniform_probs, f32_4);
    assert_eq!(action4, Action::Work);

    // AgentId 6:
    let coord6 = RngCoordinate::new(
        master_seed,
        replicate_id,
        day,
        phase,
        subsystem_id,
        6,
        draw_index,
    );
    let u64_6 = coordinate_prng_u64(&coord6);
    assert_eq!(u64_6, 0x861d787694ec5447);
    let f32_6 = coordinate_prng_f32(&coord6);
    assert_eq!(f32_6.to_bits(), 0x3f061d78);
    let action6 = select_action(&uniform_probs, f32_6);
    assert_eq!(action6, Action::GiveFood);

    // Now test through phase4_primary_action_selection
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    assert_eq!(cfg.world.master_seed, master_seed);
    assert_eq!(cfg.world.replicate_id, replicate_id);

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            AgentState {
                agent_id: AgentId(0),
                dense_slot: DenseSlot(0),
                alive: true,
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 10.0,
                wealth: 1000,
                productivity: 1.0,
                cooperation: 0.0,
                aggression: 0.0,
                risk_tolerance: 0.0,
                group_id: GroupId(0),
            },
            AgentState {
                agent_id: AgentId(4),
                dense_slot: DenseSlot(1),
                alive: true,
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 10.0,
                wealth: 1000,
                productivity: 1.0,
                cooperation: 0.0,
                aggression: 0.0,
                risk_tolerance: 0.0,
                group_id: GroupId(0),
            },
            AgentState {
                agent_id: AgentId(6),
                dense_slot: DenseSlot(2),
                alive: true,
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 10.0,
                wealth: 1000,
                productivity: 1.0,
                cooperation: 0.0,
                aggression: 0.0,
                risk_tolerance: 0.0,
                group_id: GroupId(0),
            },
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 4000,
    };

    let features = vec![
        AgentFeatures {
            agent_id: AgentId(0),
            features: FeatureVector::new([0.0; 5]),
        },
        AgentFeatures {
            agent_id: AgentId(4),
            features: FeatureVector::new([0.0; 5]),
        },
        AgentFeatures {
            agent_id: AgentId(6),
            features: FeatureVector::new([0.0; 5]),
        },
    ];

    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    assert_eq!(choices.len(), 3);
    assert_eq!(
        choices[0],
        PrimaryActionChoice {
            agent_id: AgentId(0),
            action: Action::SellFood,
        }
    );
    assert_eq!(
        choices[1],
        PrimaryActionChoice {
            agent_id: AgentId(4),
            action: Action::Work,
        }
    );
    assert_eq!(
        choices[2],
        PrimaryActionChoice {
            agent_id: AgentId(6),
            action: Action::GiveFood,
        }
    );
}

#[test]
fn test_replicate_isolation() {
    let cfg7 = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut cfg8 = cfg7.clone();
    cfg8.world.replicate_id = 8;

    let agent = AgentState {
        agent_id: AgentId(0),
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
    };

    let features = FeatureVector::new([0.5, 0.25, 0.75, 0.0, 1.0]);

    // Utilities and softmax probabilities are completely independent of ReplicateId
    let u7 = evaluate_utilities(&agent, &features, &cfg7);
    let u8 = evaluate_utilities(&agent, &features, &cfg8);
    assert_eq!(u7, u8);

    let p7 = stable_softmax(&u7, cfg7.decision.decision_temperature);
    let p8 = stable_softmax(&u8, cfg8.decision.decision_temperature);
    assert_eq!(p7, p8);

    // But PRNG draw may differ between replicates
    let coord7 = RngCoordinate::new(
        cfg7.world.master_seed,
        cfg7.world.replicate_id,
        0,
        4,
        Subsystem::Decision.id(),
        agent.agent_id.0,
        0,
    );
    let coord8 = RngCoordinate::new(
        cfg8.world.master_seed,
        cfg8.world.replicate_id,
        0,
        4,
        Subsystem::Decision.id(),
        agent.agent_id.0,
        0,
    );
    assert_ne!(
        coordinate_prng_u64(&coord7),
        coordinate_prng_u64(&coord8),
        "different replicates should produce different coordinate draws"
    );
}

#[test]
fn test_eligibility_consistency() {
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
            AgentState {
                agent_id: AgentId(3),
                dense_slot: DenseSlot(2),
                alive: true,
                birth_day: SimulationDay(0),
                health: 0.0, // zero health
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
        initial_money_supply: 4000,
    };

    // 1. Missing AgentId fails explicitly
    let missing_features = vec![AgentFeatures {
        agent_id: AgentId(99),
        features: FeatureVector::new([0.0; 5]),
    }];
    let err = phase4_primary_action_selection(&world, &cfg, &missing_features).unwrap_err();
    assert_eq!(err, DecisionError::MissingAgent(AgentId(99)));

    // 2. Dead agent fails explicitly
    let dead_features = vec![AgentFeatures {
        agent_id: AgentId(2),
        features: FeatureVector::new([0.0; 5]),
    }];
    let err = phase4_primary_action_selection(&world, &cfg, &dead_features).unwrap_err();
    assert_eq!(err, DecisionError::IneligibleAgent(AgentId(2)));

    // 3. Zero health agent fails explicitly
    let zero_health_features = vec![AgentFeatures {
        agent_id: AgentId(3),
        features: FeatureVector::new([0.0; 5]),
    }];
    let err = phase4_primary_action_selection(&world, &cfg, &zero_health_features).unwrap_err();
    assert_eq!(err, DecisionError::IneligibleAgent(AgentId(3)));
}

#[test]
fn test_canonical_ordering_independent_of_input_order() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32| AgentState {
        agent_id: AgentId(id),
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
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![make_agent(9), make_agent(2), make_agent(5)],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 4000,
    };

    // Features passed in non-ascending order: 9, 2, 5
    let features = vec![
        AgentFeatures {
            agent_id: AgentId(9),
            features: FeatureVector::new([0.0; 5]),
        },
        AgentFeatures {
            agent_id: AgentId(2),
            features: FeatureVector::new([0.0; 5]),
        },
        AgentFeatures {
            agent_id: AgentId(5),
            features: FeatureVector::new([0.0; 5]),
        },
    ];

    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    assert_eq!(choices.len(), 3);
    assert_eq!(choices[0].agent_id, AgentId(2));
    assert_eq!(choices[1].agent_id, AgentId(5));
    assert_eq!(choices[2].agent_id, AgentId(9));
}

#[test]
fn test_observer_independence_read_only() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();
    let cloned_world = world.clone();

    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    let _choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();

    // WorldState must be completely unmodified
    assert_eq!(world, cloned_world);
    assert_eq!(world.current_day, SimulationDay(0));
}

#[test]
fn test_deterministic_replay() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();
    let features = phase3_observation_and_features(&world, &cfg).unwrap();

    let run1 = phase4_primary_action_selection(&world, &cfg, &features).unwrap();
    let run2 = phase4_primary_action_selection(&world, &cfg, &features).unwrap();

    assert_eq!(run1, run2);
}

#[test]
fn test_integration_phases_1_to_4() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let initial_day = world.current_day;
    let agent_count = world.agents.len();

    // Phase 1 + 2
    execute_phases_1_and_2(&mut world, &cfg);

    // Phase 3
    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), agent_count);

    let world_before_phase4 = world.clone();

    // Phase 4
    let choices = phase4_primary_action_selection(&world, &cfg, &features).unwrap();

    // 1. Exactly one primary action choice per eligible agent
    assert_eq!(choices.len(), agent_count);

    // 2. Ascending AgentId
    for i in 1..choices.len() {
        assert!(choices[i - 1].agent_id < choices[i].agent_id);
    }

    // 3. No authoritative state mutation occurred
    assert_eq!(world, world_before_phase4);
    assert_eq!(world.current_day, initial_day);
}
