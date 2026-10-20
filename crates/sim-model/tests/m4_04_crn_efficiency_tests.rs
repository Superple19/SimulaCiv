use sim_core::GroupId;
use sim_model::crn_efficiency::{
    CrnEfficiencyRequest, CrnOutcome, CrnVarianceRatio, run_crn_efficiency,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchIntervention, SimConfig, SimulationProfile, initialize_world,
};
use std::fmt::Write;

fn request(population: u64, replicate_count: usize, horizons: Vec<u32>) -> CrnEfficiencyRequest {
    let mut config =
        SimConfig::parse_and_validate(include_str!("fixtures/m3p_propagation_sustained.toml"))
            .unwrap();
    config.world.initial_population = population;
    config.world.settlement_count = 3;
    config.world.master_seed = 81_985_529_216_486_895;
    config.world.replicate_id = 7;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.validate().unwrap();

    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &context).unwrap();
    CrnEfficiencyRequest {
        config,
        branch_point: encode_m3p_snapshot(&state, &metadata).unwrap(),
        intervention: BranchIntervention::AgentFieldOverride {
            agent_id: sim_core::AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        target_group_id: GroupId(0),
        horizons,
        replicate_count,
    }
}

fn encode_result(result: &sim_model::crn_efficiency::CrnEfficiencyResult) -> String {
    let mut output = String::from(
        "# outcome|H|paired-mean|paired-mean-ci-low|paired-mean-ci-high|paired-sample-var|paired-est-var|independent-mean|independent-base-var|independent-treatment-var|independent-est-var|ratio|ratio-ci-low|ratio-ci-high|mean-D|mean-newborn-crn-loss\n",
    );
    for estimate in &result.estimates {
        let (ratio, ci_low, ci_high) = match estimate.variance_ratio {
            CrnVarianceRatio::ExactPairedVarianceZero => {
                ("exact".to_owned(), "".to_owned(), "".to_owned())
            }
            CrnVarianceRatio::Estimated {
                ratio,
                ci95_lower,
                ci95_upper,
            } => (
                format!("{:016x}", ratio.to_bits()),
                format!("{:016x}", ci95_lower.to_bits()),
                format!("{:016x}", ci95_upper.to_bits()),
            ),
        };
        writeln!(
            output,
            "{}|{}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{:016x}|{}|{}|{}|{:016x}|{:016x}",
            estimate.outcome,
            estimate.horizon_days,
            estimate.paired_mean_effect.to_bits(),
            estimate.paired_mean_ci95_lower.to_bits(),
            estimate.paired_mean_ci95_upper.to_bits(),
            estimate.paired_sample_variance.to_bits(),
            estimate.paired_estimator_variance.to_bits(),
            estimate.independent_mean_effect.to_bits(),
            estimate.independent_base_sample_variance.to_bits(),
            estimate.independent_intervention_sample_variance.to_bits(),
            estimate.independent_estimator_variance.to_bits(),
            ratio,
            ci_low,
            ci_high,
            estimate.mean_paired_divergent_records.to_bits(),
            estimate.mean_newborn_crn_loss.to_bits(),
        )
        .unwrap();
    }
    output
}

#[test]
fn replicate_runner_is_deterministic_for_a_small_horizon() {
    let request = request(300, 2, vec![7]);
    assert_eq!(
        run_crn_efficiency(&request).unwrap(),
        run_crn_efficiency(&request).unwrap()
    );
}

#[test]
fn m3p_crn_replicate_set_matches_the_committed_ci_golden() {
    let result = run_crn_efficiency(&request(2_000, 10, vec![30])).unwrap();
    assert_eq!(result.estimates.len(), CrnOutcome::ALL.len());
    assert_eq!(
        encode_result(&result),
        include_str!("fixtures/m4_04_crn_efficiency_golden.txt").replace("\r\n", "\n")
    );
}
