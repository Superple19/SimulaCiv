// re-run in M5-04: deterministic differences recorded under docs/data/m5_04/m4_05/
use serde_json::Value;
use sim_core::GroupId;
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchInterventionBatchRequest, ConfigParameter, ConfigValue,
    CrnOutcome, EffectComparison, IdentifiedIntervention, MinimalCausePredicate,
    MinimalCauseRequest, MinimalCauseResult, SimConfig, SimulationProfile,
    build_agent_field_candidates, build_config_candidates, evaluate_subset, search_minimal_cause,
};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MASTER_SEED: u64 = 81_985_529_216_486_895;
const REPLICATE_ID: u32 = 7;
const HORIZON: u32 = 90;
const REPLICATES: usize = 10;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let enumerate_size4 = args.iter().any(|arg| arg == "--enumerate-size4");
    let enumeration_output = args
        .iter()
        .position(|arg| arg == "--output")
        .and_then(|index| args.get(index + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/m4_05_case_a_size4.jsonl"));
    let mut config = SimConfig::parse_and_validate(include_str!(
        "../tests/fixtures/m3p_propagation_sustained.toml"
    ))?;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = 2_000;
    config.world.settlement_count = 3;
    config.world.master_seed = MASTER_SEED;
    config.world.replicate_id = REPLICATE_ID;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    config.validate()?;
    let context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let world = sim_model::initialize_world(&config)?;
    let mut group_counts = BTreeMap::<GroupId, usize>::new();
    for agent in world.agents.iter().filter(|agent| agent.alive) {
        *group_counts.entry(agent.group_id).or_default() += 1;
    }
    let max_count = group_counts
        .values()
        .copied()
        .max()
        .ok_or("empty population")?;
    let target_group = group_counts
        .iter()
        .find(|(_, count)| **count == max_count)
        .map(|(group, _)| *group)
        .ok_or("no target settlement")?;
    let mut target_agents = world
        .agents
        .iter()
        .filter(|agent| agent.alive && agent.group_id == target_group)
        .map(|agent| agent.agent_id)
        .collect::<Vec<_>>();
    target_agents.sort_unstable();
    if target_agents.len() < 16 {
        return Err("target settlement has fewer than 16 live agents".into());
    }

    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        MASTER_SEED,
        REPLICATE_ID,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world.clone(), &config, &context)?;
    let snapshot = encode_m3p_snapshot(&state, &metadata)?;
    let batch_template = BranchInterventionBatchRequest {
        snapshot,
        config: config.clone(),
        run_context: context,
        interventions: Vec::new(),
        horizon_days: HORIZON,
        snapshot_days: Vec::new(),
    };

    let health8 = build_agent_field_candidates(
        &world,
        &target_agents[..8],
        Some(target_group),
        AgentField::Health,
        &[AgentFieldValue::Health(0.01)],
    )?;
    let health16 = build_agent_field_candidates(
        &world,
        &target_agents[..16],
        Some(target_group),
        AgentField::Health,
        &[AgentFieldValue::Health(0.01)],
    )?;
    let parameters = [
        (
            ConfigParameter::BaseMetabolicCost,
            ConfigValue::F32(config.environment.base_metabolic_cost * 1.01),
        ),
        (
            ConfigParameter::HealthDecayRate,
            ConfigValue::F32(config.environment.health_decay_rate * 1.01),
        ),
        (
            ConfigParameter::BaseWorkYield,
            ConfigValue::F32(config.economy.base_work_yield * 1.01),
        ),
        (
            ConfigParameter::RegrowthRate,
            ConfigValue::F32(config.environment.regrowth_rate * 1.01),
        ),
        (
            ConfigParameter::CarryingCapacity,
            ConfigValue::F32(config.environment.carrying_capacity * 1.01),
        ),
        (
            ConfigParameter::BirthProbability,
            ConfigValue::F32(config.population.as_ref().unwrap().birth_probability * 1.01),
        ),
    ];
    let config6 = build_config_candidates(&parameters)?;

    let target_predicate = MinimalCausePredicate {
        outcome: CrnOutcome::TargetSettlementAlivePopulation,
        target_group_id: Some(target_group),
        comparison: EffectComparison::LessThanOrEqual,
        threshold: -1.0,
    };
    let world_predicate = MinimalCausePredicate {
        outcome: CrnOutcome::AlivePopulation,
        target_group_id: None,
        comparison: EffectComparison::LessThanOrEqual,
        threshold: -5.0,
    };

    if enumerate_size4 {
        let request = MinimalCauseRequest {
            batch: batch_template.clone(),
            candidates: health8,
            predicate: target_predicate,
            replicate_ids: (1..=REPLICATES as u32).collect(),
            exploratory_single_replicate: false,
            max_branch_evaluations: 100_000,
            max_wall_time: Duration::from_secs(21_600),
            evaluation_log_path: PathBuf::from("target/m4_05_enumeration_unused.jsonl"),
        };
        return enumerate_size_four(&request, &enumeration_output);
    }

    let case_a = run_case(
        "a_health8",
        &batch_template,
        health8,
        target_predicate.clone(),
        100_000,
        21_600,
    )?;
    let case_b = run_case(
        "b_health16",
        &batch_template,
        health16.clone(),
        target_predicate,
        100_000,
        21_600,
    )?;
    let _case_c = run_case(
        "c_config6",
        &batch_template,
        config6,
        world_predicate,
        100_000,
        21_600,
    )?;

    let overlap = case_a
        .result
        .cause_ids
        .iter()
        .filter(|id| case_b.result.cause_ids.contains(id))
        .cloned()
        .collect::<Vec<_>>();
    println!("OVERLAP|a_b={}", overlap.join(","));
    Ok(())
}

