use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::{
    AgentState, SettlementState, SimConfig, WorldState, execute_phases_1_and_2, initialize_world,
    phase1_resource_regrowth, phase2_biological_degradation,
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
target_food = 10.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 4.0
theft_success_probability = 0.7
starvation_threshold = 5.0

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
fn test_phase1_external_golden_case_a() {
    // Case A — normal logistic regrowth
    // resource = 25.0, K = 100.0, r = 0.1 -> 26.875 (0x41d70000)
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: Vec::new(),
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 25.0,
            treasury: 1000,
        }],
        initial_money_supply: 1000,
    };

    phase1_resource_regrowth(&mut world, &cfg);

    assert_eq!(world.settlements[0].resource, 26.875);
    assert_eq!(world.settlements[0].resource.to_bits(), 0x41d70000);
}

#[test]
fn test_phase1_external_golden_case_b() {
    // Case B — zero resource remains 0.0
    let toml = BASE_TOML.replace("regrowth_rate = 0.1", "regrowth_rate = 0.5");
    let cfg = SimConfig::parse_and_validate(&toml).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: Vec::new(),
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 0.0,
            treasury: 1000,
        }],
        initial_money_supply: 1000,
    };

    phase1_resource_regrowth(&mut world, &cfg);

    assert_eq!(world.settlements[0].resource, 0.0);
}

#[test]
fn test_phase1_external_golden_case_c() {
    // Case C — carrying capacity remains K
    let toml = BASE_TOML.replace("regrowth_rate = 0.1", "regrowth_rate = 0.5");
    let cfg = SimConfig::parse_and_validate(&toml).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: Vec::new(),
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 100.0,
            treasury: 1000,
        }],
        initial_money_supply: 1000,
    };

    phase1_resource_regrowth(&mut world, &cfg);

    assert_eq!(world.settlements[0].resource, 100.0);
}

#[test]
fn test_phase1_external_golden_case_d() {
    // Case D — zero regrowth leaves resource unchanged
    let toml = BASE_TOML.replace("regrowth_rate = 0.1", "regrowth_rate = 0.0");
    let cfg = SimConfig::parse_and_validate(&toml).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: Vec::new(),
        settlements: vec![SettlementState {
            group_id: GroupId(0),
            resource: 37.5,
            treasury: 1000,
        }],
        initial_money_supply: 1000,
    };

    phase1_resource_regrowth(&mut world, &cfg);

    assert_eq!(world.settlements[0].resource, 37.5);
}

#[test]
fn test_phase1_non_resource_invariants() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let treasury_before = world.settlements[0].treasury;
    let agents_before = world.agents.clone();

    phase1_resource_regrowth(&mut world, &cfg);

    assert_eq!(world.settlements[0].treasury, treasury_before);
    assert_eq!(world.agents, agents_before);
    assert_eq!(world.current_day, SimulationDay(0));
}

#[test]
fn test_phase2_external_golden_case_a() {
    // Case A — enough food
    // food = 10.0, health = 0.8, metabolic = 2.0, decay = 0.25 -> food = 8.0, health = 0.8
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let agent = AgentState {
        agent_id: AgentId(0),
        dense_slot: DenseSlot(0),
        alive: true,
        birth_day: SimulationDay(0),
        health: 0.8,
        food: 10.0,
        wealth: 500,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(0),
    };

    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![agent.clone()],
        settlements: Vec::new(),
        initial_money_supply: 500,
    };

    phase2_biological_degradation(&mut world, &cfg);

    let updated = &world.agents[0];
    assert_eq!(updated.food, 8.0);
    assert_eq!(updated.health, 0.8);
    assert!(updated.alive);
    assert!(updated.is_behaviorally_eligible());

    // wealth and traits untouched
    assert_eq!(updated.wealth, agent.wealth);
    assert_eq!(updated.productivity, agent.productivity);
    assert_eq!(updated.cooperation, agent.cooperation);
    assert_eq!(updated.aggression, agent.aggression);
    assert_eq!(updated.risk_tolerance, agent.risk_tolerance);
}

