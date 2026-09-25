use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::{
    AgentState, Command, CommandExecutionError, Intent, Phase8Error, SettlementIntentPartition,
    SettlementState, SimConfig, WelfareRecipientResolution, WelfareRecipientUpdate, WorldState,
    phase7_market_clearance, phase8_welfare_distribution, phase8_welfare_distribution_with_config,
    phase8_welfare_distribution_with_subconfigs, phase8_welfare_resolution,
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
        current_day: SimulationDay(0),
        agents,
        settlements,
        initial_money_supply,
    }
}

fn total_world_money(world: &WorldState) -> Money {
    let mut total: Money = 0;
    for a in &world.agents {
        total = total.checked_add(a.wealth).expect("total wealth valid");
    }
    for s in &world.settlements {
        total = total.checked_add(s.treasury).expect("total treasury valid");
    }
    total
}

// =============================================================================
// 1. Eligibility Tests
// =============================================================================

#[test]
fn test_01_living_health_gt_zero_food_below_threshold_is_eligible() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 0.8)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res.len(), 1);
    assert_eq!(res[0].group_id, GroupId(0));
    assert_eq!(res[0].eligible_count, 1);
    assert_eq!(res[0].total_distributed, 20);
    assert_eq!(res[0].treasury_after, 30);
    assert_eq!(res[0].recipients.len(), 1);
    assert_eq!(res[0].recipients[0].agent_id, AgentId(1));
    assert_eq!(res[0].recipients[0].payout, 20);

    assert_eq!(world.agents[0].wealth, 30);
    assert_eq!(world.settlements[0].treasury, 30);
}

#[test]
fn test_02_food_equal_to_threshold_not_eligible() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 10.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(res[0].treasury_after, 50);
    assert!(res[0].recipients.is_empty());

    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.settlements[0].treasury, 50);
}

#[test]
fn test_03_food_above_threshold_not_eligible() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 10.001, 10, true, 1.0),
            make_test_agent(2, 0, 25.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(res[0].treasury_after, 50);
    assert!(res[0].recipients.is_empty());

    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, 10);
    assert_eq!(world.settlements[0].treasury, 50);
}

#[test]
fn test_04_dead_agent_excluded() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, false, 0.9), // dead
            make_test_agent(2, 0, 2.0, 10, true, 0.9),  // living
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 1);
    assert_eq!(res[0].total_distributed, 20);
    assert_eq!(res[0].recipients.len(), 1);
    assert_eq!(res[0].recipients[0].agent_id, AgentId(2));

    assert_eq!(world.agents[0].wealth, 10); // dead agent unchanged
    assert_eq!(world.agents[1].wealth, 30);
}

#[test]
fn test_05_health_zero_excluded() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 0.0), // health == 0.0
            make_test_agent(2, 0, 2.0, 10, true, 0.5), // health > 0.0
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 1);
    assert_eq!(res[0].total_distributed, 20);
    assert_eq!(res[0].recipients.len(), 1);
    assert_eq!(res[0].recipients[0].agent_id, AgentId(2));

    assert_eq!(world.agents[0].wealth, 10); // health 0 unchanged
    assert_eq!(world.agents[1].wealth, 30);
}

