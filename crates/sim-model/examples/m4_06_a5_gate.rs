use serde_json::{Value, json};
use sim_model::hashing::{CanonicalHash, canonical_m3p_state_hash_from_storage};
use sim_model::state::HybridWorldState;
use sim_model::{
    ConfiguredDayOutcome, ConfiguredStorageRunContext, DayExecutionOptions, M0RunContext,
    M3pDayOutcome, M3pStorageRunContext, MigrationConfig, SimConfig, initialize_world,
    run_configured_storage_days, run_m3p_storage_days,
};
use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Instant;

const DAYS_PER_OPERATION: u32 = 3;
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const TARGET_SAMPLE_NS: u128 = 1_000_000_000;
const MIGRATION_COST: i64 = 10;
const MIGRATION_COOLDOWN_DAYS: u32 = 30;
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;

enum RunnerContext {
    Direct(M3pStorageRunContext),
    Configured(ConfiguredStorageRunContext),
}

struct RunnerState {
    world: HybridWorldState,
    context: RunnerContext,
}

impl RunnerState {
    fn new(
        initial: &sim_model::WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
        configured_dispatch: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let world = HybridWorldState::hybrid(initial.clone());
        let context = M3pStorageRunContext::new(&world, config, run_context)?;
        let context = if configured_dispatch {
            RunnerContext::Configured(ConfiguredStorageRunContext::M3p(context))
        } else {
            RunnerContext::Direct(context)
        };
        Ok(Self { world, context })
    }

    fn state_hash(&self) -> Result<CanonicalHash, Box<dyn Error>> {
        let storage = self.world.storage().ok_or("segmented storage missing")?;
        let (graph, next_agent_id) = match &self.context {
            RunnerContext::Direct(context) => {
                (context.relationship_graph(), context.next_agent_id())
            }
            RunnerContext::Configured(ConfiguredStorageRunContext::M3p(context)) => {
                (context.relationship_graph(), context.next_agent_id())
            }
            _ => return Err("expected an M3P runner context".into()),
        };
        Ok(canonical_m3p_state_hash_from_storage(
            storage,
            self.world.world.current_day,
            &self.world.world.settlements,
            graph,
            next_agent_id,
        )?)
    }

    fn run(&mut self, config: &SimConfig) -> Result<(u128, Vec<M3pDayOutcome>), Box<dyn Error>> {
        let started = Instant::now();
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        };
        let outcomes = match &mut self.context {
            RunnerContext::Direct(context) => run_m3p_storage_days(
                &mut self.world,
                config,
                context,
                DAYS_PER_OPERATION,
                &options,
            )?,
            RunnerContext::Configured(context) => {
                let run = run_configured_storage_days(
                    &mut self.world,
                    config,
                    context,
                    DAYS_PER_OPERATION,
                    &options,
                )?;
                run.days
                    .into_iter()
                    .map(|day| match day {
                        ConfiguredDayOutcome::M3p(day) => Ok(day),
                        _ => Err("configured migration-off run selected a non-M3P path".into()),
                    })
                    .collect::<Result<Vec<_>, Box<dyn Error>>>()?
            }
        };
        let elapsed = started.elapsed().as_nanos();
        Ok((elapsed, black_box(outcomes)))
    }
}

fn config_for(population: u64, explicit_migration_off: bool) -> Result<SimConfig, Box<dyn Error>> {
    let mut source = include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
        .replace("propagation = true", "propagation = false");
    if explicit_migration_off {
        source = source.replace(
            "propagation = false",
            "propagation = false\nmigration = false",
        );
    }
    if let Some(propagation_section) = source.find("\n[propagation]") {
        source.truncate(propagation_section);
    }
    let mut config = SimConfig::from_toml_str(&source)?;
    config.propagation = None;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200)?;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    config.validate()?;
    Ok(config)
}

