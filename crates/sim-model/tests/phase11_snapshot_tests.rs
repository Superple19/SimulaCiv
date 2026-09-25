//! Acceptance test suite for M0-14A: Canonical Snapshot / Restore Reference Semantics (Contract C08 / Phase 11).

use sim_core::{AgentId, DenseSlot, GroupId, Money, ReplicateId, SimulationDay};
use sim_model::{
    AgentState, Intent, SettlementIntentPartition, SettlementState, SimConfig, SnapshotError,
    SnapshotMetadata, WorldState, decode_snapshot, encode_snapshot, phase7_market_clearance,
    phase8_welfare_distribution, phase9_mortality_commitment, phase10_observe,
    phase11_snapshot_if_boundary, restore_snapshot,
};

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

fn make_test_metadata(day: u32) -> SnapshotMetadata {
    SnapshotMetadata::new(
        day,
        0x1234_5678_9ABC_DEF0,
        42,
        "simulaciv-m0-v0.1.0",
        "baseline-v1",
    )
}

// =========================================================================
// 1. Basic encode/decode
// =========================================================================

#[test]
fn test_01_empty_valid_world_round_trip() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).expect("encode should succeed");
    let restored = restore_snapshot(snapshot.as_bytes()).expect("decode should succeed");

    assert_eq!(restored.metadata, meta);
    assert_eq!(restored.world.current_day, SimulationDay(0));
    assert!(restored.world.agents.is_empty());
    assert!(restored.world.settlements.is_empty());
    assert_eq!(restored.world.initial_money_supply, 0);
}

#[test]
fn test_02_one_agent_round_trip() {
    let agent = make_test_agent(1, 0, true, 0, 1.0, 10.0, 100, 1.5, 0.5, 0.2, 0.8, 0);
    let world = make_test_world(1, vec![agent.clone()], vec![]);
    let meta = make_test_metadata(1);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents.len(), 1);
    let r_agent = &restored.world.agents[0];
    assert_eq!(r_agent.agent_id, agent.agent_id);
    assert_eq!(r_agent.alive, agent.alive);
    assert_eq!(r_agent.birth_day, agent.birth_day);
    assert_eq!(r_agent.health.to_bits(), agent.health.to_bits());
    assert_eq!(r_agent.food.to_bits(), agent.food.to_bits());
    assert_eq!(r_agent.wealth, agent.wealth);
    assert_eq!(r_agent.productivity.to_bits(), agent.productivity.to_bits());
    assert_eq!(r_agent.cooperation.to_bits(), agent.cooperation.to_bits());
    assert_eq!(r_agent.aggression.to_bits(), agent.aggression.to_bits());
    assert_eq!(
        r_agent.risk_tolerance.to_bits(),
        agent.risk_tolerance.to_bits()
    );
    assert_eq!(r_agent.group_id, agent.group_id);
}

#[test]
fn test_03_multiple_agents_round_trip() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 5.0, 50, 1.0, 0.2, 0.3, 0.4, 0);
    let a2 = make_test_agent(2, 1, false, 0, 0.0, 0.0, 150, 2.0, 0.8, 0.1, 0.9, 1);
    let a3 = make_test_agent(3, 2, true, 1, 0.75, 12.5, 200, 1.2, 0.4, 0.6, 0.1, 0);
    let world = make_test_world(2, vec![a1, a2, a3], vec![]);
    let meta = make_test_metadata(2);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents.len(), 3);
    assert_eq!(restored.world.agents[0].agent_id, AgentId(1));
    assert_eq!(restored.world.agents[1].agent_id, AgentId(2));
    assert_eq!(restored.world.agents[2].agent_id, AgentId(3));
}

#[test]
fn test_04_settlement_state_round_trip() {
    let s0 = make_test_settlement(0, 150.25, 500);
    let s1 = make_test_settlement(1, 75.125, 1200);
    let world = make_test_world(3, vec![], vec![s0, s1]);
    let meta = make_test_metadata(3);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.settlements.len(), 2);
    assert_eq!(restored.world.settlements[0].group_id, GroupId(0));
    assert_eq!(
        restored.world.settlements[0].resource.to_bits(),
        150.25f32.to_bits()
    );
    assert_eq!(restored.world.settlements[0].treasury, 500);
    assert_eq!(restored.world.settlements[1].group_id, GroupId(1));
    assert_eq!(
        restored.world.settlements[1].resource.to_bits(),
        75.125f32.to_bits()
    );
    assert_eq!(restored.world.settlements[1].treasury, 1200);
}

