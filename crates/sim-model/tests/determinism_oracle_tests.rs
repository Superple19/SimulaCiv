//! Acceptance test suite for M0-15: Canonical Determinism Oracles (Contracts C03, C07, C09, C10 / §19.1).

use sha2::{Digest, Sha256};
use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::events::{
    Event, EventBuffer, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    StateTransitionEvent, event_from_daily_metrics, event_from_snapshot_emission,
    events_from_market_resolution, events_from_mortality_resolution,
    events_from_welfare_resolution, phase11_flush_events,
};
use sim_model::hashing::{
    CanonicalHash, CanonicalHashError, DOMAIN_EVENTS, DOMAIN_METRICS, DOMAIN_STATE,
    EVENT_CATEGORY_STATE_TRANSITION, STATE_EVENT_MORTALITY_COMMITTED, canonical_event_bytes,
    canonical_event_hash, canonical_metrics_bytes, canonical_metrics_hash, canonical_state_bytes,
    canonical_state_hash,
};
use sim_model::metrics::DailyMetrics;
use sim_model::phases::Phase9MortalityResolution;
use sim_model::resolution::{
    SettlementMarketResolution, SettlementWelfareResolution, TargetedActionKind,
    WelfareRecipientResolution,
};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot, restore_snapshot};
use sim_model::state::{AgentState, SettlementState, WorldState};

#[allow(clippy::too_many_arguments)]
fn make_test_agent(
    id: u32,
    slot: u32,
    alive: bool,
    birth_day: u32,
    health: f32,
    food: f32,
    wealth: Money,
    productivity: f32,
    cooperation: f32,
    aggression: f32,
    risk_tolerance: f32,
    group_id: u16,
) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(slot),
        alive,
        birth_day: SimulationDay(birth_day),
        health,
        food,
        wealth,
        productivity,
        cooperation,
        aggression,
        risk_tolerance,
        group_id: GroupId(group_id),
    }
}

fn make_test_settlement(group_id: u16, resource: f32, treasury: Money) -> SettlementState {
    SettlementState {
        group_id: GroupId(group_id),
        resource,
        treasury,
    }
}

fn make_test_world(
    day: u32,
    agents: Vec<AgentState>,
    settlements: Vec<SettlementState>,
) -> WorldState {
    let mut initial_money_supply: Money = 0;
    for a in &agents {
        initial_money_supply = initial_money_supply.saturating_add(a.wealth);
    }
    for s in &settlements {
        initial_money_supply = initial_money_supply.saturating_add(s.treasury);
    }

    WorldState {
        current_day: SimulationDay(day),
        agents,
        settlements,
        initial_money_supply,
    }
}

// =========================================================================
// Group 1: SHA-256 Sanity (Tests 1–3)
// =========================================================================

