use sim_core::AgentId;
use sim_model::config::{SimulationConfig, SimulationProfile, SimulationSystems};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention, BranchRequest,
    ConfigParameter, ConfigValue, run_counterfactual_branch,
};
use sim_model::decision::Action;
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::m3pprop::{M3pPropagationReferenceState, encode_m3pprop_snapshot};
use sim_model::m3prop::{M3PropagationReferenceState, encode_m3prop_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot};
use sim_model::{SimConfig, initialize_world};

const M3_PROPAGATION_CONFIG: &str = include_str!("fixtures/m3_propagation.toml");
const M3P_PROPAGATION_CONFIG: &str = include_str!("fixtures/m3p_propagation_sustained.toml");

fn config(propagation: bool, population: bool) -> SimConfig {
    let mut config = if population {
        SimConfig::parse_and_validate(M3P_PROPAGATION_CONFIG).unwrap()
    } else {
        SimConfig::parse_and_validate(M3_PROPAGATION_CONFIG).unwrap()
    };
    config.simulation = None;
    config.propagation = propagation.then_some(config.propagation).flatten();
    if !population {
        config.profile = SimulationProfile::M3;
        config.population = None;
    } else {
        config.profile = SimulationProfile::M3p;
    }
    config.validate().unwrap();
    config
}

fn configured_with_propagation(population: bool) -> SimConfig {
    let mut config = config(false, population);
    config.simulation = Some(SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: population,
            propagation: true,
        },
    });
    config.propagation = Some(SimulaCivPropagationConfig::default());
    config.validate().unwrap();
    config
}

struct SimulaCivPropagationConfig;
impl SimulaCivPropagationConfig {
    fn default() -> sim_model::PropagationConfig {
        sim_model::PropagationConfig {
            seed_probability: 0.002,
            transmit_probability: 0.2,
            forget_probability: 0.02,
            max_hops: 4,
        }
    }
}

fn snapshot(config: &SimConfig) -> sim_model::CanonicalSnapshot {
    let world = initialize_world(config).unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        run_context.master_seed,
        run_context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    match config.runner_selection() {
        sim_model::SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
        sim_model::SimulationRunnerSelection::M3Propagation => {
            let state = M3PropagationReferenceState::new(world, config, &run_context).unwrap();
            encode_m3prop_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M3p => {
            let state = M3pReferenceState::new(world, config, &run_context).unwrap();
            encode_m3p_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M3pPropagation => {
            let state = M3pPropagationReferenceState::new(world, config, &run_context).unwrap();
            encode_m3pprop_snapshot(&state, &metadata).unwrap()
        }
        sim_model::SimulationRunnerSelection::M2 => unreachable!(),
    }
}

fn request(
    config: SimConfig,
    intervention: BranchIntervention,
    horizon_days: u32,
) -> BranchRequest {
    let snapshot = snapshot(&config);
    BranchRequest {
        run_context: M0RunContext::new(config.world.master_seed, config.world.replicate_id),
        config,
        snapshot,
        intervention,
        horizon_days,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![],
    }
}

#[test]
fn no_op_intervention_is_identity_for_all_supported_profiles() {
    let configs = [
        config(false, false),
        configured_with_propagation(false),
        config(false, true),
        configured_with_propagation(true),
    ];
    for config in configs {
        let existing = config.environment.base_metabolic_cost;
        let mut branch_request = request(
            config,
            BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(existing),
            },
            3,
        );
        branch_request.snapshot_days = vec![1];
        let result = run_counterfactual_branch(&branch_request).unwrap();
        assert_eq!(result.base_days, result.branch_days);
        assert_eq!(result.first_divergence, None);
        assert!(result.daily_differences.iter().all(|daily| {
            daily.changed_agent_records == 0
                && !daily.graph_changed
                && !daily.allocator_changed
                && !daily.propagation_changed
                && daily.newborn_crn_loss == 0
        }));
    }
}

#[test]
fn field_and_forced_action_interventions_change_only_the_branch() {
    let config = config(false, false);
    let field_result = run_counterfactual_branch(&request(
        config.clone(),
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Health,
            value: AgentFieldValue::Health(0.5),
        },
        2,
    ))
    .unwrap();
    assert_ne!(
        field_result.base_days[0].state_digest,
        field_result.branch_days[0].state_digest
    );
    assert_eq!(
        field_result.first_divergence.as_ref().unwrap().slot,
        sim_model::RecordedSlot::InterventionBoundary,
    );

    let forced_result = run_counterfactual_branch(&request(
        config,
        BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::Idle,
        },
        1,
    ))
    .unwrap();
    assert_eq!(
        forced_result.forced_action,
        Some(sim_model::ForcedActionOutcome::Applied)
    );
    assert_ne!(
        forced_result.base_days[0].events_digest,
        forced_result.branch_days[0].events_digest
    );
    assert!(forced_result.first_divergence.is_some());
}

#[test]
fn forced_action_without_neighbors_reports_the_existing_fallback() {
    let mut config = config(false, false);
    config.interaction.relationship_degree_k = Some(0);
    config.validate().unwrap();
    let result = run_counterfactual_branch(&request(
        config,
        BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::GiveFood,
        },
        1,
    ))
    .unwrap();
    assert_eq!(
        result.forced_action,
        Some(sim_model::ForcedActionOutcome::Fallback)
    );
}

