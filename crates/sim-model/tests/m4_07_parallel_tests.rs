use rayon::ThreadPoolBuilder;
use sim_core::{AgentId, GroupId};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchDayScanDigestMode, BranchExecutionOption,
    BranchIntervention, BranchInterventionBatchRequest, BranchRequest, IdentifiedIntervention,
    run_counterfactual_branch_with_direct_day_scan,
    run_counterfactual_branch_with_divergence_series,
};
use sim_model::crn_efficiency::{
    CrnEfficiencyRequest, run_crn_efficiency, run_crn_efficiency_parallel,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::minimal_cause::{
    EffectComparison, MinimalCausePredicate, MinimalCauseRequest, search_minimal_cause,
    search_minimal_cause_parallel,
};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{M0RunContext, SimConfig, SimulationProfile, initialize_world};
use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

fn small_m3p() -> (SimConfig, M0RunContext, sim_model::CanonicalSnapshot) {
    let mut config =
        SimConfig::parse_and_validate(include_str!("fixtures/m3p_propagation_sustained.toml"))
            .unwrap();
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = 60;
    config.world.settlement_count = 3;
    config.world.master_seed = 0x7799;
    config.world.replicate_id = 7;
    config.validate().unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        run_context.master_seed,
        run_context.replicate_id,
        sim_model::runner::DEFAULT_MODEL_VERSION,
        sim_model::runner::DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &run_context).unwrap();
    (
        config,
        run_context,
        encode_m3p_snapshot(&state, &metadata).unwrap(),
    )
}

fn request(
    config: &SimConfig,
    run_context: M0RunContext,
    snapshot: &sim_model::CanonicalSnapshot,
) -> BranchRequest {
    BranchRequest {
        snapshot: snapshot.clone(),
        config: config.clone(),
        run_context,
        intervention: BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        horizon_days: 7,
        execution: BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    }
}

fn temp_log(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "simulaciv-m4-07-{label}-{}.jsonl",
        std::process::id()
    ))
}

#[test]
fn direct_column_day_scan_matches_sha_scan_and_hashes_only_as_requested() {
    let (config, run_context, snapshot) = small_m3p();
    let request = request(&config, run_context, &snapshot);
    let sha = run_counterfactual_branch_with_divergence_series(&request).unwrap();
    let first = run_counterfactual_branch_with_direct_day_scan(
        &request,
        BranchDayScanDigestMode::FirstDivergentDay,
    )
    .unwrap();
    assert_eq!(
        first.first_divergence, sha.branch.first_divergence,
        "first phase/day must match SHA scan"
    );
    assert_eq!(
        first
            .daily_records
            .iter()
            .map(|record| &record.divergence)
            .collect::<Vec<_>>(),
        sha.daily_series.iter().collect::<Vec<_>>()
    );
    let digest_records = first
        .daily_records
        .iter()
        .filter(|record| record.base_state_digest.is_some())
        .collect::<Vec<_>>();
    assert_eq!(digest_records.len(), 1);
    assert!(digest_records[0].digests_at_intervention_boundary);
    assert!(digest_records[0].base_state_digest.is_some());
    assert!(digest_records[0].branch_state_digest.is_some());

    let every =
        run_counterfactual_branch_with_direct_day_scan(&request, BranchDayScanDigestMode::EveryDay)
            .unwrap();
    assert_eq!(every.daily_records.len(), sha.branch.base_days.len());
    for (index, record) in every.daily_records.iter().enumerate() {
        assert_eq!(
            record.base_state_digest,
            Some(sha.branch.base_days[index].state_digest)
        );
        assert_eq!(
            record.branch_state_digest,
            Some(sha.branch.branch_days[index].state_digest)
        );
    }
}

