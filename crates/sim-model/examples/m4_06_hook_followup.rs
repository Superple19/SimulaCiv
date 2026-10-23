use serde_json::json;
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3pDayOutcome, M3pStorageRunContext, MigrationConfig,
    SimConfig, initialize_world, run_m3p_storage_days,
};
use std::error::Error;
use std::fs;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;

#[path = "common/process_controls.rs"]
mod process_controls;

const DAYS_PER_OPERATION: u32 = 3;
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const TARGET_SAMPLE_NS: u128 = 1_000_000_000;
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;

struct Runner {
    world: HybridWorldState,
    context: M3pStorageRunContext,
}

impl Runner {
    fn new(
        initial: &sim_model::WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, Box<dyn Error>> {
        let world = HybridWorldState::hybrid(initial.clone());
        let context = M3pStorageRunContext::new(&world, config, run_context)?;
        Ok(Self { world, context })
    }

    fn run(&mut self, config: &SimConfig) -> Result<(u128, Vec<M3pDayOutcome>), Box<dyn Error>> {
        let started = Instant::now();
        let outcomes = run_m3p_storage_days(
            &mut self.world,
            config,
            &mut self.context,
            DAYS_PER_OPERATION,
            &DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: false,
            },
        )?;
        Ok((started.elapsed().as_nanos(), black_box(outcomes)))
    }
}

fn config_for(population: u64, probability: f32) -> Result<SimConfig, Box<dyn Error>> {
    let source = include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
        .replace("propagation = true", "propagation = false");
    let source = if let Some(index) = source.find("\n[propagation]") {
        source[..index].to_owned()
    } else {
        source
    };
    let mut config = SimConfig::from_toml_str(&source)?;
    config.propagation = None;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200)?;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    config.enable_migration(MigrationConfig {
        migration_probability: probability,
        migration_threshold: 0.0,
        migration_cost: 10,
        migration_cooldown_days: 30,
    });
    config.validate()?;
    Ok(config)
}

