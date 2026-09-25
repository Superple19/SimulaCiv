use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::{
    AgentState, Intent, Phase7Error, SettlementIntentPartition, SettlementState, SimConfig,
    WorldState, initialize_world, phase5_partition_intents, phase6b_targeted_resolution,
    phase7_market_clearance, phase7_market_clearance_with_config, phase7_market_resolution,
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

// -----------------------------------------------------------------------------
// 1. Core Clearance Tests
// -----------------------------------------------------------------------------

#[test]
fn test_01_sufficient_supply() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0), // buyer
            make_test_agent(1, 0, 20.0, 1000, true, 1.0), // seller 1
            make_test_agent(2, 0, 20.0, 1000, true, 1.0), // seller 2
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 15.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 15.0,
            },
        ],
    };

    let food_price: Money = 100;
    let tax_rate = 0.10f32; // 10%
    let res = phase7_market_clearance(&mut world, &[partition], food_price, tax_rate).unwrap();

    assert_eq!(res.len(), 1);
    let sres = &res[0];
    assert_eq!(sres.total_effective_demand, 10.0);
    assert_eq!(sres.total_effective_supply, 30.0);
    assert_eq!(sres.total_sold, 10.0);
    assert_eq!(sres.total_revenue, 1000);
    assert_eq!(sres.tax_withheld, 100);
    assert_eq!(sres.net_pool_proceeds, 900);
    assert_eq!(sres.proceeds_balance, 0);

    // Buyer 0 bought 10.0, debited 1000
    assert_eq!(sres.buyers[0].bought_units, 10.0);
    assert_eq!(sres.buyers[0].debit, 1000);

    // Sellers 1 & 2 each sold 5.0, netted 450
    assert_eq!(sres.sellers[0].sold_units, 5.0);
    assert_eq!(sres.sellers[0].seller_net, 450);
    assert_eq!(sres.sellers[1].sold_units, 5.0);
    assert_eq!(sres.sellers[1].seller_net, 450);

    // Verify world state updates
    assert_eq!(world.agents[0].food, 10.0);
    assert_eq!(world.agents[0].wealth, 9000);
    assert_eq!(world.agents[1].food, 15.0);
    assert_eq!(world.agents[1].wealth, 1450);
    assert_eq!(world.agents[2].food, 15.0);
    assert_eq!(world.agents[2].wealth, 1450);
    assert_eq!(world.settlements[0].treasury, 600);
}

#[test]
fn test_02_supply_deficit() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0), // buyer 1
            make_test_agent(1, 0, 0.0, 10000, true, 1.0), // buyer 2
            make_test_agent(2, 0, 20.0, 1000, true, 1.0), // seller
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
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
    };

    let food_price: Money = 100;
    let tax_rate = 0.10f32;
    let res = phase7_market_clearance(&mut world, &[partition], food_price, tax_rate).unwrap();

    let sres = &res[0];
    assert_eq!(sres.total_effective_demand, 20.0);
    assert_eq!(sres.total_effective_supply, 10.0);
    assert_eq!(sres.total_sold, 10.0);

    // Each buyer gets 5.0 food (half), debited 500
    assert_eq!(sres.buyers[0].bought_units, 5.0);
    assert_eq!(sres.buyers[0].debit, 500);
    assert_eq!(sres.buyers[1].bought_units, 5.0);
    assert_eq!(sres.buyers[1].debit, 500);

    // Seller sells all 10.0, nets 900
    assert_eq!(sres.sellers[0].sold_units, 10.0);
    assert_eq!(sres.sellers[0].seller_net, 900);

    assert_eq!(world.agents[0].food, 5.0);
    assert_eq!(world.agents[0].wealth, 9500);
    assert_eq!(world.agents[1].food, 5.0);
    assert_eq!(world.agents[1].wealth, 9500);
    assert_eq!(world.agents[2].food, 10.0);
    assert_eq!(world.agents[2].wealth, 1900);
    assert_eq!(world.settlements[0].treasury, 600);
}

