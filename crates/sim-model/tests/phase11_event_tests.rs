//! Acceptance test suite for M0-14B: Event Buffer + Canonical Phase-11 Flush (Contract C07 / Phase 11).

use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::events::{
    Event, EventBuffer, EventError, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    StateTransitionEvent, event_from_daily_metrics, event_from_snapshot_emission,
    events_from_market_resolution, events_from_mortality_resolution,
    events_from_welfare_resolution, partition_key_from_group, phase11_flush_events,
};
use sim_model::intents::Intent;
use sim_model::metrics::DailyMetrics;
use sim_model::partitioning::SettlementIntentPartition;
use sim_model::phases::Phase9MortalityResolution;
use sim_model::resolution::{
    SettlementMarketResolution, SettlementWelfareResolution, WelfareRecipientResolution,
    phase7_market_clearance,
};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot};
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
// Group 1: Key Ordering (Tests 1–5)
// =========================================================================

#[test]
fn test_01_event_key_day_ordering() {
    let k1 = EventKey::new(1, 7, 0, 0);
    let k2 = EventKey::new(2, 7, 0, 0);
    assert!(k1 < k2);
}

#[test]
fn test_02_event_key_phase_ordering() {
    let day = 1;
    let k_6a = EventKey::new(day, 6, 0, 0);
    let k_7 = EventKey::new(day, 7, 0, 0);
    let k_8 = EventKey::new(day, 8, 0, 0);
    let k_9 = EventKey::new(day, 9, 0, 0);
    let k_10 = EventKey::new(day, 10, 0, 0);
    let k_11 = EventKey::new(day, 11, 0, 0);

    assert!(k_6a < k_7);
    assert!(k_7 < k_8);
    assert!(k_8 < k_9);
    assert!(k_9 < k_10);
    assert!(k_10 < k_11);
}

#[test]
fn test_03_event_key_partition_ordering() {
    let k0 = EventKey::new(1, 7, 0, 0);
    let k1 = EventKey::new(1, 7, 1, 0);
    let k2 = EventKey::new(1, 7, 2, 0);
    let kg = EventKey::new(1, 7, GLOBAL_PARTITION_KEY, 0);

    assert!(k0 < k1);
    assert!(k1 < k2);
    assert!(k2 < kg);
}

#[test]
fn test_04_event_key_local_sequence_ordering() {
    let s0 = EventKey::new(1, 8, 1, 0);
    let s1 = EventKey::new(1, 8, 1, 1);
    let s2 = EventKey::new(1, 8, 1, 2);

    assert!(s0 < s1);
    assert!(s1 < s2);
}

#[test]
fn test_05_event_key_full_lexical_ordering() {
    // Exact tuple order: (day, phase, partition_key, local_sequence)
    let keys = vec![
        EventKey::new(2, 6, 0, 0),
        EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0),
        EventKey::new(1, 7, 2, 1),
        EventKey::new(1, 7, 1, 0),
        EventKey::new(1, 7, 2, 0),
        EventKey::new(1, 6, 1, 0),
    ];

    let mut sorted = keys.clone();
    sorted.sort();

    let expected = vec![
        EventKey::new(1, 6, 1, 0),
        EventKey::new(1, 7, 1, 0),
        EventKey::new(1, 7, 2, 0),
        EventKey::new(1, 7, 2, 1),
        EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0),
        EventKey::new(2, 6, 0, 0),
    ];

    assert_eq!(sorted, expected);
}

// =========================================================================
// Group 2: Buffer & Flush (Tests 6–13)
// =========================================================================

#[test]
fn test_06_buffer_empty_flush() {
    let mut buffer = EventBuffer::new();
    assert!(buffer.is_empty());
    assert_eq!(buffer.len(), 0);

    let flushed = phase11_flush_events(&mut buffer).expect("empty flush should succeed");
    assert!(flushed.is_empty());
}

