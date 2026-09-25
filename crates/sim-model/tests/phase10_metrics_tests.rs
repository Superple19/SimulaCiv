use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::{
    AgentState, Intent, Phase10Error, SettlementIntentPartition, SettlementState, SimConfig,
    WorldState, phase7_market_clearance, phase8_welfare_distribution, phase9_mortality_commitment,
    phase10_metrics, phase10_metrics_observation, phase10_observe, phase10_observe_with_config,
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
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.1,
        risk_tolerance: 0.2,
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
        current_day: SimulationDay(5),
        agents,
        settlements,
        initial_money_supply,
    }
}

// =============================================================================
// 1. Population Tests
// =============================================================================

#[test]
fn test_01_all_living_agents_counted() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 5.0, 10, true, 1.0),
            make_test_agent(3, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.population, 3);
}

#[test]
fn test_02_dead_tombstones_excluded() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 5.0, 10, false, 0.0), // dead tombstone
            make_test_agent(3, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.population, 2);
}

#[test]
fn test_03_empty_world_population_zero() {
    let world = make_test_world(vec![], vec![make_test_settlement(0, 100.0, 50)]);

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.population, 0);
}

#[test]
fn test_04_physical_storage_order_does_not_change_population() {
    let world_a = make_test_world(
        vec![
            make_test_agent(10, 0, 5.0, 10, true, 1.0),
            make_test_agent(20, 0, 5.0, 10, false, 0.0),
            make_test_agent(30, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let world_b = make_test_world(
        vec![
            make_test_agent(30, 0, 5.0, 10, true, 1.0),
            make_test_agent(10, 0, 5.0, 10, true, 1.0),
            make_test_agent(20, 0, 5.0, 10, false, 0.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    assert_eq!(
        phase10_observe(&world_a, 5).unwrap().population,
        phase10_observe(&world_b, 5).unwrap().population
    );
}

// =============================================================================
// 2. Food Reserves Tests
// =============================================================================

#[test]
fn test_05_sum_food_of_living_agents_only() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 12.5, 10, true, 1.0),
            make_test_agent(2, 0, 7.25, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_food_reserves, 19.75_f64);
}

#[test]
fn test_06_dead_tombstone_food_excluded() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 10.0, 10, true, 1.0),
            make_test_agent(2, 0, 100.0, 10, false, 0.0), // dead: food must be excluded
            make_test_agent(3, 0, 5.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_food_reserves, 15.0_f64);
}

#[test]
fn test_07_canonical_agent_id_accumulation_order() {
    // Agents in reversed storage order [2, 1]
    let world_rev = make_test_world(
        vec![
            make_test_agent(2, 0, 2.5, 10, true, 1.0),
            make_test_agent(1, 0, 10.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let world_fwd = make_test_world(
        vec![
            make_test_agent(1, 0, 10.0, 10, true, 1.0),
            make_test_agent(2, 0, 2.5, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let m_rev = phase10_observe(&world_rev, 5).unwrap();
    let m_fwd = phase10_observe(&world_fwd, 5).unwrap();
    assert_eq!(m_rev.total_food_reserves, m_fwd.total_food_reserves);
    assert_eq!(
        m_rev.total_food_reserves.to_bits(),
        m_fwd.total_food_reserves.to_bits()
    );
}

#[test]
fn test_08_zero_living_agents_food_reserves_zero() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, 10.0, 10, false, 0.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_food_reserves, 0.0_f64);
}

#[test]
fn test_09_nan_food_rejected() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, f32::NAN, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let err = phase10_observe(&world, 5).unwrap_err();
    assert!(matches!(err, Phase10Error::NonFiniteFood { .. }));
}

#[test]
fn test_10_infinite_food_rejected() {
    let world_pos = make_test_world(
        vec![make_test_agent(1, 0, f32::INFINITY, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert!(matches!(
        phase10_observe(&world_pos, 5).unwrap_err(),
        Phase10Error::NonFiniteFood { .. }
    ));

    let world_neg = make_test_world(
        vec![make_test_agent(1, 0, f32::NEG_INFINITY, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert!(matches!(
        phase10_observe(&world_neg, 5).unwrap_err(),
        Phase10Error::NonFiniteFood { .. }
    ));
}

#[test]
fn test_11_negative_food_rejected() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, -0.01, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let err = phase10_observe(&world, 5).unwrap_err();
    assert!(matches!(err, Phase10Error::NegativeFood { .. }));
}

// =============================================================================
// 3. Treasury Tests
// =============================================================================

#[test]
fn test_12_sum_all_settlement_treasuries() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![
            make_test_settlement(0, 100.0, 500),
            make_test_settlement(1, 100.0, 300),
            make_test_settlement(2, 100.0, 200),
        ],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_treasury, 1000);
}

#[test]
fn test_13_canonical_group_id_ordering_for_treasury() {
    // Settlements stored in reversed GroupId order [2, 0, 1]
    let world_rev = make_test_world(
        vec![],
        vec![
            make_test_settlement(2, 100.0, 200),
            make_test_settlement(0, 100.0, 500),
            make_test_settlement(1, 100.0, 300),
        ],
    );

    let metrics = phase10_observe(&world_rev, 5).unwrap();
    assert_eq!(metrics.total_treasury, 1000);
}

#[test]
fn test_14_zero_settlements_zero_treasury() {
    let world = make_test_world(vec![], vec![]);

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_treasury, 0);
}

#[test]
fn test_15_negative_treasury_rejected() {
    let world = make_test_world(vec![], vec![make_test_settlement(0, 100.0, -50)]);

    let err = phase10_observe(&world, 5).unwrap_err();
    assert_eq!(
        err,
        Phase10Error::NegativeTreasury {
            group_id: GroupId(0),
            treasury: -50
        }
    );
}

#[test]
fn test_16_treasury_checked_add_overflow_rejected() {
    let world = make_test_world(
        vec![],
        vec![
            make_test_settlement(0, 100.0, Money::MAX),
            make_test_settlement(1, 100.0, 1),
        ],
    );

    let err = phase10_observe(&world, 5).unwrap_err();
    assert_eq!(err, Phase10Error::ArithmeticOverflow);
}

// =============================================================================
// 4. Gini Exact Reference Cases
// =============================================================================

#[test]
fn test_17_empty_living_population_gini_zero() {
    let world = make_test_world(vec![], vec![make_test_settlement(0, 100.0, 50)]);

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.0);
}

#[test]
fn test_18_one_living_agent_gini_zero() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 500, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.0);
}

#[test]
fn test_19_all_living_wealth_zero_gini_zero() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 0, true, 1.0),
            make_test_agent(3, 0, 5.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.0);
}

#[test]
fn test_20_all_living_agents_equal_wealth_gini_zero() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 250, true, 1.0),
            make_test_agent(2, 0, 5.0, 250, true, 1.0),
            make_test_agent(3, 0, 5.0, 250, true, 1.0),
            make_test_agent(4, 0, 5.0, 250, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.0);
}

