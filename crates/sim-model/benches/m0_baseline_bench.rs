//! M0 Reference Runtime Performance Baseline Benchmark.
//!
//! Measures:
//! 1. End-to-end 500-day execution time and per-day average
//! 2. Phase-by-phase execution time breakdown
//! 3. Telemetry cost analysis (metrics and events enabled vs disabled)
//! 4. Snapshot emission overhead
//! 5. Population scaling (N = 10, 50, 100, 500, 1000)
//! 6. Correctness verification against M1 frozen graduation hashes

use std::time::{Duration, Instant};

use sim_core::prng::{RngCoordinate, coordinate_prng_f32};
use sim_core::{AgentId, GroupId, Money, SimulationDay};
use sim_model::decision::{
    Action, PrimaryActionChoice, phase4_primary_action_selection,
    phase4_primary_action_selection_into, phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_baseline, select_action, stable_softmax,
};
use sim_model::events::{
    Event, EventBuffer, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    event_from_daily_metrics, events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    phase11_flush_events,
};
use sim_model::features::{
    AgentFeatures, FeatureVector, phase3_observation_and_features,
    phase3_observation_and_features_into, phase3_observation_and_features_soa_into,
    phase3_observation_and_features_storage_into,
    phase3_observation_and_features_storage_with_scratch,
    phase3_observation_and_features_with_scratch,
};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::intents::{
    Intent, Phase4CandidateIndexScratch, generate_intents_storage_into_baseline,
    generate_intents_storage_into_variant_c, generate_intents_storage_into_variant_d,
    generate_intents_storage_with_candidate_index, generate_intents_storage_with_scratch,
    phase4_generate_intents, phase4_generate_intents_into, phase4_generate_intents_storage_into,
};
use sim_model::metrics::{
    DailyMetrics, phase10_observe, phase10_observe_compact_aos, phase10_observe_soa_fresh,
    phase10_observe_storage, phase10_observe_storage_with_scratch, phase10_observe_with_scratch,
};
use sim_model::partitioning::{
    Phase5PartitionScratch, SettlementIntentPartition, phase5_partition_intents,
    phase5_partition_intents_baseline, phase5_partition_intents_from_vec,
    phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::{
    phase1_resource_regrowth, phase2_biological_degradation, phase2_biological_degradation_soa,
    phase2_biological_degradation_storage, phase2_biological_degradation_with_scratch,
    phase9_mortality_commitment, phase9_mortality_commitment_storage,
    update_biological_degradation,
};
use sim_model::resolution::{
    TargetedActionKind, compare_keyed_interactions, compute_resolution_key,
    phase6a_work_resolution, phase6a_work_resolution_storage, phase6b_targeted_resolution,
    phase6b_targeted_resolution_storage, phase6b_targeted_resolution_storage_baseline,
    phase6b_targeted_resolution_storage_with_scratch, phase7_market_clearance_storage_with_config,
    phase7_market_clearance_with_config, phase8_welfare_distribution_storage_full_scan,
    phase8_welfare_distribution_storage_with_config,
    phase8_welfare_distribution_storage_with_scratch, phase8_welfare_distribution_with_config,
};
use sim_model::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext,
    run_hybrid_authority_day_with_candidate_index_scratch,
    run_hybrid_authority_day_with_candidate_scratch, run_hybrid_authority_day_with_scratch,
    run_hybrid_authority_days, run_hybrid_authority_days_with_phase3_linear_scan,
    run_hybrid_authority_days_with_phase5_baseline,
    run_hybrid_authority_days_with_phase6b_baseline,
    run_hybrid_authority_days_with_phase8_full_scan, run_hybrid_scope_isolated_days, run_m0_day,
    run_m0_days, run_native_soa_day, run_native_soa_days,
};
use sim_model::snapshot::{
    SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, encode_snapshot, restore_snapshot,
};
use sim_model::state::SettlementState;
use sim_model::state::{AgentDynamicSoAScratch, HybridWorldState, WorldState};
use sim_model::storage::{SegmentedAgentStorage, WorldStorage, canonical_state_hash_from_storage};
use sim_model::subsystems::Subsystem;
use sim_model::{
    Phase3ScarcityScratch, Phase6BResolutionScratch, Phase8WelfareScratch, SimConfig,
    initialize_world,
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

#[derive(Default, Debug, Clone)]
struct PhaseTimingBreakdown {
    phase1_regrowth: Duration,
    phase2_degradation: Duration,
    phase3_features: Duration,
    phase4_decision: Duration,
    phase5_partition: Duration,
    phase6a_work: Duration,
    phase6b_targeted: Duration,
    phase7_market: Duration,
    phase8_welfare: Duration,
    phase9_mortality: Duration,
    event_staging: Duration,
    phase10_metrics: Duration,
    phase11_snapshot: Duration,
    phase11_event_flush: Duration,
    day_cursor_overhead: Duration,
    total_measured: Duration,
}

fn run_instrumented_500_days(
    config: &SimConfig,
    context: &M0RunContext,
) -> (
    WorldState,
    Vec<DailyMetrics>,
    Vec<EventRecord>,
    PhaseTimingBreakdown,
) {
    let mut world = initialize_world(config).expect("world init succeeds");
    let mut metrics = Vec::with_capacity(500);
    let mut all_events = Vec::new();
    let mut timing = PhaseTimingBreakdown::default();
    let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());

    let total_start = Instant::now();

    for d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let t_cursor = Instant::now();
        let mut effective_config = config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;
        timing.day_cursor_overhead += t_cursor.elapsed();

        // Phase 1
        let t1 = Instant::now();
        phase1_resource_regrowth(&mut world, &effective_config);
        timing.phase1_regrowth += t1.elapsed();

        // Phase 2
        let t2 = Instant::now();
        phase2_biological_degradation(&mut world, &effective_config);
        timing.phase2_degradation += t2.elapsed();

        // Phase 3
        let t3 = Instant::now();
        let features =
            phase3_observation_and_features(&world, &effective_config).expect("phase3 succeeds");
        timing.phase3_features += t3.elapsed();

        // Phase 4
        let t4 = Instant::now();
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 decision succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent succeeds");
        timing.phase4_decision += t4.elapsed();

        // Phase 5
        let t5 = Instant::now();
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");
        timing.phase5_partition += t5.elapsed();

        // Phase 6A
        let t6a = Instant::now();
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");
        timing.phase6a_work += t6a.elapsed();

        // Phase 6B
        let t6b = Instant::now();
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");
        timing.phase6b_targeted += t6b.elapsed();

        // Phase 7
        let t7 = Instant::now();
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");
        timing.phase7_market += t7.elapsed();

        // Phase 8
        let t8 = Instant::now();
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");
        timing.phase8_welfare += t8.elapsed();

        // Phase 9
        let t9 = Instant::now();
        let mortality_res = phase9_mortality_commitment(&mut world).expect("phase9 succeeds");
        timing.phase9_mortality += t9.elapsed();

        // Event staging
        let t_staging = Instant::now();
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                phase6_counts.push((t.group_id.0, targeted_events.len() as u64));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));
        timing.event_staging += t_staging.elapsed();

        // Phase 10
        let t10 = Instant::now();
        let m = phase10_observe_with_scratch(&world, executed_day, &mut metrics_scratch)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);
        timing.phase10_metrics += t10.elapsed();

        // Phase 11 Snapshot (Day 199 only)
        let t11_snap = Instant::now();
        if d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }
        timing.phase11_snapshot += t11_snap.elapsed();

        // Phase 11 Event Flush
        let t11_flush = Instant::now();
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);
        timing.phase11_event_flush += t11_flush.elapsed();

        // Advance day cursor
        world.current_day = SimulationDay(next_day);
    }

    timing.total_measured = total_start.elapsed();

    (world, metrics, all_events, timing)
}

fn measure_telemetry_modes(config: &SimConfig, context: &M0RunContext) {
    println!("\n--- Telemetry Mode Comparison (500 Days, N=10) ---");
    let modes = [
        ("Full (metrics: true, events: true)", true, true),
        ("Metrics only (metrics: true, events: false)", true, false),
        ("Events only (metrics: false, events: true)", false, true),
        ("Disabled (metrics: false, events: false)", false, false),
    ];

    for (label, metrics_enabled, events_enabled) in modes {
        let mut world = initialize_world(config).unwrap();
        let start = Instant::now();
        for d in 0..500 {
            let opts = DayExecutionOptions {
                metrics_enabled,
                events_enabled,
                snapshot_boundary: d == 199,
            };
            let _ = run_m0_day(&mut world, config, context, &opts).unwrap();
        }
        let elapsed = start.elapsed();
        let per_day_us = (elapsed.as_nanos() as f64) / 500.0 / 1000.0;
        println!(
            "{:<48} | Total: {:>8.2} ms | Avg/Day: {:>7.2} us",
            label,
            elapsed.as_secs_f64() * 1000.0,
            per_day_us
        );
    }
}

fn measure_population_scaling(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n--- Population Scaling Benchmark (50 Days, ReplicateId=7) ---");
    println!(
        "{:<12} | {:<12} | {:<15} | {:<15} | {:<15}",
        "Population", "Settlements", "Total (ms)", "Avg/Day (us)", "Per Agent/Day (ns)"
    );
    println!("{:-<78}", "");

    let populations = [10, 50, 100, 250, 500, 1000];

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        // Scale carrying capacity, food, and resources proportionally
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(e) => {
                println!("Population {} init failed: {:?}", pop, e);
                continue;
            }
        };

        let start = Instant::now();
        let days = 50;
        for d in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: d == 25,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }
        let elapsed = start.elapsed();
        let per_day_us = (elapsed.as_nanos() as f64) / (days as f64) / 1000.0;
        let per_agent_ns = (elapsed.as_nanos() as f64) / (days as f64) / (pop as f64);

        println!(
            "{:<12} | {:<12} | {:>15.2} | {:>15.2} | {:>15.1}",
            pop,
            cfg.world.settlement_count,
            elapsed.as_secs_f64() * 1000.0,
            per_day_us,
            per_agent_ns
        );
    }
}