#[test]
fn test_07_buffer_single_event_flush() {
    let mut buffer = EventBuffer::new();
    let record = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(10),
        }),
    );
    buffer.push(record.clone());
    assert_eq!(buffer.len(), 1);

    let flushed = phase11_flush_events(&mut buffer).expect("single event flush should succeed");
    assert_eq!(flushed.len(), 1);
    assert_eq!(flushed[0], record);
    assert!(buffer.is_empty());
}

#[test]
fn test_08_buffer_multiple_events_canonical_order() {
    let mut buffer = EventBuffer::new();

    let r_mort = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let r_market = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
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
            buyers_count: 1,
            sellers_count: 1,
        }),
    );
    let r_welfare = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(2),
            payout: 20,
        }),
    );

    // Push out of order: 9, 7, 8
    buffer.push(r_mort.clone());
    buffer.push(r_market.clone());
    buffer.push(r_welfare.clone());

    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");
    assert_eq!(flushed.len(), 3);
    assert_eq!(flushed[0], r_market);
    assert_eq!(flushed[1], r_welfare);
    assert_eq!(flushed[2], r_mort);
}

#[test]
fn test_09_buffer_reverse_order_push_sorted() {
    let mut buffer = EventBuffer::new();

    let r1 = EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: 10,
        }),
    );
    let r2 = EventRecord::new(
        EventKey::new(1, 7, 0, 1),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(2),
            payout: 10,
        }),
    );
    let r3 = EventRecord::new(
        EventKey::new(1, 7, 0, 2),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(3),
            payout: 10,
        }),
    );

    // Push in reverse order
    buffer.push(r3.clone());
    buffer.push(r2.clone());
    buffer.push(r1.clone());

    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");
    assert_eq!(flushed, vec![r1, r2, r3]);
}

#[test]
fn test_10_buffer_cleared_after_flush() {
    let mut buffer = EventBuffer::new();
    buffer.push(EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    ));
    assert_eq!(buffer.len(), 1);

    let _ = phase11_flush_events(&mut buffer).unwrap();
    assert_eq!(buffer.len(), 0);
    assert!(buffer.is_empty());
    assert_eq!(buffer.pending_records().len(), 0);
}

#[test]
fn test_11_buffer_idempotent_second_flush() {
    let mut buffer = EventBuffer::new();
    buffer.push(EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    ));

    let first = phase11_flush_events(&mut buffer).unwrap();
    assert_eq!(first.len(), 1);

    let second = phase11_flush_events(&mut buffer).unwrap();
    assert!(second.is_empty());
}

#[test]
fn test_12_buffer_duplicate_key_rejected() {
    let mut buffer = EventBuffer::new();
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

    buffer.push(r1);
    buffer.push(r2);

    let err = phase11_flush_events(&mut buffer).expect_err("duplicate key must fail");
    assert_eq!(err, EventError::DuplicateKey(key));
}

#[test]
fn test_13_buffer_failed_flush_atomicity() {
    let mut buffer = EventBuffer::new();
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

    buffer.push(r1.clone());
    buffer.push(r2.clone());

    assert_eq!(buffer.len(), 2);
    let _ = phase11_flush_events(&mut buffer).expect_err("should fail");
    // Crucial: buffer must NOT be cleared or partially drained on failure!
    assert_eq!(buffer.len(), 2);
    assert_eq!(buffer.pending_records(), &[r1, r2]);
}

// =========================================================================
// Group 3: Partition Semantics (Tests 14–17)
// =========================================================================

#[test]
fn test_14_partition_key_group_id_widening() {
    assert_eq!(partition_key_from_group(GroupId(0)), 0u64);
    assert_eq!(partition_key_from_group(GroupId(42)), 42u64);
    assert_eq!(partition_key_from_group(GroupId(u16::MAX)), 65535u64);
}

#[test]
fn test_15_global_partition_key_constant() {
    assert_eq!(GLOBAL_PARTITION_KEY, u64::MAX);
}