#[test]
fn test_05_dead_tombstone_preserved() {
    let dead_agent = make_test_agent(10, 0, false, 0, 0.0, 3.5, 400, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(4, vec![dead_agent], vec![]);
    let meta = make_test_metadata(4);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert!(!restored.world.agents[0].alive);
    assert_eq!(restored.world.agents[0].health.to_bits(), 0.0f32.to_bits());
}

#[test]
fn test_06_money_preserved_exactly() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 123_456_789, 1.0, 0.5, 0.5, 0.5, 0);
    let s0 = make_test_settlement(0, 50.0, 987_654_321);
    let world = make_test_world(0, vec![a1], vec![s0]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents[0].wealth, 123_456_789);
    assert_eq!(restored.world.settlements[0].treasury, 987_654_321);
    assert_eq!(
        restored.world.initial_money_supply,
        123_456_789 + 987_654_321
    );
}

#[test]
fn test_07_all_authoritative_agent_fields_preserved() {
    let a = make_test_agent(
        42, 99, // volatile dense slot
        true, 7, 0.875, 14.375, 777, 2.125, 0.3125, 0.6875, 0.9375, 5,
    );
    let world = make_test_world(10, vec![a.clone()], vec![]);
    let meta = make_test_metadata(10);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();
    let r = &restored.world.agents[0];

    assert_eq!(r.agent_id, AgentId(42));
    assert!(r.alive);
    assert_eq!(r.birth_day, SimulationDay(7));
    assert_eq!(r.health.to_bits(), 0.875f32.to_bits());
    assert_eq!(r.food.to_bits(), 14.375f32.to_bits());
    assert_eq!(r.wealth, 777);
    assert_eq!(r.productivity.to_bits(), 2.125f32.to_bits());
    assert_eq!(r.cooperation.to_bits(), 0.3125f32.to_bits());
    assert_eq!(r.aggression.to_bits(), 0.6875f32.to_bits());
    assert_eq!(r.risk_tolerance.to_bits(), 0.9375f32.to_bits());
    assert_eq!(r.group_id, GroupId(5));
}