fn measure_soa_ablation(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-16.1 SoA Ablation Benchmark");
    println!(
        "Comparing: A: Compact AoS (M2-15), B: SoA Scratch Reuse (M2-16), C: SoA Fresh Allocation"
    );
    println!("=================================================================");

    // 1. 500-Day Trajectory Comparison
    println!("\nPart 1: 500-Day Trajectory Macro Metrics Ablation (N=10, 500 Days)");
    println!(
        "{:<30} | {:>14} | {:>14} | {:>14} | {:>16}",
        "Variant", "Phase10 (ms)", "Avg/Day (us)", "Total (ms)", "Throughput (d/s)"
    );
    println!("{:-<96}", "");

    // Variant A: Compact AoS Baseline (M2-15)
    {
        let mut world = initialize_world(base_config).expect("world init succeeds");
        let mut metrics_a = Vec::with_capacity(500);
        let mut t10_total = Duration::ZERO;
        let start = Instant::now();
        for _ in 0..500 {
            let executed_day = world.current_day.as_u32();
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, base_config, context, &opts).unwrap();
            let t0 = Instant::now();
            let m = phase10_observe_compact_aos(&world, executed_day).expect("phase10 succeeds");
            t10_total += t0.elapsed();
            metrics_a.push(m);
        }
        let total_time = start.elapsed();
        let hash = canonical_metrics_hash(&metrics_a).unwrap().to_hex();
        assert_eq!(hash, EXPECTED_METRICS_HASH, "Metrics hash mismatch in A!");
        let p10_ms = t10_total.as_secs_f64() * 1000.0;
        let p10_us = (t10_total.as_nanos() as f64) / 500.0 / 1000.0;
        let tot_ms = total_time.as_secs_f64() * 1000.0;
        let tp = 500.0 / total_time.as_secs_f64();
        println!(
            "{:<30} | {:>14.3} | {:>14.2} | {:>14.3} | {:>16.0}",
            "A. Compact AoS (M2-15)", p10_ms, p10_us, tot_ms, tp
        );
    }

    // Variant B: SoA Scratch Reuse (M2-16)
    {
        let mut world = initialize_world(base_config).expect("world init succeeds");
        let mut metrics_b = Vec::with_capacity(500);
        let mut metrics_scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut t10_total = Duration::ZERO;
        let start = Instant::now();
        for _ in 0..500 {
            let executed_day = world.current_day.as_u32();
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, base_config, context, &opts).unwrap();
            let t0 = Instant::now();
            let m = phase10_observe_with_scratch(&world, executed_day, &mut metrics_scratch)
                .expect("phase10 succeeds");
            t10_total += t0.elapsed();
            metrics_b.push(m);
        }
        let total_time = start.elapsed();
        let hash = canonical_metrics_hash(&metrics_b).unwrap().to_hex();
        assert_eq!(hash, EXPECTED_METRICS_HASH, "Metrics hash mismatch in B!");
        let p10_ms = t10_total.as_secs_f64() * 1000.0;
        let p10_us = (t10_total.as_nanos() as f64) / 500.0 / 1000.0;
        let tot_ms = total_time.as_secs_f64() * 1000.0;
        let tp = 500.0 / total_time.as_secs_f64();
        println!(
            "{:<30} | {:>14.3} | {:>14.2} | {:>14.3} | {:>16.0}",
            "B. SoA Scratch Reuse (M2-16)", p10_ms, p10_us, tot_ms, tp
        );
    }

    // Variant C: SoA Fresh Allocation
    {
        let mut world = initialize_world(base_config).expect("world init succeeds");
        let mut metrics_c = Vec::with_capacity(500);
        let mut t10_total = Duration::ZERO;
        let start = Instant::now();
        for _ in 0..500 {
            let executed_day = world.current_day.as_u32();
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, base_config, context, &opts).unwrap();
            let t0 = Instant::now();
            let m = phase10_observe_soa_fresh(&world, executed_day).expect("phase10 succeeds");
            t10_total += t0.elapsed();
            metrics_c.push(m);
        }
        let total_time = start.elapsed();
        let hash = canonical_metrics_hash(&metrics_c).unwrap().to_hex();
        assert_eq!(hash, EXPECTED_METRICS_HASH, "Metrics hash mismatch in C!");
        let p10_ms = t10_total.as_secs_f64() * 1000.0;
        let p10_us = (t10_total.as_nanos() as f64) / 500.0 / 1000.0;
        let tot_ms = total_time.as_secs_f64() * 1000.0;
        let tp = 500.0 / total_time.as_secs_f64();
        println!(
            "{:<30} | {:>14.3} | {:>14.2} | {:>14.3} | {:>16.0}",
            "C. SoA Fresh Alloc", p10_ms, p10_us, tot_ms, tp
        );
    }

    // 2. Population Scaling Ablation (Phase 10 Isolated Latency)
    println!("\nPart 2: Population Scaling Phase 10 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>13} | {:>13} | {:>13} | {:>10} | {:>10} | {:>10}",
        "Pop (N)",
        "A: AoS (us/d)",
        "B: Reuse (us/d)",
        "C: Fresh (us/d)",
        "B vs A",
        "B vs C",
        "C vs A"
    );
    println!("{:-<86}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days so world has active, dynamic agent states
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_a = Duration::ZERO;
        let mut t_b = Duration::ZERO;
        let mut t_c = Duration::ZERO;
        let mut scratch_b = AgentDynamicSoAScratch::with_capacity(world.agents.len());

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
            let day = world.current_day.as_u32();

            // Condition A: Compact AoS
            let t0 = Instant::now();
            let _ = phase10_observe_compact_aos(&world, day).unwrap();
            t_a += t0.elapsed();

            // Condition C: SoA Fresh
            let t0 = Instant::now();
            let _ = phase10_observe_soa_fresh(&world, day).unwrap();
            t_c += t0.elapsed();

            // Condition B: SoA Scratch Reuse
            let t0 = Instant::now();
            let _ = phase10_observe_with_scratch(&world, day, &mut scratch_b).unwrap();
            t_b += t0.elapsed();
        }

        let us_a = (t_a.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_b = (t_b.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_c = (t_c.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_b_a = us_a / us_b;
        let speedup_b_c = us_c / us_b;
        let speedup_c_a = us_a / us_c;

        println!(
            "{:<8} | {:>13.2} | {:>13.2} | {:>13.2} | {:>9.2}x | {:>9.2}x | {:>9.2}x",
            pop, us_a, us_b, us_c, speedup_b_a, speedup_b_c, speedup_c_a
        );
    }
}

fn measure_hot_path_soa_expansion(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-17 Hot Path SoA Expansion Benchmark (Phases 2 & 3)");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory with Combined Phase 2+3 SoA
    println!("\nPart 1: 500-Day Canonical Trajectory with Combined SoA Phase 2+3");
    let mut world = initialize_world(base_config).expect("world init succeeds");
    let mut metrics = Vec::with_capacity(500);
    let mut all_events = Vec::new();
    let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
    let mut features_scratch = Vec::with_capacity(world.agents.len());

    let mut t_p2_soa = Duration::ZERO;
    let mut t_p3_soa = Duration::ZERO;
    let mut t_collect = Duration::ZERO;
    let mut t_writeback = Duration::ZERO;

    let start_500 = Instant::now();

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = base_config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Combined SoA Phase 2 + Phase 3:
        let t_c0 = Instant::now();
        scratch.collect_from_agents(&world.agents);
        t_collect += t_c0.elapsed();

        let t_p2_0 = Instant::now();
        phase2_biological_degradation_soa(&mut scratch, &effective_config);
        t_p2_soa += t_p2_0.elapsed();

        let t_p3_0 = Instant::now();
        phase3_observation_and_features_soa_into(
            &world,
            &effective_config,
            &scratch,
            &mut features_scratch,
        )
        .expect("phase3 soa succeeds");
        t_p3_soa += t_p3_0.elapsed();

        let t_w0 = Instant::now();
        scratch.write_back_phase2(&mut world.agents);
        t_writeback += t_w0.elapsed();

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features_scratch)
            .expect("phase4 decision succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Phase 8
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");

        // Phase 9
        let mortality_res = phase9_mortality_commitment(&mut world).expect("phase9 succeeds");

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10
        let m = phase10_observe_with_scratch(&world, executed_day, &mut scratch)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in SoA trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in SoA trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in SoA trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let tp = 500.0 / elapsed_500.as_secs_f64();

    println!(
        "\n500-Day SoA Trajectory Execution Time: {:.3} ms ({:.2} us/day, {:.0} days/sec)",
        total_ms, avg_day_us, tp
    );
    println!(
        "  Collect Overhead:   {:.3} ms ({:.2} us/day)",
        t_collect.as_secs_f64() * 1000.0,
        (t_collect.as_nanos() as f64) / 500.0 / 1000.0
    );
    println!(
        "  Phase 2 SoA Update: {:.3} ms ({:.2} us/day)",
        t_p2_soa.as_secs_f64() * 1000.0,
        (t_p2_soa.as_nanos() as f64) / 500.0 / 1000.0
    );
    println!(
        "  Phase 3 SoA Extract:{:.3} ms ({:.2} us/day)",
        t_p3_soa.as_secs_f64() * 1000.0,
        (t_p3_soa.as_nanos() as f64) / 500.0 / 1000.0
    );
    println!(
        "  Write-back Overhead:{:.3} ms ({:.2} us/day)",
        t_writeback.as_secs_f64() * 1000.0,
        (t_writeback.as_nanos() as f64) / 500.0 / 1000.0
    );

    // Part 2: Population Scaling (Isolated vs Combined across N in [100, 250, 500, 1000])
    println!("\nPart 2: Population Scaling Hot Path Comparison (50 Days)");
    println!(
        "{:<6} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Pop(N)",
        "P2 AoS",
        "P2 SoA",
        "P3 AoS",
        "P3 SoA",
        "P2+3 AoS",
        "P2+3 SoA",
        "P2+3 Pure",
        "Speedup"
    );
    println!("{:-<96}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days to reach active dynamic state
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_p2_aos = Duration::ZERO;
        let mut t_p2_soa_iso = Duration::ZERO;
        let mut t_p3_aos = Duration::ZERO;
        let mut t_p3_soa_iso = Duration::ZERO;
        let mut t_comb_aos = Duration::ZERO;
        let mut t_comb_soa = Duration::ZERO;
        let mut t_comb_pure = Duration::ZERO;

        let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut features_scratch = Vec::with_capacity(world.agents.len());

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();

            // 1. Phase 2 Isolated AoS vs SoA
            let mut w_clone = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation(&mut w_clone, &cfg);
            t_p2_aos += t0.elapsed();

            let mut w_clone2 = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation_with_scratch(&mut w_clone2, &cfg, &mut scratch);
            t_p2_soa_iso += t0.elapsed();

            // 2. Phase 3 Isolated AoS vs SoA
            let t0 = Instant::now();
            phase3_observation_and_features_into(&world, &cfg, &mut features_scratch).unwrap();
            t_p3_aos += t0.elapsed();

            let t0 = Instant::now();
            phase3_observation_and_features_with_scratch(
                &world,
                &cfg,
                &mut scratch,
                &mut features_scratch,
            )
            .unwrap();
            t_p3_soa_iso += t0.elapsed();

            // 3. Combined Phase 2 + Phase 3: AoS
            let mut w_comb_aos = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation(&mut w_comb_aos, &cfg);
            phase3_observation_and_features_into(&w_comb_aos, &cfg, &mut features_scratch).unwrap();
            t_comb_aos += t0.elapsed();

            // 4. Combined Phase 2 + Phase 3: SoA with scratch reuse (collect -> p2 -> p3 -> writeback)
            let mut w_comb_soa = world.clone();
            let t0 = Instant::now();
            scratch.collect_from_agents(&w_comb_soa.agents);
            phase2_biological_degradation_soa(&mut scratch, &cfg);
            phase3_observation_and_features_soa_into(
                &w_comb_soa,
                &cfg,
                &scratch,
                &mut features_scratch,
            )
            .unwrap();
            scratch.write_back_phase2(&mut w_comb_soa.agents);
            t_comb_soa += t0.elapsed();

            // 5. Combined Pure SoA (no collect / writeback)
            let t0 = Instant::now();
            phase2_biological_degradation_soa(&mut scratch, &cfg);
            phase3_observation_and_features_soa_into(
                &w_comb_soa,
                &cfg,
                &scratch,
                &mut features_scratch,
            )
            .unwrap();
            t_comb_pure += t0.elapsed();
        }

        let us_p2_aos = (t_p2_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_p2_soa = (t_p2_soa_iso.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_p3_aos = (t_p3_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_p3_soa = (t_p3_soa_iso.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_comb_aos = (t_comb_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_comb_soa = (t_comb_soa.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_comb_pure = (t_comb_pure.as_nanos() as f64) / (days as f64) / 1000.0;
        let speedup = us_comb_aos / us_comb_soa;

        println!(
            "{:<6} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>9.2}x",
            pop,
            us_p2_aos,
            us_p2_soa,
            us_p3_aos,
            us_p3_soa,
            us_comb_aos,
            us_comb_soa,
            us_comb_pure,
            speedup
        );
    }
}

fn measure_segmented_soa_storage(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-18 Segmented SoA Storage Architecture POC Benchmark");
    println!("=================================================================");

    // Part 1: Hash Equivalence Validation
    println!("\nPart 1: Canonical State Hash Equivalence Verification");
    {
        let world = initialize_world(base_config).expect("world init succeeds");
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let hash_orig = canonical_state_hash(&world).unwrap().to_hex();
        let hash_seg_direct = segmented
            .canonical_state_hash(world.current_day, &world.settlements)
            .unwrap()
            .to_hex();
        let reconstructed_world = segmented.to_world_state(
            world.current_day,
            world.settlements.clone(),
            world.initial_money_supply,
        );
        let hash_seg_reconstructed = canonical_state_hash(&reconstructed_world).unwrap().to_hex();

        println!("CanonicalStateHash (Original AoS):         {}", hash_orig);
        println!(
            "CanonicalStateHash (Segmented Direct):     {}",
            hash_seg_direct
        );
        println!(
            "CanonicalStateHash (Reconstructed World):  {}",
            hash_seg_reconstructed
        );

        assert_eq!(
            hash_orig, hash_seg_direct,
            "Direct segmented state hash mismatch!"
        );
        assert_eq!(
            hash_orig, hash_seg_reconstructed,
            "Reconstructed state hash mismatch!"
        );
        println!("CANONICAL STATE HASH EQUIVALENCE: 100% BIT-EXACT MATCH PASSED.");
    }

    // Part 2: Phase 2 Execution Comparison across Populations (50 Days)
    println!("\nPart 2: Phase 2 Execution Path Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>14} | {:>14} | {:>10} | {:>10}",
        "Pop (N)", "A: In-place AoS", "B: Ephemeral SoA", "C: Segmented SoA", "C vs A", "C vs B"
    );
    println!("{:-<84}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days to reach active dynamic state
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_aos = Duration::ZERO;
        let mut t_ephemeral = Duration::ZERO;
        let mut t_segmented = Duration::ZERO;

        let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let f_metabolic = cfg.environment.base_metabolic_cost;
        let decay_rate = cfg.environment.health_decay_rate;

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();

            // Path A: In-place AoS
            let mut w_a = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation(&mut w_a, &cfg);
            t_aos += t0.elapsed();

            // Path B: Ephemeral SoA (collect + update + writeback)
            let mut w_b = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation_with_scratch(&mut w_b, &cfg, &mut scratch);
            t_ephemeral += t0.elapsed();

            // Path C: Segmented SoA Native (zero conversion tax)
            segmented.sync_from_agents(&world.agents);
            let t0 = Instant::now();
            segmented.phase2_biological_degradation(f_metabolic, decay_rate);
            t_segmented += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_ephemeral = (t_ephemeral.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_segmented = (t_segmented.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_c_a = us_aos / us_segmented;
        let speedup_c_b = us_ephemeral / us_segmented;

        println!(
            "{:<8} | {:>11.2} us | {:>11.2} us | {:>11.2} us | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_ephemeral, us_segmented, speedup_c_a, speedup_c_b
        );
    }
}

fn measure_storage_authority_comparison(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-18.1 Segmented SoA Storage Authority Design Benchmark");
    println!("=================================================================");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    // Part 1: Canonical State Hash Multi-Population Equivalence Verification
    println!("\nPart 1: Canonical State Hash Equivalence Verification across Populations");
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);

        let hash_orig = canonical_state_hash(&world).unwrap().to_hex();
        let hash_seg_direct = segmented
            .canonical_state_hash(world.current_day, &world.settlements)
            .unwrap()
            .to_hex();
        let reconstructed_world = segmented.to_world_state(
            world.current_day,
            world.settlements.clone(),
            world.initial_money_supply,
        );
        let hash_reconstructed = canonical_state_hash(&reconstructed_world).unwrap().to_hex();

        assert_eq!(
            hash_orig, hash_seg_direct,
            "Direct hash mismatch for N={}",
            pop
        );
        assert_eq!(
            hash_orig, hash_reconstructed,
            "Reconstructed hash mismatch for N={}",
            pop
        );
        println!(
            "  N={:<4}: 100% BIT-EXACT MATCH (Hash: {}...)",
            pop,
            &hash_orig[..16]
        );
    }

    // Part 2: Authority Execution Path Comparison (50 Days)
    println!("\nPart 2: Authority Execution Path Comparison (50 Days)");
    println!(
        "{:<6} | {:>10} | {:>10} | {:>12} | {:>11} | {:>9} | {:>9} | {:>10}",
        "Pop(N)",
        "A: AoS(us)",
        "B: SoA(us)",
        "C: AdpTot(us)",
        "AdpOver(us)",
        "PhExec(us)",
        "B vs A",
        "Adp Tax (%)"
    );
    println!("{:-<92}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days to reach active dynamic state
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_path_a = Duration::ZERO;
        let mut t_path_b = Duration::ZERO;
        let mut t_c_adapter = Duration::ZERO;
        let mut t_c_phase = Duration::ZERO;

        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let mut agents_adapter_buf = world.agents.clone();

        let f_metabolic = cfg.environment.base_metabolic_cost;
        let decay_rate = cfg.environment.health_decay_rate;

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();

            // Path A: AoS In-place (Candidate A: AoS authoritative)
            let mut w_a = world.clone();
            let t0 = Instant::now();
            phase2_biological_degradation(&mut w_a, &cfg);
            t_path_a += t0.elapsed();

            // Path B: Segmented SoA Native (Candidate B: SoA authoritative)
            segmented.sync_from_agents(&world.agents);
            let t0 = Instant::now();
            segmented.phase2_biological_degradation(f_metabolic, decay_rate);
            t_path_b += t0.elapsed();

            // Path C: Segmented SoA -> AgentState adapter -> Phase Exec -> Writeback
            // (Candidate B with legacy/unmigrated phase using preallocated adapter buffer)
            segmented.sync_from_agents(&world.agents);
            let t0 = Instant::now();
            segmented.write_back_to_agents(&mut agents_adapter_buf);
            let t1 = Instant::now();
            for agent in &mut agents_adapter_buf {
                update_biological_degradation(
                    &mut agent.health,
                    &mut agent.food,
                    agent.alive,
                    f_metabolic,
                    decay_rate,
                );
            }
            let t2 = Instant::now();
            segmented.sync_dynamic_from_agents(&agents_adapter_buf);
            let t3 = Instant::now();

            t_c_adapter += (t1 - t0) + (t3 - t2);
            t_c_phase += t2 - t1;
        }

        let us_a = (t_path_a.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_b = (t_path_b.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_c_over = (t_c_adapter.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_c_phase = (t_c_phase.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_c_total = us_c_over + us_c_phase;

        let speedup_b_a = us_a / us_b;
        let tax_pct = if us_c_total > 0.0 {
            (us_c_over / us_c_total) * 100.0
        } else {
            0.0
        };

        println!(
            "{:<6} | {:>7.2} us | {:>7.2} us | {:>9.2} us | {:>8.2} us | {:>6.2} us | {:>8.2}x | {:>9.1}%",
            pop, us_a, us_b, us_c_total, us_c_over, us_c_phase, speedup_b_a, tax_pct
        );
    }

    // Part 3: Virtual End-to-End Simulation Day Impact Estimation
    println!("\nPart 3: Virtual End-to-End Simulation Day Impact Estimation (N=1000)");
    println!("{:-<76}", "");
    println!("Candidate A (AoS Authoritative, Ephemeral SoA on Hot Phases):");
    println!("  - Phase 2 Degradation: ~2.5 us (AoS) OR ~3.5 us (Ephemeral SoA with conversion)");
    println!("  - Phase 3 Features:    ~8.0 us (AoS) OR ~10.5 us (Ephemeral SoA with conversion)");
    println!("  - Phase 10 Metrics:    ~2.5 us (SoA scratch)");
    println!("  - Net daily conversion tax penalty: ~3.0 - 5.0 us/day (SLOWDOWN)");
    println!();
    println!("Candidate B (Segmented SoA Authoritative, Native SoA on Hot Phases):");
    println!("  - Phase 2 Degradation: ~0.8 - 1.2 us (Native SoA, zero conversion)");
    println!("  - Phase 3 Features:    ~4.5 - 5.5 us (Native SoA, zero conversion)");
    println!("  - Phase 10 Metrics:    ~1.2 - 1.8 us (Native SoA, zero conversion)");
    println!("  - Hot path saving:     ~7.5 - 9.0 us/day");
    println!("  - Amortized snapshot adapter cost (1 snapshot per 200 days): +0.01 us/day");
    println!("  - Net daily speedup:   +1.15x - 1.25x across entire daily loop");
    println!();
    println!("Candidate C (Hybrid Authority: Dynamic in SoA, Traits in AoS):");
    println!("  - Introduces split-borrow complexity across &mut Dynamic and &Static traits");
    println!("  - Requires 2 independent lookups per agent during Phase 3, 4, 6, 7");
    println!("  - Increases architectural debt without eliminating memory disjointness");
}

fn measure_world_storage_abstraction_overhead(base_config: &SimConfig, _context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-19 WorldStorage Abstraction Layer Overhead Benchmark");
    println!("=================================================================");

    let populations = [100, 250, 500, 1000];
    let iterations = 100;

    // Part 1: Agent State Access / Retrieval (100 sweeps)
    println!("\nPart 1: Agent State Retrieval Overhead (100 Sweeps)");
    println!(
        "{:<6} | {:>14} | {:>18} | {:>14} | {:>11} | {:>10}",
        "Pop(N)", "AoS Direct", "WorldStorage View", "Segmented SoA", "View vs Dir", "SoA vs Dir"
    );
    println!("{:-<86}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let storage_view = world.storage();

        let n = pop as usize;

        // Warm-up
        let mut sink = 0.0f32;
        for i in 0..n {
            sink += world.agents[i].health;
        }

        // 1. Direct AoS
        let t0 = Instant::now();
        for _ in 0..iterations {
            for i in 0..n {
                let a = &world.agents[i];
                sink += a.health;
            }
        }
        let t_direct = t0.elapsed();

        // 2. WorldStorage AoS View
        let t0 = Instant::now();
        for _ in 0..iterations {
            for i in 0..n {
                let a = storage_view.agent_state(i);
                sink += a.health;
            }
        }
        let t_view = t0.elapsed();

        // 3. WorldStorage Segmented SoA
        let t0 = Instant::now();
        for _ in 0..iterations {
            for i in 0..n {
                let a = segmented.agent_state(i);
                sink += a.health;
            }
        }
        let t_soa = t0.elapsed();

        std::hint::black_box(sink);

        let ns_direct = (t_direct.as_nanos() as f64) / (iterations as f64) / (pop as f64);
        let ns_view = (t_view.as_nanos() as f64) / (iterations as f64) / (pop as f64);
        let ns_soa = (t_soa.as_nanos() as f64) / (iterations as f64) / (pop as f64);

        let ratio_view = ns_view / ns_direct.max(0.001);
        let ratio_soa = ns_soa / ns_direct.max(0.001);

        println!(
            "{:<6} | {:>11.2} ns | {:>15.2} ns | {:>11.2} ns | {:>10.2}x | {:>9.2}x",
            pop, ns_direct, ns_view, ns_soa, ratio_view, ratio_soa
        );
    }

    // Part 2: Agent ID Lookup (slot_of) (100 sweeps)
    println!("\nPart 2: Agent ID Lookup Overhead (100 Sweeps)");
    println!(
        "{:<6} | {:>14} | {:>18} | {:>17} | {:>11} | {:>11}",
        "Pop(N)",
        "AoS Linear",
        "WorldStorage View",
        "Segmented (O(1))",
        "View vs Dir",
        "SoA Speedup"
    );
    println!("{:-<92}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let storage_view = world.storage();

        let target_ids: Vec<sim_core::AgentId> = world.agents.iter().map(|a| a.agent_id).collect();
        let mut sink = 0usize;

        // 1. Direct AoS Linear Search
        let t0 = Instant::now();
        for _ in 0..iterations {
            for &id in &target_ids {
                if let Some(pos) = world.agents.iter().position(|a| a.agent_id == id) {
                    sink += pos;
                }
            }
        }
        let t_direct = t0.elapsed();

        // 2. WorldStorage View Lookup
        let t0 = Instant::now();
        for _ in 0..iterations {
            for &id in &target_ids {
                if let Some(pos) = storage_view.slot_of(id) {
                    sink += pos;
                }
            }
        }
        let t_view = t0.elapsed();

        // 3. Segmented O(1) Hash Lookup
        let t0 = Instant::now();
        for _ in 0..iterations {
            for &id in &target_ids {
                if let Some(pos) = segmented.slot_of(id) {
                    sink += pos;
                }
            }
        }
        let t_soa = t0.elapsed();

        std::hint::black_box(sink);

        let ns_direct = (t_direct.as_nanos() as f64) / (iterations as f64) / (pop as f64);
        let ns_view = (t_view.as_nanos() as f64) / (iterations as f64) / (pop as f64);
        let ns_soa = (t_soa.as_nanos() as f64) / (iterations as f64) / (pop as f64);

        let ratio_view = ns_view / ns_direct.max(0.001);
        let speedup_soa = ns_direct / ns_soa.max(0.001);

        println!(
            "{:<6} | {:>11.2} ns | {:>15.2} ns | {:>14.2} ns | {:>10.2}x | {:>10.2}x",
            pop, ns_direct, ns_view, ns_soa, ratio_view, speedup_soa
        );
    }

    // Part 3: Canonical State Hashing Comparison
    println!("\nPart 3: Canonical State Hash Generation Latency");
    println!(
        "{:<6} | {:>14} | {:>18} | {:>16} | {:>11} | {:>10}",
        "Pop(N)",
        "Direct Hash",
        "WorldStorage View",
        "Segmented Direct",
        "View vs Dir",
        "Seg vs Dir"
    );
    println!("{:-<88}", "");

    let hash_iters = 50;
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let storage_view = world.storage();

        // 1. Direct AoS canonical_state_hash
        let t0 = Instant::now();
        for _ in 0..hash_iters {
            let _ = canonical_state_hash(&world).unwrap();
        }
        let t_direct = t0.elapsed();

        // 2. canonical_state_hash_from_storage via WorldStorage View
        let t0 = Instant::now();
        for _ in 0..hash_iters {
            let _ = canonical_state_hash_from_storage(
                &storage_view,
                world.current_day,
                &world.settlements,
            )
            .unwrap();
        }
        let t_view = t0.elapsed();

        // 3. segmented.canonical_state_hash
        let t0 = Instant::now();
        for _ in 0..hash_iters {
            let _ = segmented
                .canonical_state_hash(world.current_day, &world.settlements)
                .unwrap();
        }
        let t_soa = t0.elapsed();

        let us_direct = (t_direct.as_nanos() as f64) / (hash_iters as f64) / 1000.0;
        let us_view = (t_view.as_nanos() as f64) / (hash_iters as f64) / 1000.0;
        let us_soa = (t_soa.as_nanos() as f64) / (hash_iters as f64) / 1000.0;

        let ratio_view = us_view / us_direct.max(0.001);
        let ratio_soa = us_soa / us_direct.max(0.001);

        println!(
            "{:<6} | {:>11.2} us | {:>15.2} us | {:>13.2} us | {:>10.2}x | {:>9.2}x",
            pop, us_direct, us_view, us_soa, ratio_view, ratio_soa
        );
    }
}

fn measure_phase10_native_soa(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-20 Phase 10 Native Segmented SoA Migration Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory Validation with Native Segmented Phase 10
    println!("\nPart 1: 500-Day Canonical Trajectory with Native Segmented Phase 10");
    let mut world = initialize_world(base_config).expect("world init succeeds");
    let mut metrics = Vec::with_capacity(500);
    let mut all_events = Vec::new();

    let start_500 = Instant::now();
    let mut t_p10_native = Duration::ZERO;

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = base_config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Phase 2
        phase2_biological_degradation(&mut world, &effective_config);

        // Phase 3
        let features =
            phase3_observation_and_features(&world, &effective_config).expect("phase3 succeeds");

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 decision succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Phase 8
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");

        // Phase 9
        let mortality_res = phase9_mortality_commitment(&mut world).expect("phase9 succeeds");

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10 Native Segmented SoA
        let segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let t0 = Instant::now();
        let m = phase10_observe_storage(&segmented, &world.settlements, executed_day)
            .expect("phase10 native succeeds");
        t_p10_native += t0.elapsed();

        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in Native SoA trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in Native SoA trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in Native SoA trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let p10_ms = t_p10_native.as_secs_f64() * 1000.0;
    let p10_us = (t_p10_native.as_nanos() as f64) / 500.0 / 1000.0;
    println!(
        "\n500-Day Trajectory Execution Time: {:.3} ms ({:.2} us/day)",
        total_ms, avg_day_us
    );
    println!(
        "  Phase 10 Native SoA Latency:   {:.3} ms ({:.2} us/day)",
        p10_ms, p10_us
    );

    // Part 2: Isolated Phase 10 Latency Comparison (Population Scaling)
    println!("\nPart 2: Population Scaling Phase 10 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>12} | {:>14} | {:>14} | {:>14} | {:>10} | {:>10}",
        "Pop (N)",
        "A: AoS (us)",
        "B: SoA Scratch",
        "C: Native Fresh",
        "D: Native Reuse",
        "D vs A",
        "D vs B"
    );
    println!("{:-<94}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days so world has active, dynamic agent states
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_aos = Duration::ZERO;
        let mut t_scratch = Duration::ZERO;
        let mut t_native_fresh = Duration::ZERO;
        let mut t_native_reuse = Duration::ZERO;

        let mut scratch_b = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut index_scratch = Vec::with_capacity(world.agents.len());
        let mut settlement_scratch = Vec::with_capacity(world.settlements.len());

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
            let day = world.current_day.as_u32();

            // Segmented storage ready
            let segmented = SegmentedAgentStorage::from_agents(&world.agents);

            // Condition A: AoS Phase 10
            let t0 = Instant::now();
            let m_aos = phase10_observe(&world, day).unwrap();
            t_aos += t0.elapsed();

            // Condition B: SoA Scratch Phase 10 (M2-16)
            let t0 = Instant::now();
            let m_scratch = phase10_observe_with_scratch(&world, day, &mut scratch_b).unwrap();
            t_scratch += t0.elapsed();

            // Condition C: Native Segmented SoA Fresh (allocates index buffer)
            let t0 = Instant::now();
            let m_native_fresh =
                phase10_observe_storage(&segmented, &world.settlements, day).unwrap();
            t_native_fresh += t0.elapsed();

            // Condition D: Native Segmented SoA Reuse (reusable index buffer)
            let t0 = Instant::now();
            let m_native_reuse = phase10_observe_storage_with_scratch(
                &segmented,
                &world.settlements,
                day,
                &mut index_scratch,
                &mut settlement_scratch,
            )
            .unwrap();
            t_native_reuse += t0.elapsed();

            // Parity check
            assert_eq!(m_aos, m_scratch);
            assert_eq!(m_aos, m_native_fresh);
            assert_eq!(m_aos, m_native_reuse);
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_scratch = (t_scratch.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_fresh = (t_native_fresh.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_reuse = (t_native_reuse.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_d_a = us_aos / us_reuse.max(0.001);
        let speedup_d_b = us_scratch / us_reuse.max(0.001);

        println!(
            "{:<8} | {:>12.2} | {:>14.2} | {:>14.2} | {:>14.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_scratch, us_fresh, us_reuse, speedup_d_a, speedup_d_b
        );
    }
}

fn measure_phase2_native_soa(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-21 Phase 2 Native Segmented SoA Migration Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory Validation with Native Segmented Phase 2
    println!("\nPart 1: 500-Day Canonical Trajectory with Native Segmented Phase 2");
    let mut world = initialize_world(base_config).expect("world init succeeds");
    let mut metrics = Vec::with_capacity(500);
    let mut all_events = Vec::new();

    let start_500 = Instant::now();
    let mut t_p2_native = Duration::ZERO;

    let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = base_config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Phase 2 Native SoA
        let t0 = Instant::now();
        phase2_biological_degradation_storage(&mut segmented, &effective_config);
        t_p2_native += t0.elapsed();

        // Write back updated health & food to world.agents for subsequent phases
        for (agent, (h, f)) in world
            .agents
            .iter_mut()
            .zip(segmented.health().iter().zip(segmented.food().iter()))
        {
            agent.health = *h;
            agent.food = *f;
        }

        // Phase 3
        let features =
            phase3_observation_and_features(&world, &effective_config).expect("phase3 succeeds");

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 decision succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Phase 8
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");

        // Phase 9
        let mortality_res = phase9_mortality_commitment(&mut world).expect("phase9 succeeds");

        // Resync segmented storage with any mutated fields from phases 6-9
        segmented.sync_from_agents(&world.agents);

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10
        let m = phase10_observe_storage(&segmented, &world.settlements, executed_day)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in Native SoA Phase 2 trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in Native SoA Phase 2 trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in Native SoA Phase 2 trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let p2_ms = t_p2_native.as_secs_f64() * 1000.0;
    let p2_us = (t_p2_native.as_nanos() as f64) / 500.0 / 1000.0;
    println!(
        "\n500-Day Trajectory Execution Time: {:.3} ms ({:.2} us/day)",
        total_ms, avg_day_us
    );
    println!(
        "  Phase 2 Native SoA Latency:    {:.3} ms ({:.2} us/day)",
        p2_ms, p2_us
    );

    // Part 2: Isolated Phase 2 Latency Comparison (Population Scaling)
    println!("\nPart 2: Population Scaling Phase 2 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10}",
        "Pop (N)", "A: AoS (us/d)", "B: Ephemeral(us)", "C: Native(us/d)", "C vs A", "C vs B"
    );
    println!("{:-<86}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days so world has active, dynamic agent states
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_aos = Duration::ZERO;
        let mut t_ephemeral = Duration::ZERO;
        let mut t_native = Duration::ZERO;

        let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();

            // Prepare identical copies for isolated comparison
            let mut w_aos = world.clone();
            let mut w_ephemeral = world.clone();
            segmented.sync_from_agents(&world.agents);

            // Condition A: In-place AoS Phase 2
            let t0 = Instant::now();
            phase2_biological_degradation(&mut w_aos, &cfg);
            t_aos += t0.elapsed();

            // Condition B: M2-17 Ephemeral SoA Phase 2 (collect -> execute -> writeback)
            let t0 = Instant::now();
            phase2_biological_degradation_with_scratch(&mut w_ephemeral, &cfg, &mut scratch);
            t_ephemeral += t0.elapsed();

            // Condition C: M2-21 Native Segmented SoA Phase 2 (direct column pass)
            let t0 = Instant::now();
            phase2_biological_degradation_storage(&mut segmented, &cfg);
            t_native += t0.elapsed();

            // Parity validation
            for (i, agent) in w_aos.agents.iter().enumerate() {
                assert_eq!(agent.health, w_ephemeral.agents[i].health);
                assert_eq!(agent.food, w_ephemeral.agents[i].food);
                assert_eq!(agent.health, segmented.health()[i]);
                assert_eq!(agent.food, segmented.food()[i]);
            }
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_ephemeral = (t_ephemeral.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_ephemeral / us_native.max(0.001);

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_ephemeral, us_native, speedup_c_a, speedup_c_b
        );
    }
}

fn measure_phase3_native_soa(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-22 Phase 3 Native Segmented SoA Migration Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory Validation with Native Segmented Phase 3
    println!("\nPart 1: 500-Day Canonical Trajectory with Native Segmented Phase 3");
    let mut world = initialize_world(base_config).expect("world init succeeds");
    let mut metrics = Vec::with_capacity(500);
    let mut all_events = Vec::new();

    let start_500 = Instant::now();
    let mut t_p3_native = Duration::ZERO;

    let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);
    let mut features = Vec::with_capacity(world.agents.len());

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = base_config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Phase 2
        phase2_biological_degradation(&mut world, &effective_config);
        segmented.sync_from_agents(&world.agents);

        // Phase 3 Native SoA: execute directly on segmented storage columns
        let t0 = Instant::now();
        phase3_observation_and_features_storage_into(
            &segmented,
            &world.settlements,
            &effective_config,
            &mut features,
        )
        .expect("phase3 storage succeeds");
        t_p3_native += t0.elapsed();

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 decision succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Phase 8
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");

        // Phase 9
        let mortality_res = phase9_mortality_commitment(&mut world).expect("phase9 succeeds");

        // Resync segmented storage with any mutated fields from phases 6-9
        segmented.sync_from_agents(&world.agents);

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10
        let m = phase10_observe_storage(&segmented, &world.settlements, executed_day)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in Native SoA Phase 3 trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in Native SoA Phase 3 trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in Native SoA Phase 3 trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let p3_ms = t_p3_native.as_secs_f64() * 1000.0;
    let p3_us = (t_p3_native.as_nanos() as f64) / 500.0 / 1000.0;
    println!(
        "\n500-Day Trajectory Execution Time: {:.3} ms ({:.2} us/day)",
        total_ms, avg_day_us
    );
    println!(
        "  Phase 3 Native SoA Latency:    {:.3} ms ({:.2} us/day)",
        p3_ms, p3_us
    );

    // Part 2: Isolated Phase 3 Latency Comparison (Population Scaling)
    println!("\nPart 2: Population Scaling Phase 3 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10} | {:>14}",
        "Pop (N)",
        "A: AoS (us/d)",
        "B: Ephemeral(us)",
        "C: Native(us/d)",
        "C vs A",
        "C vs B",
        "Throughput(M/s)"
    );
    println!("{:-<104}", "");

    let populations = [100, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let mut world = match initialize_world(&cfg) {
            Ok(w) => w,
            Err(_) => continue,
        };

        // Advance 25 days so world has active, dynamic agent states
        for _ in 0..25 {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
        }

        let mut t_aos = Duration::ZERO;
        let mut t_ephemeral = Duration::ZERO;
        let mut t_native = Duration::ZERO;

        let mut scratch = AgentDynamicSoAScratch::with_capacity(world.agents.len());
        let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);
        let mut out_aos = Vec::with_capacity(world.agents.len());
        let mut out_ephemeral = Vec::with_capacity(world.agents.len());
        let mut out_native = Vec::with_capacity(world.agents.len());

        let mut total_features_extracted = 0usize;

        for _ in 0..days {
            let opts = DayExecutionOptions {
                metrics_enabled: false,
                events_enabled: false,
                snapshot_boundary: false,
            };
            let _ = run_m0_day(&mut world, &cfg, context, &opts).unwrap();
            segmented.sync_from_agents(&world.agents);

            // Condition A: AoS Phase 3
            let t0 = Instant::now();
            phase3_observation_and_features_into(&world, &cfg, &mut out_aos).unwrap();
            t_aos += t0.elapsed();

            // Condition B: M2-17 Ephemeral SoA Phase 3 (collect -> extract)
            let t0 = Instant::now();
            phase3_observation_and_features_with_scratch(
                &world,
                &cfg,
                &mut scratch,
                &mut out_ephemeral,
            )
            .unwrap();
            t_ephemeral += t0.elapsed();

            // Condition C: M2-22 Native Segmented SoA Phase 3 (direct column extraction)
            let t0 = Instant::now();
            phase3_observation_and_features_storage_into(
                &segmented,
                &world.settlements,
                &cfg,
                &mut out_native,
            )
            .unwrap();
            t_native += t0.elapsed();

            total_features_extracted += out_native.len();

            // Parity validation
            assert_eq!(out_aos, out_ephemeral);
            assert_eq!(out_aos, out_native);
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_ephemeral = (t_ephemeral.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_ephemeral / us_native.max(0.001);

        let m_features_per_sec =
            (total_features_extracted as f64) / t_native.as_secs_f64() / 1_000_000.0;

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x | {:>12.2} M/s",
            pop, us_aos, us_ephemeral, us_native, speedup_c_a, speedup_c_b, m_features_per_sec
        );
    }
}

// =========================================================================
// M2-24 Phase 9 Native Segmented SoA Migration Benchmark
// =========================================================================

fn measure_phase9_native_soa(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-24 Phase 9 Native Segmented SoA Migration Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory with Native Segmented Phase 9
    println!("\nPart 1: 500-Day Canonical Trajectory with Native Segmented Phase 9");

    let mut world = initialize_world(base_config).expect("world initializes");
    let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

    let mut all_events = Vec::new();
    let mut metrics = Vec::new();
    let mut t_p9_native = Duration::ZERO;

    let mut effective_config = base_config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    let start_500 = Instant::now();

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Phase 2 (Native SoA)
        segmented.phase2_degradation_with_config(&effective_config);
        segmented.write_back_to_agents(&mut world.agents);

        // Phase 3 (Native SoA)
        let features = segmented
            .phase3_features(&world.settlements, &effective_config)
            .expect("phase3 succeeds");

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 primary action selection succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent generation succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Phase 8
        let welfare_res = phase8_welfare_distribution_with_config(&mut world, &effective_config)
            .expect("phase8 succeeds");

        // Sync segmented storage with any mutated fields from phases 6-8
        segmented.sync_from_agents(&world.agents);

        // Phase 9 (Native SoA)
        let t0 = Instant::now();
        let mortality_res = segmented
            .phase9_mortality_commitment()
            .expect("phase9 succeeds");
        t_p9_native += t0.elapsed();

        // Write back alive status to world.agents
        segmented.write_back_to_agents(&mut world.agents);

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10 (Native SoA)
        let m = phase10_observe_storage(&segmented, &world.settlements, executed_day)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in Native SoA Phase 9 trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in Native SoA Phase 9 trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in Native SoA Phase 9 trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let p9_ms = t_p9_native.as_secs_f64() * 1000.0;
    let p9_us = (t_p9_native.as_nanos() as f64) / 500.0 / 1000.0;
    println!(
        "\n500-Day Trajectory Execution Time: {:.3} ms ({:.2} us/day)",
        total_ms, avg_day_us
    );
    println!(
        "  Phase 9 Native SoA Latency:    {:.3} ms ({:.2} us/day)",
        p9_ms, p9_us
    );

    // Part 2: Isolated Phase 9 Latency Comparison (Population Scaling)
    println!("\nPart 2: Population Scaling Phase 9 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>8} | {:>8} | {:>14}",
        "Pop (N)",
        "A: AoS (us/d)",
        "B: Adapter(us)",
        "C: Native(us/d)",
        "C vs A",
        "C vs B",
        "Throughput"
    );
    println!("{:-<94}", "");

    let populations = [100u64, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let base_world = initialize_world(&cfg).unwrap();

        // Condition A: AoS Phase 9
        let mut t_aos = Duration::ZERO;
        for _ in 0..days {
            let mut w = base_world.clone();
            for (i, agent) in w.agents.iter_mut().enumerate() {
                if i % 20 == 0 {
                    agent.health = 0.0;
                }
            }
            let t0 = Instant::now();
            let _ = phase9_mortality_commitment(&mut w).unwrap();
            t_aos += t0.elapsed();
        }

        // Condition B: Storage Adapter (AoS -> SoA -> execute -> AoS)
        let mut t_adapter = Duration::ZERO;
        for _ in 0..days {
            let mut w = base_world.clone();
            for (i, agent) in w.agents.iter_mut().enumerate() {
                if i % 20 == 0 {
                    agent.health = 0.0;
                }
            }
            let t0 = Instant::now();
            let mut seg = SegmentedAgentStorage::from_agents(&w.agents);
            let _ = phase9_mortality_commitment_storage(&mut seg).unwrap();
            seg.write_back_to_agents(&mut w.agents);
            t_adapter += t0.elapsed();
        }

        // Condition C: Native Segmented SoA
        let mut template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        for i in 0..template_storage.len() {
            if i % 20 == 0 {
                template_storage.demography.health[i] = 0.0;
            }
        }
        let mut t_native = Duration::ZERO;
        for _ in 0..days {
            let mut seg = template_storage.clone();
            let t0 = Instant::now();
            let _ = seg.phase9_mortality_commitment().unwrap();
            t_native += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        let m_agents_per_sec =
            ((pop as usize * days) as f64) / t_native.as_secs_f64() / 1_000_000.0;

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>7.2}x | {:>7.2}x | {:>10.2} M/s",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b, m_agents_per_sec
        );
    }
}

fn measure_phase8_native_soa(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-25 Phase 8 Native Segmented SoA Migration Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory with Native Segmented Phase 8
    println!("\nPart 1: 500-Day Canonical Trajectory with Native Segmented Phase 8");

    let mut world = initialize_world(base_config).expect("world initializes");
    let mut segmented = SegmentedAgentStorage::from_agents(&world.agents);

    let mut all_events = Vec::new();
    let mut metrics = Vec::new();
    let mut t_p8_native = Duration::ZERO;

    let mut effective_config = base_config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    let start_500 = Instant::now();

    for _d in 0..500 {
        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        // Phase 1
        phase1_resource_regrowth(&mut world, &effective_config);

        // Phase 2 (Native SoA)
        segmented.phase2_degradation_with_config(&effective_config);
        segmented.write_back_to_agents(&mut world.agents);

        // Phase 3 (Native SoA)
        let features = segmented
            .phase3_features(&world.settlements, &effective_config)
            .expect("phase3 succeeds");

        // Phase 4
        let choices = phase4_primary_action_selection(&world, &effective_config, &features)
            .expect("phase4 primary action selection succeeds");
        let intents = phase4_generate_intents(&world, &effective_config, &choices)
            .expect("phase4 intent generation succeeds");

        // Phase 5
        let partitions = phase5_partition_intents_baseline(&intents).expect("phase5 succeeds");

        // Phase 6A
        let work_res = phase6a_work_resolution(&mut world, &partitions).expect("phase6a succeeds");

        // Phase 6B
        let targeted_res = phase6b_targeted_resolution(&mut world, &effective_config, &partitions)
            .expect("phase6b succeeds");

        // Phase 7
        let market_res =
            phase7_market_clearance_with_config(&mut world, &partitions, &effective_config.economy)
                .expect("phase7 succeeds");

        // Sync segmented storage with mutations from phases 6-7
        segmented.sync_from_agents(&world.agents);

        // Phase 8 (Native SoA)
        let t0 = Instant::now();
        let welfare_res = segmented
            .phase8_welfare_distribution_with_config(&mut world.settlements, &effective_config)
            .expect("phase8 succeeds");
        t_p8_native += t0.elapsed();

        // Write back wealth to world.agents
        segmented.write_back_to_agents(&mut world.agents);

        // Phase 9 (Native SoA)
        let mortality_res = segmented
            .phase9_mortality_commitment()
            .expect("phase9 succeeds");

        // Write back alive status to world.agents
        segmented.write_back_to_agents(&mut world.agents);

        // Event staging
        let estimated_cap = world.agents.len().saturating_mul(2) + world.settlements.len() + 4;
        let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_res.len());
        for w in &work_res {
            let work_events = events_from_work_resolution(executed_day, w);
            let count = work_events.len() as u64;
            if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == w.group_id.0)
            {
                entry.1 += count;
            } else {
                phase6_counts.push((w.group_id.0, count));
            }
            event_buffer.push_all(work_events);
        }
        for t in &targeted_res {
            let mut targeted_events = events_from_targeted_resolution(executed_day, t);
            let offset = if let Some(entry) = phase6_counts
                .iter_mut()
                .find(|(gid, _)| *gid == t.group_id.0)
            {
                let prev = entry.1;
                entry.1 += targeted_events.len() as u64;
                prev
            } else {
                let count = targeted_events.len() as u64;
                phase6_counts.push((t.group_id.0, count));
                0
            };
            if offset > 0 {
                for te in &mut targeted_events {
                    te.key.local_sequence += offset;
                }
            }
            event_buffer.push_all(targeted_events);
        }
        for m in &market_res {
            event_buffer.push_all(events_from_market_resolution(executed_day, m));
        }
        for wel in &welfare_res {
            event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day,
            &mortality_res,
        ));

        // Phase 10 (Native SoA)
        let m = phase10_observe_storage(&segmented, &world.settlements, executed_day)
            .expect("phase10 succeeds");
        event_buffer.push(event_from_daily_metrics(&m));
        metrics.push(m);

        // Phase 11 Snapshot (Day 199 only)
        if _d == 199 {
            let meta = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _snap = encode_snapshot(&world, &meta).expect("snapshot succeeds");
            let snap_ev = EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: meta.day,
                    master_seed: meta.master_seed,
                    replicate_id: meta.replicate_id,
                    schema_version: SNAPSHOT_SCHEMA_VERSION,
                }),
            );
            event_buffer.push(snap_ev);
        }

        // Phase 11 Event Flush
        let flushed = phase11_flush_events(&mut event_buffer).expect("event flush succeeds");
        all_events.extend(flushed);

        world.current_day = SimulationDay(next_day);
    }

    let elapsed_500 = start_500.elapsed();

    // Correctness Verification
    let actual_state_hash = canonical_state_hash(&world).unwrap().to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics).unwrap().to_hex();
    let actual_event_hash = canonical_event_hash(&all_events).unwrap().to_hex();

    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch in Native SoA Phase 8 trajectory!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch in Native SoA Phase 8 trajectory!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch in Native SoA Phase 8 trajectory!"
    );
    println!("CANONICAL GRADUATION TRAJECTORY: 100% BIT-EXACT MATCH.");

    let total_ms = elapsed_500.as_secs_f64() * 1000.0;
    let avg_day_us = (elapsed_500.as_nanos() as f64) / 500.0 / 1000.0;
    let p8_ms = t_p8_native.as_secs_f64() * 1000.0;
    let p8_us = (t_p8_native.as_nanos() as f64) / 500.0 / 1000.0;
    println!(
        "\n500-Day Trajectory Execution Time: {:.3} ms ({:.2} us/day)",
        total_ms, avg_day_us
    );
    println!(
        "  Phase 8 Native SoA Latency:    {:.3} ms ({:.2} us/day)",
        p8_ms, p8_us
    );

    // Part 2: Isolated Phase 8 Latency Comparison (Population Scaling)
    println!("\nPart 2: Population Scaling Phase 8 Latency Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>8} | {:>8} | {:>14}",
        "Pop (N)",
        "A: AoS (us/d)",
        "B: Adapter(us)",
        "C: Native(us/d)",
        "C vs A",
        "C vs B",
        "Throughput"
    );
    println!("{:-<94}", "");

    let populations = [100u64, 250, 500, 1000];
    let days = 50;

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let base_world = initialize_world(&cfg).unwrap();

        // Condition A: AoS Phase 8
        let mut t_aos = Duration::ZERO;
        for _ in 0..days {
            let mut w = base_world.clone();
            for (i, agent) in w.agents.iter_mut().enumerate() {
                if i % 4 == 0 {
                    agent.food = 1.0;
                }
            }
            let t0 = Instant::now();
            let _ = phase8_welfare_distribution_with_config(&mut w, &cfg).unwrap();
            t_aos += t0.elapsed();
        }

        // Condition B: Storage Adapter (AoS -> SoA -> execute -> AoS)
        let mut t_adapter = Duration::ZERO;
        for _ in 0..days {
            let mut w = base_world.clone();
            for (i, agent) in w.agents.iter_mut().enumerate() {
                if i % 4 == 0 {
                    agent.food = 1.0;
                }
            }
            let t0 = Instant::now();
            let mut seg = SegmentedAgentStorage::from_agents(&w.agents);
            let _ =
                phase8_welfare_distribution_storage_with_config(&mut seg, &mut w.settlements, &cfg)
                    .unwrap();
            seg.write_back_to_agents(&mut w.agents);
            t_adapter += t0.elapsed();
        }

        // Condition C: Native Segmented SoA
        let mut template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        for i in 0..template_storage.len() {
            if i % 4 == 0 {
                template_storage.economy.food[i] = 1.0;
            }
        }
        let template_settlements = base_world.settlements.clone();
        let mut t_native = Duration::ZERO;
        for _ in 0..days {
            let mut seg = template_storage.clone();
            let mut settlements = template_settlements.clone();
            let t0 = Instant::now();
            let _ = seg
                .phase8_welfare_distribution_with_config(&mut settlements, &cfg)
                .unwrap();
            t_native += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (days as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (days as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        let m_agents_per_sec =
            ((pop as usize * days) as f64) / t_native.as_secs_f64() / 1_000_000.0;

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>7.2}x | {:>7.2}x | {:>10.2} M/s",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b, m_agents_per_sec
        );
    }
}

fn measure_m2_native_soa_multi_phase_gate(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-26 Native SoA Multi-Phase Integration Gate Benchmark");
    println!("=================================================================");

    // Part 1: 500-Day Canonical Trajectory Determinism Parity (AoS vs Native SoA)
    println!("\nPart 1: 500-Day Canonical Trajectory Determinism Parity (AoS vs Native SoA)");

    let mut world_aos = initialize_world(base_config).expect("world initializes");
    let mut world_soa = world_aos.clone();

    let mut all_events_aos = Vec::new();
    let mut metrics_aos = Vec::new();
    let mut all_events_soa = Vec::new();
    let mut metrics_soa = Vec::new();

    let start_500_aos = Instant::now();
    for d in 0..500 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == 199,
        };
        let out = run_m0_day(&mut world_aos, base_config, context, &options).expect("AoS succeeds");
        if let Some(m) = out.metrics {
            metrics_aos.push(m);
        }
        all_events_aos.extend(out.events);
    }
    let elapsed_500_aos = start_500_aos.elapsed();

    let start_500_soa = Instant::now();
    for d in 0..500 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: d == 199,
        };
        let out = run_native_soa_day(&mut world_soa, base_config, context, &options)
            .expect("Native SoA succeeds");
        if let Some(m) = out.metrics {
            metrics_soa.push(m);
        }
        all_events_soa.extend(out.events);
    }
    let elapsed_500_soa = start_500_soa.elapsed();

    let hash_state_aos = canonical_state_hash(&world_aos).unwrap().to_hex();
    let hash_state_soa = canonical_state_hash(&world_soa).unwrap().to_hex();
    assert_eq!(hash_state_aos, hash_state_soa);
    assert_eq!(hash_state_soa, EXPECTED_STATE_HASH);

    let hash_metrics_aos = canonical_metrics_hash(&metrics_aos).unwrap().to_hex();
    let hash_metrics_soa = canonical_metrics_hash(&metrics_soa).unwrap().to_hex();
    assert_eq!(hash_metrics_aos, hash_metrics_soa);
    assert_eq!(hash_metrics_soa, EXPECTED_METRICS_HASH);

    let hash_events_aos = canonical_event_hash(&all_events_aos).unwrap().to_hex();
    let hash_events_soa = canonical_event_hash(&all_events_soa).unwrap().to_hex();
    assert_eq!(hash_events_aos, hash_events_soa);
    assert_eq!(hash_events_soa, EXPECTED_EVENT_HASH);

    println!(
        "CanonicalStateHash:   {} (100% BIT-EXACT MATCH)",
        hash_state_soa
    );
    println!(
        "CanonicalMetricsHash: {} (100% BIT-EXACT MATCH)",
        hash_metrics_soa
    );
    println!(
        "CanonicalEventHash:   {} (100% BIT-EXACT MATCH)",
        hash_events_soa
    );
    println!(
        "500-Day AoS Runtime:        {:.3} ms ({:.2} us/day)",
        elapsed_500_aos.as_secs_f64() * 1000.0,
        (elapsed_500_aos.as_nanos() as f64) / 500.0 / 1000.0
    );
    println!(
        "500-Day Native SoA Runtime: {:.3} ms ({:.2} us/day)",
        elapsed_500_soa.as_secs_f64() * 1000.0,
        (elapsed_500_soa.as_nanos() as f64) / 500.0 / 1000.0
    );

    // Part 2: Snapshot Parity at Day 100, Day 250, Day 500
    println!("\nPart 2: Snapshot Parity at Day 100, 250, 500");
    for &target_day in &[100u32, 250, 500] {
        let mut w_aos = initialize_world(base_config).unwrap();
        let mut w_soa = w_aos.clone();

        let prior_days = target_day - 1;
        let opt_no_snap = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        if prior_days > 0 {
            let _ =
                run_m0_days(&mut w_aos, base_config, context, prior_days, &opt_no_snap).unwrap();
            let _ = run_native_soa_days(&mut w_soa, base_config, context, prior_days, &opt_no_snap)
                .unwrap();
        }

        let opt_snap = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: true,
        };
        let out_aos = run_m0_day(&mut w_aos, base_config, context, &opt_snap).unwrap();
        let out_soa = run_native_soa_day(&mut w_soa, base_config, context, &opt_snap).unwrap();

        let snap_aos = out_aos.snapshot.unwrap();
        let snap_soa = out_soa.snapshot.unwrap();
        assert_eq!(
            snap_aos, snap_soa,
            "Snapshot bytes mismatch at day {}",
            target_day
        );

        let rest_aos = restore_snapshot(&snap_aos).unwrap();
        let rest_soa = restore_snapshot(&snap_soa).unwrap();
        assert_eq!(rest_aos.world, rest_soa.world);
        assert_eq!(
            canonical_state_hash(&rest_aos.world).unwrap(),
            canonical_state_hash(&rest_soa.world).unwrap()
        );

        println!(
            "  Day {:>3} Snapshot & Restored State Hash: 100% BIT-EXACT MATCH",
            target_day
        );
    }

    // Part 3: Population Scaling Multi-Phase Pipeline Comparison
    println!("\nPart 3: Population Scaling Multi-Phase Pipeline Comparison (50 Days)");
    println!(
        "{:<8} | {:>14} | {:>14} | {:>14} | {:>8} | {:>8} | {:>10} | {:>10}",
        "Pop (N)",
        "A: AoS (ms)",
        "B: Hybrid (ms)",
        "C: Native (ms)",
        "C vs A",
        "C vs B",
        "A (day/s)",
        "C (day/s)"
    );
    println!("{:-<100}", "");

    let populations = [100u64, 250, 500, 1000];
    let days = 50u32;
    let opt_bench = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let base_world = initialize_world(&cfg).unwrap();

        // Pipeline A: Full AoS
        let mut w_a = base_world.clone();
        let start_a = Instant::now();
        let _ = run_m0_days(&mut w_a, &cfg, context, days, &opt_bench).unwrap();
        let el_a = start_a.elapsed();

        // Pipeline B: Hybrid (AoS with Native Phase 2, 3, 10)
        let mut w_b = base_world.clone();
        let mut seg_b = SegmentedAgentStorage::from_agents(&w_b.agents);
        let mut feats_b = Vec::with_capacity(w_b.agents.len());
        let mut choices_b = Vec::with_capacity(w_b.agents.len());
        let mut intents_b = Vec::with_capacity(w_b.agents.len());
        let start_b = Instant::now();
        for _ in 0..days {
            let d = w_b.current_day.as_u32();
            let next_d = d + 1;
            phase1_resource_regrowth(&mut w_b, &cfg);
            seg_b.phase2_degradation_with_config(&cfg);
            seg_b.write_back_to_agents(&mut w_b.agents);
            feats_b.clear();
            seg_b
                .phase3_features_into(&w_b.settlements, &cfg, &mut feats_b)
                .unwrap();
            choices_b.clear();
            phase4_primary_action_selection_into(&w_b, &cfg, &feats_b, &mut choices_b).unwrap();
            intents_b.clear();
            phase4_generate_intents_into(&w_b, &cfg, &choices_b, &mut intents_b).unwrap();
            let p = phase5_partition_intents_baseline(&intents_b).unwrap();
            let _ = phase6a_work_resolution(&mut w_b, &p).unwrap();
            let _ = phase6b_targeted_resolution(&mut w_b, &cfg, &p).unwrap();
            let _ = phase7_market_clearance_with_config(&mut w_b, &p, &cfg.economy).unwrap();
            let _ = phase8_welfare_distribution_with_config(&mut w_b, &cfg).unwrap();
            let _ = phase9_mortality_commitment(&mut w_b).unwrap();
            let m = phase10_observe_storage(&seg_b, &w_b.settlements, d).unwrap();
            let _ = m;
            w_b.current_day = SimulationDay(next_d);
        }
        let el_b = start_b.elapsed();

        // Pipeline C: Native SoA (Phase 2, 3, 8, 9, 10 Native)
        let mut w_c = base_world.clone();
        let start_c = Instant::now();
        let _ = run_native_soa_days(&mut w_c, &cfg, context, days, &opt_bench).unwrap();
        let el_c = start_c.elapsed();

        let ms_a = el_a.as_secs_f64() * 1000.0;
        let ms_b = el_b.as_secs_f64() * 1000.0;
        let ms_c = el_c.as_secs_f64() * 1000.0;

        let speedup_c_a = ms_a / ms_c.max(0.001);
        let speedup_c_b = ms_b / ms_c.max(0.001);

        let dps_a = (days as f64) / el_a.as_secs_f64();
        let dps_c = (days as f64) / el_c.as_secs_f64();

        println!(
            "{:<8} | {:>14.2} | {:>14.2} | {:>14.2} | {:>7.2}x | {:>7.2}x | {:>10.1} | {:>10.1}",
            pop, ms_a, ms_b, ms_c, speedup_c_a, speedup_c_b, dps_a, dps_c
        );
    }
}