fn enumerate_size_four(
    request: &MinimalCauseRequest,
    output: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(request.candidates.len(), 8, "case (a) has eight candidates");
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(fs::File::create(output)?);
    writeln!(
        writer,
        "{{\"record\":\"header\",\"case\":\"a\",\"candidate_count\":8,\"subset_size\":4,\"replicate_ids\":[1,2,3,4,5,6,7,8,9,10]}}"
    )?;
    let ids = request
        .candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    let mut evaluated = 0_usize;
    let mut satisfying = 0_usize;
    for a in 0..ids.len() - 3 {
        for b in a + 1..ids.len() - 2 {
            for c in b + 1..ids.len() - 1 {
                for d in c + 1..ids.len() {
                    let subset = [&ids[a], &ids[b], &ids[c], &ids[d]]
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    let result = evaluate_subset(request, &subset)?;
                    evaluated += 1;
                    satisfying += usize::from(result.predicate_value);
                    writeln!(writer, "{}", serde_json::to_string(&result)?)?;
                    writer.flush()?;
                }
            }
        }
    }
    assert_eq!(evaluated, 70, "C(8, 4) subsets are evaluated");
    assert_eq!(satisfying, 15, "report addendum records fifteen minima");
    println!("ENUMERATION|case=a|subset_size=4|evaluated={evaluated}|satisfying={satisfying}");
    Ok(())
}

struct CaseOutput {
    result: MinimalCauseResult,
}

type EvaluationMap = BTreeMap<Vec<String>, (Option<f64>, bool)>;

fn run_case(
    label: &str,
    template: &BranchInterventionBatchRequest,
    candidates: Vec<IdentifiedIntervention>,
    predicate: MinimalCausePredicate,
    max_evaluations: u64,
    max_wall_time_seconds: u64,
) -> Result<CaseOutput, Box<dyn std::error::Error>> {
    let log = PathBuf::from(format!("target/m4_05_{label}.jsonl"));
    let _ = fs::remove_file(&log);
    let request = MinimalCauseRequest {
        batch: template.clone(),
        candidates: candidates.clone(),
        predicate,
        replicate_ids: (1..=REPLICATES as u32).collect(),
        exploratory_single_replicate: false,
        max_branch_evaluations: max_evaluations,
        max_wall_time: Duration::from_secs(max_wall_time_seconds),
        evaluation_log_path: log.clone(),
    };
    let start = Instant::now();
    let result = search_minimal_cause(&request)?;
    let search_wall = start.elapsed();
    let evals = read_eval(&log)?;
    let full_ids = sorted_ids(&candidates);
    let full_effect = evals.get(&full_ids).and_then(|entry| entry.0);
    let (found_effect, one_remove) = if result.predicate_value {
        let found_effect = evals
            .get(&result.cause_ids)
            .and_then(|entry| entry.0)
            .or(result.mean_effect);
        let mut all_break = true;
        let mut removals = Vec::new();
        for id in &result.cause_ids {
            let mut subset = result
                .cause_ids
                .iter()
                .filter(|candidate| *candidate != id)
                .cloned()
                .collect::<Vec<_>>();
            subset.sort();
            let passes = evals.get(&subset).is_some_and(|entry| entry.1);
            all_break &= !passes;
            removals.push(format!(
                "{}:{}",
                id,
                if passes { "still_true" } else { "breaks" }
            ));
        }
        (
            found_effect,
            if all_break {
                format!("yes [{}]", removals.join(";"))
            } else {
                format!("no [{}]", removals.join(";"))
            },
        )
    } else {
        (None, "n/a (no satisfying cause)".to_owned())
    };
    println!(
        "CASE|name={label}|guarantee={:?}|status={:?}|evals={}|search_wall_s={:.3}|cause={}|found_effect={}|full_effect={}|single_removals={}",
        result.guarantee,
        result.status,
        result.evaluation_count,
        search_wall.as_secs_f64(),
        result.cause_ids.join(","),
        found_effect.map_or_else(|| "none".to_owned(), |value| format!("{value:.9}")),
        full_effect.map_or_else(|| "not_evaluated".to_owned(), |value| format!("{value:.9}")),
        one_remove,
    );

    if full_effect.is_none() {
        let probe_candidates = full_probe_candidates(&candidates, &request.batch.config)?;
        let probe_log = PathBuf::from(format!("target/m4_05_{label}_full_probe.jsonl"));
        let _ = fs::remove_file(&probe_log);
        let probe_request = MinimalCauseRequest {
            batch: template.clone(),
            candidates: probe_candidates.clone(),
            predicate: request.predicate.clone(),
            replicate_ids: request.replicate_ids.clone(),
            exploratory_single_replicate: false,
            max_branch_evaluations: (2 * REPLICATES) as u64,
            max_wall_time: Duration::from_secs(max_wall_time_seconds),
            evaluation_log_path: probe_log.clone(),
        };
        let probe_start = Instant::now();
        let probe_result = search_minimal_cause(&probe_request)?;
        let probe_wall = probe_start.elapsed();
        let probe_evals = read_eval(&probe_log)?;
        let probe_effect = probe_evals
            .get(&sorted_ids(&probe_candidates))
            .and_then(|entry| entry.0)
            .or(probe_result.mean_effect);
        println!(
            "FULL_PROBE|name={label}|status={:?}|evals={}|wall_s={:.3}|effect={} | added_noop_candidates={}",
            probe_result.status,
            probe_result.evaluation_count,
            probe_wall.as_secs_f64(),
            probe_effect.map_or_else(|| "none".to_owned(), |value| format!("{value:.9}")),
            probe_candidates.len() - candidates.len()
        );
    }
    Ok(CaseOutput { result })
}