#[test]
fn test_06_health_lt_zero_and_invalid_state_handling() {
    // 1. health < 0.0 is excluded from eligibility (health <= 0.0)
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 2.0, 10, true, -0.5)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(world.agents[0].wealth, 10);

    // 2. Non-finite health fails fast
    let mut world_nan_health = make_test_world(
        vec![make_test_agent(1, 0, 2.0, 10, true, f32::NAN)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert!(matches!(
        phase8_welfare_distribution(&mut world_nan_health, 10.0, 20),
        Err(Phase8Error::InvalidAgentState { .. })
    ));

    // 3. Negative / non-finite food fails fast
    let mut world_neg_food = make_test_world(
        vec![make_test_agent(1, 0, -1.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert!(matches!(
        phase8_welfare_distribution(&mut world_neg_food, 10.0, 20),
        Err(Phase8Error::InvalidAgentState { .. })
    ));

    let mut world_nan_food = make_test_world(
        vec![make_test_agent(1, 0, f32::NAN, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert!(matches!(
        phase8_welfare_distribution(&mut world_nan_food, 10.0, 20),
        Err(Phase8Error::InvalidAgentState { .. })
    ));

    // 4. Negative wealth fails fast
    let mut world_neg_wealth = make_test_world(
        vec![make_test_agent(1, 0, 5.0, -10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert_eq!(
        phase8_welfare_distribution(&mut world_neg_wealth, 10.0, 20),
        Err(Phase8Error::NegativeAgentWealth {
            agent_id: AgentId(1),
            wealth: -10
        })
    );

    // 5. Negative treasury fails fast
    let mut world_neg_treasury = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, -50)],
    );
    assert_eq!(
        phase8_welfare_distribution(&mut world_neg_treasury, 10.0, 20),
        Err(Phase8Error::NegativeTreasury {
            group_id: GroupId(0),
            treasury: -50
        })
    );

    // 6. Invalid starvation threshold fails fast
    let mut world_valid = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 50)],
    );
    assert_eq!(
        phase8_welfare_distribution(&mut world_valid, 0.0, 20),
        Err(Phase8Error::InvalidStarvationThreshold(0.0))
    );
    assert_eq!(
        phase8_welfare_distribution(&mut world_valid, -5.0, 20),
        Err(Phase8Error::InvalidStarvationThreshold(-5.0))
    );

    // 7. Negative welfare payment fails fast
    assert_eq!(
        phase8_welfare_distribution(&mut world_valid, 10.0, -1),
        Err(Phase8Error::NegativeWelfarePayment(-1))
    );
}

// =============================================================================
// 2. Fully Funded Tests
// =============================================================================

#[test]
fn test_07_single_eligible_agent_full_payment() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 3.0, 5, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 30).unwrap();
    assert_eq!(res[0].eligible_count, 1);
    assert_eq!(res[0].payment_per_agent, 30);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 30);
    assert_eq!(res[0].treasury_before, 100);
    assert_eq!(res[0].treasury_after, 70);
    assert_eq!(
        res[0].recipients,
        vec![WelfareRecipientResolution {
            agent_id: AgentId(1),
            payout: 30
        }]
    );

    assert_eq!(world.agents[0].wealth, 35);
    assert_eq!(world.settlements[0].treasury, 70);
}

#[test]
fn test_08_multiple_eligible_agents_full_payment() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 3.0, 10, true, 1.0),
            make_test_agent(2, 0, 4.0, 20, true, 0.9),
            make_test_agent(3, 0, 5.0, 30, true, 0.8),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 25).unwrap();
    assert_eq!(res[0].eligible_count, 3);
    assert_eq!(res[0].payment_per_agent, 25);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 75);
    assert_eq!(res[0].treasury_after, 25);
    assert_eq!(res[0].recipients.len(), 3);
    for r in &res[0].recipients {
        assert_eq!(r.payout, 25);
    }

    assert_eq!(world.agents[0].wealth, 35);
    assert_eq!(world.agents[1].wealth, 45);
    assert_eq!(world.agents[2].wealth, 55);
    assert_eq!(world.settlements[0].treasury, 25);
}

#[test]
fn test_09_treasury_decreases_by_exact_total_payout() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 2.0, 0, true, 1.0),
            make_test_agent(3, 0, 2.0, 0, true, 1.0),
            make_test_agent(4, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 40).unwrap();
    assert_eq!(res[0].total_distributed, 160);
    assert_eq!(
        res[0].treasury_before - res[0].treasury_after,
        res[0].total_distributed
    );
    assert_eq!(world.settlements[0].treasury, 40);
}

#[test]
fn test_10_excess_treasury_remains_untouched() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 1.0, 0, true, 1.0),
            make_test_agent(2, 0, 1.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 1000)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(res[0].total_distributed, 100);
    assert_eq!(res[0].treasury_after, 900);
    assert_eq!(world.settlements[0].treasury, 900);
}

// =============================================================================
// 3. Underfunded Tests
// =============================================================================

#[test]
fn test_11_underfunded_treasury_evenly_divisible() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 2.0, 10, true, 1.0),
            make_test_agent(3, 0, 2.0, 10, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 30)],
    );

    // required = 3 * 20 = 60 > 30 => underfunded
    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].eligible_count, 3);
    assert_eq!(res[0].payment_per_agent, 10);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 30);
    assert_eq!(res[0].treasury_after, 0);

    for r in &res[0].recipients {
        assert_eq!(r.payout, 10);
    }
    for a in &world.agents {
        assert_eq!(a.wealth, 20);
    }
    assert_eq!(world.settlements[0].treasury, 0);
}

