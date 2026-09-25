use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::{
    AgentState, Command, CommandExecutionError, Phase9Error, SettlementState, SimConfig,
    WorldState, phase2_biological_degradation, phase8_welfare_distribution,
    phase9_mortality_commitment, phase9_mortality_commitment_with_config,
    phase9_mortality_resolution,
};

fn make_test_agent(
    id: u32,
    gid: u16,
    food: f32,
    wealth: Money,
    alive: bool,
    health: f32,
) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive,
        birth_day: SimulationDay(0),
        health,
        food,
        wealth,
        group_id: GroupId(gid),
        productivity: 1.25,
        cooperation: 0.6,
        aggression: 0.2,
        risk_tolerance: 0.4,
    }
}

fn make_test_settlement(gid: u16, resource: f32, treasury: Money) -> SettlementState {
    SettlementState {
        group_id: GroupId(gid),
        resource,
        treasury,
    }
}

fn make_test_world(agents: Vec<AgentState>, settlements: Vec<SettlementState>) -> WorldState {
    let mut initial_money_supply: Money = 0;
    for a in &agents {
        initial_money_supply = initial_money_supply.saturating_add(a.wealth);
    }
    for s in &settlements {
        initial_money_supply = initial_money_supply.saturating_add(s.treasury);
    }
    WorldState {
        current_day: SimulationDay(0),
        agents,
        settlements,
        initial_money_supply,
    }
}

const TEST_CONFIG_TOML: &str = r#"
[world]
master_seed = 42
replicate_id = 0
initial_population = 2
settlement_count = 1
initial_health = 1.0
initial_food = 5.0
initial_wealth = 10
initial_settlement_resource = 100.0
initial_treasury = 100

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
base_metabolic_cost = 1.0
health_decay_rate = 0.1

[economy]
base_work_yield = 2.0
food_price = 10
target_food = 20.0
target_reserve = 1000
tax_rate = 0.1
welfare_payment = 25

[interaction]
gift_amount = 5.0
theft_amount = 5.0
theft_success_probability = 0.5
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

// =============================================================================
// 1. Core Mortality Tests
// =============================================================================

#[test]
fn test_01_alive_true_health_zero_becomes_dead() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(res.newly_deceased, vec![AgentId(1)]);
    assert_eq!(res.newly_deceased_count, 1);
    assert_eq!(res.already_dead_count, 0);
    assert_eq!(res.survivors_count, 0);

    assert!(!world.agents[0].alive);
}

#[test]
fn test_02_alive_true_health_negative_becomes_dead() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, -0.5)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(res.newly_deceased, vec![AgentId(1)]);
    assert_eq!(res.newly_deceased_count, 1);
    assert!(!world.agents[0].alive);
}

#[test]
fn test_03_health_gt_zero_remains_alive() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 0.5)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert!(res.newly_deceased.is_empty());
    assert_eq!(res.newly_deceased_count, 0);
    assert_eq!(res.already_dead_count, 0);
    assert_eq!(res.survivors_count, 1);

    assert!(world.agents[0].alive);
}

#[test]
fn test_04_already_dead_agent_remains_dead() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, false, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert!(res.newly_deceased.is_empty());
    assert_eq!(res.already_dead_count, 1);
    assert!(!world.agents[0].alive);
}

#[test]
fn test_05_already_dead_agent_not_reported_as_newly_deceased() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, false, 0.0), // already dead
            make_test_agent(2, 0, 5.0, 10, true, 0.0),  // newly deceased
            make_test_agent(3, 0, 5.0, 10, true, 0.8),  // survivor
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(res.newly_deceased, vec![AgentId(2)]);
    assert_eq!(res.newly_deceased_count, 1);
    assert_eq!(res.already_dead_count, 1);
    assert_eq!(res.survivors_count, 1);

    assert!(!world.agents[0].alive);
    assert!(!world.agents[1].alive);
    assert!(world.agents[2].alive);
}

// =============================================================================
// 2. Boundary / Validation Tests
// =============================================================================

#[test]
fn test_06_smallest_positive_finite_health_remains_alive() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, f32::MIN_POSITIVE),
            make_test_agent(2, 0, 5.0, 10, true, 1e-7),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert!(res.newly_deceased.is_empty());
    assert_eq!(res.survivors_count, 2);

    assert!(world.agents[0].alive);
    assert!(world.agents[1].alive);
}

