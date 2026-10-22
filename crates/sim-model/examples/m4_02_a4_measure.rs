// restored from session history; not re-run
// Historical measurement protocol; the final A-5 harness is m4_02_a5_measure.rs.
use std::hint::black_box;
use std::time::Instant;

use sim_core::AgentId;
use sim_model::config::{SimulationConfig, SimulationProfile, SimulationSystems};
use sim_model::counterfactual::{
    BranchAnalysisMode, BranchAnalysisOutput, BranchExecutionOption, BranchIntervention,
    BranchRequest, ConfigParameter, ConfigValue, FirstDivergence, run_counterfactual_branch,
    run_counterfactual_branch_with_analysis_mode,
};
use sim_model::m3p::{M3pReferenceState, decode_m3p_snapshot, encode_m3p_snapshot};
use sim_model::m3pprop::{
    M3pPropagationReferenceState, decode_m3pprop_snapshot, encode_m3pprop_snapshot,
};
use sim_model::m3prop::{
    M3PropagationReferenceState, decode_m3prop_snapshot, encode_m3prop_snapshot,
};
use sim_model::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext,
    M3PropagationStorageRunContext, M3StorageRunContext, M3pIncrementalTopologyStorageRunContext,
    M3pPropagationStorageRunContext, M3pStorageRunContext, run_m3_storage_days,
    run_m3p_propagation_storage_days, run_m3p_storage_days,
    run_m3p_storage_days_incremental_topology, run_m3prop_storage_days,
};
use sim_model::snapshot::{SnapshotMetadata, decode_snapshot, encode_snapshot};
use sim_model::state::{AuthorityMode, HybridWorldState};
use sim_model::{Action, SimConfig, SimulationRunnerSelection, initialize_world};

const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const HORIZON: u32 = 30;
const WARMUPS: usize = 2;
const SAMPLES: usize = 41;
const BOOTSTRAP_SEED: u64 = 0xA3_11_70;
const BOOTSTRAP_RESAMPLES: usize = 10_000;
const OPTIONS: DayExecutionOptions = DayExecutionOptions {
    metrics_enabled: true,
    events_enabled: true,
    snapshot_boundary: false,
};

struct Cell {
    id: String,
    request: BranchRequest,
}

fn config(population: u64, propagation: bool, population_dynamics: bool) -> SimConfig {
    let fixture = if population_dynamics {
        include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
    } else {
        include_str!("../tests/fixtures/m3_propagation.toml")
    };
    let mut config = SimConfig::parse_and_validate(fixture).unwrap();
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200).unwrap();
    config.simulation = None;
    config.propagation = propagation.then_some(config.propagation).flatten();
    if population_dynamics {
        config.profile = SimulationProfile::M3p;
    } else {
        config.profile = SimulationProfile::M3;
        config.population = None;
    }
    if propagation {
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics,
                propagation: true,
            },
        });
    }
    config.validate().unwrap();
    config
}

