use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::{
    AgentState, Phase3Error, SettlementState, SimConfig, WorldState, execute_phases_1_and_2,
    initialize_world, phase3_observation_and_features,
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
fn test_external_feature_golden_fixture() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let agent = AgentState {
        agent_id: AgentId(42),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 0.8,
        food: 4.0,
        wealth: 2500,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let settlement = SettlementState {
        group_id: GroupId(0),
        resource: 25.0,
        treasury: 1000,
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![agent],
        settlements: vec![settlement],
        initial_money_supply: 3500,
    };

    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), 1);
    assert_eq!(features[0].agent_id, AgentId(42));

    let f = features[0].features.values;
    assert_eq!(f[0], 0.6);
    assert_eq!(f[1], 0.75);
    assert!((f[2] - 0.2).abs() < 1e-6);
    assert_eq!(f[3], 0.75);
    assert_eq!(f[4], 0.0);

    // Exact bit equality per specification
    assert_eq!(f[0].to_bits(), 0x3f19999a);
    assert_eq!(f[1].to_bits(), 0x3f400000);
    assert_eq!(f[2].to_bits(), 0x3e4ccccc);
    assert_eq!(f[3].to_bits(), 0x3f400000);
    assert_eq!(f[4].to_bits(), 0x00000000);
}

#[test]
fn test_additional_external_boundary_fixture() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let agent = AgentState {
        agent_id: AgentId(7),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 0.25,
        food: 40.0,
        wealth: 20000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let settlement = SettlementState {
        group_id: GroupId(0),
        resource: 100.0,
        treasury: 1000,
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![agent],
        settlements: vec![settlement],
        initial_money_supply: 21000,
    };

    let features = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(features.len(), 1);

    let f = features[0].features.values;
    assert_eq!(f[0], 0.0);
    assert_eq!(f[1], 0.0);
    assert_eq!(f[2], 0.75);
    assert_eq!(f[3], 0.0);
    assert_eq!(f[4], 1.0);

    let expected_bits = [0x00000000, 0x00000000, 0x3f400000, 0x00000000, 0x3f800000];
    let actual_bits: Vec<u32> = f.iter().map(|v| v.to_bits()).collect();
    assert_eq!(actual_bits, expected_bits);
}

#[test]
fn test_clamp_boundaries_all_features() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let test_cases = [
        // (food, wealth, health, resource)
        (0.0, 0, 1.0, 0.0),
        (100.0, 50000, 0.01, 100.0),
        (10.0, 10000, 0.5, 50.0),
        (0.0, 10000, 1.0, 100.0),
    ];

    for (food, wealth, health, resource) in test_cases {
        let world = WorldState {
            current_day: SimulationDay(0),
            agents: vec![AgentState {
                agent_id: AgentId(1),
                dense_slot: DenseSlot(0),
                alive: true,
                birth_day: SimulationDay(0),
                health,
                food,
                wealth,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.5,
                risk_tolerance: 0.5,
                group_id: GroupId(0),
            }],
            settlements: vec![SettlementState {
                group_id: GroupId(0),
                resource,
                treasury: 1000,
            }],
            initial_money_supply: wealth + 1000,
        };

        let feats = phase3_observation_and_features(&world, &cfg).unwrap();
        assert_eq!(feats.len(), 1);
        for &val in &feats[0].features.values {
            assert!((0.0..=1.0).contains(&val), "value {} out of bounds", val);
        }
    }
}

#[test]
fn test_local_settlement_observation() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            AgentState {
                agent_id: AgentId(1),
                dense_slot: DenseSlot(0),
                alive: true,
                birth_day: SimulationDay(0),
                health: 0.8,
                food: 10.0,
                wealth: 5000,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.5,
                risk_tolerance: 0.5,
                group_id: GroupId(0),
            },
            AgentState {
                agent_id: AgentId(2),
                dense_slot: DenseSlot(1),
                alive: true,
                birth_day: SimulationDay(0),
                health: 0.8,
                food: 10.0,
                wealth: 5000,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.5,
                risk_tolerance: 0.5,
                group_id: GroupId(1),
            },
        ],
        settlements: vec![
            SettlementState {
                group_id: GroupId(0),
                resource: 20.0, // scarcity = 1.0 - 0.2 = 0.8
                treasury: 1000,
            },
            SettlementState {
                group_id: GroupId(1),
                resource: 80.0, // scarcity = 1.0 - 0.8 = 0.2
                treasury: 1000,
            },
        ],
        initial_money_supply: 12000,
    };

    let feats = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(feats.len(), 2);

    let scarcity_agent1 = feats[0].features.local_scarcity();
    let scarcity_agent2 = feats[1].features.local_scarcity();

    assert_eq!(scarcity_agent1, 0.8);
    assert_eq!(scarcity_agent2.to_bits(), 0x3e4ccccc);
    assert!((scarcity_agent2 - 0.2).abs() < 1e-6);
    assert_ne!(scarcity_agent1, scarcity_agent2);
}