#[test]
fn test_21_two_agent_zero_and_hundred_gini_half() {
    // [0, 100] -> n=2, S=100, W = 1*0 + 2*100 = 200
    // numerator = 2*200 - 3*100 = 100, denominator = 2*100 = 200 -> Gini = 0.5
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 100, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.5);
}

#[test]
fn test_22_known_four_agent_unequal_distribution_exact_gini() {
    // [10, 20, 30, 40]
    // n=4, S=100, W = 10 + 40 + 90 + 160 = 300
    // numerator = 2*300 - 5*100 = 100, denominator = 4*100 = 400 -> Gini = 0.25 exactly
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 5.0, 20, true, 1.0),
            make_test_agent(3, 0, 5.0, 30, true, 1.0),
            make_test_agent(4, 0, 5.0, 40, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.25);
}

#[test]
fn test_23_dead_agent_wealth_excluded() {
    // Alive: [0, 100] -> Gini 0.5
    // Dead: wealth 10000 -> must not influence Gini
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 100, true, 1.0),
            make_test_agent(3, 0, 5.0, 10000, false, 0.0), // dead
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.wealth_gini, 0.5);
}

#[test]
fn test_24_treasury_excluded_from_gini() {
    // Alive: [0, 100] -> Gini 0.5 regardless of treasury size
    let world1 = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 100, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let world2 = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 100, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 1_000_000)],
    );

    assert_eq!(
        phase10_observe(&world1, 5).unwrap().wealth_gini,
        phase10_observe(&world2, 5).unwrap().wealth_gini
    );
}

