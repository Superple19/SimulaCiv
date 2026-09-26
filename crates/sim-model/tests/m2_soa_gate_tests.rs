//! M2-26: Native SoA Multi-Phase Integration Gate Acceptance Tests.
//!
//! Validates end-to-end:
//! 1. 500-Day Determinism Parity: Full AoS pipeline vs Native Segmented SoA pipeline
//!    (Phase 2, Phase 3, Phase 8, Phase 9, Phase 10) produces bit-exact identical
//!    CanonicalStateHash, CanonicalMetricsHash, and CanonicalEventHash.
//! 2. Snapshot Parity: Snapshots emitted at days 100, 250, 500 by both pipelines
//!    are bit-exact identical and restore to identical state hashes with full event continuity.

use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, run_hybrid_authority_day,
    run_hybrid_authority_day_with_candidate_scratch, run_hybrid_authority_days,
    run_hybrid_authority_days_with_phase3_linear_scan,
    run_hybrid_authority_days_with_phase5_baseline,
    run_hybrid_authority_days_with_phase6b_baseline,
    run_hybrid_authority_days_with_phase8_full_scan, run_hybrid_scope_isolated_day,
    run_hybrid_scope_isolated_days, run_m0_day, run_m0_days, run_native_soa_day,
    run_native_soa_days,
};
use sim_model::snapshot::{decode_snapshot, restore_snapshot};
use sim_model::state::{HybridWorldState, SettlementState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Action, AgentState, Intent, Phase3ScarcityScratch, Phase4CandidateIndexScratch,
    Phase8WelfareScratch, PrimaryActionChoice, SimConfig, generate_intents_storage_into_baseline,
    generate_intents_storage_into_variant_c, generate_intents_storage_into_variant_d,
    generate_intents_storage_with_candidate_index, generate_intents_storage_with_scratch,
    initialize_world, phase3_observation_and_features,
    phase3_observation_and_features_storage_into,
    phase3_observation_and_features_storage_with_scratch, phase4_generate_intents_into,
    phase4_generate_intents_storage_into, phase4_primary_action_selection_into,
    phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_baseline, phase5_partition_intents_baseline,
    phase5_partition_intents_from_vec, phase8_welfare_distribution_storage_full_scan,
    phase8_welfare_distribution_storage_with_scratch,
};

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

#[test]
fn test_03_hybrid_authority_500_day_trajectory_parity() {
    let config = make_config();
    let context = make_context();

    let mut world_aos = initialize_world(&config).expect("world initializes");
    let mut hybrid_world = HybridWorldState::hybrid(world_aos.clone());

    let mut metrics_aos = Vec::with_capacity(500);
    let mut events_aos = Vec::new();

    let mut metrics_hybrid = Vec::with_capacity(500);
    let mut events_hybrid = Vec::new();

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

        let outcome_hybrid =
            run_hybrid_authority_day(&mut hybrid_world, &config, &context, &options)
                .expect("Hybrid authority day execution succeeds");
        if let Some(m) = outcome_hybrid.metrics {
            metrics_hybrid.push(m);
        }
        events_hybrid.extend(outcome_hybrid.events);
    }

    // 1. Bit-exact state equivalence
    let reconstructed_agents = hybrid_world.agents_view();
    assert_eq!(
        world_aos.agents, reconstructed_agents,
        "Agent state mismatch between AoS and Hybrid Authority!"
    );
    assert_eq!(
        world_aos.settlements, hybrid_world.world.settlements,
        "Settlement state mismatch between AoS and Hybrid Authority!"
    );

    let hash_state_aos = canonical_state_hash(&world_aos).unwrap().to_hex();
    let hash_state_hybrid = hybrid_world.canonical_state_hash().unwrap().to_hex();
    assert_eq!(hash_state_aos, hash_state_hybrid);
    assert_eq!(hash_state_hybrid, EXPECTED_STATE_HASH);

    // 2. Bit-exact metrics equivalence
    assert_eq!(
        metrics_aos, metrics_hybrid,
        "DailyMetrics mismatch between AoS and Hybrid Authority!"
    );
    let hash_metrics_aos = canonical_metrics_hash(&metrics_aos).unwrap().to_hex();
    let hash_metrics_hybrid = canonical_metrics_hash(&metrics_hybrid).unwrap().to_hex();
    assert_eq!(hash_metrics_aos, hash_metrics_hybrid);
    assert_eq!(hash_metrics_hybrid, EXPECTED_METRICS_HASH);

    // 3. Bit-exact events equivalence
    assert_eq!(
        events_aos.len(),
        events_hybrid.len(),
        "Event count mismatch!"
    );
    assert_eq!(
        events_aos, events_hybrid,
        "Events mismatch between AoS and Hybrid Authority!"
    );
    let hash_events_aos = canonical_event_hash(&events_aos).unwrap().to_hex();
    let hash_events_hybrid = canonical_event_hash(&events_hybrid).unwrap().to_hex();
    assert_eq!(hash_events_aos, hash_events_hybrid);
    assert_eq!(hash_events_hybrid, EXPECTED_EVENT_HASH);
}

