//! Comprehensive integration test suite for M0-16A: Full-Day M0 Reference Runner.

use sim_core::{AgentId, DenseSlot, GroupId, Money, SimulationDay};
use sim_model::events::{Event, GLOBAL_PARTITION_KEY, ObservationEvent};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::runner::{DayExecutionOptions, M0RunContext, M0RunError, run_m0_day, run_m0_days};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::{AgentState, SettlementState, WorldState};
use sim_model::{SimConfig, initialize_world};

const TEST_CONFIG_TOML: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 6
settlement_count = 2
initial_health = 1.0
initial_food = 25.0
initial_wealth = 10000
initial_settlement_resource = 1000.0
initial_treasury = 5000

[traits]
prod_min = 0.8
prod_max = 1.5
coop_min = 0.2
coop_max = 0.8
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.1
risk_max = 0.5

[environment]
carrying_capacity = 5000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
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
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

fn make_test_config() -> SimConfig {
    SimConfig::parse_and_validate(TEST_CONFIG_TOML).expect("valid test toml")
}

fn make_test_context() -> M0RunContext {
    M0RunContext::new(81985529216486895, 7)
}

fn make_test_world() -> WorldState {
    let config = make_test_config();
    initialize_world(&config).expect("world init succeeds")
}

#[allow(clippy::too_many_arguments)]
fn make_manual_agent(
    id: u32,
    slot: u32,
    alive: bool,
    birth_day: u32,
    health: f32,
    food: f32,
    wealth: Money,
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
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.1,
        risk_tolerance: 0.2,
        group_id: GroupId(group_id),
    }
}

fn make_manual_settlement(gid: u16, resource: f32, treasury: Money) -> SettlementState {
    SettlementState {
        group_id: GroupId(gid),
        resource,
        treasury,
    }
}

// =========================================================================
// Group 1: Phase Order & Plumbing (Tests 1–9)
// =========================================================================

#[test]
fn test_01_one_full_day_executes_successfully() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome =
        run_m0_day(&mut world, &config, &context, &options).expect("day execution should succeed");

    assert_eq!(outcome.executed_day, 0);
    assert_eq!(world.current_day.as_u32(), 1);
    assert!(outcome.metrics.is_some());
    assert!(outcome.snapshot.is_some());
    assert!(!outcome.events.is_empty());
}

#[test]
fn test_02_phase1_state_visible_downstream() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let initial_r0 = world.settlements[0].resource;
    let _ = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // After Phase 1 regrowth and Phase 6A work, resource changed deterministically
    assert_ne!(world.settlements[0].resource, initial_r0);
}

#[test]
fn test_03_phase2_behavioral_ineligibility_affects_later_phases() {
    // Agent with 0 food and 0.01 health: Phase 2 starvation will reduce health to 0.0
    let a1 = make_manual_agent(1, 0, true, 0, 0.01, 0.0, 100, 0);
    let s0 = make_manual_settlement(0, 500.0, 1000);
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![a1],
        settlements: vec![s0],
        initial_money_supply: 1100,
    };

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // In Phase 9, agent should be newly deceased because health <= 0
    assert!(!world.agents[0].alive);
    // In Phase 10 metrics, living population should be 0
    assert_eq!(outcome.metrics.unwrap().population, 0);
}

#[test]
fn test_04_phase4_intents_feed_same_day_phase5() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Outcomes include Phase 6 events derived from Phase 5 partitions
    assert!(outcome.events.iter().any(|e| e.key.phase == 6));
}

#[test]
fn test_05_phase6_state_visible_to_phase7() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    // Verify run succeeds through market clearance with post-Phase-6 live state
    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.events.iter().any(|e| e.key.phase == 7));
}

#[test]
fn test_06_phase7_state_visible_to_phase8() {
    let mut world = make_test_world();
    // Make an agent eligible for welfare (food < starvation_threshold = 5.0)
    world.agents[0].food = 2.0;
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Phase 8 executed after Phase 7 and generated welfare distributed events
    assert!(outcome.events.iter().any(|e| e.key.phase == 8));
}