fn migration_config() -> MigrationConfig {
    MigrationConfig {
        migration_probability: 0.0,
        migration_threshold: 0.0,
        migration_cost: MIGRATION_COST,
        migration_cooldown_days: MIGRATION_COOLDOWN_DAYS,
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    if sorted.len().is_multiple_of(2) {
        (sorted[sorted.len() / 2 - 1] + sorted[sorted.len() / 2]) / 2.0
    } else {
        sorted[sorted.len() / 2]
    }
}

fn mad(values: &[f64]) -> f64 {
    let center = median(values);
    median(
        &values
            .iter()
            .map(|value| (value - center).abs())
            .collect::<Vec<_>>(),
    )
}

fn bootstrap_u(deltas: &[f64]) -> f64 {
    let mut rng = 0xA3_11_70u64;
    let mut medians = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let mut resample = Vec::with_capacity(deltas.len());
        for _ in 0..deltas.len() {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            resample.push(deltas[(rng % deltas.len() as u64) as usize]);
        }
        medians.push(median(&resample));
    }
    medians.sort_by(f64::total_cmp);
    medians[9_499]
}

fn run_one(
    state: &mut RunnerState,
    config: &SimConfig,
) -> Result<(u128, Vec<M3pDayOutcome>), Box<dyn Error>> {
    state.run(config)
}

fn compare_day_outputs(left: &[M3pDayOutcome], right: &[M3pDayOutcome]) {
    assert_eq!(left, right, "migration-off run outputs diverged");
}

fn warmup_pair(
    initial: &sim_model::WorldState,
    base_config: &SimConfig,
    explicit_off_config: &SimConfig,
    run_context: &M0RunContext,
    base_first: bool,
) -> Result<u128, Box<dyn Error>> {
    let mut base = RunnerState::new(initial, base_config, run_context, false)?;
    let mut explicit_off = RunnerState::new(initial, explicit_off_config, run_context, true)?;
    let (base_ns, base_outcomes, other_ns, other_outcomes) = if base_first {
        let (a_ns, a) = run_one(&mut base, base_config)?;
        let (b_ns, b) = run_one(&mut explicit_off, explicit_off_config)?;
        (a_ns, a, b_ns, b)
    } else {
        let (b_ns, b) = run_one(&mut explicit_off, explicit_off_config)?;
        let (a_ns, a) = run_one(&mut base, base_config)?;
        (a_ns, a, b_ns, b)
    };
    compare_day_outputs(&base_outcomes, &other_outcomes);
    assert_eq!(base.state_hash()?, explicit_off.state_hash()?);
    Ok(base_ns + other_ns)
}

#[derive(Debug)]
struct Attempt {
    baseline_us: Vec<f64>,
    explicit_off_us: Vec<f64>,
    deltas_us: Vec<f64>,
    migration_on_ns_per_agent_day: Vec<f64>,
    migration_count: u64,
}