#[test]
fn test_04_hybrid_authority_snapshot_parity_at_day_100_250_500() {
    let config = make_config();
    let context = make_context();

    let snapshot_days = [100u32, 250, 500];

    for &target_day in &snapshot_days {
        let mut world_aos = initialize_world(&config).unwrap();
        let mut hybrid_world = HybridWorldState::hybrid(world_aos.clone());

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
            let _ = run_hybrid_authority_days(
                &mut hybrid_world,
                &config,
                &context,
                prior_days,
                &options_normal,
            )
            .expect("Hybrid Authority prior days succeeds");
        }

        // Execute snapshot emission boundary day
        let options_boundary = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        };

        let outcome_aos = run_m0_day(&mut world_aos, &config, &context, &options_boundary)
            .expect("AoS boundary day succeeds");
        let outcome_hybrid =
            run_hybrid_authority_day(&mut hybrid_world, &config, &context, &options_boundary)
                .expect("Hybrid Authority boundary day succeeds");

        // Verify snapshot binary payload match
        let snap_bytes_aos = outcome_aos.snapshot.expect("AoS emitted snapshot");
        let snap_bytes_hybrid = outcome_hybrid
            .snapshot
            .expect("Hybrid Authority emitted snapshot");
        assert_eq!(
            snap_bytes_aos, snap_bytes_hybrid,
            "Snapshot byte mismatch at day {}",
            target_day
        );

        // Decode and restore from both
        let decoded_aos = decode_snapshot(&snap_bytes_aos).expect("decode AoS snapshot");
        let decoded_hybrid = decode_snapshot(&snap_bytes_hybrid).expect("decode Hybrid snapshot");
        assert_eq!(decoded_aos, decoded_hybrid);

        let restored_aos = restore_snapshot(&snap_bytes_aos).expect("restore AoS snapshot");
        let restored_hybrid =
            restore_snapshot(&snap_bytes_hybrid).expect("restore Hybrid snapshot");

        // Compare restored world states
        assert_eq!(restored_aos.world, restored_hybrid.world);
        let hash_restored_aos = canonical_state_hash(&restored_aos.world).unwrap();
        let hash_restored_hybrid = canonical_state_hash(&restored_hybrid.world).unwrap();
        assert_eq!(hash_restored_aos, hash_restored_hybrid);

        // Resume execution from restored state for 25 days using Hybrid Authority
        let mut resume_hybrid = HybridWorldState::hybrid(restored_hybrid.world);
        let outcomes_resumed =
            run_hybrid_authority_days(&mut resume_hybrid, &config, &context, 25, &options_normal)
                .expect("resume 25 days succeeds");

        // Continue continuous execution for 25 days
        let outcomes_continuous =
            run_hybrid_authority_days(&mut hybrid_world, &config, &context, 25, &options_normal)
                .expect("continuous 25 days succeeds");

        // Verify resumed trajectory matches continuous trajectory
        assert_eq!(
            resume_hybrid.canonical_state_hash().unwrap(),
            hybrid_world.canonical_state_hash().unwrap()
        );
        assert_eq!(outcomes_resumed, outcomes_continuous);
    }
}

#[test]
fn test_05_hybrid_scope_isolated_500_day_trajectory_parity() {
    let config = make_config();
    let context = make_context();

    let mut world_aos = initialize_world(&config).expect("world initializes");
    let mut world_soa = world_aos.clone();
    let mut hybrid_isolated = HybridWorldState::hybrid(world_aos.clone());
    let mut hybrid_full = HybridWorldState::hybrid(world_aos.clone());

    let mut metrics_aos = Vec::with_capacity(500);
    let mut events_aos = Vec::new();

    let mut metrics_soa = Vec::with_capacity(500);
    let mut events_soa = Vec::new();

    let mut metrics_isolated = Vec::with_capacity(500);
    let mut events_isolated = Vec::new();

    let mut metrics_full = Vec::with_capacity(500);
    let mut events_full = Vec::new();

    for d in 0..500 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == 199,
        };

        // Pipeline A: Legacy AoS
        let outcome_a = run_m0_day(&mut world_aos, &config, &context, &options)
            .expect("Pipeline A (AoS) day execution succeeds");
        if let Some(m) = outcome_a.metrics {
            metrics_aos.push(m);
        }
        events_aos.extend(outcome_a.events);

        // Pipeline B: M2-26 Native SoA Gate
        let outcome_b = run_native_soa_day(&mut world_soa, &config, &context, &options)
            .expect("Pipeline B (M2-26) day execution succeeds");
        if let Some(m) = outcome_b.metrics {
            metrics_soa.push(m);
        }
        events_soa.extend(outcome_b.events);

        // Pipeline C: Hybrid Authority Scope-Isolated
        let outcome_c =
            run_hybrid_scope_isolated_day(&mut hybrid_isolated, &config, &context, &options)
                .expect("Pipeline C (Scope-Isolated) day execution succeeds");
        if let Some(m) = outcome_c.metrics {
            metrics_isolated.push(m);
        }
        events_isolated.extend(outcome_c.events);

        // Pipeline D: Current Full Hybrid
        let outcome_d = run_hybrid_authority_day(&mut hybrid_full, &config, &context, &options)
            .expect("Pipeline D (Full Hybrid) day execution succeeds");
        if let Some(m) = outcome_d.metrics {
            metrics_full.push(m);
        }
        events_full.extend(outcome_d.events);
    }

    // 1. Bit-exact CanonicalStateHash across A, B, C, D
    let hash_a = canonical_state_hash(&world_aos).unwrap().to_hex();
    let hash_b = canonical_state_hash(&world_soa).unwrap().to_hex();
    let hash_c = hybrid_isolated.canonical_state_hash().unwrap().to_hex();
    let hash_d = hybrid_full.canonical_state_hash().unwrap().to_hex();

    assert_eq!(hash_a, EXPECTED_STATE_HASH);
    assert_eq!(hash_b, EXPECTED_STATE_HASH);
    assert_eq!(hash_c, EXPECTED_STATE_HASH);
    assert_eq!(hash_d, EXPECTED_STATE_HASH);

    // 2. Bit-exact CanonicalMetricsHash across A, B, C, D
    let mhash_a = canonical_metrics_hash(&metrics_aos).unwrap().to_hex();
    let mhash_b = canonical_metrics_hash(&metrics_soa).unwrap().to_hex();
    let mhash_c = canonical_metrics_hash(&metrics_isolated).unwrap().to_hex();
    let mhash_d = canonical_metrics_hash(&metrics_full).unwrap().to_hex();

    assert_eq!(mhash_a, EXPECTED_METRICS_HASH);
    assert_eq!(mhash_b, EXPECTED_METRICS_HASH);
    assert_eq!(mhash_c, EXPECTED_METRICS_HASH);
    assert_eq!(mhash_d, EXPECTED_METRICS_HASH);

    // 3. Bit-exact CanonicalEventHash across A, B, C, D
    let ehash_a = canonical_event_hash(&events_aos).unwrap().to_hex();
    let ehash_b = canonical_event_hash(&events_soa).unwrap().to_hex();
    let ehash_c = canonical_event_hash(&events_isolated).unwrap().to_hex();
    let ehash_d = canonical_event_hash(&events_full).unwrap().to_hex();

    assert_eq!(ehash_a, EXPECTED_EVENT_HASH);
    assert_eq!(ehash_b, EXPECTED_EVENT_HASH);
    assert_eq!(ehash_c, EXPECTED_EVENT_HASH);
    assert_eq!(ehash_d, EXPECTED_EVENT_HASH);
}