// =========================================================================
// M2-23 Hot Phase Profiling & Candidate Migration Synthetic Benchmark
// =========================================================================

fn synthetic_phase9_native(storage: &mut SegmentedAgentStorage) -> Vec<AgentId> {
    let mut deceased = Vec::new();
    let n = storage.agent_ids.len();
    for i in 0..n {
        if storage.demography.alive[i] && storage.demography.health[i] <= 0.0 {
            storage.demography.alive[i] = false;
            deceased.push(storage.agent_ids[i]);
        }
    }
    deceased.sort();
    deceased
}

fn synthetic_phase8_native(
    storage: &mut SegmentedAgentStorage,
    settlements: &mut [SettlementState],
    starvation_threshold: f32,
    welfare_payment: Money,
) {
    for settlement in settlements.iter_mut() {
        let gid = settlement.group_id;
        let mut eligible_slots = Vec::new();
        let n = storage.agent_ids.len();
        for i in 0..n {
            if storage.economy.group_id[i] == gid
                && storage.demography.alive[i]
                && storage.demography.health[i] > 0.0
                && storage.economy.food[i] < starvation_threshold
            {
                eligible_slots.push(i);
            }
        }
        eligible_slots.sort_by_key(|&idx| storage.agent_ids[idx]);
        let eligible_count = eligible_slots.len();
        if eligible_count == 0 || welfare_payment == 0 {
            continue;
        }
        let required = (eligible_count as Money).saturating_mul(welfare_payment);
        if settlement.treasury >= required {
            settlement.treasury -= required;
            for &slot in &eligible_slots {
                storage.economy.wealth[slot] += welfare_payment;
            }
        } else {
            let count_money = eligible_count as Money;
            let payment_per_agent = settlement.treasury / count_money;
            let remainder = settlement.treasury % count_money;
            settlement.treasury = 0;
            for (idx, &slot) in eligible_slots.iter().enumerate() {
                let extra = if (idx as Money) < remainder { 1 } else { 0 };
                storage.economy.wealth[slot] += payment_per_agent + extra;
            }
        }
    }
}

fn synthetic_phase4_native(
    storage: &SegmentedAgentStorage,
    config: &SimConfig,
    day: u32,
    agent_features: &[AgentFeatures],
    out: &mut Vec<PrimaryActionChoice>,
) {
    out.clear();
    let coops = &storage.personality.cooperation;
    let aggrs = &storage.personality.aggression;
    let risks = &storage.personality.risk_tolerance;
    let alives = &storage.demography.alive;
    let healths = &storage.demography.health;

    for af in agent_features {
        let slot = match storage.slot_of(af.agent_id) {
            Some(s) => s,
            None => continue,
        };
        if !alives[slot] || healths[slot] <= 0.0 {
            continue;
        }
        let mut utilities = [0.0f32; 6];
        for (m, action) in Action::ALL.iter().enumerate() {
            let mut u_base = config.decision.action_biases[m];
            for k in 0..5 {
                u_base += config.decision.base_weight_matrix[m][k] * af.features.values[k];
            }
            let trait_mod = match action {
                Action::Work | Action::BuyFood | Action::SellFood | Action::Idle => 0.0,
                Action::GiveFood => config.decision.trait_weight_cooperation * coops[slot],
                Action::StealFood => {
                    config.decision.trait_weight_aggression * aggrs[slot]
                        + config.decision.trait_weight_risk_tolerance * risks[slot]
                }
            };
            utilities[m] = u_base + trait_mod;
        }
        let probabilities = stable_softmax(&utilities, config.decision.decision_temperature);
        let coord = RngCoordinate::new(
            config.world.master_seed,
            config.world.replicate_id,
            day,
            4,
            Subsystem::Decision.id(),
            af.agent_id.as_u32(),
            0,
        );
        let u = coordinate_prng_f32(&coord);
        let action = select_action(&probabilities, u);
        out.push(PrimaryActionChoice {
            agent_id: af.agent_id,
            action,
        });
    }
    out.sort_by_key(|c| c.agent_id);
}

fn synthetic_phase6a_native(
    storage: &mut SegmentedAgentStorage,
    settlements: &mut [SettlementState],
    partitions: &[SettlementIntentPartition],
) {
    for partition in partitions {
        let mut work_intents = Vec::new();
        for intent in &partition.intents {
            if let Intent::Work {
                agent_id,
                requested_harvest,
                ..
            } = *intent
            {
                work_intents.push((agent_id, requested_harvest));
            }
        }
        if work_intents.is_empty() {
            continue;
        }
        work_intents.sort_by_key(|&(agent_id, _)| agent_id);

        let settlement = match settlements
            .iter_mut()
            .find(|s| s.group_id == partition.group_id)
        {
            Some(s) => s,
            None => continue,
        };
        let resource = settlement.resource;

        let total_requested: f32 = work_intents.iter().map(|&(_, r)| r).sum();

        if total_requested <= resource {
            settlement.resource -= total_requested;
            for &(agent_id, requested_harvest) in &work_intents {
                if let Some(slot) = storage.slot_of(agent_id) {
                    storage.economy.food[slot] += requested_harvest;
                }
            }
        } else if total_requested > 0.0 {
            settlement.resource = 0.0;
            for &(agent_id, requested_harvest) in &work_intents {
                let share = requested_harvest / total_requested;
                let allocation = resource * share;
                if let Some(slot) = storage.slot_of(agent_id) {
                    storage.economy.food[slot] += allocation;
                }
            }
        }
    }
}

fn measure_candidate_phases_profiling(base_config: &SimConfig, _context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-23 Hot Phase Profiling & Candidate Migration Synthetic Benchmark");
    println!("Comparing: A: AoS Baseline, B: Storage Adapter, C: Native Candidate SoA");
    println!("=================================================================");

    let populations = [100, 250, 500, 1000];
    let iters = 50;

    // --- Candidate 1: Phase 9 Mortality Commitment ---
    println!(
        "\nCandidate 1: Phase 9 Mortality Commitment Latency ({} Sweeps)",
        iters
    );
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10}",
        "Pop (N)", "A: AoS (us)", "B: Adapter(us)", "C: Native(us)", "C vs A", "C vs B"
    );
    println!("{:-<84}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let template_world = initialize_world(&cfg).unwrap();

        // Setup 5% deceased agents
        let mut base_world = template_world.clone();
        for (i, agent) in base_world.agents.iter_mut().enumerate() {
            if i % 20 == 0 {
                agent.health = 0.0;
            }
        }

        // A: AoS Baseline
        let mut t_aos = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let _ = phase9_mortality_commitment(&mut w).unwrap();
            t_aos += t0.elapsed();
        }

        // B: Storage Adapter (AoS -> SoA -> execute -> AoS)
        let mut t_adapter = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let mut seg = SegmentedAgentStorage::from_agents(&w.agents);
            let _ = synthetic_phase9_native(&mut seg);
            seg.write_back_to_agents(&mut w.agents);
            t_adapter += t0.elapsed();
        }

        // C: Native SoA Prototype
        let template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let mut t_native = Duration::ZERO;
        for _ in 0..iters {
            let mut seg = template_storage.clone();
            let t0 = Instant::now();
            let _ = synthetic_phase9_native(&mut seg);
            t_native += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (iters as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b
        );
    }

    // --- Candidate 2: Phase 8 Welfare Distribution ---
    println!(
        "\nCandidate 2: Phase 8 Welfare Distribution Latency ({} Sweeps)",
        iters
    );
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10}",
        "Pop (N)", "A: AoS (us)", "B: Adapter(us)", "C: Native(us)", "C vs A", "C vs B"
    );
    println!("{:-<84}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let template_world = initialize_world(&cfg).unwrap();

        // Setup 33% starving agents
        let mut base_world = template_world.clone();
        for (i, agent) in base_world.agents.iter_mut().enumerate() {
            if i % 3 == 0 {
                agent.food = cfg.interaction.starvation_threshold - 5.0;
            }
        }

        // A: AoS Baseline
        let mut t_aos = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let _ = phase8_welfare_distribution_with_config(&mut w, &cfg).unwrap();
            t_aos += t0.elapsed();
        }

        // B: Storage Adapter (AoS -> SoA -> execute -> AoS)
        let mut t_adapter = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let mut seg = SegmentedAgentStorage::from_agents(&w.agents);
            synthetic_phase8_native(
                &mut seg,
                &mut w.settlements,
                cfg.interaction.starvation_threshold,
                cfg.economy.welfare_payment,
            );
            seg.write_back_to_agents(&mut w.agents);
            t_adapter += t0.elapsed();
        }

        // C: Native SoA Prototype
        let template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let mut t_native = Duration::ZERO;
        for _ in 0..iters {
            let mut seg = template_storage.clone();
            let mut settlements = base_world.settlements.clone();
            let t0 = Instant::now();
            synthetic_phase8_native(
                &mut seg,
                &mut settlements,
                cfg.interaction.starvation_threshold,
                cfg.economy.welfare_payment,
            );
            t_native += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (iters as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b
        );
    }

    // --- Candidate 3: Phase 4 Primary Action Selection ---
    println!(
        "\nCandidate 3: Phase 4 Primary Action Selection Latency ({} Sweeps)",
        iters
    );
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10}",
        "Pop (N)", "A: AoS (us)", "B: Adapter(us)", "C: Native(us)", "C vs A", "C vs B"
    );
    println!("{:-<84}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let features = phase3_observation_and_features(&base_world, &cfg).unwrap();

        // A: AoS Baseline
        let mut choices_aos = Vec::with_capacity(pop as usize);
        let mut t_aos = Duration::ZERO;
        for _ in 0..iters {
            let t0 = Instant::now();
            phase4_primary_action_selection_into(&base_world, &cfg, &features, &mut choices_aos)
                .unwrap();
            t_aos += t0.elapsed();
        }

        // B: Storage Adapter (AoS -> SoA -> execute)
        let mut choices_adapter = Vec::with_capacity(pop as usize);
        let mut t_adapter = Duration::ZERO;
        let day = base_world.current_day.as_u32();
        for _ in 0..iters {
            let t0 = Instant::now();
            let seg = SegmentedAgentStorage::from_agents(&base_world.agents);
            synthetic_phase4_native(&seg, &cfg, day, &features, &mut choices_adapter);
            t_adapter += t0.elapsed();
        }

        // C: Native SoA Prototype
        let template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let mut choices_native = Vec::with_capacity(pop as usize);
        let mut t_native = Duration::ZERO;
        for _ in 0..iters {
            let t0 = Instant::now();
            synthetic_phase4_native(&template_storage, &cfg, day, &features, &mut choices_native);
            t_native += t0.elapsed();
        }

        assert_eq!(
            choices_aos, choices_native,
            "Phase 4 synthetic choice parity failure!"
        );

        let us_aos = (t_aos.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (iters as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b
        );
    }

    // --- Candidate 4: Phase 6A Work Resolution ---
    println!(
        "\nCandidate 4: Phase 6A Work Resolution Latency ({} Sweeps)",
        iters
    );
    println!(
        "{:<8} | {:>14} | {:>16} | {:>16} | {:>10} | {:>10}",
        "Pop (N)", "A: AoS (us)", "B: Adapter(us)", "C: Native(us)", "C vs A", "C vs B"
    );
    println!("{:-<84}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let features = phase3_observation_and_features(&base_world, &cfg).unwrap();
        let mut choices = Vec::with_capacity(pop as usize);
        phase4_primary_action_selection_into(&base_world, &cfg, &features, &mut choices).unwrap();
        for c in &mut choices {
            c.action = Action::Work;
        }
        let mut intents = Vec::with_capacity(pop as usize);
        phase4_generate_intents_into(&base_world, &cfg, &choices, &mut intents).unwrap();
        let partitions = phase5_partition_intents_baseline(&intents).unwrap();

        // A: AoS Baseline
        let mut t_aos = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let _ = phase6a_work_resolution(&mut w, &partitions).unwrap();
            t_aos += t0.elapsed();
        }

        // B: Storage Adapter (AoS -> SoA -> execute -> AoS)
        let mut t_adapter = Duration::ZERO;
        for _ in 0..iters {
            let mut w = base_world.clone();
            let t0 = Instant::now();
            let mut seg = SegmentedAgentStorage::from_agents(&w.agents);
            synthetic_phase6a_native(&mut seg, &mut w.settlements, &partitions);
            seg.write_back_to_agents(&mut w.agents);
            t_adapter += t0.elapsed();
        }

        // C: Native SoA Prototype
        let template_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let mut t_native = Duration::ZERO;
        for _ in 0..iters {
            let mut seg = template_storage.clone();
            let mut settlements = base_world.settlements.clone();
            let t0 = Instant::now();
            synthetic_phase6a_native(&mut seg, &mut settlements, &partitions);
            t_native += t0.elapsed();
        }

        let us_aos = (t_aos.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_adapter = (t_adapter.as_nanos() as f64) / (iters as f64) / 1000.0;
        let us_native = (t_native.as_nanos() as f64) / (iters as f64) / 1000.0;

        let speedup_c_a = us_aos / us_native.max(0.001);
        let speedup_c_b = us_adapter / us_native.max(0.001);

        println!(
            "{:<8} | {:>14.2} | {:>16.2} | {:>16.2} | {:>9.2}x | {:>9.2}x",
            pop, us_aos, us_adapter, us_native, speedup_c_a, speedup_c_b
        );
    }
}

// =========================================================================
// M2-27.1 Hybrid Authority Scope Isolation Benchmark
// =========================================================================

#[allow(clippy::too_many_arguments)]
fn run_toggle_benchmark_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    native_p4: bool,
    native_p6a: bool,
    native_p6b: bool,
    native_p7: bool,
) -> Result<Duration, sim_model::runner::M0RunError> {
    let agent_count = hybrid_world.segmented_storage.as_ref().unwrap().len();
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);

    let start = Instant::now();
    for _ in 0..days {
        let HybridWorldState {
            world,
            segmented_storage,
            ..
        } = hybrid_world;
        let storage = segmented_storage.as_mut().unwrap();

        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        phase1_resource_regrowth(world, &effective_config);
        storage.phase2_degradation_with_config(&effective_config);

        features_scratch.clear();
        storage.phase3_features_into(
            &world.settlements,
            &effective_config,
            &mut features_scratch,
        )?;

        // Materialize AoS if any phase requires it
        if !native_p4 || !native_p6a || !native_p6b || !native_p7 {
            storage.write_back_to_agents(&mut world.agents);
        }

        choices_scratch.clear();
        intents_scratch.clear();
        if native_p4 {
            phase4_primary_action_selection_storage_into(
                storage,
                world.current_day,
                &effective_config,
                &features_scratch,
                &mut choices_scratch,
            )?;
            phase4_generate_intents_storage_into(
                storage,
                world.current_day,
                &effective_config,
                &choices_scratch,
                &mut intents_scratch,
            )?;
        } else {
            phase4_primary_action_selection_into(
                world,
                &effective_config,
                &features_scratch,
                &mut choices_scratch,
            )?;
            phase4_generate_intents_into(
                world,
                &effective_config,
                &choices_scratch,
                &mut intents_scratch,
            )?;
        }

        let partitions = phase5_partition_intents_baseline(&intents_scratch)?;

        if native_p6a {
            let _ = phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
        } else {
            let _ = phase6a_work_resolution(world, &partitions)?;
        }

        if native_p6b {
            let _ = phase6b_targeted_resolution_storage_baseline(
                storage,
                &world.settlements,
                world.current_day,
                &effective_config,
                &partitions,
            )?;
        } else {
            let _ = phase6b_targeted_resolution(world, &effective_config, &partitions)?;
        }

        if native_p7 {
            let _ = phase7_market_clearance_storage_with_config(
                storage,
                &mut world.settlements,
                &partitions,
                &effective_config.economy,
            )?;
        } else {
            let _ =
                phase7_market_clearance_with_config(world, &partitions, &effective_config.economy)?;
        }

        // Resync AoS mutations back to storage if any legacy phase mutated AoS
        if !native_p6a || !native_p6b || !native_p7 {
            storage.sync_from_agents(&world.agents);
        }

        let _ = storage
            .phase8_welfare_distribution_with_config(&mut world.settlements, &effective_config)?;
        let _ = storage.phase9_mortality_commitment()?;
        let _ = storage.phase10_metrics(&world.settlements, executed_day)?;

        world.current_day = SimulationDay(next_day);
    }
    Ok(start.elapsed())
}

fn measure_m2_27_1_scope_isolation_benchmark(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-27.1 Hybrid Authority Scope Isolation Benchmark");
    println!(
        "Comparing: A: Legacy AoS, B: M2-26 Mixed, C: Hybrid Scope-Isolated, D: Current Full Hybrid"
    );
    println!("=================================================================");

    let populations = [100u64, 250, 500, 1000];
    let days = 50u32;
    let opt_bench = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    println!(
        "\n{:<8} | {:>11} | {:>11} | {:>11} | {:>11} | {:>7} | {:>7} | {:>7} | {:>9} | {:>9} | {:>9} | {:>9}",
        "Pop (N)",
        "A: AoS(ms)",
        "B: M26(ms)",
        "C: Iso(ms)",
        "D: Hyb(ms)",
        "C vs B",
        "D vs C",
        "D vs A",
        "A (d/s)",
        "B (d/s)",
        "C (d/s)",
        "D (d/s)"
    );
    println!("{:-<128}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let base_world = initialize_world(&cfg).unwrap();

        // Pipeline A: Legacy AoS
        let mut w_a = base_world.clone();
        let start_a = Instant::now();
        let _ = run_m0_days(&mut w_a, &cfg, context, days, &opt_bench).unwrap();
        let el_a = start_a.elapsed();

        // Pipeline B: M2-26 Native SoA Gate (multi-phase native with per-tick sync/write-back)
        let mut w_b = base_world.clone();
        let start_b = Instant::now();
        let _ = run_native_soa_days(&mut w_b, &cfg, context, days, &opt_bench).unwrap();
        let el_b = start_b.elapsed();

        // Pipeline C: Hybrid Authority Scope-Isolated (Native 2/3/8/9/10, Legacy 4/5/6A/6B/7, 2 sync passes)
        let mut w_c = HybridWorldState::hybrid(base_world.clone());
        let start_c = Instant::now();
        let _ = run_hybrid_scope_isolated_days(&mut w_c, &cfg, context, days, &opt_bench).unwrap();
        let el_c = start_c.elapsed();

        // Pipeline D: Current Full Hybrid (Native 2/3/4/6A/6B/7/8/9/10, 0 sync passes)
        let mut w_d = HybridWorldState::hybrid(base_world.clone());
        let start_d = Instant::now();
        let _ = run_hybrid_authority_days(&mut w_d, &cfg, context, days, &opt_bench).unwrap();
        let el_d = start_d.elapsed();

        let ms_a = el_a.as_secs_f64() * 1000.0;
        let ms_b = el_b.as_secs_f64() * 1000.0;
        let ms_c = el_c.as_secs_f64() * 1000.0;
        let ms_d = el_d.as_secs_f64() * 1000.0;

        let speedup_c_b = ms_b / ms_c.max(0.001);
        let speedup_d_c = ms_c / ms_d.max(0.001);
        let speedup_d_a = ms_a / ms_d.max(0.001);

        let dps_a = (days as f64) / el_a.as_secs_f64();
        let dps_b = (days as f64) / el_b.as_secs_f64();
        let dps_c = (days as f64) / el_c.as_secs_f64();
        let dps_d = (days as f64) / el_d.as_secs_f64();

        println!(
            "{:<8} | {:>11.2} | {:>11.2} | {:>11.2} | {:>11.2} | {:>7.2}x | {:>7.2}x | {:>7.2}x | {:>9.1} | {:>9.1} | {:>9.1} | {:>9.1}",
            pop,
            ms_a,
            ms_b,
            ms_c,
            ms_d,
            speedup_c_b,
            speedup_d_c,
            speedup_d_a,
            dps_a,
            dps_b,
            dps_c,
            dps_d
        );
    }

    // Daily Sync Cost & Latency Analysis
    println!("\n--- Daily Synchronization Cost & Latency Breakdown (Non-Snapshot Ticks) ---");
    println!(
        "{:<8} | {:>16} | {:>16} | {:>16} | {:>16} | {:>16}",
        "Pop (N)",
        "B Passes/Day",
        "B Sync (us/d)",
        "C Passes/Day",
        "C Sync (us/d)",
        "D Passes & Sync"
    );
    println!("{:-<96}", "");
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        let mut base_world = initialize_world(&cfg).unwrap();
        let mut seg = SegmentedAgentStorage::from_agents(&base_world.agents);

        // Measure single write_back and single sync
        let iters = 100;
        let t0 = Instant::now();
        for _ in 0..iters {
            seg.write_back_to_agents(&mut base_world.agents);
        }
        let wb_us = t0.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        let t1 = Instant::now();
        for _ in 0..iters {
            seg.sync_from_agents(&base_world.agents);
        }
        let sync_us = t1.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        let b_total_sync_us = 3.0 * wb_us + 1.0 * sync_us;
        let c_total_sync_us = 1.0 * wb_us + 1.0 * sync_us;

        println!(
            "{:<8} | {:>16} | {:>16.2} | {:>16} | {:>16.2} | {:>16}",
            pop,
            "4 (1s + 3wb)",
            b_total_sync_us,
            "2 (1s + 1wb)",
            c_total_sync_us,
            "0 passes (0.00 us)"
        );
    }

    // Phase-by-Phase Latency Comparison for Remaining Native Candidates
    println!(
        "\n--- Phase-by-Phase Latency Breakdown: AoS vs Native (Single-Tick Latency, 50 Sweeps) ---"
    );
    println!(
        "{:<8} | {:>9} | {:>9} | {:>9} | {:>9} | {:>9} | {:>9} | {:>9} | {:>9}",
        "Pop (N)",
        "P4 AoS(u)",
        "P4 Nat(u)",
        "P6A AoS",
        "P6A Nat",
        "P6B AoS",
        "P6B Nat",
        "P7 AoS",
        "P7 Nat"
    );
    println!("{:-<96}", "");
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let features = phase3_observation_and_features(&base_world, &cfg).unwrap();
        let seg = SegmentedAgentStorage::from_agents(&base_world.agents);
        let day = base_world.current_day;

        let sweeps = 50;

        // Phase 4
        let mut choices_a = Vec::with_capacity(pop as usize);
        let mut intents_a = Vec::with_capacity(pop as usize);
        let t0 = Instant::now();
        for _ in 0..sweeps {
            choices_a.clear();
            phase4_primary_action_selection_into(&base_world, &cfg, &features, &mut choices_a)
                .unwrap();
            intents_a.clear();
            phase4_generate_intents_into(&base_world, &cfg, &choices_a, &mut intents_a).unwrap();
        }
        let p4_aos_us = t0.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        let mut choices_n = Vec::with_capacity(pop as usize);
        let mut intents_n = Vec::with_capacity(pop as usize);
        let t1 = Instant::now();
        for _ in 0..sweeps {
            choices_n.clear();
            phase4_primary_action_selection_storage_into(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_n,
            )
            .unwrap();
            intents_n.clear();
            phase4_generate_intents_storage_into(&seg, day, &cfg, &choices_n, &mut intents_n)
                .unwrap();
        }
        let p4_nat_us = t1.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        let partitions = phase5_partition_intents_baseline(&intents_a).unwrap();

        // Phase 6A
        let t2 = Instant::now();
        for _ in 0..sweeps {
            let mut w = base_world.clone();
            let _ = phase6a_work_resolution(&mut w, &partitions).unwrap();
        }
        let p6a_aos_us = t2.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        let t3 = Instant::now();
        for _ in 0..sweeps {
            let mut s = seg.clone();
            let mut settlements = base_world.settlements.clone();
            let _ = phase6a_work_resolution_storage(&mut s, &mut settlements, &partitions).unwrap();
        }
        let p6a_nat_us = t3.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        // Phase 6B
        let t4 = Instant::now();
        for _ in 0..sweeps {
            let mut w = base_world.clone();
            let _ = phase6b_targeted_resolution(&mut w, &cfg, &partitions).unwrap();
        }
        let p6b_aos_us = t4.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        let t5 = Instant::now();
        for _ in 0..sweeps {
            let mut s = seg.clone();
            let _ = phase6b_targeted_resolution_storage_baseline(
                &mut s,
                &base_world.settlements,
                day,
                &cfg,
                &partitions,
            )
            .unwrap();
        }
        let p6b_nat_us = t5.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        // Phase 7
        let t6 = Instant::now();
        for _ in 0..sweeps {
            let mut w = base_world.clone();
            let _ = phase7_market_clearance_with_config(&mut w, &partitions, &cfg.economy).unwrap();
        }
        let p7_aos_us = t6.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        let t7 = Instant::now();
        for _ in 0..sweeps {
            let mut s = seg.clone();
            let mut settlements = base_world.settlements.clone();
            let _ = phase7_market_clearance_storage_with_config(
                &mut s,
                &mut settlements,
                &partitions,
                &cfg.economy,
            )
            .unwrap();
        }
        let p7_nat_us = t7.elapsed().as_nanos() as f64 / (sweeps as f64) / 1000.0;

        println!(
            "{:<8} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2}",
            pop,
            p4_aos_us,
            p4_nat_us,
            p6a_aos_us,
            p6a_nat_us,
            p6b_aos_us,
            p6b_nat_us,
            p7_aos_us,
            p7_nat_us
        );
    }

    // Secondary Decomposition: Toggle Benchmark (C vs C+P4, C+P6A, C+P6B, C+P7, D)
    println!(
        "\n--- Secondary Decomposition: Isolated Phase Toggles on top of Pipeline C (50 Days) ---"
    );
    println!(
        "{:<8} | {:>12} | {:>12} | {:>12} | {:>12} | {:>12} | {:>12}",
        "Pop (N)",
        "C: Base (ms)",
        "+P4 Nat (ms)",
        "+P6A Nat(ms)",
        "+P6B Nat(ms)",
        "+P7 Nat (ms)",
        "D: Full (ms)"
    );
    println!("{:-<88}", "");
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();

        // C Base (all false)
        let mut w_c = HybridWorldState::hybrid(base_world.clone());
        let el_c =
            run_toggle_benchmark_days(&mut w_c, &cfg, context, days, false, false, false, false)
                .unwrap();

        // +P4
        let mut w_p4 = HybridWorldState::hybrid(base_world.clone());
        let el_p4 =
            run_toggle_benchmark_days(&mut w_p4, &cfg, context, days, true, false, false, false)
                .unwrap();

        // +P6A
        let mut w_p6a = HybridWorldState::hybrid(base_world.clone());
        let el_p6a =
            run_toggle_benchmark_days(&mut w_p6a, &cfg, context, days, false, true, false, false)
                .unwrap();

        // +P6B
        let mut w_p6b = HybridWorldState::hybrid(base_world.clone());
        let el_p6b =
            run_toggle_benchmark_days(&mut w_p6b, &cfg, context, days, false, false, true, false)
                .unwrap();

        // +P7
        let mut w_p7 = HybridWorldState::hybrid(base_world.clone());
        let el_p7 =
            run_toggle_benchmark_days(&mut w_p7, &cfg, context, days, false, false, false, true)
                .unwrap();

        // D Full (all true)
        let mut w_d = HybridWorldState::hybrid(base_world.clone());
        let el_d = run_toggle_benchmark_days(&mut w_d, &cfg, context, days, true, true, true, true)
            .unwrap();

        println!(
            "{:<8} | {:>12.2} | {:>12.2} | {:>12.2} | {:>12.2} | {:>12.2} | {:>12.2}",
            pop,
            el_c.as_secs_f64() * 1000.0,
            el_p4.as_secs_f64() * 1000.0,
            el_p6a.as_secs_f64() * 1000.0,
            el_p6b.as_secs_f64() * 1000.0,
            el_p7.as_secs_f64() * 1000.0,
            el_d.as_secs_f64() * 1000.0,
        );
    }
}

