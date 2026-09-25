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
use sim_model::features::phase3_observation_and_features;
use sim_model::hashing::{canonical_event_hash, canonical_metrics_hash, canonical_state_hash};
use sim_model::intents::phase4_generate_intents;
use sim_model::metrics::{DailyMetrics, phase10_observe_with_scratch};
use sim_model::partitioning::phase5_partition_intents;
use sim_model::phases::{
    phase1_resource_regrowth, phase2_biological_degradation, phase9_mortality_commitment,
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

    println!("\n=================================================================");
    println!("Benchmark Completed Successfully.");
    println!("=================================================================");
}