#[test]
fn test_06_hybrid_scope_isolated_snapshot_parity_at_day_100_250_500() {
    let config = make_config();
    let context = make_context();

    let snapshot_days = [100u32, 250, 500];

    for &target_day in &snapshot_days {
        let mut world_aos = initialize_world(&config).unwrap();
        let mut hybrid_isolated = HybridWorldState::hybrid(world_aos.clone());
        let mut hybrid_full = HybridWorldState::hybrid(world_aos.clone());

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
            let _ = run_hybrid_scope_isolated_days(
                &mut hybrid_isolated,
                &config,
                &context,
                prior_days,
                &options_normal,
            )
            .expect("Scope-Isolated prior days succeeds");
            let _ = run_hybrid_authority_days(
                &mut hybrid_full,
                &config,
                &context,
                prior_days,
                &options_normal,
            )
            .expect("Full Hybrid prior days succeeds");
        }

        let options_boundary = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        };

        let outcome_a = run_m0_day(&mut world_aos, &config, &context, &options_boundary)
            .expect("AoS boundary day succeeds");
        let outcome_c = run_hybrid_scope_isolated_day(
            &mut hybrid_isolated,
            &config,
            &context,
            &options_boundary,
        )
        .expect("Scope-Isolated boundary day succeeds");
        let outcome_d =
            run_hybrid_authority_day(&mut hybrid_full, &config, &context, &options_boundary)
                .expect("Full Hybrid boundary day succeeds");

        // Verify snapshot binary payload match (A == C == D)
        let snap_bytes_a = outcome_a.snapshot.expect("A emitted snapshot");
        let snap_bytes_c = outcome_c.snapshot.expect("C emitted snapshot");
        let snap_bytes_d = outcome_d.snapshot.expect("D emitted snapshot");

        assert_eq!(
            snap_bytes_a, snap_bytes_c,
            "Snapshot mismatch A vs C at day {}",
            target_day
        );
        assert_eq!(
            snap_bytes_a, snap_bytes_d,
            "Snapshot mismatch A vs D at day {}",
            target_day
        );

        // Decode and restore from C
        let _decoded_c = decode_snapshot(&snap_bytes_c).expect("decode C snapshot");
        let restored_c = restore_snapshot(&snap_bytes_c).expect("restore C snapshot");
        assert_eq!(restored_c.world, world_aos);
        let hash_restored_c = canonical_state_hash(&restored_c.world).unwrap();
        let hash_restored_a = canonical_state_hash(&world_aos).unwrap();
        assert_eq!(hash_restored_a, hash_restored_c);

        // Resume execution from restored state for 25 days using Scope-Isolated pipeline
        let mut resume_c = HybridWorldState::hybrid(restored_c.world);
        let outcomes_resumed =
            run_hybrid_scope_isolated_days(&mut resume_c, &config, &context, 25, &options_normal)
                .expect("resume 25 days succeeds");

        // Continue continuous execution for 25 days
        let outcomes_continuous = run_hybrid_scope_isolated_days(
            &mut hybrid_isolated,
            &config,
            &context,
            25,
            &options_normal,
        )
        .expect("continuous 25 days succeeds");

        // Verify resumed trajectory matches continuous trajectory
        assert_eq!(
            resume_c.canonical_state_hash().unwrap(),
            hybrid_isolated.canonical_state_hash().unwrap()
        );
        assert_eq!(outcomes_resumed, outcomes_continuous);
    }
}

#[test]
fn test_07_phase4_variants_semantic_parity() {
    let populations = [10, 25, 50, 100];
    for &pop in &populations {
        let toml = format!(
            r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = {}
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
action_biases = [0.1, 0.2, 0.15, 0.3, 0.25, 0.05]
base_weight_matrix = [
  [0.1, 0.2, -0.1, 0.0, 0.1],
  [0.3, -0.2, 0.1, 0.2, -0.1],
  [-0.1, 0.3, 0.0, -0.1, 0.2],
  [0.2, 0.1, -0.3, 0.1, 0.0],
  [-0.2, 0.0, 0.3, -0.2, 0.1],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#,
            pop
        );

        let config = SimConfig::parse_and_validate(&toml).unwrap();
        let _context = M0RunContext {
            master_seed: config.world.master_seed,
            replicate_id: config.world.replicate_id,
        };

        let mut world = initialize_world(&config).unwrap();
        // Mutate some agent values across settlements to induce diverse actions
        for (i, agent) in world.agents.iter_mut().enumerate() {
            if i % 7 == 0 {
                agent.food = 2.0; // below starvation threshold -> candidate for GiveFood
            } else if i % 5 == 0 {
                agent.food = 35.0; // surplus food -> potential seller / giver
            } else if i % 11 == 0 {
                agent.health = 0.3; // low health
            }
        }

        let features = phase3_observation_and_features(&world, &config).unwrap();
        let storage = SegmentedAgentStorage::from_agents(&world.agents);
        let current_day = world.current_day;

        // Variant A: AoS
        let mut choices_a = Vec::new();
        phase4_primary_action_selection_into(&world, &config, &features, &mut choices_a).unwrap();
        let mut intents_a = Vec::new();
        phase4_generate_intents_into(&world, &config, &choices_a, &mut intents_a).unwrap();

        // Variant B: Current Native (Baseline)
        let mut choices_b = Vec::new();
        phase4_primary_action_selection_storage_into_baseline(
            &storage,
            current_day,
            &config,
            &features,
            &mut choices_b,
        )
        .unwrap();
        let mut intents_b = Vec::new();
        generate_intents_storage_into_baseline(
            &storage,
            current_day,
            &config,
            &choices_b,
            &mut intents_b,
        )
        .unwrap();

        // Variant C: Native + DenseSlot Direct
        let mut choices_c = Vec::new();
        phase4_primary_action_selection_storage_into(
            &storage,
            current_day,
            &config,
            &features,
            &mut choices_c,
        )
        .unwrap();
        let mut intents_c = Vec::new();
        generate_intents_storage_into_variant_c(
            &storage,
            current_day,
            &config,
            &choices_c,
            &mut intents_c,
        )
        .unwrap();

        // Variant D: Native + DenseSlot Direct + Candidate Scratch Reuse
        let mut choices_d = Vec::new();
        phase4_primary_action_selection_storage_into(
            &storage,
            current_day,
            &config,
            &features,
            &mut choices_d,
        )
        .unwrap();
        let mut candidate_scratch = Vec::with_capacity(storage.len());
        let mut intents_d = Vec::new();
        generate_intents_storage_into_variant_d(
            &storage,
            current_day,
            &config,
            &choices_d,
            &mut candidate_scratch,
            &mut intents_d,
        )
        .unwrap();

        // Variant E: Final Optimized Native Phase 4
        let mut choices_e = Vec::new();
        phase4_primary_action_selection_storage_into(
            &storage,
            current_day,
            &config,
            &features,
            &mut choices_e,
        )
        .unwrap();
        let mut intents_e = Vec::new();
        phase4_generate_intents_storage_into(
            &storage,
            current_day,
            &config,
            &choices_e,
            &mut intents_e,
        )
        .unwrap();

        // Verify 100% bit-exact parity across all variants
        assert_eq!(choices_a, choices_b, "Pop {}: Choices A != B", pop);
        assert_eq!(choices_a, choices_c, "Pop {}: Choices A != C", pop);
        assert_eq!(choices_a, choices_d, "Pop {}: Choices A != D", pop);
        assert_eq!(choices_a, choices_e, "Pop {}: Choices A != E", pop);

        assert_eq!(intents_a, intents_b, "Pop {}: Intents A != B", pop);
        assert_eq!(intents_a, intents_c, "Pop {}: Intents A != C", pop);
        assert_eq!(intents_a, intents_d, "Pop {}: Intents A != D", pop);
        assert_eq!(intents_a, intents_e, "Pop {}: Intents A != E", pop);
    }
}

#[test]
fn test_08_phase4_candidate_scratch_reuse_preserves_runner_outputs() {
    let mut config = make_config();
    config.world.initial_population = 100;
    config.world.settlement_count = 5;
    config.decision.action_biases = [0.0, 0.0, 0.0, 8.0, 8.0, 0.0];
    let context = make_context();

    let mut initial_world = initialize_world(&config).unwrap();
    for (index, agent) in initial_world.agents.iter_mut().enumerate() {
        if index % 5 == 0 {
            agent.food = 2.0;
        } else if index % 7 == 0 {
            agent.food = 0.0;
        }
    }

    let mut production_path = HybridWorldState::hybrid(initial_world.clone());
    let mut persistent_path = HybridWorldState::hybrid(initial_world);
    let mut candidate_scratch = Vec::with_capacity(config.world.initial_population as usize);

    for day in 0..40 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: day == 19 || day == 39,
        };
        let production_outcome =
            run_hybrid_authority_day(&mut production_path, &config, &context, &options).unwrap();
        let persistent_outcome = run_hybrid_authority_day_with_candidate_scratch(
            &mut persistent_path,
            &config,
            &context,
            &options,
            &mut candidate_scratch,
        )
        .unwrap();

        assert_eq!(production_outcome, persistent_outcome);
        assert_eq!(
            production_path.canonical_state_hash().unwrap(),
            persistent_path.canonical_state_hash().unwrap()
        );
    }
}