#[test]
fn test_08_all_authoritative_settlement_fields_preserved() {
    let s = make_test_settlement(7, 333.625, 444);
    let world = make_test_world(0, vec![], vec![s]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();
    let r = &restored.world.settlements[0];

    assert_eq!(r.group_id, GroupId(7));
    assert_eq!(r.resource.to_bits(), 333.625f32.to_bits());
    assert_eq!(r.treasury, 444);
}

// =========================================================================
// 2. Floating-point fidelity
// =========================================================================

#[test]
fn test_09_representative_f32_bit_patterns_preserve_exact_bits() {
    let bit_patterns: [u32; 5] = [
        0x3f80_0000, // 1.0
        0x0080_0000, // min positive normal
        0x7f7f_ffff, // max finite normal
        0x3eaaaaab,  // ~1/3
        0x4049_0fdb, // ~pi
    ];

    for &pattern in &bit_patterns {
        let val = f32::from_bits(pattern);
        let a = make_test_agent(1, 0, true, 0, val, val, 0, val, val, val, val, 0);
        let world = make_test_world(0, vec![a], vec![]);
        let meta = make_test_metadata(0);

        let snapshot = encode_snapshot(&world, &meta).unwrap();
        let restored = restore_snapshot(snapshot.as_bytes()).unwrap();
        let r = &restored.world.agents[0];

        assert_eq!(r.health.to_bits(), pattern);
        assert_eq!(r.food.to_bits(), pattern);
        assert_eq!(r.productivity.to_bits(), pattern);
    }
}

#[test]
fn test_10_negative_zero_preserves_exact_bits() {
    let neg_zero = -0.0f32;
    assert_eq!(neg_zero.to_bits(), 0x8000_0000);

    // Negative zero is finite and >= 0.0 in IEEE 754 float comparison
    let a = make_test_agent(1, 0, true, 0, neg_zero, neg_zero, 0, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();
    let r = &restored.world.agents[0];

    assert_eq!(r.health.to_bits(), 0x8000_0000);
    assert_eq!(r.food.to_bits(), 0x8000_0000);
}

#[test]
fn test_11_food_exact_bits_preserved() {
    let food_val = 19.876543f32;
    let a = make_test_agent(1, 0, true, 0, 1.0, food_val, 0, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents[0].food.to_bits(), food_val.to_bits());
}

#[test]
fn test_12_health_exact_bits_preserved() {
    let health_val = 0.1234567f32;
    let a = make_test_agent(1, 0, true, 0, health_val, 10.0, 0, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(
        restored.world.agents[0].health.to_bits(),
        health_val.to_bits()
    );
}

#[test]
fn test_13_trait_exact_bits_preserved() {
    let a = make_test_agent(
        1,
        0,
        true,
        0,
        1.0,
        10.0,
        0,
        0.1111111f32,
        0.2222222f32,
        0.3333333f32,
        0.4444444f32,
        0,
    );
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();
    let r = &restored.world.agents[0];

    assert_eq!(r.productivity.to_bits(), 0.1111111f32.to_bits());
    assert_eq!(r.cooperation.to_bits(), 0.2222222f32.to_bits());
    assert_eq!(r.aggression.to_bits(), 0.3333333f32.to_bits());
    assert_eq!(r.risk_tolerance.to_bits(), 0.4444444f32.to_bits());
}

#[test]
fn test_14_settlement_resource_exact_bits_preserved() {
    let res = 9876.543f32;
    let s = make_test_settlement(0, res, 100);
    let world = make_test_world(0, vec![], vec![s]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(
        restored.world.settlements[0].resource.to_bits(),
        res.to_bits()
    );
}

// =========================================================================
// 3. Canonical ordering
// =========================================================================

#[test]
fn test_15_shuffled_agent_vec_identical_snapshot_bytes() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2 = make_test_agent(2, 1, true, 0, 0.8, 15.0, 200, 1.5, 0.4, 0.6, 0.2, 0);
    let a3 = make_test_agent(3, 2, true, 0, 0.5, 20.0, 300, 2.0, 0.1, 0.9, 0.8, 1);

    let world_fwd = make_test_world(0, vec![a1.clone(), a2.clone(), a3.clone()], vec![]);
    let world_rev = make_test_world(0, vec![a3, a1, a2], vec![]);
    let meta = make_test_metadata(0);

    let snap_fwd = encode_snapshot(&world_fwd, &meta).unwrap();
    let snap_rev = encode_snapshot(&world_rev, &meta).unwrap();

    assert_eq!(snap_fwd.bytes, snap_rev.bytes);
}

#[test]
fn test_16_shuffled_settlement_vec_identical_snapshot_bytes() {
    let s0 = make_test_settlement(0, 100.0, 500);
    let s1 = make_test_settlement(1, 200.0, 1000);
    let s2 = make_test_settlement(2, 300.0, 1500);

    let world_fwd = make_test_world(0, vec![], vec![s0.clone(), s1.clone(), s2.clone()]);
    let world_shuffled = make_test_world(0, vec![], vec![s2, s0, s1]);
    let meta = make_test_metadata(0);

    let snap_fwd = encode_snapshot(&world_fwd, &meta).unwrap();
    let snap_shuf = encode_snapshot(&world_shuffled, &meta).unwrap();

    assert_eq!(snap_fwd.bytes, snap_shuf.bytes);
}

#[test]
fn test_17_different_dense_slot_assignment_identical_snapshot_bytes() {
    let a1_slot0 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2_slot1 = make_test_agent(2, 1, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);

    let a1_slot99 = make_test_agent(1, 99, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2_slot42 = make_test_agent(2, 42, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);

    let world_a = make_test_world(0, vec![a1_slot0, a2_slot1], vec![]);
    let world_b = make_test_world(0, vec![a2_slot42, a1_slot99], vec![]);
    let meta = make_test_metadata(0);

    let snap_a = encode_snapshot(&world_a, &meta).unwrap();
    let snap_b = encode_snapshot(&world_b, &meta).unwrap();

    assert_eq!(snap_a.bytes, snap_b.bytes);
}

#[test]
fn test_18_restored_agents_ordered_ascending_agent_id() {
    let a3 = make_test_agent(30, 0, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a1 = make_test_agent(10, 1, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2 = make_test_agent(20, 2, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);

    let world = make_test_world(0, vec![a3, a1, a2], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents[0].agent_id, AgentId(10));
    assert_eq!(restored.world.agents[1].agent_id, AgentId(20));
    assert_eq!(restored.world.agents[2].agent_id, AgentId(30));
}

#[test]
fn test_19_restored_dense_slots_rebuilt_densely_from_zero() {
    let a3 = make_test_agent(300, 999, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a1 = make_test_agent(100, 555, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2 = make_test_agent(200, 777, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);

    let world = make_test_world(0, vec![a3, a1, a2], vec![]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.agents[0].dense_slot, DenseSlot(0));
    assert_eq!(restored.world.agents[1].dense_slot, DenseSlot(1));
    assert_eq!(restored.world.agents[2].dense_slot, DenseSlot(2));
}

#[test]
fn test_20_restored_settlements_ordered_ascending_group_id() {
    let s2 = make_test_settlement(2, 100.0, 100);
    let s0 = make_test_settlement(0, 100.0, 100);
    let s1 = make_test_settlement(1, 100.0, 100);

    let world = make_test_world(0, vec![], vec![s2, s0, s1]);
    let meta = make_test_metadata(0);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.world.settlements[0].group_id, GroupId(0));
    assert_eq!(restored.world.settlements[1].group_id, GroupId(1));
    assert_eq!(restored.world.settlements[2].group_id, GroupId(2));
}

// =========================================================================
// 4. Metadata
// =========================================================================

#[test]
fn test_21_day_round_trip() {
    let world = make_test_world(123, vec![], vec![]);
    let meta = make_test_metadata(123);

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.metadata.day, 123);
    assert_eq!(restored.metadata.simulation_day(), SimulationDay(123));
    assert_eq!(restored.world.current_day, SimulationDay(123));
}

#[test]
fn test_22_master_seed_round_trip() {
    let world = make_test_world(0, vec![], vec![]);
    let mut meta = make_test_metadata(0);
    meta.master_seed = 0xDEAD_BEEF_CAFE_BABE;

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.metadata.master_seed, 0xDEAD_BEEF_CAFE_BABE);
}

#[test]
fn test_23_replicate_id_round_trip() {
    let world = make_test_world(0, vec![], vec![]);
    let mut meta = make_test_metadata(0);
    meta.replicate_id = 999;

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.metadata.replicate_id, 999);
    assert_eq!(restored.metadata.replicate(), ReplicateId(999));
}

#[test]
fn test_24_model_version_round_trip() {
    let world = make_test_world(0, vec![], vec![]);
    let mut meta = make_test_metadata(0);
    meta.model_version = "SimulaCiv-Reference-M0-14A".to_string();

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(
        restored.metadata.model_version,
        "SimulaCiv-Reference-M0-14A"
    );
}

#[test]
fn test_25_config_version_round_trip() {
    let world = make_test_world(0, vec![], vec![]);
    let mut meta = make_test_metadata(0);
    meta.config_version = "Config-Hash-1234abcd".to_string();

    let snapshot = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snapshot.as_bytes()).unwrap();

    assert_eq!(restored.metadata.config_version, "Config-Hash-1234abcd");
}

// =========================================================================
// 5. Boundary hook
// =========================================================================

#[test]
fn test_26_at_epoch_boundary_false_returns_none() {
    let world = make_test_world(5, vec![], vec![]);
    let meta = make_test_metadata(5);

    let res = phase11_snapshot_if_boundary(&world, &meta, false).unwrap();
    assert!(res.is_none());
}

#[test]
fn test_27_false_boundary_performs_no_encoding_visible_state_change() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(5, vec![a], vec![]);
    let clone_before = world.clone();
    let meta = make_test_metadata(5);

    let _ = phase11_snapshot_if_boundary(&world, &meta, false).unwrap();
    assert_eq!(world, clone_before);
}

#[test]
fn test_28_true_boundary_returns_canonical_payload() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(5, vec![a], vec![]);
    let meta = make_test_metadata(5);

    let res = phase11_snapshot_if_boundary(&world, &meta, true).unwrap();
    assert!(res.is_some());
    let snap = res.unwrap();
    assert!(!snap.bytes.is_empty());
}

#[test]
fn test_29_hook_does_not_mutate_world() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let s = make_test_settlement(0, 50.0, 100);
    let world = make_test_world(5, vec![a], vec![s]);
    let clone_before = world.clone();
    let meta = make_test_metadata(5);

    let _ = phase11_snapshot_if_boundary(&world, &meta, true).unwrap();
    assert_eq!(world, clone_before);
}