#[test]
fn test_07_phase8_state_visible_to_phase9() {
    let mut world = make_test_world();
    world.agents[0].food = 2.0; // triggers Phase 8 welfare
    world.agents[1].health = 0.01; // triggers Phase 9 mortality
    world.agents[1].food = 0.0;
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.events.iter().any(|e| e.key.phase == 8));
    assert!(outcome.events.iter().any(|e| e.key.phase == 9));
}

#[test]
fn test_08_phase9_death_excluded_from_phase10_metrics() {
    let a_dying = make_manual_agent(1, 0, true, 0, 0.01, 0.0, 50, 0);
    let a_healthy = make_manual_agent(2, 1, true, 0, 1.0, 50.0, 100, 0);
    let s0 = make_manual_settlement(0, 500.0, 1000);
    let mut world = WorldState {
        current_day: SimulationDay(0),
        agents: vec![a_dying, a_healthy],
        settlements: vec![s0],
        initial_money_supply: 1150,
    };

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Agent 1 dies; living population in Phase 10 must be exactly 1
    assert_eq!(outcome.metrics.unwrap().population, 1);
}

#[test]
fn test_09_phase10_occurs_before_phase11_flush() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Flushed events contain Phase 10 metrics observation before Phase 11 snapshot
    let p10_idx = outcome.events.iter().position(|e| e.key.phase == 10);
    let p11_idx = outcome.events.iter().position(|e| e.key.phase == 11);

    assert!(p10_idx.is_some());
    assert!(p11_idx.is_some());
    assert!(p10_idx.unwrap() < p11_idx.unwrap());
}

// =========================================================================
// Group 2: Day Semantics (Tests 10–15)
// =========================================================================

#[test]
fn test_10_day_d_used_for_day_d_metrics() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(7);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert_eq!(outcome.executed_day, 7);
    assert_eq!(outcome.metrics.unwrap().day, 7);
}

#[test]
fn test_11_day_d_event_keys_use_d() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(7);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    for event in &outcome.events {
        assert_eq!(event.key.day, 7);
    }
}

#[test]
fn test_12_successful_run_advances_current_day_d_to_d_plus_1_exactly_once() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(3);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert_eq!(outcome.executed_day, 3);
    assert_eq!(world.current_day.as_u32(), 4);
}

#[test]
fn test_13_repeated_successful_calls_execute_d_then_d_plus_1() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(10);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o1 = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert_eq!(o1.executed_day, 10);
    assert_eq!(world.current_day.as_u32(), 11);

    let o2 = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert_eq!(o2.executed_day, 11);
    assert_eq!(world.current_day.as_u32(), 12);
}

#[test]
fn test_14_day_overflow_rejected() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(u32::MAX);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let err = run_m0_day(&mut world, &config, &context, &options).unwrap_err();
    assert_eq!(err, M0RunError::DayOverflow);
    assert_eq!(world.current_day.as_u32(), u32::MAX);
}

#[test]
fn test_15_failed_execution_before_day_complete_does_not_advance_current_day() {
    // Agent references a non-existent settlement (causes Phase 3 to fail)
    let a_bad = make_manual_agent(1, 0, true, 0, 1.0, 10.0, 50, 999);
    let s0 = make_manual_settlement(0, 500.0, 1000);
    let mut world = WorldState {
        current_day: SimulationDay(5),
        agents: vec![a_bad],
        settlements: vec![s0],
        initial_money_supply: 1050,
    };

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let err = run_m0_day(&mut world, &config, &context, &options).unwrap_err();
    match err {
        M0RunError::Phase3(_) => {}
        other => panic!("expected Phase3 error, got {:?}", other),
    }

    // Day cursor must remain at day 5
    assert_eq!(world.current_day.as_u32(), 5);
}