#[test]
fn test_09_phase4_candidate_index_edge_case_parity() {
    let config = make_config();
    let make_agent = |id: u32, group_id: u16, alive: bool, health: f32, food: f32| AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive,
        birth_day: SimulationDay(0),
        health,
        food,
        wealth: 1000,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(group_id),
    };
    let agents = vec![
        make_agent(81, 2, true, 1.0, 1.0),
        make_agent(20, 0, true, 1.0, 4.0),
        make_agent(90, 3, true, 1.0, 0.0),
        make_agent(10, 0, true, 1.0, 1.0),
        make_agent(30, 0, true, 1.0, config.interaction.starvation_threshold),
        make_agent(50, 1, true, 1.0, 0.0),
        make_agent(40, 0, true, 1.0, 6.0),
        make_agent(80, 2, true, 1.0, 5.0),
        make_agent(16, 0, true, 0.0, 1.0),
        make_agent(15, 0, false, 1.0, 1.0),
        make_agent(82, 2, false, 1.0, 2.0),
        make_agent(51, 1, false, 1.0, 2.0),
        make_agent(52, 1, true, 0.0, 2.0),
        make_agent(91, 3, false, 1.0, 5.0),
        make_agent(92, 3, true, 0.0, 3.0),
    ];
    let mut choices: Vec<_> = agents
        .iter()
        .filter(|agent| agent.alive && agent.health > 0.0)
        .map(|agent| PrimaryActionChoice {
            agent_id: agent.agent_id,
            action: match agent.agent_id {
                AgentId(20) | AgentId(40) | AgentId(50) => Action::GiveFood,
                AgentId(30) | AgentId(80) | AgentId(90) => Action::StealFood,
                _ => Action::Idle,
            },
        })
        .collect();
    choices.sort_by_key(|choice| choice.agent_id);

    // This storage order differs from AgentId order; the index must restore semantic order.
    let storage = SegmentedAgentStorage::from_agents(&agents);
    let mut full_scan_scratch = Vec::new();
    let mut index_scratch = Phase4CandidateIndexScratch::with_capacity(4);
    let mut full_scan = Vec::new();
    let mut indexed = Vec::new();
    generate_intents_storage_with_scratch(
        &storage,
        SimulationDay(0),
        &config,
        &choices,
        &mut full_scan_scratch,
        &mut full_scan,
    )
    .unwrap();
    generate_intents_storage_with_candidate_index(
        &storage,
        SimulationDay(0),
        &config,
        &choices,
        &mut index_scratch,
        &mut indexed,
    )
    .unwrap();
    assert_eq!(full_scan, indexed);
    assert!(
        indexed
            .windows(2)
            .all(|pair| pair[0].agent_id() < pair[1].agent_id())
    );

    let target = |agent_id| {
        indexed
            .iter()
            .find(|intent| intent.agent_id() == AgentId(agent_id))
            .and_then(|intent| match intent {
                Intent::GiveFood {
                    target_agent_id, ..
                }
                | Intent::StealFood {
                    target_agent_id, ..
                } => *target_agent_id,
                _ => None,
            })
    };
    assert_eq!(target(20), Some(AgentId(10))); // self was the other GiveFood candidate
    assert_eq!(target(80), Some(AgentId(81))); // self excluded from a two-agent steal bucket
    assert_eq!(target(50), None); // only self qualifies for GiveFood
    assert_eq!(target(90), None); // no living, healthy, positive-food steal target
    assert!(matches!(
        indexed
            .iter()
            .find(|intent| intent.agent_id() == AgentId(40)),
        Some(Intent::GiveFood {
            target_agent_id: Some(AgentId(10) | AgentId(20)),
            ..
        })
    ));
}