#[test]
fn test_12_underfunded_non_zero_remainder() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 2.0, 0, true, 1.0),
            make_test_agent(3, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 32)],
    );

    // required = 3 * 20 = 60 > 32 => underfunded
    // payment_per_agent = 32 / 3 = 10, remainder = 32 % 3 = 2
    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(res[0].payment_per_agent, 10);
    assert_eq!(res[0].remainder, 2);
    assert_eq!(res[0].total_distributed, 32);
    assert_eq!(res[0].treasury_after, 0);
    assert_eq!(world.settlements[0].treasury, 0);
}

#[test]
fn test_13_underfunded_remainder_to_lowest_ascending_agent_ids() {
    let mut world = make_test_world(
        vec![
            make_test_agent(10, 0, 2.0, 0, true, 1.0),
            make_test_agent(20, 0, 2.0, 0, true, 1.0),
            make_test_agent(30, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 32)],
    );

    // 3 eligible agents, treasury 32, remainder 2:
    // Agent 10 gets 10 + 1 = 11
    // Agent 20 gets 10 + 1 = 11
    // Agent 30 gets 10 + 0 = 10
    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    assert_eq!(
        res[0].recipients,
        vec![
            WelfareRecipientResolution {
                agent_id: AgentId(10),
                payout: 11
            },
            WelfareRecipientResolution {
                agent_id: AgentId(20),
                payout: 11
            },
            WelfareRecipientResolution {
                agent_id: AgentId(30),
                payout: 10
            },
        ]
    );

    assert_eq!(world.agents[0].wealth, 11);
    assert_eq!(world.agents[1].wealth, 11);
    assert_eq!(world.agents[2].wealth, 10);
    assert_eq!(world.settlements[0].treasury, 0);
}

#[test]
fn test_14_physical_input_order_does_not_affect_recipients() {
    // Agents stored in reverse order in WorldState: [30, 20, 10]
    let mut world = make_test_world(
        vec![
            make_test_agent(30, 0, 2.0, 0, true, 1.0),
            make_test_agent(20, 0, 2.0, 0, true, 1.0),
            make_test_agent(10, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 32)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    // Canonical resolution ordering: strictly ascending AgentId
    assert_eq!(res[0].recipients[0].agent_id, AgentId(10));
    assert_eq!(res[0].recipients[0].payout, 11);
    assert_eq!(res[0].recipients[1].agent_id, AgentId(20));
    assert_eq!(res[0].recipients[1].payout, 11);
    assert_eq!(res[0].recipients[2].agent_id, AgentId(30));
    assert_eq!(res[0].recipients[2].payout, 10);

    // Agents in storage get the correct payout regardless of their index
    let a10 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(10))
        .unwrap();
    let a20 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(20))
        .unwrap();
    let a30 = world
        .agents
        .iter()
        .find(|a| a.agent_id == AgentId(30))
        .unwrap();
    assert_eq!(a10.wealth, 11);
    assert_eq!(a20.wealth, 11);
    assert_eq!(a30.wealth, 10);
}

#[test]
fn test_15_underfunded_drains_treasury_exactly_to_zero() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 1.0, 0, true, 1.0),
            make_test_agent(2, 0, 1.0, 0, true, 1.0),
            make_test_agent(3, 0, 1.0, 0, true, 1.0),
            make_test_agent(4, 0, 1.0, 0, true, 1.0),
            make_test_agent(5, 0, 1.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 47)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(res[0].total_distributed, 47);
    assert_eq!(res[0].treasury_after, 0);
    assert_eq!(world.settlements[0].treasury, 0);

    let total_agent_payout: Money = world.agents.iter().map(|a| a.wealth).sum();
    assert_eq!(total_agent_payout, 47);
}

// =============================================================================
// 4. Edge Cases Tests
// =============================================================================

#[test]
fn test_16_zero_eligible_agents_zero_op() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 15.0, 10, true, 1.0),
            make_test_agent(2, 0, 20.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(res[0].eligible_count, 0);
    assert_eq!(res[0].payment_per_agent, 0);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(res[0].treasury_after, 500);
    assert!(res[0].recipients.is_empty());

    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, 20);
    assert_eq!(world.settlements[0].treasury, 500);
}

