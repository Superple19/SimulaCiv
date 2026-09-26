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
use sim_core::{AgentId, Money, SimulationDay};
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
    AgentFeatures, phase3_observation_and_features, phase3_observation_and_features_into,
    phase3_observation_and_features_soa_into, phase3_observation_and_features_storage_into,
    phase3_observation_and_features_with_scratch,
};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::intents::{
    Intent, generate_intents_storage_into_baseline, generate_intents_storage_into_variant_c,
    generate_intents_storage_into_variant_d, generate_intents_storage_with_scratch,
    phase4_generate_intents, phase4_generate_intents_into, phase4_generate_intents_storage_into,
};
use sim_model::metrics::{
    DailyMetrics, phase10_observe, phase10_observe_compact_aos, phase10_observe_soa_fresh,
    phase10_observe_storage, phase10_observe_storage_with_scratch, phase10_observe_with_scratch,
};
use sim_model::partitioning::{SettlementIntentPartition, phase5_partition_intents};
use sim_model::phases::{
    phase1_resource_regrowth, phase2_biological_degradation, phase2_biological_degradation_soa,
    phase2_biological_degradation_storage, phase2_biological_degradation_with_scratch,
    phase9_mortality_commitment, phase9_mortality_commitment_storage,
    update_biological_degradation,
};
use sim_model::resolution::{
    phase6a_work_resolution, phase6a_work_resolution_storage, phase6b_targeted_resolution,
    phase6b_targeted_resolution_storage, phase7_market_clearance_storage_with_config,
    phase7_market_clearance_with_config, phase8_welfare_distribution_storage_with_config,
    phase8_welfare_distribution_with_config,
};
use sim_model::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext,
    run_hybrid_authority_days, run_hybrid_scope_isolated_days, run_m0_day, run_m0_days,
    run_native_soa_day, run_native_soa_days,
};
use sim_model::snapshot::{
    SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, encode_snapshot, restore_snapshot,
};
use sim_model::state::SettlementState;
use sim_model::state::{AgentDynamicSoAScratch, HybridWorldState, WorldState};
use sim_model::storage::{SegmentedAgentStorage, WorldStorage, canonical_state_hash_from_storage};
use sim_model::subsystems::Subsystem;
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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");
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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
        let partitions = phase5_partition_intents(&intents).expect("phase5 succeeds");

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
            let p = phase5_partition_intents(&intents_b).unwrap();
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
        let partitions = phase5_partition_intents(&intents).unwrap();

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

        let partitions = phase5_partition_intents(&intents_scratch)?;

        if native_p6a {
            let _ = phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
        } else {
            let _ = phase6a_work_resolution(world, &partitions)?;
        }

        if native_p6b {
            let _ = phase6b_targeted_resolution_storage(
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

        let partitions = phase5_partition_intents(&intents_a).unwrap();

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
            let _ = phase6b_targeted_resolution_storage(
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

        let partitions = phase5_partition_intents(&intents_scratch)?;
        let _ = phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
        let _ = phase6b_targeted_resolution_storage(
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

    println!("\n=================================================================");
    println!("Benchmark Completed Successfully.");
    println!("=================================================================");
}