#[test]
fn m3p_propagation_phase9b_divergence_is_attributed_to_its_hook() {
    let mut config = configured_with_propagation(true);
    let population = config.population.as_mut().unwrap();
    population.birth_min_health = 0.0;
    population.birth_probability = 0.1;
    population.birth_food_threshold = 0.5;
    population.birth_food_cost = 0.5;
    config.validate().unwrap();
    let result = run_counterfactual_branch(&request(
        config,
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BirthProbability,
            value: ConfigValue::F32(1.0),
        },
        1,
    ))
    .unwrap();
    let divergence = result.first_divergence.unwrap();
    assert!(
        matches!(divergence.slot, sim_model::RecordedSlot::Hook(hook)
        if hook.system_id == sim_model::SystemId::PopulationDynamics
            && hook.slot == sim_model::PhaseSlot::Phase9B)
    );
    assert_eq!(divergence.rng_subsystem_ids, vec![7]);
    assert!(divergence.differing_components.contains(&"allocator"));
    assert!(result.daily_differences[0].propagation_changed);
    assert!(!result.daily_differences[0].matched_births.is_empty());
    assert!(result.daily_differences[0].newborn_crn_loss > 0);
}

#[test]
fn repeated_request_is_deterministic_and_missing_target_fails_before_mutation() {
    let config = config(false, false);
    let repeated = request(
        config.clone(),
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(config.environment.base_metabolic_cost),
        },
        2,
    );
    assert_eq!(
        run_counterfactual_branch(&repeated).unwrap(),
        run_counterfactual_branch(&repeated).unwrap()
    );
    let missing = request(
        config.clone(),
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(10_000),
            field: AgentField::Food,
            value: AgentFieldValue::Food(1.0),
        },
        1,
    );
    assert!(matches!(
        run_counterfactual_branch(&missing),
        Err(sim_model::BranchError::InvalidIntervention(_))
    ));

    let mut dead_config = config.clone();
    let mut dead_world = initialize_world(&dead_config).unwrap();
    dead_world.agents[0].alive = false;
    let context = M0RunContext::new(
        dead_config.world.master_seed,
        dead_config.world.replicate_id,
    );
    let metadata = SnapshotMetadata::new(
        dead_world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let dead_request = BranchRequest {
        snapshot: encode_snapshot(&dead_world, &metadata).unwrap(),
        config: dead_config.clone(),
        run_context: context,
        intervention: BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::Idle,
        },
        horizon_days: 1,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![],
    };
    assert!(matches!(
        run_counterfactual_branch(&dead_request),
        Err(sim_model::BranchError::InvalidIntervention(_))
    ));

    dead_config.profile = SimulationProfile::M2;
    dead_config.interaction.relationship_degree_k = None;
    dead_config.interaction.p_local = None;
    dead_config.simulation = None;
    dead_config.population = None;
    dead_config.propagation = None;
    dead_config.validate().unwrap();
    let m2_world = initialize_world(&dead_config).unwrap();
    let m2_context = M0RunContext::new(
        dead_config.world.master_seed,
        dead_config.world.replicate_id,
    );
    let m2_metadata = SnapshotMetadata::new(
        m2_world.current_day.as_u32(),
        m2_context.master_seed,
        m2_context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let unsupported = BranchRequest {
        snapshot: encode_snapshot(&m2_world, &m2_metadata).unwrap(),
        config: dead_config,
        run_context: m2_context,
        intervention: BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::Idle,
        },
        horizon_days: 1,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![],
    };
    assert_eq!(
        run_counterfactual_branch(&unsupported),
        Err(sim_model::BranchError::UnsupportedProfile)
    );
}

#[test]
fn branch_snapshot_pause_resume_matches_uninterrupted_run() {
    let config = config(false, false);
    let existing = config.environment.base_metabolic_cost;
    let mut uninterrupted_request = request(
        config.clone(),
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(existing),
        },
        3,
    );
    uninterrupted_request.snapshot_days = vec![1];
    let uninterrupted = run_counterfactual_branch(&uninterrupted_request).unwrap();
    let pause_snapshot = uninterrupted.branch_days[1].snapshot.clone().unwrap();
    let resumed_request = BranchRequest {
        snapshot: pause_snapshot,
        config: config.clone(),
        run_context: M0RunContext::new(config.world.master_seed, config.world.replicate_id),
        intervention: BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(existing),
        },
        horizon_days: 1,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![],
    };
    let resumed = run_counterfactual_branch(&resumed_request).unwrap();
    assert_eq!(uninterrupted.branch_days[2], resumed.branch_days[0]);
}

#[test]
fn explicit_parallel_is_rejected_with_typed_error() {
    let config = config(false, false);
    let mut request = request(
        config.clone(),
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(config.environment.base_metabolic_cost),
        },
        1,
    );
    request.execution = BranchExecutionOption::ExplicitParallel;
    assert_eq!(
        run_counterfactual_branch(&request),
        Err(sim_model::BranchError::UnsupportedExecutionOption)
    );
}
