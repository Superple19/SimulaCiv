use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::{SimConfig, initialize_world};

const GOLDEN_TOML: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 4
settlement_count = 2
initial_health = 0.75
initial_food = 12.5
initial_wealth = 10000
initial_settlement_resource = 100.0
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
carrying_capacity = 10000.0
regrowth_rate = 0.05
base_metabolic_cost = 1.0
health_decay_rate = 0.05

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
fn test_external_day0_golden_fixture() {
    let cfg = SimConfig::parse_and_validate(GOLDEN_TOML).expect("golden toml must be valid");
    assert_eq!(cfg.world.master_seed, 0x0123456789abcdef);

    let world = initialize_world(&cfg).expect("world initialization must succeed");

    assert_eq!(world.current_day, SimulationDay(0));
    assert_eq!(world.initial_money_supply, 45_000);
    assert_eq!(world.agents.len(), 4);
    assert_eq!(world.settlements.len(), 2);

    // Expected settlements
    assert_eq!(world.settlements[0].group_id, GroupId(0));
    assert_eq!(world.settlements[0].resource, 100.0);
    assert_eq!(world.settlements[0].treasury, 2500);

    assert_eq!(world.settlements[1].group_id, GroupId(1));
    assert_eq!(world.settlements[1].resource, 100.0);
    assert_eq!(world.settlements[1].treasury, 2500);

    // Expected structural agent fields
    let expected_agent_ids = [AgentId(0), AgentId(1), AgentId(2), AgentId(3)];
    let expected_dense_slots = [DenseSlot(0), DenseSlot(1), DenseSlot(2), DenseSlot(3)];
    let expected_groups = [GroupId(0), GroupId(1), GroupId(0), GroupId(1)];

    for i in 0..4 {
        let a = &world.agents[i];
        assert_eq!(a.agent_id, expected_agent_ids[i]);
        assert_eq!(a.dense_slot, expected_dense_slots[i]);
        assert_eq!(a.group_id, expected_groups[i]);
        assert!(a.alive);
        assert_eq!(a.birth_day, SimulationDay(0));
        assert_eq!(a.health, 0.75);
        assert_eq!(a.food, 12.5);
        assert_eq!(a.wealth, 10_000);
    }

    // Exact trait golden bits
    // Agent 0
    assert_eq!(world.agents[0].productivity.to_bits(), 0x3ff660da);
    assert_eq!(world.agents[0].cooperation.to_bits(), 0x3e838b4e);
    assert_eq!(world.agents[0].aggression.to_bits(), 0x3f0c24ce);
    assert_eq!(world.agents[0].risk_tolerance.to_bits(), 0x3ed92588);

    // Agent 1
    assert_eq!(world.agents[1].productivity.to_bits(), 0x3feacdbb);
    assert_eq!(world.agents[1].cooperation.to_bits(), 0x3ef642f4);
    assert_eq!(world.agents[1].aggression.to_bits(), 0x3de50150);
    assert_eq!(world.agents[1].risk_tolerance.to_bits(), 0x3e225424);

    // Agent 2
    assert_eq!(world.agents[2].productivity.to_bits(), 0x3f1a3bca);
    assert_eq!(world.agents[2].cooperation.to_bits(), 0x3f20b2ce);
    assert_eq!(world.agents[2].aggression.to_bits(), 0x3e42e910);
    assert_eq!(world.agents[2].risk_tolerance.to_bits(), 0x3f2b5a31);

    // Agent 3
    assert_eq!(world.agents[3].productivity.to_bits(), 0x3f3ced36);
    assert_eq!(world.agents[3].cooperation.to_bits(), 0x3ef287a4);
    assert_eq!(world.agents[3].aggression.to_bits(), 0x3d197d20);
    assert_eq!(world.agents[3].risk_tolerance.to_bits(), 0x3f4d30c7);
}

#[test]
fn test_deterministic_replay() {
    let cfg = SimConfig::parse_and_validate(GOLDEN_TOML).expect("golden toml must be valid");
    let world1 = initialize_world(&cfg).expect("initialization 1 must succeed");
    let world2 = initialize_world(&cfg).expect("initialization 2 must succeed");

    assert_eq!(world1, world2);
}

