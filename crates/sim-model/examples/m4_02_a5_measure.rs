// restored from session history; not re-run
use sim_core::AgentId;
use sim_model::config::{SimulationConfig, SimulationProfile, SimulationSystems};
use sim_model::counterfactual::{
    BranchAnalysisMode, BranchAnalysisOutput, BranchExecutionOption, BranchIntervention,
    BranchRequest, ConfigParameter, ConfigValue, FirstDivergence, run_counterfactual_branch,
    run_counterfactual_branch_with_analysis_mode,
};
use sim_model::m3p::{M3pReferenceState, decode_m3p_snapshot, encode_m3p_snapshot};
use sim_model::m3prop::{
    M3PropagationReferenceState, decode_m3prop_snapshot, encode_m3prop_snapshot,
};
use sim_model::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext,
    M3PropagationStorageRunContext, M3StorageRunContext, M3pIncrementalTopologyStorageRunContext,
    M3pStorageRunContext, run_m3_storage_days, run_m3p_storage_days,
    run_m3p_storage_days_incremental_topology, run_m3prop_storage_days,
};
use sim_model::snapshot::{CanonicalSnapshot, SnapshotMetadata, decode_snapshot, encode_snapshot};
use sim_model::state::{AuthorityMode, HybridWorldState};
use sim_model::{
    M3pPropagationReferenceState, SimConfig, encode_m3pprop_snapshot, initialize_world,
};
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const DAYS: u32 = 30;
const SAMPLES: usize = 41;
const TARGET_SAMPLE_NS: u128 = 1_000_000_000;

#[derive(Clone, Copy)]
enum Runner {
    M3,
    M3p,
    M3pIncremental,
    M3Propagation,
    Recorder,
}

#[derive(Clone)]
struct Cell {
    id: String,
    runner: Runner,
    config: SimConfig,
    run_context: M0RunContext,
    snapshot: CanonicalSnapshot,
    request: BranchRequest,
    expected: Option<FirstDivergence>,
}

#[derive(Clone, Copy)]
enum Path {
    DirectPair,
    BranchDisabled,
    RecorderFull,
    RecorderDisabled,
}

fn config(population: u32, settlements: u32, runner: Runner) -> SimConfig {
    let m3p = matches!(
        runner,
        Runner::M3p | Runner::M3pIncremental | Runner::Recorder
    );
    let mut config = SimConfig::parse_and_validate(if m3p {
        include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
    } else {
        include_str!("../tests/fixtures/m3_propagation.toml")
    })
    .unwrap();
    config.world.initial_population = u64::from(population);
    config.world.settlement_count = settlements;
    config.simulation = None;
    if m3p {
        config.profile = SimulationProfile::M3p;
    } else {
        config.profile = SimulationProfile::M3;
        config.population = None;
    }
    if matches!(runner, Runner::M3Propagation) {
        config.propagation = Some(config.propagation.unwrap());
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics: false,
                propagation: true,
            },
        });
    } else {
        config.propagation = None;
    }
    config.validate().unwrap();
    config
}