#[test]
fn test_01_sha256_standard_vectors() {
    // NIST Standard SHA-256 test vector 1: empty input
    let hash_empty = Sha256::digest(b"");
    assert_eq!(
        format!("{:x}", hash_empty),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    // NIST Standard SHA-256 test vector 2: "abc"
    let hash_abc = Sha256::digest(b"abc");
    assert_eq!(
        format!("{:x}", hash_abc),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn test_02_sha256_same_bytes_identical_hash() {
    let payload = b"SimulaCiv M0 Determinism Oracle Test Vector";
    let h1 = CanonicalHash(Sha256::digest(payload).into());
    let h2 = CanonicalHash(Sha256::digest(payload).into());
    assert_eq!(h1, h2);
    assert_eq!(h1.to_hex(), h2.to_hex());
}

#[test]
fn test_03_sha256_one_byte_difference() {
    let b1 = b"SimulaCiv M0 Determinism Oracle Test Vector 1";
    let b2 = b"SimulaCiv M0 Determinism Oracle Test Vector 2";
    let h1 = CanonicalHash(Sha256::digest(b1).into());
    let h2 = CanonicalHash(Sha256::digest(b2).into());
    assert_ne!(h1, h2);
    assert_ne!(h1.to_hex(), h2.to_hex());
}

// =========================================================================
// Group 2: State Bytes & Hash (Tests 4–18)
// =========================================================================

#[test]
fn test_04_state_bytes_empty_world() {
    let world = make_test_world(0, vec![], vec![]);
    let bytes = canonical_state_bytes(&world).expect("empty world encoding should succeed");

    // Header: "SIMCIV_STATE_V1" (15) + current_day (4) + agent_count (4) + settlement_count (4) = 27
    assert_eq!(bytes.len(), 27);
    assert_eq!(&bytes[0..15], DOMAIN_STATE);
    assert_eq!(&bytes[15..19], &0u32.to_le_bytes()); // current_day
    assert_eq!(&bytes[19..23], &0u32.to_le_bytes()); // agent_count
    assert_eq!(&bytes[23..27], &0u32.to_le_bytes()); // settlement_count

    let hash = canonical_state_hash(&world).expect("empty world hash should succeed");
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_05_state_bytes_identical_worlds() {
    let w1 = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let w2 = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let b1 = canonical_state_bytes(&w1).unwrap();
    let b2 = canonical_state_bytes(&w2).unwrap();
    assert_eq!(b1, b2);

    let h1 = canonical_state_hash(&w1).unwrap();
    let h2 = canonical_state_hash(&w2).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_06_state_bytes_shuffled_agents() {
    let a1 = make_test_agent(10, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a2 = make_test_agent(20, 1, true, 0, 0.8, 8.0, 40, 0.9, 0.4, 0.2, 0.2, 0);
    let a3 = make_test_agent(30, 2, true, 0, 0.6, 6.0, 30, 0.8, 0.3, 0.3, 0.3, 0);

    let w_ordered = make_test_world(
        1,
        vec![a1.clone(), a2.clone(), a3.clone()],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let w_shuffled = make_test_world(
        1,
        vec![a3, a1, a2],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    assert_eq!(
        canonical_state_bytes(&w_ordered).unwrap(),
        canonical_state_bytes(&w_shuffled).unwrap()
    );
    assert_eq!(
        canonical_state_hash(&w_ordered).unwrap(),
        canonical_state_hash(&w_shuffled).unwrap()
    );
}

#[test]
fn test_07_state_bytes_shuffled_settlements() {
    let s1 = make_test_settlement(1, 100.0, 200);
    let s2 = make_test_settlement(2, 200.0, 400);

    let w_ordered = make_test_world(1, vec![], vec![s1.clone(), s2.clone()]);
    let w_reversed = make_test_world(1, vec![], vec![s2, s1]);

    assert_eq!(
        canonical_state_bytes(&w_ordered).unwrap(),
        canonical_state_bytes(&w_reversed).unwrap()
    );
    assert_eq!(
        canonical_state_hash(&w_ordered).unwrap(),
        canonical_state_hash(&w_reversed).unwrap()
    );
}

#[test]
fn test_08_state_bytes_dense_slot_invariance() {
    // DenseSlot changes must NOT change canonical bytes or hash
    let a_slot0 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a_slot999 = make_test_agent(1, 999, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);

    let w1 = make_test_world(1, vec![a_slot0], vec![]);
    let w2 = make_test_world(1, vec![a_slot999], vec![]);

    assert_eq!(
        canonical_state_bytes(&w1).unwrap(),
        canonical_state_bytes(&w2).unwrap()
    );
    assert_eq!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_09_state_hash_current_day_difference() {
    let w1 = make_test_world(1, vec![], vec![]);
    let w2 = make_test_world(2, vec![], vec![]);

    assert_ne!(
        canonical_state_bytes(&w1).unwrap(),
        canonical_state_bytes(&w2).unwrap()
    );
    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_10_state_hash_alive_difference() {
    let a_alive = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a_dead = make_test_agent(1, 0, false, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);

    let w1 = make_test_world(1, vec![a_alive], vec![]);
    let w2 = make_test_world(1, vec![a_dead], vec![]);

    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_11_state_hash_health_bit_difference() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let mut bits = 1.0f32.to_bits();
    bits ^= 1; // flip 1 bit
    let a2 = make_test_agent(
        1,
        0,
        true,
        0,
        f32::from_bits(bits),
        10.0,
        50,
        1.0,
        0.5,
        0.1,
        0.1,
        0,
    );

    let w1 = make_test_world(1, vec![a1], vec![]);
    let w2 = make_test_world(1, vec![a2], vec![]);

    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_12_state_hash_wealth_difference() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a2 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 51, 1.0, 0.5, 0.1, 0.1, 0);

    let w1 = make_test_world(1, vec![a1], vec![]);
    let w2 = make_test_world(1, vec![a2], vec![]);

    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_13_state_hash_settlement_resource_difference() {
    let w1 = make_test_world(1, vec![], vec![make_test_settlement(0, 100.0, 50)]);
    let w2 = make_test_world(1, vec![], vec![make_test_settlement(0, 100.1, 50)]);

    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_14_state_hash_settlement_treasury_difference() {
    let w1 = make_test_world(1, vec![], vec![make_test_settlement(0, 100.0, 50)]);
    let w2 = make_test_world(1, vec![], vec![make_test_settlement(0, 100.0, 51)]);

    assert_ne!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_15_state_hash_duplicate_agent_rejected() {
    let a1 = make_test_agent(5, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a2 = make_test_agent(5, 1, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);

    let world = make_test_world(1, vec![a1, a2], vec![]);
    let err = canonical_state_bytes(&world).expect_err("duplicate AgentId must fail");
    assert_eq!(err, CanonicalHashError::DuplicateAgent(AgentId(5)));
}

#[test]
fn test_16_state_hash_duplicate_settlement_rejected() {
    let s1 = make_test_settlement(3, 100.0, 50);
    let s2 = make_test_settlement(3, 200.0, 60);

    let world = make_test_world(1, vec![], vec![s1, s2]);
    let err = canonical_state_bytes(&world).expect_err("duplicate GroupId must fail");
    assert_eq!(err, CanonicalHashError::DuplicateSettlement(GroupId(3)));
}

#[test]
fn test_17_state_hashing_does_not_mutate_world() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world_before = world.clone();

    let _ = canonical_state_bytes(&world).unwrap();
    let _ = canonical_state_hash(&world).unwrap();

    assert_eq!(world, world_before);
}

#[test]
fn test_18_state_hash_snapshot_restore_equivalence() {
    let world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(2, 1, false, 0, 0.0, 0.0, 10, 0.8, 0.3, 0.2, 0.1, 0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let meta = SnapshotMetadata::new(1, 42, 1, "0.1.0", "1.0");
    let bytes = encode_snapshot(&world, &meta).expect("snapshot encode succeeds");
    let restored = restore_snapshot(&bytes).expect("snapshot restore succeeds");

    let h_original = canonical_state_hash(&world).unwrap();
    let h_restored = canonical_state_hash(&restored.world).unwrap();
    assert_eq!(h_original, h_restored);
}

// =========================================================================
// Group 3: Metrics Bytes & Hash (Tests 19–28)
// =========================================================================

#[test]
fn test_19_metrics_bytes_empty_time_series() {
    let metrics: [DailyMetrics; 0] = [];
    let bytes = canonical_metrics_bytes(&metrics).expect("empty metrics encode succeeds");

    // Header: "SIMCIV_METRICS_V1" (17) + record_count (4) = 21
    assert_eq!(bytes.len(), 21);
    assert_eq!(&bytes[0..17], DOMAIN_METRICS);
    assert_eq!(&bytes[17..21], &0u32.to_le_bytes());

    let hash = canonical_metrics_hash(&metrics).expect("empty metrics hash succeeds");
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_20_metrics_bytes_one_record() {
    let m = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.25,
        total_food_reserves: 1500.0,
        total_treasury: 5000,
    };

    let bytes = canonical_metrics_bytes(&[m]).unwrap();
    // 21 + 36 = 57 bytes
    assert_eq!(bytes.len(), 57);
}

#[test]
fn test_21_metrics_bytes_input_order_independence() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let m2 = DailyMetrics {
        day: 2,
        population: 105,
        wealth_gini: 0.22,
        total_food_reserves: 1100.0,
        total_treasury: 1200,
    };

    let forward = vec![m1.clone(), m2.clone()];
    let reversed = vec![m2, m1];

    assert_eq!(
        canonical_metrics_bytes(&forward).unwrap(),
        canonical_metrics_bytes(&reversed).unwrap()
    );
    assert_eq!(
        canonical_metrics_hash(&forward).unwrap(),
        canonical_metrics_hash(&reversed).unwrap()
    );
}

#[test]
fn test_22_metrics_hash_duplicate_day_rejected() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let m2 = DailyMetrics {
        day: 1,
        population: 105,
        wealth_gini: 0.22,
        total_food_reserves: 1100.0,
        total_treasury: 1200,
    };

    let err = canonical_metrics_bytes(&[m1, m2]).expect_err("duplicate day must fail");
    assert_eq!(err, CanonicalHashError::DuplicateMetricDay(1));
}

#[test]
fn test_23_metrics_hash_population_difference() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let m2 = DailyMetrics {
        day: 1,
        population: 101,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };

    assert_ne!(
        canonical_metrics_hash(&[m1]).unwrap(),
        canonical_metrics_hash(&[m2]).unwrap()
    );
}

#[test]
fn test_24_metrics_hash_gini_bit_difference() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let mut bits = 0.2f64.to_bits();
    bits ^= 1;
    let m2 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: f64::from_bits(bits),
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };

    assert_ne!(
        canonical_metrics_hash(&[m1]).unwrap(),
        canonical_metrics_hash(&[m2]).unwrap()
    );
}

#[test]
fn test_25_metrics_hash_food_reserves_bit_difference() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let mut bits = 1000.0f64.to_bits();
    bits ^= 1;
    let m2 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: f64::from_bits(bits),
        total_treasury: 1000,
    };

    assert_ne!(
        canonical_metrics_hash(&[m1]).unwrap(),
        canonical_metrics_hash(&[m2]).unwrap()
    );
}

#[test]
fn test_26_metrics_hash_treasury_difference() {
    let m1 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    let m2 = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1001,
    };

    assert_ne!(
        canonical_metrics_hash(&[m1]).unwrap(),
        canonical_metrics_hash(&[m2]).unwrap()
    );
}

#[test]
fn test_27_metrics_hash_non_finite_or_invalid_rejected() {
    let m_nan = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: f64::NAN,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    assert_eq!(
        canonical_metrics_bytes(&[m_nan]).unwrap_err(),
        CanonicalHashError::NonFiniteFloat("wealth_gini")
    );

    let m_out_of_bounds = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 1.5,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    };
    match canonical_metrics_bytes(&[m_out_of_bounds]) {
        Err(CanonicalHashError::InvalidState(_)) => {}
        other => panic!("expected InvalidState, got {:?}", other),
    }
}

#[test]
fn test_28_metrics_hashing_does_not_mutate_input() {
    let metrics = vec![DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.2,
        total_food_reserves: 1000.0,
        total_treasury: 1000,
    }];
    let before = metrics.clone();

    let _ = canonical_metrics_bytes(&metrics).unwrap();
    let _ = canonical_metrics_hash(&metrics).unwrap();

    assert_eq!(metrics, before);
}

// =========================================================================
// Group 4: Event Bytes & Hash (Tests 29–43)
// =========================================================================

#[test]
fn test_29_event_bytes_empty_stream() {
    let events: [EventRecord; 0] = [];
    let bytes = canonical_event_bytes(&events).expect("empty event stream encode succeeds");

    // Header: "SIMCIV_EVENTS_V1" (16) + record_count (4) = 20
    assert_eq!(bytes.len(), 20);
    assert_eq!(&bytes[0..16], DOMAIN_EVENTS);
    assert_eq!(&bytes[16..20], &0u32.to_le_bytes());

    let hash = canonical_event_hash(&events).expect("empty event stream hash succeeds");
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_30_event_bytes_one_event() {
    let r = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(42),
        }),
    );

    let bytes = canonical_event_bytes(&[r]).unwrap();
    // 20 + 21 (key) + 2 (tags) + 4 (agent_id) = 47 bytes
    assert_eq!(bytes.len(), 47);
}

#[test]
fn test_31_event_bytes_reversed_order_invariant() {
    let r1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(2),
        }),
    );

    let ordered = vec![r1.clone(), r2.clone()];
    let reversed = vec![r2, r1];

    assert_eq!(
        canonical_event_bytes(&ordered).unwrap(),
        canonical_event_bytes(&reversed).unwrap()
    );
    assert_eq!(
        canonical_event_hash(&ordered).unwrap(),
        canonical_event_hash(&reversed).unwrap()
    );
}

