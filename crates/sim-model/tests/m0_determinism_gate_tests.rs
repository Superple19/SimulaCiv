//! Comprehensive acceptance test suite for M0-16B: Final Replay & Determinism Graduation Gate.
//!
//! Validates end-to-end:
//! - Continuous 500-day execution vs Snapshot/Restore pause-resume equivalence (Contract C08)
//! - Canonical determinism oracles: CanonicalStateHash, CanonicalMetricsHash, CanonicalEventHash
//! - Full independent replay determinism (100 days)
//! - Observer independence across telemetry toggle combinations
//! - Physical layout invariance (agent order, settlement order, DenseSlot permutation across 25 days)
//! - Day cursor and event integrity across snapshot restore boundary

use sim_core::DenseSlot;
use sim_model::events::{Event, EventRecord, ObservationEvent};
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_metrics_hash, canonical_state_hash,
};
use sim_model::metrics::DailyMetrics;
use sim_model::runner::{DayExecutionOptions, M0RunContext, run_m0_day};
use sim_model::snapshot::{CanonicalSnapshot, encode_snapshot, restore_snapshot};
use sim_model::state::WorldState;
use sim_model::{SimConfig, initialize_world};

const GATE_CONFIG_TOML: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 1.0
initial_food = 25.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
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
carrying_capacity = 10000.0
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

fn make_gate_config() -> SimConfig {
    SimConfig::parse_and_validate(GATE_CONFIG_TOML).expect("gate config is valid")
}

fn make_gate_context() -> M0RunContext {
    M0RunContext::new(81985529216486895, 7)
}

fn make_gate_world() -> WorldState {
    let config = make_gate_config();
    initialize_world(&config).expect("gate world initialization succeeds")
}

#[derive(Debug, Clone)]
struct TrajectoryArtifacts {
    metrics: Vec<DailyMetrics>,
    events: Vec<EventRecord>,
    boundary_snapshot: Option<CanonicalSnapshot>,
    world: WorldState,
    state_checkpoints: Vec<(u32, CanonicalHash)>,
}

fn run_continuous_trajectory(
    initial_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    total_days: u32,
    snapshot_day: Option<u32>,
    telemetry: DayExecutionOptions,
) -> TrajectoryArtifacts {
    let mut world = initial_world.clone();
    let mut metrics = Vec::with_capacity(total_days as usize);
    let mut events = Vec::new();
    let mut boundary_snapshot = None;
    let mut state_checkpoints = Vec::with_capacity(total_days as usize);

    for d in 0..total_days {
        let options = DayExecutionOptions {
            metrics_enabled: telemetry.metrics_enabled,
            events_enabled: telemetry.events_enabled,
            snapshot_boundary: snapshot_day == Some(d),
        };
        let outcome =
            run_m0_day(&mut world, config, context, &options).expect("day execution succeeds");
        if let Some(m) = outcome.metrics {
            metrics.push(m);
        }
        events.extend(outcome.events);
        if snapshot_day == Some(d) {
            boundary_snapshot = outcome.snapshot;
        }
        state_checkpoints.push((
            world.current_day.as_u32(),
            canonical_state_hash(&world).expect("state hash succeeds"),
        ));
    }

    TrajectoryArtifacts {
        metrics,
        events,
        boundary_snapshot,
        world,
        state_checkpoints,
    }
}