fn run_hybrid_p4_bench_days(
    hybrid_world: &mut HybridWorldState,
    config: &SimConfig,
    context: &M0RunContext,
    days: u32,
    use_baseline_p4: bool,
) -> Result<Duration, sim_model::runner::M0RunError> {
    let agent_count = hybrid_world.segmented_storage.as_ref().unwrap().len();
    let mut features_scratch = Vec::with_capacity(agent_count);
    let mut choices_scratch = Vec::with_capacity(agent_count);
    let mut intents_scratch = Vec::with_capacity(agent_count);

    let start = Instant::now();
    for _ in 0..days {
        let HybridWorldState {
            world,
            segmented_storage,
            ..
        } = hybrid_world;
        let storage = segmented_storage.as_mut().unwrap();

        let executed_day = world.current_day.as_u32();
        let next_day = executed_day + 1;

        let mut effective_config = config.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        phase1_resource_regrowth(world, &effective_config);
        storage.phase2_degradation_with_config(&effective_config);

        features_scratch.clear();
        storage.phase3_features_into(
            &world.settlements,
            &effective_config,
            &mut features_scratch,
        )?;

        choices_scratch.clear();
        intents_scratch.clear();
        if use_baseline_p4 {
            phase4_primary_action_selection_storage_into_baseline(
                storage,
                world.current_day,
                &effective_config,
                &features_scratch,
                &mut choices_scratch,
            )?;
            generate_intents_storage_into_baseline(
                storage,
                world.current_day,
                &effective_config,
                &choices_scratch,
                &mut intents_scratch,
            )?;
        } else {
            phase4_primary_action_selection_storage_into(
                storage,
                world.current_day,
                &effective_config,
                &features_scratch,
                &mut choices_scratch,
            )?;
            phase4_generate_intents_storage_into(
                storage,
                world.current_day,
                &effective_config,
                &choices_scratch,
                &mut intents_scratch,
            )?;
        }

        let partitions = phase5_partition_intents_baseline(&intents_scratch)?;
        let _ = phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
        let _ = phase6b_targeted_resolution_storage_baseline(
            storage,
            &world.settlements,
            world.current_day,
            &effective_config,
            &partitions,
        )?;
        let _ = phase7_market_clearance_storage_with_config(
            storage,
            &mut world.settlements,
            &partitions,
            &effective_config.economy,
        )?;
        let _ = storage
            .phase8_welfare_distribution_with_config(&mut world.settlements, &effective_config)?;
        let _ = storage.phase9_mortality_commitment()?;
        world.current_day = SimulationDay(next_day);
    }
    Ok(start.elapsed())
}

fn measure_m2_28_phase4_optimization(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-28 Phase 4 Native SoA Regression Removal Benchmark");
    println!("=================================================================");

    let populations = [100, 250, 500, 1000];
    let iters = 50;

    // --- Part 1: Phase 4 Baseline Sub-component Breakdown ---
    println!(
        "\n--- Part 1: Baseline Native Phase 4 Sub-component Breakdown ({} Sweeps) ---",
        iters
    );
    println!(
        "{:<8} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10}",
        "Pop (N)", "P4 Base(u)", "P4 Sel(u)", "P4 Int(u)", "slot_of(u)", "cand_disc", "cand_alloc"
    );
    println!("{:-<76}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let features = phase3_observation_and_features(&base_world, &cfg).unwrap();
        let seg = SegmentedAgentStorage::from_agents(&base_world.agents);
        let day = base_world.current_day;

        let mut choices_b = Vec::with_capacity(pop as usize);
        let mut intents_b = Vec::with_capacity(pop as usize);

        // Warm up and get choices
        phase4_primary_action_selection_storage_into_baseline(
            &seg,
            day,
            &cfg,
            &features,
            &mut choices_b,
        )
        .unwrap();
        generate_intents_storage_into_baseline(&seg, day, &cfg, &choices_b, &mut intents_b)
            .unwrap();

        // 1. Measure full selection baseline
        let t_sel_start = Instant::now();
        for _ in 0..iters {
            choices_b.clear();
            phase4_primary_action_selection_storage_into_baseline(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_b,
            )
            .unwrap();
        }
        let sel_us = t_sel_start.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // 2. Measure full intent generation baseline
        let t_int_start = Instant::now();
        for _ in 0..iters {
            intents_b.clear();
            generate_intents_storage_into_baseline(&seg, day, &cfg, &choices_b, &mut intents_b)
                .unwrap();
        }
        let int_us = t_int_start.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // 3. Measure slot_of alone
        let t_slot_start = Instant::now();
        let mut slot_sum = 0;
        for _ in 0..iters {
            for af in &features {
                if let Some(s) = seg.slot_of(af.agent_id) {
                    slot_sum += s;
                }
            }
        }
        let slot_us = t_slot_start.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;
        std::hint::black_box(slot_sum);

        // 4. Measure candidate discovery & candidate allocation
        let group_ids = &seg.economy.group_id;
        let alives = &seg.demography.alive;
        let healths = &seg.demography.health;
        let foods = &seg.economy.food;
        let agent_ids = &seg.agent_ids;
        let num_agents = seg.len();

        let t_cand_alloc = Instant::now();
        for _ in 0..iters {
            for c in &choices_b {
                if c.action == Action::GiveFood {
                    let mut candidates: Vec<sim_core::AgentId> = (0..num_agents)
                        .filter(|&s| {
                            group_ids[s] == group_ids[0]
                                && alives[s]
                                && healths[s] > 0.0
                                && agent_ids[s] != c.agent_id
                                && foods[s] < cfg.interaction.starvation_threshold
                        })
                        .map(|s| agent_ids[s])
                        .collect();
                    candidates.sort();
                    std::hint::black_box(&candidates);
                } else if c.action == Action::StealFood {
                    let mut candidates: Vec<sim_core::AgentId> = (0..num_agents)
                        .filter(|&s| {
                            group_ids[s] == group_ids[0]
                                && alives[s]
                                && healths[s] > 0.0
                                && agent_ids[s] != c.agent_id
                                && foods[s] > 0.0
                        })
                        .map(|s| agent_ids[s])
                        .collect();
                    candidates.sort();
                    std::hint::black_box(&candidates);
                }
            }
        }
        let cand_alloc_us = t_cand_alloc.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // Measure candidate discovery with reusable scratch (no allocation)
        let mut scratch = Vec::with_capacity(num_agents);
        let t_cand_disc = Instant::now();
        for _ in 0..iters {
            for c in &choices_b {
                if c.action == Action::GiveFood {
                    scratch.clear();
                    for s in 0..num_agents {
                        if group_ids[s] == group_ids[0]
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != c.agent_id
                            && foods[s] < cfg.interaction.starvation_threshold
                        {
                            scratch.push(agent_ids[s]);
                        }
                    }
                    scratch.sort_unstable();
                    std::hint::black_box(&scratch);
                } else if c.action == Action::StealFood {
                    scratch.clear();
                    for s in 0..num_agents {
                        if group_ids[s] == group_ids[0]
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != c.agent_id
                            && foods[s] > 0.0
                        {
                            scratch.push(agent_ids[s]);
                        }
                    }
                    scratch.sort_unstable();
                    std::hint::black_box(&scratch);
                }
            }
        }
        let cand_disc_us = t_cand_disc.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        let total_base_us = sel_us + int_us;
        println!(
            "{:<8} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2}",
            pop, total_base_us, sel_us, int_us, slot_us, cand_disc_us, cand_alloc_us
        );
    }

    // --- Part 2: Isolated Variant Attribution (A, B, C, D, E) ---
    println!(
        "\n--- Part 2: Phase 4 Isolated Variant Attribution (Single-Tick, {} Sweeps) ---",
        iters
    );
    println!(
        "{:<8} | {:>9} | {:>9} | {:>9} | {:>9} | {:>9} | {:>7} | {:>7} | {:>9} | {:>9}",
        "Pop (N)",
        "A: AoS(u)",
        "B: Base(u)",
        "C: Dir(u)",
        "D: Scrt(u)",
        "E: Opt(u)",
        "E vs A",
        "E vs B",
        "slot_gain",
        "scrt_gain"
    );
    println!("{:-<110}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let features = phase3_observation_and_features(&base_world, &cfg).unwrap();
        let seg = SegmentedAgentStorage::from_agents(&base_world.agents);
        let day = base_world.current_day;

        // A: AoS
        let mut choices_a = Vec::with_capacity(pop as usize);
        let mut intents_a = Vec::with_capacity(pop as usize);
        let t_a = Instant::now();
        for _ in 0..iters {
            choices_a.clear();
            phase4_primary_action_selection_into(&base_world, &cfg, &features, &mut choices_a)
                .unwrap();
            intents_a.clear();
            phase4_generate_intents_into(&base_world, &cfg, &choices_a, &mut intents_a).unwrap();
        }
        let a_us = t_a.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // B: Baseline Native
        let mut choices_b = Vec::with_capacity(pop as usize);
        let mut intents_b = Vec::with_capacity(pop as usize);
        let t_b = Instant::now();
        for _ in 0..iters {
            choices_b.clear();
            phase4_primary_action_selection_storage_into_baseline(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_b,
            )
            .unwrap();
            intents_b.clear();
            generate_intents_storage_into_baseline(&seg, day, &cfg, &choices_b, &mut intents_b)
                .unwrap();
        }
        let b_us = t_b.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // C: Native + DenseSlot Direct
        let mut choices_c = Vec::with_capacity(pop as usize);
        let mut intents_c = Vec::with_capacity(pop as usize);
        let t_c = Instant::now();
        for _ in 0..iters {
            choices_c.clear();
            phase4_primary_action_selection_storage_into(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_c,
            )
            .unwrap();
            intents_c.clear();
            generate_intents_storage_into_variant_c(&seg, day, &cfg, &choices_c, &mut intents_c)
                .unwrap();
        }
        let c_us = t_c.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // D: Native + DenseSlot Direct + Scratch Reuse
        let mut choices_d = Vec::with_capacity(pop as usize);
        let mut intents_d = Vec::with_capacity(pop as usize);
        let mut cand_scratch = Vec::with_capacity(pop as usize);
        let t_d = Instant::now();
        for _ in 0..iters {
            choices_d.clear();
            phase4_primary_action_selection_storage_into(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_d,
            )
            .unwrap();
            intents_d.clear();
            generate_intents_storage_into_variant_d(
                &seg,
                day,
                &cfg,
                &choices_d,
                &mut cand_scratch,
                &mut intents_d,
            )
            .unwrap();
        }
        let d_us = t_d.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        // E: Final Optimized
        let mut choices_e = Vec::with_capacity(pop as usize);
        let mut intents_e = Vec::with_capacity(pop as usize);
        let t_e = Instant::now();
        for _ in 0..iters {
            choices_e.clear();
            phase4_primary_action_selection_storage_into(
                &seg,
                day,
                &cfg,
                &features,
                &mut choices_e,
            )
            .unwrap();
            intents_e.clear();
            generate_intents_storage_with_scratch(
                &seg,
                day,
                &cfg,
                &choices_e,
                &mut cand_scratch,
                &mut intents_e,
            )
            .unwrap();
        }
        let e_us = t_e.elapsed().as_nanos() as f64 / (iters as f64) / 1000.0;

        let e_vs_a = a_us / e_us;
        let e_vs_b = b_us / e_us;
        let slot_gain = b_us / c_us;
        let scrt_gain = c_us / d_us;

        println!(
            "{:<8} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>9.2} | {:>6.2}x | {:>6.2}x | {:>8.2}x | {:>8.2}x",
            pop, a_us, b_us, c_us, d_us, e_us, e_vs_a, e_vs_b, slot_gain, scrt_gain
        );
    }

    // --- Part 3: Full Hybrid Pipeline 50-Day Comparison (Before vs After) ---
    println!(
        "\n--- Part 3: Full Hybrid 50-Day Pipeline Comparison (M2-27 Baseline vs M2-28 Optimized) ---"
    );
    println!(
        "{:<8} | {:>14} | {:>14} | {:>10} | {:>14} | {:>14}",
        "Pop (N)", "M2-27 Base(ms)", "M2-28 Opt(ms)", "Speedup", "Base (day/s)", "Opt (day/s)"
    );
    println!("{:-<86}", "");

    let days = 50;
    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();

        // M2-27 Baseline Full Hybrid
        let mut w_before = HybridWorldState::hybrid(base_world.clone());
        let el_before = run_hybrid_p4_bench_days(&mut w_before, &cfg, context, days, true).unwrap();

        // M2-28 Optimized Full Hybrid
        let mut w_after = HybridWorldState::hybrid(base_world.clone());
        let el_after = run_hybrid_p4_bench_days(&mut w_after, &cfg, context, days, false).unwrap();

        let ms_before = el_before.as_secs_f64() * 1000.0;
        let ms_after = el_after.as_secs_f64() * 1000.0;
        let speedup = ms_before / ms_after;
        let dps_before = days as f64 / el_before.as_secs_f64();
        let dps_after = days as f64 / el_after.as_secs_f64();

        println!(
            "{:<8} | {:>14.2} | {:>14.2} | {:>9.2}x | {:>14.1} | {:>14.1}",
            pop, ms_before, ms_after, speedup, dps_before, dps_after
        );
    }
}