#[test]
fn test_32_event_hash_duplicate_key_rejected() {
    let key = EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0);
    let r1 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let r2 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(2),
        }),
    );

    let err = canonical_event_bytes(&[r1, r2]).expect_err("duplicate key must fail");
    assert_eq!(err, CanonicalHashError::DuplicateEventKey(key));
}

#[test]
fn test_33_event_hash_key_day_difference() {
    let r1 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(2, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_34_event_hash_key_phase_difference() {
    let r1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_35_event_hash_key_partition_difference() {
    let r1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(1, 8, 1, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_36_event_hash_key_local_sequence_difference() {
    let r1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(1, 8, 0, 1),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_37_event_hash_event_variant_difference() {
    let key = EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0);
    let r1 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let r2 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_38_event_hash_payload_difference() {
    let key = EventKey::new(1, 8, 0, 0);
    let r1 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 25,
        }),
    );
    let r2 = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 26,
        }),
    );

    assert_ne!(
        canonical_event_hash(&[r1]).unwrap(),
        canonical_event_hash(&[r2]).unwrap()
    );
}

#[test]
fn test_39_event_hash_all_state_transition_variants() {
    let e_work = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::WorkResolved {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            requested_harvest: 5.0,
            allocated_harvest: 4.5,
        }),
    );

    let e_food = EventRecord::new(
        EventKey::new(1, 6, 0, 1),
        Event::StateTransition(StateTransitionEvent::FoodTransferred {
            group_id: GroupId(0),
            initiator_agent_id: AgentId(1),
            target_agent_id: Some(AgentId(2)),
            action_kind: TargetedActionKind::GiveFood,
            amount: 2.0,
            success: true,
        }),
    );

    let e_market = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: GroupId(0),
            food_price: 15,
            tax_rate: 0.1,
            total_effective_supply: 50.0,
            total_effective_demand: 40.0,
            total_sold: 40.0,
            total_revenue: 600,
            tax_withheld: 60,
            net_pool_proceeds: 540,
            proceeds_balance: 0,
            buyers_count: 2,
            sellers_count: 3,
        }),
    );

    let e_welfare = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(2),
            payout: 30,
        }),
    );

    let e_mortality = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(3),
        }),
    );

    let stream = vec![e_work, e_food, e_market, e_welfare, e_mortality];
    let hash = canonical_event_hash(&stream).expect("all state transition variants encode");
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_40_event_hash_all_observation_variants() {
    let e_metrics = EventRecord::new(
        EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::DailyMetricsObserved(DailyMetrics {
            day: 1,
            population: 50,
            wealth_gini: 0.1,
            total_food_reserves: 500.0,
            total_treasury: 1000,
        })),
    );

    let e_snapshot = EventRecord::new(
        EventKey::new(1, 11, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::SnapshotEmitted {
            day: 1,
            master_seed: 12345,
            replicate_id: 1,
            schema_version: 1,
        }),
    );

    let stream = vec![e_metrics, e_snapshot];
    let hash = canonical_event_hash(&stream).expect("all observation variants encode");
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_41_negative_proceeds_balance_hashes_successfully() {
    let e_neg = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: GroupId(0),
            food_price: 33_554_432,
            tax_rate: 0.0,
            total_effective_supply: 1.0,
            total_effective_demand: 1.0,
            total_sold: 1.0,
            total_revenue: 33_554_432,
            tax_withheld: 0,
            net_pool_proceeds: 33_554_432,
            proceeds_balance: -1, // intentionally signed negative value
            buyers_count: 1,
            sellers_count: 3,
        }),
    );

    let bytes = canonical_event_bytes(std::slice::from_ref(&e_neg))
        .expect("negative proceeds_balance must encode");
    let hash = canonical_event_hash(std::slice::from_ref(&e_neg))
        .expect("negative proceeds_balance must hash");

    // Verify exact i64 LE -1 bytes in preimage
    assert!(bytes.windows(8).any(|w| w == (-1i64).to_le_bytes()));
    assert_eq!(hash.as_bytes().len(), 32);
}

