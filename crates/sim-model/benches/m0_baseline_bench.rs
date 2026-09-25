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

use sim_core::SimulationDay;
use sim_model::decision::phase4_primary_action_selection;
use sim_model::events::{
    Event, EventBuffer, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    event_from_daily_metrics, events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    phase11_flush_events,
};
use sim_model::features::{
    phase3_observation_and_features, phase3_observation_and_features_into,
    phase3_observation_and_features_soa_into, phase3_observation_and_features_with_scratch,
};
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::intents::phase4_generate_intents;
use sim_model::metrics::{
    DailyMetrics, phase10_observe_compact_aos, phase10_observe_soa_fresh,
    phase10_observe_with_scratch,
};
use sim_model::partitioning::phase5_partition_intents;
use sim_model::phases::{
    phase1_resource_regrowth, phase2_biological_degradation, phase2_biological_degradation_soa,
    phase2_biological_degradation_with_scratch, phase9_mortality_commitment,
};
use sim_model::resolution::{
    phase6a_work_resolution, phase6b_targeted_resolution, phase7_market_clearance_with_config,
    phase8_welfare_distribution_with_config,
};
use sim_model::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext, run_m0_day,
};
use sim_model::snapshot::{SNAPSHOT_SCHEMA_VERSION, SnapshotMetadata, encode_snapshot};
use sim_model::state::{AgentDynamicSoAScratch, WorldState};
use sim_model::storage::SegmentedAgentStorage;
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

    println!("\n=================================================================");
    println!("Benchmark Completed Successfully.");
    println!("=================================================================");
}