fn run_attempt(
    initial: &sim_model::WorldState,
    baseline_config: &SimConfig,
    explicit_off_config: &SimConfig,
    migration_on_config: &SimConfig,
    run_context: &M0RunContext,
    repetitions: usize,
) -> Result<Attempt, Box<dyn Error>> {
    let mut baseline_us = Vec::with_capacity(SAMPLES);
    let mut explicit_off_us = Vec::with_capacity(SAMPLES);
    let mut deltas_us = Vec::with_capacity(SAMPLES);
    let mut migration_on_ns_per_agent_day = Vec::with_capacity(SAMPLES);
    let mut migration_count = 0u64;

    for sample in 0..SAMPLES {
        let mut baseline = RunnerState::new(initial, baseline_config, run_context, false)?;
        let mut explicit_off = RunnerState::new(initial, explicit_off_config, run_context, true)?;
        let mut migration_on = RunnerState::new(initial, migration_on_config, run_context, false)?;
        let (mut baseline_ns, mut explicit_off_ns, mut migration_on_ns) = (0_u128, 0_u128, 0_u128);
        let mut alive_agent_days = 0_u64;

        for repetition in 0..repetitions {
            let baseline_first = (repetition % 2 == 0) == (sample % 2 == 0);
            let (base_duration, base_outcomes, off_duration, off_outcomes) = if baseline_first {
                let (base_duration, base_outcomes) = run_one(&mut baseline, baseline_config)?;
                let (off_duration, off_outcomes) = run_one(&mut explicit_off, explicit_off_config)?;
                (base_duration, base_outcomes, off_duration, off_outcomes)
            } else {
                let (off_duration, off_outcomes) = run_one(&mut explicit_off, explicit_off_config)?;
                let (base_duration, base_outcomes) = run_one(&mut baseline, baseline_config)?;
                (base_duration, base_outcomes, off_duration, off_outcomes)
            };
            compare_day_outputs(&base_outcomes, &off_outcomes);
            assert_eq!(baseline.state_hash()?, explicit_off.state_hash()?);
            baseline_ns = baseline_ns.saturating_add(base_duration);
            explicit_off_ns = explicit_off_ns.saturating_add(off_duration);

            let (enabled_duration, enabled_outcomes) =
                run_one(&mut migration_on, migration_on_config)?;
            migration_on_ns = migration_on_ns.saturating_add(enabled_duration);
            assert_eq!(baseline.state_hash()?, migration_on.state_hash()?);
            for outcome in enabled_outcomes {
                let metrics = outcome
                    .metrics
                    .ok_or("migration-on runner omitted metrics")?;
                alive_agent_days = alive_agent_days
                    .checked_add(metrics.live_population)
                    .ok_or("alive agent-day count overflow")?;
                let migration = outcome.migration.ok_or("migration hook did not run")?;
                if let Some(metrics) = migration.metrics {
                    migration_count = migration_count.saturating_add(metrics.migrations);
                }
            }
        }

        let baseline_mean = baseline_ns as f64 / repetitions as f64 / 1_000.0;
        let explicit_off_mean = explicit_off_ns as f64 / repetitions as f64 / 1_000.0;
        baseline_us.push(baseline_mean);
        explicit_off_us.push(explicit_off_mean);
        deltas_us.push(explicit_off_mean - baseline_mean);
        let active_ns_per_agent_day = if alive_agent_days == 0 {
            0.0
        } else {
            (migration_on_ns as f64 - baseline_ns as f64) / alive_agent_days as f64
        };
        migration_on_ns_per_agent_day.push(active_ns_per_agent_day);
    }
    Ok(Attempt {
        baseline_us,
        explicit_off_us,
        deltas_us,
        migration_on_ns_per_agent_day,
        migration_count,
    })
}

fn attempt_json(attempt: &Attempt, repetitions: usize, retry: bool) -> Value {
    let base_median = median(&attempt.baseline_us);
    let candidate_median = median(&attempt.explicit_off_us);
    let delta_median = median(&attempt.deltas_us);
    let delta_mad = mad(&attempt.deltas_us);
    let upper = bootstrap_u(&attempt.deltas_us);
    let threshold = base_median * 0.05;
    let base_spread = mad(&attempt.baseline_us) / base_median.abs();
    let candidate_spread = mad(&attempt.explicit_off_us) / candidate_median.abs();
    let valid = base_spread <= 0.05 && candidate_spread <= 0.05;
    let status = if !valid {
        "INVALID"
    } else if upper <= threshold {
        "PASS"
    } else if delta_median > threshold {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    };
    json!({
        "repetitions_per_sample": repetitions,
        "samples": SAMPLES,
        "retry": retry,
        "baseline_median_us_per_3_days": base_median,
        "baseline_mad_us": mad(&attempt.baseline_us),
        "explicit_off_median_us_per_3_days": candidate_median,
        "explicit_off_mad_us": mad(&attempt.explicit_off_us),
        "delta_median_us": delta_median,
        "delta_mad_us": delta_mad,
        "bootstrap_upper_95_us": upper,
        "threshold_us": threshold,
        "baseline_spread_ratio": base_spread,
        "explicit_off_spread_ratio": candidate_spread,
        "valid": valid,
        "status": status,
        "migration_on_zero_move_ns_per_alive_agent_day_median": median(&attempt.migration_on_ns_per_agent_day),
        "migration_on_zero_move_ns_per_alive_agent_day_mad": mad(&attempt.migration_on_ns_per_agent_day),
        "migration_on_moves": attempt.migration_count,
        "baseline_samples_us_per_3_days": attempt.baseline_us,
        "explicit_off_samples_us_per_3_days": attempt.explicit_off_us,
        "paired_delta_samples_us": attempt.deltas_us,
        "migration_on_samples_ns_per_alive_agent_day": attempt.migration_on_ns_per_agent_day
    })
}