fn make_request(
    config: &SimConfig,
    execution: BranchExecutionOption,
    intervention: BranchIntervention,
    horizon: u32,
) -> BranchRequest {
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        run_context.master_seed,
        run_context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let snapshot = match config.runner_selection() {
        SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
        SimulationRunnerSelection::M3Propagation => {
            let state = M3PropagationReferenceState::new(world, config, &run_context).unwrap();
            encode_m3prop_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M3p => {
            let state = M3pReferenceState::new(world, config, &run_context).unwrap();
            encode_m3p_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M3pPropagation => {
            let state = M3pPropagationReferenceState::new(world, config, &run_context).unwrap();
            encode_m3pprop_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M2 => unreachable!(),
    };
    BranchRequest {
        config: config.clone(),
        run_context,
        snapshot,
        intervention,
        horizon_days: horizon,
        execution,
        snapshot_days: Vec::new(),
    }
}

fn direct_pass(request: &BranchRequest) {
    let config = &request.config;
    let run_context = &request.run_context;
    match config.runner_selection() {
        SimulationRunnerSelection::M3 => {
            let restored = decode_snapshot(request.snapshot.as_ref()).unwrap();
            let mut world = HybridWorldState::new(restored.world, AuthorityMode::Hybrid);
            let mut context = M3StorageRunContext::new(&world, config, run_context).unwrap();
            black_box(
                run_m3_storage_days(&mut world, config, &mut context, HORIZON, &OPTIONS).unwrap(),
            );
        }
        SimulationRunnerSelection::M3Propagation => {
            let restored = decode_m3prop_snapshot(request.snapshot.as_ref(), config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            let mut context = M3PropagationStorageRunContext::from_reference_state(
                &world,
                config,
                run_context,
                &restored.state,
            )
            .unwrap();
            world.sync_to_world_agents();
            black_box(
                run_m3prop_storage_days(&mut world, config, &mut context, HORIZON, &OPTIONS)
                    .unwrap(),
            );
        }
        SimulationRunnerSelection::M3p => {
            let restored = decode_m3p_snapshot(request.snapshot.as_ref(), config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            world.sync_to_world_agents();
            if request.execution == BranchExecutionOption::IncrementalTopology {
                let mut context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
                    &world,
                    config,
                    run_context,
                    &restored.state,
                )
                .unwrap();
                black_box(
                    run_m3p_storage_days_incremental_topology(
                        &mut world,
                        config,
                        &mut context,
                        HORIZON,
                        &OPTIONS,
                    )
                    .unwrap(),
                );
            } else {
                let mut context = M3pStorageRunContext::from_reference_state(
                    &world,
                    config,
                    run_context,
                    &restored.state,
                )
                .unwrap();
                black_box(
                    run_m3p_storage_days(&mut world, config, &mut context, HORIZON, &OPTIONS)
                        .unwrap(),
                );
            }
        }
        SimulationRunnerSelection::M3pPropagation => {
            let restored = decode_m3pprop_snapshot(request.snapshot.as_ref(), config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            world.sync_to_world_agents();
            let mut context = M3pPropagationStorageRunContext::from_reference_state(
                &world,
                config,
                run_context,
                &restored.state,
            )
            .unwrap();
            black_box(
                run_m3p_propagation_storage_days(
                    &mut world,
                    config,
                    &mut context,
                    HORIZON,
                    &OPTIONS,
                )
                .unwrap(),
            );
        }
        SimulationRunnerSelection::M2 => unreachable!(),
    }
}

fn analysis_disabled(request: &BranchRequest) {
    black_box(
        run_counterfactual_branch_with_analysis_mode(request, BranchAnalysisMode::Disabled)
            .unwrap(),
    );
}

fn timed<F: FnOnce()>(f: F) -> u128 {
    let start = Instant::now();
    f();
    start.elapsed().as_nanos()
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn median_mad(values: &[f64]) -> (f64, f64) {
    let med = median(values);
    let deviations: Vec<_> = values.iter().map(|v| (v - med).abs()).collect();
    (med, median(&deviations))
}

fn bootstrap_upper(values: &[f64]) -> f64 {
    let mut state = BOOTSTRAP_SEED;
    let mut medians = Vec::with_capacity(BOOTSTRAP_RESAMPLES);
    for _ in 0..BOOTSTRAP_RESAMPLES {
        let mut sample = Vec::with_capacity(values.len());
        for _ in 0..values.len() {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            sample.push(values[(state % values.len() as u64) as usize]);
        }
        medians.push(median(&sample));
    }
    medians.sort_by(f64::total_cmp);
    medians[9_499]
}

fn millis(ns: f64) -> f64 {
    ns / 1_000_000.0
}

fn classify(median_delta: f64, upper: f64, threshold: f64) -> &'static str {
    if upper <= threshold {
        "PASS"
    } else if median_delta > threshold {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    }
}

fn orchestration_cell(cell: &Cell) {
    let mut direct = Vec::with_capacity(SAMPLES);
    let mut disabled = Vec::with_capacity(SAMPLES);
    let mut delta = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let direct_first = sample % 2 == 0;
        let (d, b) = if direct_first {
            let d = timed(|| {
                direct_pass(&cell.request);
                direct_pass(&cell.request);
            });
            let b = timed(|| analysis_disabled(&cell.request));
            (d, b)
        } else {
            let b = timed(|| analysis_disabled(&cell.request));
            let d = timed(|| {
                direct_pass(&cell.request);
                direct_pass(&cell.request);
            });
            (d, b)
        };
        if sample >= WARMUPS {
            direct.push(d as f64);
            disabled.push(b as f64);
            delta.push(b as f64 - d as f64);
        }
        println!("SAMPLE,{},{}", cell.id, sample);
    }
    let (dm, dd) = median_mad(&direct);
    let (bm, bd) = median_mad(&disabled);
    let (xm, xd) = median_mad(&delta);
    let upper = bootstrap_upper(&delta);
    let threshold = 0.05 * dm;
    println!(
        "ORCH,{},{},{},{},{},{},{},{},{},{},{},{}",
        cell.id,
        millis(dm),
        millis(dd),
        millis(bm),
        millis(bd),
        millis(xm),
        millis(xd),
        dd / dm,
        bd / bm,
        millis(upper),
        millis(threshold),
        classify(xm, upper, threshold),
    );
}

fn timed_recorder_disabled(request: &BranchRequest, expected: &FirstDivergence) -> (u128, u128) {
    let start = Instant::now();
    let output = run_counterfactual_branch_with_analysis_mode(
        request,
        BranchAnalysisMode::RecorderComparisonsDisabled {
            expected: expected.clone(),
        },
    )
    .unwrap();
    let elapsed = start.elapsed().as_nanos();
    let replay = match &output {
        BranchAnalysisOutput::Full { costs, .. } => costs.recorder_replay_ns,
        BranchAnalysisOutput::AnalysisDisabled(_) => unreachable!(),
    };
    black_box(output);
    (elapsed, replay)
}

fn recorder_cell(population: u64) {
    let cfg = config(population, false, true);
    let request = make_request(
        &cfg,
        BranchExecutionOption::Serial,
        BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::Idle,
        },
        1,
    );
    let id = format!("recorder_m3p_serial_n{population}");
    let expected = run_counterfactual_branch(&request)
        .unwrap()
        .first_divergence
        .expect("forced action should cause a located divergence");
    let mut full = Vec::with_capacity(SAMPLES);
    let mut comparison_disabled = Vec::with_capacity(SAMPLES);
    let mut delta = Vec::with_capacity(SAMPLES);
    let mut replay = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let full_first = sample % 2 == 0;
        let (f, c, r) = if full_first {
            let f = timed(|| {
                black_box(run_counterfactual_branch(&request).unwrap());
            });
            let (c, r) = timed_recorder_disabled(&request, &expected);
            (f, c, r)
        } else {
            let (c, r) = timed_recorder_disabled(&request, &expected);
            let f = timed(|| {
                black_box(run_counterfactual_branch(&request).unwrap());
            });
            (f, c, r)
        };
        if sample >= WARMUPS {
            full.push(f as f64);
            comparison_disabled.push(c as f64);
            delta.push(f as f64 - c as f64);
            replay.push(r as f64);
        }
        println!("SAMPLE,{},{}", id, sample);
    }
    let (fm, fd) = median_mad(&full);
    let (cm, cd) = median_mad(&comparison_disabled);
    let (xm, xd) = median_mad(&delta);
    let (rm, rd) = median_mad(&replay);
    let upper = bootstrap_upper(&delta);
    let threshold = 0.05 * cm;
    println!(
        "RECORDER,{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
        id,
        millis(fm),
        millis(fd),
        millis(cm),
        millis(cd),
        millis(xm),
        millis(xd),
        fd / fm,
        cd / cm,
        millis(upper),
        millis(threshold),
        millis(rm),
        millis(rd),
        classify(xm, upper, threshold),
    );
}

fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for (population, label) in [(10_000, "10k"), (100_000, "100k")] {
        let k = population / 200;
        for (profile, popdyn) in [("m3", false), ("m3p", true)] {
            let cfg = config(population, false, popdyn);
            let request = make_request(
                &cfg,
                BranchExecutionOption::Serial,
                BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BaseMetabolicCost,
                    value: ConfigValue::F32(cfg.environment.base_metabolic_cost),
                },
                HORIZON,
            );
            cells.push(Cell {
                id: format!("{profile}_serial_n{label}_k{k}"),
                request,
            });
        }
        if population == 100_000 {
            let cfg = config(population, true, false);
            let request = make_request(
                &cfg,
                BranchExecutionOption::Serial,
                BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BaseMetabolicCost,
                    value: ConfigValue::F32(cfg.environment.base_metabolic_cost),
                },
                HORIZON,
            );
            cells.push(Cell {
                id: format!("m3prop_serial_n{label}_k{k}"),
                request,
            });
        }
        if population <= 100_000 {
            let cfg = config(population, false, true);
            let request = make_request(
                &cfg,
                BranchExecutionOption::IncrementalTopology,
                BranchIntervention::ConfigOverride {
                    parameter: ConfigParameter::BaseMetabolicCost,
                    value: ConfigValue::F32(cfg.environment.base_metabolic_cost),
                },
                HORIZON,
            );
            cells.push(Cell {
                id: format!("m3p_incremental_n{label}_k{k}"),
                request,
            });
        }
    }
    cells
}

fn main() {
    let selected = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let all = cells();
    let all_selected = selected == "all";
    let mut selected_ids: Vec<String> = if all_selected {
        all.iter().map(|cell| cell.id.clone()).collect()
    } else {
        selected.split(',').map(str::to_owned).collect()
    };
    if all_selected {
        selected_ids.push("recorder_m3p_serial_n10000".into());
        selected_ids.push("recorder_m3p_serial_n100000".into());
    }
    for id in selected_ids {
        if id.starts_with("recorder_m3p_serial_n") {
            let n = id
                .strip_prefix("recorder_m3p_serial_n")
                .unwrap()
                .parse()
                .unwrap();
            recorder_cell(n);
        } else {
            let cell = all.iter().find(|cell| cell.id == id).unwrap();
            orchestration_cell(cell);
        }
    }
}