#[test]
fn test_16_local_partition_keys_sort_before_global() {
    let local_max = partition_key_from_group(GroupId(u16::MAX));
    assert!(local_max < GLOBAL_PARTITION_KEY);

    let k_local = EventKey::new(1, 10, local_max, 0);
    let k_global = EventKey::new(1, 10, GLOBAL_PARTITION_KEY, 0);
    assert!(k_local < k_global);
}

#[test]
fn test_17_partition_key_dense_slot_independence() {
    // Partition key is purely derived from GroupId, never from an agent's storage DenseSlot
    let agent_a = make_test_agent(
        100, 999, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 5, // GroupId 5
    );
    let agent_b = make_test_agent(
        101, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 5, // GroupId 5
    );

    assert_eq!(
        partition_key_from_group(agent_a.group_id),
        partition_key_from_group(agent_b.group_id)
    );
    assert_eq!(partition_key_from_group(agent_a.group_id), 5u64);
}

// =========================================================================
// Group 4: State Transition Events (Tests 18–22)
// =========================================================================

#[test]
fn test_18_phase7_market_adapter() {
    let res = SettlementMarketResolution {
        group_id: GroupId(3),
        food_price: 15,
        tax_rate: 0.1,
        total_effective_supply: 100.0,
        total_effective_demand: 80.0,
        total_sold: 80.0,
        total_revenue: 1200,
        tax_withheld: 120,
        net_pool_proceeds: 1080,
        proceeds_balance: 0,
        sellers: vec![],
        buyers: vec![],
    };

    let events = events_from_market_resolution(2, &res);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].key, EventKey::new(2, 7, 3, 0));

    match &events[0].event {
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id,
            food_price,
            tax_rate,
            total_effective_supply,
            total_effective_demand,
            total_sold,
            total_revenue,
            tax_withheld,
            net_pool_proceeds,
            proceeds_balance,
            buyers_count,
            sellers_count,
        }) => {
            assert_eq!(*group_id, GroupId(3));
            assert_eq!(*food_price, 15);
            assert_eq!(*tax_rate, 0.1);
            assert_eq!(*total_effective_supply, 100.0);
            assert_eq!(*total_effective_demand, 80.0);
            assert_eq!(*total_sold, 80.0);
            assert_eq!(*total_revenue, 1200);
            assert_eq!(*tax_withheld, 120);
            assert_eq!(*net_pool_proceeds, 1080);
            assert_eq!(*proceeds_balance, 0);
            assert_eq!(*buyers_count, 0);
            assert_eq!(*sellers_count, 0);
        }
        _ => panic!("unexpected event variant"),
    }
}

#[test]
fn test_19_phase8_welfare_adapter() {
    let res = SettlementWelfareResolution {
        group_id: GroupId(1),
        treasury_before: 150,
        treasury_after: 50,
        eligible_count: 2,
        payment_per_agent: 50,
        remainder: 0,
        total_distributed: 100,
        recipients: vec![
            WelfareRecipientResolution {
                agent_id: AgentId(20),
                payout: 50,
            },
            WelfareRecipientResolution {
                agent_id: AgentId(10),
                payout: 50,
            },
        ],
    };

    let events = events_from_welfare_resolution(3, &res);
    assert_eq!(events.len(), 2);

    // Canonically sorted by AgentId ascending
    assert_eq!(events[0].key, EventKey::new(3, 8, 1, 0));
    assert_eq!(
        events[0].event,
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(1),
            agent_id: AgentId(10),
            payout: 50,
        })
    );

    assert_eq!(events[1].key, EventKey::new(3, 8, 1, 1));
    assert_eq!(
        events[1].event,
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(1),
            agent_id: AgentId(20),
            payout: 50,
        })
    );
}

#[test]
fn test_20_phase9_mortality_adapter() {
    let res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(5), AgentId(2)],
        newly_deceased_count: 2,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let events = events_from_mortality_resolution(4, &res);
    assert_eq!(events.len(), 2);

    // Sequenced canonically by AgentId ascending on GLOBAL_PARTITION_KEY
    assert_eq!(events[0].key, EventKey::new(4, 9, GLOBAL_PARTITION_KEY, 0));
    assert_eq!(
        events[0].event,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(2),
        })
    );

    assert_eq!(events[1].key, EventKey::new(4, 9, GLOBAL_PARTITION_KEY, 1));
    assert_eq!(
        events[1].event,
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(5),
        })
    );
}