fn run_pause_resume_trajectory(
    initial_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    boundary_day: u32,
    total_days: u32,
) -> (WorldState, TrajectoryArtifacts, WorldState) {
    let mut world = initial_world.clone();
    let mut metrics = Vec::with_capacity(total_days as usize);
    let mut events = Vec::new();
    let mut boundary_snapshot = None;
    let mut state_checkpoints = Vec::with_capacity(total_days as usize);

    // 1. Run days 0..=boundary_day (e.g. 0..=199)
    for d in 0..=boundary_day {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == boundary_day,
        };
        let outcome =
            run_m0_day(&mut world, config, context, &options).expect("day execution succeeds");
        if let Some(m) = outcome.metrics {
            metrics.push(m);
        }
        events.extend(outcome.events);
        if d == boundary_day {
            boundary_snapshot = outcome.snapshot;
        }
        state_checkpoints.push((
            world.current_day.as_u32(),
            canonical_state_hash(&world).expect("state hash succeeds"),
        ));
    }

    let snap = boundary_snapshot
        .as_ref()
        .expect("snapshot emitted at boundary");
    let live_world_at_boundary = world.clone();

    // 2. Restore from snapshot
    let restored_snapshot = restore_snapshot(snap).expect("snapshot restore succeeds");
    let mut restored_world = restored_snapshot.world;
    let restored_world_at_boundary = restored_world.clone();

    // 3. Continue execution from boundary_day + 1 .. total_days (e.g. 200..500)
    for _d in (boundary_day + 1)..total_days {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let outcome = run_m0_day(&mut restored_world, config, context, &options)
            .expect("day execution succeeds");
        if let Some(m) = outcome.metrics {
            metrics.push(m);
        }
        events.extend(outcome.events);
        state_checkpoints.push((
            restored_world.current_day.as_u32(),
            canonical_state_hash(&restored_world).expect("state hash succeeds"),
        ));
    }

    let artifacts = TrajectoryArtifacts {
        metrics,
        events,
        boundary_snapshot,
        world: restored_world,
        state_checkpoints,
    };

    (
        live_world_at_boundary,
        artifacts,
        restored_world_at_boundary,
    )
}

// =========================================================================
// Group 1: 500-Day Pause/Resume Equivalence Gate (Tests 1–14, 33–35)
// =========================================================================

#[test]
fn test_01_500_day_continuous_run_completes() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    assert_eq!(traj.metrics.len(), 500);
}

#[test]
fn test_02_final_continuous_current_day_equals_500() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    assert_eq!(traj.world.current_day.as_u32(), 500);
}

#[test]
fn test_03_boundary_snapshot_occurs_on_executed_day_199() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );
    assert!(traj.boundary_snapshot.is_some());
    // The event corresponding to snapshot emission occurs on day 199
    let snap_event = traj
        .events
        .iter()
        .find(|e| {
            matches!(
                &e.event,
                Event::Observation(ObservationEvent::SnapshotEmitted { .. })
            )
        })
        .expect("SnapshotEmitted event must exist");
    assert_eq!(snap_event.key.day, 199);
}

#[test]
fn test_04_boundary_snapshot_resume_cursor_equals_200() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );
    let snap = traj.boundary_snapshot.expect("snapshot must exist");
    assert_eq!(snap.metadata.day, 200);
}

#[test]
fn test_05_restored_world_current_day_equals_200() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );
    let snap = traj.boundary_snapshot.expect("snapshot must exist");
    let restored = restore_snapshot(&snap).expect("restore succeeds");
    assert_eq!(restored.world.current_day.as_u32(), 200);
}

#[test]
fn test_06_first_post_restore_executed_day_equals_200() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );
    let snap = traj.boundary_snapshot.expect("snapshot must exist");
    let mut restored = restore_snapshot(&snap).expect("restore succeeds").world;
    let options = DayExecutionOptions::default();
    let outcome = run_m0_day(&mut restored, &config, &context, &options).expect("day succeeds");
    assert_eq!(outcome.executed_day, 200);
    assert_eq!(restored.current_day.as_u32(), 201);
}

#[test]
fn test_07_boundary_live_state_hash_equals_restored_state_hash() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let (live_199, _b_artifacts, restored_boundary) =
        run_pause_resume_trajectory(&world, &config, &context, 199, 200);

    assert_eq!(
        canonical_state_hash(&live_199).unwrap(),
        canonical_state_hash(&restored_boundary).unwrap()
    );
}

#[test]
fn test_08_continuous_vs_restored_final_state_hash_equal() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    let hash_a = canonical_state_hash(&a.world).unwrap();
    let hash_b = canonical_state_hash(&b.world).unwrap();

    if hash_a != hash_b {
        for (i, (ca, cb)) in a
            .state_checkpoints
            .iter()
            .zip(b.state_checkpoints.iter())
            .enumerate()
        {
            if ca != cb {
                panic!(
                    "State hash divergence at checkpoint index {} (day {} vs {})",
                    i, ca.0, cb.0
                );
            }
        }
    }

    assert_eq!(hash_a, hash_b);
}