#[test]
fn test_25_shuffled_physical_storage_produces_identical_gini() {
    let world_a = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 5.0, 20, true, 1.0),
            make_test_agent(3, 0, 5.0, 30, true, 1.0),
            make_test_agent(4, 0, 5.0, 40, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let world_b = make_test_world(
        vec![
            make_test_agent(3, 0, 5.0, 30, true, 1.0),
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(4, 0, 5.0, 40, true, 1.0),
            make_test_agent(2, 0, 5.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    assert_eq!(
        phase10_observe(&world_a, 5).unwrap().wealth_gini,
        phase10_observe(&world_b, 5).unwrap().wealth_gini
    );
}

#[test]
fn test_26_equal_wealth_agent_id_tie_order_does_not_alter_result() {
    // Multiple agents with equal wealth: [20, 20, 20]
    let world_1 = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 20, true, 1.0),
            make_test_agent(2, 0, 5.0, 20, true, 1.0),
            make_test_agent(3, 0, 5.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let world_2 = make_test_world(
        vec![
            make_test_agent(3, 0, 5.0, 20, true, 1.0),
            make_test_agent(1, 0, 5.0, 20, true, 1.0),
            make_test_agent(2, 0, 5.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let g1 = phase10_observe(&world_1, 5).unwrap().wealth_gini;
    let g2 = phase10_observe(&world_2, 5).unwrap().wealth_gini;
    assert_eq!(g1, 0.0);
    assert_eq!(g2, 0.0);
}

#[test]
fn test_27_gini_remains_finite_and_within_zero_to_one() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 0, true, 1.0),
            make_test_agent(2, 0, 5.0, 0, true, 1.0),
            make_test_agent(3, 0, 5.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let metrics = phase10_observe(&world, 5).unwrap();
    assert!(metrics.wealth_gini.is_finite());
    assert!((0.0..=1.0).contains(&metrics.wealth_gini));
    // 2/3 for [0, 0, 1000]
    assert!((metrics.wealth_gini - 2.0 / 3.0).abs() < 1e-12);
}

// =============================================================================
// 5. Observer Independence Tests
// =============================================================================

#[test]
fn test_28_leaves_every_agent_field_unchanged() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 6.0, 20, false, 0.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let snapshot_agents = world.agents.clone();
    phase10_observe(&world, 5).unwrap();
    assert_eq!(world.agents, snapshot_agents);
}

#[test]
fn test_29_leaves_every_settlement_field_unchanged() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 123.45, 6789)],
    );

    let snapshot_settlements = world.settlements.clone();
    phase10_observe(&world, 5).unwrap();
    assert_eq!(world.settlements, snapshot_settlements);
}

#[test]
fn test_30_repeated_observation_returns_identical_result() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 15.0, 50, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let m1 = phase10_observe(&world, 5).unwrap();
    let m2 = phase10_observe(&world, 5).unwrap();
    assert_eq!(m1, m2);
}

#[test]
fn test_31_observing_vs_skipping_observation_produces_identical_authoritative_state() {
    let world_observed = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, 20.0, 100, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let world_skipped = world_observed.clone();

    // Run observation on world_observed
    phase10_observe(&world_observed, 5).unwrap();

    // world_observed must be strictly identical to world_skipped
    assert_eq!(world_observed, world_skipped);
}

#[test]
fn test_32_phase10_consumes_no_rng() {
    let make_w = || {
        make_test_world(
            vec![
                make_test_agent(1, 0, 5.0, 10, true, 1.0),
                make_test_agent(2, 0, 20.0, 100, true, 1.0),
            ],
            vec![make_test_settlement(0, 100.0, 50)],
        )
    };

    let w1 = make_w();
    let w2 = make_w();

    let m1 = phase10_observe(&w1, 5).unwrap();
    let m2 = phase10_observe(&w2, 5).unwrap();

    assert_eq!(m1, m2);
    assert_eq!(w1, w2);
}

#[test]
fn test_33_no_command_emitted_or_executed() {
    // phase10_observe takes &WorldState (immutable borrow),
    // proving at compile time that no Command::execute(&mut world) can be called.
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let initial_money = world.initial_money_supply;
    let res = phase10_observe(&world, 5);
    assert!(res.is_ok());
    assert_eq!(world.initial_money_supply, initial_money);
}

// =============================================================================
// 6. Validation / Canonicality Tests
// =============================================================================

#[test]
fn test_34_duplicate_agent_id_rejected() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(1, 0, 5.0, 10, true, 1.0), // duplicate AgentId
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let err = phase10_observe(&world, 5).unwrap_err();
    assert_eq!(err, Phase10Error::DuplicateAgent(AgentId(1)));
}

#[test]
fn test_35_negative_living_wealth_rejected() {
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, -10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let err = phase10_observe(&world, 5).unwrap_err();
    assert_eq!(
        err,
        Phase10Error::NegativeWealth {
            agent_id: AgentId(1),
            wealth: -10
        }
    );
}