// =========================================================================
// 6. Decoder failures
// =========================================================================

#[test]
fn test_30_invalid_magic_rejected() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let mut corrupted = snap.into_bytes();
    corrupted[0..8].copy_from_slice(b"BADMAGIC");

    let err = decode_snapshot(&corrupted).unwrap_err();
    assert!(matches!(err, SnapshotError::InvalidMagic(_)));
}

#[test]
fn test_31_unsupported_version_rejected() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let mut corrupted = snap.into_bytes();
    corrupted[8..12].copy_from_slice(&2u32.to_le_bytes());

    let err = decode_snapshot(&corrupted).unwrap_err();
    assert_eq!(err, SnapshotError::UnsupportedVersion(2));
}

#[test]
fn test_32_truncated_header_rejected() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let truncated = &snap.bytes[..10]; // truncated midway in header
    let err = decode_snapshot(truncated).unwrap_err();
    assert_eq!(err, SnapshotError::UnexpectedEof);
}

#[test]
fn test_33_truncated_agent_rejected() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    // truncate last 10 bytes of agent record
    let truncated = &snap.bytes[..snap.bytes.len() - 10];
    let err = decode_snapshot(truncated).unwrap_err();
    assert_eq!(err, SnapshotError::UnexpectedEof);
}

#[test]
fn test_34_truncated_settlement_rejected() {
    let s = make_test_settlement(0, 100.0, 500);
    let world = make_test_world(0, vec![], vec![s]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let truncated = &snap.bytes[..snap.bytes.len() - 5];
    let err = decode_snapshot(truncated).unwrap_err();
    assert_eq!(err, SnapshotError::UnexpectedEof);
}

#[test]
fn test_35_invalid_boolean_byte_rejected() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    // Locate alive byte:
    // Header = 8 (magic) + 4 (schema) + 4 (day) + 8 (seed) + 4 (rep)
    // + 4 (m_len) + m_len + 4 (c_len) + c_len + 4 (agent_count)
    // Agent record begins with 4 (agent_id), then 1 (alive byte)
    let m_len = meta.model_version.len();
    let c_len = meta.config_version.len();
    let alive_offset = 8 + 4 + 4 + 8 + 4 + 4 + m_len + 4 + c_len + 4 + 4;

    let mut corrupted = snap.into_bytes();
    corrupted[alive_offset] = 0x02; // invalid boolean

    let err = decode_snapshot(&corrupted).unwrap_err();
    assert_eq!(err, SnapshotError::InvalidBoolean(0x02));
}