#[test]
fn test_03_exact_supply_equals_demand() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 15.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 15.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 15.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.total_effective_supply, 15.0);
    assert_eq!(sres.total_effective_demand, 15.0);
    assert_eq!(sres.total_sold, 15.0);
    assert_eq!(sres.buyers[0].bought_units, 15.0);
    assert_eq!(sres.sellers[0].sold_units, 15.0);
    assert_eq!(sres.total_revenue, 1500);
    assert_eq!(sres.net_pool_proceeds, 1500);
    assert_eq!(world.agents[0].food, 15.0);
    assert_eq!(world.agents[1].food, 0.0);
}

#[test]
fn test_04_zero_supply() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 0.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 0.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.1).unwrap();
    let sres = &res[0];
    assert_eq!(sres.total_effective_supply, 0.0);
    assert_eq!(sres.total_sold, 0.0);
    assert_eq!(sres.total_revenue, 0);
    assert_eq!(sres.tax_withheld, 0);
    assert_eq!(sres.net_pool_proceeds, 0);
    assert_eq!(sres.buyers[0].bought_units, 0.0);
    assert_eq!(sres.buyers[0].debit, 0);
    assert_eq!(sres.sellers[0].sold_units, 0.0);
    assert_eq!(sres.sellers[0].seller_net, 0);

    // State completely unchanged
    assert_eq!(world.agents[0].food, 0.0);
    assert_eq!(world.agents[0].wealth, 10000);
    assert_eq!(world.agents[1].food, 0.0);
    assert_eq!(world.settlements[0].treasury, 200);
}

#[test]
fn test_05_zero_demand() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 0.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.1).unwrap();
    let sres = &res[0];
    assert_eq!(sres.total_effective_demand, 0.0);
    assert_eq!(sres.total_sold, 0.0);
    assert_eq!(sres.total_revenue, 0);
    assert_eq!(world.agents[1].food, 20.0);
}

// -----------------------------------------------------------------------------
// 2. Live-State Reconciliation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_06_seller_submitted_more_than_remains_after_phase6() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 4.0, 1000, true, 1.0), // seller only has 4.0 live food
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0, // submitted 10.0 originally
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.sellers[0].submitted_units, 10.0);
    assert_eq!(sres.sellers[0].effective_units, 4.0);
    assert_eq!(sres.sellers[0].sold_units, 4.0);
    assert_eq!(world.agents[1].food, 0.0);
}

#[test]
fn test_07_live_reconciliation_prevents_negative_seller_food() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 3.5, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 100.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 50.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].sellers[0].sold_units, 3.5);
    assert_eq!(world.agents[1].food, 0.0);
    assert!(world.agents[1].food >= 0.0);
}

#[test]
fn test_08_original_sellfood_intent_is_not_behaviorally_recomputed() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 8.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 6.0, // submitted 6.0, live food is 8.0
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].sellers[0].effective_units, 6.0);
    assert_eq!(res[0].sellers[0].sold_units, 6.0);
    assert_eq!(world.agents[1].food, 2.0);
}

// -----------------------------------------------------------------------------
// 3. Buyer Affordability Tests
// -----------------------------------------------------------------------------

#[test]
fn test_09_fully_affordable_buyer() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 5000, true, 1.0), // 5000 wealth
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0, // cost = 500 <= 5000
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].buyers[0].max_affordable_units, 50.0);
    assert_eq!(res[0].buyers[0].effective_units, 5.0);
    assert_eq!(res[0].buyers[0].bought_units, 5.0);
}

#[test]
fn test_10_partially_bounded_by_integer_affordable_units() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 350, true, 1.0), // 350 wealth, food_price = 100
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0, // wants 5.0, can only afford 3
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].buyers[0].max_affordable_units, 3.0);
    assert_eq!(res[0].buyers[0].effective_units, 3.0);
    assert_eq!(res[0].buyers[0].bought_units, 3.0);
    assert_eq!(res[0].buyers[0].debit, 300);
    assert_eq!(world.agents[0].wealth, 50);
}