#[test]
fn test_10_phase4_candidate_index_randomized_storage_parity() {
    let mut full_scan_scratch = Vec::new();
    let mut index_scratch = Phase4CandidateIndexScratch::with_capacity(13);
    let mut full_scan = Vec::new();
    let mut indexed = Vec::new();

    for &(population, group_count) in &[
        (100, 1),
        (100, 2),
        (250, 2),
        (500, 3),
        (1000, 2),
        (1000, 5),
        (1000, 13),
    ] {
        for seed in [7_u64, 23, 91] {
            let mut config = make_config();
            config.world.initial_population = population;
            config.world.settlement_count = group_count;
            config.world.master_seed = seed;
            config.world.replicate_id = seed as u32;
            config.interaction.starvation_threshold = 5.0;
            let mut world = initialize_world(&config).unwrap();
            for (slot, agent) in world.agents.iter_mut().enumerate() {
                let pattern = (slot as u64 * 37 + seed) % 11;
                agent.food = match pattern {
                    0 => 0.0,
                    1 => 5.0,
                    2 => 4.999,
                    3 => 6.0,
                    _ => pattern as f32,
                };
                if pattern == 9 {
                    agent.alive = false;
                } else if pattern == 10 {
                    agent.health = 0.0;
                }
            }

            let mut choices: Vec<_> = world
                .agents
                .iter()
                .filter(|agent| agent.alive && agent.health > 0.0)
                .map(|agent| {
                    let action_index = (agent.agent_id.as_u32() * 7 + seed as u32) % 6;
                    PrimaryActionChoice {
                        agent_id: agent.agent_id,
                        action: match action_index {
                            0 => Action::Work,
                            1 => Action::BuyFood,
                            2 => Action::SellFood,
                            3 => Action::GiveFood,
                            4 => Action::StealFood,
                            _ => Action::Idle,
                        },
                    }
                })
                .collect();
            choices.sort_by_key(|choice| choice.agent_id);

            for shuffled in [false, true] {
                let mut agents = world.agents.clone();
                if shuffled {
                    agents.reverse();
                }
                let storage = SegmentedAgentStorage::from_agents(&agents);
                generate_intents_storage_with_scratch(
                    &storage,
                    world.current_day,
                    &config,
                    &choices,
                    &mut full_scan_scratch,
                    &mut full_scan,
                )
                .unwrap();
                generate_intents_storage_with_candidate_index(
                    &storage,
                    world.current_day,
                    &config,
                    &choices,
                    &mut index_scratch,
                    &mut indexed,
                )
                .unwrap();
                assert_eq!(
                    full_scan, indexed,
                    "N={population}, groups={group_count}, seed={seed}, shuffled={shuffled}"
                );
            }
        }
    }
}

fn phase8_random_input(
    population: u64,
    settlement_count: u32,
    reverse_storage: bool,
    seed: u64,
) -> (SegmentedAgentStorage, Vec<SettlementState>) {
    let mut config = make_config();
    config.world.initial_population = population;
    config.world.settlement_count = settlement_count;
    config.environment.carrying_capacity = 1000.0 * population as f32;
    let mut world = initialize_world(&config).expect("Phase8 test world initializes");
    let mut random = seed;

    for (index, agent) in world.agents.iter_mut().enumerate() {
        random = random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let value = (random >> 32) as u32;
        agent.group_id = GroupId((value % settlement_count) as u16);
        agent.alive = !value.is_multiple_of(11);
        agent.health = match value % 17 {
            0 => 0.0,
            1 => 0.25,
            _ => 1.0,
        };
        agent.food = match value % 5 {
            0 => 0.0,
            1 => 4.999,
            2 => 5.0,
            _ => 8.0,
        };
        agent.wealth = (index % 997) as i64;
    }
    for (index, settlement) in world.settlements.iter_mut().enumerate() {
        settlement.group_id = GroupId(index as u16);
        settlement.treasury = match index % 3 {
            0 => 0,
            1 => 19,
            _ => 211,
        };
    }

    if reverse_storage {
        world.agents.reverse();
    }
    (
        SegmentedAgentStorage::from_agents(&world.agents),
        world.settlements,
    )
}

fn assert_phase8_full_scan_matches_one_pass(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    payment: i64,
    scratch: &mut Phase8WelfareScratch,
) -> Vec<sim_model::SettlementWelfareResolution> {
    let mut full_scan_storage = storage.clone();
    let mut full_scan_settlements = settlements.to_vec();
    let full_scan = phase8_welfare_distribution_storage_full_scan(
        &mut full_scan_storage,
        &mut full_scan_settlements,
        5.0,
        payment,
    )
    .expect("reference Phase8 succeeds");

    let mut one_pass_storage = storage.clone();
    let mut one_pass_settlements = settlements.to_vec();
    let one_pass = phase8_welfare_distribution_storage_with_scratch(
        &mut one_pass_storage,
        &mut one_pass_settlements,
        5.0,
        payment,
        scratch,
    )
    .expect("one-pass Phase8 succeeds");

    assert_eq!(full_scan, one_pass);
    assert_eq!(full_scan_storage, one_pass_storage);
    assert_eq!(full_scan_settlements, one_pass_settlements);
    assert!(
        one_pass
            .windows(2)
            .all(|pair| pair[0].group_id < pair[1].group_id)
    );
    assert!(one_pass.iter().all(|settlement| {
        settlement
            .recipients
            .windows(2)
            .all(|pair| pair[0].agent_id < pair[1].agent_id)
    }));
    assert_eq!(
        scratch.last_slot_inspections(),
        if settlements.is_empty() {
            0
        } else {
            storage.len()
        }
    );
    one_pass
}