#[test]
fn test_36_invalid_later_agent_causes_no_mutation() {
    let world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 10, true, 1.0),
            make_test_agent(2, 0, -5.0, 10, true, 1.0), // invalid negative food
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let snapshot = world.clone();
    let err = phase10_observe(&world, 5).unwrap_err();
    assert_eq!(
        err,
        Phase10Error::NegativeFood {
            agent_id: AgentId(2),
            food: -5.0,
        }
    );
    assert_eq!(world, snapshot);
}

#[test]
fn test_37_canonical_result_independent_of_dense_slot() {
    // World 1: Agent 1 has DenseSlot 0, Agent 2 has DenseSlot 1
    let mut a1 = make_test_agent(1, 0, 5.0, 10, true, 1.0);
    a1.dense_slot = DenseSlot(0);
    let mut a2 = make_test_agent(2, 0, 10.0, 50, true, 1.0);
    a2.dense_slot = DenseSlot(1);
    let world_1 = make_test_world(vec![a1, a2], vec![make_test_settlement(0, 100.0, 50)]);

    // World 2: Agent 1 has DenseSlot 999, Agent 2 has DenseSlot 42
    let mut b1 = make_test_agent(1, 0, 5.0, 10, true, 1.0);
    b1.dense_slot = DenseSlot(999);
    let mut b2 = make_test_agent(2, 0, 10.0, 50, true, 1.0);
    b2.dense_slot = DenseSlot(42);
    let world_2 = make_test_world(vec![b1, b2], vec![make_test_settlement(0, 100.0, 50)]);

    assert_eq!(
        phase10_observe(&world_1, 5).unwrap(),
        phase10_observe(&world_2, 5).unwrap()
    );
}

// =============================================================================
// 7. Integration Tests
// =============================================================================

#[test]
fn test_38_phase9_newly_dead_agent_absent_from_phase10_metrics() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 100, true, 0.0), // dies in Phase 9
            make_test_agent(2, 0, 10.0, 50, true, 1.0), // survives
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    // Run Phase 9: Agent 1 becomes alive = false
    let mort_res = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(mort_res.newly_deceased, vec![AgentId(1)]);

    // Phase 10 observes post-Phase-9 world
    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.population, 1); // only Agent 2
    assert_eq!(metrics.total_food_reserves, 10.0_f64); // only Agent 2's food
    assert_eq!(metrics.wealth_gini, 0.0); // 1 living agent -> Gini 0.0
    assert_eq!(metrics.total_treasury, 200);
}

#[test]
fn test_39_phase8_welfare_is_visible_in_phase10_wealth_gini() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0), // starving, 0 wealth
            make_test_agent(2, 0, 20.0, 100, true, 1.0), // not starving, 100 wealth
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    // Before welfare: [0, 100] -> Gini = 0.5
    let metrics_before = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics_before.wealth_gini, 0.5);

    // Execute Phase 8 welfare: Agent 1 gets 50 welfare from treasury
    phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();

    // After welfare: Agent 1 has 50, Agent 2 has 100 -> [50, 100]
    // Gini = 50 / 300 = 1/6
    let metrics_after = phase10_observe(&world, 5).unwrap();
    assert!((metrics_after.wealth_gini - 1.0 / 6.0).abs() < 1e-12);
    assert_eq!(metrics_after.total_treasury, 50); // 100 - 50 = 50
}

#[test]
fn test_40_phase7_tax_is_visible_in_phase10_treasury() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 0.0, 1000, true, 1.0), // Buyer
            make_test_agent(2, 0, 10.0, 0, true, 1.0),   // Seller
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    }];

    // Phase 7: food_price = 10, tax_rate = 0.10 (10%)
    // Buyer buys 10 food -> total revenue = 100. Tax withheld = 10.
    // Settlement treasury: 100 + 10 = 110.
    phase7_market_clearance(&mut world, &partitions, 10, 0.10).unwrap();

    let metrics = phase10_observe(&world, 5).unwrap();
    assert_eq!(metrics.total_treasury, 110);
}

#[test]
fn test_41_convenience_aliases_and_wrapper() {
    const TOML: &str = r#"
[world]
master_seed = 42
replicate_id = 0
initial_population = 1
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

    let cfg = SimConfig::parse_and_validate(TOML).unwrap();
    let world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let m1 = phase10_observe(&world, 5).unwrap();
    let m2 = phase10_metrics_observation(&world, 5).unwrap();
    let m3 = phase10_metrics(&world, 5).unwrap();
    let m4 = phase10_observe_with_config(&world, &cfg).unwrap();

    assert_eq!(m1, m2);
    assert_eq!(m1, m3);
    assert_eq!(m1, m4);
}