#[test]
fn test_replicate_isolation() {
    let cfg_rep7 = SimConfig::parse_and_validate(GOLDEN_TOML).expect("golden toml must be valid");
    let toml_rep8 = GOLDEN_TOML.replace("replicate_id = 7", "replicate_id = 8");
    let cfg_rep8 = SimConfig::parse_and_validate(&toml_rep8).expect("rep8 toml must be valid");

    let world_rep7 = initialize_world(&cfg_rep7).expect("rep7 world");
    let world_rep8 = initialize_world(&cfg_rep8).expect("rep8 world");

    // Structural state must remain identical
    assert_eq!(world_rep7.current_day, world_rep8.current_day);
    assert_eq!(
        world_rep7.initial_money_supply,
        world_rep8.initial_money_supply
    );
    assert_eq!(world_rep7.settlements, world_rep8.settlements);
    assert_eq!(world_rep7.agents.len(), world_rep8.agents.len());

    for (a7, a8) in world_rep7.agents.iter().zip(world_rep8.agents.iter()) {
        assert_eq!(a7.agent_id, a8.agent_id);
        assert_eq!(a7.dense_slot, a8.dense_slot);
        assert_eq!(a7.group_id, a8.group_id);
        assert_eq!(a7.alive, a8.alive);
        assert_eq!(a7.birth_day, a8.birth_day);
        assert_eq!(a7.health, a8.health);
        assert_eq!(a7.food, a8.food);
        assert_eq!(a7.wealth, a8.wealth);

        // Stochastic traits should differ due to replicate coordinate shift
        assert_ne!(
            (
                a7.productivity.to_bits(),
                a7.cooperation.to_bits(),
                a7.aggression.to_bits(),
                a7.risk_tolerance.to_bits()
            ),
            (
                a8.productivity.to_bits(),
                a8.cooperation.to_bits(),
                a8.aggression.to_bits(),
                a8.risk_tolerance.to_bits()
            )
        );
    }
}

#[test]
fn test_zero_population_initialization() {
    let toml_zero_pop = GOLDEN_TOML
        .replace("initial_population = 4", "initial_population = 0")
        .replace("settlement_count = 2", "settlement_count = 3");
    let cfg = SimConfig::parse_and_validate(&toml_zero_pop).expect("zero pop toml must be valid");
    let world = initialize_world(&cfg).expect("initialization must succeed");

    assert!(world.agents.is_empty());
    assert_eq!(world.settlements.len(), 3);
    assert_eq!(world.initial_money_supply, 3 * cfg.world.initial_treasury);

    for (idx, s) in world.settlements.iter().enumerate() {
        assert_eq!(s.group_id, GroupId(idx as u16));
        assert_eq!(s.resource, cfg.world.initial_settlement_resource);
        assert_eq!(s.treasury, cfg.world.initial_treasury);
    }
}

#[test]
fn test_group_assignment_modular_distribution() {
    // N = 7, G = 3 -> [0, 1, 2, 0, 1, 2, 0]
    let toml_uneven = GOLDEN_TOML
        .replace("initial_population = 4", "initial_population = 7")
        .replace("settlement_count = 2", "settlement_count = 3");
    let cfg = SimConfig::parse_and_validate(&toml_uneven).expect("uneven toml must be valid");
    let world = initialize_world(&cfg).expect("initialization must succeed");

    let expected_groups = [
        GroupId(0),
        GroupId(1),
        GroupId(2),
        GroupId(0),
        GroupId(1),
        GroupId(2),
        GroupId(0),
    ];
    let actual_groups: Vec<GroupId> = world.agents.iter().map(|a| a.group_id).collect();
    assert_eq!(actual_groups, expected_groups);
}

#[test]
fn test_money_conservation_at_day_0() {
    let cfg = SimConfig::parse_and_validate(GOLDEN_TOML).expect("golden toml must be valid");
    let world = initialize_world(&cfg).expect("initialization must succeed");

    let sum_agent_wealth: i64 = world.agents.iter().map(|a| a.wealth).sum();
    let sum_settlement_treasury: i64 = world.settlements.iter().map(|s| s.treasury).sum();

    assert_eq!(
        sum_agent_wealth + sum_settlement_treasury,
        world.initial_money_supply
    );
}

#[test]
fn test_no_accidental_phase_execution() {
    let cfg = SimConfig::parse_and_validate(GOLDEN_TOML).expect("golden toml must be valid");
    let world = initialize_world(&cfg).expect("initialization must succeed");

    // Must be exactly Day 0, before Phase 1
    assert_eq!(world.current_day, SimulationDay(0));

    for a in &world.agents {
        assert_eq!(a.health, cfg.world.initial_health);
        assert_eq!(a.food, cfg.world.initial_food);
        assert_eq!(a.wealth, cfg.world.initial_wealth);
        assert!(a.alive);
        assert_eq!(a.birth_day, SimulationDay(0));
    }

    for s in &world.settlements {
        assert_eq!(s.resource, cfg.world.initial_settlement_resource);
        assert_eq!(s.treasury, cfg.world.initial_treasury);
    }
}

#[test]
fn test_money_calculation_overflow_fails_explicitly() {
    let toml_overflow = GOLDEN_TOML
        .replace("initial_population = 4", "initial_population = 4294967295")
        .replace(
            "initial_wealth = 10000",
            "initial_wealth = 9223372036854775807",
        );
    let cfg = SimConfig::parse_and_validate(&toml_overflow).expect("valid config");
    let err = initialize_world(&cfg).unwrap_err();
    assert_eq!(err, sim_model::InitializationError::MoneyOverflow);
}