// =========================================================================
// Group 3: Snapshot Cursor (Tests 16–21)
// =========================================================================

#[test]
fn test_16_snapshot_disabled_returns_none() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        snapshot_boundary: false,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.snapshot.is_none());
}

#[test]
fn test_17_snapshot_enabled_returns_some() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        snapshot_boundary: true,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.snapshot.is_some());
}

#[test]
fn test_18_snapshot_metadata_day_equals_d_plus_1() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(4);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let snapshot = outcome.snapshot.unwrap();
    let restored = restore_snapshot(&snapshot.bytes).unwrap();
    // Resume cursor in metadata must be D + 1 = 5
    assert_eq!(restored.metadata.day, 5);
}

#[test]
fn test_19_snapshot_event_key_day_equals_d() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(4);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let snap_event = outcome
        .events
        .iter()
        .find(|e| {
            matches!(
                &e.event,
                Event::Observation(ObservationEvent::SnapshotEmitted { .. })
            )
        })
        .expect("SnapshotEmitted event must exist");

    // EventKey day is the occurrence day D = 4
    assert_eq!(snap_event.key.day, 4);
    assert_eq!(snap_event.key.phase, 11);
    assert_eq!(snap_event.key.partition_key, GLOBAL_PARTITION_KEY);

    // Event payload carries resume day D + 1 = 5
    if let Event::Observation(ObservationEvent::SnapshotEmitted { day, .. }) = snap_event.event {
        assert_eq!(day, 5);
    } else {
        panic!("expected SnapshotEmitted payload");
    }
}

#[test]
fn test_20_restore_snapshot_sets_current_day_equals_d_plus_1() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(4);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let restored = restore_snapshot(&outcome.snapshot.unwrap().bytes).unwrap();
    assert_eq!(restored.world.current_day.as_u32(), 5);
}

#[test]
fn test_21_restored_snapshot_does_not_require_rerunning_d() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(4);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let mut restored = restore_snapshot(&outcome.snapshot.unwrap().bytes).unwrap();

    // Directly execute next day on restored world without re-running day 4
    let next_outcome = run_m0_day(&mut restored.world, &config, &context, &options).unwrap();
    assert_eq!(next_outcome.executed_day, 5);
    assert_eq!(restored.world.current_day.as_u32(), 6);
}

// =========================================================================
// Group 4: Metrics (Tests 22–25)
// =========================================================================

#[test]
fn test_22_metrics_enabled_returns_correct_dailymetrics() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        metrics_enabled: true,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let m = outcome.metrics.expect("metrics must be present");
    assert_eq!(m.day, 0);
    assert_eq!(m.population, 6);
    assert!(m.wealth_gini >= 0.0 && m.wealth_gini <= 1.0);
}

#[test]
fn test_23_metrics_disabled_returns_none() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        metrics_enabled: false,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.metrics.is_none());
}

#[test]
fn test_24_metrics_enabled_disabled_final_state_hash_identical() {
    let mut w_with = make_test_world();
    let mut w_without = w_with.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt_with = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: false,
        snapshot_boundary: false,
    };
    let opt_without = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: false,
        snapshot_boundary: false,
    };

    let _ = run_m0_day(&mut w_with, &config, &context, &opt_with).unwrap();
    let _ = run_m0_day(&mut w_without, &config, &context, &opt_without).unwrap();

    assert_eq!(
        canonical_state_hash(&w_with).unwrap(),
        canonical_state_hash(&w_without).unwrap()
    );
}

#[test]
fn test_25_metrics_event_only_exists_when_metrics_record_exists() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt1 = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let opt2 = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: true,
        snapshot_boundary: false,
    };

    let o1 = run_m0_day(&mut w1, &config, &context, &opt1).unwrap();
    let o2 = run_m0_day(&mut w2, &config, &context, &opt2).unwrap();

    assert!(o1.events.iter().any(|e| e.key.phase == 10));
    assert!(!o2.events.iter().any(|e| e.key.phase == 10));
}