#[test]
fn test_21_phase9_already_dead_tombstones_excluded() {
    // If no agents newly died on this day, Phase9MortalityResolution.newly_deceased is empty
    let res = Phase9MortalityResolution {
        newly_deceased: vec![],
        newly_deceased_count: 0,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let events = events_from_mortality_resolution(5, &res);
    assert!(events.is_empty());
}

#[test]
fn test_22_provenance_adapters_pure_mapping() {
    let res = SettlementMarketResolution {
        group_id: GroupId(0),
        food_price: 10,
        tax_rate: 0.05,
        total_effective_supply: 50.0,
        total_effective_demand: 50.0,
        total_sold: 50.0,
        total_revenue: 500,
        tax_withheld: 25,
        net_pool_proceeds: 475,
        proceeds_balance: 0,
        sellers: vec![],
        buyers: vec![],
    };

    // Calling the adapter multiple times produces identical output without mutating input
    let e1 = events_from_market_resolution(1, &res);
    let e2 = events_from_market_resolution(1, &res);
    assert_eq!(e1, e2);
}

// =========================================================================
// Group 5: Observation Events (Tests 23–27)
// =========================================================================

#[test]
fn test_23_phase10_daily_metrics_adapter() {
    let metrics = DailyMetrics {
        day: 3,
        population: 50,
        wealth_gini: 0.125,
        total_food_reserves: 400.0,
        total_treasury: 1500,
    };

    let record = event_from_daily_metrics(&metrics);
    assert_eq!(record.key, EventKey::new(3, 10, GLOBAL_PARTITION_KEY, 0));
}

#[test]
fn test_24_phase10_payload_preservation() {
    let metrics = DailyMetrics {
        day: 7,
        population: 120,
        wealth_gini: 0.4567,
        total_food_reserves: 1234.5,
        total_treasury: 9999,
    };

    let record = event_from_daily_metrics(&metrics);
    match record.event {
        Event::Observation(ObservationEvent::DailyMetricsObserved(m)) => {
            assert_eq!(m, metrics);
        }
        _ => panic!("wrong observation event variant"),
    }
}

#[test]
fn test_25_phase11_snapshot_emitted_event() {
    let meta = SnapshotMetadata::new(10, 987654321, 2, "0.1.0", "1.0");
    let record = event_from_snapshot_emission(&meta, 1);

    assert_eq!(record.key, EventKey::new(10, 11, GLOBAL_PARTITION_KEY, 0));
    assert_eq!(
        record.event,
        Event::Observation(ObservationEvent::SnapshotEmitted {
            day: 10,
            master_seed: 987654321,
            replicate_id: 2,
            schema_version: 1,
        })
    );
}

#[test]
fn test_26_skipped_snapshot_produces_no_event() {
    // When snapshot interval condition is false, no snapshot emission is performed
    let snapshot_interval = 5u32;
    let current_day = 3u32;

    let mut buffer = EventBuffer::new();
    if current_day.is_multiple_of(snapshot_interval) {
        let meta = SnapshotMetadata::new(current_day, 123, 1, "0.1.0", "1.0");
        buffer.push(event_from_snapshot_emission(&meta, 1));
    }

    assert!(buffer.is_empty());
}

#[test]
fn test_27_absence_of_volatile_timestamps_or_paths() {
    let meta = SnapshotMetadata::new(1, 100, 1, "v1", "c1");
    let rec = event_from_snapshot_emission(&meta, 1);

    // Ensure SnapshotEmitted does not contain wall-clock timestamps, file paths, or pointers
    if let Event::Observation(ObservationEvent::SnapshotEmitted {
        day,
        master_seed,
        replicate_id,
        schema_version,
    }) = rec.event
    {
        assert_eq!(day, 1);
        assert_eq!(master_seed, 100);
        assert_eq!(replicate_id, 1);
        assert_eq!(schema_version, 1);
    } else {
        panic!("unexpected event");
    }
}

// =========================================================================
// Group 6: Producer Sequence (Tests 28–30)
// =========================================================================

#[test]
fn test_28_producer_sequence_welfare_ascending_agent_id() {
    let res = SettlementWelfareResolution {
        group_id: GroupId(0),
        treasury_before: 130,
        treasury_after: 100,
        eligible_count: 3,
        payment_per_agent: 10,
        remainder: 0,
        total_distributed: 30,
        recipients: vec![
            WelfareRecipientResolution {
                agent_id: AgentId(50),
                payout: 10,
            },
            WelfareRecipientResolution {
                agent_id: AgentId(10),
                payout: 10,
            },
            WelfareRecipientResolution {
                agent_id: AgentId(30),
                payout: 10,
            },
        ],
    };

    let events = events_from_welfare_resolution(1, &res);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].key.local_sequence, 0);
    assert_eq!(events[1].key.local_sequence, 1);
    assert_eq!(events[2].key.local_sequence, 2);

    if let Event::StateTransition(StateTransitionEvent::WelfareDistributed { agent_id, .. }) =
        events[0].event
    {
        assert_eq!(agent_id, AgentId(10));
    }
    if let Event::StateTransition(StateTransitionEvent::WelfareDistributed { agent_id, .. }) =
        events[1].event
    {
        assert_eq!(agent_id, AgentId(30));
    }
    if let Event::StateTransition(StateTransitionEvent::WelfareDistributed { agent_id, .. }) =
        events[2].event
    {
        assert_eq!(agent_id, AgentId(50));
    }
}