#[test]
fn test_07_nan_health_fails_before_mutation() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, f32::NAN)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase9_mortality_commitment(&mut world).unwrap_err();
    assert!(matches!(
        err,
        Phase9Error::NonFiniteHealth {
            agent_id: AgentId(1),
            ..
        }
    ));

    assert!(world.agents[0].alive);
}

#[test]
fn test_08_pos_inf_health_fails_before_mutation() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, f32::INFINITY)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase9_mortality_commitment(&mut world).unwrap_err();
    assert!(matches!(
        err,
        Phase9Error::NonFiniteHealth {
            agent_id: AgentId(1),
            ..
        }
    ));

    assert!(world.agents[0].alive);
}

#[test]
fn test_09_neg_inf_health_fails_before_mutation() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, f32::NEG_INFINITY)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase9_mortality_commitment(&mut world).unwrap_err();
    assert!(matches!(
        err,
        Phase9Error::NonFiniteHealth {
            agent_id: AgentId(1),
            ..
        }
    ));

    assert!(world.agents[0].alive);
}

#[test]
fn test_10_invalid_later_agent_zero_partial_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),      // would die
            make_test_agent(2, 0, 5.0, 10, true, f32::NAN), // invalid
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase9_mortality_commitment(&mut world).unwrap_err();
    assert!(matches!(
        err,
        Phase9Error::NonFiniteHealth {
            agent_id: AgentId(2),
            ..
        }
    ));

    // Agent 1 must remain alive! Zero partial mutation.
    assert!(world.agents[0].alive);
    assert!(world.agents[1].alive);
}

// =============================================================================
// 3. Mutation Isolation Tests
// =============================================================================

#[test]
fn test_11_food_unchanged_on_death() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 7.5, 10, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].food, 7.5);
}

#[test]
fn test_12_wealth_unchanged_on_death() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 4200, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].wealth, 4200);
}

#[test]
fn test_13_health_unchanged_on_death() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
            make_test_agent(2, 0, 5.0, 10, true, -0.25),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].health, 0.0);
    assert_eq!(world.agents[1].health, -0.25);
}

#[test]
fn test_14_traits_unchanged_on_death() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].productivity, 1.25);
    assert_eq!(world.agents[0].cooperation, 0.6);
    assert_eq!(world.agents[0].aggression, 0.2);
    assert_eq!(world.agents[0].risk_tolerance, 0.4);
    assert_eq!(world.agents[0].birth_day, SimulationDay(0));
    assert_eq!(world.agents[0].group_id, GroupId(0));
}

#[test]
fn test_15_settlement_treasury_and_resource_unchanged() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 0.0)],
        vec![make_test_settlement(0, 123.45, 6789)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.settlements[0].resource, 123.45);
    assert_eq!(world.settlements[0].treasury, 6789);
}

// =============================================================================
// 4. Identity / M0 Storage Semantics Tests
// =============================================================================

#[test]
fn test_16_agent_id_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(10, 0, 5.0, 10, true, 0.0),
            make_test_agent(20, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].agent_id, AgentId(10));
    assert_eq!(world.agents[1].agent_id, AgentId(20));
}

#[test]
fn test_17_dense_slot_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(10, 0, 5.0, 10, true, 0.0),
            make_test_agent(20, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].dense_slot, DenseSlot(10));
    assert_eq!(world.agents[1].dense_slot, DenseSlot(20));
}

#[test]
fn test_18_vec_length_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
            make_test_agent(2, 0, 5.0, 10, true, 0.0),
            make_test_agent(3, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    assert_eq!(world.agents.len(), 3);
    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents.len(), 3);
}

#[test]
fn test_19_physical_agent_order_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(50, 0, 5.0, 10, true, 0.0),
            make_test_agent(10, 0, 5.0, 10, true, 1.0),
            make_test_agent(30, 0, 5.0, 10, true, 0.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].agent_id, AgentId(50));
    assert_eq!(world.agents[1].agent_id, AgentId(10));
    assert_eq!(world.agents[2].agent_id, AgentId(30));
}