// =========================================================================
// Group 5: Events (Tests 26–31)
// =========================================================================

#[test]
fn test_26_events_disabled_returns_empty() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        events_enabled: false,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(outcome.events.is_empty());
}

#[test]
fn test_27_events_enabled_returns_canonical_sorted_stream() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions {
        events_enabled: true,
        ..Default::default()
    };

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert!(!outcome.events.is_empty());
    for window in outcome.events.windows(2) {
        assert!(window[0].key < window[1].key);
    }
}

#[test]
fn test_28_event_buffer_empty_after_flush() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Flush guarantees all events are in outcome and no keys are duplicated
    let mut seen_keys = std::collections::HashSet::new();
    for e in outcome.events {
        assert!(seen_keys.insert(e.key));
    }
}

#[test]
fn test_29_all_returned_event_keys_are_canonical_ascending() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    let is_sorted = outcome.events.windows(2).all(|w| w[0].key < w[1].key);
    assert!(is_sorted);
}

#[test]
fn test_30_phase7_8_9_adapters_represented_where_applicable() {
    let mut world = make_test_world();
    // Agent 0 eligible for welfare
    world.agents[0].food = 2.0;
    // Agent 1 dying
    world.agents[1].health = 0.01;
    world.agents[1].food = 0.0;
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    // Phase 7, 8, and 9 events must be present
    assert!(outcome.events.iter().any(|e| e.key.phase == 7));
    assert!(outcome.events.iter().any(|e| e.key.phase == 8));
    assert!(outcome.events.iter().any(|e| e.key.phase == 9));
}

#[test]
fn test_31_snapshot_event_only_when_snapshot_actually_emitted() {
    let mut w_snap = make_test_world();
    let mut w_no_snap = w_snap.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt_snap = DayExecutionOptions {
        snapshot_boundary: true,
        events_enabled: true,
        metrics_enabled: true,
    };
    let opt_no_snap = DayExecutionOptions {
        snapshot_boundary: false,
        events_enabled: true,
        metrics_enabled: true,
    };

    let o_snap = run_m0_day(&mut w_snap, &config, &context, &opt_snap).unwrap();
    let o_no_snap = run_m0_day(&mut w_no_snap, &config, &context, &opt_no_snap).unwrap();

    assert!(o_snap.events.iter().any(|e| e.key.phase == 11));
    assert!(!o_no_snap.events.iter().any(|e| e.key.phase == 11));
}

// =========================================================================
// Group 6: Observer Independence (Tests 32–35)
// =========================================================================

#[test]
fn test_32_full_telemetry_vs_no_telemetry_final_state_hash_identical() {
    let mut w_full = make_test_world();
    let mut w_none = w_full.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt_full = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let opt_none = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: false,
        snapshot_boundary: false,
    };

    let _ = run_m0_day(&mut w_full, &config, &context, &opt_full).unwrap();
    let _ = run_m0_day(&mut w_none, &config, &context, &opt_none).unwrap();

    assert_eq!(w_full.current_day, w_none.current_day);
    assert_eq!(
        canonical_state_hash(&w_full).unwrap(),
        canonical_state_hash(&w_none).unwrap()
    );
}

