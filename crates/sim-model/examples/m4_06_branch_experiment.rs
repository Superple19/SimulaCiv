use serde::Serialize;
use serde_json::json;
use sim_core::{AgentId, GroupId};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention, BranchRequest,
    M0RunContext, M3pReferenceState, MigrationConfig, MigrationProfileTag, MigrationState,
    SimConfig, SimulationConfig, SimulationProfile, SimulationSystems, encode_m3p_snapshot,
    encode_migration_snapshot, initialize_world, run_counterfactual_branch_with_divergence_series,
};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;

const HORIZON: u32 = 365;
const REPLICATE: u32 = 7;
const SEEDS: [u64; 5] = [0, 1, 3, 7, 81_985_529_216_486_895];
const MIGRATION: MigrationConfig = MigrationConfig {
    migration_probability: 0.02,
    migration_threshold: 0.2,
    migration_cost: 10,
    migration_cooldown_days: 30,
};

#[derive(Serialize)]
struct DailyRecord {
    day: u32,
    base_moves: u64,
    branch_moves: u64,
    changed_agent_records: u64,
    matched_records: u64,
    divergence_fraction: f64,
    daily_settlements_touched: u64,
    cumulative_settlements_touched: u64,
    first_other_settlement_day: Option<u32>,
    divergence_by_settlement: Vec<(u16, u64)>,
}

fn config_for(seed: u64, migration: bool) -> Result<SimConfig, Box<dyn Error>> {
    let source = include_str!("../tests/fixtures/m3p_propagation_sustained.toml")
        .replace("propagation = true", "propagation = false");
    let source = if let Some(index) = source.find("\n[propagation]") {
        source[..index].to_owned()
    } else {
        source
    };
    let mut config = SimConfig::from_toml_str(&source)?;
    config.profile = SimulationProfile::M3p;
    config.population = Some(config.population.ok_or("M3P population config missing")?);
    config.propagation = None;
    config.simulation = Some(SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: true,
            propagation: false,
        },
    });
    config.world.initial_population = 10_000;
    config.world.settlement_count = 50;
    config.world.master_seed = seed;
    config.world.replicate_id = REPLICATE;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    if migration {
        config.enable_migration(MIGRATION);
    }
    config.validate()?;
    Ok(config)
}

fn target_agent(world: &sim_model::WorldState) -> (AgentId, GroupId) {
    let mut counts = BTreeMap::<GroupId, usize>::new();
    for agent in &world.agents {
        if agent.alive {
            *counts.entry(agent.group_id).or_default() += 1;
        }
    }
    let max_count = counts
        .values()
        .copied()
        .max()
        .expect("initial population is positive");
    let group = counts
        .iter()
        .find_map(|(&group, &count)| (count == max_count).then_some(group))
        .expect("largest settlement exists");
    let agent = world
        .agents
        .iter()
        .filter(|agent| agent.alive && agent.group_id == group)
        .map(|agent| agent.agent_id)
        .min()
        .expect("largest settlement has a live agent");
    (agent, group)
}

fn branch_point_snapshot(
    world: &sim_model::WorldState,
    config: &SimConfig,
    run_context: &M0RunContext,
) -> Result<sim_model::CanonicalSnapshot, Box<dyn Error>> {
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        run_context.master_seed,
        run_context.replicate_id,
        sim_model::runner::DEFAULT_MODEL_VERSION,
        sim_model::runner::DEFAULT_CONFIG_VERSION,
    );
    let reference = M3pReferenceState::new(world.clone(), config, run_context)?;
    let base = encode_m3p_snapshot(&reference, &metadata)?;
    if !config.migration_enabled() {
        return Ok(base);
    }
    let migration_state = MigrationState::new(world.agents.iter().map(|agent| agent.agent_id))?;
    let wrapped = encode_migration_snapshot(
        MigrationProfileTag::M3pm,
        &base,
        &migration_state,
        world.agents.iter().map(|agent| agent.agent_id),
        reference.relationship_graph(),
    )?;
    Ok(wrapped)
}

fn run_case(seed: u64, migration: bool) -> Result<serde_json::Value, Box<dyn Error>> {
    let config = config_for(seed, migration)?;
    let run_context = M0RunContext::new(seed, REPLICATE);
    let world = initialize_world(&config)?;
    let (target_agent, target_group) = target_agent(&world);
    let snapshot = branch_point_snapshot(&world, &config, &run_context)?;
    let result = run_counterfactual_branch_with_divergence_series(&BranchRequest {
        snapshot,
        config: config.clone(),
        run_context,
        intervention: BranchIntervention::AgentFieldOverride {
            agent_id: target_agent,
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        horizon_days: HORIZON,
        execution: BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    })?;
    let mut touched = BTreeSet::new();
    let mut first_other = None;
    let days = result
        .daily_series
        .iter()
        .zip(
            result
                .branch
                .base_days
                .iter()
                .zip(&result.branch.branch_days),
        )
        .map(|(difference, (base, branch))| {
            touched.extend(
                difference
                    .divergence_by_settlement
                    .iter()
                    .map(|(group, _)| *group),
            );
            if first_other.is_none()
                && difference
                    .divergence_by_settlement
                    .iter()
                    .any(|(group, _)| *group != target_group.0)
            {
                first_other = Some(difference.day + 1);
            }
            let base_moves = base
                .migration_metrics
                .map_or(0, |metrics| metrics.migrations);
            let branch_moves = branch
                .migration_metrics
                .map_or(0, |metrics| metrics.migrations);
            DailyRecord {
                day: difference.day + 1,
                base_moves,
                branch_moves,
                changed_agent_records: difference.changed_agent_records,
                matched_records: difference.matched_entities_with_record,
                divergence_fraction: difference.normalized_divergence,
                daily_settlements_touched: difference.settlements_touched,
                cumulative_settlements_touched: touched.len() as u64,
                first_other_settlement_day: first_other,
                divergence_by_settlement: difference.divergence_by_settlement.clone(),
            }
        })
        .collect::<Vec<_>>();
    let final_day = days.last().expect("positive horizon");
    Ok(json!({
        "seed": seed,
        "migration_enabled": migration,
        "target_agent_id": target_agent.0,
        "target_settlement_id": target_group.0,
        "classification": format!("{:?}", result.summary.classification),
        "first_divergent_day": result.summary.first_divergent_day.map(|day| day + 1),
        "cascade_onset_day": result.summary.cascade_onset_day.map(|day| day + 1),
        "first_other_settlement_day": first_other,
        "settlements_touched_day_365": final_day.cumulative_settlements_touched,
        "day_365_divergence_fraction": final_day.divergence_fraction,
        "days": days
    }))
}

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("docs/data/m4_06/branch_experiment.json"));
    let migration_off = SEEDS
        .iter()
        .map(|&seed| run_case(seed, false))
        .collect::<Result<Vec<_>, _>>()?;
    let migration_on = SEEDS
        .iter()
        .map(|&seed| run_case(seed, true))
        .collect::<Result<Vec<_>, _>>()?;
    let output_value = json!({
        "horizon_days": HORIZON,
        "replicate_id": REPLICATE,
        "preset": "M3P sustained, propagation disabled",
        "population": 10_000,
        "settlements": 50,
        "degree_k": 8,
        "p_local": 0.5,
        "migration": MIGRATION,
        "intervention": "set food=0 for lowest live AgentId in largest initial settlement",
        "seed_set": SEEDS,
        "migration_off": migration_off,
        "migration_on": migration_on
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&output_value)?)?;
    println!("wrote {}", output.display());
    Ok(())
}