#[test]
fn test_42_equivalent_flushed_unflushed_input_order_yields_same_hash() {
    let e1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let e2 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(2),
        }),
    );

    // Unflushed reverse input
    let unflushed = vec![e2.clone(), e1.clone()];
    let h_unflushed = canonical_event_hash(&unflushed).unwrap();

    // Flushed through EventBuffer
    let mut buffer = EventBuffer::new();
    buffer.push(e2);
    buffer.push(e1);
    let flushed = phase11_flush_events(&mut buffer).unwrap();
    let h_flushed = canonical_event_hash(&flushed).unwrap();

    assert_eq!(h_unflushed, h_flushed);
}

#[test]
fn test_43_event_hashing_does_not_mutate_events() {
    let events = vec![EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(42),
        }),
    )];
    let before = events.clone();

    let _ = canonical_event_bytes(&events).unwrap();
    let _ = canonical_event_hash(&events).unwrap();

    assert_eq!(events, before);
}

// =========================================================================
// Group 5: Domain Separation (Test 44)
// =========================================================================

#[test]
fn test_44_empty_domain_hashes_differ() {
    let world = make_test_world(0, vec![], vec![]);
    let metrics: [DailyMetrics; 0] = [];
    let events: [EventRecord; 0] = [];

    let h_state = canonical_state_hash(&world).unwrap();
    let h_metrics = canonical_metrics_hash(&metrics).unwrap();
    let h_events = canonical_event_hash(&events).unwrap();

    assert_ne!(h_state, h_metrics);
    assert_ne!(h_state, h_events);
    assert_ne!(h_metrics, h_events);
}

// =========================================================================
// Group 6: Cross-Component Integration (Tests 45–50)
// =========================================================================

