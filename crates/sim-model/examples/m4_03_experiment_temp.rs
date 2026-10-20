// restored from session history; not re-run
use sim_core::AgentId;
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention, BranchRequest,
    ConfigParameter, ConfigValue, run_counterfactual_branch,
    run_counterfactual_branch_with_divergence_series,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{SimulationProfile, SimConfig, initialize_world};
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufWriter, Write};

fn request(seed: u64, horizon: u32, intervention_kind: &str) -> (BranchRequest, AgentId, u16) {
    let mut config = SimConfig::parse_and_validate(include_str!(
        "../tests/fixtures/m3p_propagation_sustained.toml"
    ))
    .unwrap();
    config.world.initial_population = 10_000;
    config.world.settlement_count = 50;
    config.world.master_seed = seed;
    config.world.replicate_id = 7;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.validate().unwrap();

    let context = M0RunContext::new(seed, 7);
    let world = initialize_world(&config).unwrap();
    let mut groups: BTreeMap<u16, Vec<AgentId>> = BTreeMap::new();
    for agent in &world.agents {
        if agent.alive {
            groups.entry(agent.group_id.0).or_default().push(agent.agent_id);
        }
    }
    let (target_group, candidates) = groups
        .iter()
        .min_by_key(|(group, agents)| (Reverse(agents.len()), **group))
        .unwrap();
    let target = *candidates.iter().min().unwrap();
    let target_group = *target_group;
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        seed,
        7,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &context).unwrap();
    let snapshot = encode_m3p_snapshot(&state, &metadata).unwrap();
    let intervention = match intervention_kind {
        "food0" => BranchIntervention::AgentFieldOverride {
            agent_id: target,
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        "health001" => BranchIntervention::AgentFieldOverride {
            agent_id: target,
            field: AgentField::Health,
            value: AgentFieldValue::Health(0.01),
        },
        "idle" => BranchIntervention::ForcedAction {
            agent_id: target,
            action: sim_model::Action::Idle,
        },
        "cost1pct" => BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(config.environment.base_metabolic_cost * 1.01),
        },
        _ => unreachable!(),
    };
    (
        BranchRequest {
            config,
            run_context: context,
            snapshot,
            intervention,
            horizon_days: horizon,
            execution: BranchExecutionOption::Serial,
            snapshot_days: vec![],
        },
        target,
        target_group,
    )
}

fn main() {
    let file = File::create("target/m4_03_a1_experiments.csv").unwrap();
    let mut out = BufWriter::new(file);
    writeln!(out, "RUN,kind,seed,horizon,target_id,target_group,first_div_day,first_slot,first_components,classification,onset,plateau,growth,doubling,peak_D,peak_fraction,days_to_peak,base_death,branch_death,death_relation,newborn_crn_loss,forced_matches_natural,final_base_alive,final_branch_alive,final_D,final_fraction").unwrap();
    writeln!(out, "DAY,kind,seed,horizon,day,D,L,normalized,target_settlement_D,other_settlements_D,settlements_touched,D_by_settlement,base_alive,branch_alive,graph,allocator,propagation,settlement_global").unwrap();

    let seeds = [0, 1, 3, 7, 81_985_529_216_486_895];
    let cases = ["food0", "health001", "idle", "cost1pct"];
    for (kind, horizon, selected_cases) in [
        ("H90", 90, cases.as_slice()),
        ("H365", 365, ["food0", "health001"].as_slice()),
    ] {
        for case in selected_cases {
            for seed in seeds {
                let (request, target, target_group) = request(seed, horizon, case);
                let expected = run_counterfactual_branch(&request).unwrap();
                let measured = run_counterfactual_branch_with_divergence_series(&request).unwrap();
                assert_eq!(measured.branch, expected, "M4-02 output differs: {kind}/{case}/{seed}");
                let first = measured.branch.first_divergence.as_ref();
                let last = measured.daily_series.last().unwrap();
                let summary = &measured.summary;
                writeln!(
                    out,
                    "RUN,{kind}/{case},{seed},{horizon},{},{},{:?},{:?},{:?},{:?},{:?},{:?},{:?},{:?},{},{:.12},{:?},{:?},{:?},{:?},{},{:?},{},{},{},{:.12}",
                    target.0,
                    target_group,
                    first.map(|value| value.day),
                    first.map(|value| format!("{:?}", value.slot)),
                    first.map(|value| value.differing_components.join("+")),
                    summary.classification,
                    summary.cascade_onset_day,
                    summary.plateau_days,
                    summary.growth_slope,
                    summary.doubling_time_days,
                    summary.peak_changed_agent_records,
                    summary.peak_normalized_fraction,
                    summary.days_to_peak,
                    summary.base_target_death_day,
                    summary.branch_target_death_day,
                    summary.onset_vs_branch_target_death,
                    summary.newborn_crn_loss,
                    summary.forced_matches_natural,
                    last.base_alive_population,
                    last.branch_alive_population,
                    last.changed_agent_records,
                    last.normalized_divergence,
                )
                .unwrap();
                for day in &measured.daily_series {
                    let target_divergence = day
                        .divergence_by_settlement
                        .iter()
                        .filter(|(group, _)| *group == target_group)
                        .map(|(_, count)| count)
                        .sum::<u64>();
                    let other_divergence = day
                        .divergence_by_settlement
                        .iter()
                        .filter(|(group, _)| *group != target_group)
                        .map(|(_, count)| count)
                        .sum::<u64>();
                    let by_settlement = day
                        .divergence_by_settlement
                        .iter()
                        .map(|(group, count)| format!("{group}:{count}"))
                        .collect::<Vec<_>>()
                        .join(";");
                    writeln!(
                        out,
                        "DAY,{kind}/{case},{seed},{horizon},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                        day.day,
                        day.changed_agent_records,
                        day.matched_entities_with_record,
                        day.normalized_divergence,
                        target_divergence,
                        other_divergence,
                        day.settlements_touched,
                        by_settlement,
                        day.base_alive_population,
                        day.branch_alive_population,
                        day.graph_changed,
                        day.allocator_changed,
                        day.propagation_changed,
                        day.settlement_global_changed,
                    )
                    .unwrap();
                }
                out.flush().unwrap();
                println!("DONE,{kind}/{case},{seed},class={:?},Dpeak={},fraction={:.6},day={:?},forced_matches={:?}", summary.classification, summary.peak_changed_agent_records, summary.peak_normalized_fraction, summary.days_to_peak, summary.forced_matches_natural);
            }
        }
    }
}