#[test]
fn test_11_wealth_less_than_food_price_zero_effective_demand() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 99, true, 1.0), // wealth = 99 < food_price = 100
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].buyers[0].max_affordable_units, 0.0);
    assert_eq!(res[0].buyers[0].effective_units, 0.0);
    assert_eq!(res[0].total_effective_demand, 0.0);
    assert_eq!(res[0].total_sold, 0.0);
    assert_eq!(world.agents[0].wealth, 99);
}

#[test]
fn test_12_buyer_wealth_never_becomes_negative() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 100, true, 1.0),
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].buyers[0].debit, 100);
    assert_eq!(world.agents[0].wealth, 0);
    assert!(world.agents[0].wealth >= 0);
}

// -----------------------------------------------------------------------------
// 4. Numeric Semantics Tests
// -----------------------------------------------------------------------------

#[test]
fn test_13_canonical_sequential_f32_totals() {
    let mut world = make_test_world(
        vec![
            make_test_agent(1, 0, 10.0, 1000, true, 1.0),
            make_test_agent(2, 0, 10.0, 1000, true, 1.0),
            make_test_agent(3, 0, 10.0, 1000, true, 1.0),
            make_test_agent(4, 0, 0.0, 10000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1.1,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 2.2,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 3.3,
            },
            Intent::BuyFood {
                agent_id: AgentId(4),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let expected_seq = 1.1f32 + 2.2f32 + 3.3f32;
    assert_eq!(
        res[0].total_effective_supply.to_bits(),
        expected_seq.to_bits()
    );
}

#[test]
fn test_14_exact_sufficient_supply_operand_order() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
            make_test_agent(2, 0, 30.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 13.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 20.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let total_sup = 10.0f32 + 20.0f32;
    let total_dem = 13.0f32;
    let expected_sold1 = (10.0f32 / total_sup) * total_dem;
    let expected_sold2 = (20.0f32 / total_sup) * total_dem;
    assert_eq!(
        res[0].sellers[0].sold_units.to_bits(),
        expected_sold1.to_bits()
    );
    assert_eq!(
        res[0].sellers[1].sold_units.to_bits(),
        expected_sold2.to_bits()
    );
}

#[test]
fn test_15_exact_deficit_operand_order() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 0.0, 10000, true, 1.0),
            make_test_agent(2, 0, 10.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 11.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 19.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let total_dem = 11.0f32 + 19.0f32;
    let total_sup = 10.0f32;
    let expected_bought0 = (11.0f32 / total_dem) * total_sup;
    let expected_bought1 = (19.0f32 / total_dem) * total_sup;
    assert_eq!(
        res[0].buyers[0].bought_units.to_bits(),
        expected_bought0.to_bits()
    );
    assert_eq!(
        res[0].buyers[1].bought_units.to_bits(),
        expected_bought1.to_bits()
    );
}

#[test]
fn test_16_buyer_f32_to_f64_to_money_floor_boundary() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 3.3333333,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let gross_f64 = (3.3333333f32 as f64) * 100.0f64;
    let expected_debit = gross_f64.floor() as Money;
    assert_eq!(res[0].buyers[0].debit, expected_debit);
    assert_eq!(res[0].buyers[0].debit, 333);
}

#[test]
fn test_17_tax_f64_floor_boundary() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 3.334,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.15).unwrap();
    assert_eq!(res[0].total_revenue, 333);
    assert_eq!(res[0].tax_withheld, 49);
    assert_eq!(res[0].net_pool_proceeds, 284);
}