#[test]
fn test_45_snapshot_restore_retains_state_hash() {
    let world = make_test_world(
        5,
        vec![
            make_test_agent(1, 0, true, 0, 0.9, 12.0, 100, 1.2, 0.6, 0.1, 0.2, 0),
            make_test_agent(2, 1, false, 0, 0.0, 0.0, 50, 0.8, 0.4, 0.3, 0.4, 0),
        ],
        vec![make_test_settlement(0, 500.0, 1500)],
    );

    let h1 = canonical_state_hash(&world).unwrap();

    let meta = SnapshotMetadata::new(5, 99999, 1, "0.1.0", "1.0");
    let encoded = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(&encoded).unwrap();

    let h2 = canonical_state_hash(&restored.world).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_46_phase10_metrics_record_hashes_deterministically() {
    let m1 = DailyMetrics {
        day: 1,
        population: 50,
        wealth_gini: 0.15,
        total_food_reserves: 300.0,
        total_treasury: 1000,
    };
    let m2 = DailyMetrics {
        day: 2,
        population: 48,
        wealth_gini: 0.16,
        total_food_reserves: 280.0,
        total_treasury: 1100,
    };

    let series = vec![m1, m2];
    let h1 = canonical_metrics_hash(&series).unwrap();
    let h2 = canonical_metrics_hash(&series).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_47_phase7_to_11_event_batch_hashes_deterministically() {
    let mkt_res = SettlementMarketResolution {
        group_id: GroupId(0),
        food_price: 10,
        tax_rate: 0.1,
        total_effective_supply: 20.0,
        total_effective_demand: 20.0,
        total_sold: 20.0,
        total_revenue: 200,
        tax_withheld: 20,
        net_pool_proceeds: 180,
        proceeds_balance: 0,
        sellers: vec![],
        buyers: vec![],
    };
    let wlf_res = SettlementWelfareResolution {
        group_id: GroupId(0),
        treasury_before: 120,
        treasury_after: 100,
        eligible_count: 1,
        payment_per_agent: 20,
        remainder: 0,
        total_distributed: 20,
        recipients: vec![WelfareRecipientResolution {
            agent_id: AgentId(1),
            payout: 20,
        }],
    };
    let mort_res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(2)],
        newly_deceased_count: 1,
        already_dead_count: 0,
        survivors_count: 0,
    };
    let metrics = DailyMetrics {
        day: 1,
        population: 10,
        wealth_gini: 0.05,
        total_food_reserves: 100.0,
        total_treasury: 100,
    };
    let meta = SnapshotMetadata::new(1, 42, 1, "0.1.0", "1.0");

    let mut events = Vec::new();
    events.extend(events_from_market_resolution(1, &mkt_res));
    events.extend(events_from_welfare_resolution(1, &wlf_res));
    events.extend(events_from_mortality_resolution(1, &mort_res));
    events.push(event_from_daily_metrics(&metrics));
    events.push(event_from_snapshot_emission(&meta, 1));

    let h1 = canonical_event_hash(&events).unwrap();
    let h2 = canonical_event_hash(&events).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_48_deterministic_inputs_produce_identical_hashes_repeated() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let m = vec![DailyMetrics {
        day: 1,
        population: 1,
        wealth_gini: 0.0,
        total_food_reserves: 10.0,
        total_treasury: 200,
    }];
    let e = vec![event_from_daily_metrics(&m[0])];

    assert_eq!(
        canonical_state_hash(&world).unwrap(),
        canonical_state_hash(&world).unwrap()
    );
    assert_eq!(
        canonical_metrics_hash(&m).unwrap(),
        canonical_metrics_hash(&m).unwrap()
    );
    assert_eq!(
        canonical_event_hash(&e).unwrap(),
        canonical_event_hash(&e).unwrap()
    );
}

#[test]
fn test_49_hashing_consumes_zero_rng() {
    // Pure hashing operations do not take, draw from, or mutate an RNG
    let world = make_test_world(1, vec![], vec![]);
    let _ = canonical_state_hash(&world).unwrap();
    let _ = canonical_metrics_hash(&[]).unwrap();
    let _ = canonical_event_hash(&[]).unwrap();
}