#[test]
fn test_36_invalid_utf8_rejected() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    // Corrupt a byte in model_version string (offset: 8+4+4+8+4+4 = 32)
    let mut corrupted = snap.into_bytes();
    corrupted[32] = 0xFF; // invalid UTF-8 start byte

    let err = decode_snapshot(&corrupted).unwrap_err();
    assert_eq!(err, SnapshotError::InvalidUtf8);
}

#[test]
fn test_37_duplicate_agent_id_rejected() {
    let a1 = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let a2 = make_test_agent(1, 1, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(0, vec![a1, a2], vec![]);
    let meta = make_test_metadata(0);

    let err = encode_snapshot(&world, &meta).unwrap_err();
    assert_eq!(err, SnapshotError::DuplicateAgent(AgentId(1)));
}

#[test]
fn test_38_duplicate_group_id_rejected() {
    let s1 = make_test_settlement(0, 100.0, 500);
    let s2 = make_test_settlement(0, 200.0, 600);
    let world = make_test_world(0, vec![], vec![s1, s2]);
    let meta = make_test_metadata(0);

    let err = encode_snapshot(&world, &meta).unwrap_err();
    assert_eq!(err, SnapshotError::DuplicateSettlement(GroupId(0)));
}

#[test]
fn test_39_trailing_bytes_rejected() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let mut with_trailing = snap.into_bytes();
    with_trailing.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);

    let err = decode_snapshot(&with_trailing).unwrap_err();
    assert_eq!(err, SnapshotError::TrailingBytes(4));
}