fn baseline_config(population: u64) -> Result<SimConfig, Box<dyn Error>> {
    let source = include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
        .replace("propagation = true", "propagation = false");
    let source = if let Some(index) = source.find("\n[propagation]") {
        source[..index].to_owned()
    } else {
        source
    };
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

fn assert_off_parity(
    base_runner: &Runner,
    enabled_runner: &Runner,
    base: &[M3pDayOutcome],
    enabled: &[M3pDayOutcome],
) {
    assert_eq!(base.len(), enabled.len());
    assert_eq!(
        base_runner
            .world
            .segmented_storage
            .as_ref()
            .unwrap()
            .to_agents(),
        enabled_runner
            .world
            .segmented_storage
            .as_ref()
            .unwrap()
            .to_agents()
    );
    assert_eq!(
        base_runner.world.world.settlements,
        enabled_runner.world.world.settlements
    );
    for (left, right) in base.iter().zip(enabled) {
        assert_eq!(left.metrics, right.metrics);
        assert_eq!(left.events, right.events);
        let migration = right.migration.as_ref().expect("migration hook ran");
        assert_eq!(migration.metrics.unwrap().migrations, 0);
        assert!(migration.events.is_empty());
    }
}

fn run_cell(population: u64, probability: f32) -> Result<serde_json::Value, Box<dyn Error>> {
    let base_config = baseline_config(population)?;
    let enabled_config = config_for(population, probability)?;
    let initial = initialize_world(&base_config)?;
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut warmup_pairs = Vec::with_capacity(WARMUPS);
    for index in 0..WARMUPS {
        let mut baseline = Runner::new(&initial, &base_config, &run_context)?;
        let mut enabled = Runner::new(&initial, &enabled_config, &run_context)?;
        let (base_ns, base_outcomes, enabled_ns, enabled_outcomes) = if index % 2 == 0 {
            let (base_ns, base_outcomes) = baseline.run(&base_config)?;
            let (enabled_ns, enabled_outcomes) = enabled.run(&enabled_config)?;
            (base_ns, base_outcomes, enabled_ns, enabled_outcomes)
        } else {
            let (enabled_ns, enabled_outcomes) = enabled.run(&enabled_config)?;
            let (base_ns, base_outcomes) = baseline.run(&base_config)?;
            (base_ns, base_outcomes, enabled_ns, enabled_outcomes)
        };
        if probability == 0.0 {
            assert_off_parity(&baseline, &enabled, &base_outcomes, &enabled_outcomes);
        }
        warmup_pairs.push(base_ns.saturating_add(enabled_ns));
    }
    let warmup_median = median(&warmup_pairs.iter().map(|&ns| ns as f64).collect::<Vec<_>>());
    let repetitions =
        usize::try_from(TARGET_SAMPLE_NS.div_ceil(warmup_median.max(1.0) as u128))?.max(1);
    let mut baseline_samples = Vec::with_capacity(SAMPLES);
    let mut enabled_samples = Vec::with_capacity(SAMPLES);
    let mut delta_samples = Vec::with_capacity(SAMPLES);
    let mut normalized_samples = Vec::with_capacity(SAMPLES);
    let mut moves_by_sample = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let mut baseline = Runner::new(&initial, &base_config, &run_context)?;
        let mut enabled = Runner::new(&initial, &enabled_config, &run_context)?;
        let (mut base_ns, mut enabled_ns, mut alive_agent_days, mut moves) =
            (0_u128, 0_u128, 0_u64, 0_u64);
        for repetition in 0..repetitions {
            let baseline_first = (repetition % 2 == 0) == (sample % 2 == 0);
            let (base_elapsed, base_outcomes, enabled_elapsed, enabled_outcomes) = if baseline_first
            {
                let (base_elapsed, base_outcomes) = baseline.run(&base_config)?;
                let (enabled_elapsed, enabled_outcomes) = enabled.run(&enabled_config)?;
                (
                    base_elapsed,
                    base_outcomes,
                    enabled_elapsed,
                    enabled_outcomes,
                )
            } else {
                let (enabled_elapsed, enabled_outcomes) = enabled.run(&enabled_config)?;
                let (base_elapsed, base_outcomes) = baseline.run(&base_config)?;
                (
                    base_elapsed,
                    base_outcomes,
                    enabled_elapsed,
                    enabled_outcomes,
                )
            };
            if probability == 0.0 {
                assert_off_parity(&baseline, &enabled, &base_outcomes, &enabled_outcomes);
            }
            base_ns = base_ns.saturating_add(base_elapsed);
            enabled_ns = enabled_ns.saturating_add(enabled_elapsed);
            for day in enabled_outcomes {
                let metrics = day.metrics.expect("metrics enabled");
                alive_agent_days = alive_agent_days
                    .checked_add(metrics.live_population)
                    .ok_or("alive agent-day count overflow")?;
                let migration = day.migration.expect("migration hook ran");
                moves = moves.saturating_add(migration.metrics.unwrap().migrations);
            }
        }
        let base_us = base_ns as f64 / repetitions as f64 / 1_000.0;
        let enabled_us = enabled_ns as f64 / repetitions as f64 / 1_000.0;
        baseline_samples.push(base_us);
        enabled_samples.push(enabled_us);
        delta_samples.push(enabled_us - base_us);
        normalized_samples.push(if alive_agent_days == 0 {
            0.0
        } else {
            (enabled_ns as f64 - base_ns as f64) / alive_agent_days as f64
        });
        moves_by_sample.push(moves);
    }
    Ok(json!({
        "population": population,
        "settlements": population / 200,
        "probability": probability,
        "repetitions_per_sample": repetitions,
        "warmups": WARMUPS,
        "samples": SAMPLES,
        "baseline_median_us_per_3_days": median(&baseline_samples),
        "baseline_mad_us": mad(&baseline_samples),
        "enabled_median_us_per_3_days": median(&enabled_samples),
        "enabled_mad_us": mad(&enabled_samples),
        "delta_median_us": median(&delta_samples),
        "delta_mad_us": mad(&delta_samples),
        "migration_hook_ns_per_alive_agent_day_median": median(&normalized_samples),
        "migration_hook_ns_per_alive_agent_day_mad": mad(&normalized_samples),
        "moves_min": moves_by_sample.iter().min().copied().unwrap_or(0),
        "moves_max": moves_by_sample.iter().max().copied().unwrap_or(0),
        "baseline_samples_us_per_3_days": baseline_samples,
        "enabled_samples_us_per_3_days": enabled_samples,
        "delta_samples_us": delta_samples,
        "migration_hook_samples_ns_per_alive_agent_day": normalized_samples
    }))
}

fn apply_windows_controls() -> Result<String, Box<dyn Error>> {
    #[cfg(windows)]
    {
        let status = process_controls::apply_high_priority_affinity();
        if !status.priority_set || !status.affinity_set {
            return Err(std::io::Error::last_os_error().into());
        }
        let settings =
            process_controls::current_settings().ok_or_else(std::io::Error::last_os_error)?;
        Ok(format!(
            "priority_class=0x{:x}; affinity=0x{:x}; system=0x{:x}",
            settings.priority_class, settings.process_mask, settings.system_mask
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
        .unwrap_or_else(|| PathBuf::from("docs/data/m4_06/hook_followup.json"));
    let controls = apply_windows_controls()?;
    let cells = [10_000, 100_000]
        .into_iter()
        .flat_map(|population| [0.0, 0.05].map(move |probability| (population, probability)))
        .map(|(population, probability)| run_cell(population, probability))
        .collect::<Result<Vec<_>, _>>()?;
    let result = json!({
        "experiment": "M4-06 migration hook follow-up",
        "host": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "logical_processors": std::thread::available_parallelism().map(|value| value.get()).unwrap_or(0),
            "controls": controls
        },
        "seed": SEED,
        "replicate_id": REPLICATE,
        "days_per_operation": DAYS_PER_OPERATION,
        "cells": cells
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&result)?)?;
    println!("wrote {}", output.display());
    Ok(())
}