#[test]
fn test_phase2_external_golden_case_b() {
    // Case B — partial deficit
    // food = 1.0, health = 0.8, metabolic = 2.0, decay = 0.25
    // F_consumed = 1.0, F_deficit = 1.0, health_delta = -0.25
    // food = 0.0, health = 0.55 (0x3f0ccccd), alive = true, eligible = true
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![AgentState {
            agent_id: AgentId(0),
            dense_slot: DenseSlot(0),
            alive: true,
            birth_day: SimulationDay(0),
            health: 0.8,
            food: 1.0,
            wealth: 500,
            productivity: 1.0,
            cooperation: 0.5,
            aggression: 0.5,
            risk_tolerance: 0.5,
            group_id: GroupId(0),
        }],
        settlements: Vec::new(),
        initial_money_supply: 500,
    };

    phase2_biological_degradation(&mut world, &cfg);

    let updated = &world.agents[0];
    assert_eq!(updated.food, 0.0);
    assert_eq!(updated.health, 0.55);
    assert_eq!(updated.health.to_bits(), 0x3f0ccccd);
    assert!(updated.alive);
    assert!(updated.is_behaviorally_eligible());
}

#[test]
fn test_phase2_external_golden_case_c() {
    // Case C — reaches zero health
    // food = 0.0, health = 0.25, metabolic = 2.0, decay = 0.25
    // F_deficit = 2.0, health_delta = -0.5
    // food = 0.0, health = 0.0, alive = true, eligible = false
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![AgentState {
            agent_id: AgentId(0),
            dense_slot: DenseSlot(0),
            alive: true,
            birth_day: SimulationDay(0),
            health: 0.25,
            food: 0.0,
            wealth: 500,
            productivity: 1.0,
            cooperation: 0.5,
            aggression: 0.5,
            risk_tolerance: 0.5,
            group_id: GroupId(0),
        }],
        settlements: Vec::new(),
        initial_money_supply: 500,
    };

    phase2_biological_degradation(&mut world, &cfg);

    let updated = &world.agents[0];
    assert_eq!(updated.food, 0.0);
    assert_eq!(updated.health, 0.0);
    assert!(updated.alive, "alive must remain true in Phase 2");
    assert!(!updated.is_behaviorally_eligible());
}

#[test]
fn test_phase2_external_golden_case_d() {
    // Case D — already dead
    // food = 10.0, health = 0.5, alive = false -> no mutation
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![AgentState {
            agent_id: AgentId(0),
            dense_slot: DenseSlot(0),
            alive: false,
            birth_day: SimulationDay(0),
            health: 0.5,
            food: 10.0,
            wealth: 500,
            productivity: 1.0,
            cooperation: 0.5,
            aggression: 0.5,
            risk_tolerance: 0.5,
            group_id: GroupId(0),
        }],
        settlements: Vec::new(),
        initial_money_supply: 500,
    };

    phase2_biological_degradation(&mut world, &cfg);

    let updated = &world.agents[0];
    assert_eq!(updated.food, 10.0);
    assert_eq!(updated.health, 0.5);
    assert!(!updated.alive);
    assert!(!updated.is_behaviorally_eligible());
}

#[test]
fn test_behavioral_eligibility_predicate() {
    let make_agent = |alive: bool, health: f32| AgentState {
        agent_id: AgentId(0),
        dense_slot: DenseSlot(0),
        alive,
        birth_day: SimulationDay(0),
        health,
        food: 10.0,
        wealth: 100,
        productivity: 1.0,
        cooperation: 0.0,
        aggression: 0.0,
        risk_tolerance: 0.0,
        group_id: GroupId(0),
    };

    assert!(make_agent(true, 0.8).is_behaviorally_eligible());
    assert!(!make_agent(true, 0.0).is_behaviorally_eligible());
    assert!(!make_agent(false, 0.8).is_behaviorally_eligible());
    assert!(!make_agent(false, 0.0).is_behaviorally_eligible());
}

#[test]
fn test_phase_ordering_deterministic_replay() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();

    let mut world1 = initialize_world(&cfg).unwrap();
    let mut world2 = initialize_world(&cfg).unwrap();
    assert_eq!(world1, world2);

    execute_phases_1_and_2(&mut world1, &cfg);
    execute_phases_1_and_2(&mut world2, &cfg);

    assert_eq!(world1, world2);
    assert_eq!(world1.current_day, SimulationDay(0));
}

#[test]
fn test_phase2_settlement_isolation() {
    let cfg = SimConfig::parse_and_validate(BASE_TOML).unwrap();
    let mut world = initialize_world(&cfg).unwrap();

    let settlements_before = world.settlements.clone();
    phase2_biological_degradation(&mut world, &cfg);
    assert_eq!(world.settlements, settlements_before);
}