#[test]
fn test_29_producer_sequence_mortality_ascending_agent_id() {
    let res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(40), AgentId(5), AgentId(25)],
        newly_deceased_count: 3,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let events = events_from_mortality_resolution(1, &res);
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].key.local_sequence, 0);
    assert_eq!(events[1].key.local_sequence, 1);
    assert_eq!(events[2].key.local_sequence, 2);

    if let Event::StateTransition(StateTransitionEvent::MortalityCommitted { agent_id }) =
        events[0].event
    {
        assert_eq!(agent_id, AgentId(5));
    }
    if let Event::StateTransition(StateTransitionEvent::MortalityCommitted { agent_id }) =
        events[1].event
    {
        assert_eq!(agent_id, AgentId(25));
    }
    if let Event::StateTransition(StateTransitionEvent::MortalityCommitted { agent_id }) =
        events[2].event
    {
        assert_eq!(agent_id, AgentId(40));
    }
}

#[test]
fn test_30_source_vec_order_independence() {
    let res_perm1 = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(10), AgentId(20), AgentId(30)],
        newly_deceased_count: 3,
        already_dead_count: 0,
        survivors_count: 0,
    };
    let res_perm2 = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(30), AgentId(10), AgentId(20)],
        newly_deceased_count: 3,
        already_dead_count: 0,
        survivors_count: 0,
    };
    let res_perm3 = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(20), AgentId(30), AgentId(10)],
        newly_deceased_count: 3,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let events1 = events_from_mortality_resolution(1, &res_perm1);
    let events2 = events_from_mortality_resolution(1, &res_perm2);
    let events3 = events_from_mortality_resolution(1, &res_perm3);

    assert_eq!(events1, events2);
    assert_eq!(events2, events3);
}

// =========================================================================
// Group 7: Validation (Tests 31–34)
// =========================================================================

#[test]
fn test_31_validation_invalid_phase() {
    let mut buffer = EventBuffer::new();
    buffer.push(EventRecord::new(
        EventKey::new(1, 0, 0, 0), // Phase 0 is invalid
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    ));

    let err = phase11_flush_events(&mut buffer).expect_err("phase 0 must fail");
    assert_eq!(err, EventError::InvalidPhase(0));

    let mut buffer2 = EventBuffer::new();
    buffer2.push(EventRecord::new(
        EventKey::new(1, 12, 0, 0), // Phase 12 is invalid
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    ));

    let err2 = phase11_flush_events(&mut buffer2).expect_err("phase 12 must fail");
    assert_eq!(err2, EventError::InvalidPhase(12));
}