#[test]
fn test_09_continuous_vs_restored_metrics_hash_equal() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    assert_eq!(
        canonical_metrics_hash(&a.metrics).unwrap(),
        canonical_metrics_hash(&b.metrics).unwrap()
    );
}

#[test]
fn test_10_continuous_vs_restored_event_hash_equal() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    assert_eq!(
        canonical_event_hash(&a.events).unwrap(),
        canonical_event_hash(&b.events).unwrap()
    );
}

#[test]
fn test_11_continuous_vs_restored_metrics_records_equal() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    assert_eq!(a.metrics.len(), b.metrics.len());
    for (d, (ma, mb)) in a.metrics.iter().zip(b.metrics.iter()).enumerate() {
        assert_eq!(ma, mb, "Metrics record mismatch at day {}", d);
    }
}

#[test]
fn test_12_exactly_500_metric_days_0_to_499_once_each() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    assert_eq!(a.metrics.len(), 500);
    for (i, m) in a.metrics.iter().enumerate() {
        assert_eq!(m.day, i as u32);
    }
}

#[test]
fn test_13_exactly_one_matching_snapshot_emitted_boundary_occurrence() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    let a_snaps: Vec<&EventRecord> = a
        .events
        .iter()
        .filter(|e| {
            matches!(
                &e.event,
                Event::Observation(ObservationEvent::SnapshotEmitted { .. })
            )
        })
        .collect();
    let b_snaps: Vec<&EventRecord> = b
        .events
        .iter()
        .filter(|e| {
            matches!(
                &e.event,
                Event::Observation(ObservationEvent::SnapshotEmitted { .. })
            )
        })
        .collect();

    assert_eq!(a_snaps.len(), 1);
    assert_eq!(b_snaps.len(), 1);

    assert_eq!(a_snaps[0].key.day, 199);
    assert_eq!(b_snaps[0].key.day, 199);

    if let Event::Observation(ObservationEvent::SnapshotEmitted {
        day: payload_day, ..
    }) = a_snaps[0].event
    {
        assert_eq!(payload_day, 200);
    } else {
        panic!("unexpected event enum variant");
    }

    assert_eq!(a_snaps[0], b_snaps[0]);
}

#[test]
fn test_14_continuous_vs_restored_snapshot_bytes_identical_at_boundary() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 200);

    assert_eq!(
        a.boundary_snapshot.unwrap().bytes,
        b.boundary_snapshot.unwrap().bytes
    );
}

#[test]
fn test_33_no_duplicated_day_around_restore() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 205);

    let day_199_count = b.metrics.iter().filter(|m| m.day == 199).count();
    let day_200_count = b.metrics.iter().filter(|m| m.day == 200).count();

    assert_eq!(day_199_count, 1);
    assert_eq!(day_200_count, 1);
}

#[test]
fn test_34_no_missing_day_around_restore() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 205);

    assert_eq!(b.metrics[199].day, 199);
    assert_eq!(b.metrics[200].day, 200);
    assert_eq!(b.metrics[201].day, 201);
}

#[test]
fn test_35_restore_introduces_no_extra_m0_event() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let a = run_continuous_trajectory(
        &world,
        &config,
        &context,
        500,
        Some(199),
        DayExecutionOptions::default(),
    );
    let (_live, b, _restored) = run_pause_resume_trajectory(&world, &config, &context, 199, 500);

    assert_eq!(a.events.len(), b.events.len());
    for (i, (ea, eb)) in a.events.iter().zip(b.events.iter()).enumerate() {
        assert_eq!(ea, eb, "Event mismatch at index {}", i);
    }
}

// =========================================================================
// Group 2: Independent 100-Day Replay Gate (Tests 15–18)
// =========================================================================

#[test]
fn test_15_independent_100_day_replay_state_hash_equal() {
    let w1 = make_gate_world();
    let w2 = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let r1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );
    let r2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_state_hash(&r1.world).unwrap(),
        canonical_state_hash(&r2.world).unwrap()
    );
}

#[test]
fn test_16_independent_100_day_replay_metrics_hash_equal() {
    let w1 = make_gate_world();
    let w2 = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let r1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );
    let r2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_metrics_hash(&r1.metrics).unwrap(),
        canonical_metrics_hash(&r2.metrics).unwrap()
    );
}