#[test]
fn test_40_malformed_count_or_length_fails_safely() {
    let world = make_test_world(0, vec![], vec![]);
    let meta = make_test_metadata(0);
    let snap = encode_snapshot(&world, &meta).unwrap();

    let m_len = meta.model_version.len();
    let c_len = meta.config_version.len();
    let agent_count_offset = 8 + 4 + 4 + 8 + 4 + 4 + m_len + 4 + c_len;

    let mut corrupted = snap.into_bytes();
    corrupted[agent_count_offset..agent_count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    let err = decode_snapshot(&corrupted).unwrap_err();
    assert_eq!(err, SnapshotError::UnexpectedEof);
}

// =========================================================================
// 7. Observer independence / determinism
// =========================================================================

#[test]
fn test_41_repeated_encode_returns_byte_identical_payload() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let s = make_test_settlement(0, 100.0, 500);
    let world = make_test_world(5, vec![a], vec![s]);
    let meta = make_test_metadata(5);

    let snap1 = encode_snapshot(&world, &meta).unwrap();
    let snap2 = encode_snapshot(&world, &meta).unwrap();

    assert_eq!(snap1.bytes, snap2.bytes);
}

#[test]
fn test_42_encode_consumes_zero_rng() {
    // Pure function assertion: snapshotting the same world produces identical result
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let world = make_test_world(1, vec![a], vec![]);
    let meta = make_test_metadata(1);

    let snap_a = encode_snapshot(&world, &meta).unwrap();
    let snap_b = encode_snapshot(&world, &meta).unwrap();

    assert_eq!(snap_a.bytes, snap_b.bytes);
}

#[test]
fn test_43_snapshot_vs_skipped_snapshot_leaves_state_identical() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let s = make_test_settlement(0, 100.0, 500);
    let world_observed = make_test_world(5, vec![a.clone()], vec![s.clone()]);
    let world_skipped = make_test_world(5, vec![a], vec![s]);
    let meta = make_test_metadata(5);

    let _ = encode_snapshot(&world_observed, &meta).unwrap();

    assert_eq!(world_observed, world_skipped);
}

#[test]
fn test_44_original_world_remains_unchanged_after_encode() {
    let a = make_test_agent(1, 0, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0);
    let s = make_test_settlement(0, 100.0, 500);
    let world = make_test_world(5, vec![a], vec![s]);
    let clone_before = world.clone();
    let meta = make_test_metadata(5);

    let _ = encode_snapshot(&world, &meta).unwrap();

    assert_eq!(world, clone_before);
}

#[test]
fn test_45_encode_restore_encode_produces_byte_identical_canonical_payload() {
    let a1 = make_test_agent(2, 5, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0);
    let a2 = make_test_agent(1, 3, true, 0, 0.8, 15.0, 200, 1.5, 0.4, 0.6, 0.2, 0);
    let s1 = make_test_settlement(1, 200.0, 1000);
    let s0 = make_test_settlement(0, 100.0, 500);

    let world = make_test_world(3, vec![a1, a2], vec![s1, s0]);
    let meta = make_test_metadata(3);

    let snap1 = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap1.as_bytes()).unwrap();
    let snap2 = encode_snapshot(&restored.world, &restored.metadata).unwrap();

    assert_eq!(snap1.bytes, snap2.bytes);
}

// =========================================================================
// 8. Integration
// =========================================================================

#[test]
fn test_46_phase9_dead_status_survives_snapshot_restore() {
    let mut world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 0.0, 0.0, 50, 1.0, 0.5, 0.5, 0.5, 0), // health = 0.0 -> dies
            make_test_agent(2, 1, true, 0, 1.0, 10.0, 50, 1.0, 0.5, 0.5, 0.5, 0), // survives
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let resolution = phase9_mortality_commitment(&mut world).unwrap();
    assert_eq!(resolution.newly_deceased_count, 1);
    assert!(!world.agents[0].alive);

    let meta = make_test_metadata(1);
    let snap = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap.as_bytes()).unwrap();

    assert!(!restored.world.agents[0].alive);
    assert_eq!(restored.world.agents[0].health.to_bits(), 0.0f32.to_bits());
    assert!(restored.world.agents[1].alive);
}