#[test]
fn test_17_zero_treasury_deterministic_zero_payouts() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 3.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(res[0].eligible_count, 2);
    assert_eq!(res[0].payment_per_agent, 0);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(res[0].treasury_after, 0);
    assert_eq!(res[0].recipients.len(), 2);
    assert_eq!(res[0].recipients[0].payout, 0);
    assert_eq!(res[0].recipients[1].payout, 0);

    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, 20);
    assert_eq!(world.settlements[0].treasury, 0);
}

#[test]
fn test_18_zero_welfare_payment_behavior() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 3.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 0).unwrap();
    assert_eq!(res[0].eligible_count, 2);
    assert_eq!(res[0].payment_per_agent, 0);
    assert_eq!(res[0].remainder, 0);
    assert_eq!(res[0].total_distributed, 0);
    assert_eq!(res[0].treasury_after, 500);
    assert_eq!(res[0].recipients.len(), 2);
    assert_eq!(res[0].recipients[0].payout, 0);
    assert_eq!(res[0].recipients[1].payout, 0);

    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, 20);
    assert_eq!(world.settlements[0].treasury, 500);
}

#[test]
fn test_19_welfare_credit_overflow_fails_before_mutation() {
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 2.0, Money::MAX - 5, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    // Payout of 10 would overflow Money::MAX
    let err = phase8_welfare_distribution(&mut world, 10.0, 10).unwrap_err();
    assert_eq!(err, Phase8Error::WealthOverflow(AgentId(1)));

    // World state remains unchanged
    assert_eq!(world.agents[0].wealth, Money::MAX - 5);
    assert_eq!(world.settlements[0].treasury, 100);
}

#[test]
fn test_20_multiplication_overflow_fails_before_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, Money::MAX)],
    );

    // 2 * (Money::MAX / 2 + 10) overflows Money
    let huge_payment = (Money::MAX / 2) + 10;
    let err = phase8_welfare_distribution(&mut world, 10.0, huge_payment).unwrap_err();
    assert_eq!(err, Phase8Error::FinancialOverflow);

    // World state remains unchanged
    assert_eq!(world.agents[0].wealth, 0);
    assert_eq!(world.agents[1].wealth, 0);
    assert_eq!(world.settlements[0].treasury, Money::MAX);
}

// =============================================================================
// 5. Conservation Tests
// =============================================================================

#[test]
fn test_21_settlement_treasury_before_equals_after_plus_payouts() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 5, true, 1.0),
            make_test_agent(2, 0, 3.0, 10, true, 1.0),
            make_test_agent(3, 0, 4.0, 15, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 77)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 30).unwrap();
    let s_res = &res[0];
    assert_eq!(
        s_res.treasury_before,
        s_res.treasury_after + s_res.total_distributed
    );
    let sum_payouts: Money = s_res.recipients.iter().map(|r| r.payout).sum();
    assert_eq!(sum_payouts, s_res.total_distributed);
}

#[test]
fn test_22_global_money_supply_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 3.0, 20, true, 1.0),
            make_test_agent(3, 1, 4.0, 30, true, 1.0),
            make_test_agent(4, 1, 15.0, 40, true, 1.0), // not eligible
        ],
        vec![
            make_test_settlement(0, 100.0, 150),
            make_test_settlement(1, 100.0, 25), // underfunded
        ],
    );

    let money_before = total_world_money(&world);
    assert_eq!(money_before, world.initial_money_supply);

    phase8_welfare_distribution(&mut world, 10.0, 30).unwrap();

    let money_after = total_world_money(&world);
    assert_eq!(money_after, money_before);
    assert_eq!(money_after, world.initial_money_supply);
}

#[test]
fn test_23_no_tax_withholding_or_treasury_recredit() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 30).unwrap();
    assert_eq!(res[0].total_distributed, 60);
    assert_eq!(res[0].treasury_after, 40);

    // Sum of agent wealth increases must EXACTLY equal treasury decrease (no tax deducted, no tax re-credit)
    let total_wealth_gain: Money = world.agents.iter().map(|a| a.wealth).sum();
    let treasury_loss = 100 - world.settlements[0].treasury;
    assert_eq!(total_wealth_gain, 60);
    assert_eq!(treasury_loss, 60);
}