fn full_probe_candidates(
    candidates: &[IdentifiedIntervention],
    config: &SimConfig,
) -> Result<Vec<IdentifiedIntervention>, Box<dyn std::error::Error>> {
    let mut probe = candidates.to_vec();
    let noops = [
        (
            ConfigParameter::FoodPrice,
            ConfigValue::Money(config.economy.food_price),
        ),
        (
            ConfigParameter::TaxRate,
            ConfigValue::F32(config.economy.tax_rate),
        ),
        (
            ConfigParameter::DecisionTemperature,
            ConfigValue::F32(config.decision.decision_temperature),
        ),
        (
            ConfigParameter::TraitWeightCooperation,
            ConfigValue::F32(config.decision.trait_weight_cooperation),
        ),
        (
            ConfigParameter::WelfarePayment,
            ConfigValue::Money(config.economy.welfare_payment),
        ),
        (
            ConfigParameter::TargetFood,
            ConfigValue::F32(config.economy.target_food),
        ),
        (
            ConfigParameter::TargetReserve,
            ConfigValue::Money(config.economy.target_reserve),
        ),
        (
            ConfigParameter::TraitWeightAggression,
            ConfigValue::F32(config.decision.trait_weight_aggression),
        ),
    ];
    for (parameter, value) in noops {
        if probe.len() > 10 {
            break;
        }
        if !probe.iter().any(|candidate| matches!(candidate.intervention, sim_model::BranchIntervention::ConfigOverride { parameter: existing, .. } if existing == parameter)) {
            probe.push(IdentifiedIntervention::config_override(parameter, value));
        }
    }
    probe.sort_by(|left, right| left.id.cmp(&right.id));
    if probe.len() <= 10 {
        return Err("full-set effect probe failed to select the ddmin path".into());
    }
    Ok(probe)
}

fn sorted_ids(candidates: &[IdentifiedIntervention]) -> Vec<String> {
    let mut ids = candidates
        .iter()
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn read_eval(path: &Path) -> Result<EvaluationMap, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let record: Value = serde_json::from_str(line)?;
        if record.get("kind").and_then(Value::as_str) != Some("evaluation") {
            continue;
        }
        let ids = record
            .get("subset_ids")
            .and_then(Value::as_array)
            .ok_or("missing subset_ids")?
            .iter()
            .map(|id| id.as_str().map(str::to_owned).ok_or("invalid subset id"))
            .collect::<Result<Vec<_>, _>>()?;
        let mean = record.get("mean_effect").and_then(Value::as_f64);
        let pred = record
            .get("predicate_value")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        values.insert(ids, (mean, pred));
    }
    Ok(values)
}