#[test]
fn test_18_seller_f32_share_to_f64_money_boundary() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 1000, true, 1.0),
            make_test_agent(2, 0, 10.0, 1000, true, 1.0),
            make_test_agent(3, 0, 10.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 3.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.total_revenue, 300);
    assert_eq!(sres.net_pool_proceeds, 300);
    for s in &sres.sellers {
        assert_eq!(s.seller_share_f32, 1.0 / 3.0);
        assert_eq!(s.seller_net_base, 100);
    }
}

// -----------------------------------------------------------------------------
// 5. Signed Proceeds Reconciliation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_19_positive_proceeds_balance() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 0, true, 1.0),
            make_test_agent(2, 0, 10.0, 0, true, 1.0),
            make_test_agent(3, 0, 10.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.net_pool_proceeds, 100);
    assert_eq!(sres.proceeds_balance, 1);
    assert_eq!(sres.sellers[0].seller_net_base, 33);
    assert_eq!(sres.sellers[0].seller_net, 34); // got +1
    assert_eq!(sres.sellers[1].seller_net_base, 33);
    assert_eq!(sres.sellers[1].seller_net, 33);
    assert_eq!(sres.sellers[2].seller_net_base, 33);
    assert_eq!(sres.sellers[2].seller_net, 33);
}

#[test]
fn test_20_zero_proceeds_balance() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 0, true, 1.0),
            make_test_agent(2, 0, 10.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.net_pool_proceeds, 100);
    assert_eq!(sres.proceeds_balance, 0);
    assert_eq!(sres.sellers[0].seller_net, 50);
    assert_eq!(sres.sellers[1].seller_net, 50);
}

#[test]
fn test_21_negative_proceeds_balance_regression() {
    // Exact mathematical regression:
    // With 3 sellers having equal shares:
    // seller_share_f32 = 0.3333333432674408 (which is > 1/3)
    // When net_pool_proceeds = 33,554,432:
    // seller_base_f64 = 33554432 * 0.3333333432674408 = 11184811.00000008
    // seller_net_base = floor(11184811.00000008) = 11,184,811
    // seller_base_total = 11184811 * 3 = 33,554,433
    // proceeds_balance = 33,554,432 - 33,554,433 = -1!
    // Negative reconciliation cycles through participating sellers in ascending AgentId:
    // Seller 1 (AgentId 1) has net 11184811 > 0, decrements by 1 to 11,184,810.
    // Balance reaches 0.
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 50_000_000, true, 1.0),
            make_test_agent(1, 0, 10.0, 0, true, 1.0),
            make_test_agent(2, 0, 10.0, 0, true, 1.0),
            make_test_agent(3, 0, 10.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let food_price: Money = 33_554_432;
    let res = phase7_market_clearance(&mut world, &[partition], food_price, 0.0).unwrap();
    let sres = &res[0];

    assert_eq!(sres.net_pool_proceeds, 33_554_432);
    assert_eq!(sres.proceeds_balance, -1); // initial balance was negative!

    // Base proceeds were 11,184,811 each
    assert_eq!(sres.sellers[0].seller_net_base, 11_184_811);
    assert_eq!(sres.sellers[1].seller_net_base, 11_184_811);
    assert_eq!(sres.sellers[2].seller_net_base, 11_184_811);

    // Negative reconciliation decremented S1 (lowest AgentId) by 1
    assert_eq!(sres.sellers[0].seller_net, 11_184_810);
    assert_eq!(sres.sellers[1].seller_net, 11_184_811);
    assert_eq!(sres.sellers[2].seller_net, 11_184_811);

    // Exact financial conservation
    let total_net: Money = sres.sellers.iter().map(|s| s.seller_net).sum();
    assert_eq!(total_net, 33_554_432);
    assert_eq!(sres.total_revenue, total_net + sres.tax_withheld);
}

