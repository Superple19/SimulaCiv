use sim_core::AgentId;
use sim_model::config::{SimulationConfig, SimulationProfile, SimulationSystems};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchAnalysisMode, BranchAnalysisOutput, BranchDayRecord,
    BranchEventsRecord, BranchExecutionOption, BranchIntervention, BranchMetricsRecord,
    BranchRequest, BranchSimulationDay, ConfigParameter, ConfigValue, run_counterfactual_branch,
    run_counterfactual_branch_with_analysis_mode,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::m3pprop::{M3pPropagationReferenceState, encode_m3pprop_snapshot};
use sim_model::m3prop::{M3PropagationReferenceState, encode_m3prop_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot};
use sim_model::{SimConfig, SimulationRunnerSelection, initialize_world};

fn config(propagation: bool, population: bool) -> SimConfig {
    let fixture = if population {
        include_str!("fixtures/m3p_propagation_sustained.toml")
    } else {
        include_str!("fixtures/m3_propagation.toml")
    };
    let mut config = SimConfig::parse_and_validate(fixture).unwrap();
    config.simulation = None;
    config.propagation = propagation.then_some(config.propagation).flatten();
    if !population {
        config.profile = SimulationProfile::M3;
        config.population = None;
    } else {
        config.profile = SimulationProfile::M3p;
    }
    if propagation {
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics: population,
                propagation: true,
            },
        });
    }
    config.validate().unwrap();
    config
}

fn request(
    config: SimConfig,
    intervention: BranchIntervention,
    horizon_days: u32,
) -> BranchRequest {
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).unwrap();
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        run_context.master_seed,
        run_context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let snapshot = match config.runner_selection() {
        SimulationRunnerSelection::M3 => encode_snapshot(&world, &metadata).unwrap(),
        SimulationRunnerSelection::M3Propagation => {
            let state = M3PropagationReferenceState::new(world, &config, &run_context).unwrap();
            encode_m3prop_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M3p => {
            let state = M3pReferenceState::new(world, &config, &run_context).unwrap();
            encode_m3p_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M3pPropagation => {
            let state = M3pPropagationReferenceState::new(world, &config, &run_context).unwrap();
            encode_m3pprop_snapshot(&state, &metadata).unwrap()
        }
        SimulationRunnerSelection::M2 => unreachable!(),
    };
    BranchRequest {
        config,
        run_context,
        snapshot,
        intervention,
        horizon_days,
        execution: BranchExecutionOption::Serial,
        snapshot_days: vec![1],
    }
}

fn assert_simulation_day_matches_record(day: &BranchSimulationDay, record: &BranchDayRecord) {
    assert_eq!(day.executed_day(), record.day);
    match (day, &record.metrics, &record.events) {
        (
            BranchSimulationDay::M3(day),
            BranchMetricsRecord::M3(metrics),
            BranchEventsRecord::Standard(events),
        ) => {
            assert_eq!(day.metrics.as_ref(), Some(metrics));
            assert_eq!(&day.events, events);
            assert_eq!(&day.snapshot, &record.snapshot);
        }
        (
            BranchSimulationDay::M3Propagation(day),
            BranchMetricsRecord::M3Propagation(metrics),
            BranchEventsRecord::Standard(events),
        ) => {
            assert_eq!(day.metrics.as_ref(), Some(metrics));
            assert_eq!(&day.events, events);
            assert_eq!(&day.snapshot, &record.snapshot);
        }
        (
            BranchSimulationDay::M3p(day),
            BranchMetricsRecord::M3p(metrics),
            BranchEventsRecord::Population(events),
        ) => {
            assert_eq!(day.metrics.as_ref(), Some(metrics));
            assert_eq!(&day.events, events);
            assert_eq!(&day.snapshot, &record.snapshot);
        }
        (
            BranchSimulationDay::M3pPropagation(day),
            BranchMetricsRecord::M3pPropagation(metrics),
            BranchEventsRecord::Population(events),
        ) => {
            assert_eq!(day.metrics.as_ref(), Some(metrics));
            assert_eq!(&day.events, events);
            assert_eq!(&day.snapshot, &record.snapshot);
        }
        _ => panic!("analysis mode changed the selected profile output shape"),
    }
}

#[test]
fn analysis_switch_preserves_default_and_simulation_outputs_for_all_profiles() {
    for config in [
        config(false, false),
        config(true, false),
        config(false, true),
        config(true, true),
    ] {
        let current = config.environment.base_metabolic_cost;
        let request = request(
            config,
            BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(current),
            },
            3,
        );
        let default = run_counterfactual_branch(&request).unwrap();
        let full = run_counterfactual_branch_with_analysis_mode(&request, BranchAnalysisMode::Full)
            .unwrap();
        let BranchAnalysisOutput::Full { result, .. } = full else {
            panic!("full mode returned the wrong output variant");
        };
        assert_eq!(result, default);

        let disabled =
            run_counterfactual_branch_with_analysis_mode(&request, BranchAnalysisMode::Disabled)
                .unwrap();
        let BranchAnalysisOutput::AnalysisDisabled(simulation) = disabled else {
            panic!("disabled mode returned the wrong output variant");
        };
        assert_eq!(simulation.base_days.len(), default.base_days.len());
        assert_eq!(simulation.branch_days.len(), default.branch_days.len());
        for (day, record) in simulation.base_days.iter().zip(&default.base_days) {
            assert_simulation_day_matches_record(day, record);
        }
        for (day, record) in simulation.branch_days.iter().zip(&default.branch_days) {
            assert_simulation_day_matches_record(day, record);
        }
        assert_eq!(simulation.forced_action, default.forced_action);
    }
}

#[test]
fn comparison_disabled_recorder_replays_to_the_same_boundary() {
    let mut config = config(false, true);
    let population = config.population.as_mut().unwrap();
    population.birth_min_health = 0.0;
    population.birth_probability = 0.1;
    population.birth_food_threshold = 0.5;
    population.birth_food_cost = 0.5;
    config.validate().unwrap();
    let request = request(
        config,
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BirthProbability,
            value: ConfigValue::F32(1.0),
        },
        2,
    );
    let default = run_counterfactual_branch(&request).unwrap();
    let expected = default
        .first_divergence
        .clone()
        .expect("the birth override should diverge");
    let measured = run_counterfactual_branch_with_analysis_mode(
        &request,
        BranchAnalysisMode::RecorderComparisonsDisabled {
            expected: expected.clone(),
        },
    )
    .unwrap();
    let BranchAnalysisOutput::Full { result, costs } = measured else {
        panic!("recorder mode returned the wrong output variant");
    };
    assert_eq!(result, default);
    assert!(costs.recorder_replay_ns > 0);
    assert_eq!(costs.recorder_compare_ns, 0);
}

#[test]
fn default_analysis_path_matches_explicit_full_mode_for_field_override() {
    let config = config(false, false);
    let request = request(
        config,
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Food,
            value: AgentFieldValue::Food(0.0),
        },
        2,
    );
    let default = run_counterfactual_branch(&request).unwrap();
    let measured =
        run_counterfactual_branch_with_analysis_mode(&request, BranchAnalysisMode::Full).unwrap();
    let BranchAnalysisOutput::Full { result, .. } = measured else {
        panic!("full mode returned the wrong output variant");
    };
    assert_eq!(result, default);
}