fn run_cell(population: u64) -> Result<Value, Box<dyn Error>> {
    let baseline_config = config_for(population, false)?;
    let explicit_off_config = config_for(population, true)?;
    let mut migration_on_config = baseline_config.clone();
    migration_on_config.enable_migration(migration_config());
    migration_on_config.validate()?;
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let initial = initialize_world(&baseline_config)?;

    let mut warmup_pair_ns = Vec::with_capacity(WARMUPS);
    for warmup in 0..WARMUPS {
        warmup_pair_ns.push(warmup_pair(
            &initial,
            &baseline_config,
            &explicit_off_config,
            &run_context,
            warmup.is_multiple_of(2),
        )?);
    }
    let warmup_median = median(
        &warmup_pair_ns
            .iter()
            .map(|&ns| ns as f64)
            .collect::<Vec<_>>(),
    );
    let repetitions =
        usize::try_from(TARGET_SAMPLE_NS.div_ceil(warmup_median.max(1.0) as u128))?.max(1);

    let run = |retry| {
        run_attempt(
            &initial,
            &baseline_config,
            &explicit_off_config,
            &migration_on_config,
            &run_context,
            repetitions,
        )
        .map(|attempt| attempt_json(&attempt, repetitions, retry))
    };
    let first = run(false)?;
    let valid = first.get("valid").and_then(Value::as_bool).unwrap_or(false);
    let result = if valid { first } else { run(true)? };
    Ok(json!({
        "population": population,
        "settlements": population / 200,
        "degree_k": 8,
        "p_local": 0.5,
        "master_seed": SEED,
        "replicate_id": REPLICATE,
        "days_per_operation": DAYS_PER_OPERATION,
        "warmup_pair_total_ns": warmup_pair_ns,
        "repetitions_fixed_from_warmup": repetitions,
        "result": result
    }))
}

fn apply_windows_controls() -> Result<String, Box<dyn Error>> {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut c_void;
            fn SetPriorityClass(process: *mut c_void, priority_class: u32) -> i32;
            fn SetProcessAffinityMask(process: *mut c_void, mask: usize) -> i32;
            fn GetPriorityClass(process: *mut c_void) -> u32;
            fn GetProcessAffinityMask(
                process: *mut c_void,
                process_mask: *mut usize,
                system_mask: *mut usize,
            ) -> i32;
        }
        const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
        const AFFINITY_MASK: usize = 0x555;
        // SAFETY: GetCurrentProcess returns a borrowed pseudo-handle valid for these process APIs.
        let process = unsafe { GetCurrentProcess() };
        // SAFETY: The pseudo-handle is valid and both calls use scalar arguments only.
        if unsafe { SetPriorityClass(process, HIGH_PRIORITY_CLASS) } == 0
            || unsafe { SetProcessAffinityMask(process, AFFINITY_MASK) } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut process_mask = 0_usize;
        let mut system_mask = 0_usize;
        // SAFETY: Both output pointers reference initialized local usize values.
        if unsafe { GetProcessAffinityMask(process, &mut process_mask, &mut system_mask) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: The pseudo-handle is valid for GetPriorityClass.
        let priority = unsafe { GetPriorityClass(process) };
        Ok(format!(
            "priority_class=0x{priority:x}; affinity=0x{process_mask:x}; system=0x{system_mask:x}"
        ))
    }
    #[cfg(not(windows))]
    {
        Ok("not-applied (non-Windows host)".to_owned())
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/data/m4_06/a5_gate.json"));
    let controls = apply_windows_controls()?;
    let host = json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "logical_processors": std::thread::available_parallelism().map(NonZeroUsize::get).unwrap_or(0),
        "process_controls": controls,
        "warmups": WARMUPS,
        "samples": SAMPLES,
        "target_sample_ns": TARGET_SAMPLE_NS,
        "bootstrap_resamples": 10_000,
        "bootstrap_seed": "0xA3_11_70"
    });
    let cells = [10_000, 100_000]
        .into_iter()
        .map(run_cell)
        .collect::<Result<Vec<_>, _>>()?;
    let result = json!({
        "gate": "M4-06 migration-off A-5",
        "baseline": "existing direct run_m3p_storage_days runner with migration omitted",
        "candidate": "run_configured_storage_days with explicit migration=false",
        "migration_on_measurement": "direct M3P storage path with migration enabled and probability=0; no moves",
        "host": host,
        "cells": cells
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&result)?)?;
    println!("wrote {}", output.display());
    Ok(())
}