#[test]
fn parallel_crn_replicates_match_serial_estimates_exactly() {
    let (config, run_context, snapshot) = small_m3p();
    assert_eq!(run_context.replicate_id, 7);
    let interventions = [
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Health,
            value: AgentFieldValue::Health(0.01),
        },
        BranchIntervention::ConfigOverride {
            parameter: sim_model::ConfigParameter::BaseMetabolicCost,
            value: sim_model::ConfigValue::F32(1.01),
        },
    ];
    for intervention in interventions {
        let request = CrnEfficiencyRequest {
            config: config.clone(),
            branch_point: snapshot.clone(),
            intervention,
            target_group_id: GroupId(0),
            horizons: vec![7, 30, 90, 365],
            replicate_count: 2,
        };
        let serial = run_crn_efficiency(&request).unwrap();
        for (threads, chunk) in [(1, 1), (2, 64), (4, 256), (6, 1024)] {
            let pool = ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let parallel =
                run_crn_efficiency_parallel(&request, &pool, NonZeroUsize::new(chunk).unwrap())
                    .unwrap();
            assert_eq!(parallel, serial, "{threads} threads, chunk {chunk}");
        }
    }
}

#[test]
fn parallel_minimal_cause_matches_serial_search_and_log() {
    let (config, run_context, snapshot) = small_m3p();
    let world = initialize_world(&config).unwrap();
    let candidates: Vec<IdentifiedIntervention> = [0, 1]
        .into_iter()
        .map(|agent_id| {
            IdentifiedIntervention::agent_field_override(
                AgentId(agent_id),
                AgentField::Health,
                AgentFieldValue::Health(0.01),
            )
        })
        .collect();
    let request_for = |evaluation_log_path| MinimalCauseRequest {
        batch: BranchInterventionBatchRequest {
            snapshot: snapshot.clone(),
            config: config.clone(),
            run_context,
            interventions: Vec::new(),
            horizon_days: 5,
            snapshot_days: Vec::new(),
        },
        candidates: candidates.clone(),
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::AlivePopulation,
            target_group_id: None,
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: (1..=10).collect(),
        exploratory_single_replicate: false,
        max_branch_evaluations: 200,
        max_wall_time: Duration::from_secs(120),
        evaluation_log_path,
    };
    assert!(!world.agents.is_empty());
    let serial_log = temp_log("serial");
    let serial = search_minimal_cause(&request_for(serial_log.clone())).unwrap();
    let serial_contents = fs::read_to_string(&serial_log).unwrap();
    let normalize = |contents: String| {
        contents
            .lines()
            .map(|line| {
                let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
                if record["kind"] == "result" {
                    record["result"]["evaluation_log_path"] =
                        serde_json::Value::String("<evaluation-log>".into());
                }
                record
            })
            .collect::<Vec<_>>()
    };
    for (threads, chunk) in [(1, 1), (2, 64), (4, 256), (6, 1024)] {
        let parallel_log = temp_log(&format!("parallel-{threads}-{chunk}"));
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let parallel = search_minimal_cause_parallel(
            &request_for(parallel_log.clone()),
            &pool,
            NonZeroUsize::new(chunk).unwrap(),
        )
        .unwrap();
        assert_eq!(parallel.status, serial.status);
        assert_eq!(parallel.cause_ids, serial.cause_ids);
        assert_eq!(parallel.guarantee, serial.guarantee);
        assert_eq!(parallel.evaluation_count, serial.evaluation_count);
        assert_eq!(parallel.mean_effect, serial.mean_effect);
        assert_eq!(parallel.predicate_value, serial.predicate_value);
        let parallel_contents = fs::read_to_string(&parallel_log).unwrap();
        assert_eq!(
            normalize(serial_contents.clone()),
            normalize(parallel_contents)
        );
        let _ = fs::remove_file(parallel_log);
    }
    let _ = fs::remove_file(serial_log);
}

#[test]
fn every_day_direct_scan_digest_sequence_matches_serial_branch_records() {
    let (config, run_context, snapshot) = small_m3p();
    let request = request(&config, run_context, &snapshot);
    let branch = run_counterfactual_branch_with_divergence_series(&request).unwrap();
    let scan =
        run_counterfactual_branch_with_direct_day_scan(&request, BranchDayScanDigestMode::EveryDay)
            .unwrap();
    for (record, (base, intervention)) in scan.daily_records.iter().zip(
        branch
            .branch
            .base_days
            .iter()
            .zip(&branch.branch.branch_days),
    ) {
        assert_eq!(record.base_state_digest, Some(base.state_digest));
        assert_eq!(record.branch_state_digest, Some(intervention.state_digest));
    }
}
