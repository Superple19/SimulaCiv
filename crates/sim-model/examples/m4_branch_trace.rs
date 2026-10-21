use serde_json::{Value, json};
use sim_core::AgentId;
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention, BranchMetricsRecord,
    BranchRequest, run_counterfactual_branch, run_counterfactual_branch_with_divergence_series,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{M0RunContext, SimConfig, SimulationProfile, initialize_world};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

const FIXTURE: &str = include_str!("../tests/fixtures/m3p_propagation_sustained.toml");
const POPULATION: u64 = 10_000;
const SETTLEMENTS: u32 = 50;
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut case = "food_0".to_owned();
    let mut horizon = 90_u32;
    let mut output = PathBuf::from("docs/data/m4_viewer/food_0_h90.jsonl");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--case" => case = args.next().ok_or("--case requires food_0 or health_0.01")?,
            "--horizon" => horizon = args.next().ok_or("--horizon requires 90 or 365")?.parse()?,
            "--output" => output = PathBuf::from(args.next().ok_or("--output requires a path")?),
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    if !matches!(case.as_str(), "food_0" | "health_0.01") || !matches!(horizon, 90 | 365) {
        return Err("supported cases are food_0/health_0.01 and horizons are 90/365".into());
    }

    let mut config = SimConfig::parse_and_validate(FIXTURE)?;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = POPULATION;
    config.world.settlement_count = SETTLEMENTS;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.validate()?;
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let world = initialize_world(&config)?;
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        SEED,
        REPLICATE,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &run_context)?;
    let snapshot = encode_m3p_snapshot(&state, &metadata)?;
    let field = match case.as_str() {
        "food_0" => AgentField::Food,
        "health_0.01" => AgentField::Health,
        _ => unreachable!(),
    };
    let value = match field {
        AgentField::Food => AgentFieldValue::Food(0.0),
        AgentField::Health => AgentFieldValue::Health(0.01),
        AgentField::Wealth => unreachable!(),
    };
    let request = BranchRequest {
        snapshot,
        config,
        run_context,
        intervention: BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field,
            value,
        },
        horizon_days: horizon,
        execution: BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    };
    let result = run_counterfactual_branch_with_divergence_series(&request)?;
    assert_eq!(
        result.branch,
        run_counterfactual_branch(&request)?,
        "writing a trace must not change the M4-02 branch result"
    );

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(output)?);
    let first = result.branch.first_divergence.as_ref().map(|first| {
        json!({
            "day": first.day,
            "slot": format!("{:?}", first.slot),
            "agent_id": first.first_differing_agent.map(|id| id.0),
            "components": first.differing_components,
            "system_id": first.system_id.map(|id| format!("{id:?}")),
            "rng_subsystem_ids": first.rng_subsystem_ids,
        })
    });
    serde_json::to_writer(
        &mut writer,
        &json!({
            "kind": "header",
            "schema": "simulaciv-m4-branch-trace-v1",
            "preset": "m3p_sustained",
            "case": case,
            "intervention": format!("{case} on AgentId(0)"),
            "population": POPULATION,
            "settlements": SETTLEMENTS,
            "k": 8,
            "p_local": 0.5,
            "seed": SEED,
            "replicate": REPLICATE,
            "runner": "M4-03 serial branch API; M4-02 exact-oracle assertion",
            "horizon_days": horizon,
            "first_divergence": first,
            "summary": {
                "classification": format!("{:?}", result.summary.classification),
                "first_divergent_day": result.summary.first_divergent_day,
                "cascade_onset_day": result.summary.cascade_onset_day,
                "peak_changed_agent_records": result.summary.peak_changed_agent_records,
                "peak_normalized_fraction": result.summary.peak_normalized_fraction,
                "days_to_peak": result.summary.days_to_peak,
                "newborn_crn_loss": result.summary.newborn_crn_loss,
            }
        }),
    )?;
    writeln!(writer)?;

    for (index, divergence) in result.daily_series.iter().enumerate() {
        let base = &result.branch.base_days[index];
        let branch = &result.branch.branch_days[index];
        let metrics = |record: &BranchMetricsRecord| -> Result<Value, serde_json::Error> {
            match record {
                BranchMetricsRecord::M3p(metrics) => serde_json::to_value(metrics),
                _ => unreachable!("the preset is M3P without propagation"),
            }
        };
        let settlement_counts: Vec<_> = divergence
            .divergence_by_settlement
            .iter()
            .map(|(group, count)| json!({"settlement": group, "changed_agents": count}))
            .collect();
        serde_json::to_writer(
            &mut writer,
            &json!({
                "kind": "day",
                "day": divergence.day,
                "base_metrics": metrics(&base.metrics)?,
                "branch_metrics": metrics(&branch.metrics)?,
                "base_state_digest": base.state_digest.to_hex(),
                "branch_state_digest": branch.state_digest.to_hex(),
                "divergence": {
                    "changed_agent_records": divergence.changed_agent_records,
                    "matched_entities_with_record": divergence.matched_entities_with_record,
                    "normalized_divergence": divergence.normalized_divergence,
                    "newborn_crn_loss": divergence.newborn_crn_loss,
                    "unmatched_birth_records": divergence.unmatched_birth_records,
                    "unmatched_birth_fraction": divergence.unmatched_birth_fraction,
                    "settlements_touched": divergence.settlements_touched,
                    "by_settlement": settlement_counts,
                    "state_digests_differ": divergence.state_digests_differ,
                    "graph_changed": divergence.graph_changed,
                    "allocator_changed": divergence.allocator_changed,
                    "propagation_changed": divergence.propagation_changed,
                    "settlement_global_changed": divergence.settlement_global_changed,
                }
            }),
        )?;
        writeln!(writer)?;
    }
    let last_base = result.branch.base_days.last().ok_or("empty base result")?;
    let last_branch = result
        .branch
        .branch_days
        .last()
        .ok_or("empty branch result")?;
    serde_json::to_writer(
        &mut writer,
        &json!({
            "kind": "final",
            "base_digests": {"state": last_base.state_digest.to_hex(), "metrics": last_base.metrics_digest.to_hex(), "events": last_base.events_digest.to_hex()},
            "branch_digests": {"state": last_branch.state_digest.to_hex(), "metrics": last_branch.metrics_digest.to_hex(), "events": last_branch.events_digest.to_hex()},
        }),
    )?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}
