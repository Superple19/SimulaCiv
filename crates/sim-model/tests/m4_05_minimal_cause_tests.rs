use sim_core::{AgentId, GroupId};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{
    AgentField, AgentFieldValue, BranchInterventionBatchRequest, BranchRequest, ConfigParameter,
    ConfigValue, EffectComparison, IdentifiedIntervention, MinimalCauseGuarantee,
    MinimalCausePredicate, MinimalCauseRequest, MinimalCauseStatus, SimConfig, SimulationConfig,
    SimulationProfile, SimulationSystems, build_agent_field_candidates, build_config_candidates,
    run_counterfactual_branch, run_counterfactual_branch_with_divergence_series,
    run_counterfactual_intervention_batch, search_minimal_cause,
};
use std::fs;
use std::time::Duration;

fn fixture(population: u64) -> (SimConfig, M0RunContext, sim_model::CanonicalSnapshot) {
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
    let world = sim_model::initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, &config, &context).unwrap();
    (
        config,
        context,
        encode_m3p_snapshot(&state, &metadata).unwrap(),
    )
}

fn batch_request(
    config: SimConfig,
    context: M0RunContext,
    snapshot: sim_model::CanonicalSnapshot,
    interventions: Vec<IdentifiedIntervention>,
    horizon_days: u32,
) -> BranchInterventionBatchRequest {
    BranchInterventionBatchRequest {
        snapshot,
        config,
        run_context: context,
        interventions,
        horizon_days,
        snapshot_days: Vec::new(),
    }
}

#[test]
fn batch_of_one_matches_m4_02_records_and_snapshot_bytes() {
    let (config, context, snapshot) = fixture(300);
    let intervention = IdentifiedIntervention::agent_field_override(
        AgentId(0),
        AgentField::Food,
        AgentFieldValue::Food(0.0),
    );
    let batch = batch_request(
        config.clone(),
        context,
        snapshot.clone(),
        vec![intervention.clone()],
        3,
    );
    let actual = run_counterfactual_intervention_batch(&batch).unwrap();
    let expected = run_counterfactual_branch(&BranchRequest {
        snapshot,
        config,
        run_context: batch.run_context,
        intervention: intervention.intervention,
        horizon_days: batch.horizon_days,
        execution: sim_model::BranchExecutionOption::Serial,
        snapshot_days: batch.snapshot_days,
    })
    .unwrap();
    assert_eq!(actual.base_days, expected.base_days);
    assert_eq!(actual.branch_days, expected.branch_days);
    assert!(actual.forced_action_outcomes.is_empty());
}

#[test]
fn m3_batch_of_one_matches_the_reference_branch_path() {
    let (mut config, context, _) = fixture(300);
    config.profile = SimulationProfile::M3;
    config.simulation = Some(SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: false,
            propagation: false,
        },
    });
    config.population = None;
    config.validate().unwrap();
    let world = sim_model::initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let snapshot = sim_model::snapshot::encode_snapshot(&world, &metadata).unwrap();
    let intervention = IdentifiedIntervention::agent_field_override(
        AgentId(0),
        AgentField::Food,
        AgentFieldValue::Food(0.0),
    );
    let batch = batch_request(
        config.clone(),
        context,
        snapshot.clone(),
        vec![intervention.clone()],
        2,
    );
    let actual = run_counterfactual_intervention_batch(&batch).unwrap();
    let expected = run_counterfactual_branch(&BranchRequest {
        snapshot,
        config,
        run_context: context,
        intervention: intervention.intervention,
        horizon_days: 2,
        execution: sim_model::BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    })
    .unwrap();
    assert_eq!(actual.base_days, expected.base_days);
    assert_eq!(actual.branch_days, expected.branch_days);
}

#[test]
fn batch_of_one_config_override_matches_m4_02() {
    let (config, context, snapshot) = fixture(300);
    let intervention = IdentifiedIntervention::config_override(
        ConfigParameter::BaseMetabolicCost,
        ConfigValue::F32(config.environment.base_metabolic_cost + 0.01),
    );
    let batch = batch_request(
        config.clone(),
        context,
        snapshot.clone(),
        vec![intervention.clone()],
        2,
    );
    let actual = run_counterfactual_intervention_batch(&batch).unwrap();
    let expected = run_counterfactual_branch(&BranchRequest {
        snapshot,
        config,
        run_context: context,
        intervention: intervention.intervention,
        horizon_days: 2,
        execution: sim_model::BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    })
    .unwrap();
    assert_eq!(actual.base_days, expected.base_days);
    assert_eq!(actual.branch_days, expected.branch_days);
}