#[test]
fn test_22_zero_payout_seller_skipped_during_negative_reconciliation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 50_000_000, true, 1.0),
            make_test_agent(1, 0, 1e-9, 0, true, 1.0), // tiny supply -> base = 0
            make_test_agent(2, 0, 1.0, 0, true, 1.0),
            make_test_agent(3, 0, 1.0, 0, true, 1.0),
            make_test_agent(4, 0, 1.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 1e-9,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(4),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let food_price: Money = 33_554_432;
    let res = phase7_market_clearance(&mut world, &[partition], food_price, 0.0).unwrap();
    let sres = &res[0];

    assert_eq!(sres.sellers[0].seller_net_base, 0);
    assert_eq!(sres.sellers[0].seller_net, 0);
}

#[test]
fn test_23_participating_sellers_only() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 0.0, 0, true, 1.0), // sold 0
            make_test_agent(2, 0, 10.0, 0, true, 1.0), // sold > 0
            make_test_agent(3, 0, 10.0, 0, true, 1.0), // sold > 0
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 0.0, // non-participating
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 1.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 99, 0.0).unwrap();
    let sres = &res[0];
    assert_eq!(sres.sellers[0].seller_net, 0); // S1 gets 0
    assert_eq!(sres.sellers[1].seller_net, 50); // S2 gets 49 + 1 = 50
    assert_eq!(sres.sellers[2].seller_net, 49); // S3 gets 49
}

#[test]
fn test_24_exact_sum_seller_payouts_equals_net_pool_proceeds() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 0, true, 1.0),
            make_test_agent(2, 0, 10.0, 0, true, 1.0),
            make_test_agent(3, 0, 10.0, 0, true, 1.0),
            make_test_agent(4, 0, 10.0, 0, true, 1.0),
            make_test_agent(5, 0, 10.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 7.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 2.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 3.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(4),
                group_id: GroupId(0),
                submitted_supply: 7.0,
            },
            Intent::SellFood {
                agent_id: AgentId(5),
                group_id: GroupId(0),
                submitted_supply: 11.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 123, 0.17).unwrap();
    let sres = &res[0];
    let sum_payouts: Money = sres.sellers.iter().map(|s| s.seller_net).sum();
    assert_eq!(sum_payouts, sres.net_pool_proceeds);
}

#[test]
fn test_25_exact_total_revenue_equals_seller_payouts_plus_tax() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 0.0, 10000, true, 1.0),
            make_test_agent(2, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.5,
            },
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 4.5,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 15.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 147, 0.13).unwrap();
    let sres = &res[0];
    let sum_payouts: Money = sres.sellers.iter().map(|s| s.seller_net).sum();
    assert_eq!(sres.total_revenue, sum_payouts + sres.tax_withheld);
}

// -----------------------------------------------------------------------------
// 6. Determinism & Isolation Tests
// -----------------------------------------------------------------------------