#[test]
fn test_50_all_existing_m0_behavior_preserved() {
    let world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(2, 1, true, 0, 0.0, 0.0, 0, 1.0, 0.5, 0.1, 0.1, 0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    assert_eq!(world.agents.len(), 2);
    assert_eq!(world.settlements.len(), 1);

    let h = canonical_state_hash(&world).unwrap();
    assert_eq!(h.as_bytes().len(), 32);
}

// =========================================================================
// Group 7: Independent Fixed-Vector Acceptance Tests (§26)
// =========================================================================

#[test]
fn test_51_independent_fixed_vector_state() {
    // Frozen independent test vector generated via Python hashlib.sha256:
    const EXPECTED_STATE_BYTES: [u8; 84] = [
        83, 73, 77, 67, 73, 86, 95, 83, 84, 65, 84, 69, 95, 86, 49, // b"SIMCIV_STATE_V1"
        1, 0, 0, 0, // current_day = 1
        1, 0, 0, 0, // agent_count = 1
        42, 0, 0, 0, // agent_id = 42
        1, // alive = true
        0, 0, 0, 0, // birth_day = 0
        0, 0, 128, 63, // health = 1.0f32
        0, 0, 32, 65, // food = 10.0f32
        100, 0, 0, 0, 0, 0, 0, 0, // wealth = 100i64
        0, 0, 128, 63, // productivity = 1.0f32
        0, 0, 0, 63, // cooperation = 0.5f32
        205, 204, 204, 61, // aggression = 0.1f32
        205, 204, 76, 62, // risk_tolerance = 0.2f32
        7, 0, // group_id = 7
        1, 0, 0, 0, // settlement_count = 1
        7, 0, // group_id = 7
        0, 0, 250, 67, // resource = 500.0f32
        232, 3, 0, 0, 0, 0, 0, 0, // treasury = 1000i64
    ];
    const EXPECTED_STATE_HASH_HEX: &str =
        "df23dfb04459b91e3d34432897bcc4cdbaa3973b832ae93311dc4b7918da77c4";

    let world = make_test_world(
        1,
        vec![make_test_agent(
            42, 999, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.1, 0.2, 7,
        )],
        vec![make_test_settlement(7, 500.0, 1000)],
    );

    let actual_bytes = canonical_state_bytes(&world).expect("state bytes encoding succeeds");
    assert_eq!(actual_bytes.as_slice(), &EXPECTED_STATE_BYTES);

    let actual_hash = canonical_state_hash(&world).expect("state hash succeeds");
    assert_eq!(actual_hash.to_hex(), EXPECTED_STATE_HASH_HEX);
}

#[test]
fn test_52_independent_fixed_vector_metrics() {
    // Frozen independent test vector generated via Python hashlib.sha256:
    const EXPECTED_METRICS_BYTES: [u8; 57] = [
        83, 73, 77, 67, 73, 86, 95, 77, 69, 84, 82, 73, 67, 83, 95, 86,
        49, // b"SIMCIV_METRICS_V1"
        1, 0, 0, 0, // record_count = 1
        1, 0, 0, 0, // day = 1
        100, 0, 0, 0, 0, 0, 0, 0, // population = 100
        0, 0, 0, 0, 0, 0, 208, 63, // wealth_gini = 0.25f64
        0, 0, 0, 0, 0, 112, 151, 64, // total_food_reserves = 1500.0f64
        136, 19, 0, 0, 0, 0, 0, 0, // total_treasury = 5000i64
    ];
    const EXPECTED_METRICS_HASH_HEX: &str =
        "92589a21bf9cbec1dbdc910e4040d1d9f8ce46c8c669d4ec29de8f08728b6581";

    let metrics = vec![DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.25,
        total_food_reserves: 1500.0,
        total_treasury: 5000,
    }];

    let actual_bytes = canonical_metrics_bytes(&metrics).expect("metrics bytes encoding succeeds");
    assert_eq!(actual_bytes.as_slice(), &EXPECTED_METRICS_BYTES);

    let actual_hash = canonical_metrics_hash(&metrics).expect("metrics hash succeeds");
    assert_eq!(actual_hash.to_hex(), EXPECTED_METRICS_HASH_HEX);
}

#[test]
fn test_53_independent_fixed_vector_events() {
    // Frozen independent test vector generated via Python hashlib.sha256:
    const EXPECTED_EVENTS_BYTES: [u8; 84] = [
        83, 73, 77, 67, 73, 86, 95, 69, 86, 69, 78, 84, 83, 95, 86, 49, // b"SIMCIV_EVENTS_V1"
        2, 0, 0, 0, // record_count = 2
        // Event 1
        1, 0, 0, 0, // day = 1
        8, // phase = 8
        0, 0, 0, 0, 0, 0, 0, 0, // partition_key = 0
        0, 0, 0, 0, 0, 0, 0, 0, // local_sequence = 0
        0, // category = StateTransition (0x00)
        3, // variant = WelfareDistributed (0x03)
        0, 0, // group_id = 0
        42, 0, 0, 0, // agent_id = 42
        25, 0, 0, 0, 0, 0, 0, 0, // payout = 25
        // Event 2
        1, 0, 0, 0, // day = 1
        9, // phase = 9
        255, 255, 255, 255, 255, 255, 255, 255, // partition_key = u64::MAX
        0, 0, 0, 0, 0, 0, 0, 0, // local_sequence = 0
        0, // category = StateTransition (0x00)
        4, // variant = MortalityCommitted (0x04)
        99, 0, 0, 0, // agent_id = 99
    ];
    const EXPECTED_EVENTS_HASH_HEX: &str =
        "8b8882d12434930e8f2e7dddf36e2cf8e7c36f4911e8a1d3d87a1b9444d07b17";

    let e1 = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(42),
            payout: 25,
        }),
    );
    let e2 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(99),
        }),
    );

    // Push in reverse order to also test ordering canonicalization in vector test
    let events = vec![e2, e1];

    let actual_bytes = canonical_event_bytes(&events).expect("events bytes encoding succeeds");
    assert_eq!(actual_bytes.as_slice(), &EXPECTED_EVENTS_BYTES);

    let actual_hash = canonical_event_hash(&events).expect("events hash succeeds");
    assert_eq!(actual_hash.to_hex(), EXPECTED_EVENTS_HASH_HEX);
}

// =========================================================================
// Group 6: Contract-Shape Regression Tests (Tests 54–60)
// =========================================================================

#[test]
fn test_54_contract_shape_state_settlement_count_after_agents() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0);
    let a2 = make_test_agent(2, 1, true, 0, 0.8, 8.0, 40, 0.9, 0.4, 0.2, 0.2, 0);
    let s1 = make_test_settlement(0, 100.0, 200);

    let world = make_test_world(1, vec![a1, a2], vec![s1]);
    let bytes = canonical_state_bytes(&world).expect("state bytes succeed");

    // Header: DOMAIN_STATE (15) + current_day (4) + agent_count (4) = 23 bytes
    assert_eq!(&bytes[0..15], DOMAIN_STATE);
    assert_eq!(&bytes[15..19], &1u32.to_le_bytes()); // current_day = 1
    assert_eq!(&bytes[19..23], &2u32.to_le_bytes()); // agent_count = 2

    // 2 agent records: 2 * 43 = 86 bytes (spanning 23..109)
    assert_eq!(&bytes[23..27], &1u32.to_le_bytes()); // agent 1 id
    assert_eq!(&bytes[23 + 43..23 + 43 + 4], &2u32.to_le_bytes()); // agent 2 id

    // settlement_count MUST be at offset 109 (immediately AFTER agent records), NOT at offset 23
    assert_eq!(&bytes[109..113], &1u32.to_le_bytes()); // settlement_count = 1

    // 1 settlement record: 14 bytes (spanning 113..127)
    assert_eq!(&bytes[113..115], &0u16.to_le_bytes()); // settlement group_id = 0
    assert_eq!(bytes.len(), 127);
}