// =============================================================================
// 6. Live-State Timing Tests
// =============================================================================

#[test]
fn test_24_phase7_food_purchase_makes_agent_ineligible_for_phase8() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 100, true, 1.0), // Buyer: food 5 < 10
            make_test_agent(2, 0, 20.0, 0, true, 1.0),  // Seller
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 6.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 6.0,
            },
        ],
    }];

    // Execute Phase 7 market clearance: food_price = 10, tax_rate = 0.0
    let mkt_res = phase7_market_clearance(&mut world, &partitions, 10, 0.0).unwrap();
    assert_eq!(mkt_res[0].total_sold, 6.0);

    // Buyer's food is now 5.0 + 6.0 = 11.0 >= 10.0
    assert_eq!(world.agents[0].food, 11.0);

    // Now execute Phase 8 welfare: starvation_threshold = 10.0
    let wlf_res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    // Buyer is NO LONGER eligible in Phase 8
    assert_eq!(wlf_res[0].eligible_count, 0);
    assert_eq!(wlf_res[0].total_distributed, 0);
    assert_eq!(world.settlements[0].treasury, 200); // treasury untouched by Phase 8
}

#[test]
fn test_25_phase7_food_purchase_failure_leaves_agent_eligible() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 5.0, 100, true, 1.0), // Buyer: food 5 < 10
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    // Buyer wants to buy, but there are zero sellers
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![Intent::BuyFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            requested_demand: 6.0,
        }],
    }];

    let mkt_res = phase7_market_clearance(&mut world, &partitions, 10, 0.0).unwrap();
    assert_eq!(mkt_res[0].total_sold, 0.0);
    assert_eq!(world.agents[0].food, 5.0);

    // Execute Phase 8 welfare: agent remains eligible!
    let wlf_res = phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(wlf_res[0].eligible_count, 1);
    assert_eq!(wlf_res[0].total_distributed, 50);
    assert_eq!(world.agents[0].wealth, 150);
    assert_eq!(world.settlements[0].treasury, 150);
}

#[test]
fn test_26_phase8_does_not_reopen_phase7() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    // In Phase 7, Buyer had 0 wealth so bought 0 food
    let partitions = vec![SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    }];

    phase7_market_clearance(&mut world, &partitions, 10, 0.0).unwrap();
    assert_eq!(world.agents[0].food, 2.0);
    assert_eq!(world.agents[0].wealth, 0);

    // Phase 8 disburses 50 welfare to Buyer
    phase8_welfare_distribution(&mut world, 10.0, 50).unwrap();
    assert_eq!(world.agents[0].wealth, 50);

    // Invariant: Phase 7 is NOT reopened. Buyer's food remains 2.0, Seller's food remains 20.0
    assert_eq!(world.agents[0].food, 2.0);
    assert_eq!(world.agents[1].food, 20.0);
    assert_eq!(world.settlements[0].treasury, 50);
}