#[test]
fn test_20_no_agent_id_reuse_or_deletion() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
            make_test_agent(2, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    let ids: Vec<u32> = world.agents.iter().map(|a| a.agent_id.0).collect();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn test_21_no_swap_remove_compaction_in_m0() {
    // In M2 swap-remove, agent 0 (dead) would be replaced by agent 2 (last alive).
    // In M0 reference model, agent 0 MUST remain in index 0 as a tombstone.
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0), // dies
            make_test_agent(2, 0, 5.0, 10, true, 1.0), // alive
            make_test_agent(3, 0, 5.0, 10, true, 1.0), // alive
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(world.agents[0].agent_id, AgentId(1));
    assert!(!world.agents[0].alive);
    assert_eq!(world.agents[1].agent_id, AgentId(2));
    assert!(world.agents[1].alive);
    assert_eq!(world.agents[2].agent_id, AgentId(3));
    assert!(world.agents[2].alive);
}

// =============================================================================
// 5. Determinism Tests
// =============================================================================

#[test]
fn test_22_returned_newly_deceased_ids_sorted_ascending_agent_id() {
    // Agents in reverse/arbitrary ID order: [99, 3, 42, 1]
    let mut world = make_test_world(
        vec![
            make_test_agent(99, 0, 5.0, 10, true, 0.0),
            make_test_agent(3, 0, 5.0, 10, true, 0.0),
            make_test_agent(42, 0, 5.0, 10, true, 0.0),
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(
        res.newly_deceased,
        vec![AgentId(1), AgentId(3), AgentId(42), AgentId(99)]
    );
}

#[test]
fn test_23_shuffled_physical_storage_produces_identical_logical_mortality_result() {
    let mut world_a = make_test_world(
        vec![
            make_test_agent(10, 0, 5.0, 10, true, 0.0),
            make_test_agent(20, 0, 5.0, 10, true, 1.0),
            make_test_agent(30, 0, 5.0, 10, true, 0.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let mut world_b = make_test_world(
        vec![
            make_test_agent(30, 0, 5.0, 10, true, 0.0),
            make_test_agent(10, 0, 5.0, 10, true, 0.0),
            make_test_agent(20, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res_a = phase9_mortality_commitment(&mut world_a).unwrap();
    let res_b = phase9_mortality_commitment(&mut world_b).unwrap();

    assert_eq!(res_a, res_b);
    assert_eq!(res_a.newly_deceased, vec![AgentId(10), AgentId(30)]);
}

#[test]
fn test_24_zero_rng_consumption() {
    let make_world = || {
        make_test_world(
            vec![
                make_test_agent(1, 0, 5.0, 10, true, 0.0),
                make_test_agent(2, 0, 5.0, 10, true, 0.5),
            ],
            vec![make_test_settlement(0, 100.0, 100)],
        )
    };

    let mut w1 = make_world();
    let mut w2 = make_world();

    let res1 = phase9_mortality_commitment(&mut w1).unwrap();
    let res2 = phase9_mortality_commitment(&mut w2).unwrap();

    assert_eq!(res1, res2);
    assert_eq!(w1, w2);
}

#[test]
fn test_25_repeated_phase9_invocation_is_idempotent() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
            make_test_agent(2, 0, 5.0, 10, true, 0.5),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res1 = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(res1.newly_deceased, vec![AgentId(1)]);
    assert_eq!(res1.newly_deceased_count, 1);
    assert_eq!(res1.already_dead_count, 0);
    assert_eq!(res1.survivors_count, 1);

    let snapshot = world.clone();

    // Second invocation: agent 1 is already dead, so newly_deceased is empty
    let res2 = phase9_mortality_commitment(&mut world).unwrap();
    assert!(res2.newly_deceased.is_empty());
    assert_eq!(res2.newly_deceased_count, 0);
    assert_eq!(res2.already_dead_count, 1);
    assert_eq!(res2.survivors_count, 1);

    assert_eq!(world, snapshot);
}

// =============================================================================
// 6. Integration Tests
// =============================================================================

#[test]
fn test_26_phase2_starvation_degradation_reaching_zero_formally_committed_in_phase9() {
    let cfg = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();

    // Agent has food 0.0 and health 0.05
    // Phase 2: base_metabolic_cost = 1.0, deficit = 1.0, health_decay_rate = 0.1
    // delta = -0.1 => health becomes (0.05 - 0.1).clamp(0.0, 1.0) = 0.0
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 10, true, 0.05)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    phase2_biological_degradation(&mut world, &cfg);
    assert_eq!(world.agents[0].health, 0.0);
    // Crucial contract: Phase 2 does NOT set alive = false!
    assert!(world.agents[0].alive);

    // Phase 9 executes: formal death commitment
    let res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(res.newly_deceased, vec![AgentId(1)]);
    assert!(!world.agents[0].alive);
}

#[test]
fn test_27_agent_with_health_zero_excluded_from_phases_3_to_8_before_phase9() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 0.0, 0, true, 0.0),  // health <= 0
            make_test_agent(2, 0, 10.0, 0, true, 1.0), // living
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    // Contract: Agent 1 has health <= 0.0, so is_behaviorally_eligible is false
    assert!(!world.agents[0].is_behaviorally_eligible());

    // Phase 8 welfare excludes agent 1 even though food 0.0 < starvation_threshold 10.0
    let wlf_res = phase8_welfare_distribution(&mut world, 10.0, 25).unwrap();
    assert_eq!(wlf_res[0].eligible_count, 0); // agent 1 excluded due to health <= 0, agent 2 food 10 >= 10
    assert_eq!(world.agents[0].wealth, 0);

    // Phase 9 formally commits death
    let mort_res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(mort_res.newly_deceased, vec![AgentId(1)]);
    assert!(!world.agents[0].alive);
}

#[test]
fn test_28_phase8_welfare_cannot_revive_or_alter_mortality() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 0, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    // Run Phase 8
    let wlf_res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(wlf_res[0].eligible_count, 0);
    assert_eq!(world.agents[0].health, 0.0);
    assert!(world.agents[0].alive); // still true prior to Phase 9

    // Run Phase 9
    let mort_res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(mort_res.newly_deceased, vec![AgentId(1)]);
    assert!(!world.agents[0].alive);
}

#[test]
fn test_29_convenience_wrappers_and_command_execution() {
    let cfg = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();

    // Test with_config wrapper
    let mut world1 = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 0, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );
    let res1 = phase9_mortality_commitment_with_config(&mut world1, &cfg).unwrap();
    assert_eq!(res1.newly_deceased, vec![AgentId(1)]);

    // Test alias
    let mut world2 = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 0, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );
    let res2 = phase9_mortality_resolution(&mut world2).unwrap();
    assert_eq!(res2, res1);

    // Test Command::MortalityStatusCommitment direct execution
    let mut world3 = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 0, true, 0.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );
    let cmd = Command::MortalityStatusCommitment {
        deceased_agents: vec![AgentId(1)],
    };
    cmd.execute(&mut world3).unwrap();
    assert!(!world3.agents[0].alive);

    // Command fails if agent has positive health
    let mut world4 = make_test_world(
        vec![make_test_agent(1, 0, 0.0, 0, true, 0.5)],
        vec![make_test_settlement(0, 100.0, 100)],
    );
    let bad_cmd = Command::MortalityStatusCommitment {
        deceased_agents: vec![AgentId(1)],
    };
    assert_eq!(
        bad_cmd.execute(&mut world4),
        Err(CommandExecutionError::IneligibleMortalityAgent(AgentId(1)))
    );

    // Command fails if agent is missing
    let missing_cmd = Command::MortalityStatusCommitment {
        deceased_agents: vec![AgentId(999)],
    };
    assert_eq!(
        missing_cmd.execute(&mut world4),
        Err(CommandExecutionError::MissingAgent(AgentId(999)))
    );
}

#[test]
fn test_30_duplicate_agent_fails_fast() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 0.0),
            make_test_agent(1, 0, 5.0, 10, true, 0.0), // duplicate
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase9_mortality_commitment(&mut world).unwrap_err();
    assert_eq!(err, Phase9Error::DuplicateAgent(AgentId(1)));

    // World state remains untouched
    assert!(world.agents[0].alive);
    assert!(world.agents[1].alive);
}