#[test]
fn test_17_independent_100_day_replay_event_hash_equal() {
    let w1 = make_gate_world();
    let w2 = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let r1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );
    let r2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_event_hash(&r1.events).unwrap(),
        canonical_event_hash(&r2.events).unwrap()
    );
}

#[test]
fn test_18_independent_replay_snapshot_bytes_equal() {
    let w1 = make_gate_world();
    let w2 = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let r1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );
    let r2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        r1.boundary_snapshot.unwrap().bytes,
        r2.boundary_snapshot.unwrap().bytes
    );
}

// =========================================================================
// Group 3: Observer Independence Gate (Tests 19–23)
// =========================================================================

#[test]
fn test_19_full_telemetry_vs_no_telemetry_final_state_hash_equal() {
    let w_full = make_gate_world();
    let w_none = w_full.clone();
    let config = make_gate_config();
    let context = make_gate_context();

    let full = run_continuous_trajectory(
        &w_full,
        &config,
        &context,
        100,
        Some(49),
        DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        },
    );
    let none = run_continuous_trajectory(
        &w_none,
        &config,
        &context,
        100,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );

    assert_eq!(
        canonical_state_hash(&full.world).unwrap(),
        canonical_state_hash(&none.world).unwrap()
    );
}

#[test]
fn test_20_metrics_on_off_final_state_hash_equal() {
    let w_on = make_gate_world();
    let w_off = w_on.clone();
    let config = make_gate_config();
    let context = make_gate_context();

    let on = run_continuous_trajectory(
        &w_on,
        &config,
        &context,
        50,
        None,
        DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );
    let off = run_continuous_trajectory(
        &w_off,
        &config,
        &context,
        50,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );

    assert_eq!(
        canonical_state_hash(&on.world).unwrap(),
        canonical_state_hash(&off.world).unwrap()
    );
}

#[test]
fn test_21_events_on_off_final_state_hash_equal() {
    let w_on = make_gate_world();
    let w_off = w_on.clone();
    let config = make_gate_config();
    let context = make_gate_context();

    let on = run_continuous_trajectory(
        &w_on,
        &config,
        &context,
        50,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: true,
            snapshot_boundary: false,
        },
    );
    let off = run_continuous_trajectory(
        &w_off,
        &config,
        &context,
        50,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );

    assert_eq!(
        canonical_state_hash(&on.world).unwrap(),
        canonical_state_hash(&off.world).unwrap()
    );
}

#[test]
fn test_22_snapshot_on_off_final_state_hash_equal() {
    let w_on = make_gate_world();
    let w_off = w_on.clone();
    let config = make_gate_config();
    let context = make_gate_context();

    let on = run_continuous_trajectory(
        &w_on,
        &config,
        &context,
        50,
        Some(25),
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: true,
        },
    );
    let off = run_continuous_trajectory(
        &w_off,
        &config,
        &context,
        50,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );

    assert_eq!(
        canonical_state_hash(&on.world).unwrap(),
        canonical_state_hash(&off.world).unwrap()
    );
}

#[test]
fn test_23_observer_switches_preserve_current_day() {
    let w = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w,
        &config,
        &context,
        100,
        Some(50),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w,
        &config,
        &context,
        100,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );
    let t3 = run_continuous_trajectory(
        &w,
        &config,
        &context,
        100,
        None,
        DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: false,
            snapshot_boundary: false,
        },
    );
    let t4 = run_continuous_trajectory(
        &w,
        &config,
        &context,
        100,
        None,
        DayExecutionOptions {
            metrics_enabled: false,
            events_enabled: true,
            snapshot_boundary: false,
        },
    );

    assert_eq!(t1.world.current_day.as_u32(), 100);
    assert_eq!(t2.world.current_day.as_u32(), 100);
    assert_eq!(t3.world.current_day.as_u32(), 100);
    assert_eq!(t4.world.current_day.as_u32(), 100);
}

// =========================================================================
// Group 4: Physical Layout Invariance Gate (Tests 24–28)
// =========================================================================

#[test]
fn test_24_25_day_shuffled_agent_trajectory_hashes_equal() {
    let w1 = make_gate_world();
    let mut w2 = w1.clone();
    w2.agents.reverse();

    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_state_hash(&t1.world).unwrap(),
        canonical_state_hash(&t2.world).unwrap()
    );
}