#[test]
fn batch_of_one_forced_action_matches_m4_02_outcome() {
    let (config, context, snapshot) = fixture(300);
    let intervention =
        IdentifiedIntervention::forced_action(AgentId(0), 0, sim_model::Action::Idle);
    let batch = batch_request(
        config.clone(),
        context,
        snapshot.clone(),
        vec![intervention.clone()],
        2,
    );
    let actual = run_counterfactual_intervention_batch(&batch).unwrap();
    let expected = run_counterfactual_branch_with_divergence_series(&BranchRequest {
        snapshot,
        config,
        run_context: context,
        intervention: intervention.intervention,
        horizon_days: 2,
        execution: sim_model::BranchExecutionOption::Serial,
        snapshot_days: Vec::new(),
    })
    .unwrap();
    assert_eq!(actual.base_days, expected.branch.base_days);
    assert_eq!(actual.branch_days, expected.branch.branch_days);
    assert_eq!(
        actual.forced_action_outcomes[0].matches_natural,
        expected.summary.forced_matches_natural
    );
    assert_eq!(
        actual.forced_action_outcomes[0].outcome,
        expected.branch.forced_action
    );
}

#[test]
fn batch_rejects_duplicate_targets_before_running() {
    let (config, context, snapshot) = fixture(300);
    let batch = batch_request(
        config,
        context,
        snapshot,
        vec![
            IdentifiedIntervention::agent_field_override(
                AgentId(0),
                AgentField::Food,
                AgentFieldValue::Food(0.0),
            ),
            IdentifiedIntervention::agent_field_override(
                AgentId(0),
                AgentField::Food,
                AgentFieldValue::Food(1.0),
            ),
        ],
        1,
    );
    assert!(matches!(
        run_counterfactual_intervention_batch(&batch),
        Err(sim_model::BranchError::InvalidIntervention(_))
    ));
}

#[test]
fn batch_application_order_is_independent_of_caller_order() {
    let (config, context, snapshot) = fixture(300);
    let a = IdentifiedIntervention::agent_field_override(
        AgentId(2),
        AgentField::Food,
        AgentFieldValue::Food(0.0),
    );
    let b = IdentifiedIntervention::agent_field_override(
        AgentId(1),
        AgentField::Health,
        AgentFieldValue::Health(0.01),
    );
    let first = batch_request(
        config.clone(),
        context,
        snapshot.clone(),
        vec![a.clone(), b.clone()],
        3,
    );
    let second = batch_request(config, context, snapshot, vec![b, a], 3);
    assert_eq!(
        run_counterfactual_intervention_batch(&first).unwrap(),
        run_counterfactual_intervention_batch(&second).unwrap(),
    );
}

#[test]
fn candidate_builders_sort_ids_and_filter_settlement_scope() {
    let (config, _, _) = fixture(300);
    let world = sim_model::initialize_world(&config).unwrap();
    let candidates = build_agent_field_candidates(
        &world,
        &[AgentId(2), AgentId(0), AgentId(1)],
        Some(GroupId(0)),
        AgentField::Health,
        &[AgentFieldValue::Health(0.01)],
    )
    .unwrap();
    assert!(candidates.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(
        candidates
            .iter()
            .all(|candidate| candidate.id.starts_with("F:agent=0:"))
    );
    let configs = build_config_candidates(&[
        (ConfigParameter::BirthProbability, ConfigValue::F32(0.1)),
        (ConfigParameter::BaseMetabolicCost, ConfigValue::F32(0.2)),
    ])
    .unwrap();
    assert!(configs.windows(2).all(|pair| pair[0].id < pair[1].id));
}

#[test]
fn minimal_cause_incomplete_result_replays_from_jsonl_log() {
    let (config, context, snapshot) = fixture(300);
    let path = std::env::temp_dir().join(format!("m4-05-incomplete-{}.jsonl", std::process::id()));
    let _ = fs::remove_file(&path);
    let request = MinimalCauseRequest {
        batch: batch_request(config, context, snapshot, Vec::new(), 1),
        candidates: vec![IdentifiedIntervention::config_override(
            ConfigParameter::BirthProbability,
            ConfigValue::F32(0.1),
        )],
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::AlivePopulation,
            target_group_id: None,
            comparison: EffectComparison::GreaterThan,
            threshold: 1000.0,
        },
        replicate_ids: vec![1],
        exploratory_single_replicate: true,
        max_branch_evaluations: 1,
        max_wall_time: Duration::from_secs(60),
        evaluation_log_path: path.clone(),
    };
    let first = search_minimal_cause(&request).unwrap();
    assert_eq!(first.status, MinimalCauseStatus::Incomplete);
    assert_eq!(first.guarantee, MinimalCauseGuarantee::None);
    assert_eq!(first.stop_reason.as_deref(), Some("max_branch_evaluations"));
    assert_eq!(search_minimal_cause(&request).unwrap(), first);
    let _ = fs::remove_file(path);
}