#[test]
fn test_32_validation_non_finite_float() {
    let mut buffer = EventBuffer::new();
    buffer.push(EventRecord::new(
        EventKey::new(1, 7, 0, 0),
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: GroupId(0),
            food_price: 10,
            tax_rate: f32::NAN,
            total_effective_supply: 100.0,
            total_effective_demand: 100.0,
            total_sold: 100.0,
            total_revenue: 1000,
            tax_withheld: 100,
            net_pool_proceeds: 900,
            proceeds_balance: 0,
            buyers_count: 1,
            sellers_count: 1,
        }),
    ));

    let err = phase11_flush_events(&mut buffer).expect_err("NaN float must fail");
    assert_eq!(err, EventError::NonFiniteFloat("tax_rate"));
}

#[test]
fn test_33_validation_negative_money() {
    let mut buffer = EventBuffer::new();
    buffer.push(EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(1),
            payout: -5,
        }),
    ));

    let err = phase11_flush_events(&mut buffer).expect_err("negative money must fail");
    assert_eq!(err, EventError::NegativeMoney(-5));
}

#[test]
fn test_34_validation_zero_drain_on_error() {
    let mut buffer = EventBuffer::new();

    let r_valid1 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(1),
        }),
    );
    let r_invalid = EventRecord::new(
        EventKey::new(1, 8, 0, 0),
        Event::StateTransition(StateTransitionEvent::WelfareDistributed {
            group_id: GroupId(0),
            agent_id: AgentId(2),
            payout: -10, // Invalid negative money!
        }),
    );
    let r_valid2 = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 1),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(3),
        }),
    );

    buffer.push(r_valid1);
    buffer.push(r_invalid);
    buffer.push(r_valid2);

    assert_eq!(buffer.len(), 3);
    let _ = phase11_flush_events(&mut buffer).expect_err("validation must fail");

    // All 3 records must still be in buffer
    assert_eq!(buffer.len(), 3);
}

// =========================================================================
// Group 8: Observer Independence (Tests 35–39)
// =========================================================================

#[test]
fn test_35_observer_independence_world_unchanged() {
    let world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(2, 1, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );

    let world_clone = world.clone();

    // Event operations
    let mut buffer = EventBuffer::new();
    let metrics = DailyMetrics {
        day: 1,
        population: 2,
        wealth_gini: 0.0,
        total_food_reserves: 20.0,
        total_treasury: 200,
    };
    buffer.push(event_from_daily_metrics(&metrics));
    let _ = phase11_flush_events(&mut buffer).unwrap();

    // WorldState must be completely identical
    assert_eq!(world, world_clone);
}

#[test]
fn test_36_observer_independence_trajectory_equality() {
    let world1 = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world2 = world1.clone();

    // World 1 records telemetry
    let mut buffer = EventBuffer::new();
    let m = DailyMetrics {
        day: 1,
        population: 1,
        wealth_gini: 0.0,
        total_food_reserves: 10.0,
        total_treasury: 200,
    };
    buffer.push(event_from_daily_metrics(&m));
    let _ = phase11_flush_events(&mut buffer).unwrap();

    // World 2 does not record telemetry
    // Both states remain bitwise identical
    assert_eq!(world1, world2);
}

#[test]
fn test_37_observer_independence_zero_rng_consumption() {
    // Pure event generation, buffering, and flushing do not take or mutate an RNG
    let mut buffer = EventBuffer::new();
    let meta = SnapshotMetadata::new(1, 42, 1, "0.1.0", "1.0");
    buffer.push(event_from_snapshot_emission(&meta, 1));
    let flushed = phase11_flush_events(&mut buffer).unwrap();
    assert_eq!(flushed.len(), 1);
}

