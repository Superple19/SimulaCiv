// restored from session history; not re-run
use sim_core::{AgentId, GroupId};
use sim_model::crn_efficiency::{CrnEfficiencyRequest, CrnVarianceRatio, run_crn_efficiency};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchIntervention, ConfigParameter, ConfigValue, SimConfig,
    SimulationProfile, initialize_world,
};
use std::fs;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(include_str!(
        "../tests/fixtures/m3p_propagation_sustained.toml"
    ))?;
    config.world.initial_population = 10_000;
    config.world.settlement_count = 50;
    config.world.master_seed = 81_985_529_216_486_895;
    config.world.replicate_id = 7;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.validate()?;

    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config)?;
    let target_group = world
        .settlements
        .iter()
        .max_by_key(|settlement| {
            (
                world
                    .agents
                    .iter()
                    .filter(|agent| agent.alive && agent.group_id == settlement.group_id)
                    .count(),
                std::cmp::Reverse(settlement.group_id),
            )
        })
        .expect("initialization creates settlements")
        .group_id;
    let target_agent = world
        .agents
        .iter()
        .filter(|agent| agent.alive && agent.group_id == target_group)
        .map(|agent| agent.agent_id)
        .min()
        .expect("largest settlement has an alive agent");
    assert_eq!(target_agent, AgentId(0));
    assert_eq!(target_group, GroupId(0));

    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &context)?;
    let branch_point = encode_m3p_snapshot(&state, &metadata)?;

    let interventions = [
        (
            "food_0",
            BranchIntervention::AgentFieldOverride {
                agent_id: target_agent,
                field: AgentField::Food,
                value: AgentFieldValue::Food(0.0),
            },
        ),
        (
            "health_0_01",
            BranchIntervention::AgentFieldOverride {
                agent_id: target_agent,
                field: AgentField::Health,
                value: AgentFieldValue::Health(0.01),
            },
        ),
        (
            "base_metabolic_cost_plus_1pct",
            BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(config.environment.base_metabolic_cost * 1.01),
            },
        ),
    ];

    let total_start = Instant::now();
    let mut output = String::from(
        "intervention|horizon|outcome|paired_mean_effect|paired_ci95_low|paired_ci95_high|paired_sample_variance|paired_estimator_variance|independent_mean_effect|independent_base_sample_variance|independent_intervention_sample_variance|independent_estimator_variance|variance_ratio|ratio_ci95_low|ratio_ci95_high|mean_D_H|mean_newborn_crn_loss\n",
    );
    for (name, intervention) in interventions {
        let start = Instant::now();
        let result = run_crn_efficiency(&CrnEfficiencyRequest {
            config: config.clone(),
            branch_point: branch_point.clone(),
            intervention,
            target_group_id: target_group,
            horizons: vec![7, 30, 90, 365],
            replicate_count: 100,
        })?;
        for estimate in &result.estimates {
            let (ratio, low, high) = match estimate.variance_ratio {
                CrnVarianceRatio::ExactPairedVarianceZero => {
                    ("exact".to_owned(), String::new(), String::new())
                }
                CrnVarianceRatio::Estimated {
                    ratio,
                    ci95_lower,
                    ci95_upper,
                } => (
                    format!("{ratio:.12e}"),
                    format!("{ci95_lower:.12e}"),
                    format!("{ci95_upper:.12e}"),
                ),
            };
            output.push_str(&format!(
                "{name}|{}|{}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{:.12e}|{ratio}|{low}|{high}|{:.6}|{:.6}\n",
                estimate.horizon_days,
                estimate.outcome,
                estimate.paired_mean_effect,
                estimate.paired_mean_ci95_lower,
                estimate.paired_mean_ci95_upper,
                estimate.paired_sample_variance,
                estimate.paired_estimator_variance,
                estimate.independent_mean_effect,
                estimate.independent_base_sample_variance,
                estimate.independent_intervention_sample_variance,
                estimate.independent_estimator_variance,
                estimate.mean_paired_divergent_records,
                estimate.mean_newborn_crn_loss,
            ));
        }
        fs::write(format!("target/m4_04_crn_{name}.csv"), &output)?;
        println!(
            "completed {name} in {:.1} minutes",
            start.elapsed().as_secs_f64() / 60.0
        );
    }
    fs::write("target/m4_04_crn_efficiency.csv", output)?;
    println!("TOTAL_SECONDS={:.3}", total_start.elapsed().as_secs_f64());
    Ok(())
}