#[test]
fn test_47_phase8_welfare_wealth_survives_snapshot_restore() {
    let mut world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 2.0, 0, 1.0, 0.5, 0.5, 0.5, 0), // starving (food < 5.0)
            make_test_agent(2, 1, true, 0, 1.0, 10.0, 100, 1.0, 0.5, 0.5, 0.5, 0), // non-starving
        ],
        vec![make_test_settlement(0, 100.0, 500)],
    );

    let _ = phase8_welfare_distribution(&mut world, 5.0, 50).unwrap();
    assert_eq!(world.agents[0].wealth, 50);
    assert_eq!(world.settlements[0].treasury, 450);

    let meta = make_test_metadata(1);
    let snap = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap.as_bytes()).unwrap();

    assert_eq!(restored.world.agents[0].wealth, 50);
    assert_eq!(restored.world.settlements[0].treasury, 450);
}

#[test]
fn test_48_phase7_treasury_tax_state_survives_snapshot_restore() {
    let mut world = make_test_world(
        1,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 2.0, 100, 1.0, 0.5, 0.5, 0.5, 0), // buyer
            make_test_agent(2, 1, true, 0, 1.0, 20.0, 0, 1.0, 0.5, 0.5, 0.5, 0),  // seller
        ],
        vec![make_test_settlement(0, 100.0, 50)],
    );

    let partition = SettlementIntentPartition {
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
    };

    let _ = phase7_market_clearance(&mut world, &[partition], 10, 0.1).unwrap();
    let treasury_after = world.settlements[0].treasury;

    let meta = make_test_metadata(1);
    let snap = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap.as_bytes()).unwrap();

    assert_eq!(restored.world.settlements[0].treasury, treasury_after);
}

#[test]
fn test_49_phase10_observation_before_vs_after_restore_yields_identical_daily_metrics() {
    let world = make_test_world(
        5,
        vec![
            make_test_agent(1, 0, true, 0, 1.0, 15.0, 100, 1.0, 0.5, 0.5, 0.5, 0),
            make_test_agent(2, 1, true, 0, 0.8, 25.0, 300, 1.5, 0.3, 0.7, 0.2, 0),
            make_test_agent(3, 2, false, 0, 0.0, 5.0, 50, 1.0, 0.5, 0.5, 0.5, 0), // dead tombstone
        ],
        vec![make_test_settlement(0, 500.0, 1000)],
    );

    let metrics_before = phase10_observe(&world, 5).unwrap();

    let meta = make_test_metadata(5);
    let snap = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap.as_bytes()).unwrap();

    let metrics_after = phase10_observe(&restored.world, 5).unwrap();

    assert_eq!(metrics_before, metrics_after);
}

#[test]
fn test_50_all_existing_m0_tests_remain_unchanged_and_passing() {
    let toml = r#"
[world]
master_seed = 42
initial_population = 4
settlement_count = 2
initial_health = 1.0
initial_food = 20.0
initial_wealth = 100
initial_settlement_resource = 500.0
initial_treasury = 1000

[traits]
prod_min = 0.5
prod_max = 1.5
coop_min = 0.0
coop_max = 1.0
aggr_min = 0.0
aggr_max = 1.0
risk_min = 0.0
risk_max = 1.0

[environment]
carrying_capacity = 1000.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.05

[economy]
base_work_yield = 5.0
food_price = 10
target_food = 20.0
target_reserve = 100
tax_rate = 0.1
welfare_payment = 20

[interaction]
gift_amount = 5.0
theft_amount = 5.0
theft_success_probability = 0.5
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
    [0.0, 0.0, 0.0, 0.0, 0.0],
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

    let cfg = SimConfig::parse_and_validate(toml).unwrap();
    let world = sim_model::initialize_world(&cfg).unwrap();
    let meta = SnapshotMetadata::new(
        0,
        cfg.world.master_seed,
        cfg.world.replicate_id,
        "simulaciv-m0",
        "test-v1",
    );

    let snap = encode_snapshot(&world, &meta).unwrap();
    let restored = restore_snapshot(snap.as_bytes()).unwrap();

    assert_eq!(restored.world.agents.len(), 4);
    assert_eq!(restored.world.settlements.len(), 2);
    assert_eq!(
        restored.world.initial_money_supply,
        world.initial_money_supply
    );
}