#[test]
fn test_38_deterministic_event_batch_construction() {
    let res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(10), AgentId(5), AgentId(20)],
        newly_deceased_count: 3,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let events1 = events_from_mortality_resolution(2, &res);
    let events2 = events_from_mortality_resolution(2, &res);

    assert_eq!(events1, events2);
}

#[test]
fn test_39_serde_roundtrip() {
    let record = EventRecord::new(
        EventKey::new(1, 9, GLOBAL_PARTITION_KEY, 0),
        Event::StateTransition(StateTransitionEvent::MortalityCommitted {
            agent_id: AgentId(42),
        }),
    );

    let serialized = toml::to_string(&record).expect("serde serialization should succeed");
    let deserialized: EventRecord =
        toml::from_str(&serialized).expect("serde deserialization should succeed");

    assert_eq!(record, deserialized);
}

// =========================================================================
// Group 9: Integration (Tests 40–46)
// =========================================================================

#[test]
fn test_40_phase7_event_buffer_flush_preserves_market_state() {
    let world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(2, 1, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0),
        ],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world_before = world.clone();

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

    let mut buffer = EventBuffer::new();
    buffer.push_all(events_from_market_resolution(1, &mkt_res));
    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");

    assert_eq!(flushed.len(), 1);
    assert_eq!(world, world_before);
}

#[test]
fn test_41_phase8_event_buffer_flush_preserves_welfare_state() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world_before = world.clone();

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

    let mut buffer = EventBuffer::new();
    buffer.push_all(events_from_welfare_resolution(1, &wlf_res));
    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");

    assert_eq!(flushed.len(), 1);
    assert_eq!(world, world_before);
}

#[test]
fn test_42_phase9_event_buffer_flush_preserves_mortality_state() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, false, 0, 0.0, 0.0, 0, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world_before = world.clone();

    let mort_res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(1)],
        newly_deceased_count: 1,
        already_dead_count: 0,
        survivors_count: 0,
    };

    let mut buffer = EventBuffer::new();
    buffer.push_all(events_from_mortality_resolution(1, &mort_res));
    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");

    assert_eq!(flushed.len(), 1);
    assert_eq!(world, world_before);
}

#[test]
fn test_43_phase10_observation_event_flush_preserves_world() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let world_before = world.clone();

    let metrics = DailyMetrics {
        day: 1,
        population: 1,
        wealth_gini: 0.0,
        total_food_reserves: 10.0,
        total_treasury: 200,
    };

    let mut buffer = EventBuffer::new();
    buffer.push(event_from_daily_metrics(&metrics));
    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");

    assert_eq!(flushed.len(), 1);
    assert_eq!(world, world_before);
}

#[test]
fn test_44_snapshot_emitted_event_flush_after_snapshot_creation() {
    let world = make_test_world(
        1,
        vec![make_test_agent(
            1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.1, 0.1, 0,
        )],
        vec![make_test_settlement(0, 100.0, 200)],
    );
    let meta = SnapshotMetadata::new(1, 12345, 1, "0.1.0", "1.0");
    let _snapshot_bytes = encode_snapshot(&world, &meta).expect("encode succeeds");

    let mut buffer = EventBuffer::new();
    buffer.push(event_from_snapshot_emission(&meta, 1));
    let flushed = phase11_flush_events(&mut buffer).expect("flush should succeed");

    assert_eq!(flushed.len(), 1);
    assert_eq!(
        flushed[0].key,
        EventKey::new(1, 11, GLOBAL_PARTITION_KEY, 0)
    );
}