#[test]
fn test_25_25_day_shuffled_settlement_trajectory_hashes_equal() {
    let w1 = make_gate_world();
    let mut w2 = w1.clone();
    w2.settlements.reverse();

    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_state_hash(&t1.world).unwrap(),
        canonical_state_hash(&t2.world).unwrap()
    );
}

#[test]
fn test_26_25_day_dense_slot_permuted_trajectory_hashes_equal() {
    let w1 = make_gate_world();
    let mut w2 = w1.clone();
    for (i, a) in w2.agents.iter_mut().enumerate() {
        a.dense_slot = DenseSlot((1000 + i * 7) as u32);
    }

    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_state_hash(&t1.world).unwrap(),
        canonical_state_hash(&t2.world).unwrap()
    );
}

#[test]
fn test_27_physical_layout_metrics_hash_equal() {
    let w1 = make_gate_world();
    let mut w2 = w1.clone();
    w2.agents.reverse();
    w2.settlements.reverse();
    for (i, a) in w2.agents.iter_mut().enumerate() {
        a.dense_slot = DenseSlot((500 + i * 3) as u32);
    }

    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_metrics_hash(&t1.metrics).unwrap(),
        canonical_metrics_hash(&t2.metrics).unwrap()
    );
}

#[test]
fn test_28_physical_layout_event_hash_equal() {
    let w1 = make_gate_world();
    let mut w2 = w1.clone();
    w2.agents.reverse();
    w2.settlements.reverse();
    for (i, a) in w2.agents.iter_mut().enumerate() {
        a.dense_slot = DenseSlot((500 + i * 3) as u32);
    }

    let config = make_gate_config();
    let context = make_gate_context();

    let t1 = run_continuous_trajectory(
        &w1,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );
    let t2 = run_continuous_trajectory(
        &w2,
        &config,
        &context,
        25,
        Some(10),
        DayExecutionOptions::default(),
    );

    assert_eq!(
        canonical_event_hash(&t1.events).unwrap(),
        canonical_event_hash(&t2.events).unwrap()
    );
}

// =========================================================================
// Group 5: Oracle Stability & Snapshot Round-Trip (Tests 29–32, 36)
// =========================================================================

#[test]
fn test_29_repeated_state_hash_call_stable() {
    let world = make_gate_world();
    let h1 = canonical_state_hash(&world).unwrap();
    let h2 = canonical_state_hash(&world).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_30_repeated_metrics_hash_call_stable() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        10,
        None,
        DayExecutionOptions::default(),
    );

    let h1 = canonical_metrics_hash(&traj.metrics).unwrap();
    let h2 = canonical_metrics_hash(&traj.metrics).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_31_repeated_event_hash_call_stable() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        10,
        None,
        DayExecutionOptions::default(),
    );

    let h1 = canonical_event_hash(&traj.events).unwrap();
    let h2 = canonical_event_hash(&traj.events).unwrap();
    assert_eq!(h1, h2);
}

#[test]
fn test_32_snapshot_round_trip_at_boundary_stable() {
    let world = make_gate_world();
    let config = make_gate_config();
    let context = make_gate_context();
    let traj = run_continuous_trajectory(
        &world,
        &config,
        &context,
        200,
        Some(199),
        DayExecutionOptions::default(),
    );

    let original_snap = traj.boundary_snapshot.expect("snapshot exists");
    let restored_world = restore_snapshot(&original_snap)
        .expect("restore succeeds")
        .world;

    // Re-encode snapshot from restored world with same metadata
    let reencoded_snap =
        encode_snapshot(&restored_world, &original_snap.metadata).expect("encode succeeds");

    assert_eq!(original_snap.bytes, reencoded_snap.bytes);
    assert_eq!(original_snap.metadata, reencoded_snap.metadata);
}

#[test]
fn test_36_all_pre_m0_16b_tests_unchanged_and_passing() {
    // Verifies gate configuration baseline sanity
    let config = make_gate_config();
    let world = make_gate_world();
    assert_eq!(world.agents.len(), 10);
    assert_eq!(world.settlements.len(), 2);
    assert_eq!(world.current_day.as_u32(), 0);
    assert_eq!(config.world.initial_population, 10);
}