#[test]
fn test_26_input_order_independence() {
    let partition_forward = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 4.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                requested_demand: 6.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(4),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let partition_shuffled = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::SellFood {
                agent_id: AgentId(4),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                requested_demand: 6.0,
            },
            Intent::SellFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                requested_demand: 4.0,
            },
        ],
    };

    let mut world1 = make_test_world(
        vec![
            make_test_agent(1, 0, 0.0, 5000, true, 1.0),
            make_test_agent(2, 0, 20.0, 1000, true, 1.0),
            make_test_agent(3, 0, 0.0, 5000, true, 1.0),
            make_test_agent(4, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );
    let mut world2 = world1.clone();

    let res1 = phase7_market_clearance(&mut world1, &[partition_forward], 100, 0.1).unwrap();
    let res2 = phase7_market_clearance(&mut world2, &[partition_shuffled], 100, 0.1).unwrap();

    assert_eq!(res1, res2);
    assert_eq!(world1, world2);
}

#[test]
fn test_27_canonical_buyer_ordering() {
    let mut world = make_test_world(
        vec![
            make_test_agent(10, 0, 0.0, 5000, true, 1.0),
            make_test_agent(2, 0, 0.0, 5000, true, 1.0),
            make_test_agent(5, 0, 0.0, 5000, true, 1.0),
            make_test_agent(1, 0, 20.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(10),
                group_id: GroupId(0),
                requested_demand: 1.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(2),
                group_id: GroupId(0),
                requested_demand: 2.0,
            },
            Intent::BuyFood {
                agent_id: AgentId(5),
                group_id: GroupId(0),
                requested_demand: 3.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].buyers[0].agent_id, AgentId(2));
    assert_eq!(res[0].buyers[1].agent_id, AgentId(5));
    assert_eq!(res[0].buyers[2].agent_id, AgentId(10));
}

#[test]
fn test_28_canonical_seller_ordering() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(9, 0, 10.0, 1000, true, 1.0),
            make_test_agent(3, 0, 10.0, 1000, true, 1.0),
            make_test_agent(7, 0, 10.0, 1000, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(9),
                group_id: GroupId(0),
                submitted_supply: 2.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(0),
                submitted_supply: 2.0,
            },
            Intent::SellFood {
                agent_id: AgentId(7),
                group_id: GroupId(0),
                submitted_supply: 2.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[partition], 100, 0.0).unwrap();
    assert_eq!(res[0].sellers[0].agent_id, AgentId(3));
    assert_eq!(res[0].sellers[1].agent_id, AgentId(7));
    assert_eq!(res[0].sellers[2].agent_id, AgentId(9));
}

#[test]
fn test_29_multiple_settlements_remain_isolated() {
    let mut world = make_test_world(
        vec![
            // Settlement 0
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
            // Settlement 1
            make_test_agent(2, 1, 0.0, 10000, true, 1.0),
            make_test_agent(3, 1, 20.0, 0, true, 1.0),
        ],
        vec![
            make_test_settlement(0, 100.0, 100),
            make_test_settlement(1, 100.0, 200),
        ],
    );

    let p0 = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    let p1 = SettlementIntentPartition {
        group_id: GroupId(1),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(2),
                group_id: GroupId(1),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(1),
                submitted_supply: 5.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[p0, p1], 100, 0.1).unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].group_id, GroupId(0));
    assert_eq!(res[0].tax_withheld, 100);
    assert_eq!(res[1].group_id, GroupId(1));
    assert_eq!(res[1].tax_withheld, 50);

    // Settlement 0 treasury: 100 + 100 = 200
    assert_eq!(world.settlements[0].treasury, 200);
    // Settlement 1 treasury: 200 + 50 = 250
    assert_eq!(world.settlements[1].treasury, 250);
}

#[test]
fn test_30_phase7_consumes_no_rng() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let p = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    };

    let res = phase7_market_clearance(&mut world, &[p], 100, 0.0).unwrap();
    assert_eq!(res[0].total_sold, 5.0);
}

// -----------------------------------------------------------------------------
// 7. Atomicity & Failures Tests
// -----------------------------------------------------------------------------

#[test]
fn test_31_invalid_non_finite_market_quantity_fails_before_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );
    let initial_world = world.clone();

    let p = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: f32::NAN,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    };

    let err = phase7_market_clearance(&mut world, &[p], 100, 0.0).unwrap_err();
    match err {
        Phase7Error::InvalidRequestedDemand { agent_id, .. } => {
            assert_eq!(agent_id, AgentId(0));
        }
        _ => panic!("unexpected error variant: {:?}", err),
    }
    assert_eq!(world, initial_world);
}

#[test]
fn test_32_checked_arithmetic_overflow_fails_before_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, Money::MAX, true, 1.0),
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, Money::MAX)],
    );
    let initial_world = world.clone();

    let p = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 10.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 10.0,
            },
        ],
    };

    // Treasury is at Money::MAX, withholding tax will overflow treasury!
    let err = phase7_market_clearance(&mut world, &[p], 100, 0.1).unwrap_err();
    assert_eq!(err, Phase7Error::FinancialOverflow);
    assert_eq!(world, initial_world);
}