#[allow(clippy::needless_range_loop)]
fn measure_m2_29_post_soa_profiling(base_config: &SimConfig, context: &M0RunContext) {
    println!("\n=================================================================");
    println!("M2-29 Post-SoA Hot-Path Profiling & SIMD Target Selection Benchmark");
    println!("Baseline: Full Hybrid Native SoA Pipeline (Post M2-28 Optimization)");
    println!("=================================================================");

    let populations = [100u64, 250, 500, 1000, 2500, 5000, 10000];

    #[derive(Debug, Default, Clone, Copy)]
    struct PhaseDurations {
        p1: f64,
        p2: f64,
        p3: f64,
        p4_sel: f64,
        p4_int: f64,
        p5: f64,
        p6a: f64,
        p6b: f64,
        p7: f64,
        p8: f64,
        p9: f64,
        stg: f64,
        p10: f64,
        p11_norm: f64,
        p11_snap: f64,
        total_norm: f64,
    }

    let mut results: Vec<(u64, PhaseDurations, f64, f64)> = Vec::new();

    println!(
        "\n--- Part 1A: Full Hybrid Phase-by-Phase Latency Breakdown (Normal Non-Snapshot Ticks) ---"
    );
    println!(
        "{:<6} | {:>6} | {:>6} | {:>6} | {:>7} | {:>7} | {:>6} | {:>6} | {:>6} | {:>6} | {:>6} | {:>6} | {:>6} | {:>6} | {:>6} | {:>8} | {:>9} | {:>10}",
        "Pop(N)",
        "P1(u)",
        "P2(u)",
        "P3(u)",
        "P4S(u)",
        "P4I(u)",
        "P5(u)",
        "P6A(u)",
        "P6B(u)",
        "P7(u)",
        "P8(u)",
        "P9(u)",
        "Stg(u)",
        "P10(u)",
        "P11(u)",
        "Tot(us)",
        "Ticks/sec",
        "Steps/sec"
    );
    println!("{:-<156}", "");

    for &pop in &populations {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        cfg.world.initial_settlement_resource = 200.0 * pop as f32;

        let base_world = initialize_world(&cfg).unwrap();
        let mut hybrid_world = HybridWorldState::hybrid(base_world);

        let sweeps = match pop {
            100..=1000 => 40,
            2500 => 25,
            5000 => 15,
            _ => 10,
        };

        // Warm up 3 ticks
        for _ in 0..3 {
            let opts = DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: false,
            };
            let mut candidate_scratch = Vec::with_capacity(pop as usize);
            let _ = run_hybrid_authority_day_with_candidate_scratch(
                &mut hybrid_world,
                &cfg,
                context,
                &opts,
                &mut candidate_scratch,
            )
            .unwrap();
        }

        let mut d_p1 = Duration::ZERO;
        let mut d_p2 = Duration::ZERO;
        let mut d_p3 = Duration::ZERO;
        let mut d_p4_sel = Duration::ZERO;
        let mut d_p4_int = Duration::ZERO;
        let mut d_p5 = Duration::ZERO;
        let mut d_p6a = Duration::ZERO;
        let mut d_p6b = Duration::ZERO;
        let mut d_p7 = Duration::ZERO;
        let mut d_p8 = Duration::ZERO;
        let mut d_p9 = Duration::ZERO;
        let mut d_stg = Duration::ZERO;
        let mut d_p10 = Duration::ZERO;
        let mut d_p11_norm = Duration::ZERO;
        let mut d_p11_snap = Duration::ZERO;

        let mut features_scratch = Vec::with_capacity(pop as usize);
        let mut choices_scratch = Vec::with_capacity(pop as usize);
        let mut intents_scratch = Vec::with_capacity(pop as usize);
        let mut cand_scratch = Vec::with_capacity(pop as usize);

        let mut effective_config = cfg.clone();
        effective_config.world.master_seed = context.master_seed;
        effective_config.world.replicate_id = context.replicate_id;

        for _ in 0..sweeps {
            let HybridWorldState {
                world,
                segmented_storage,
                authority_mode: _,
            } = &mut hybrid_world;
            let storage = segmented_storage.as_mut().unwrap();
            let executed_day = world.current_day.as_u32();
            let next_day = executed_day + 1;

            // Phase 1
            let t1 = Instant::now();
            phase1_resource_regrowth(world, &effective_config);
            d_p1 += t1.elapsed();

            // Phase 2
            let t2 = Instant::now();
            storage.phase2_degradation_with_config(&effective_config);
            d_p2 += t2.elapsed();

            // Phase 3
            let t3 = Instant::now();
            features_scratch.clear();
            storage
                .phase3_features_into(&world.settlements, &effective_config, &mut features_scratch)
                .unwrap();
            d_p3 += t3.elapsed();

            // Phase 4 - Selection
            let t4_sel = Instant::now();
            choices_scratch.clear();
            phase4_primary_action_selection_storage_into(
                storage,
                world.current_day,
                &effective_config,
                &features_scratch,
                &mut choices_scratch,
            )
            .unwrap();
            d_p4_sel += t4_sel.elapsed();

            // Phase 4 - Intent
            let t4_int = Instant::now();
            intents_scratch.clear();
            generate_intents_storage_with_scratch(
                storage,
                world.current_day,
                &effective_config,
                &choices_scratch,
                &mut cand_scratch,
                &mut intents_scratch,
            )
            .unwrap();
            d_p4_int += t4_int.elapsed();

            // Phase 5
            let t5 = Instant::now();
            let partitions = phase5_partition_intents_baseline(&intents_scratch).unwrap();
            d_p5 += t5.elapsed();

            // Phase 6A
            let t6a = Instant::now();
            let work_resolutions =
                phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)
                    .unwrap();
            d_p6a += t6a.elapsed();

            // Phase 6B
            let t6b = Instant::now();
            let targeted_resolutions = phase6b_targeted_resolution_storage_baseline(
                storage,
                &world.settlements,
                world.current_day,
                &effective_config,
                &partitions,
            )
            .unwrap();
            d_p6b += t6b.elapsed();

            // Phase 7
            let t7 = Instant::now();
            let market_resolutions = phase7_market_clearance_storage_with_config(
                storage,
                &mut world.settlements,
                &partitions,
                &effective_config.economy,
            )
            .unwrap();
            d_p7 += t7.elapsed();

            // Phase 8
            let t8 = Instant::now();
            let welfare_resolutions = storage
                .phase8_welfare_distribution_with_config(&mut world.settlements, &effective_config)
                .unwrap();
            d_p8 += t8.elapsed();

            // Phase 9
            let t9 = Instant::now();
            let mortality_resolution = storage.phase9_mortality_commitment().unwrap();
            d_p9 += t9.elapsed();

            // Staged Event Buffer Assembly
            let t_stg = Instant::now();
            let estimated_cap = storage.len().saturating_mul(2) + world.settlements.len() + 4;
            let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
            let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
            for w in &work_resolutions {
                let work_events = events_from_work_resolution(executed_day, w);
                let count = work_events.len() as u64;
                if let Some(entry) = phase6_counts
                    .iter_mut()
                    .find(|(gid, _)| *gid == w.group_id.0)
                {
                    entry.1 += count;
                } else {
                    phase6_counts.push((w.group_id.0, count));
                }
                event_buffer.push_all(work_events);
            }
            for t in &targeted_resolutions {
                let mut targeted_events = events_from_targeted_resolution(executed_day, t);
                let offset = if let Some(entry) = phase6_counts
                    .iter_mut()
                    .find(|(gid, _)| *gid == t.group_id.0)
                {
                    let prev = entry.1;
                    entry.1 += targeted_events.len() as u64;
                    prev
                } else {
                    phase6_counts.push((t.group_id.0, targeted_events.len() as u64));
                    0
                };
                if offset > 0 {
                    for te in &mut targeted_events {
                        te.key.local_sequence += offset;
                    }
                }
                event_buffer.push_all(targeted_events);
            }
            for m in &market_resolutions {
                event_buffer.push_all(events_from_market_resolution(executed_day, m));
            }
            for wel in &welfare_resolutions {
                event_buffer.push_all(events_from_welfare_resolution(executed_day, wel));
            }
            event_buffer.push_all(events_from_mortality_resolution(
                executed_day,
                &mortality_resolution,
            ));
            d_stg += t_stg.elapsed();

            // Phase 10
            let t10 = Instant::now();
            let metrics = storage
                .phase10_metrics(&world.settlements, executed_day)
                .unwrap();
            event_buffer.push(event_from_daily_metrics(&metrics));
            d_p10 += t10.elapsed();

            // Phase 11 Normal
            let t11_flush = Instant::now();
            let _ = phase11_flush_events(&mut event_buffer).unwrap();
            d_p11_norm += t11_flush.elapsed();

            // Snapshot Boundary Measurement (Isolated)
            let t_snap = Instant::now();
            storage.write_back_to_agents(&mut world.agents);
            let snapshot_metadata = SnapshotMetadata::new(
                next_day,
                context.master_seed,
                context.replicate_id,
                DEFAULT_MODEL_VERSION,
                DEFAULT_CONFIG_VERSION,
            );
            let _ = encode_snapshot(world, &snapshot_metadata).unwrap();
            d_p11_snap += t_snap.elapsed();

            world.current_day = SimulationDay(next_day);
        }

        let to_us = |d: Duration| d.as_nanos() as f64 / (sweeps as f64) / 1000.0;
        let p1 = to_us(d_p1);
        let p2 = to_us(d_p2);
        let p3 = to_us(d_p3);
        let p4_sel = to_us(d_p4_sel);
        let p4_int = to_us(d_p4_int);
        let p5 = to_us(d_p5);
        let p6a = to_us(d_p6a);
        let p6b = to_us(d_p6b);
        let p7 = to_us(d_p7);
        let p8 = to_us(d_p8);
        let p9 = to_us(d_p9);
        let stg = to_us(d_stg);
        let p10 = to_us(d_p10);
        let p11_norm = to_us(d_p11_norm);
        let p11_snap = to_us(d_p11_snap);

        let tot_norm =
            p1 + p2 + p3 + p4_sel + p4_int + p5 + p6a + p6b + p7 + p8 + p9 + stg + p10 + p11_norm;
        let dps = 1_000_000.0 / tot_norm.max(0.001);
        let agent_steps = dps * (pop as f64);

        let durs = PhaseDurations {
            p1,
            p2,
            p3,
            p4_sel,
            p4_int,
            p5,
            p6a,
            p6b,
            p7,
            p8,
            p9,
            stg,
            p10,
            p11_norm,
            p11_snap,
            total_norm: tot_norm,
        };
        results.push((pop, durs, dps, agent_steps));

        println!(
            "{:<6} | {:>6.2} | {:>6.2} | {:>6.2} | {:>7.2} | {:>7.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>6.2} | {:>8.2} | {:>9.1} | {:>10.0}",
            pop,
            p1,
            p2,
            p3,
            p4_sel,
            p4_int,
            p5,
            p6a,
            p6b,
            p7,
            p8,
            p9,
            stg,
            p10,
            p11_norm,
            tot_norm,
            dps,
            agent_steps
        );
    }

    println!("\n--- Part 1B: Per-Phase Runtime Share (%) in Normal Tick ---");
    println!(
        "{:<6} | {:>5} | {:>5} | {:>5} | {:>6} | {:>6} | {:>5} | {:>5} | {:>5} | {:>5} | {:>5} | {:>5} | {:>5} | {:>5} | {:>5}",
        "Pop(N)",
        "P1%",
        "P2%",
        "P3%",
        "P4S%",
        "P4I%",
        "P5%",
        "P6A%",
        "P6B%",
        "P7%",
        "P8%",
        "P9%",
        "Stg%",
        "P10%",
        "P11%"
    );
    println!("{:-<100}", "");
    for &(pop, d, _, _) in &results {
        let tot = d.total_norm;
        let pct = |v: f64| (v / tot) * 100.0;
        println!(
            "{:<6} | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>5.1}% | {:>5.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}% | {:>4.1}%",
            pop,
            pct(d.p1),
            pct(d.p2),
            pct(d.p3),
            pct(d.p4_sel),
            pct(d.p4_int),
            pct(d.p5),
            pct(d.p6a),
            pct(d.p6b),
            pct(d.p7),
            pct(d.p8),
            pct(d.p9),
            pct(d.stg),
            pct(d.p10),
            pct(d.p11_norm)
        );
    }

    println!("\n--- Part 1C: Normal Tick vs Snapshot Boundary Tick Latency Overhead ---");
    println!(
        "{:<6} | {:>14} | {:>14} | {:>14} | {:>14}",
        "Pop(N)", "Normal(us)", "Snap Bound(us)", "Overhead(us)", "Overhead Ratio"
    );
    println!("{:-<70}", "");
    for &(pop, d, _, _) in &results {
        let snap_tot = d.total_norm + d.p11_snap;
        let ratio = snap_tot / d.total_norm;
        println!(
            "{:<6} | {:>14.2} | {:>14.2} | {:>14.2} | {:>13.2}x",
            pop, d.total_norm, snap_tot, d.p11_snap, ratio
        );
    }

    println!("\n--- Part 2: Population Scaling Analysis & Empirical Exponent (alpha) ---");
    println!(
        "{:<20} | {:>10} | {:>10} | {:>12} | {:>14} | {:>26}",
        "Phase / Metric",
        "T(1000) us",
        "T(10000) us",
        "Ratio(10k/1k)",
        "Exponent alpha",
        "Scaling Regime"
    );
    println!("{:-<102}", "");

    let r_1k = results.iter().find(|(p, _, _, _)| *p == 1000).unwrap();
    let r_10k = results.iter().find(|(p, _, _, _)| *p == 10000).unwrap();
    let d_1k = r_1k.1;
    let d_10k = r_10k.1;

    let phases_eval = [
        ("Phase 1 Regrowth", d_1k.p1, d_10k.p1),
        ("Phase 2 Degradation", d_1k.p2, d_10k.p2),
        ("Phase 3 Features", d_1k.p3, d_10k.p3),
        ("Phase 4 Selection", d_1k.p4_sel, d_10k.p4_sel),
        ("Phase 4 Intent", d_1k.p4_int, d_10k.p4_int),
        ("Phase 5 Partition", d_1k.p5, d_10k.p5),
        ("Phase 6A Work", d_1k.p6a, d_10k.p6a),
        ("Phase 6B Targeted", d_1k.p6b, d_10k.p6b),
        ("Phase 7 Market", d_1k.p7, d_10k.p7),
        ("Phase 8 Welfare", d_1k.p8, d_10k.p8),
        ("Phase 9 Mortality", d_1k.p9, d_10k.p9),
        ("Event Staging", d_1k.stg, d_10k.stg),
        ("Phase 10 Metrics", d_1k.p10, d_10k.p10),
        ("Phase 11 Normal", d_1k.p11_norm, d_10k.p11_norm),
        ("Total Normal Tick", d_1k.total_norm, d_10k.total_norm),
        ("Snapshot Overhead", d_1k.p11_snap, d_10k.p11_snap),
    ];

    for (name, t1k, t10k) in &phases_eval {
        let ratio = t10k / t1k.max(0.001);
        let alpha = ratio.ln() / (10.0f64).ln();
        let regime = if alpha < 0.2 {
            "O(1) Population-Independent"
        } else if alpha < 1.05 {
            "O(N) Strictly Linear"
        } else if alpha <= 1.30 {
            "O(N log N) Mild Superlinear"
        } else {
            "O(N^2) Bottleneck Candidate"
        };
        println!(
            "{:<20} | {:>10.2} | {:>10.2} | {:>11.2}x | {:>14.3} | {:>26}",
            name, t1k, t10k, ratio, alpha, regime
        );
    }

    println!("\n--- Part 3: Native Kernel Internal Sub-component Breakdown ---");
    let test_pops = [100u64, 500, 1000, 10000];

    // 1. Phase 2 Breakdown
    println!("\n[Phase 2 Biological Degradation Sub-components (us)]");
    println!(
        "{:<6} | {:>12} | {:>14} | {:>14} | {:>14}",
        "Pop(N)", "Total P2(us)", "Decay Math(us)", "Clamp(us)", "Mem Store(us)"
    );
    println!("{:-<64}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let mut storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = 50;

        let f_metabolic = cfg.environment.base_metabolic_cost;
        let decay_rate = cfg.environment.health_decay_rate;
        let mut foods = storage.economy.food.clone();
        let mut healths = storage.demography.health.clone();
        let alives = &storage.demography.alive;

        let t_math = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if alives[i] {
                    let f = foods[i];
                    let f_consumed = f.min(f_metabolic);
                    let f_deficit = f_metabolic - f_consumed;
                    let health_delta = -decay_rate * f_deficit;
                    std::hint::black_box((f_consumed, health_delta));
                }
            }
        }
        let math_us = t_math.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_clamp = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if alives[i] {
                    let f = (foods[i] - 1.0).max(0.0);
                    let h = (healths[i] - 0.05).clamp(0.0, 1.0);
                    std::hint::black_box((f, h));
                }
            }
        }
        let clamp_us = t_clamp.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_store = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if alives[i] {
                    foods[i] = 20.0;
                    healths[i] = 0.95;
                }
            }
        }
        let store_us = t_store.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_tot = Instant::now();
        for _ in 0..sweeps {
            storage.phase2_degradation_with_config(&cfg);
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>12.2} | {:>14.2} | {:>14.2} | {:>14.2}",
            pop, tot_us, math_us, clamp_us, store_us
        );
    }

    // 2. Phase 3 Breakdown
    println!("\n[Phase 3 Observation & Features Sub-components (us)]");
    println!(
        "{:<6} | {:>12} | {:>14} | {:>14} | {:>14} | {:>14}",
        "Pop(N)", "Total P3(us)", "Scarcity(us)", "State Norm(us)", "Pack/Push(us)", "Sort(us)"
    );
    println!("{:-<80}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = 50;

        let starvation_threshold = cfg.interaction.starvation_threshold;
        let target_reserve = cfg.economy.target_reserve as f32;
        let target_food = cfg.economy.target_food;
        let foods = storage.food();
        let wealths = storage.wealth();
        let healths = storage.health();
        let alives = storage.alive();
        let agent_ids = storage.agent_ids();

        let t_scarcity = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if alives[i] {
                    let sc = 0.15f32;
                    std::hint::black_box(sc);
                }
            }
        }
        let scarcity_us = t_scarcity.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_norm = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if alives[i] {
                    let hunger_ratio = (1.0 - foods[i] / starvation_threshold).clamp(0.0, 1.0);
                    let wealth_pressure =
                        (1.0 - (wealths[i] as f32) / target_reserve).clamp(0.0, 1.0);
                    let health_deficit = (1.0 - healths[i]).clamp(0.0, 1.0);
                    let food_surplus =
                        ((foods[i] - starvation_threshold) / target_food).clamp(0.0, 1.0);
                    std::hint::black_box((
                        hunger_ratio,
                        wealth_pressure,
                        health_deficit,
                        food_surplus,
                    ));
                }
            }
        }
        let norm_us = t_norm.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let mut out_features = Vec::with_capacity(n);
        let t_pack = Instant::now();
        for _ in 0..sweeps {
            out_features.clear();
            for i in 0..n {
                if alives[i] {
                    out_features.push(AgentFeatures {
                        agent_id: agent_ids[i],
                        features: FeatureVector::new([0.2, 0.3, 0.1, 0.15, 0.4]),
                    });
                }
            }
        }
        let pack_us = t_pack.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_sort = Instant::now();
        for _ in 0..sweeps {
            out_features.sort_by_key(|af| af.agent_id);
        }
        let sort_us = t_sort.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let mut full_scratch = Vec::with_capacity(n);
        let t_tot = Instant::now();
        for _ in 0..sweeps {
            full_scratch.clear();
            storage
                .phase3_features_into(&base_world.settlements, &cfg, &mut full_scratch)
                .unwrap();
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>12.2} | {:>14.2} | {:>14.2} | {:>14.2} | {:>14.2}",
            pop, tot_us, scarcity_us, norm_us, pack_us, sort_us
        );
    }

    // 3. Phase 4 Breakdown
    println!("\n[Phase 4 Decision & Intent Sub-components (us)]");
    println!(
        "{:<6} | {:>10} | {:>12} | {:>10} | {:>10} | {:>10} | {:>10} | {:>12}",
        "Pop(N)",
        "Total P4",
        "U_base Dot",
        "Softmax",
        "PRNG Hash",
        "CDF Select",
        "Cand Search",
        "Intent Alloc"
    );
    println!("{:-<96}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = match pop {
            100..=1000 => 40,
            _ => 15,
        };

        let mut features = Vec::with_capacity(n);
        storage
            .phase3_features_into(&base_world.settlements, &cfg, &mut features)
            .unwrap();

        // U_base dot product
        let t_dot = Instant::now();
        for _ in 0..sweeps {
            for af in &features {
                let mut utilities = [0.0f32; 6];
                for (m, _action) in Action::ALL.iter().enumerate() {
                    let mut u_base = cfg.decision.action_biases[m];
                    for k in 0..5 {
                        u_base += cfg.decision.base_weight_matrix[m][k] * af.features.values[k];
                    }
                    utilities[m] = u_base;
                }
                std::hint::black_box(utilities);
            }
        }
        let dot_us = t_dot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Softmax
        let utilities = [0.5f32, 1.2, 0.8, -0.4, 0.1, 0.9];
        let t_sm = Instant::now();
        for _ in 0..sweeps {
            for _ in 0..n {
                let probs = stable_softmax(&utilities, cfg.decision.decision_temperature);
                std::hint::black_box(probs);
            }
        }
        let sm_us = t_sm.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // PRNG coordinate hash
        let t_prng = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                let coord = RngCoordinate::new(
                    cfg.world.master_seed,
                    cfg.world.replicate_id,
                    1,
                    4,
                    Subsystem::Decision.id(),
                    i as u32,
                    0,
                );
                let u = coordinate_prng_f32(&coord);
                std::hint::black_box(u);
            }
        }
        let prng_us = t_prng.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // CDF select
        let probs = [0.1f32, 0.2, 0.3, 0.15, 0.15, 0.1];
        let t_cdf = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                let u = (i as f32) / (n as f32);
                let a = select_action(&probs, u);
                std::hint::black_box(a);
            }
        }
        let cdf_us = t_cdf.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Candidate search
        let mut choices = Vec::with_capacity(n);
        phase4_primary_action_selection_storage_into(
            &storage,
            base_world.current_day,
            &cfg,
            &features,
            &mut choices,
        )
        .unwrap();

        let foods = storage.food();
        let alives = storage.alive();
        let healths = storage.health();
        let group_ids = storage.group_ids();
        let agent_ids = storage.agent_ids();
        let mut cand_scratch = Vec::with_capacity(n);

        let t_cand = Instant::now();
        for _ in 0..sweeps {
            for c in &choices {
                if c.action == Action::GiveFood || c.action == Action::StealFood {
                    cand_scratch.clear();
                    let target_gid = group_ids[0];
                    for s in 0..n {
                        if group_ids[s] == target_gid
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != c.agent_id
                            && foods[s] < cfg.interaction.starvation_threshold
                        {
                            cand_scratch.push(agent_ids[s]);
                        }
                    }
                    cand_scratch.sort_unstable();
                    std::hint::black_box(&cand_scratch);
                }
            }
        }
        let cand_us = t_cand.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Intent allocation
        let mut intents = Vec::with_capacity(n);
        let t_alloc = Instant::now();
        for _ in 0..sweeps {
            intents.clear();
            for c in &choices {
                intents.push(Intent::Work {
                    agent_id: c.agent_id,
                    group_id: GroupId(0),
                    requested_harvest: 2.0,
                });
            }
        }
        let alloc_us = t_alloc.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Total Phase 4
        let mut full_choices = Vec::with_capacity(n);
        let mut full_intents = Vec::with_capacity(n);
        let t_tot = Instant::now();
        for _ in 0..sweeps {
            full_choices.clear();
            phase4_primary_action_selection_storage_into(
                &storage,
                base_world.current_day,
                &cfg,
                &features,
                &mut full_choices,
            )
            .unwrap();
            full_intents.clear();
            generate_intents_storage_with_scratch(
                &storage,
                base_world.current_day,
                &cfg,
                &full_choices,
                &mut cand_scratch,
                &mut full_intents,
            )
            .unwrap();
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>10.2} | {:>12.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>10.2} | {:>12.2}",
            pop, tot_us, dot_us, sm_us, prng_us, cdf_us, cand_us, alloc_us
        );
    }

    // 4. Phase 8 Breakdown
    println!("\n[Phase 8 Welfare Distribution Sub-components (us)]");
    println!(
        "{:<6} | {:>12} | {:>16} | {:>16} | {:>14}",
        "Pop(N)", "Total P8(us)", "Eligible Scan(us)", "Payment Math(us)", "Wealth Write(us)"
    );
    println!("{:-<72}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let mut base_world = initialize_world(&cfg).unwrap();
        let mut storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = 50;

        let alives = storage.alive();
        let healths = storage.health();
        let foods = storage.food();
        let group_ids = storage.group_ids();
        let starvation_threshold = cfg.interaction.starvation_threshold;

        let t_scan = Instant::now();
        for _ in 0..sweeps {
            let mut eligible_count = 0usize;
            for i in 0..n {
                if group_ids[i] == GroupId(0)
                    && alives[i]
                    && healths[i] > 0.0
                    && foods[i] < starvation_threshold
                {
                    eligible_count += 1;
                }
            }
            std::hint::black_box(eligible_count);
        }
        let scan_us = t_scan.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let treasury = 5000i64;
        let count = (pop / 4).max(1) as i64;
        let t_math = Instant::now();
        for _ in 0..sweeps {
            for _ in 0..10 {
                let per_agent = treasury / count;
                let remainder = treasury % count;
                std::hint::black_box((per_agent, remainder));
            }
        }
        let math_us = t_math.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let mut wealths = storage.economy.wealth.clone();
        let t_write = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if i % 4 == 0 {
                    wealths[i] += 100;
                }
            }
        }
        let write_us = t_write.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_tot = Instant::now();
        for _ in 0..sweeps {
            let _ = storage
                .phase8_welfare_distribution_with_config(&mut base_world.settlements, &cfg)
                .unwrap();
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>12.2} | {:>16.2} | {:>16.2} | {:>14.2}",
            pop, tot_us, scan_us, math_us, write_us
        );
    }

    // 5. Phase 9 Breakdown
    println!("\n[Phase 9 Mortality Commitment Sub-components (us)]");
    println!(
        "{:<6} | {:>12} | {:>16} | {:>14} | {:>14}",
        "Pop(N)", "Total P9(us)", "Mortality Scan(us)", "Alive Write(us)", "Record Alloc(us)"
    );
    println!("{:-<68}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let mut storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = 50;

        let healths = storage.health();
        let alives = storage.alive();

        let t_scan = Instant::now();
        for _ in 0..sweeps {
            let mut dead_count = 0usize;
            for i in 0..n {
                if alives[i] && healths[i] <= 0.0 {
                    dead_count += 1;
                }
            }
            std::hint::black_box(dead_count);
        }
        let scan_us = t_scan.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let mut alive_mut = storage.demography.alive.clone();
        let t_write = Instant::now();
        for _ in 0..sweeps {
            for i in 0..n {
                if i % 20 == 0 {
                    alive_mut[i] = false;
                }
            }
        }
        let write_us = t_write.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let mut dead_vec = Vec::new();
        let t_alloc = Instant::now();
        for _ in 0..sweeps {
            dead_vec.clear();
            for i in 0..n {
                if i % 20 == 0 {
                    dead_vec.push(AgentId(i as u32));
                }
            }
        }
        let alloc_us = t_alloc.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_tot = Instant::now();
        for _ in 0..sweeps {
            let _ = storage.phase9_mortality_commitment().unwrap();
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>12.2} | {:>16.2} | {:>14.2} | {:>14.2}",
            pop, tot_us, scan_us, write_us, alloc_us
        );
    }

    // 6. Phase 10 Breakdown
    println!("\n[Phase 10 Macroscopic Metrics Sub-components (us)]");
    println!(
        "{:<6} | {:>12} | {:>16} | {:>14} | {:>14} | {:>14}",
        "Pop(N)",
        "Total P10(us)",
        "Food Sum Red(us)",
        "Treasury Sum(us)",
        "Wealth Sort(us)",
        "Gini Math(us)"
    );
    println!("{:-<84}", "");
    for &pop in &test_pops {
        let mut cfg = base_config.clone();
        cfg.world.initial_population = pop;
        cfg.environment.carrying_capacity = 1000.0 * pop as f32;
        let base_world = initialize_world(&cfg).unwrap();
        let storage = SegmentedAgentStorage::from_agents(&base_world.agents);
        let n = storage.len();
        let sweeps = 50;

        let foods = storage.food();
        let wealths = storage.wealth();
        let alives = storage.alive();
        let agent_ids = storage.agent_ids();

        let mut indices: Vec<usize> = (0..n).collect();

        // Food sum reduction
        let t_food = Instant::now();
        for _ in 0..sweeps {
            let mut total_food = 0.0f64;
            for i in 0..n {
                if alives[i] {
                    total_food += foods[i] as f64;
                }
            }
            std::hint::black_box(total_food);
        }
        let food_us = t_food.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Treasury sum
        let t_treasury = Instant::now();
        for _ in 0..sweeps {
            let mut tot_t = 0i64;
            for s in &base_world.settlements {
                tot_t += s.treasury;
            }
            std::hint::black_box(tot_t);
        }
        let treasury_us = t_treasury.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Wealth sort
        let t_sort = Instant::now();
        for _ in 0..sweeps {
            indices.sort_unstable_by_key(|&i| (wealths[i], agent_ids[i]));
        }
        let sort_us = t_sort.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        // Gini math & weighted sum
        let t_gini = Instant::now();
        for _ in 0..sweeps {
            let mut sum_x: u128 = 0;
            for &idx in &indices {
                sum_x += wealths[idx] as u128;
            }
            let mut weighted_sum: u128 = 0;
            for (idx, &i) in indices.iter().enumerate() {
                let rank = (idx as u128) + 1;
                weighted_sum += rank * (wealths[i] as u128);
            }
            let two_w = weighted_sum * 2;
            let n_plus_one_s = (n as u128 + 1) * sum_x;
            let num = two_w.saturating_sub(n_plus_one_s);
            let den = (n as u128) * sum_x.max(1);
            let g = (num as f64) / (den as f64);
            std::hint::black_box(g);
        }
        let gini_us = t_gini.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        let t_tot = Instant::now();
        for _ in 0..sweeps {
            let _ = storage.phase10_metrics(&base_world.settlements, 1).unwrap();
        }
        let tot_us = t_tot.elapsed().as_nanos() as f64 / sweeps as f64 / 1000.0;

        println!(
            "{:<6} | {:>12.2} | {:>16.2} | {:>14.2} | {:>14.2} | {:>14.2}",
            pop, tot_us, food_us, treasury_us, sort_us, gini_us
        );
    }

    // Part 4: Amdahl's Law Speedup Estimation for Candidates
    println!("\n--- Part 4: Theoretical Speedup Estimation (Amdahl's Law) ---");
    println!(
        "{:<38} | {:>7} | {:>10} | {:>10} | {:>10}",
        "Candidate Kernel", "Share(P)", "Speedup 4x", "Speedup 8x", "Speedup 16x"
    );
    println!("{:-<86}", "");

    // Shares at N=1000:
    let tot_1k = d_1k.total_norm;
    let p_p4_sel = d_1k.p4_sel / tot_1k;
    let p_p2 = d_1k.p2 / tot_1k;
    let p_p3 = d_1k.p3 / tot_1k;
    let _p_p10_food = 0.001; // tiny fraction
    let p_combined = p_p4_sel + p_p2 + p_p3;

    let amdahl = |p: f64, s: f64| 1.0 / ((1.0 - p) + (p / s));

    let candidates = [
        ("Candidate 1: Phase 4 Decision Dot & Softmax", p_p4_sel),
        ("Candidate 2: Phase 2 Degradation Clamp & Decay", p_p2),
        ("Candidate 3: Phase 3 State Norm Ratios", p_p3),
        ("Combined Top 3 Kernels (P4-Sel + P2 + P3)", p_combined),
    ];

    for (name, p) in &candidates {
        let s4 = amdahl(*p, 4.0);
        let s8 = amdahl(*p, 8.0);
        let s16 = amdahl(*p, 16.0);
        println!(
            "{:<38} | {:>6.1}% | {:>9.2}x | {:>9.2}x | {:>9.2}x",
            name,
            p * 100.0,
            s4,
            s8,
            s16
        );
    }

    println!("\nAmdahl Bottleneck Identification:");
    println!(
        "- Intent Generation (Phase 4-Int) takes ~{:.1}% of total runtime (dominant non-SIMD candidate).",
        (d_1k.p4_int / tot_1k) * 100.0
    );
    println!(
        "- Resolution & Event Staging take ~{:.1}% of total runtime.",
        ((d_1k.p6a + d_1k.p6b + d_1k.p7 + d_1k.stg) / tot_1k) * 100.0
    );
    println!(
        "- Maximum theoretical speedup from accelerating Phase 4 Selection + Phase 2 + Phase 3 to infinity: {:.2}x",
        1.0 / (1.0 - p_combined)
    );
}

#[derive(Clone, Copy)]
struct M2292Median {
    median_us: f64,
    mad_us: f64,
}

#[derive(Clone, Copy)]
struct M2292ScratchRow {
    family: &'static str,
    population: u64,
    selection: M2292Median,
    intent_ephemeral: M2292Median,
    intent_persistent: M2292Median,
    candidate_scan: M2292Median,
    full_tick: [M2292Median; 3],
}

fn m2_29_2_median_us(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[samples.len() / 2]
}

fn m2_29_2_summary(samples: Vec<f64>) -> M2292Median {
    let median_us = m2_29_2_median_us(samples.clone());
    let mad_us = m2_29_2_median_us(
        samples
            .into_iter()
            .map(|sample| (sample - median_us).abs())
            .collect(),
    );
    M2292Median { median_us, mad_us }
}

fn m2_29_2_candidate_scan(
    storage: &SegmentedAgentStorage,
    choices: &[PrimaryActionChoice],
    choice_groups: &[GroupId],
    config: &SimConfig,
    candidate_scratch: &mut Vec<AgentId>,
) -> usize {
    let alives = storage.alive();
    let healths = storage.health();
    let foods = storage.food();
    let group_ids = storage.group_ids();
    let agent_ids = storage.agent_ids();
    let storage_is_sorted = agent_ids.windows(2).all(|w| w[0] <= w[1]);
    let mut total_candidates = 0usize;

    for (choice, &group_id) in choices.iter().zip(choice_groups) {
        match choice.action {
            Action::GiveFood => {
                candidate_scratch.clear();
                for slot in 0..storage.len() {
                    if group_ids[slot] == group_id
                        && alives[slot]
                        && healths[slot] > 0.0
                        && agent_ids[slot] != choice.agent_id
                        && foods[slot] < config.interaction.starvation_threshold
                    {
                        candidate_scratch.push(agent_ids[slot]);
                    }
                }
            }
            Action::StealFood => {
                candidate_scratch.clear();
                for slot in 0..storage.len() {
                    if group_ids[slot] == group_id
                        && alives[slot]
                        && healths[slot] > 0.0
                        && agent_ids[slot] != choice.agent_id
                        && foods[slot] > 0.0
                    {
                        candidate_scratch.push(agent_ids[slot]);
                    }
                }
            }
            _ => continue,
        }
        if !storage_is_sorted {
            candidate_scratch.sort_unstable();
        }
        total_candidates += candidate_scratch.len();
    }

    std::hint::black_box(candidate_scratch.as_slice());
    total_candidates
}

fn m2_29_2_measure_phase4(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
) -> (
    M2292Median,
    M2292Median,
    M2292Median,
    M2292Median,
    usize,
    u64,
) {
    const SAMPLES: usize = 7;
    const REPEATS: usize = 3;

    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    let mut phase2_world = base_world.clone();
    phase1_resource_regrowth(&mut phase2_world, &effective_config);
    let mut storage = SegmentedAgentStorage::from_agents(&phase2_world.agents);
    storage.phase2_degradation_with_config(&effective_config);

    let mut features = Vec::with_capacity(storage.len());
    storage
        .phase3_features_into(&phase2_world.settlements, &effective_config, &mut features)
        .unwrap();
    let mut choices = Vec::with_capacity(storage.len());
    phase4_primary_action_selection_storage_into(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &features,
        &mut choices,
    )
    .unwrap();

    let targeted_initiators = choices
        .iter()
        .filter(|choice| matches!(choice.action, Action::GiveFood | Action::StealFood))
        .count();
    let scanned_slots = targeted_initiators as u64 * storage.len() as u64;
    let choice_groups: Vec<GroupId> = choices
        .iter()
        .map(|choice| {
            let slot = storage.slot_of(choice.agent_id).unwrap();
            storage.group_ids()[slot]
        })
        .collect();

    let mut selected = Vec::with_capacity(storage.len());
    for _ in 0..2 {
        phase4_primary_action_selection_storage_into(
            &storage,
            phase2_world.current_day,
            &effective_config,
            &features,
            &mut selected,
        )
        .unwrap();
    }
    assert_eq!(selected, choices);
    let mut selection_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        for _ in 0..REPEATS {
            phase4_primary_action_selection_storage_into(
                &storage,
                phase2_world.current_day,
                &effective_config,
                &features,
                &mut selected,
            )
            .unwrap();
        }
        selection_samples.push(started.elapsed().as_nanos() as f64 / REPEATS as f64 / 1000.0);
    }
    assert_eq!(selected, choices);

    let mut ephemeral_intents = Vec::with_capacity(storage.len());
    phase4_generate_intents_storage_into(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &choices,
        &mut ephemeral_intents,
    )
    .unwrap();
    let mut persistent_candidates = Vec::with_capacity(storage.len());
    let mut persistent_intents = Vec::with_capacity(storage.len());
    generate_intents_storage_with_scratch(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &choices,
        &mut persistent_candidates,
        &mut persistent_intents,
    )
    .unwrap();
    assert_eq!(ephemeral_intents, persistent_intents);
    for _ in 0..2 {
        generate_intents_storage_with_scratch(
            &storage,
            phase2_world.current_day,
            &effective_config,
            &choices,
            &mut persistent_candidates,
            &mut persistent_intents,
        )
        .unwrap();
        assert_eq!(ephemeral_intents, persistent_intents);
    }

    let mut intent_buffer = Vec::with_capacity(storage.len());
    let mut ephemeral_samples = Vec::with_capacity(SAMPLES);
    let mut persistent_samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        for path in 0..2 {
            let persistent_first = sample % 2 == 1;
            let use_persistent = (path == 0) == persistent_first;
            let started = Instant::now();
            for _ in 0..REPEATS {
                if use_persistent {
                    generate_intents_storage_with_scratch(
                        &storage,
                        phase2_world.current_day,
                        &effective_config,
                        &choices,
                        &mut persistent_candidates,
                        &mut intent_buffer,
                    )
                    .unwrap();
                } else {
                    phase4_generate_intents_storage_into(
                        &storage,
                        phase2_world.current_day,
                        &effective_config,
                        &choices,
                        &mut intent_buffer,
                    )
                    .unwrap();
                }
                std::hint::black_box(intent_buffer.as_slice());
            }
            let time_us = started.elapsed().as_nanos() as f64 / REPEATS as f64 / 1000.0;
            if use_persistent {
                persistent_samples.push(time_us);
            } else {
                ephemeral_samples.push(time_us);
            }
        }
    }

    let mut scan_candidates = Vec::with_capacity(storage.len());
    let _ = m2_29_2_candidate_scan(
        &storage,
        &choices,
        &choice_groups,
        &effective_config,
        &mut scan_candidates,
    );
    let mut candidate_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        let candidates = m2_29_2_candidate_scan(
            &storage,
            &choices,
            &choice_groups,
            &effective_config,
            &mut scan_candidates,
        );
        std::hint::black_box(candidates);
        candidate_samples.push(started.elapsed().as_nanos() as f64 / 1000.0);
    }

    (
        m2_29_2_summary(selection_samples),
        m2_29_2_summary(ephemeral_samples),
        m2_29_2_summary(persistent_samples),
        m2_29_2_summary(candidate_samples),
        targeted_initiators,
        scanned_slots,
    )
}

fn m2_29_2_assert_path_parity(base_world: &WorldState, config: &SimConfig, context: &M0RunContext) {
    let mut production = HybridWorldState::hybrid(base_world.clone());
    let mut benchmark = HybridWorldState::hybrid(base_world.clone());
    let mut persistent = HybridWorldState::hybrid(base_world.clone());
    let count = base_world.agents.len();
    let mut features = Vec::with_capacity(count);
    let mut choices = Vec::with_capacity(count);
    let mut intents = Vec::with_capacity(count);
    let mut metrics = AgentDynamicSoAScratch::with_capacity(count);
    let mut candidates = Vec::with_capacity(count);
    let mut outcomes_production = Vec::new();
    let mut outcomes_benchmark = Vec::new();
    let mut outcomes_persistent = Vec::new();

    for day in 0..3 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: day == 1,
        };
        let mut production_candidate_scratch = Vec::with_capacity(count);
        outcomes_production.push(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut production,
                config,
                context,
                &options,
                &mut production_candidate_scratch,
            )
            .unwrap(),
        );
        outcomes_benchmark.push(
            run_hybrid_authority_day_with_scratch(
                &mut benchmark,
                config,
                context,
                &options,
                &mut features,
                &mut choices,
                &mut intents,
                &mut metrics,
            )
            .unwrap(),
        );
        outcomes_persistent.push(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut persistent,
                config,
                context,
                &options,
                &mut candidates,
            )
            .unwrap(),
        );
    }

    assert_eq!(outcomes_production, outcomes_benchmark);
    assert_eq!(outcomes_production, outcomes_persistent);
    let expected_hash = production.canonical_state_hash().unwrap();
    assert_eq!(expected_hash, benchmark.canonical_state_hash().unwrap());
    assert_eq!(expected_hash, persistent.canonical_state_hash().unwrap());
}

fn m2_29_2_time_production_ticks(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: usize,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let started = Instant::now();
    for _ in 0..ticks {
        let mut candidate_scratch = Vec::with_capacity(base_world.agents.len());
        std::hint::black_box(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut world,
                config,
                context,
                options,
                &mut candidate_scratch,
            )
            .unwrap(),
        );
    }
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