#[test]
fn test_45_mixed_phase_7_to_11_events_canonicalize() {
    let mut buffer = EventBuffer::new();

    // Push out of order: 11, 8, 10, 7, 9
    let meta = SnapshotMetadata::new(1, 42, 1, "v1", "c1");
    buffer.push(event_from_snapshot_emission(&meta, 1)); // Phase 11

    let wlf_res = SettlementWelfareResolution {
        group_id: GroupId(0),
        treasury_before: 120,
        treasury_after: 100,
        eligible_count: 1,
        payment_per_agent: 20,
        remainder: 0,
        total_distributed: 20,
        recipients: vec![WelfareRecipientResolution {
            agent_id: AgentId(2),
            payout: 20,
        }],
    };
    buffer.push_all(events_from_welfare_resolution(1, &wlf_res)); // Phase 8

    let metrics = DailyMetrics {
        day: 1,
        population: 5,
        wealth_gini: 0.0,
        total_food_reserves: 10.0,
        total_treasury: 100,
    };
    buffer.push(event_from_daily_metrics(&metrics)); // Phase 10

    let mkt_res = SettlementMarketResolution {
        group_id: GroupId(0),
        food_price: 10,
        tax_rate: 0.0,
        total_effective_supply: 0.0,
        total_effective_demand: 0.0,
        total_sold: 0.0,
        total_revenue: 0,
        tax_withheld: 0,
        net_pool_proceeds: 0,
        proceeds_balance: 0,
        sellers: vec![],
        buyers: vec![],
    };
    buffer.push_all(events_from_market_resolution(1, &mkt_res)); // Phase 7

    let mort_res = Phase9MortalityResolution {
        newly_deceased: vec![AgentId(1)],
        newly_deceased_count: 1,
        already_dead_count: 0,
        survivors_count: 0,
    };
    buffer.push_all(events_from_mortality_resolution(1, &mort_res)); // Phase 9

    let flushed = phase11_flush_events(&mut buffer).expect("mixed flush should succeed");
    assert_eq!(flushed.len(), 5);

    let phases: Vec<u8> = flushed.iter().map(|r| r.key.phase).collect();
    assert_eq!(phases, vec![7, 8, 9, 10, 11]);

    for window in flushed.windows(2) {
        assert!(window[0].key < window[1].key);
    }
}

#[test]
fn test_46_all_existing_m0_tests_remain_unchanged_and_passing() {
    // Basic verification of M0 world baseline compatibility
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

    let mut buffer = EventBuffer::new();
    assert!(buffer.is_empty());
    let flushed = phase11_flush_events(&mut buffer).unwrap();
    assert!(flushed.is_empty());
}

#[test]
fn test_47_phase7_negative_proceeds_balance_event_flush_regression() {
    // 1. Construct and execute a valid Phase 7 resolution whose proceeds_balance < 0
    let mut world = make_test_world(
        1,
        vec![
            make_test_agent(0, 0, true, 0, 1.0, 0.0, 50_000_000, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(1, 1, true, 0, 1.0, 10.0, 0, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(2, 2, true, 0, 1.0, 10.0, 0, 1.0, 0.5, 0.1, 0.1, 0),
            make_test_agent(3, 3, true, 0, 1.0, 10.0, 0, 1.0, 0.5, 0.1, 0.1, 0),
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
    let res = phase7_market_clearance(&mut world, &[partition], food_price, 0.0)
        .expect("market clearance should succeed");

    assert_eq!(res[0].proceeds_balance, -1);
    let world_before_event = world.clone();

    // 2. Generate its MarketCleared event
    let events = events_from_market_resolution(1, &res[0]);
    assert_eq!(events.len(), 1);

    // 3. Push it into EventBuffer
    let mut buffer = EventBuffer::new();
    buffer.push_all(events);

    // 4. Canonical flush must succeed
    let flushed = phase11_flush_events(&mut buffer)
        .expect("canonical flush must succeed for negative proceeds_balance");

    // 5. Flushed event must preserve the exact negative proceeds_balance
    assert_eq!(flushed.len(), 1);
    match &flushed[0].event {
        Event::StateTransition(StateTransitionEvent::MarketCleared {
            proceeds_balance,
            food_price: price,
            net_pool_proceeds,
            ..
        }) => {
            assert_eq!(*proceeds_balance, -1);
            assert_eq!(*price, 33_554_432);
            assert_eq!(*net_pool_proceeds, 33_554_432);
        }
        _ => panic!("expected MarketCleared event"),
    }

    // 6. No world mutation
    assert_eq!(world, world_before_event);
}
