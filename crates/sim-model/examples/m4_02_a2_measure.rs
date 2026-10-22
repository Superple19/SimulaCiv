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
const SAMPLES: usize = 21;
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
        intervention: BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(config.environment.base_metabolic_cost),
        },
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

fn millis(ns: f64) -> f64 {
    ns / 1_000_000.0
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
    let center = median(values);
    let deviations: Vec<_> = values.iter().map(|value| (value - center).abs()).collect();
    (center, median(&deviations))
}

fn timed<F: FnOnce()>(f: F) -> u128 {
    let start = Instant::now();
    f();
    start.elapsed().as_nanos()
}

fn orchestration_cell(cell: &Cell) {
    std::fs::write("target/m4_02_a2_current_cell.txt", &cell.id).unwrap();
    let mut direct = Vec::with_capacity(SAMPLES);
    let mut disabled = Vec::with_capacity(SAMPLES);
    let mut delta = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let direct_first = sample % 2 == 0;
        let (direct_ns, disabled_ns) = if direct_first {
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
            direct.push(direct_ns as f64);
            disabled.push(disabled_ns as f64);
            delta.push(disabled_ns as f64 - direct_ns as f64);
        }
        println!("SAMPLE,{},{}", cell.id, sample);
    }
    let (direct_med, direct_mad) = median_mad(&direct);
    let (disabled_med, disabled_mad) = median_mad(&disabled);
    let (delta_med, delta_mad) = median_mad(&delta);
    let limit = 0.05 * direct_med;
    let result = if delta_med + 2.0 * delta_mad <= limit {
        "PASS"
    } else if delta_med > limit {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    };
    println!(
        "ORCH,{},{},{},{},{},{},{},{},{}",
        cell.id,
        millis(direct_med),
        millis(direct_mad),
        millis(disabled_med),
        millis(disabled_mad),
        millis(delta_med),
        millis(delta_mad),
        millis(limit),
        result,
    );
}

fn recorder_cell(population: u64) {
    let config = config(population, false, true);
    let mut request = make_request(&config, BranchExecutionOption::Serial, 1);
    request.intervention = BranchIntervention::ForcedAction {
        agent_id: AgentId(0),
        action: Action::Idle,
    };
    let id = format!("recorder_m3p_serial_n{population}");
    std::fs::write("target/m4_02_a2_current_cell.txt", &id).unwrap();
    let full_result = run_counterfactual_branch(&request).unwrap();
    let expected = full_result
        .first_divergence
        .expect("forced action should produce a phase-located divergence");
    let mut full = Vec::with_capacity(SAMPLES);
    let mut disabled = Vec::with_capacity(SAMPLES);
    let mut delta = Vec::with_capacity(SAMPLES);
    let mut replay = Vec::with_capacity(SAMPLES);
    for sample in 0..(WARMUPS + SAMPLES) {
        let full_first = sample % 2 == 0;
        let (full_ns, disabled_ns, replay_ns) = if full_first {
            let f = timed(|| {
                black_box(run_counterfactual_branch(&request).unwrap());
            });
            let (b, replay) = timed_recorder_disabled(&request, &expected);
            (f, b, replay)
        } else {
            let (b, replay) = timed_recorder_disabled(&request, &expected);
            let f = timed(|| {
                black_box(run_counterfactual_branch(&request).unwrap());
            });
            (f, b, replay)
        };
        if sample >= WARMUPS {
            full.push(full_ns as f64);
            disabled.push(disabled_ns as f64);
            delta.push(full_ns as f64 - disabled_ns as f64);
            replay.push(replay_ns as f64);
        }
        println!("SAMPLE,{},{}", id, sample);
    }
    let (full_med, full_mad) = median_mad(&full);
    let (disabled_med, disabled_mad) = median_mad(&disabled);
    let (delta_med, delta_mad) = median_mad(&delta);
    let (replay_med, replay_mad) = median_mad(&replay);
    let limit = 0.05 * disabled_med;
    let result = if delta_med + 2.0 * delta_mad <= limit {
        "PASS"
    } else if delta_med > limit {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    };
    println!(
        "RECORDER,{},{},{},{},{},{},{},{},{},{},{}",
        id,
        millis(full_med),
        millis(full_mad),
        millis(disabled_med),
        millis(disabled_mad),
        millis(delta_med),
        millis(delta_mad),
        millis(replay_med),
        millis(replay_mad),
        millis(limit),
        result,
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
        BranchAnalysisOutput::Full { result: _, costs } => costs.recorder_replay_ns,
        BranchAnalysisOutput::AnalysisDisabled(_) => unreachable!(),
    };
    black_box(output);
    (elapsed, replay)
}

fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for (population, label) in [(10_000, "10k"), (100_000, "100k")] {
        let k = population / 200;
        for (profile, prop, popdyn) in [("m3", false, false), ("m3p", false, true)] {
            let cfg = config(population, prop, popdyn);
            let request = make_request(&cfg, BranchExecutionOption::Serial, HORIZON);
            cells.push(Cell {
                id: format!("{profile}_serial_n{label}_k{k}"),
                request,
            });
        }
        if population == 100_000 {
            let cfg = config(population, true, false);
            let request = make_request(&cfg, BranchExecutionOption::Serial, HORIZON);
            cells.push(Cell {
                id: format!("m3prop_serial_n{label}_k{k}"),
                request,
            });
        }
        if population <= 100_000 {
            let cfg = config(population, false, true);
            let request = make_request(&cfg, BranchExecutionOption::IncrementalTopology, HORIZON);
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
    if selected == "all" {
        for cell in cells() {
            orchestration_cell(&cell);
        }
        recorder_cell(10_000);
        recorder_cell(100_000);
    } else if selected.starts_with("recorder_m3p_serial_n") {
        let population = selected
            .strip_prefix("recorder_m3p_serial_n")
            .unwrap()
            .parse()
            .unwrap();
        recorder_cell(population);
    } else {
        let cell = cells()
            .into_iter()
            .find(|cell| cell.id == selected)
            .unwrap();
        orchestration_cell(&cell);
    }
}