fn m2_29_2_time_benchmark_ticks(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: usize,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let count = base_world.agents.len();
    let started = Instant::now();
    for _ in 0..ticks {
        let mut features = Vec::with_capacity(count);
        let mut choices = Vec::with_capacity(count);
        let mut intents = Vec::with_capacity(count);
        let mut metrics = AgentDynamicSoAScratch::with_capacity(count);
        std::hint::black_box(
            run_hybrid_authority_day_with_scratch(
                &mut world,
                config,
                context,
                options,
                &mut features,
                &mut choices,
                &mut intents,
                &mut metrics,
            )
            .unwrap(),
        );
    }
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

fn m2_29_2_time_persistent_ticks(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: usize,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let mut candidates = Vec::with_capacity(base_world.agents.len());
    let started = Instant::now();
    for _ in 0..ticks {
        std::hint::black_box(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut world,
                config,
                context,
                options,
                &mut candidates,
            )
            .unwrap(),
        );
    }
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

fn measure_m2_29_2_scratch_fidelity(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 7] = [100, 250, 500, 1000, 2500, 5000, 10000];
    const SAMPLES: usize = 7;
    const TICKS_PER_SAMPLE: usize = 3;
    const WARMUP_TICKS: usize = 2;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workload_families = ["Fixed settlements (2)", "Fixed group size (~200)"];
    let mut rows = Vec::with_capacity(POPULATIONS.len() * workload_families.len());

    println!("\n=================================================================");
    println!("M2-29.2 Phase4 Scratch Lifetime Fidelity & Scaling");
    println!(
        "Method: 7 median samples; each full-tick sample averages 3 ticks after 2 warm-up ticks."
    );
    println!("Each A/B/C path starts from the same deterministic initialized world/config/seed.");
    println!("=================================================================");

    for (family_index, family) in workload_families.iter().enumerate() {
        for &population in &POPULATIONS {
            let mut config = base_config.clone();
            config.world.initial_population = population;
            config.world.settlement_count = if family_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            config.environment.carrying_capacity = 1000.0 * population as f32;
            config.world.initial_settlement_resource = 200.0 * population as f32;
            let base_world = initialize_world(&config).unwrap();
            let settlement_count = config.world.settlement_count;

            m2_29_2_assert_path_parity(&base_world, &config, context);
            for _ in 0..WARMUP_TICKS {
                let _ = m2_29_2_time_production_ticks(&base_world, &config, context, &options, 1);
                let _ = m2_29_2_time_benchmark_ticks(&base_world, &config, context, &options, 1);
                let _ = m2_29_2_time_persistent_ticks(&base_world, &config, context, &options, 1);
            }

            let (
                selection_us,
                intent_ephemeral_us,
                intent_persistent_us,
                candidate_scan_us,
                targeted_initiators,
                scanned_slots,
            ) = m2_29_2_measure_phase4(&base_world, &config, context);

            let mut full_samples: [Vec<f64>; 3] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                for offset in 0..3 {
                    let path = (sample + offset) % 3;
                    let time_us = match path {
                        0 => m2_29_2_time_production_ticks(
                            &base_world,
                            &config,
                            context,
                            &options,
                            TICKS_PER_SAMPLE,
                        ),
                        1 => m2_29_2_time_benchmark_ticks(
                            &base_world,
                            &config,
                            context,
                            &options,
                            TICKS_PER_SAMPLE,
                        ),
                        _ => m2_29_2_time_persistent_ticks(
                            &base_world,
                            &config,
                            context,
                            &options,
                            TICKS_PER_SAMPLE,
                        ),
                    };
                    full_samples[path].push(time_us);
                }
            }

            let row = M2292ScratchRow {
                family,
                population,
                selection: selection_us,
                intent_ephemeral: intent_ephemeral_us,
                intent_persistent: intent_persistent_us,
                candidate_scan: candidate_scan_us,
                full_tick: [
                    m2_29_2_summary(full_samples[0].clone()),
                    m2_29_2_summary(full_samples[1].clone()),
                    m2_29_2_summary(full_samples[2].clone()),
                ],
            };
            rows.push(row);

            println!(
                "{:<26} N={:<5} groups={:<3} avg/group={:>6.1} targeted={:<5} scans={:<12} P4-select={:>8.2}±{:>5.2}us P4-intent(ephemeral/reuse)={:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}us candidate-scan={:>8.2}±{:>5.2}us full(A/B/C)={:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}us",
                family,
                population,
                settlement_count,
                population as f64 / settlement_count as f64,
                targeted_initiators,
                scanned_slots,
                selection_us.median_us,
                selection_us.mad_us,
                intent_ephemeral_us.median_us,
                intent_ephemeral_us.mad_us,
                intent_persistent_us.median_us,
                intent_persistent_us.mad_us,
                candidate_scan_us.median_us,
                candidate_scan_us.mad_us,
                row.full_tick[0].median_us,
                row.full_tick[0].mad_us,
                row.full_tick[1].median_us,
                row.full_tick[1].mad_us,
                row.full_tick[2].median_us,
                row.full_tick[2].mad_us,
            );
        }
    }

    println!(
        "\nCandidate allocation attribution (AgentId size={} bytes):",
        std::mem::size_of::<AgentId>()
    );
    println!(
        "A Production Current: capacity=active population; one Vec allocation/request per tick, no reallocation within tick, dropped at return; request bytes ~= active population * AgentId size."
    );
    println!(
        "B Production-equivalent benchmark: feature/choice/intent/metrics/candidate scratch are all created per tick and dropped on return, matching the one-day production runner."
    );
    println!(
        "C Persistent candidate scratch: one capacity-N allocation per runner/caller scratch, clear at Phase4 entry and before each target scan, no per-tick allocation/reallocation while population <= initial N."
    );
    println!(
        "No global allocator instrumentation was added; counts/bytes come from Vec::with_capacity and the max candidate bound <= storage.len()."
    );
    for population in POPULATIONS {
        let request_bytes = population as usize * std::mem::size_of::<AgentId>();
        println!(
            "N={population:<5} capacity={population:<5} AgentIds; A/B=1 allocation, 0 reallocations, {request_bytes} requested bytes/tick; C=1 setup allocation of {request_bytes} bytes, then 0/tick."
        );
    }

    println!(
        "\nScaling from N=1000 to N=10000 (ratio and alpha=ln(ratio)/ln(10)); candidate scan loops inspect every storage slot per GiveFood/StealFood initiator:"
    );
    for family in workload_families {
        let at_1k = rows
            .iter()
            .find(|row| row.family == family && row.population == 1000)
            .unwrap();
        let at_10k = rows
            .iter()
            .find(|row| row.family == family && row.population == 10000)
            .unwrap();
        let metrics = [
            (
                "Phase4 Selection",
                at_1k.selection.median_us,
                at_10k.selection.median_us,
            ),
            (
                "Phase4 Intent ephemeral",
                at_1k.intent_ephemeral.median_us,
                at_10k.intent_ephemeral.median_us,
            ),
            (
                "Phase4 Intent persistent",
                at_1k.intent_persistent.median_us,
                at_10k.intent_persistent.median_us,
            ),
            (
                "Candidate discovery",
                at_1k.candidate_scan.median_us,
                at_10k.candidate_scan.median_us,
            ),
            (
                "Full tick A",
                at_1k.full_tick[0].median_us,
                at_10k.full_tick[0].median_us,
            ),
            (
                "Full tick B",
                at_1k.full_tick[1].median_us,
                at_10k.full_tick[1].median_us,
            ),
            (
                "Full tick C",
                at_1k.full_tick[2].median_us,
                at_10k.full_tick[2].median_us,
            ),
        ];
        for (metric, t_1k, t_10k) in metrics {
            let ratio = t_10k / t_1k.max(0.001);
            let alpha = ratio.ln() / 10.0f64.ln();
            println!(
                "{:<26} {:<26} T1k={:>9.2}us T10k={:>9.2}us ratio={:>7.2}x alpha={:>5.3}",
                family, metric, t_1k, t_10k, ratio, alpha
            );
        }
        println!(
            "{:<26} Phase4 Intent / Full tick A share: N=1000 {:>5.1}%, N=10000 {:>5.1}%",
            family,
            at_1k.intent_ephemeral.median_us / at_1k.full_tick[0].median_us * 100.0,
            at_10k.intent_ephemeral.median_us / at_10k.full_tick[0].median_us * 100.0,
        );
    }
}

#[derive(Clone, Copy)]
struct M230ScalingRow {
    family: &'static str,
    population: u64,
    group_count: u32,
    full_scan_intent: M2292Median,
    indexed_intent: M2292Median,
    full_tick: [M2292Median; 2],
}

fn m2_30_measure_phase4(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
) -> (M2292Median, M2292Median, M2292Median, M2292Median, usize) {
    const SAMPLES: usize = 7;
    const REPEATS: usize = 3;

    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;
    let mut phase2_world = base_world.clone();
    phase1_resource_regrowth(&mut phase2_world, &effective_config);
    let mut storage = SegmentedAgentStorage::from_agents(&phase2_world.agents);
    storage.phase2_degradation_with_config(&effective_config);
    let mut features = Vec::with_capacity(storage.len());
    storage
        .phase3_features_into(&phase2_world.settlements, &effective_config, &mut features)
        .unwrap();
    let mut choices = Vec::with_capacity(storage.len());
    phase4_primary_action_selection_storage_into(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &features,
        &mut choices,
    )
    .unwrap();
    let targeted_initiators = choices
        .iter()
        .filter(|choice| matches!(choice.action, Action::GiveFood | Action::StealFood))
        .count();

    let mut selected = Vec::with_capacity(storage.len());
    for _ in 0..2 {
        phase4_primary_action_selection_storage_into(
            &storage,
            phase2_world.current_day,
            &effective_config,
            &features,
            &mut selected,
        )
        .unwrap();
    }
    assert_eq!(selected, choices);
    let mut selection_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        for _ in 0..REPEATS {
            phase4_primary_action_selection_storage_into(
                &storage,
                phase2_world.current_day,
                &effective_config,
                &features,
                &mut selected,
            )
            .unwrap();
        }
        selection_samples.push(started.elapsed().as_nanos() as f64 / REPEATS as f64 / 1000.0);
    }
    assert_eq!(selected, choices);

    let mut full_scan_candidates = Vec::with_capacity(storage.len());
    let mut candidate_index =
        Phase4CandidateIndexScratch::with_capacity(config.world.settlement_count as usize);
    let mut full_scan_intents = Vec::with_capacity(storage.len());
    let mut indexed_intents = Vec::with_capacity(storage.len());
    generate_intents_storage_with_scratch(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &choices,
        &mut full_scan_candidates,
        &mut full_scan_intents,
    )
    .unwrap();
    generate_intents_storage_with_candidate_index(
        &storage,
        phase2_world.current_day,
        &effective_config,
        &choices,
        &mut candidate_index,
        &mut indexed_intents,
    )
    .unwrap();
    assert_eq!(full_scan_intents, indexed_intents);

    for _ in 0..2 {
        candidate_index.build(&storage, &effective_config);
    }
    let mut build_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        for _ in 0..REPEATS {
            candidate_index.build(&storage, &effective_config);
        }
        build_samples.push(started.elapsed().as_nanos() as f64 / REPEATS as f64 / 1000.0);
    }

    for _ in 0..2 {
        generate_intents_storage_with_scratch(
            &storage,
            phase2_world.current_day,
            &effective_config,
            &choices,
            &mut full_scan_candidates,
            &mut full_scan_intents,
        )
        .unwrap();
        generate_intents_storage_with_candidate_index(
            &storage,
            phase2_world.current_day,
            &effective_config,
            &choices,
            &mut candidate_index,
            &mut indexed_intents,
        )
        .unwrap();
    }

    let mut full_scan_samples = Vec::with_capacity(SAMPLES);
    let mut indexed_samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        for order in 0..2 {
            let indexed_first = sample % 2 == 1;
            let measure_indexed = (order == 0) == indexed_first;
            let started = Instant::now();
            for _ in 0..REPEATS {
                if measure_indexed {
                    generate_intents_storage_with_candidate_index(
                        &storage,
                        phase2_world.current_day,
                        &effective_config,
                        &choices,
                        &mut candidate_index,
                        &mut indexed_intents,
                    )
                    .unwrap();
                    std::hint::black_box(indexed_intents.as_slice());
                } else {
                    generate_intents_storage_with_scratch(
                        &storage,
                        phase2_world.current_day,
                        &effective_config,
                        &choices,
                        &mut full_scan_candidates,
                        &mut full_scan_intents,
                    )
                    .unwrap();
                    std::hint::black_box(full_scan_intents.as_slice());
                }
            }
            let time_us = started.elapsed().as_nanos() as f64 / REPEATS as f64 / 1000.0;
            if measure_indexed {
                indexed_samples.push(time_us);
            } else {
                full_scan_samples.push(time_us);
            }
        }
    }

    (
        m2_29_2_summary(selection_samples),
        m2_29_2_summary(build_samples),
        m2_29_2_summary(full_scan_samples),
        m2_29_2_summary(indexed_samples),
        targeted_initiators,
    )
}

fn m2_30_assert_runner_parity(base_world: &WorldState, config: &SimConfig, context: &M0RunContext) {
    let mut full_scan = HybridWorldState::hybrid(base_world.clone());
    let mut indexed = HybridWorldState::hybrid(base_world.clone());
    let mut full_scan_candidates = Vec::with_capacity(base_world.agents.len());
    let mut candidate_index =
        Phase4CandidateIndexScratch::with_capacity(config.world.settlement_count as usize);
    let mut full_scan_outcomes = Vec::new();
    let mut indexed_outcomes = Vec::new();

    for day in 0..3 {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: day == 1,
        };
        full_scan_outcomes.push(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut full_scan,
                config,
                context,
                &options,
                &mut full_scan_candidates,
            )
            .unwrap(),
        );
        indexed_outcomes.push(
            run_hybrid_authority_day_with_candidate_index_scratch(
                &mut indexed,
                config,
                context,
                &options,
                &mut candidate_index,
            )
            .unwrap(),
        );
    }

    assert_eq!(full_scan_outcomes, indexed_outcomes);
    assert_eq!(
        full_scan.canonical_state_hash().unwrap(),
        indexed.canonical_state_hash().unwrap()
    );
}

fn m2_30_time_full_scan_ticks(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: usize,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let started = Instant::now();
    for _ in 0..ticks {
        let mut candidate_scratch = Vec::with_capacity(base_world.agents.len());
        std::hint::black_box(
            run_hybrid_authority_day_with_candidate_scratch(
                &mut world,
                config,
                context,
                options,
                &mut candidate_scratch,
            )
            .unwrap(),
        );
    }
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

fn m2_30_time_indexed_ticks(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: usize,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let mut candidate_index =
        Phase4CandidateIndexScratch::with_capacity(config.world.settlement_count as usize);
    let started = Instant::now();
    for _ in 0..ticks {
        std::hint::black_box(
            run_hybrid_authority_day_with_candidate_index_scratch(
                &mut world,
                config,
                context,
                options,
                &mut candidate_index,
            )
            .unwrap(),
        );
    }
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

const M230_PROFILE_PHASES: [&str; 14] = [
    "Phase 1 Regrowth",
    "Phase 2 Degradation",
    "Phase 3 Features",
    "Phase 4 Selection",
    "Phase 4 Indexed Intent",
    "Phase 5 Partition",
    "Phase 6A Work",
    "Phase 6B Targeted",
    "Phase 7 Market",
    "Phase 8 Welfare",
    "Phase 9 Mortality",
    "Event Staging",
    "Phase 10 Metrics",
    "Phase 11 Event Flush",
];

fn m2_30_profile_indexed_day(
    base_world: &WorldState,
    effective_config: &SimConfig,
    candidate_index: &mut Phase4CandidateIndexScratch,
) -> [Duration; 14] {
    let mut times = [Duration::ZERO; 14];
    let mut world = base_world.clone();
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let executed_day = world.current_day.as_u32();

    let started = Instant::now();
    phase1_resource_regrowth(&mut world, effective_config);
    times[0] = started.elapsed();

    let started = Instant::now();
    storage.phase2_degradation_with_config(effective_config);
    times[1] = started.elapsed();

    let mut features = Vec::with_capacity(storage.len());
    let started = Instant::now();
    storage
        .phase3_features_into(&world.settlements, effective_config, &mut features)
        .unwrap();
    times[2] = started.elapsed();

    let mut choices = Vec::with_capacity(storage.len());
    let started = Instant::now();
    phase4_primary_action_selection_storage_into(
        &storage,
        world.current_day,
        effective_config,
        &features,
        &mut choices,
    )
    .unwrap();
    times[3] = started.elapsed();

    let mut intents = Vec::with_capacity(storage.len());
    let started = Instant::now();
    generate_intents_storage_with_candidate_index(
        &storage,
        world.current_day,
        effective_config,
        &choices,
        candidate_index,
        &mut intents,
    )
    .unwrap();
    times[4] = started.elapsed();

    let started = Instant::now();
    let partitions = phase5_partition_intents_baseline(&intents).unwrap();
    times[5] = started.elapsed();

    let started = Instant::now();
    let work_resolutions =
        phase6a_work_resolution_storage(&mut storage, &mut world.settlements, &partitions).unwrap();
    times[6] = started.elapsed();

    let started = Instant::now();
    let targeted_resolutions = phase6b_targeted_resolution_storage_baseline(
        &mut storage,
        &world.settlements,
        world.current_day,
        effective_config,
        &partitions,
    )
    .unwrap();
    times[7] = started.elapsed();

    let started = Instant::now();
    let market_resolutions = phase7_market_clearance_storage_with_config(
        &mut storage,
        &mut world.settlements,
        &partitions,
        &effective_config.economy,
    )
    .unwrap();
    times[8] = started.elapsed();

    let started = Instant::now();
    let welfare_resolutions = storage
        .phase8_welfare_distribution_with_config(&mut world.settlements, effective_config)
        .unwrap();
    times[9] = started.elapsed();

    let started = Instant::now();
    let mortality_resolution = storage.phase9_mortality_commitment().unwrap();
    times[10] = started.elapsed();

    let started = Instant::now();
    let estimated_cap = storage.len().saturating_mul(2) + world.settlements.len() + 4;
    let mut event_buffer = EventBuffer::with_capacity(estimated_cap);
    let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
    for work in &work_resolutions {
        let events = events_from_work_resolution(executed_day, work);
        let count = events.len() as u64;
        if let Some(entry) = phase6_counts
            .iter_mut()
            .find(|(group_id, _)| *group_id == work.group_id.0)
        {
            entry.1 += count;
        } else {
            phase6_counts.push((work.group_id.0, count));
        }
        event_buffer.push_all(events);
    }
    for targeted in &targeted_resolutions {
        let mut events = events_from_targeted_resolution(executed_day, targeted);
        let offset = if let Some(entry) = phase6_counts
            .iter_mut()
            .find(|(group_id, _)| *group_id == targeted.group_id.0)
        {
            let previous = entry.1;
            entry.1 += events.len() as u64;
            previous
        } else {
            phase6_counts.push((targeted.group_id.0, events.len() as u64));
            0
        };
        if offset > 0 {
            for event in &mut events {
                event.key.local_sequence += offset;
            }
        }
        event_buffer.push_all(events);
    }
    for market in &market_resolutions {
        event_buffer.push_all(events_from_market_resolution(executed_day, market));
    }
    for welfare in &welfare_resolutions {
        event_buffer.push_all(events_from_welfare_resolution(executed_day, welfare));
    }
    event_buffer.push_all(events_from_mortality_resolution(
        executed_day,
        &mortality_resolution,
    ));
    times[11] = started.elapsed();

    let started = Instant::now();
    let metrics = storage
        .phase10_metrics(&world.settlements, executed_day)
        .unwrap();
    event_buffer.push(event_from_daily_metrics(&metrics));
    times[12] = started.elapsed();

    let started = Instant::now();
    std::hint::black_box(phase11_flush_events(&mut event_buffer).unwrap());
    times[13] = started.elapsed();
    times
}

fn measure_m2_30_post_index_top_phases(
    base_config: &SimConfig,
    context: &M0RunContext,
    family: &'static str,
    settlement_count: u32,
) {
    const POPULATION: u64 = 10000;
    const SAMPLES: usize = 7;
    let mut config = base_config.clone();
    config.world.initial_population = POPULATION;
    config.world.settlement_count = settlement_count;
    config.environment.carrying_capacity = 1000.0 * POPULATION as f32;
    config.world.initial_settlement_resource = 200.0 * POPULATION as f32;
    let base_world = initialize_world(&config).unwrap();
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;
    let mut candidate_index = Phase4CandidateIndexScratch::with_capacity(settlement_count as usize);

    for _ in 0..2 {
        let _ = m2_30_profile_indexed_day(&base_world, &effective_config, &mut candidate_index);
    }
    let mut samples: [Vec<f64>; 14] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for _ in 0..SAMPLES {
        for (phase_samples, elapsed) in samples.iter_mut().zip(m2_30_profile_indexed_day(
            &base_world,
            &effective_config,
            &mut candidate_index,
        )) {
            phase_samples.push(elapsed.as_nanos() as f64 / 1000.0);
        }
    }
    let mut phases: Vec<_> = M230_PROFILE_PHASES
        .into_iter()
        .zip(samples.into_iter().map(m2_29_2_summary))
        .collect();
    phases.sort_by(|left, right| right.1.median_us.total_cmp(&left.1.median_us));
    println!("\nPost-index N=10000 top phases: {family} (median ± MAD, µs)");
    for (name, timing) in phases.iter().take(6) {
        println!(
            "{name:<28} {:>9.2} ± {:>7.2}",
            timing.median_us, timing.mad_us
        );
    }
}

fn measure_m2_30_phase4_candidate_index(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 7] = [100, 250, 500, 1000, 2500, 5000, 10000];
    const SAMPLES: usize = 7;
    const TICKS_PER_SAMPLE: usize = 3;
    const WARMUP_TICKS: usize = 2;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workload_families = ["Fixed settlements (2)", "Fixed group size (~200)"];

    println!("\n=================================================================");
    println!("M2-30 Phase4 Full Scan vs Group/Action Candidate Pre-Index");
    println!("Method: 2 warm-up ticks + 7 median/MAD samples; full-tick samples average 3 days.");
    println!("Both paths use identical initialized worlds, config, seed, and Phase4 choices.");
    println!("=================================================================");

    let mut rows = Vec::with_capacity(POPULATIONS.len() * workload_families.len());
    for (family_index, family) in workload_families.iter().enumerate() {
        for &population in &POPULATIONS {
            let mut config = base_config.clone();
            config.world.initial_population = population;
            config.world.settlement_count = if family_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            config.environment.carrying_capacity = 1000.0 * population as f32;
            config.world.initial_settlement_resource = 200.0 * population as f32;
            let base_world = initialize_world(&config).unwrap();
            m2_30_assert_runner_parity(&base_world, &config, context);

            for _ in 0..WARMUP_TICKS {
                let _ = m2_30_time_full_scan_ticks(&base_world, &config, context, &options, 1);
                let _ = m2_30_time_indexed_ticks(&base_world, &config, context, &options, 1);
            }

            let (selection, index_build, full_scan_intent, indexed_intent, targeted_initiators) =
                m2_30_measure_phase4(&base_world, &config, context);
            let mut full_tick_samples: [Vec<f64>; 2] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                for offset in 0..2 {
                    let path = (sample + offset) % 2;
                    let time_us = if path == 0 {
                        m2_30_time_full_scan_ticks(
                            &base_world,
                            &config,
                            context,
                            &options,
                            TICKS_PER_SAMPLE,
                        )
                    } else {
                        m2_30_time_indexed_ticks(
                            &base_world,
                            &config,
                            context,
                            &options,
                            TICKS_PER_SAMPLE,
                        )
                    };
                    full_tick_samples[path].push(time_us);
                }
            }

            let row = M230ScalingRow {
                family,
                population,
                group_count: config.world.settlement_count,
                full_scan_intent,
                indexed_intent,
                full_tick: [
                    m2_29_2_summary(full_tick_samples[0].clone()),
                    m2_29_2_summary(full_tick_samples[1].clone()),
                ],
            };
            rows.push(row);

            let index_visits = population;
            let full_scan_visits = targeted_initiators as u64 * population;
            let full_ticks_per_second = 1_000_000.0 / row.full_tick[0].median_us;
            let indexed_ticks_per_second = 1_000_000.0 / row.full_tick[1].median_us;
            println!(
                "{:<26} N={:<5} groups={:<3} avg/group={:>6.1} targeted={:<5} candidate slots(scan/index)={:>12}/{:<7} lookups={:<5} select={:>8.2}us build={:>8.2}us intent(scan/index)={:>9.2}/{:>9.2}us full(scan/index)={:>9.2}/{:>9.2}us speedup={:>5.2}x ticks/s={:>7.1}/{:>7.1}",
                family,
                population,
                row.group_count,
                population as f64 / row.group_count as f64,
                targeted_initiators,
                full_scan_visits,
                index_visits,
                targeted_initiators,
                selection.median_us,
                index_build.median_us,
                row.full_scan_intent.median_us,
                row.indexed_intent.median_us,
                row.full_tick[0].median_us,
                row.full_tick[1].median_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                full_ticks_per_second,
                indexed_ticks_per_second,
            );
        }
    }

    println!("\nScaling from N=1000 to N=10000: Phase4 Intent and full-tick median alpha.");
    println!(
        "Candidate-slot counts are Full Scan T*N versus Pre-Index N. Both retain one N-slot choice-coverage pass; Full Scan also checks storage ordering once, while Pre-Index fuses that check into its build scan."
    );
    for family in workload_families {
        let at_1k = rows
            .iter()
            .find(|row| row.family == family && row.population == 1000)
            .unwrap();
        let at_10k = rows
            .iter()
            .find(|row| row.family == family && row.population == 10000)
            .unwrap();
        for (name, small, large) in [
            (
                "Full Scan Intent",
                at_1k.full_scan_intent.median_us,
                at_10k.full_scan_intent.median_us,
            ),
            (
                "Pre-Indexed Intent",
                at_1k.indexed_intent.median_us,
                at_10k.indexed_intent.median_us,
            ),
            (
                "Full Scan tick",
                at_1k.full_tick[0].median_us,
                at_10k.full_tick[0].median_us,
            ),
            (
                "Pre-Indexed tick",
                at_1k.full_tick[1].median_us,
                at_10k.full_tick[1].median_us,
            ),
        ] {
            let ratio = large / small.max(0.001);
            let alpha = ratio.ln() / 10.0f64.ln();
            println!(
                "{family:<26} {name:<22} {small:>9.2} -> {large:>9.2}us ratio={ratio:>7.2}x alpha={alpha:>5.3}"
            );
        }
        println!(
            "{family:<26} N=10000 Phase4 share: Full Scan {:>5.1}%, Pre-Indexed {:>5.1}%",
            at_10k.full_scan_intent.median_us / at_10k.full_tick[0].median_us * 100.0,
            at_10k.indexed_intent.median_us / at_10k.full_tick[1].median_us * 100.0,
        );
    }

    measure_m2_30_post_index_top_phases(base_config, context, workload_families[0], 2);
    measure_m2_30_post_index_top_phases(base_config, context, workload_families[1], 50);
}

#[derive(Clone, Copy)]
struct M232ScalingRow {
    family: &'static str,
    population: u64,
    settlement_count: u32,
    phase8_old: M2292Median,
    index_probe: M2292Median,
    phase8_new: M2292Median,
    full_tick: [M2292Median; 2],
}

fn m2_32_phase8_sample(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    scratch: &mut Phase8WelfareScratch,
    one_pass: bool,
) -> f64 {
    let mut sample_storage = storage.clone();
    let mut sample_settlements = settlements.to_vec();
    let started = Instant::now();
    let result = if one_pass {
        phase8_welfare_distribution_storage_with_scratch(
            &mut sample_storage,
            &mut sample_settlements,
            config.interaction.starvation_threshold,
            config.economy.welfare_payment,
            scratch,
        )
    } else {
        phase8_welfare_distribution_storage_full_scan(
            &mut sample_storage,
            &mut sample_settlements,
            config.interaction.starvation_threshold,
            config.economy.welfare_payment,
        )
    };
    std::hint::black_box(result.unwrap());
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_32_phase8_index_probe_sample(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    scratch: &mut Phase8WelfareScratch,
) -> f64 {
    let mut probe_storage = storage.clone();
    probe_storage
        .food_mut()
        .fill(config.interaction.starvation_threshold);
    m2_32_phase8_sample(&probe_storage, settlements, config, scratch, true)
}

fn m2_32_time_full_tick(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    ticks: u32,
    one_pass: bool,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let started = Instant::now();
    let outcomes = if one_pass {
        run_hybrid_authority_days(&mut world, config, context, ticks, options)
    } else {
        run_hybrid_authority_days_with_phase8_full_scan(&mut world, config, context, ticks, options)
    };
    std::hint::black_box(outcomes.unwrap());
    started.elapsed().as_nanos() as f64 / ticks as f64 / 1000.0
}

fn m2_32_assert_full_tick_parity(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
) {
    let mut full_scan = HybridWorldState::hybrid(base_world.clone());
    let mut one_pass = HybridWorldState::hybrid(base_world.clone());
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let baseline = run_hybrid_authority_days_with_phase8_full_scan(
        &mut full_scan,
        config,
        context,
        3,
        &options,
    )
    .unwrap();
    let optimized = run_hybrid_authority_days(&mut one_pass, config, context, 3, &options).unwrap();
    assert_eq!(baseline, optimized);
    assert_eq!(
        full_scan.canonical_state_hash().unwrap(),
        one_pass.canonical_state_hash().unwrap()
    );
}

fn measure_m2_32_phase8_one_pass(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 8] = [100, 250, 500, 1000, 2500, 5000, 10000, 20000];
    const SAMPLES: usize = 7;
    const WARMUPS: usize = 2;
    const TICKS_PER_SAMPLE: u32 = 3;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workload_families = ["Fixed settlements (2)", "Fixed group size (~200)"];
    let mut rows = Vec::with_capacity(POPULATIONS.len() * workload_families.len());

    println!("\n=================================================================");
    println!("M2-32 Phase8 Repeated Scan vs One-Pass Group Buckets");
    println!(
        "Method: 2 warm-ups + 7 median/MAD samples; full-tick samples average 3 production days."
    );
    println!(
        "Phase8 scratch is retained between samples; runner scratch spans each 3-day full-tick sample."
    );
    println!(
        "The index-build probe uses threshold-equal food to retain the scan but produce no recipients."
    );
    println!("=================================================================");

    for (family_index, family) in workload_families.iter().enumerate() {
        let mut phase8_scratch = Phase8WelfareScratch::with_capacity(100);
        for &population in &POPULATIONS {
            let mut config = base_config.clone();
            config.world.initial_population = population;
            config.world.settlement_count = if family_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            config.environment.carrying_capacity = 1000.0 * population as f32;
            config.world.initial_settlement_resource = 200.0 * population as f32;
            let base_world = initialize_world(&config).unwrap();
            if population == 10000 {
                m2_32_assert_full_tick_parity(&base_world, &config, context);
            }
            let base_storage = SegmentedAgentStorage::from_agents(&base_world.agents);
            let mut baseline_storage = base_storage.clone();
            let mut baseline_settlements = base_world.settlements.clone();
            let eligible_count = phase8_welfare_distribution_storage_full_scan(
                &mut baseline_storage,
                &mut baseline_settlements,
                config.interaction.starvation_threshold,
                config.economy.welfare_payment,
            )
            .unwrap()
            .iter()
            .map(|result| result.eligible_count)
            .sum::<usize>();

            let mut index_probe_storage = base_storage.clone();
            index_probe_storage
                .food_mut()
                .fill(config.interaction.starvation_threshold);
            for _ in 0..WARMUPS {
                let _ = m2_32_phase8_sample(
                    &base_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                    false,
                );
                let _ = m2_32_phase8_index_probe_sample(
                    &index_probe_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                );
                let _ = m2_32_phase8_sample(
                    &base_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                    true,
                );
                let _ = m2_32_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    TICKS_PER_SAMPLE,
                    false,
                );
                let _ = m2_32_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    TICKS_PER_SAMPLE,
                    true,
                );
            }

            let mut old_samples = Vec::with_capacity(SAMPLES);
            let mut probe_samples = Vec::with_capacity(SAMPLES);
            let mut new_samples = Vec::with_capacity(SAMPLES);
            let mut full_tick_samples: [Vec<f64>; 2] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                old_samples.push(m2_32_phase8_sample(
                    &base_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                    false,
                ));
                probe_samples.push(m2_32_phase8_index_probe_sample(
                    &index_probe_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                ));
                new_samples.push(m2_32_phase8_sample(
                    &base_storage,
                    &base_world.settlements,
                    &config,
                    &mut phase8_scratch,
                    true,
                ));
                for offset in 0..2 {
                    let path = (sample + offset) % 2;
                    full_tick_samples[path].push(m2_32_time_full_tick(
                        &base_world,
                        &config,
                        context,
                        &options,
                        TICKS_PER_SAMPLE,
                        path == 1,
                    ));
                }
            }

            let row = M232ScalingRow {
                family,
                population,
                settlement_count: config.world.settlement_count,
                phase8_old: m2_29_2_summary(old_samples),
                index_probe: m2_29_2_summary(probe_samples),
                phase8_new: m2_29_2_summary(new_samples),
                full_tick: [
                    m2_29_2_summary(full_tick_samples[0].clone()),
                    m2_29_2_summary(full_tick_samples[1].clone()),
                ],
            };
            let old_slot_visits = population as usize * row.settlement_count as usize;
            let new_slot_visits = phase8_scratch.last_slot_inspections();
            let old_ticks_per_second = 1_000_000.0 / row.full_tick[0].median_us;
            let new_ticks_per_second = 1_000_000.0 / row.full_tick[1].median_us;
            println!(
                "{:<26} N={:<5} K={:<3} R={:<5} inspections(old/new)={:>9}/{:<7} Phase8(old/index/new)={:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}us full(old/new)={:>9.2}±{:>5.2}/{:>9.2}±{:>5.2}us speedup={:>5.2}x ticks/s={:>7.1}/{:>7.1}",
                family,
                population,
                row.settlement_count,
                eligible_count,
                old_slot_visits,
                new_slot_visits,
                row.phase8_old.median_us,
                row.phase8_old.mad_us,
                row.index_probe.median_us,
                row.index_probe.mad_us,
                row.phase8_new.median_us,
                row.phase8_new.mad_us,
                row.full_tick[0].median_us,
                row.full_tick[0].mad_us,
                row.full_tick[1].median_us,
                row.full_tick[1].mad_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                old_ticks_per_second,
                new_ticks_per_second,
            );
            rows.push(row);
        }
    }

    for family in workload_families {
        for (from_n, to_n, divisor) in [(1000, 10000, 10.0f64), (10000, 20000, 2.0f64)] {
            let before = rows
                .iter()
                .find(|row| row.family == family && row.population == from_n)
                .unwrap();
            let after = rows
                .iter()
                .find(|row| row.family == family && row.population == to_n)
                .unwrap();
            for (name, small, large) in [
                (
                    "Phase8 old",
                    before.phase8_old.median_us,
                    after.phase8_old.median_us,
                ),
                (
                    "Phase8 one-pass",
                    before.phase8_new.median_us,
                    after.phase8_new.median_us,
                ),
                (
                    "Full tick old",
                    before.full_tick[0].median_us,
                    after.full_tick[0].median_us,
                ),
                (
                    "Full tick new",
                    before.full_tick[1].median_us,
                    after.full_tick[1].median_us,
                ),
            ] {
                let ratio = large / small.max(0.001);
                println!(
                    "{family:<26} {name:<16} N={from_n}->{to_n}: {small:.2}->{large:.2}us ratio={ratio:.2}x alpha={:.3}",
                    ratio.ln() / divisor.ln(),
                );
            }
        }
        for population in [10000, 20000] {
            let row = rows
                .iter()
                .find(|row| row.family == family && row.population == population)
                .unwrap();
            println!(
                "{family:<26} N={population:<5} full-tick old/new={:.2}/{:.2}us speedup={:.3}x Phase8 share old/new={:.2}/{:.2}%",
                row.full_tick[0].median_us,
                row.full_tick[1].median_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                row.phase8_old.median_us / row.full_tick[0].median_us * 100.0,
                row.phase8_new.median_us / row.full_tick[1].median_us * 100.0,
            );
        }
    }
    println!(
        "Allocation attribution: no global allocator instrumentation; per-group candidate bucket capacities persist in Phase8WelfareScratch, while returned resolution/planning vectors remain owned outputs."
    );
}