#[test]
fn phase8_one_pass_matches_full_scan_boundary_cases() {
    let mut scratch = Phase8WelfareScratch::with_capacity(3);
    let (storage, mut settlements) = phase8_random_input(5, 3, true, 17);

    // No settlements means no eligibility buckets are needed.
    settlements.clear();
    assert!(
        assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch)
            .is_empty()
    );

    let (mut storage, mut settlements) = phase8_random_input(5, 3, true, 18);
    storage.group_ids_mut().fill(GroupId(0));
    storage.alive_mut().fill(true);
    storage.health_mut().fill(1.0);
    storage.food_mut().fill(5.0);
    storage.wealth_mut().fill(0);
    settlements.truncate(1);
    settlements[0].group_id = GroupId(0);
    settlements[0].treasury = 100;

    // The threshold is exclusive, so food equal to it is ineligible.
    assert_eq!(
        assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch)[0]
            .eligible_count,
        0
    );

    // Dead agents and health <= 0 remain ineligible when food is below threshold.
    storage.food_mut().fill(4.999);
    storage.alive_mut()[0] = false;
    storage.health_mut()[1] = 0.0;
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch);
    assert_eq!(result[0].eligible_count, 3);

    // A single eligible recipient receives a fully funded payout.
    storage.alive_mut().fill(false);
    storage.alive_mut()[2] = true;
    storage.health_mut().fill(1.0);
    settlements[0].treasury = 10;
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch);
    assert_eq!(result[0].eligible_count, 1);
    assert_eq!(result[0].total_distributed, 10);
    assert_eq!(result[0].treasury_after, 0);

    // Multiple recipients with zero treasury receive zero; exact funding divides evenly.
    storage.alive_mut().fill(true);
    settlements[0].treasury = 0;
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch);
    assert_eq!(result[0].total_distributed, 0);
    assert_eq!(result[0].treasury_after, 0);
    settlements[0].treasury = 50;
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch);
    assert_eq!(result[0].total_distributed, 50);
    assert_eq!(result[0].remainder, 0);

    // Underfunded remainder goes to the first AgentIds, even with reversed storage slots.
    settlements[0].treasury = 48;
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 10, &mut scratch);
    assert_eq!(result[0].payment_per_agent, 9);
    assert_eq!(result[0].remainder, 3);
    assert_eq!(
        result[0]
            .recipients
            .iter()
            .map(|recipient| recipient.payout)
            .collect::<Vec<_>>(),
        vec![10, 10, 10, 9, 9]
    );
}

#[test]
fn phase8_one_pass_randomized_parity_for_population_and_storage_layouts() {
    let mut scratch = Phase8WelfareScratch::with_capacity(50);
    for population in [100, 250, 1000, 5000] {
        for settlement_count in [1, 2, 5, 20, 50] {
            for reverse_storage in [false, true] {
                let (storage, settlements) = phase8_random_input(
                    population,
                    settlement_count,
                    reverse_storage,
                    0x51a7_0000 + population + settlement_count as u64,
                );
                assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 17, &mut scratch);
            }
        }
    }
}

#[test]
fn phase8_one_pass_validation_error_keeps_entire_state_unchanged() {
    let (mut input_storage, mut input_settlements) = phase8_random_input(2, 2, true, 29);
    input_storage.group_ids_mut()[0] = GroupId(0);
    input_storage.group_ids_mut()[1] = GroupId(1);
    input_storage.alive_mut().fill(true);
    input_storage.health_mut().fill(1.0);
    input_storage.food_mut().fill(0.0);
    input_storage.wealth_mut()[0] = 0;
    input_storage.wealth_mut()[1] = i64::MAX;
    for settlement in &mut input_settlements {
        settlement.treasury = 10;
    }

    let mut full_scan_storage = input_storage.clone();
    let mut full_scan_settlements = input_settlements.clone();
    let full_scan_error = phase8_welfare_distribution_storage_full_scan(
        &mut full_scan_storage,
        &mut full_scan_settlements,
        5.0,
        10,
    )
    .unwrap_err();

    let mut one_pass_storage = input_storage.clone();
    let mut one_pass_settlements = input_settlements.clone();
    let mut scratch = Phase8WelfareScratch::with_capacity(2);
    let one_pass_error = phase8_welfare_distribution_storage_with_scratch(
        &mut one_pass_storage,
        &mut one_pass_settlements,
        5.0,
        10,
        &mut scratch,
    )
    .unwrap_err();

    assert_eq!(full_scan_error, one_pass_error);
    assert_eq!(full_scan_storage, input_storage);
    assert_eq!(one_pass_storage, input_storage);
    assert_eq!(full_scan_settlements, input_settlements);
    assert_eq!(one_pass_settlements, input_settlements);
}

#[test]
fn phase8_one_pass_handles_unbalanced_recipient_groups() {
    let (mut storage, settlements) = phase8_random_input(100, 2, true, 31);
    storage.group_ids_mut().fill(GroupId(0));
    storage.group_ids_mut()[0] = GroupId(1);
    storage.alive_mut().fill(true);
    storage.health_mut().fill(1.0);
    storage.food_mut().fill(0.0);
    storage.wealth_mut().fill(0);

    let mut scratch = Phase8WelfareScratch::with_capacity(2);
    let result = assert_phase8_full_scan_matches_one_pass(&storage, &settlements, 3, &mut scratch);
    assert_eq!(result[0].eligible_count, 99);
    assert_eq!(result[1].eligible_count, 1);
}

#[test]
fn phase8_one_pass_preserves_three_day_runner_outputs_and_state() {
    let config = make_config();
    let context = make_context();
    let mut initial = initialize_world(&config).expect("world initializes");
    for agent in &mut initial.agents {
        agent.food = 0.0;
    }

    let mut full_scan = HybridWorldState::hybrid(initial.clone());
    let mut one_pass = HybridWorldState::hybrid(initial);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let full_scan_outcomes = run_hybrid_authority_days_with_phase8_full_scan(
        &mut full_scan,
        &config,
        &context,
        3,
        &options,
    )
    .expect("full-scan runner succeeds");
    let one_pass_outcomes =
        run_hybrid_authority_days(&mut one_pass, &config, &context, 3, &options)
            .expect("one-pass runner succeeds");

    assert_eq!(full_scan_outcomes, one_pass_outcomes);
    assert_eq!(full_scan.world, one_pass.world);
    assert_eq!(full_scan.segmented_storage, one_pass.segmented_storage);
    assert_eq!(
        full_scan.canonical_state_hash().unwrap(),
        one_pass.canonical_state_hash().unwrap()
    );
}