#[test]
fn test_27_current_food_state_used_not_original_or_intent_state() {
    // Agent originally had high food, but before Phase 8 their live food was reduced to 3.0
    let mut world = make_test_world(
        vec![make_test_agent(1, 0, 3.0, 0, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 40).unwrap();
    assert_eq!(res[0].eligible_count, 1);
    assert_eq!(res[0].total_distributed, 40);
    assert_eq!(world.agents[0].wealth, 40);
}

// =============================================================================
// 7. Determinism and Isolation Tests
// =============================================================================

#[test]
fn test_28_reversed_shuffled_storage_produces_identical_payouts() {
    // Setup A: agents [10, 20, 30, 40]
    let mut world_a = make_test_world(
        vec![
            make_test_agent(10, 0, 2.0, 0, true, 1.0),
            make_test_agent(20, 0, 2.0, 0, true, 1.0),
            make_test_agent(30, 0, 2.0, 0, true, 1.0),
            make_test_agent(40, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 35)],
    );

    // Setup B: agents shuffled [30, 10, 40, 20]
    let mut world_b = make_test_world(
        vec![
            make_test_agent(30, 0, 2.0, 0, true, 1.0),
            make_test_agent(10, 0, 2.0, 0, true, 1.0),
            make_test_agent(40, 0, 2.0, 0, true, 1.0),
            make_test_agent(20, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 35)],
    );

    let res_a = phase8_welfare_distribution(&mut world_a, 10.0, 20).unwrap();
    let res_b = phase8_welfare_distribution(&mut world_b, 10.0, 20).unwrap();

    assert_eq!(res_a, res_b);

    // Check individual agent wealth in both worlds
    for aid in [10, 20, 30, 40] {
        let wa = world_a
            .agents
            .iter()
            .find(|a| a.agent_id == AgentId(aid))
            .unwrap()
            .wealth;
        let wb = world_b
            .agents
            .iter()
            .find(|a| a.agent_id == AgentId(aid))
            .unwrap()
            .wealth;
        assert_eq!(
            wa, wb,
            "agent {} wealth differs between storage layouts",
            aid
        );
    }
}

#[test]
fn test_29_canonical_eligible_agent_ordering() {
    let mut world = make_test_world(
        vec![
            make_test_agent(99, 0, 2.0, 0, true, 1.0),
            make_test_agent(3, 0, 2.0, 0, true, 1.0),
            make_test_agent(50, 0, 2.0, 0, true, 1.0),
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    let ids: Vec<u32> = res[0].recipients.iter().map(|r| r.agent_id.0).collect();
    assert_eq!(ids, vec![1, 3, 50, 99]);
}

#[test]
fn test_30_multiple_settlements_isolated_to_own_treasury() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 0, 2.0, 0, true, 1.0),
            make_test_agent(3, 1, 2.0, 0, true, 1.0),
            make_test_agent(4, 1, 2.0, 0, true, 1.0),
        ],
        vec![
            make_test_settlement(0, 100.0, 100), // fully funded for 2 * 30 = 60
            make_test_settlement(1, 100.0, 10),  // underfunded for 2 * 30 = 60
        ],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 30).unwrap();
    assert_eq!(res.len(), 2);

    // Settlement 0
    assert_eq!(res[0].group_id, GroupId(0));
    assert_eq!(res[0].total_distributed, 60);
    assert_eq!(res[0].treasury_after, 40);
    assert_eq!(res[0].recipients[0].payout, 30);
    assert_eq!(res[0].recipients[1].payout, 30);

    // Settlement 1
    assert_eq!(res[1].group_id, GroupId(1));
    assert_eq!(res[1].total_distributed, 10);
    assert_eq!(res[1].treasury_after, 0);
    assert_eq!(res[1].recipients[0].payout, 5);
    assert_eq!(res[1].recipients[1].payout, 5);

    assert_eq!(world.settlements[0].treasury, 40);
    assert_eq!(world.settlements[1].treasury, 0);
}

#[test]
fn test_31_settlement_processing_order_independence() {
    // World with settlements [1, 0]
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 0, true, 1.0),
            make_test_agent(2, 1, 2.0, 0, true, 1.0),
        ],
        vec![
            make_test_settlement(1, 100.0, 50),
            make_test_settlement(0, 100.0, 50),
        ],
    );

    let res = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap();
    // Returns canonically sorted by GroupId: [0, 1]
    assert_eq!(res[0].group_id, GroupId(0));
    assert_eq!(res[1].group_id, GroupId(1));
}

#[test]
fn test_32_zero_rng_consumption() {
    let make_world = || {
        make_test_world(
            vec![
                make_test_agent(1, 0, 2.0, 0, true, 1.0),
                make_test_agent(2, 0, 2.0, 0, true, 1.0),
                make_test_agent(3, 0, 2.0, 0, true, 1.0),
            ],
            vec![make_test_settlement(0, 100.0, 32)],
        )
    };

    let mut w1 = make_world();
    let mut w2 = make_world();

    let res1 = phase8_welfare_distribution(&mut w1, 10.0, 20).unwrap();
    let res2 = phase8_welfare_distribution(&mut w2, 10.0, 20).unwrap();

    assert_eq!(res1, res2);
    assert_eq!(w1, w2);
}

// =============================================================================
// 8. Atomicity Tests
// =============================================================================

#[test]
fn test_33_invalid_participant_or_state_fails_before_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, -2.0, 10, true, 1.0), // invalid negative food
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap_err();
    assert!(matches!(err, Phase8Error::InvalidAgentState { .. }));

    // World state must be entirely untouched
    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, 10);
    assert_eq!(world.settlements[0].treasury, 100);
}