#[test]
fn test_33_snapshot_enabled_vs_disabled_state_hash_identical() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt1 = DayExecutionOptions {
        snapshot_boundary: true,
        ..Default::default()
    };
    let opt2 = DayExecutionOptions {
        snapshot_boundary: false,
        ..Default::default()
    };

    let _ = run_m0_day(&mut w1, &config, &context, &opt1).unwrap();
    let _ = run_m0_day(&mut w2, &config, &context, &opt2).unwrap();

    assert_eq!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_34_events_enabled_vs_disabled_state_hash_identical() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    let config = make_test_config();
    let context = make_test_context();

    let opt1 = DayExecutionOptions {
        events_enabled: true,
        ..Default::default()
    };
    let opt2 = DayExecutionOptions {
        events_enabled: false,
        ..Default::default()
    };

    let _ = run_m0_day(&mut w1, &config, &context, &opt1).unwrap();
    let _ = run_m0_day(&mut w2, &config, &context, &opt2).unwrap();

    assert_eq!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_35_no_telemetry_option_changes_current_day_progression() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    let mut w3 = w1.clone();
    let config = make_test_config();
    let context = make_test_context();

    let _ = run_m0_day(&mut w1, &config, &context, &DayExecutionOptions::default()).unwrap();
    let _ = run_m0_day(
        &mut w2,
        &config,
        &context,
        &DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    )
    .unwrap();
    let _ = run_m0_day(
        &mut w3,
        &config,
        &context,
        &DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: false,
            snapshot_boundary: true,
        },
    )
    .unwrap();

    assert_eq!(w1.current_day.as_u32(), 1);
    assert_eq!(w2.current_day.as_u32(), 1);
    assert_eq!(w3.current_day.as_u32(), 1);
}

// =========================================================================
// Group 7: Determinism & Replay Equality (Tests 36–40)
// =========================================================================

#[test]
fn test_36_identical_one_day_replay_same_state_hash() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_day(&mut w_a, &config, &context, &options).unwrap();
    let _ = run_m0_day(&mut w_b, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w_a).unwrap(),
        canonical_state_hash(&w_b).unwrap()
    );
}

#[test]
fn test_37_identical_one_day_replay_same_metrics_hash() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o_a = run_m0_day(&mut w_a, &config, &context, &options).unwrap();
    let o_b = run_m0_day(&mut w_b, &config, &context, &options).unwrap();

    let m_a = o_a.metrics.unwrap();
    let m_b = o_b.metrics.unwrap();

    assert_eq!(
        canonical_metrics_hash(&[m_a]).unwrap(),
        canonical_metrics_hash(&[m_b]).unwrap()
    );
}

#[test]
fn test_38_identical_one_day_replay_same_event_hash() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o_a = run_m0_day(&mut w_a, &config, &context, &options).unwrap();
    let o_b = run_m0_day(&mut w_b, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_event_hash(&o_a.events).unwrap(),
        canonical_event_hash(&o_b.events).unwrap()
    );
}

#[test]
fn test_39_identical_snapshot_enabled_replay_byte_identical_snapshot() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o_a = run_m0_day(&mut w_a, &config, &context, &options).unwrap();
    let o_b = run_m0_day(&mut w_b, &config, &context, &options).unwrap();

    assert_eq!(o_a.snapshot.unwrap().bytes, o_b.snapshot.unwrap().bytes);
}

#[test]
fn test_40_repeated_run_from_same_initial_clone_yields_identical_dayoutcome_logical_values() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o_a = run_m0_day(&mut w_a, &config, &context, &options).unwrap();
    let o_b = run_m0_day(&mut w_b, &config, &context, &options).unwrap();

    assert_eq!(o_a, o_b);
}

// =========================================================================
// Group 8: Physical Layout Invariance (Tests 41–44)
// =========================================================================

#[test]
fn test_41_shuffled_agent_storage_same_final_state_hash() {
    let mut w_ordered = make_test_world();
    let mut w_shuffled = w_ordered.clone();
    w_shuffled.agents.reverse();

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_day(&mut w_ordered, &config, &context, &options).unwrap();
    let _ = run_m0_day(&mut w_shuffled, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w_ordered).unwrap(),
        canonical_state_hash(&w_shuffled).unwrap()
    );
}

#[test]
fn test_42_shuffled_settlement_storage_same_final_state_hash() {
    let mut w_ordered = make_test_world();
    let mut w_shuffled = w_ordered.clone();
    w_shuffled.settlements.reverse();

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_day(&mut w_ordered, &config, &context, &options).unwrap();
    let _ = run_m0_day(&mut w_shuffled, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w_ordered).unwrap(),
        canonical_state_hash(&w_shuffled).unwrap()
    );
}

