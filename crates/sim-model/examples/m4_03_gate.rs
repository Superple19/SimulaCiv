// re-run in M5-04: A-5 cells completed with High priority and affinity 0x555
use sim_model::config::{SimulationConfig, SimulationSystems};
use sim_model::counterfactual::{
    BranchExecutionOption, BranchIntervention, BranchRequest, ConfigParameter, ConfigValue,
    run_counterfactual_branch, run_counterfactual_branch_with_divergence_series,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::m3prop::{M3PropagationReferenceState, encode_m3prop_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot};
use sim_model::{SimConfig, SimulationProfile, SimulationRunnerSelection, initialize_world};
use std::time::Instant;

fn request(n: u64, population: bool, seed: u64) -> BranchRequest {
    let fixture = if population {
        include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
    } else {
        include_str!("../tests/fixtures/m3_propagation.toml")
    };
    let mut config = SimConfig::parse_and_validate(fixture).unwrap();
    config.world.initial_population = n;
    config.world.master_seed = seed;
    config.world.replicate_id = 7;
    if population {
        config.profile = SimulationProfile::M3p;
        config.simulation = None;
        config.propagation = None;
    } else {
        config.profile = SimulationProfile::M3;
        config.population = None;
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics: false,
                propagation: true,
            },
        });
    }
    config.validate().unwrap();
    let context = M0RunContext::new(seed, 7);
    let world = initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        seed,
        7,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let snapshot = match config.runner_selection() {
        SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
        SimulationRunnerSelection::M3p => {
            let state = M3pReferenceState::new(world, &config, &context).unwrap();
            encode_m3p_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M3Propagation => {
            let state = M3PropagationReferenceState::new(world, &config, &context).unwrap();
            encode_m3prop_snapshot(&state, &metadata).unwrap()
        }
        _ => panic!("unsupported gate profile"),
    };
    BranchRequest {
        config,
        run_context: context,
        snapshot,
        intervention: BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(
                SimConfig::parse_and_validate(fixture)
                    .unwrap()
                    .environment
                    .base_metabolic_cost,
            ),
        },
        horizon_days: 30,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![],
    }
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n.is_multiple_of(2) {
        (sorted[n / 2 - 1] + sorted[n / 2]) * 0.5
    } else {
        sorted[n / 2]
    }
}

fn mad(values: &[f64], center: f64) -> f64 {
    let deviations: Vec<_> = values.iter().map(|value| (value - center).abs()).collect();
    median(&deviations)
}

fn next_u64(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

fn bootstrap_u(deltas: &[f64]) -> f64 {
    let mut state = 0xA3_11_70;
    let mut values = Vec::with_capacity(10_000);
    for _ in 0..10_000 {
        let sample: Vec<_> = (0..deltas.len())
            .map(|_| deltas[(next_u64(&mut state) as usize) % deltas.len()])
            .collect();
        values.push(median(&sample));
    }
    values.sort_by(f64::total_cmp);
    values[9_499]
}

fn timed<F: FnOnce()>(f: F) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64() * 1_000_000.0
}

fn main() {
    println!("startup delay 10s; apply High priority and affinity 0x555 now");
    std::thread::sleep(std::time::Duration::from_secs(10));
    for (label, n, population) in [
        ("M3P-10k", 10_000, true),
        ("M3P-100k", 100_000, true),
        ("M3PROP-100k", 100_000, false),
    ] {
        let request = request(n, population, 81_985_529_216_486_895);
        let direct = run_counterfactual_branch(&request).unwrap();
        let series = run_counterfactual_branch_with_divergence_series(&request).unwrap();
        assert_eq!(series.branch, direct, "M4-02 output differs at {label}");

        for warmup in 0..2 {
            if warmup % 2 == 0 {
                let _ = run_counterfactual_branch(&request).unwrap();
                let _ = run_counterfactual_branch_with_divergence_series(&request).unwrap();
            } else {
                let _ = run_counterfactual_branch_with_divergence_series(&request).unwrap();
                let _ = run_counterfactual_branch(&request).unwrap();
            }
        }
        let direct_pair = timed(|| {
            let _ = run_counterfactual_branch(&request).unwrap();
            let _ = run_counterfactual_branch(&request).unwrap();
        });
        let mut r = (1.0 / (direct_pair / 2_000_000.0)).ceil() as usize;
        r = r.max(1);
        let mut direct_times = Vec::with_capacity(41);
        let mut series_times = Vec::with_capacity(41);
        let mut deltas = Vec::with_capacity(41);
        for sample in 0..41 {
            let (mut direct_total, mut series_total) = (0.0, 0.0);
            for repeat in 0..r {
                if (sample + repeat) % 2 == 0 {
                    direct_total += timed(|| {
                        let _ = run_counterfactual_branch(&request).unwrap();
                    });
                    series_total += timed(|| {
                        let _ = run_counterfactual_branch_with_divergence_series(&request).unwrap();
                    });
                } else {
                    series_total += timed(|| {
                        let _ = run_counterfactual_branch_with_divergence_series(&request).unwrap();
                    });
                    direct_total += timed(|| {
                        let _ = run_counterfactual_branch(&request).unwrap();
                    });
                }
            }
            let direct_us = direct_total / r as f64;
            let series_us = series_total / r as f64;
            direct_times.push(direct_us);
            series_times.push(series_us);
            deltas.push(series_us - direct_us);
            println!(
                "SAMPLE,{label},{sample},{direct_us:.3},{series_us:.3},{:.3}",
                series_us - direct_us
            );
        }
        let direct_median = median(&direct_times);
        let series_median = median(&series_times);
        let delta_median = median(&deltas);
        let delta_mad = mad(&deltas, delta_median);
        let direct_spread = mad(&direct_times, direct_median) / direct_median;
        let series_spread = mad(&series_times, series_median) / series_median;
        let upper = bootstrap_u(&deltas);
        let threshold = 0.05 * direct_median;
        println!(
            "CELL,{label},{r},{direct_median:.3},{:.3},{series_median:.3},{:.3},{delta_median:.3},{delta_mad:.3},{upper:.3},{threshold:.3},{direct_spread:.6},{series_spread:.6}",
            mad(&direct_times, direct_median),
            mad(&series_times, series_median)
        );
    }
}