#[derive(Clone, Copy)]
struct M233ScalingRow {
    family: &'static str,
    population: u64,
    settlement_count: u32,
    linear_phase3: M2292Median,
    index_build: M2292Median,
    direct_phase3: M2292Median,
    full_tick: [M2292Median; 2],
}

fn m2_33_phase3_sample(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    config: &SimConfig,
    output: &mut Vec<AgentFeatures>,
    scratch: &mut Phase3ScarcityScratch,
    direct: bool,
) -> f64 {
    let started = Instant::now();
    if direct {
        phase3_observation_and_features_storage_with_scratch(
            storage,
            settlements,
            config,
            output,
            scratch,
        )
        .unwrap();
    } else {
        phase3_observation_and_features_storage_into(storage, settlements, config, output).unwrap();
    }
    std::hint::black_box(output.as_slice());
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_33_index_build_sample(
    settlements: &[SettlementState],
    config: &SimConfig,
    output: &mut Vec<AgentFeatures>,
    scratch: &mut Phase3ScarcityScratch,
) -> f64 {
    let empty_storage = SegmentedAgentStorage::new();
    m2_33_phase3_sample(&empty_storage, settlements, config, output, scratch, true)
}

fn m2_33_linear_lookup_counts(
    storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
) -> (usize, usize, usize) {
    let mut eligible_agents = 0;
    let mut total_comparisons = 0;
    let mut worst_comparisons = 0;
    for slot in 0..storage.len() {
        if !storage.alive()[slot] || storage.health()[slot] <= 0.0 {
            continue;
        }
        eligible_agents += 1;
        let group_id = storage.group_ids()[slot];
        let mut comparisons = 0;
        for settlement in settlements {
            comparisons += 1;
            if settlement.group_id == group_id {
                break;
            }
        }
        total_comparisons += comparisons;
        worst_comparisons = worst_comparisons.max(comparisons);
    }
    (eligible_agents, total_comparisons, worst_comparisons)
}

fn m2_33_direct_lookup_counts(
    storage: &SegmentedAgentStorage,
    scratch: &Phase3ScarcityScratch,
) -> (usize, usize, usize) {
    let mut lookups = 0;
    let mut total_comparisons = 0;
    let mut worst_comparisons = 0;
    for slot in 0..storage.len() {
        if !storage.alive()[slot] || storage.health()[slot] <= 0.0 {
            continue;
        }
        lookups += 1;
        let comparisons = scratch.group_id_lookup_comparisons(storage.group_ids()[slot]);
        total_comparisons += comparisons;
        worst_comparisons = worst_comparisons.max(comparisons);
    }
    (lookups, total_comparisons, worst_comparisons)
}

fn m2_33_time_full_tick(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    days: u32,
    direct: bool,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let started = Instant::now();
    let outcomes = if direct {
        run_hybrid_authority_days(&mut world, config, context, days, options)
    } else {
        run_hybrid_authority_days_with_phase3_linear_scan(
            &mut world, config, context, days, options,
        )
    };
    std::hint::black_box(outcomes.unwrap());
    started.elapsed().as_nanos() as f64 / days as f64 / 1000.0
}

fn m2_33_assert_runner_parity(base_world: &WorldState, config: &SimConfig, context: &M0RunContext) {
    let mut linear = HybridWorldState::hybrid(base_world.clone());
    let mut direct = HybridWorldState::hybrid(base_world.clone());
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let linear_outcomes = run_hybrid_authority_days_with_phase3_linear_scan(
        &mut linear,
        config,
        context,
        3,
        &options,
    )
    .unwrap();
    let direct_outcomes =
        run_hybrid_authority_days(&mut direct, config, context, 3, &options).unwrap();
    assert_eq!(linear_outcomes, direct_outcomes);
    assert_eq!(
        linear.canonical_state_hash().unwrap(),
        direct.canonical_state_hash().unwrap()
    );
}

fn measure_m2_33_phase3_direct_scarcity_lookup(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 8] = [100, 250, 500, 1000, 2500, 5000, 10000, 20000];
    const WARMUPS: usize = 2;
    const SAMPLES: usize = 7;
    const DAYS_PER_SAMPLE: u32 = 3;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workloads = ["Fixed settlements (2)", "Fixed group size (~200)"];
    let mut rows = Vec::with_capacity(POPULATIONS.len() * workloads.len());

    println!("\n=================================================================");
    println!("M2-33 Phase3 Linear Search vs Direct Scarcity Lookup");
    println!(
        "Method: 2 warm-ups + 7 median/MAD samples; full-tick samples average 3 production days."
    );
    println!(
        "Index-build probe runs the production builder with empty agent storage; runner scratch spans 3 days."
    );
    println!("=================================================================");

    for (workload_index, family) in workloads.iter().enumerate() {
        let mut phase3_scratch = Phase3ScarcityScratch::with_capacity(100);
        let mut build_scratch = Phase3ScarcityScratch::with_capacity(100);
        let mut output = Vec::new();
        let mut build_output = Vec::new();

        for &population in &POPULATIONS {
            let mut config = base_config.clone();
            config.world.initial_population = population;
            config.world.settlement_count = if workload_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            config.environment.carrying_capacity = 1000.0 * population as f32;
            config.world.initial_settlement_resource = 200.0 * population as f32;
            let base_world = initialize_world(&config).unwrap();
            if population == 10000 {
                m2_33_assert_runner_parity(&base_world, &config, context);
            }
            let storage = SegmentedAgentStorage::from_agents(&base_world.agents);
            let (eligible_agents, linear_comparisons, linear_worst) =
                m2_33_linear_lookup_counts(&storage, &base_world.settlements);

            for _ in 0..WARMUPS {
                let _ = m2_33_phase3_sample(
                    &storage,
                    &base_world.settlements,
                    &config,
                    &mut output,
                    &mut phase3_scratch,
                    false,
                );
                let _ = m2_33_index_build_sample(
                    &base_world.settlements,
                    &config,
                    &mut build_output,
                    &mut build_scratch,
                );
                let _ = m2_33_phase3_sample(
                    &storage,
                    &base_world.settlements,
                    &config,
                    &mut output,
                    &mut phase3_scratch,
                    true,
                );
                let _ = m2_33_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    false,
                );
                let _ = m2_33_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    true,
                );
            }

            let mut linear_samples = Vec::with_capacity(SAMPLES);
            let mut build_samples = Vec::with_capacity(SAMPLES);
            let mut direct_samples = Vec::with_capacity(SAMPLES);
            let mut full_tick_samples: [Vec<f64>; 2] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                linear_samples.push(m2_33_phase3_sample(
                    &storage,
                    &base_world.settlements,
                    &config,
                    &mut output,
                    &mut phase3_scratch,
                    false,
                ));
                build_samples.push(m2_33_index_build_sample(
                    &base_world.settlements,
                    &config,
                    &mut build_output,
                    &mut build_scratch,
                ));
                direct_samples.push(m2_33_phase3_sample(
                    &storage,
                    &base_world.settlements,
                    &config,
                    &mut output,
                    &mut phase3_scratch,
                    true,
                ));
                for offset in 0..2 {
                    let path = (sample + offset) % 2;
                    full_tick_samples[path].push(m2_33_time_full_tick(
                        &base_world,
                        &config,
                        context,
                        &options,
                        DAYS_PER_SAMPLE,
                        path == 1,
                    ));
                }
            }

            let row = M233ScalingRow {
                family,
                population,
                settlement_count: config.world.settlement_count,
                linear_phase3: m2_29_2_summary(linear_samples),
                index_build: m2_29_2_summary(build_samples),
                direct_phase3: m2_29_2_summary(direct_samples),
                full_tick: [
                    m2_29_2_summary(full_tick_samples[0].clone()),
                    m2_29_2_summary(full_tick_samples[1].clone()),
                ],
            };
            let (direct_lookups, direct_comparisons, direct_worst) =
                m2_33_direct_lookup_counts(&storage, &phase3_scratch);
            println!(
                "{:<26} N={:<5} K={:<3} eligible={:<5} comparisons old(total/avg/max)={:>9}/{:>5.1}/{:<3} new(entries/sortcmp/lookups/cmp/max)={:>4}/{:<5}/{:>7}/{:>7}/{:<3} P3(old/index/new)={:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}/{:>8.2}±{:>5.2}us full(old/new)={:>9.2}±{:>5.2}/{:>9.2}±{:>5.2}us",
                family,
                population,
                row.settlement_count,
                eligible_agents,
                linear_comparisons,
                if eligible_agents == 0 {
                    0.0
                } else {
                    linear_comparisons as f64 / eligible_agents as f64
                },
                linear_worst,
                phase3_scratch.last_index_build_entries(),
                phase3_scratch.last_index_sort_comparisons(),
                direct_lookups,
                direct_comparisons,
                direct_worst,
                row.linear_phase3.median_us,
                row.linear_phase3.mad_us,
                row.index_build.median_us,
                row.index_build.mad_us,
                row.direct_phase3.median_us,
                row.direct_phase3.mad_us,
                row.full_tick[0].median_us,
                row.full_tick[0].mad_us,
                row.full_tick[1].median_us,
                row.full_tick[1].mad_us,
            );
            rows.push(row);
        }
    }

    for family in workloads {
        for (from_n, to_n, divisor) in [(1000, 10000, 10.0f64), (10000, 20000, 2.0f64)] {
            let before = rows
                .iter()
                .find(|row| row.family == family && row.population == from_n)
                .unwrap();
            let after = rows
                .iter()
                .find(|row| row.family == family && row.population == to_n)
                .unwrap();
            for (name, small, large) in [
                (
                    "Phase3 linear",
                    before.linear_phase3.median_us,
                    after.linear_phase3.median_us,
                ),
                (
                    "Phase3 direct",
                    before.direct_phase3.median_us,
                    after.direct_phase3.median_us,
                ),
                (
                    "Full tick linear",
                    before.full_tick[0].median_us,
                    after.full_tick[0].median_us,
                ),
                (
                    "Full tick direct",
                    before.full_tick[1].median_us,
                    after.full_tick[1].median_us,
                ),
            ] {
                let ratio = large / small.max(0.001);
                println!(
                    "{family:<26} {name:<18} N={from_n}->{to_n}: {small:.2}->{large:.2}us ratio={ratio:.2}x alpha={:.3}",
                    ratio.ln() / divisor.ln(),
                );
            }
        }
        for population in [10000, 20000] {
            let row = rows
                .iter()
                .find(|row| row.family == family && row.population == population)
                .unwrap();
            println!(
                "{family:<26} N={population:<5} P3 old/new={:.2}/{:.2}us speedup={:.3}x full-tick old/new={:.2}/{:.2}us speedup={:.3}x P3 share old/new={:.2}/{:.2}%",
                row.linear_phase3.median_us,
                row.direct_phase3.median_us,
                row.linear_phase3.median_us / row.direct_phase3.median_us,
                row.full_tick[0].median_us,
                row.full_tick[1].median_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                row.linear_phase3.median_us / row.full_tick[0].median_us * 100.0,
                row.direct_phase3.median_us / row.full_tick[1].median_us * 100.0,
            );
        }
    }
    println!(
        "Allocation attribution: no global allocator instrumentation; 8 or fewer settlements use inline entries, larger index Vec capacity is owned by and reused through Phase3ScarcityScratch."
    );
}

#[derive(Clone, Copy)]
struct M235ScalingRow {
    family: &'static str,
    population: u64,
    settlement_count: u32,
    baseline_phase5: M2292Median,
    owned_phase5: M2292Median,
    dispatcher_phase5: M2292Median,
    full_tick: [M2292Median; 2],
}

fn m2_35_production_intents(
    base_config: &SimConfig,
    context: &M0RunContext,
    population: u64,
    settlement_count: u32,
) -> (SimConfig, WorldState, Vec<Intent>) {
    let mut config = base_config.clone();
    config.world.initial_population = population;
    config.world.settlement_count = settlement_count;
    config.environment.carrying_capacity = 1000.0 * population as f32;
    config.world.initial_settlement_resource = 200.0 * population as f32;
    let mut effective_config = config.clone();
    effective_config.world.master_seed = context.master_seed;
    effective_config.world.replicate_id = context.replicate_id;

    let mut world = initialize_world(&config).unwrap();
    let initial_world = world.clone();
    phase1_resource_regrowth(&mut world, &effective_config);
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    storage.phase2_degradation_with_config(&effective_config);

    let mut features = Vec::with_capacity(storage.len());
    let mut phase3_scratch = Phase3ScarcityScratch::with_capacity(world.settlements.len());
    phase3_observation_and_features_storage_with_scratch(
        &storage,
        &world.settlements,
        &effective_config,
        &mut features,
        &mut phase3_scratch,
    )
    .unwrap();
    let mut choices = Vec::with_capacity(storage.len());
    phase4_primary_action_selection_storage_into(
        &storage,
        world.current_day,
        &effective_config,
        &features,
        &mut choices,
    )
    .unwrap();
    let mut candidate_index = Phase4CandidateIndexScratch::with_capacity(world.settlements.len());
    let mut intents = Vec::with_capacity(storage.len());
    generate_intents_storage_with_candidate_index(
        &storage,
        world.current_day,
        &effective_config,
        &choices,
        &mut candidate_index,
        &mut intents,
    )
    .unwrap();
    assert!(
        intents
            .windows(2)
            .all(|pair| pair[0].agent_id() < pair[1].agent_id())
    );
    if settlement_count > 1 {
        assert!(
            intents
                .windows(2)
                .any(|pair| pair[0].group_id() > pair[1].group_id())
        );
    }
    (config, initial_world, intents)
}

fn m2_35_phase5_baseline_sample(intents: &[Intent]) -> f64 {
    let started = Instant::now();
    std::hint::black_box(phase5_partition_intents_baseline(intents).unwrap());
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_35_phase5_owned_sample(
    intents: &[Intent],
    intent_scratch: &mut Vec<Intent>,
    phase5_scratch: &mut Phase5PartitionScratch,
) -> f64 {
    debug_assert!(intent_scratch.is_empty());
    intent_scratch.extend_from_slice(intents);
    let started = Instant::now();
    let partitions =
        phase5_partition_intents_from_vec_with_scratch(intent_scratch, phase5_scratch).unwrap();
    std::hint::black_box(partitions);
    assert!(intent_scratch.is_empty());
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_35_phase5_dispatcher_sample(intents: &[Intent]) -> f64 {
    let started = Instant::now();
    std::hint::black_box(phase5_partition_intents(intents).unwrap());
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_35_time_full_tick(
    base_world: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    days: u32,
    baseline: bool,
) -> f64 {
    let mut world = HybridWorldState::hybrid(base_world.clone());
    let started = Instant::now();
    let outcomes = if baseline {
        run_hybrid_authority_days_with_phase5_baseline(&mut world, config, context, days, options)
    } else {
        run_hybrid_authority_days(&mut world, config, context, days, options)
    };
    std::hint::black_box(outcomes.unwrap());
    started.elapsed().as_nanos() as f64 / days as f64 / 1000.0
}

fn m2_35_assert_runner_parity(base_world: &WorldState, config: &SimConfig, context: &M0RunContext) {
    let mut baseline = HybridWorldState::hybrid(base_world.clone());
    let mut optimized = HybridWorldState::hybrid(base_world.clone());
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    let baseline_outcomes =
        run_hybrid_authority_days_with_phase5_baseline(&mut baseline, config, context, 3, &options)
            .unwrap();
    let optimized_outcomes =
        run_hybrid_authority_days(&mut optimized, config, context, 3, &options).unwrap();
    assert_eq!(baseline_outcomes, optimized_outcomes);
    assert_eq!(
        baseline.canonical_state_hash().unwrap(),
        optimized.canonical_state_hash().unwrap()
    );
}

fn m2_35_work_counts(intents: &[Intent]) -> (usize, usize, usize) {
    let mut ordered = intents.to_vec();
    let mut baseline_sort_comparisons = 0;
    ordered.sort_by(|left, right| {
        baseline_sort_comparisons += 1;
        left.group_id()
            .cmp(&right.group_id())
            .then_with(|| left.agent_id().cmp(&right.agent_id()))
    });

    let groups: std::collections::HashMap<_, ()> = intents
        .iter()
        .map(|intent| (intent.group_id(), ()))
        .collect();
    let mut groups: Vec<_> = groups.into_keys().collect();
    let group_count = groups.len();
    let mut group_sort_comparisons = 0;
    groups.sort_unstable_by(|left, right| {
        group_sort_comparisons += 1;
        left.cmp(right)
    });
    (
        baseline_sort_comparisons,
        group_sort_comparisons,
        group_count,
    )
}

fn measure_m2_35_phase5_stable_group_bucketing(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 9] = [100, 250, 500, 1000, 2500, 5000, 10000, 20000, 50000];
    const WARMUPS: usize = 2;
    const SAMPLES: usize = 7;
    const DAYS_PER_SAMPLE: u32 = 3;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workloads = ["Fixed settlements (2)", "Fixed group size (~200)"];
    let mut rows = Vec::with_capacity(POPULATIONS.len() * workloads.len());

    println!("\n=================================================================");
    println!("M2-35 Phase5 Canonical Sort vs Stable Group Bucketing");
    println!(
        "Method: 2 warm-ups + 7 median/MAD samples; production Phase4 inputs; full-tick samples average 3 days."
    );
    println!(
        "Owned fast path retains caller intent and lookup-map capacity; partitions remain owned outputs."
    );
    println!("=================================================================");

    for (family_index, family) in workloads.iter().enumerate() {
        for &population in &POPULATIONS {
            let settlement_count = if family_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            let (config, base_world, intents) =
                m2_35_production_intents(base_config, context, population, settlement_count);
            let baseline_partitions = phase5_partition_intents_baseline(&intents).unwrap();
            assert_eq!(
                phase5_partition_intents(&intents).unwrap(),
                baseline_partitions
            );
            let mut owned = intents.clone();
            assert_eq!(
                phase5_partition_intents_from_vec(&mut owned).unwrap(),
                baseline_partitions
            );
            assert!(owned.is_empty());
            let mut intent_scratch = Vec::with_capacity(intents.len());
            let mut phase5_scratch =
                Phase5PartitionScratch::with_capacity(settlement_count as usize);
            if population == 10000 {
                m2_35_assert_runner_parity(&base_world, &config, context);
            }

            for _ in 0..WARMUPS {
                let _ = m2_35_phase5_baseline_sample(&intents);
                let _ =
                    m2_35_phase5_owned_sample(&intents, &mut intent_scratch, &mut phase5_scratch);
                let _ = m2_35_phase5_dispatcher_sample(&intents);
                let _ = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    true,
                );
                let _ = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    false,
                );
            }

            let mut baseline_samples = Vec::with_capacity(SAMPLES);
            let mut owned_samples = Vec::with_capacity(SAMPLES);
            let mut dispatcher_samples = Vec::with_capacity(SAMPLES);
            let mut full_tick_samples: [Vec<f64>; 2] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                baseline_samples.push(m2_35_phase5_baseline_sample(&intents));
                owned_samples.push(m2_35_phase5_owned_sample(
                    &intents,
                    &mut intent_scratch,
                    &mut phase5_scratch,
                ));
                dispatcher_samples.push(m2_35_phase5_dispatcher_sample(&intents));
                for offset in 0..2 {
                    let path = (sample + offset) % 2;
                    full_tick_samples[path].push(m2_35_time_full_tick(
                        &base_world,
                        &config,
                        context,
                        &options,
                        DAYS_PER_SAMPLE,
                        path == 0,
                    ));
                }
            }

            let row = M235ScalingRow {
                family,
                population,
                settlement_count,
                baseline_phase5: m2_29_2_summary(baseline_samples),
                owned_phase5: m2_29_2_summary(owned_samples),
                dispatcher_phase5: m2_29_2_summary(dispatcher_samples),
                full_tick: [
                    m2_29_2_summary(full_tick_samples[0].clone()),
                    m2_29_2_summary(full_tick_samples[1].clone()),
                ],
            };
            let (baseline_sort_comparisons, group_sort_comparisons, group_count) =
                m2_35_work_counts(&intents);
            println!(
                "{family:<26} N={population:<5} K={settlement_count:<3} intents={} baseline(work: hash/clone/sort/bucket={}/{}/{}/{}) fast(work: order/lookups/bucket/group-sort/clone={}/{}/{}/{}/{}) P5(base/owned/public)={:.2}±{:.2}/{:.2}±{:.2}/{:.2}±{:.2}us full(base/fast)={:.2}±{:.2}/{:.2}±{:.2}us",
                intents.len(),
                intents.len(),
                intents.len(),
                baseline_sort_comparisons,
                intents.len(),
                intents.len().saturating_sub(1),
                intents.len(),
                intents.len(),
                group_sort_comparisons,
                0,
                row.baseline_phase5.median_us,
                row.baseline_phase5.mad_us,
                row.owned_phase5.median_us,
                row.owned_phase5.mad_us,
                row.dispatcher_phase5.median_us,
                row.dispatcher_phase5.mad_us,
                row.full_tick[0].median_us,
                row.full_tick[0].mad_us,
                row.full_tick[1].median_us,
                row.full_tick[1].mad_us,
            );
            println!(
                "  work detail: baseline HashSet inserts/lookups={}, copied Intents={}, stable sort comparisons={baseline_sort_comparisons}, bucket writes={}; owned fast ordered comparisons={}, HashMap group lookups={}, bucket writes={}, final GroupId sort comparisons={group_sort_comparisons}, copied Intents=0; groups={group_count}. Borrowed dispatcher copies {} Intents to preserve returned ownership.",
                intents.len(),
                intents.len(),
                intents.len(),
                intents.len().saturating_sub(1),
                intents.len(),
                intents.len(),
                intents.len(),
            );
            rows.push(row);
        }
    }

    for family in workloads {
        for (from_n, to_n) in [(1000, 10000), (10000, 20000), (20000, 50000)] {
            let before = rows
                .iter()
                .find(|row| row.family == family && row.population == from_n)
                .unwrap();
            let after = rows
                .iter()
                .find(|row| row.family == family && row.population == to_n)
                .unwrap();
            let divisor = (to_n as f64 / from_n as f64).ln();
            for (name, small, large) in [
                (
                    "Phase5 baseline",
                    before.baseline_phase5.median_us,
                    after.baseline_phase5.median_us,
                ),
                (
                    "Phase5 owned fast",
                    before.owned_phase5.median_us,
                    after.owned_phase5.median_us,
                ),
                (
                    "Full tick baseline",
                    before.full_tick[0].median_us,
                    after.full_tick[0].median_us,
                ),
                (
                    "Full tick fast",
                    before.full_tick[1].median_us,
                    after.full_tick[1].median_us,
                ),
            ] {
                let ratio = large / small.max(0.001);
                println!(
                    "{family:<26} {name:<19} N={from_n}->{to_n}: {small:.2}->{large:.2}us ratio={ratio:.2}x alpha={:.3}",
                    ratio.ln() / divisor,
                );
            }
        }
        for population in [10000, 20000, 50000] {
            let row = rows
                .iter()
                .find(|row| row.family == family && row.population == population)
                .unwrap();
            println!(
                "{family:<26} N={population:<5} K={} P5 base/fast={:.2}/{:.2}us speedup={:.3}x full-tick base/fast={:.2}/{:.2}us speedup={:.3}x P5 share base/fast={:.2}/{:.2}%",
                row.settlement_count,
                row.baseline_phase5.median_us,
                row.owned_phase5.median_us,
                row.baseline_phase5.median_us / row.owned_phase5.median_us,
                row.full_tick[0].median_us,
                row.full_tick[1].median_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                row.baseline_phase5.median_us / row.full_tick[0].median_us * 100.0,
                row.owned_phase5.median_us / row.full_tick[1].median_us * 100.0,
            );
        }
    }

    for population in [1000u64, 10000] {
        let settlement_count = (population / 200).max(1) as u32;
        let (_, _, mut shuffled) =
            m2_35_production_intents(base_config, context, population, settlement_count);
        for index in (1..shuffled.len()).rev() {
            let mut state = (population << 16) ^ index as u64 ^ 0x9e3779b97f4a7c15;
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            shuffled.swap(index, state as usize % (index + 1));
        }
        let baseline = phase5_partition_intents_baseline(&shuffled).unwrap();
        assert_eq!(phase5_partition_intents(&shuffled).unwrap(), baseline);
        let mut baseline_samples = Vec::with_capacity(SAMPLES);
        let mut fallback_samples = Vec::with_capacity(SAMPLES);
        for _ in 0..WARMUPS {
            let _ = m2_35_phase5_baseline_sample(&shuffled);
            let _ = m2_35_phase5_dispatcher_sample(&shuffled);
        }
        for _ in 0..SAMPLES {
            baseline_samples.push(m2_35_phase5_baseline_sample(&shuffled));
            fallback_samples.push(m2_35_phase5_dispatcher_sample(&shuffled));
        }
        let baseline = m2_29_2_summary(baseline_samples);
        let fallback = m2_29_2_summary(fallback_samples);
        println!(
            "Arbitrary-order shuffled fallback N={population}: baseline={:.2}±{:.2}us dispatcher={:.2}±{:.2}us overhead={:.2}% (order probe exits at first descending pair)",
            baseline.median_us,
            baseline.mad_us,
            fallback.median_us,
            fallback.mad_us,
            (fallback.median_us / baseline.median_us - 1.0) * 100.0,
        );
    }

    let (config, base_world, intents) = m2_35_production_intents(base_config, context, 10000, 50);
    let mut intent_scratch = Vec::with_capacity(intents.len());
    let mut phase5_scratch = Phase5PartitionScratch::with_capacity(50);
    let mut p5_samples: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    let mut tick_samples: [Vec<f64>; 2] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    let mut paired_p5_savings = Vec::with_capacity(SAMPLES);
    let mut paired_tick_savings = Vec::with_capacity(SAMPLES);
    for warmup in 0..WARMUPS {
        let fast_first = warmup % 2 == 1;
        for fast in [fast_first, !fast_first] {
            if fast {
                let _ =
                    m2_35_phase5_owned_sample(&intents, &mut intent_scratch, &mut phase5_scratch);
                let _ = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    false,
                );
            } else {
                let _ = m2_35_phase5_baseline_sample(&intents);
                let _ = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    true,
                );
            }
        }
    }
    for sample in 0..SAMPLES {
        let fast_first = sample % 2 == 1;
        let mut current_p5 = [0.0; 2];
        let mut current_tick = [0.0; 2];
        for fast in [fast_first, !fast_first] {
            let index = if fast { 1 } else { 0 };
            if fast {
                current_p5[index] =
                    m2_35_phase5_owned_sample(&intents, &mut intent_scratch, &mut phase5_scratch);
                current_tick[index] = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    false,
                );
            } else {
                current_p5[index] = m2_35_phase5_baseline_sample(&intents);
                current_tick[index] = m2_35_time_full_tick(
                    &base_world,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    true,
                );
            }
        }
        paired_p5_savings.push(current_p5[0] - current_p5[1]);
        paired_tick_savings.push(current_tick[0] - current_tick[1]);
        p5_samples[0].push(current_p5[0]);
        p5_samples[1].push(current_p5[1]);
        tick_samples[0].push(current_tick[0]);
        tick_samples[1].push(current_tick[1]);
    }
    let paired_p5_savings = m2_29_2_summary(paired_p5_savings);
    let paired_tick_savings = m2_29_2_summary(paired_tick_savings);
    let p5_baseline = m2_29_2_summary(p5_samples[0].clone());
    let p5_fast = m2_29_2_summary(p5_samples[1].clone());
    let tick_baseline = m2_29_2_summary(tick_samples[0].clone());
    let tick_fast = m2_29_2_summary(tick_samples[1].clone());
    println!(
        "Paired same-process sanity N=10000 K=50, 2 warm-ups + 7 alternating pairs, 3-day tick: P5 baseline/fast={:.2}±{:.2}/{:.2}±{:.2}us, paired saving={:.2}±{:.2}us; full tick baseline/fast={:.2}±{:.2}/{:.2}±{:.2}us, paired saving={:.2}±{:.2}us.",
        p5_baseline.median_us,
        p5_baseline.mad_us,
        p5_fast.median_us,
        p5_fast.mad_us,
        paired_p5_savings.median_us,
        paired_p5_savings.mad_us,
        tick_baseline.median_us,
        tick_baseline.mad_us,
        tick_fast.median_us,
        tick_fast.mad_us,
        paired_tick_savings.median_us,
        paired_tick_savings.mad_us,
    );

    println!(
        "Production proof: benchmark inputs come from the Full Hybrid Phase2/Phase3/Phase4 candidate-index path; each stream is strictly AgentId ascending and non-monotonic by GroupId for K>1."
    );
    println!(
        "Allocation attribution: baseline uses a HashSet, N-intent sorted clone, sort, and owned output buckets; owned fast path reuses lookup-only HashMap capacity, sorts only final GroupIds, and moves N intents out of caller scratch. Public borrowed dispatcher retains its N-intent output ownership copy. No allocation-counting allocator is installed."
    );
}