#[test]
fn test_55_contract_shape_event_envelope_phase_1_byte_local_sequence_8_bytes() {
    let key = EventKey::new(42, 7, 0x1122334455667788u64, 0xAABBCCDDEEFF0011u64);
    let event = EventRecord::new(
        key,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(99),
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");

    // Header: DOMAIN_EVENTS (16) + count (4) = 20 bytes
    assert_eq!(&bytes[0..16], DOMAIN_EVENTS);
    assert_eq!(&bytes[16..20], &1u32.to_le_bytes());

    // EventKey envelope layout:
    // day: 4 bytes (offset 20..24)
    assert_eq!(&bytes[20..24], &42u32.to_le_bytes());
    // phase: EXACTLY 1 byte (offset 24..25), NOT u32 (4 bytes)
    assert_eq!(bytes[24], 7u8);
    // partition_key: 8 bytes (offset 25..33)
    assert_eq!(&bytes[25..33], &0x1122334455667788u64.to_le_bytes());
    // local_sequence: EXACTLY 8 bytes (offset 33..41), NOT u32 (4 bytes)
    assert_eq!(&bytes[33..41], &0xAABBCCDDEEFF0011u64.to_le_bytes());

    // category tag: 1 byte (offset 41..42)
    assert_eq!(bytes[41], EVENT_CATEGORY_STATE_TRANSITION);
    // variant tag: 1 byte (offset 42..43)
    assert_eq!(bytes[42], STATE_EVENT_MORTALITY_COMMITTED);
    // payload: agent_id (offset 43..47)
    assert_eq!(&bytes[43..47], &99u32.to_le_bytes());
    assert_eq!(bytes.len(), 47);
}

#[test]
fn test_56_contract_shape_market_cleared_payload_fields() {
    let event = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: GroupId(12),
            food_price: 10,
            tax_rate: 0.15,
            total_effective_supply: 100.5,
            total_effective_demand: 80.25,
            total_sold: 75.0,
            total_revenue: 750,
            tax_withheld: 112,
            net_pool_proceeds: 638,
            proceeds_balance: -5,
            buyers_count: 3,
            sellers_count: 4,
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");

    // Envelope ends at offset 43 (20 header + 21 key + 2 tags)
    let payload = &bytes[43..];
    // Payload layout must contain declaration order:
    // group_id: u16 (2 bytes)
    assert_eq!(&payload[0..2], &12u16.to_le_bytes());
    // food_price: Money (8 bytes)
    assert_eq!(&payload[2..10], &10i64.to_le_bytes());
    // tax_rate: f32 (4 bytes bits)
    assert_eq!(&payload[10..14], &0.15f32.to_bits().to_le_bytes());
    // total_effective_supply: f32 (4 bytes bits)
    assert_eq!(&payload[14..18], &100.5f32.to_bits().to_le_bytes());
    // total_effective_demand: f32 (4 bytes bits)
    assert_eq!(&payload[18..22], &80.25f32.to_bits().to_le_bytes());
    // total_sold: f32 (4 bytes bits)
    assert_eq!(&payload[22..26], &75.0f32.to_bits().to_le_bytes());
    // total_revenue: Money (8 bytes)
    assert_eq!(&payload[26..34], &750i64.to_le_bytes());
    // tax_withheld: Money (8 bytes)
    assert_eq!(&payload[34..42], &112i64.to_le_bytes());
    // net_pool_proceeds: Money (8 bytes)
    assert_eq!(&payload[42..50], &638i64.to_le_bytes());
    // proceeds_balance: Money (8 bytes signed)
    assert_eq!(&payload[50..58], &(-5i64).to_le_bytes());
    // buyers_count: u64 (8 bytes)
    assert_eq!(&payload[58..66], &3u64.to_le_bytes());
    // sellers_count: u64 (8 bytes)
    assert_eq!(&payload[66..74], &4u64.to_le_bytes());

    assert_eq!(payload.len(), 74);
}

#[test]
fn test_57_contract_shape_welfare_distributed_payload_fields() {
    let event = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(5),
            agent_id: AgentId(17),
            payout: 42,
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");
    let payload = &bytes[43..];

    // Exactly 3 fields: group_id (2) + agent_id (4) + payout (8) = 14 bytes
    assert_eq!(&payload[0..2], &5u16.to_le_bytes());
    assert_eq!(&payload[2..6], &17u32.to_le_bytes());
    assert_eq!(&payload[6..14], &42i64.to_le_bytes());
    assert_eq!(payload.len(), 14);
}

#[test]
fn test_58_contract_shape_mortality_committed_payload_fields() {
    let event = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(999),
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");
    let payload = &bytes[43..];

    // Exactly 1 field: agent_id (4 bytes)
    assert_eq!(&payload[0..4], &999u32.to_le_bytes());
    assert_eq!(payload.len(), 4);
}

#[test]
fn test_59_contract_shape_snapshot_emitted_payload_fields() {
    let event = EventRecord::new(
        EventKey::new(1, 11, GLOBAL_PARTITION_KEY, 0),
        Event::Observation(ObservationEvent::SnapshotEmitted {
            day: 42,
            master_seed: 0xDEAD_BEEF_CAFE_BABE_u64,
            replicate_id: 7,
            schema_version: 1,
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");
    let payload = &bytes[43..];

    // Exactly 4 fields: day (4) + master_seed (8) + replicate_id (4) + schema_version (4) = 20 bytes
    assert_eq!(&payload[0..4], &42u32.to_le_bytes());
    assert_eq!(&payload[4..12], &0xDEAD_BEEF_CAFE_BABE_u64.to_le_bytes());
    assert_eq!(&payload[12..16], &7u32.to_le_bytes());
    assert_eq!(&payload[16..20], &1u32.to_le_bytes());
    assert_eq!(payload.len(), 20);
}

#[test]
fn test_60_contract_shape_food_transferred_payload_fields() {
    let event = EventRecord::new(
        EventKey::new(1, 6, 0, 0),
        Event::StateTransition(StateTransitionEvent::FoodTransferred {
            group_id: GroupId(3),
            initiator_agent_id: AgentId(10),
            target_agent_id: Some(AgentId(20)),
            action_kind: TargetedActionKind::StealFood,
            amount: 5.5,
            success: false,
        }),
    );

    let bytes = canonical_event_bytes(&[event]).expect("event bytes succeed");
    let payload = &bytes[43..];

    // group_id (2) + initiator (4) + target (1 + 4) + action_kind (1) + amount (4) + success (1) = 17 bytes
    assert_eq!(&payload[0..2], &3u16.to_le_bytes());
    assert_eq!(&payload[2..6], &10u32.to_le_bytes());
    assert_eq!(payload[6], 1u8); // Some
    assert_eq!(&payload[7..11], &20u32.to_le_bytes());
    assert_eq!(payload[11], 4u8); // StealFood = 4
    assert_eq!(&payload[12..16], &5.5f32.to_bits().to_le_bytes());
    assert_eq!(payload[16], 0u8); // false = 0
    assert_eq!(payload.len(), 17);
}

// =========================================================================
// Group 7: Domain Invariant Validation Regression Tests (Tests 61–66)
// =========================================================================

#[test]
fn test_61_state_negative_food_rejected() {
    let agent = make_test_agent(1, 0, true, 0, 1.0, -0.001, 100, 1.0, 0.5, 0.1, 0.2, 0);
    let world = make_test_world(1, vec![agent], vec![]);

    let err_bytes = canonical_state_bytes(&world).unwrap_err();
    assert_eq!(err_bytes, CanonicalHashError::NegativeValue("food"));

    let err_hash = canonical_state_hash(&world).unwrap_err();
    assert_eq!(err_hash, CanonicalHashError::NegativeValue("food"));
}

#[test]
fn test_62_state_negative_wealth_rejected() {
    let agent = make_test_agent(1, 0, true, 0, 1.0, 10.0, -1, 1.0, 0.5, 0.1, 0.2, 0);
    let world = make_test_world(1, vec![agent], vec![]);

    let err_bytes = canonical_state_bytes(&world).unwrap_err();
    assert_eq!(err_bytes, CanonicalHashError::NegativeMoney(-1));

    let err_hash = canonical_state_hash(&world).unwrap_err();
    assert_eq!(err_hash, CanonicalHashError::NegativeMoney(-1));
}

#[test]
fn test_63_state_negative_settlement_resource_rejected() {
    let settlement = make_test_settlement(0, -0.5, 100);
    let world = make_test_world(1, vec![], vec![settlement]);

    let err_bytes = canonical_state_bytes(&world).unwrap_err();
    assert_eq!(err_bytes, CanonicalHashError::NegativeValue("resource"));

    let err_hash = canonical_state_hash(&world).unwrap_err();
    assert_eq!(err_hash, CanonicalHashError::NegativeValue("resource"));
}

#[test]
fn test_64_state_negative_treasury_rejected() {
    let settlement = make_test_settlement(0, 100.0, -50);
    let world = make_test_world(1, vec![], vec![settlement]);

    let err_bytes = canonical_state_bytes(&world).unwrap_err();
    assert_eq!(err_bytes, CanonicalHashError::NegativeMoney(-50));

    let err_hash = canonical_state_hash(&world).unwrap_err();
    assert_eq!(err_hash, CanonicalHashError::NegativeMoney(-50));
}

#[test]
fn test_65_metrics_negative_total_treasury_rejected() {
    let metric = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.25,
        total_food_reserves: 1000.0,
        total_treasury: -1,
    };

    let err_bytes = canonical_metrics_bytes(std::slice::from_ref(&metric)).unwrap_err();
    assert_eq!(err_bytes, CanonicalHashError::NegativeMoney(-1));

    let err_hash = canonical_metrics_hash(std::slice::from_ref(&metric)).unwrap_err();
    assert_eq!(err_hash, CanonicalHashError::NegativeMoney(-1));
}

#[test]
fn test_66_metrics_negative_total_food_reserves_rejected() {
    let metric = DailyMetrics {
        day: 1,
        population: 100,
        wealth_gini: 0.25,
        total_food_reserves: -0.1,
        total_treasury: 1000,
    };

    let err_bytes = canonical_metrics_bytes(std::slice::from_ref(&metric)).unwrap_err();
    assert_eq!(
        err_bytes,
        CanonicalHashError::NegativeValue("total_food_reserves")
    );

    let err_hash = canonical_metrics_hash(std::slice::from_ref(&metric)).unwrap_err();
    assert_eq!(
        err_hash,
        CanonicalHashError::NegativeValue("total_food_reserves")
    );
}