#[test]
fn committed_minimal_cause_ci_golden() {
    let (config, context, snapshot) = fixture(2_000);
    let candidates = vec![
        IdentifiedIntervention::agent_field_override(
            AgentId(0),
            AgentField::Health,
            AgentFieldValue::Health(0.0),
        ),
        IdentifiedIntervention::agent_field_override(
            AgentId(1),
            AgentField::Health,
            AgentFieldValue::Health(0.0),
        ),
    ];
    let path = std::env::temp_dir().join(format!("m4-05-golden-{}.jsonl", std::process::id()));
    let _ = fs::remove_file(&path);
    let request = MinimalCauseRequest {
        batch: batch_request(config, context, snapshot, Vec::new(), 30),
        candidates,
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::TargetSettlementAlivePopulation,
            target_group_id: Some(GroupId(0)),
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: (1..=10).collect(),
        exploratory_single_replicate: false,
        max_branch_evaluations: 1_000,
        max_wall_time: Duration::from_secs(600),
        evaluation_log_path: path.clone(),
    };
    let result = search_minimal_cause(&request).unwrap();
    assert_eq!(search_minimal_cause(&request).unwrap(), result);
    let golden = format!(
        "status={:?}\nguarantee={:?}\ncause={}\nevaluations={}\nmean_effect={:016x}\npredicate={}\n",
        result.status,
        result.guarantee,
        result.cause_ids.join(","),
        result.evaluation_count,
        result.mean_effect.unwrap_or_default().to_bits(),
        result.predicate_value,
    );
    assert_eq!(
        golden,
        include_str!("fixtures/m4_05_minimal_cause_golden.txt")
    );
    let _ = fs::remove_file(path);
}

#[test]
fn ddmin_returns_a_one_minimal_set_after_single_removal_checks() {
    let (config, context, snapshot) = fixture(300);
    let candidates = (0..11)
        .map(|agent| {
            IdentifiedIntervention::agent_field_override(
                AgentId(agent),
                AgentField::Health,
                AgentFieldValue::Health(0.0),
            )
        })
        .collect();
    let path = std::env::temp_dir().join(format!("m4-05-ddmin-{}.jsonl", std::process::id()));
    let _ = fs::remove_file(&path);
    let request = MinimalCauseRequest {
        batch: batch_request(config, context, snapshot, Vec::new(), 30),
        candidates,
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::TargetSettlementAlivePopulation,
            target_group_id: Some(GroupId(0)),
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: vec![1],
        exploratory_single_replicate: true,
        max_branch_evaluations: 100,
        max_wall_time: Duration::from_secs(300),
        evaluation_log_path: path.clone(),
    };
    let result = search_minimal_cause(&request).unwrap();
    assert_eq!(result.status, MinimalCauseStatus::Complete);
    assert_eq!(result.guarantee, MinimalCauseGuarantee::OneMinimal);
    assert_eq!(result.cause_ids.len(), 1);
    assert!(result.predicate_value);
    let _ = fs::remove_file(path);
}

#[test]
fn exhaustive_tie_break_selects_the_smallest_sorted_id_set() {
    let (config, context, snapshot) = fixture(300);
    let candidates = [AgentId(3), AgentId(0)]
        .into_iter()
        .map(|agent_id| {
            IdentifiedIntervention::agent_field_override(
                agent_id,
                AgentField::Health,
                AgentFieldValue::Health(0.0),
            )
        })
        .collect();
    let path = std::env::temp_dir().join(format!("m4-05-tie-{}.jsonl", std::process::id()));
    let _ = fs::remove_file(&path);
    let request = MinimalCauseRequest {
        batch: batch_request(config, context, snapshot, Vec::new(), 30),
        candidates,
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::TargetSettlementAlivePopulation,
            target_group_id: Some(GroupId(0)),
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: vec![1],
        exploratory_single_replicate: true,
        max_branch_evaluations: 20,
        max_wall_time: Duration::from_secs(120),
        evaluation_log_path: path.clone(),
    };
    let result = search_minimal_cause(&request).unwrap();
    assert_eq!(result.guarantee, MinimalCauseGuarantee::Minimum);
    assert_eq!(result.cause_ids, vec!["F:agent=0:health=0x00000000"]);
    let _ = fs::remove_file(path);
}