fn phase3_random_input(
    population: u64,
    settlement_count: u32,
    reverse_storage: bool,
    seed: u64,
) -> (SegmentedAgentStorage, Vec<SettlementState>, SimConfig) {
    let mut config = make_config();
    config.world.initial_population = population;
    config.world.settlement_count = settlement_count;
    config.environment.carrying_capacity = 1000.0 * population as f32;
    let mut world = initialize_world(&config).expect("Phase3 test world initializes");
    let mut random = seed;

    for (index, agent) in world.agents.iter_mut().enumerate() {
        random = random
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let value = (random >> 32) as u32;
        agent.group_id = GroupId((value % settlement_count) as u16);
        agent.alive = !value.is_multiple_of(11);
        agent.health = match value % 17 {
            0 => 0.0,
            1 => 0.25,
            _ => 1.0,
        };
        agent.food = match value % 5 {
            0 => 0.0,
            1 => config.interaction.starvation_threshold,
            2 => 4.999,
            _ => 8.0,
        };
        agent.wealth = (index % 997) as i64;
    }
    for (index, settlement) in world.settlements.iter_mut().enumerate() {
        settlement.resource = index as f32 * 250.0;
    }
    if reverse_storage {
        world.agents.reverse();
    }
    (
        SegmentedAgentStorage::from_agents(&world.agents),
        world.settlements,
        config,
    )
}

fn assert_phase3_feature_bits_equal(
    left: &[sim_model::AgentFeatures],
    right: &[sim_model::AgentFeatures],
) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.agent_id, right.agent_id);
        for (left_value, right_value) in left.features.values.iter().zip(right.features.values) {
            assert_eq!(left_value.to_bits(), right_value.to_bits());
        }
    }
}

fn assert_phase3_linear_matches_direct(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    scratch: &mut Phase3ScarcityScratch,
) -> Result<usize, sim_model::Phase3Error> {
    let mut linear_output = Vec::new();
    let linear_result = phase3_observation_and_features_storage_into(
        storage,
        settlements,
        config,
        &mut linear_output,
    );

    let mut direct_output = Vec::new();
    let direct_result = phase3_observation_and_features_storage_with_scratch(
        storage,
        settlements,
        config,
        &mut direct_output,
        scratch,
    );

    assert_eq!(linear_result, direct_result);
    assert_phase3_feature_bits_equal(&linear_output, &direct_output);
    assert!(
        direct_output
            .windows(2)
            .all(|pair| pair[0].agent_id < pair[1].agent_id)
    );
    if direct_result.is_ok() {
        assert_eq!(scratch.last_index_build_entries(), settlements.len());
    }
    direct_result.map(|()| direct_output.len())
}

#[test]
fn phase3_direct_lookup_matches_linear_edge_cases() {
    let mut scratch = Phase3ScarcityScratch::with_capacity(5);
    let config = make_config();
    let empty = SegmentedAgentStorage::new();
    assert_eq!(
        assert_phase3_linear_matches_direct(&empty, &[], &config, &mut scratch),
        Ok(0)
    );

    // One settlement, every agent in it, boundary-valued food/wealth, and zero scarcity.
    let (mut one_group, mut settlements, config) = phase3_random_input(5, 1, true, 0x3301);
    one_group.alive_mut().fill(true);
    one_group.health_mut().fill(1.0);
    one_group.group_ids_mut().fill(GroupId(0));
    one_group.food_mut()[0] = config.interaction.starvation_threshold;
    one_group.wealth_mut()[0] = config.economy.target_reserve;
    settlements[0].resource = config.environment.carrying_capacity;
    assert_eq!(
        assert_phase3_linear_matches_direct(&one_group, &settlements, &config, &mut scratch),
        Ok(5)
    );
    let mut boundary_output = Vec::new();
    phase3_observation_and_features_storage_with_scratch(
        &one_group,
        &settlements,
        &config,
        &mut boundary_output,
        &mut scratch,
    )
    .unwrap();
    assert!(
        boundary_output
            .iter()
            .all(|features| features.features.local_scarcity().to_bits() == 0.0f32.to_bits())
    );

    // No agents with settlements, an empty settlement bucket, and one agent per settlement.
    let (no_agents, settlements, config) = phase3_random_input(0, 3, false, 0x3304);
    assert_eq!(
        assert_phase3_linear_matches_direct(&no_agents, &settlements, &config, &mut scratch),
        Ok(0)
    );
    let (mut empty_bucket, settlements, config) = phase3_random_input(10, 3, true, 0x3305);
    empty_bucket.group_ids_mut().fill(settlements[0].group_id);
    empty_bucket.alive_mut().fill(true);
    empty_bucket.health_mut().fill(1.0);
    assert_eq!(
        assert_phase3_linear_matches_direct(&empty_bucket, &settlements, &config, &mut scratch),
        Ok(10)
    );
    let (mut own_settlement, settlements, config) = phase3_random_input(5, 5, true, 0x3306);
    own_settlement.alive_mut().fill(true);
    own_settlement.health_mut().fill(1.0);
    for (slot, group_id) in own_settlement.group_ids_mut().iter_mut().enumerate() {
        *group_id = GroupId(slot as u16);
    }
    assert_eq!(
        assert_phase3_linear_matches_direct(&own_settlement, &settlements, &config, &mut scratch,),
        Ok(5)
    );

    // Multiple sparse GroupIds, non-GroupId settlement order, duplicate first-match behavior,
    // and an empty settlement bucket.
    let (mut sparse, mut settlements, config) = phase3_random_input(250, 3, true, 0x3302);
    let remap = [GroupId(60_000), GroupId(17), GroupId(4_096)];
    for group_id in sparse.group_ids_mut() {
        *group_id = remap[group_id.0 as usize];
    }
    for settlement in &mut settlements {
        settlement.group_id = remap[settlement.group_id.0 as usize];
    }
    settlements.reverse();
    let duplicate = SettlementState {
        group_id: settlements[0].group_id,
        resource: settlements[0].resource + 123.0,
        treasury: settlements[0].treasury,
    };
    settlements.push(duplicate);
    assert!(
        assert_phase3_linear_matches_direct(&sparse, &settlements, &config, &mut scratch).is_ok()
    );

    let (mut reordered, mut settlements, config) = phase3_random_input(250, 12, true, 0x3307);
    for group_id in reordered.group_ids_mut() {
        *group_id = GroupId(group_id.0 + 100);
    }
    for settlement in &mut settlements {
        settlement.group_id.0 += 100;
    }
    settlements.reverse();
    settlements.push(SettlementState {
        group_id: settlements[0].group_id,
        resource: settlements[0].resource + 123.0,
        treasury: settlements[0].treasury,
    });
    assert!(
        assert_phase3_linear_matches_direct(&reordered, &settlements, &config, &mut scratch)
            .is_ok()
    );
    assert_eq!(scratch.last_index_build_entries(), settlements.len());

    // Missing GroupId keeps the existing error and partial-output behavior.
    let (mut missing, settlements, config) = phase3_random_input(2, 1, false, 0x3303);
    missing.alive_mut().fill(true);
    missing.health_mut().fill(1.0);
    missing.group_ids_mut()[1] = GroupId(u16::MAX);
    assert_eq!(
        assert_phase3_linear_matches_direct(&missing, &settlements, &config, &mut scratch),
        Err(sim_model::Phase3Error::MissingSettlement(GroupId(u16::MAX)))
    );
}

