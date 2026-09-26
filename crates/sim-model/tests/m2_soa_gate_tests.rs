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
    run_hybrid_scope_isolated_day, run_hybrid_scope_isolated_days, run_m0_day, run_m0_days,
    run_native_soa_day, run_native_soa_days,
};
use sim_model::snapshot::{decode_snapshot, restore_snapshot};
use sim_model::state::HybridWorldState;
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    Action, AgentState, Intent, Phase4CandidateIndexScratch, PrimaryActionChoice, SimConfig,
    generate_intents_storage_into_baseline, generate_intents_storage_into_variant_c,
    generate_intents_storage_into_variant_d, generate_intents_storage_with_candidate_index,
    generate_intents_storage_with_scratch, initialize_world, phase3_observation_and_features,
    phase4_generate_intents_into, phase4_generate_intents_storage_into,
    phase4_primary_action_selection_into, phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_baseline,
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