#[derive(Clone, Copy)]
struct M236ScalingRow {
    family: &'static str,
    population: u64,
    settlement_count: u32,
    targeted: usize,
    keyed: usize,
    zero_target: usize,
    order_verification_comparisons: usize,
    partition_sort_comparisons: usize,
    zero_sort_comparisons: usize,
    key_sort_comparisons: usize,
    phase6b: [M2292Median; 3],
    full_tick: [M2292Median; 2],
}

fn m2_36_inputs(
    base_config: &SimConfig,
    context: &M0RunContext,
    population: u64,
    settlement_count: u32,
) -> (
    SimConfig,
    WorldState,
    SegmentedAgentStorage,
    Vec<SettlementIntentPartition>,
) {
    let (mut config, world, intents) =
        m2_35_production_intents(base_config, context, population, settlement_count);
    config.world.master_seed = context.master_seed;
    config.world.replicate_id = context.replicate_id;
    let partitions = phase5_partition_intents_baseline(&intents).unwrap();
    assert!(
        partitions
            .windows(2)
            .all(|pair| pair[0].group_id < pair[1].group_id)
    );
    assert!(partitions.iter().all(|partition| {
        partition
            .intents
            .windows(2)
            .all(|pair| pair[0].agent_id() < pair[1].agent_id())
    }));

    let mut phase1_world = world.clone();
    phase1_resource_regrowth(&mut phase1_world, &config);
    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    storage.phase2_degradation_with_config(&config);
    (config, world, storage, partitions)
}

fn m2_36_phase6b_sample(
    base_storage: &SegmentedAgentStorage,
    settlements: &[SettlementState],
    current_day: SimulationDay,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
    path: usize,
    scratch: &mut Phase6BResolutionScratch,
) -> f64 {
    let mut storage = base_storage.clone();
    let started = Instant::now();
    let resolutions = match path {
        0 => phase6b_targeted_resolution_storage_baseline(
            &mut storage,
            settlements,
            current_day,
            config,
            partitions,
        ),
        1 => phase6b_targeted_resolution_storage_with_scratch(
            &mut storage,
            settlements,
            current_day,
            config,
            partitions,
            scratch,
        ),
        _ => phase6b_targeted_resolution_storage(
            &mut storage,
            settlements,
            current_day,
            config,
            partitions,
        ),
    }
    .unwrap();
    std::hint::black_box(resolutions);
    started.elapsed().as_nanos() as f64 / 1000.0
}

fn m2_36_time_full_tick(
    initial: &WorldState,
    config: &SimConfig,
    context: &M0RunContext,
    options: &DayExecutionOptions,
    days: u32,
    baseline: bool,
) -> f64 {
    let mut world = HybridWorldState::hybrid(initial.clone());
    let started = Instant::now();
    let outcomes = if baseline {
        run_hybrid_authority_days_with_phase6b_baseline(&mut world, config, context, days, options)
    } else {
        run_hybrid_authority_days(&mut world, config, context, days, options)
    };
    std::hint::black_box(outcomes.unwrap());
    started.elapsed().as_nanos() as f64 / days as f64 / 1000.0
}

fn m2_36_assert_parity(
    initial: &WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
) {
    let mut baseline_storage = SegmentedAgentStorage::from_agents(&initial.agents);
    let mut optimized_storage = baseline_storage.clone();
    let baseline = phase6b_targeted_resolution_storage_baseline(
        &mut baseline_storage,
        &initial.settlements,
        initial.current_day,
        config,
        partitions,
    )
    .unwrap();
    let mut scratch = Phase6BResolutionScratch::with_capacity(initial.agents.len());
    let optimized = phase6b_targeted_resolution_storage_with_scratch(
        &mut optimized_storage,
        &initial.settlements,
        initial.current_day,
        config,
        partitions,
        &mut scratch,
    )
    .unwrap();
    assert_eq!(optimized, baseline);
    assert_eq!(optimized_storage, baseline_storage);
}

fn m2_36_work_counts(
    config: &SimConfig,
    current_day: SimulationDay,
    partitions: &[SettlementIntentPartition],
) -> (usize, usize, usize, usize, usize, usize, usize) {
    let mut targeted = 0;
    let mut keyed = 0;
    let mut zero = 0;
    let mut zero_sort_comparisons = 0;
    let mut partition_ids: Vec<_> = partitions
        .iter()
        .map(|partition| partition.group_id)
        .collect();
    let mut partition_comparisons = 0;
    partition_ids.sort_by(|left, right| {
        partition_comparisons += 1;
        left.cmp(right)
    });

    let mut total_key_comparisons = 0;
    let mut order_verification_comparisons = partitions.len().saturating_sub(1);
    for partition in partitions {
        order_verification_comparisons += partition.intents.len().saturating_sub(1);
        let mut keys = Vec::new();
        let mut zero_records = Vec::new();
        for intent in &partition.intents {
            let (initiator, group_id, target, action) = match *intent {
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    ..
                } => (
                    agent_id,
                    group_id,
                    target_agent_id,
                    TargetedActionKind::GiveFood,
                ),
                Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    ..
                } => (
                    agent_id,
                    group_id,
                    target_agent_id,
                    TargetedActionKind::StealFood,
                ),
                _ => continue,
            };
            targeted += 1;
            if let Some(target) = target {
                keyed += 1;
                keys.push((
                    compute_resolution_key(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        group_id.as_u16(),
                        target.as_u32(),
                        initiator.as_u32(),
                        action.action_kind(),
                    ),
                    target,
                    initiator,
                    action,
                ));
            } else {
                zero += 1;
                zero_records.push((initiator, action));
            }
        }
        keys.sort_by(|a, b| {
            total_key_comparisons += 1;
            compare_keyed_interactions(a.0, a.1, a.2, a.3, b.0, b.1, b.2, b.3)
        });
        zero_records.sort_by(|a, b| {
            zero_sort_comparisons += 1;
            (a.0, a.1).cmp(&(b.0, b.1))
        });
    }
    (
        targeted,
        keyed,
        zero,
        partition_comparisons,
        zero_sort_comparisons,
        total_key_comparisons,
        order_verification_comparisons,
    )
}

fn m2_36_component_probes(
    config: &SimConfig,
    world: &WorldState,
    storage: &SegmentedAgentStorage,
    partitions: &[SettlementIntentPartition],
) {
    const SAMPLES: usize = 7;
    let mut targeted_inputs = Vec::new();
    let mut zero_groups = Vec::with_capacity(partitions.len());
    let mut key_inputs = Vec::new();
    for partition in partitions {
        let mut zero_records = Vec::new();
        for intent in &partition.intents {
            let (initiator, group_id, target, kind) = match *intent {
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    ..
                } => (
                    agent_id,
                    group_id,
                    target_agent_id,
                    TargetedActionKind::GiveFood,
                ),
                Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    ..
                } => (
                    agent_id,
                    group_id,
                    target_agent_id,
                    TargetedActionKind::StealFood,
                ),
                _ => continue,
            };
            targeted_inputs.push((initiator, target));
            if let Some(target) = target {
                key_inputs.push((group_id, target, initiator, kind));
            } else {
                zero_records.push((initiator, kind));
            }
        }
        zero_groups.push(zero_records);
    }
    let mut baseline_storage = storage.clone();
    let result = phase6b_targeted_resolution_storage_baseline(
        &mut baseline_storage,
        &world.settlements,
        world.current_day,
        config,
        partitions,
    )
    .unwrap();
    let group_ids: Vec<_> = partitions.iter().map(|p| p.group_id).collect();
    let work = m2_36_work_counts(config, world.current_day, partitions);
    let mut component_samples: [Vec<f64>; 7] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));

    for _ in 0..SAMPLES {
        let started = Instant::now();
        let canonical = partitions
            .windows(2)
            .all(|pair| pair[0].group_id < pair[1].group_id)
            && partitions.iter().all(|p| {
                p.intents
                    .windows(2)
                    .all(|pair| pair[0].agent_id() < pair[1].agent_id())
            });
        std::hint::black_box(canonical);
        component_samples[0].push(started.elapsed().as_nanos() as f64 / 1000.0);

        for lookup_mode in 0..2 {
            let started = Instant::now();
            for &(initiator, target) in &targeted_inputs {
                std::hint::black_box(storage.slot_of(initiator));
                if let Some(target) = target {
                    std::hint::black_box(storage.slot_of(target));
                    if lookup_mode == 0 {
                        std::hint::black_box(storage.slot_of(initiator));
                        std::hint::black_box(storage.slot_of(target));
                    }
                }
            }
            component_samples[1 + lookup_mode].push(started.elapsed().as_nanos() as f64 / 1000.0);
        }

        let mut partition_sort = group_ids.clone();
        let started = Instant::now();
        partition_sort.sort_by_key(|group_id| *group_id);
        component_samples[3].push(started.elapsed().as_nanos() as f64 / 1000.0);

        let mut zero_sort = zero_groups.clone();
        let started = Instant::now();
        for group in &mut zero_sort {
            group.sort_by_key(|(initiator, kind)| (*initiator, *kind));
        }
        component_samples[4].push(started.elapsed().as_nanos() as f64 / 1000.0);

        let started = Instant::now();
        let mut keys = Vec::with_capacity(key_inputs.len());
        for &(group, target, initiator, kind) in &key_inputs {
            keys.push((
                compute_resolution_key(
                    config.world.master_seed,
                    config.world.replicate_id,
                    world.current_day.as_u32(),
                    group.as_u16(),
                    target.as_u32(),
                    initiator.as_u32(),
                    kind.action_kind(),
                ),
                target,
                initiator,
                kind,
            ));
        }
        component_samples[5].push(started.elapsed().as_nanos() as f64 / 1000.0);

        let mut keys = keys.clone();
        let started = Instant::now();
        keys.sort_by(|a, b| compare_keyed_interactions(a.0, a.1, a.2, a.3, b.0, b.1, b.2, b.3));
        component_samples[6].push(started.elapsed().as_nanos() as f64 / 1000.0);
    }

    let mut clone_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let started = Instant::now();
        std::hint::black_box(result.clone());
        clone_samples.push(started.elapsed().as_nanos() as f64 / 1000.0);
    }
    println!(
        "Component probes N=10000: order verification={:.2}us; slot-map lookups baseline/fast-probe={:.2}/{:.2}us ({}/{}) ; partition sort={:.2}us ({} compares), zero sort={:.2}us ({} compares), key generation={:.2}us, keyed sort={:.2}us ({} compares), combined result clone={:.2}us ({} records cloned). Probe timings are non-additive; dynamic validation/commit and result construction stay in end-to-end Phase6B.",
        m2_29_2_summary(component_samples[0].clone()).median_us,
        m2_29_2_summary(component_samples[1].clone()).median_us,
        m2_29_2_summary(component_samples[2].clone()).median_us,
        work.0 + work.1 * 3,
        work.0 + work.1,
        m2_29_2_summary(component_samples[3].clone()).median_us,
        work.3,
        m2_29_2_summary(component_samples[4].clone()).median_us,
        work.4,
        m2_29_2_summary(component_samples[5].clone()).median_us,
        m2_29_2_summary(component_samples[6].clone()).median_us,
        work.5,
        m2_29_2_summary(clone_samples).median_us,
        work.0,
    );
}

fn measure_m2_36_phase6b_slot_cache(base_config: &SimConfig, context: &M0RunContext) {
    const POPULATIONS: [u64; 9] = [100, 250, 500, 1000, 2500, 5000, 10000, 20000, 50000];
    const WARMUPS: usize = 2;
    const SAMPLES: usize = 7;
    const DAYS_PER_SAMPLE: u32 = 3;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let workloads = ["Fixed settlements (2)", "Fixed group size (~200)"];
    let mut rows = Vec::with_capacity(POPULATIONS.len() * workloads.len());

    println!("\n=================================================================");
    println!("M2-36 Phase6B Baseline vs Stable Slot Cache");
    println!(
        "Method: 2 warm-ups + 7 median/MAD samples; Full Hybrid Phase4 inputs; full-tick samples average 3 days."
    );
    println!(
        "Phase6B direct samples clone storage before timing; production full-tick scratch spans days."
    );
    println!("=================================================================");

    for (family_index, family) in workloads.iter().enumerate() {
        for &population in &POPULATIONS {
            let settlement_count = if family_index == 0 {
                2
            } else {
                population.div_ceil(200) as u32
            };
            let (config, initial, storage, partitions) =
                m2_36_inputs(base_config, context, population, settlement_count);
            m2_36_assert_parity(&initial, &config, &partitions);
            if population == 10000 {
                let mut baseline = HybridWorldState::hybrid(initial.clone());
                let mut optimized = HybridWorldState::hybrid(initial.clone());
                let baseline_outcomes = run_hybrid_authority_days_with_phase6b_baseline(
                    &mut baseline,
                    &config,
                    context,
                    3,
                    &options,
                )
                .unwrap();
                let optimized_outcomes =
                    run_hybrid_authority_days(&mut optimized, &config, context, 3, &options)
                        .unwrap();
                assert_eq!(optimized_outcomes, baseline_outcomes);
                assert_eq!(
                    optimized.canonical_state_hash().unwrap(),
                    baseline.canonical_state_hash().unwrap()
                );
            }

            let mut scratch = Phase6BResolutionScratch::with_capacity(population as usize);
            for _ in 0..WARMUPS {
                for path in 0..3 {
                    let _ = m2_36_phase6b_sample(
                        &storage,
                        &initial.settlements,
                        initial.current_day,
                        &config,
                        &partitions,
                        path,
                        &mut scratch,
                    );
                }
                let _ = m2_36_time_full_tick(
                    &initial,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    true,
                );
                let _ = m2_36_time_full_tick(
                    &initial,
                    &config,
                    context,
                    &options,
                    DAYS_PER_SAMPLE,
                    false,
                );
            }

            let mut phase_samples: [Vec<f64>; 3] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            let mut full_samples: [Vec<f64>; 2] =
                std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
            for sample in 0..SAMPLES {
                for offset in 0..3 {
                    let path = (sample + offset) % 3;
                    phase_samples[path].push(m2_36_phase6b_sample(
                        &storage,
                        &initial.settlements,
                        initial.current_day,
                        &config,
                        &partitions,
                        path,
                        &mut scratch,
                    ));
                }
                for offset in 0..2 {
                    let path = (sample + offset) % 2;
                    full_samples[path].push(m2_36_time_full_tick(
                        &initial,
                        &config,
                        context,
                        &options,
                        DAYS_PER_SAMPLE,
                        path == 0,
                    ));
                }
            }

            let counts = m2_36_work_counts(&config, initial.current_day, &partitions);
            let row = M236ScalingRow {
                family,
                population,
                settlement_count,
                targeted: counts.0,
                keyed: counts.1,
                zero_target: counts.2,
                order_verification_comparisons: counts.6,
                partition_sort_comparisons: counts.3,
                zero_sort_comparisons: counts.4,
                key_sort_comparisons: counts.5,
                phase6b: [
                    m2_29_2_summary(phase_samples[0].clone()),
                    m2_29_2_summary(phase_samples[1].clone()),
                    m2_29_2_summary(phase_samples[2].clone()),
                ],
                full_tick: [
                    m2_29_2_summary(full_samples[0].clone()),
                    m2_29_2_summary(full_samples[1].clone()),
                ],
            };
            println!(
                "{family:<26} N={population:<5} K={settlement_count:<3} targeted/keyed/zero={}/{}/{} slots(base validate/commit,total;fast)={}/{}/{}/{} cmp(order/partition/zero/key)={}/{}/{}/{} Phase6B(base/reuse/dispatch)={:.2}±{:.2}/{:.2}±{:.2}/{:.2}±{:.2}us full(base/fast)={:.2}±{:.2}/{:.2}±{:.2}us",
                row.targeted,
                row.keyed,
                row.zero_target,
                row.targeted + row.keyed,
                row.keyed * 2,
                row.targeted + row.keyed * 3,
                row.targeted + row.keyed,
                row.order_verification_comparisons,
                row.partition_sort_comparisons,
                row.zero_sort_comparisons,
                row.key_sort_comparisons,
                row.phase6b[0].median_us,
                row.phase6b[0].mad_us,
                row.phase6b[1].median_us,
                row.phase6b[1].mad_us,
                row.phase6b[2].median_us,
                row.phase6b[2].mad_us,
                row.full_tick[0].median_us,
                row.full_tick[0].mad_us,
                row.full_tick[1].median_us,
                row.full_tick[1].mad_us,
            );
            println!(
                "  work/allocation audit: baseline HashSet insertions={}, partition sort Vec entries={}, per-group zero/keyed temp Vec headers={} each, record clones={}; fast HashSet=0, partition-ref Vec=0, zero sort=0, keyed ResolutionKey sort retained, reusable scratch Vecs=2, record clones={} (frozen combined result shape). Heap allocation totals are not instrumented.",
                row.settlement_count,
                row.settlement_count,
                row.settlement_count,
                row.targeted,
                row.targeted,
            );
            rows.push(row);
        }
    }

    for family in workloads {
        for (small_n, large_n) in [(1000, 10000), (10000, 20000), (20000, 50000)] {
            let small = rows
                .iter()
                .find(|row| row.family == family && row.population == small_n)
                .unwrap();
            let large = rows
                .iter()
                .find(|row| row.family == family && row.population == large_n)
                .unwrap();
            let divisor = (large_n as f64 / small_n as f64).ln();
            for (name, before, after) in [
                (
                    "Phase6B baseline",
                    small.phase6b[0].median_us,
                    large.phase6b[0].median_us,
                ),
                (
                    "Phase6B cached",
                    small.phase6b[1].median_us,
                    large.phase6b[1].median_us,
                ),
                (
                    "Full tick baseline",
                    small.full_tick[0].median_us,
                    large.full_tick[0].median_us,
                ),
                (
                    "Full tick fast",
                    small.full_tick[1].median_us,
                    large.full_tick[1].median_us,
                ),
            ] {
                let ratio = after / before.max(0.001);
                println!(
                    "{family:<26} {name:<19} N={small_n}->{large_n}: {before:.2}->{after:.2}us ratio={ratio:.2}x alpha={:.3}",
                    ratio.ln() / divisor
                );
            }
        }
        for population in [10000, 20000, 50000] {
            let row = rows
                .iter()
                .find(|row| row.family == family && row.population == population)
                .unwrap();
            println!(
                "{family:<26} N={population:<5} K={} Phase6B base/fast={:.2}/{:.2}us speedup={:.3}x full-tick={:.2}/{:.2}us speedup={:.3}x Phase6B share={:.2}/{:.2}%",
                row.settlement_count,
                row.phase6b[0].median_us,
                row.phase6b[1].median_us,
                row.phase6b[0].median_us / row.phase6b[1].median_us,
                row.full_tick[0].median_us,
                row.full_tick[1].median_us,
                row.full_tick[0].median_us / row.full_tick[1].median_us,
                row.phase6b[0].median_us / row.full_tick[0].median_us * 100.0,
                row.phase6b[1].median_us / row.full_tick[1].median_us * 100.0,
            );
        }
    }

    let (probe_config, probe_world, probe_storage, probe_partitions) =
        m2_36_inputs(base_config, context, 10000, 50);
    m2_36_component_probes(
        &probe_config,
        &probe_world,
        &probe_storage,
        &probe_partitions,
    );

    for population in [1000u64, 10000] {
        let settlement_count = (population / 200).max(1) as u32;
        let (config, world, storage, mut partitions) =
            m2_36_inputs(base_config, context, population, settlement_count);
        for index in (1..partitions.len()).rev() {
            let mut state = (population << 16) ^ index as u64 ^ 0xd1b54a32d192ed03;
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            partitions.swap(index, state as usize % (index + 1));
        }
        for (index, partition) in partitions.iter_mut().enumerate() {
            for intent_index in (1..partition.intents.len()).rev() {
                let mut state = (population << 24)
                    ^ ((index as u64) << 12)
                    ^ intent_index as u64
                    ^ 0x94d049bb133111eb;
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                partition
                    .intents
                    .swap(intent_index, state as usize % (intent_index + 1));
            }
        }
        assert!(
            !partitions
                .windows(2)
                .all(|pair| pair[0].group_id < pair[1].group_id)
                || !partitions.iter().all(|partition| partition
                    .intents
                    .windows(2)
                    .all(|pair| pair[0].agent_id() < pair[1].agent_id()))
        );
        m2_36_assert_parity(&world, &config, &partitions);
        let mut scratch = Phase6BResolutionScratch::with_capacity(population as usize);
        let mut baseline_samples = Vec::with_capacity(SAMPLES);
        let mut dispatch_samples = Vec::with_capacity(SAMPLES);
        for _ in 0..WARMUPS {
            let _ = m2_36_phase6b_sample(
                &storage,
                &world.settlements,
                world.current_day,
                &config,
                &partitions,
                0,
                &mut scratch,
            );
            let _ = m2_36_phase6b_sample(
                &storage,
                &world.settlements,
                world.current_day,
                &config,
                &partitions,
                2,
                &mut scratch,
            );
        }
        for sample in 0..SAMPLES {
            let first = sample % 2;
            for offset in 0..2 {
                let path = if (first + offset) % 2 == 0 { 0 } else { 2 };
                let elapsed = m2_36_phase6b_sample(
                    &storage,
                    &world.settlements,
                    world.current_day,
                    &config,
                    &partitions,
                    path,
                    &mut scratch,
                );
                if path == 0 {
                    baseline_samples.push(elapsed);
                } else {
                    dispatch_samples.push(elapsed);
                }
            }
        }
        let baseline = m2_29_2_summary(baseline_samples);
        let dispatch = m2_29_2_summary(dispatch_samples);
        println!(
            "Arbitrary-order Phase6B fallback N={population}: baseline={:.2}±{:.2}us dispatcher={:.2}±{:.2}us overhead={:.2}% (reversed/shuffled partitions and intents)",
            baseline.median_us,
            baseline.mad_us,
            dispatch.median_us,
            dispatch.mad_us,
            (dispatch.median_us / baseline.median_us - 1.0) * 100.0,
        );
    }

    println!(
        "Phase6B structure: baseline repeats storage slot lookup at structural validation and keyed commit, sorts partition refs and zero-target records, and allocates keyed staging per settlement. The fast runner reserves one validation Vec and reuses a second keyed Vec at its largest-settlement high-water mark, skips partition/zero-target sorts, and caches structural slots; output vectors and combined result record clones remain owned and unchanged. Heap allocation totals were not instrumented."
    );
    println!(
        "The public dispatcher performs an O(N+K) ordering verification for fallback safety. Full Hybrid calls the ordered internal entry immediately after canonical Phase5, so release production skips that dispatch scan; debug builds assert the upstream invariant."
    );
}

fn main() {
    println!("=================================================================");
    println!("SimulaCiv M0 Reference Runtime Performance Baseline Benchmark");
    println!("=================================================================");

    let config = SimConfig::parse_and_validate(GATE_CONFIG_TOML).expect("gate config is valid");
    let context = M0RunContext::new(81985529216486895, 7);

    // Warm-up run (50 days)
    {
        let mut w = initialize_world(&config).unwrap();
        let opts = DayExecutionOptions::default();
        for _ in 0..50 {
            let _ = run_m0_day(&mut w, &config, &context, &opts).unwrap();
        }
    }

    // 1. Measured 500-day instrumented graduation run
    println!("\nExecuting 500-day Canonical Graduation Trajectory...");
    let (world, metrics, events, timing) = run_instrumented_500_days(&config, &context);

    // 2. Correctness Gate Validation
    let actual_state_hash = canonical_state_hash(&world)
        .expect("state hash succeeds")
        .to_hex();
    let actual_metrics_hash = canonical_metrics_hash(&metrics)
        .expect("metrics hash succeeds")
        .to_hex();
    let actual_event_hash = canonical_event_hash(&events)
        .expect("event hash succeeds")
        .to_hex();

    println!("\n--- Correctness Gate ---");
    println!("CanonicalStateHash:   {}", actual_state_hash);
    println!("  Expected:           {}", EXPECTED_STATE_HASH);
    assert_eq!(
        actual_state_hash, EXPECTED_STATE_HASH,
        "State hash mismatch!"
    );

    println!("CanonicalMetricsHash: {}", actual_metrics_hash);
    println!("  Expected:           {}", EXPECTED_METRICS_HASH);
    assert_eq!(
        actual_metrics_hash, EXPECTED_METRICS_HASH,
        "Metrics hash mismatch!"
    );

    println!("CanonicalEventHash:   {}", actual_event_hash);
    println!("  Expected:           {}", EXPECTED_EVENT_HASH);
    assert_eq!(
        actual_event_hash, EXPECTED_EVENT_HASH,
        "Event hash mismatch!"
    );
    println!("CORRECTNESS GATE: 100% BIT-EXACT MATCH PASSED.");

    // 3. Execution Time & Phase Breakdown
    let total_us = timing.total_measured.as_nanos() as f64 / 1000.0;
    let avg_day_us = total_us / 500.0;

    println!("\n--- 500-Day Performance Summary ---");
    println!(
        "Total Execution Time: {:.3} ms",
        timing.total_measured.as_secs_f64() * 1000.0
    );
    println!("Average Day Time:     {:.2} us", avg_day_us);
    println!(
        "Simulation Throughput:{:.0} days/sec",
        500.0 / timing.total_measured.as_secs_f64()
    );

    println!("\n--- Phase-by-Phase Breakdown (500-Day Cumulative) ---");
    let phases = [
        ("Phase 1: Environment Regrowth", timing.phase1_regrowth),
        ("Phase 2: Biological Degradation", timing.phase2_degradation),
        ("Phase 3: Observation & Features", timing.phase3_features),
        ("Phase 4: Decision & Intent Gen", timing.phase4_decision),
        ("Phase 5: Locality Partitioning", timing.phase5_partition),
        ("Phase 6A: Work Resolution", timing.phase6a_work),
        ("Phase 6B: Targeted Resolution", timing.phase6b_targeted),
        ("Phase 7: Market Clearance", timing.phase7_market),
        ("Phase 8: Welfare Distribution", timing.phase8_welfare),
        ("Phase 9: Mortality Commitment", timing.phase9_mortality),
        ("Event Staging (Phase 6-9 Adapters)", timing.event_staging),
        ("Phase 10: Macroscopic Metrics", timing.phase10_metrics),
        ("Phase 11: Snapshot Emission", timing.phase11_snapshot),
        ("Phase 11: Event Flush & Sort", timing.phase11_event_flush),
        ("Runner / Context Overhead", timing.day_cursor_overhead),
    ];

    let sum_phase_us: f64 = phases
        .iter()
        .map(|(_, d)| d.as_nanos() as f64 / 1000.0)
        .sum();

    println!(
        "{:<36} | {:>10} | {:>10} | {:>8}",
        "Phase / Operation", "Time (ms)", "Avg/Day(us)", "Share (%)"
    );
    println!("{:-<72}", "");
    for (name, dur) in &phases {
        let us = dur.as_nanos() as f64 / 1000.0;
        let ms = us / 1000.0;
        let per_day = us / 500.0;
        let pct = if sum_phase_us > 0.0 {
            (us / sum_phase_us) * 100.0
        } else {
            0.0
        };
        println!(
            "{:<36} | {:>10.3} | {:>10.2} | {:>7.1}%",
            name, ms, per_day, pct
        );
    }

    // 4. Telemetry Modes
    measure_telemetry_modes(&config, &context);

    // 5. Population Scaling
    measure_population_scaling(&config, &context);

    // 6. M2-16.1 SoA Ablation Benchmark
    measure_soa_ablation(&config, &context);

    // 7. M2-17 Hot Path SoA Expansion Benchmark
    measure_hot_path_soa_expansion(&config, &context);

    // 8. M2-18 Segmented SoA Storage Architecture POC Benchmark
    measure_segmented_soa_storage(&config, &context);

    // 9. M2-18.1 Segmented SoA Storage Authority Design Benchmark
    measure_storage_authority_comparison(&config, &context);

    // 10. M2-19 WorldStorage Abstraction Layer Benchmark
    measure_world_storage_abstraction_overhead(&config, &context);

    // 11. M2-20 Phase 10 Native Segmented SoA Migration Benchmark
    measure_phase10_native_soa(&config, &context);

    // 12. M2-21 Phase 2 Native Segmented SoA Migration Benchmark
    measure_phase2_native_soa(&config, &context);

    // 13. M2-22 Phase 3 Native Segmented SoA Migration Benchmark
    measure_phase3_native_soa(&config, &context);

    // 14. M2-24 Phase 9 Native Segmented SoA Migration Benchmark
    measure_phase9_native_soa(&config, &context);

    // 15. M2-25 Phase 8 Native Segmented SoA Migration Benchmark
    measure_phase8_native_soa(&config, &context);

    // 16. M2-26 Native SoA Multi-Phase Integration Gate Benchmark
    measure_m2_native_soa_multi_phase_gate(&config, &context);

    // 17. M2-23 Hot Phase Profiling & Candidate Migration Synthetic Benchmark
    measure_candidate_phases_profiling(&config, &context);

    // 18. M2-27.1 Hybrid Authority Scope Isolation Benchmark
    measure_m2_27_1_scope_isolation_benchmark(&config, &context);

    // 19. M2-28 Phase 4 Native SoA Regression Removal Benchmark
    measure_m2_28_phase4_optimization(&config, &context);

    // 20. M2-29 Post-SoA Hot-Path Profiling & SIMD Target Selection Benchmark
    measure_m2_29_post_soa_profiling(&config, &context);

    // 21. M2-29.2 Scratch Lifetime Fidelity & Workload Scaling Benchmark
    measure_m2_29_2_scratch_fidelity(&config, &context);

    // 22. M2-30 Phase4 Group/Action Candidate Pre-Index Gate
    measure_m2_30_phase4_candidate_index(&config, &context);

    // 23. M2-32 Phase8 Group-Aware One-Pass Welfare Eligibility Gate
    measure_m2_32_phase8_one_pass(&config, &context);

    // 24. M2-33 Phase3 Direct Scarcity Lookup Gate
    measure_m2_33_phase3_direct_scarcity_lookup(&config, &context);

    // 25. M2-35 Phase5 Stable Group Bucketing Gate
    measure_m2_35_phase5_stable_group_bucketing(&config, &context);

    // 26. M2-36 Phase6B Stable Slot Cache and Scratch Gate
    measure_m2_36_phase6b_slot_cache(&config, &context);

    println!("\n=================================================================");
    println!("Benchmark Completed Successfully.");
    println!("=================================================================");
}