#[test]
fn test_43_dense_slot_permutation_same_final_state_hash() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    // Permute dense slots
    for (i, a) in w2.agents.iter_mut().enumerate() {
        a.dense_slot = DenseSlot((i as u32) + 100);
    }

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_day(&mut w1, &config, &context, &options).unwrap();
    let _ = run_m0_day(&mut w2, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );
}

#[test]
fn test_44_full_observation_hashes_invariant_to_permitted_physical_layout_differences() {
    let mut w1 = make_test_world();
    let mut w2 = w1.clone();
    w2.agents.reverse();
    for (i, a) in w2.agents.iter_mut().enumerate() {
        a.dense_slot = DenseSlot((i as u32) + 50);
    }
    w2.settlements.reverse();

    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let o1 = run_m0_day(&mut w1, &config, &context, &options).unwrap();
    let o2 = run_m0_day(&mut w2, &config, &context, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w1).unwrap(),
        canonical_state_hash(&w2).unwrap()
    );

    let m1 = o1.metrics.unwrap();
    let m2 = o2.metrics.unwrap();
    assert_eq!(
        canonical_metrics_hash(&[m1]).unwrap(),
        canonical_metrics_hash(&[m2]).unwrap()
    );

    assert_eq!(
        canonical_event_hash(&o1.events).unwrap(),
        canonical_event_hash(&o2.events).unwrap()
    );
}

// =========================================================================
// Group 9: Sequential Multi-Day Smoke (Tests 45–50)
// =========================================================================

#[test]
fn test_45_run_3_sequential_days_successfully() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcomes = run_m0_days(&mut world, &config, &context, 3, &options).unwrap();
    assert_eq!(outcomes.len(), 3);
}

#[test]
fn test_46_current_day_advances_by_exactly_3() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(0);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_days(&mut world, &config, &context, 3, &options).unwrap();
    assert_eq!(world.current_day.as_u32(), 3);
}

#[test]
fn test_47_each_metric_day_is_distinct_ascending() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(0);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcomes = run_m0_days(&mut world, &config, &context, 3, &options).unwrap();
    let days: Vec<u32> = outcomes
        .iter()
        .map(|o| o.metrics.as_ref().unwrap().day)
        .collect();
    assert_eq!(days, vec![0, 1, 2]);
}

#[test]
fn test_48_event_days_are_bounded_to_their_executed_day() {
    let mut world = make_test_world();
    world.current_day = SimulationDay(0);
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcomes = run_m0_days(&mut world, &config, &context, 3, &options).unwrap();
    for (expected_day, outcome) in outcomes.iter().enumerate() {
        for event in &outcome.events {
            assert_eq!(event.key.day, expected_day as u32);
        }
    }
}

#[test]
fn test_49_same_3_day_replay_produces_same_final_state_hash() {
    let mut w_a = make_test_world();
    let mut w_b = w_a.clone();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let _ = run_m0_days(&mut w_a, &config, &context, 3, &options).unwrap();
    let _ = run_m0_days(&mut w_b, &config, &context, 3, &options).unwrap();

    assert_eq!(
        canonical_state_hash(&w_a).unwrap(),
        canonical_state_hash(&w_b).unwrap()
    );
}

#[test]
fn test_50_all_existing_m0_behavior_preserved() {
    let mut world = make_test_world();
    let config = make_test_config();
    let context = make_test_context();
    let options = DayExecutionOptions::default();

    let outcome = run_m0_day(&mut world, &config, &context, &options).unwrap();
    assert_eq!(outcome.executed_day, 0);
    assert_eq!(world.current_day.as_u32(), 1);
    assert!(world.agents.iter().all(|a| a.food >= 0.0));
    assert!(world.agents.iter().all(|a| a.wealth >= 0));
    assert!(world.settlements.iter().all(|s| s.resource >= 0.0));
    assert!(world.settlements.iter().all(|s| s.treasury >= 0));
}