fn snapshot(config: &SimConfig, context: &M0RunContext) -> CanonicalSnapshot {
    let world = initialize_world(config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    match config.runner_selection() {
        sim_model::SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
        sim_model::SimulationRunnerSelection::M3Propagation => {
            let state = M3PropagationReferenceState::new(world, config, context).unwrap();
            encode_m3prop_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M3p => {
            let state = M3pReferenceState::new(world, config, context).unwrap();
            encode_m3p_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M3pPropagation => {
            let state = M3pPropagationReferenceState::new(world, config, context).unwrap();
            encode_m3pprop_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M2 => unreachable!(),
    }
}

fn make_cell(id: &str, runner: Runner, population: u32, recorder: bool) -> Cell {
    let settlements = population / 200;
    let config = config(population, settlements, runner);
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let snapshot = snapshot(&config, &run_context);
    let intervention = if recorder {
        BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: sim_model::decision::Action::Idle,
        }
    } else {
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(config.environment.base_metabolic_cost),
        }
    };
    let request = BranchRequest {
        snapshot: snapshot.clone(),
        config: config.clone(),
        run_context,
        intervention,
        horizon_days: if recorder { 1 } else { DAYS },
        execution: BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    };
    let expected = if recorder {
        let result = run_counterfactual_branch(&request).unwrap();
        Some(
            result
                .first_divergence
                .expect("forced action locates a phase"),
        )
    } else {
        None
    };
    Cell {
        id: id.to_string(),
        runner,
        config,
        run_context,
        snapshot,
        request,
        expected,
    }
}

fn direct_pass(cell: &Cell) {
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    match cell.runner {
        Runner::M3 => {
            let restored = decode_snapshot(cell.snapshot.as_ref()).unwrap();
            let mut world = HybridWorldState::new(restored.world, AuthorityMode::Hybrid);
            let mut context =
                M3StorageRunContext::new(&world, &cell.config, &cell.run_context).unwrap();
            std::hint::black_box(
                run_m3_storage_days(&mut world, &cell.config, &mut context, DAYS, &options)
                    .unwrap(),
            );
        }
        Runner::M3p => {
            let restored = decode_m3p_snapshot(cell.snapshot.as_ref(), &cell.config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            let mut context = M3pStorageRunContext::from_reference_state(
                &world,
                &cell.config,
                &cell.run_context,
                &restored.state,
            )
            .unwrap();
            std::hint::black_box(
                run_m3p_storage_days(&mut world, &cell.config, &mut context, DAYS, &options)
                    .unwrap(),
            );
        }
        Runner::M3pIncremental => {
            let restored = decode_m3p_snapshot(cell.snapshot.as_ref(), &cell.config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            let mut context = M3pIncrementalTopologyStorageRunContext::from_reference_state(
                &world,
                &cell.config,
                &cell.run_context,
                &restored.state,
            )
            .unwrap();
            std::hint::black_box(
                run_m3p_storage_days_incremental_topology(
                    &mut world,
                    &cell.config,
                    &mut context,
                    DAYS,
                    &options,
                )
                .unwrap(),
            );
        }
        Runner::M3Propagation => {
            let restored = decode_m3prop_snapshot(cell.snapshot.as_ref(), &cell.config).unwrap();
            let mut world =
                HybridWorldState::new(restored.state.world().clone(), AuthorityMode::Hybrid);
            let mut context = M3PropagationStorageRunContext::from_reference_state(
                &world,
                &cell.config,
                &cell.run_context,
                &restored.state,
            )
            .unwrap();
            std::hint::black_box(
                run_m3prop_storage_days(&mut world, &cell.config, &mut context, DAYS, &options)
                    .unwrap(),
            );
        }
        Runner::Recorder => unreachable!(),
    }
}

fn run_path(cell: &Cell, path: Path) {
    match path {
        Path::DirectPair => {
            direct_pass(cell);
            direct_pass(cell);
        }
        Path::BranchDisabled => {
            std::hint::black_box(
                run_counterfactual_branch_with_analysis_mode(
                    &cell.request,
                    BranchAnalysisMode::Disabled,
                )
                .unwrap(),
            );
        }
        Path::RecorderFull => {
            std::hint::black_box(run_counterfactual_branch(&cell.request).unwrap());
        }
        Path::RecorderDisabled => {
            std::hint::black_box(
                run_counterfactual_branch_with_analysis_mode(
                    &cell.request,
                    BranchAnalysisMode::RecorderComparisonsDisabled {
                        expected: cell.expected.clone().unwrap(),
                    },
                )
                .unwrap(),
            );
        }
    }
}

fn timed(cell: &Cell, path: Path) -> u128 {
    let started = Instant::now();
    run_path(cell, path);
    started.elapsed().as_nanos()
}

fn run_batch(cell: &Cell, r: usize, start_direct: bool) -> (f64, f64) {
    let (direct, branch) = if matches!(cell.runner, Runner::Recorder) {
        (Path::RecorderDisabled, Path::RecorderFull)
    } else {
        (Path::DirectPair, Path::BranchDisabled)
    };
    let (mut direct_ns, mut branch_ns) = (0u128, 0u128);
    for index in 0..r {
        let direct_first = (index % 2 == 0) == start_direct;
        if direct_first {
            direct_ns += timed(cell, direct);
            branch_ns += timed(cell, branch);
        } else {
            branch_ns += timed(cell, branch);
            direct_ns += timed(cell, direct);
        }
    }
    (
        direct_ns as f64 / r as f64 / 1_000_000.0,
        branch_ns as f64 / r as f64 / 1_000_000.0,
    )
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
    let deviations = values
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    median(&deviations)
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

fn run_cell(cell: &Cell, fixed_r: Option<usize>, sample_file: &mut File) -> usize {
    let r = if let Some(r) = fixed_r {
        r
    } else {
        let elapsed = timed(
            cell,
            if matches!(cell.runner, Runner::Recorder) {
                Path::RecorderFull
            } else {
                Path::DirectPair
            },
        ) + timed(
            cell,
            if matches!(cell.runner, Runner::Recorder) {
                Path::RecorderDisabled
            } else {
                Path::BranchDisabled
            },
        );
        usize::try_from(TARGET_SAMPLE_NS.div_ceil(elapsed.max(1)))
            .unwrap()
            .max(1)
    };
    println!("REPETITIONS\t{}\t{r}", cell.id);
    run_batch(cell, r, true);
    run_batch(cell, r, false);
    let (mut direct, mut other, mut deltas) = (Vec::new(), Vec::new(), Vec::new());
    for sample in 0..SAMPLES {
        let (direct_ms, other_ms) = run_batch(cell, r, sample % 2 == 0);
        let delta = other_ms - direct_ms;
        writeln!(
            sample_file,
            "{}\t{sample}\t{direct_ms:.9}\t{other_ms:.9}\t{delta:.9}",
            cell.id
        )
        .unwrap();
        direct.push(direct_ms);
        other.push(other_ms);
        deltas.push(delta);
    }
    let direct_median = median(&direct);
    let other_median = median(&other);
    let delta_median = median(&deltas);
    let delta_mad = mad(&deltas);
    let u = bootstrap_u(&deltas);
    let threshold = 0.05
        * if matches!(cell.runner, Runner::Recorder) {
            other_median
        } else {
            direct_median
        };
    let direct_spread = 100.0 * mad(&direct) / direct_median.abs();
    let other_spread = 100.0 * mad(&other) / other_median.abs();
    let valid = direct_spread <= 5.0 && other_spread <= 5.0;
    let status = if !valid {
        "INVALID"
    } else if u <= threshold {
        "PASS"
    } else if delta_median > threshold {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    };
    println!(
        "RESULT\t{}\t{r}\t{direct_median:.6}\t{:.6}\t{other_median:.6}\t{:.6}\t{delta_median:.6}\t{delta_mad:.6}\t{u:.6}\t{threshold:.6}\t{direct_spread:.3}\t{other_spread:.3}\t{status}",
        cell.id,
        mad(&direct),
        mad(&other),
    );
    r
}

fn cells() -> Vec<Cell> {
    vec![
        make_cell("m3-10k", Runner::M3, 10_000, false),
        make_cell("m3p-10k", Runner::M3p, 10_000, false),
        make_cell("m3p-incremental-10k", Runner::M3pIncremental, 10_000, false),
        make_cell("m3-100k", Runner::M3, 100_000, false),
        make_cell("m3p-100k", Runner::M3p, 100_000, false),
        make_cell("m3prop-100k", Runner::M3Propagation, 100_000, false),
        make_cell(
            "m3p-incremental-100k",
            Runner::M3pIncremental,
            100_000,
            false,
        ),
        make_cell("recorder-10k", Runner::Recorder, 10_000, true),
        make_cell("recorder-100k", Runner::Recorder, 100_000, true),
    ]
}

fn parse_rmap(value: Option<String>) -> HashMap<String, usize> {
    let Some(value) = value else {
        return HashMap::new();
    };
    value
        .split(',')
        .filter_map(|entry| {
            let (id, r) = entry.split_once('=')?;
            Some((id.to_string(), r.parse().ok()?))
        })
        .collect()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let selected = args.next().unwrap_or_else(|| "all".to_string());
    let rmap = parse_rmap(args.next());
    let selected = selected.split(',').collect::<Vec<_>>();
    let mut output = File::create("target/m4_02_a5_samples.tsv").unwrap();
    for cell in cells() {
        if selected != ["all"] && !selected.contains(&cell.id.as_str()) {
            continue;
        }
        let fixed_r = rmap.get(&cell.id).copied();
        run_cell(&cell, fixed_r, &mut output);
        output.flush().unwrap();
    }
    if let Some(name) = std::env::var_os("M4_A5_COST_CELL") {
        let name = name.to_string_lossy();
        let cell = cells().into_iter().find(|cell| cell.id == name).unwrap();
        if !matches!(cell.runner, Runner::Recorder) {
            return;
        }
        let mut costs = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let BranchAnalysisOutput::Full {
                costs: measured, ..
            } = run_counterfactual_branch_with_analysis_mode(
                &cell.request,
                BranchAnalysisMode::Full,
            )
            .unwrap()
            else {
                unreachable!()
            };
            costs.push(measured.recorder_compare_ns as f64 / 1_000_000.0);
        }
        println!(
            "COMPARE_COST\t{name}\t{:.6}\t{:.6}",
            median(&costs),
            mad(&costs)
        );
    }
}
