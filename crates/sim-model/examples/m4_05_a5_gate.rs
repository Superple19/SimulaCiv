// re-run in M5-04: A-5 raw samples are stored under docs/data/m5_04/m4_05/
use sim_core::AgentId;
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention,
    BranchInterventionBatchRequest, BranchRequest, IdentifiedIntervention, SimConfig,
    SimulationProfile, run_counterfactual_branch, run_counterfactual_intervention_batch,
};
use std::hint::black_box;
use std::time::{Duration, Instant};

const SAMPLES: usize = 41;
const WARMUPS: usize = 2;
const BOOTSTRAP_RESAMPLES: usize = 10_000;
const BOOTSTRAP_SEED: u64 = 0xA3_11_70;
const TARGET_SAMPLE: Duration = Duration::from_secs(2);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(include_str!(
        "../tests/fixtures/m3p_propagation_sustained.toml"
    ))?;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = 10_000;
    config.world.settlement_count = 50;
    config.world.master_seed = 81_985_529_216_486_895;
    config.world.replicate_id = 7;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    config.validate()?;

    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = sim_model::initialize_world(&config)?;
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let m3p_state = M3pReferenceState::new(world, &config, &context)?;
    let snapshot = encode_m3p_snapshot(&m3p_state, &metadata)?;
    let intervention = BranchIntervention::AgentFieldOverride {
        agent_id: AgentId(0),
        field: AgentField::Food,
        value: AgentFieldValue::Food(0.0),
    };
    let identified = IdentifiedIntervention::agent_field_override(
        AgentId(0),
        AgentField::Food,
        AgentFieldValue::Food(0.0),
    );
    let direct = BranchRequest {
        snapshot: snapshot.clone(),
        config: config.clone(),
        run_context: context,
        intervention,
        horizon_days: 30,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![29],
    };
    let batch = BranchInterventionBatchRequest {
        snapshot,
        config,
        run_context: context,
        interventions: vec![identified],
        horizon_days: 30,
        snapshot_days: vec![29],
    };

    let mut warmup_pairs = Vec::with_capacity(WARMUPS);
    for warmup in 0..WARMUPS {
        let start = Instant::now();
        let (direct_result, batch_result) = if warmup % 2 == 0 {
            let direct_result = run_counterfactual_branch(&direct)?;
            let batch_result = run_counterfactual_intervention_batch(&batch)?;
            (direct_result, batch_result)
        } else {
            let batch_result = run_counterfactual_intervention_batch(&batch)?;
            let direct_result = run_counterfactual_branch(&direct)?;
            (direct_result, batch_result)
        };
        assert_eq!(direct_result.base_days, batch_result.base_days);
        assert_eq!(direct_result.branch_days, batch_result.branch_days);
        warmup_pairs.push(start.elapsed());
    }
    let mean_pair_ns =
        warmup_pairs.iter().map(Duration::as_nanos).sum::<u128>() / warmup_pairs.len() as u128;
    let r = TARGET_SAMPLE.as_nanos().div_ceil(mean_pair_ns).max(1) as usize;

    let mut direct_samples = Vec::with_capacity(SAMPLES);
    let mut batch_samples = Vec::with_capacity(SAMPLES);
    let mut deltas = Vec::with_capacity(SAMPLES);
    let mut sample_wall = Vec::with_capacity(SAMPLES);
    for sample_index in 0..SAMPLES {
        let mut direct_total = Duration::ZERO;
        let mut batch_total = Duration::ZERO;
        let sample_start = Instant::now();
        for repetition in 0..r {
            if (sample_index + repetition) % 2 == 0 {
                let start = Instant::now();
                black_box(run_counterfactual_branch(black_box(&direct))?);
                direct_total += start.elapsed();
                let start = Instant::now();
                black_box(run_counterfactual_intervention_batch(black_box(&batch))?);
                batch_total += start.elapsed();
            } else {
                let start = Instant::now();
                black_box(run_counterfactual_intervention_batch(black_box(&batch))?);
                batch_total += start.elapsed();
                let start = Instant::now();
                black_box(run_counterfactual_branch(black_box(&direct))?);
                direct_total += start.elapsed();
            }
        }
        let wall = sample_start.elapsed();
        direct_samples.push(direct_total.as_secs_f64() * 1_000_000.0 / r as f64);
        batch_samples.push(batch_total.as_secs_f64() * 1_000_000.0 / r as f64);
        deltas.push(
            (batch_total.as_secs_f64() - direct_total.as_secs_f64()) * 1_000_000.0 / r as f64,
        );
        sample_wall.push(wall.as_secs_f64());
    }

    let direct_median = median(&direct_samples);
    let batch_median = median(&batch_samples);
    let direct_mad = mad(&direct_samples, direct_median);
    let batch_mad = mad(&batch_samples, batch_median);
    let delta_median = median(&deltas);
    let delta_mad = mad(&deltas, delta_median);
    let upper = bootstrap_upper(&deltas);
    let threshold = direct_median * 0.05;
    let valid = direct_mad / direct_median <= 0.05 && batch_mad / batch_median <= 0.05;
    let status = if !valid {
        "INVALID"
    } else if upper <= threshold {
        "PASS"
    } else if delta_median > threshold {
        "FAIL"
    } else {
        "INCONCLUSIVE"
    };
    println!(
        "GATE|status={status}|r={r}|samples={SAMPLES}|warmups={WARMUPS}|min_sample_s={:.6}|median_sample_s={:.6}|direct_median_us={direct_median:.3}|direct_mad_us={direct_mad:.3}|batch_median_us={batch_median:.3}|batch_mad_us={batch_mad:.3}|delta_median_us={delta_median:.3}|delta_mad_us={delta_mad:.3}|U_us={upper:.3}|T_us={threshold:.3}|direct_spread={:.6}|batch_spread={:.6}",
        sample_wall.iter().copied().fold(f64::INFINITY, f64::min),
        median(&sample_wall),
        direct_mad / direct_median,
        batch_mad / batch_median,
    );
    for (sample, ((direct, batch), delta)) in direct_samples
        .iter()
        .zip(&batch_samples)
        .zip(&deltas)
        .enumerate()
    {
        println!("SAMPLE,{sample},{r},{direct:.3},{batch:.3},{delta:.3}");
    }
    Ok(())
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn mad(values: &[f64], center: f64) -> f64 {
    let deviations = values
        .iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    median(&deviations)
}

fn bootstrap_upper(deltas: &[f64]) -> f64 {
    let mut state = BOOTSTRAP_SEED;
    let mut medians = Vec::with_capacity(BOOTSTRAP_RESAMPLES);
    for _ in 0..BOOTSTRAP_RESAMPLES {
        let sample = (0..deltas.len())
            .map(|_| deltas[next_u64(&mut state) as usize % deltas.len()])
            .collect::<Vec<_>>();
        medians.push(median(&sample));
    }
    medians.sort_by(f64::total_cmp);
    medians[9_499]
}

fn next_u64(state: &mut u64) -> u64 {
    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *state = value;
    value
}