#[test]
fn test_33_structural_invalid_state_fails_before_mutation() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, false, 0.0), // dead/ineligible agent
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );
    let initial_world = world.clone();

    let p = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    };

    let err = phase7_market_clearance(&mut world, &[p], 100, 0.0).unwrap_err();
    assert_eq!(err, Phase7Error::IneligibleParticipant(AgentId(0)));
    assert_eq!(world, initial_world);
}

#[test]
fn test_34_failed_settlement_plan_leaves_all_settlements_unchanged() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 20.0, 0, true, 1.0),
            make_test_agent(2, 1, 0.0, 10000, false, 0.0), // ineligible in settlement 1
            make_test_agent(3, 1, 20.0, 0, true, 1.0),
        ],
        vec![
            make_test_settlement(0, 100.0, 0),
            make_test_settlement(1, 100.0, 0),
        ],
    );
    let initial_world = world.clone();

    let p0 = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    };

    let p1 = SettlementIntentPartition {
        group_id: GroupId(1),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(2),
                group_id: GroupId(1),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(3),
                group_id: GroupId(1),
                submitted_supply: 5.0,
            },
        ],
    };

    let err = phase7_market_clearance(&mut world, &[p0, p1], 100, 0.0).unwrap_err();
    assert_eq!(err, Phase7Error::IneligibleParticipant(AgentId(2)));
    assert_eq!(world, initial_world);
}

// -----------------------------------------------------------------------------
// 8. Pipeline Integration Tests
// -----------------------------------------------------------------------------

const INTEGRATION_TOML: &str = r#"
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
fn test_35_phase6_live_state_to_phase7_reconciliation_integration() {
    let config = SimConfig::parse_and_validate(INTEGRATION_TOML).unwrap();
    let mut world = initialize_world(&config).unwrap();

    // Seller (Agent 0) has 10.0 food initially.
    // In Phase 6B, Agent 2 steals 6.0 food from Agent 0.
    let p6b_intents = vec![Intent::StealFood {
        agent_id: AgentId(2),
        group_id: GroupId(0),
        target_agent_id: Some(AgentId(0)),
        requested_amount: 6.0,
    }];
    let p6b_partitions = phase5_partition_intents(&p6b_intents).unwrap();
    phase6b_targeted_resolution(&mut world, &config, &p6b_partitions).unwrap();

    let agent0_food_after_theft = world.agents[0].food;

    // Agent 0 had submitted SellFood for 10.0 in Phase 4
    let p7_intents = vec![
        Intent::SellFood {
            agent_id: AgentId(0),
            group_id: GroupId(0),
            submitted_supply: 10.0,
        },
        Intent::BuyFood {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            requested_demand: 10.0,
        },
    ];
    let p7_partitions = phase5_partition_intents(&p7_intents).unwrap();
    let p7_res =
        phase7_market_clearance_with_config(&mut world, &p7_partitions, &config.economy).unwrap();

    assert_eq!(
        p7_res[0].sellers[0].effective_units,
        agent0_food_after_theft
    );
    assert!(world.agents[0].food >= 0.0);
}

#[test]
fn test_36_existing_phases_and_convenience_alias() {
    let mut world = make_test_world(
        vec![
            make_test_agent(0, 0, 0.0, 10000, true, 1.0),
            make_test_agent(1, 0, 10.0, 0, true, 1.0),
        ],
        vec![make_test_settlement(0, 100.0, 0)],
    );

    let partition = SettlementIntentPartition {
        group_id: GroupId(0),
        intents: vec![
            Intent::BuyFood {
                agent_id: AgentId(0),
                group_id: GroupId(0),
                requested_demand: 5.0,
            },
            Intent::SellFood {
                agent_id: AgentId(1),
                group_id: GroupId(0),
                submitted_supply: 5.0,
            },
        ],
    };

    let res = phase7_market_resolution(&mut world, &[partition], 100, 0.1).unwrap();
    assert_eq!(res[0].total_sold, 5.0);
}