#[test]
fn phase3_direct_lookup_randomized_population_and_layout_parity() {
    let mut scratch = Phase3ScarcityScratch::with_capacity(50);
    for population in [100, 250, 1000, 5000] {
        for settlement_count in [1, 2, 5, 20, 50] {
            for reverse_storage in [false, true] {
                let (storage, settlements, config) = phase3_random_input(
                    population,
                    settlement_count,
                    reverse_storage,
                    0x3300_0000 + population + settlement_count as u64,
                );
                assert!(
                    assert_phase3_linear_matches_direct(
                        &storage,
                        &settlements,
                        &config,
                        &mut scratch,
                    )
                    .is_ok()
                );
            }
        }
    }
}

#[test]
fn phase3_direct_lookup_preserves_three_day_runner_results() {
    let mut config = make_config();
    config.world.initial_population = 250;
    config.world.settlement_count = 5;
    config.environment.carrying_capacity = 250_000.0;
    let context = make_context();
    let initial = initialize_world(&config).expect("world initializes");
    let mut linear = HybridWorldState::hybrid(initial.clone());
    let mut indexed = HybridWorldState::hybrid(initial);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let linear_outcomes = run_hybrid_authority_days_with_phase3_linear_scan(
        &mut linear,
        &config,
        &context,
        3,
        &options,
    )
    .expect("linear Phase3 runner succeeds");
    let indexed_outcomes = run_hybrid_authority_days(&mut indexed, &config, &context, 3, &options)
        .expect("indexed Phase3 runner succeeds");

    assert_eq!(linear_outcomes, indexed_outcomes);
    assert_eq!(linear.world, indexed.world);
    assert_eq!(linear.segmented_storage, indexed.segmented_storage);
    assert_eq!(
        linear.canonical_state_hash().unwrap(),
        indexed.canonical_state_hash().unwrap()
    );
}

#[test]
fn phase5_production_phase4_stream_uses_owned_ordered_bucketing() {
    let mut config = make_config();
    config.world.initial_population = 1000;
    config.world.settlement_count = 5;
    config.environment.carrying_capacity = 1_000_000.0;
    let world = initialize_world(&config).expect("world initializes");
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    storage.phase2_degradation_with_config(&config);

    let mut features = Vec::with_capacity(storage.len());
    let mut phase3_scratch =
        Phase3ScarcityScratch::with_capacity(config.world.settlement_count as usize);
    phase3_observation_and_features_storage_with_scratch(
        &storage,
        &world.settlements,
        &config,
        &mut features,
        &mut phase3_scratch,
    )
    .expect("Phase3 succeeds");

    let mut choices = Vec::with_capacity(storage.len());
    phase4_primary_action_selection_storage_into(
        &storage,
        world.current_day,
        &config,
        &features,
        &mut choices,
    )
    .expect("production Phase4 selection succeeds");
    let mut phase4_scratch = Phase4CandidateIndexScratch::with_capacity(5);
    let mut intents = Vec::with_capacity(storage.len());
    generate_intents_storage_with_candidate_index(
        &storage,
        world.current_day,
        &config,
        &choices,
        &mut phase4_scratch,
        &mut intents,
    )
    .expect("production Phase4 intent generation succeeds");

    assert!(
        intents
            .windows(2)
            .all(|pair| pair[0].agent_id() < pair[1].agent_id())
    );
    let baseline = phase5_partition_intents_baseline(&intents).unwrap();
    let original_capacity = intents.capacity();
    let partitions = phase5_partition_intents_from_vec(&mut intents).unwrap();
    assert_eq!(partitions, baseline);
    assert!(intents.is_empty());
    assert_eq!(intents.capacity(), original_capacity);
}

#[test]
fn phase5_owned_production_runner_matches_canonical_runner() {
    let mut config = make_config();
    config.world.initial_population = 250;
    config.world.settlement_count = 5;
    config.environment.carrying_capacity = 250_000.0;
    let context = make_context();
    let initial = initialize_world(&config).expect("world initializes");
    let mut optimized = HybridWorldState::hybrid(initial.clone());
    let mut canonical = HybridWorldState::hybrid(initial);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };

    let optimized_outcomes =
        run_hybrid_authority_days(&mut optimized, &config, &context, 3, &options)
            .expect("ordered Phase5 runner succeeds");
    let canonical_outcomes = run_hybrid_authority_days_with_phase5_baseline(
        &mut canonical,
        &config,
        &context,
        3,
        &options,
    )
    .expect("canonical Phase5 runner succeeds");

    assert_eq!(optimized_outcomes, canonical_outcomes);
    assert_eq!(optimized.world, canonical.world);
    assert_eq!(optimized.segmented_storage, canonical.segmented_storage);
    assert_eq!(
        optimized.canonical_state_hash().unwrap(),
        canonical.canonical_state_hash().unwrap()
    );
}

#[test]
fn phase6b_scratch_production_runner_matches_existing_storage_resolver() {
    let mut config = make_config();
    config.world.initial_population = 500;
    config.world.settlement_count = 5;
    config.environment.carrying_capacity = 500_000.0;
    let context = make_context();
    let initial = initialize_world(&config).expect("world initializes");
    let mut optimized = HybridWorldState::hybrid(initial.clone());
    let mut baseline = HybridWorldState::hybrid(initial);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };

    let optimized_outcomes =
        run_hybrid_authority_days(&mut optimized, &config, &context, 3, &options)
            .expect("Phase6B scratch path succeeds");
    let baseline_outcomes = run_hybrid_authority_days_with_phase6b_baseline(
        &mut baseline,
        &config,
        &context,
        3,
        &options,
    )
    .expect("existing Phase6B path succeeds");

    assert_eq!(optimized_outcomes, baseline_outcomes);
    assert_eq!(optimized.world, baseline.world);
    assert_eq!(optimized.segmented_storage, baseline.segmented_storage);
    assert_eq!(
        optimized.canonical_state_hash().unwrap(),
        baseline.canonical_state_hash().unwrap()
    );
}