#[test]
fn test_eligibility_filtering() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32, alive: bool, health: f32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive,
        birth_day: SimulationDay(0),
        health,
        food: 10.0,
        wealth: 5000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![
            make_agent(1, true, 0.8),  // eligible
            make_agent(2, true, 0.0),  // ineligible: health == 0
            make_agent(3, false, 0.8), // ineligible: dead
            make_agent(4, false, 0.0), // ineligible: dead and health == 0
            make_agent(5, true, 0.5),  // eligible
        ],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 26000,
    };

    let feats = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(feats.len(), 2);
    assert_eq!(feats[0].agent_id, AgentId(1));
    assert_eq!(feats[1].agent_id, AgentId(5));
}

#[test]
fn test_canonical_ordering_independent_of_storage() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let make_agent = |id: u32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive: true,
        birth_day: SimulationDay(0),
        health: 0.8,
        food: 10.0,
        wealth: 5000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    // Stored out of AgentId order: 5, 2, 8, 1
    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![make_agent(5), make_agent(2), make_agent(8), make_agent(1)],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 21000,
    };

    let world_clone = world.clone();
    let feats = phase3_observation_and_features(&world, &cfg).unwrap();

    // Output order must be strictly ascending AgentId: 1, 2, 5, 8
    let output_ids: Vec<u32> = feats.iter().map(|af| af.agent_id.as_u32()).collect();
    assert_eq!(output_ids, vec![1, 2, 5, 8]);

    // Authoritative WorldState must remain unmodified
    assert_eq!(world, world_clone);
}

#[test]
fn test_observer_independence_read_only() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();
    let world_before = world.clone();

    let _feats = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(world, world_before);
}

#[test]
fn test_deterministic_replay() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let world = initialize_world(&cfg).unwrap();

    let feats1 = phase3_observation_and_features(&world, &cfg).unwrap();
    let feats2 = phase3_observation_and_features(&world, &cfg).unwrap();

    assert_eq!(feats1, feats2);
    for (f1, f2) in feats1.iter().zip(feats2.iter()) {
        assert_eq!(f1.agent_id, f2.agent_id);
        for (&v1, &v2) in f1.features.values.iter().zip(f2.features.values.iter()) {
            assert_eq!(v1.to_bits(), v2.to_bits());
        }
    }
}

#[test]
fn test_missing_settlement_fails_explicitly() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![AgentState {
            agent_id: AgentId(1),
            dense_slot: DenseSlot(0),
            alive: true,
            birth_day: SimulationDay(0),
            health: 0.8,
            food: 10.0,
            wealth: 5000,
            productivity: 1.0,
            cooperation: 0.5,
            aggression: 0.5,
            risk_tolerance: 0.5,
            group_id: GroupId(99), // non-existent
        }],
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 50.0,
            treasury: 1000,
        }],
        initial_money_supply: 6000,
    };

    let err = phase3_observation_and_features(&world, &cfg).unwrap_err();
    assert_eq!(err, Phase3Error::MissingSettlement(GroupId(99)));
}

#[test]
fn test_integration_phases_1_2_3() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    // Initial state check
    assert_eq!(world.settlements[0].resource, 25.0);
    assert_eq!(world.agents[0].food, 10.0);

    // Execute Phase 1 -> Phase 2
    execute_phases_1_and_2(&mut world, &cfg);

    // Phase 1 mutated resource: 25.0 -> 26.875
    assert_eq!(world.settlements[0].resource, 26.875);
    // Phase 2 mutated food: 10.0 - 2.0 = 8.0
    assert_eq!(world.agents[0].food, 8.0);

    let world_before_phase3 = world.clone();

    // Execute Phase 3
    let feats = phase3_observation_and_features(&world, &cfg).unwrap();
    assert_eq!(feats.len(), world.agents.len());

    // Phase 3 observed post-Phase-1 resource (local_scarcity uses 26.875)
    let expected_scarcity = 1.0 - (26.875f32 / 100.0f32).clamp(0.0, 1.0);
    assert_eq!(feats[0].features.local_scarcity(), expected_scarcity);

    // Phase 3 observed post-Phase-2 food (hunger_ratio uses 8.0)
    let expected_hunger = (1.0 - 8.0f32 / 10.0f32).clamp(0.0, 1.0);
    assert_eq!(feats[0].features.hunger_ratio(), expected_hunger);

    // Phase 3 performed zero mutation
    assert_eq!(world, world_before_phase3);
    assert_eq!(world.current_day, SimulationDay(0));
}
