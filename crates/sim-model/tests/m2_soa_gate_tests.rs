//! M2-26: Native SoA Multi-Phase Integration Gate Acceptance Tests.
//!
//! Validates end-to-end:
//! 1. 500-Day Determinism Parity: Full AoS pipeline vs Native Segmented SoA pipeline
//!    (Phase 2, Phase 3, Phase 8, Phase 9, Phase 10) produces bit-exact identical
//!    CanonicalStateHash, CanonicalMetricsHash, and CanonicalEventHash.
//! 2. Snapshot Parity: Snapshots emitted at days 100, 250, 500 by both pipelines
//!    are bit-exact identical and restore to identical state hashes with full event continuity.

use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, run_m0_day, run_m0_days, run_native_soa_day,
    run_native_soa_days,
};
use sim_model::snapshot::{decode_snapshot, restore_snapshot};
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

const EXPECTED_STATE_HASH: &str =
    "5b396f23a8195fd7155a7b9577b0eaca265e59768a81f0cafd8ab68c0d9d67b9";
const EXPECTED_METRICS_HASH: &str =
    "ffbadbfda9bba1f799d4e72eac222e4e58deca4905ee8447a44ece8cec3baa3b";
const EXPECTED_EVENT_HASH: &str =
    "2a40e01a7cd0b981eba037a14cf2f40c748ae0ff9e0df290ed802ba8b0c51cac";

fn make_config() -> SimConfig {
    SimConfig::parse_and_validate(GATE_CONFIG_TOML).expect("gate config is valid")
}

fn make_context() -> M0RunContext {
    M0RunContext::new(81985529216486895, 7)
}

#[test]
fn test_01_500_day_trajectory_parity() {
    let config = make_config();
    let context = make_context();

    let mut world_aos = initialize_world(&config).expect("world initializes");
    let mut world_soa = world_aos.clone();

    let mut metrics_aos = Vec::with_capacity(500);
    let mut events_aos = Vec::new();

    let mut metrics_soa = Vec::with_capacity(500);
    let mut events_soa = Vec::new();

    // Run 500 days with both pipelines, snapshot on day 199 (canonical graduation trajectory)
    for d in 0..500 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == 199,
        };

        let outcome_aos = run_m0_day(&mut world_aos, &config, &context, &options)
            .expect("AoS day execution succeeds");
        if let Some(m) = outcome_aos.metrics {
            metrics_aos.push(m);
        }
        events_aos.extend(outcome_aos.events);

        let outcome_soa = run_native_soa_day(&mut world_soa, &config, &context, &options)
            .expect("Native SoA day execution succeeds");
        if let Some(m) = outcome_soa.metrics {
            metrics_soa.push(m);
        }
        events_soa.extend(outcome_soa.events);
    }

    // 1. Bit-exact state equivalence
    assert_eq!(
        world_aos, world_soa,
        "WorldState mismatch between AoS and Native SoA!"
    );
    let hash_state_aos = canonical_state_hash(&world_aos).unwrap().to_hex();
    let hash_state_soa = canonical_state_hash(&world_soa).unwrap().to_hex();
    assert_eq!(hash_state_aos, hash_state_soa);
    assert_eq!(hash_state_soa, EXPECTED_STATE_HASH);

    // 2. Bit-exact metrics equivalence
    assert_eq!(
        metrics_aos, metrics_soa,
        "DailyMetrics mismatch between AoS and Native SoA!"
    );
    let hash_metrics_aos = canonical_metrics_hash(&metrics_aos).unwrap().to_hex();
    let hash_metrics_soa = canonical_metrics_hash(&metrics_soa).unwrap().to_hex();
    assert_eq!(hash_metrics_aos, hash_metrics_soa);
    assert_eq!(hash_metrics_soa, EXPECTED_METRICS_HASH);

    // 3. Bit-exact events equivalence
    assert_eq!(events_aos.len(), events_soa.len(), "Event count mismatch!");
    assert_eq!(
        events_aos, events_soa,
        "Events mismatch between AoS and Native SoA!"
    );
    let hash_events_aos = canonical_event_hash(&events_aos).unwrap().to_hex();
    let hash_events_soa = canonical_event_hash(&events_soa).unwrap().to_hex();
    assert_eq!(hash_events_aos, hash_events_soa);
    assert_eq!(hash_events_soa, EXPECTED_EVENT_HASH);
}

#[test]
fn test_02_snapshot_parity_at_day_100_250_500() {
    let config = make_config();
    let context = make_context();

    let snapshot_days = [100u32, 250, 500];

    for &target_day in &snapshot_days {
        let mut world_aos = initialize_world(&config).unwrap();
        let mut world_soa = world_aos.clone();

        // Run until boundary day (target_day - 1 produces snapshot with resume day = target_day)
        let prior_days = target_day - 1;
        let options_normal = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };

        if prior_days > 0 {
            let _ = run_m0_days(
                &mut world_aos,
                &config,
                &context,
                prior_days,
                &options_normal,
            )
            .expect("AoS prior days succeeds");
            let _ = run_native_soa_days(
                &mut world_soa,
                &config,
                &context,
                prior_days,
                &options_normal,
            )
            .expect("Native SoA prior days succeeds");
        }

        // Execute snapshot emission boundary day
        let options_boundary = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        };

        let outcome_aos = run_m0_day(&mut world_aos, &config, &context, &options_boundary)
            .expect("AoS boundary day succeeds");
        let outcome_soa = run_native_soa_day(&mut world_soa, &config, &context, &options_boundary)
            .expect("Native SoA boundary day succeeds");

        // Verify snapshot binary payload match
        let snap_bytes_aos = outcome_aos.snapshot.expect("AoS emitted snapshot");
        let snap_bytes_soa = outcome_soa.snapshot.expect("Native SoA emitted snapshot");
        assert_eq!(
            snap_bytes_aos, snap_bytes_soa,
            "Snapshot byte mismatch at day {}",
            target_day
        );

        // Decode and restore from both
        let decoded_aos = decode_snapshot(&snap_bytes_aos).expect("decode AoS snapshot");
        let decoded_soa = decode_snapshot(&snap_bytes_soa).expect("decode SoA snapshot");
        assert_eq!(decoded_aos, decoded_soa);

        let restored_aos = restore_snapshot(&snap_bytes_aos).expect("restore AoS snapshot");
        let restored_soa = restore_snapshot(&snap_bytes_soa).expect("restore SoA snapshot");

        // Compare restored world states
        assert_eq!(restored_aos.world, restored_soa.world);
        let hash_restored_aos = canonical_state_hash(&restored_aos.world).unwrap();
        let hash_restored_soa = canonical_state_hash(&restored_soa.world).unwrap();
        assert_eq!(hash_restored_aos, hash_restored_soa);

        // Resume execution from restored state for 25 days using Native SoA
        let mut resume_world = restored_soa.world;
        let outcomes_resumed =
            run_native_soa_days(&mut resume_world, &config, &context, 25, &options_normal)
                .expect("resume 25 days succeeds");

        // Continue continuous execution for 25 days
        let outcomes_continuous =
            run_native_soa_days(&mut world_soa, &config, &context, 25, &options_normal)
                .expect("continuous 25 days succeeds");

        // Verify resumed trajectory matches continuous trajectory
        assert_eq!(resume_world, world_soa);
        assert_eq!(
            canonical_state_hash(&resume_world).unwrap(),
            canonical_state_hash(&world_soa).unwrap()
        );
        assert_eq!(outcomes_resumed, outcomes_continuous);
    }
}