#[test]
fn test_34_overflow_failure_leaves_world_state_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 2.0, Money::MAX - 5, true, 1.0), // will overflow
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let err = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap_err();
    assert_eq!(err, Phase8Error::WealthOverflow(AgentId(2)));

    // Neither agent 1 nor agent 2 nor settlement 0 mutated
    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.agents[1].wealth, Money::MAX - 5);
    assert_eq!(world.settlements[0].treasury, 100);
}

#[test]
fn test_35_multi_settlement_planning_failure_zero_partial_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0), // Settlement 0
            make_test_agent(2, 1, 2.0, Money::MAX - 5, true, 1.0), // Settlement 1 (will overflow)
        ],
        vec![
            make_test_settlement(0, 100.0, 100),
            make_test_settlement(1, 100.0, 100),
        ],
    );

    let err = phase8_welfare_distribution(&mut world, 10.0, 20).unwrap_err();
    assert_eq!(err, Phase8Error::WealthOverflow(AgentId(2)));

    // Settlement 0 must NOT have been mutated even though it preceded Settlement 1!
    assert_eq!(world.agents[0].wealth, 10);
    assert_eq!(world.settlements[0].treasury, 100);
    assert_eq!(world.agents[1].wealth, Money::MAX - 5);
    assert_eq!(world.settlements[1].treasury, 100);
}

// =============================================================================
// 9. Config and Command Integration Tests
// =============================================================================

const TEST_CONFIG_TOML: &str = r#"
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

#[test]
fn test_36_phase8_with_config_and_subconfigs_wrappers() {
    let cfg = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();

    let mut world1 = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res1 = phase8_welfare_distribution_with_config(&mut world1, &cfg).unwrap();
    assert_eq!(res1[0].total_distributed, 25);
    assert_eq!(world1.agents[0].wealth, 35);
    assert_eq!(world1.settlements[0].treasury, 75);

    let mut world2 = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let res2 =
        phase8_welfare_distribution_with_subconfigs(&mut world2, &cfg.interaction, &cfg.economy)
            .unwrap();
    assert_eq!(res2[0].total_distributed, 25);
    assert_eq!(world2.agents[0].wealth, 35);
    assert_eq!(world2.settlements[0].treasury, 75);

    let mut world3 = make_test_world(
        vec![make_test_agent(1, 0, 5.0, 10, true, 1.0)],
        vec![make_test_settlement(0, 100.0, 100)],
    );
    let res3 = phase8_welfare_resolution(&mut world3, 10.0, 25).unwrap();
    assert_eq!(res3, res1);
}

#[test]
fn test_37_command_welfare_distribution_direct_execution() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 2.0, 10, true, 1.0),
            make_test_agent(2, 0, 2.0, 20, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 100)],
    );

    let cmd = Command::WelfareDistribution {
        group_id: GroupId(0),
        recipient_updates: vec![
            WelfareRecipientUpdate {
                agent_id: AgentId(1),
                payout: 15,
            },
            WelfareRecipientUpdate {
                agent_id: AgentId(2),
                payout: 15,
            },
        ],
        treasury_debit: 30,
    };

    cmd.execute(&mut world).unwrap();
    assert_eq!(world.agents[0].wealth, 25);
    assert_eq!(world.agents[1].wealth, 35);
    assert_eq!(world.settlements[0].treasury, 70);

    // Negative treasury debit error
    let bad_cmd = Command::WelfareDistribution {
        group_id: GroupId(0),
        recipient_updates: vec![],
        treasury_debit: -1,
    };
    assert_eq!(
        bad_cmd.execute(&mut world),
        Err(CommandExecutionError::NegativeTreasuryDebit(-1))
    );

    // Insufficient treasury error
    let big_cmd = Command::WelfareDistribution {
        group_id: GroupId(0),
        recipient_updates: vec![WelfareRecipientUpdate {
            agent_id: AgentId(1),
            payout: 200,
        }],
        treasury_debit: 200,
    };
    assert_eq!(
        big_cmd.execute(&mut world),
        Err(CommandExecutionError::InsufficientTreasury {
            group_id: GroupId(0),
            treasury: 70,
            required: 200,
        })
    );
}
