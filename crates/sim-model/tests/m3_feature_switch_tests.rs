use sim_model::state::HybridWorldState;
use sim_model::{
    ConfigValidationError, ConfiguredDayOutcome, ConfiguredStorageRunContext, DayExecutionOptions,
    M0RunContext, M3StorageRunContext, M3pStorageRunContext, SimConfig, SimulationProfile,
    SimulationRunnerSelection, SimulationSystems, initialize_world, run_configured_storage_days,
    run_hybrid_authority_days, run_m3_storage_days, run_m3p_storage_days,
};

const BASE: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 30
settlement_count = 3
initial_health = 1.0
initial_food = 10.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
initial_treasury = 5000

[traits]
prod_min = 0.8
prod_max = 1.5
coop_min = 0.2
coop_max = 0.8
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.1
risk_max = 0.5

[environment]
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.15

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 20.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 100.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

fn config(profile: &str) -> SimConfig {
    let mut source = BASE.to_owned();
    if profile != "m2" {
        source = source.replace("[world]", &format!("profile = \"{profile}\"\n[world]"));
        source = source.replace(
            "starvation_threshold = 5.0",
            "starvation_threshold = 5.0\nrelationship_degree_k = 8\np_local = 0.5",
        );
    }
    if profile == "m3p" {
        source.push_str(
            "\n[population]\nbirth_min_health = 0.0\nbirth_food_threshold = 0.0\nbirth_probability = 0.001\nbirth_food_cost = 0.0\nbirths_max_per_day = 0\ncompaction_interval_days = 50\n",
        );
    }
    SimConfig::parse_and_validate(&source).expect("legacy config validates")
}

fn with_systems(config: &SimConfig) -> SimConfig {
    let mut with_switches = config.clone();
    with_switches.simulation = Some(sim_model::SimulationConfig {
        systems: SimulationSystems::for_profile(config.profile),
    });
    with_switches.validate().expect("switch alias validates");
    with_switches
}

fn options() -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

#[test]
fn switch_aliases_dispatch_to_existing_m2_m3_and_m3p_paths() {
    for profile in ["m2", "m3", "m3p"] {
        let legacy = config(profile);
        let switched = with_systems(&legacy);
        assert_eq!(switched.runner_selection(), legacy.runner_selection());
        assert_eq!(
            switched.run_manifest().systems,
            SimulationSystems::for_profile(legacy.profile)
        );

        let run_context = M0RunContext::new(legacy.world.master_seed, legacy.world.replicate_id);
        let mut legacy_world =
            HybridWorldState::hybrid(initialize_world(&legacy).expect("world initializes"));
        let mut switched_world = legacy_world.clone();
        let legacy_outcomes = match legacy.profile {
            SimulationProfile::M2 => {
                run_hybrid_authority_days(&mut legacy_world, &legacy, &run_context, 3, &options())
                    .unwrap()
                    .into_iter()
                    .map(ConfiguredDayOutcome::M2)
                    .collect::<Vec<_>>()
            }
            SimulationProfile::M3 => {
                let mut context = M3StorageRunContext::new(&legacy_world, &legacy, &run_context)
                    .expect("M3 context builds");
                run_m3_storage_days(&mut legacy_world, &legacy, &mut context, 3, &options())
                    .unwrap()
                    .into_iter()
                    .map(ConfiguredDayOutcome::M3)
                    .collect()
            }
            SimulationProfile::M3p => {
                let mut context = M3pStorageRunContext::new(&legacy_world, &legacy, &run_context)
                    .expect("M3P context builds");
                run_m3p_storage_days(&mut legacy_world, &legacy, &mut context, 3, &options())
                    .unwrap()
                    .into_iter()
                    .map(ConfiguredDayOutcome::M3p)
                    .collect()
            }
        };

        let mut configured_context = match switched.profile {
            SimulationProfile::M2 => ConfiguredStorageRunContext::M2(run_context),
            SimulationProfile::M3 => ConfiguredStorageRunContext::M3(
                M3StorageRunContext::new(&switched_world, &switched, &run_context)
                    .expect("M3 switch context builds"),
            ),
            SimulationProfile::M3p => ConfiguredStorageRunContext::M3p(
                M3pStorageRunContext::new(&switched_world, &switched, &run_context)
                    .expect("M3P switch context builds"),
            ),
        };
        let configured = run_configured_storage_days(
            &mut switched_world,
            &switched,
            &mut configured_context,
            3,
            &options(),
        )
        .expect("feature dispatcher uses existing runner");
        assert_eq!(configured.manifest, switched.run_manifest());
        assert_eq!(configured.days, legacy_outcomes);
        assert_eq!(switched_world.world, legacy_world.world);
        assert_eq!(
            switched_world.segmented_storage,
            legacy_world.segmented_storage
        );
    }
}

#[test]
fn switch_config_parses_systems_without_a_legacy_profile() {
    let source = BASE
        .replace(
            "[world]",
            "[simulation.systems]\nrelationship_graph = true\n\n[world]",
        )
        .replace(
            "starvation_threshold = 5.0",
            "starvation_threshold = 5.0\nrelationship_degree_k = 8\np_local = 0.5",
        );
    let parsed = SimConfig::parse_and_validate(&source).expect("switch config validates");
    assert_eq!(parsed.profile, SimulationProfile::M3);
    assert_eq!(parsed.runner_selection(), SimulationRunnerSelection::M3);
    assert!(parsed.effective_systems().relationship_graph);
}

#[test]
fn invalid_dependencies_missing_settings_and_profile_conflicts_are_rejected() {
    let m3 = config("m3");
    let mut missing_graph = m3.clone();
    missing_graph.simulation = Some(sim_model::SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: false,
            population_dynamics: true,
            propagation: false,
        },
    });
    assert!(matches!(
        missing_graph.validate(),
        Err(ConfigValidationError::PopulationDynamicsRequiresRelationshipGraph)
    ));

    let mut propagation = m3.clone();
    propagation.simulation = Some(sim_model::SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: false,
            propagation: true,
        },
    });
    assert!(matches!(
        propagation.validate(),
        Err(ConfigValidationError::MissingPropagationSettings)
    ));

    let mut disabled_settings = m3;
    disabled_settings.propagation = Some(sim_model::PropagationConfig {
        seed_probability: 0.1,
        transmit_probability: 0.2,
        forget_probability: 0.3,
        max_hops: 1,
    });
    assert!(matches!(
        disabled_settings.validate(),
        Err(ConfigValidationError::PropagationSettingsRequirePropagation)
    ));

    let conflict = BASE.replace(
        "[world]",
        "profile = \"m3\"\n[simulation.systems]\nrelationship_graph = false\n\n[world]",
    );
    assert!(SimConfig::parse_and_validate(&conflict).is_err());
}

#[test]
fn propagation_probability_and_hop_boundaries_are_validated() {
    let mut enabled = config("m3");
    enabled.simulation = Some(sim_model::SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: false,
            propagation: true,
        },
    });
    for value in [0.0, 1.0] {
        enabled.propagation = Some(sim_model::PropagationConfig {
            seed_probability: value,
            transmit_probability: 0.5,
            forget_probability: 0.5,
            max_hops: 64,
        });
        enabled.validate().expect("closed boundaries validate");
    }
    enabled.propagation = Some(sim_model::PropagationConfig {
        seed_probability: f32::NAN,
        transmit_probability: 0.5,
        forget_probability: 0.5,
        max_hops: 1,
    });
    assert!(matches!(
        enabled.validate(),
        Err(ConfigValidationError::InvalidPropagationProbability { .. })
    ));
    enabled.propagation = Some(sim_model::PropagationConfig {
        seed_probability: 0.5,
        transmit_probability: 0.5,
        forget_probability: 0.5,
        max_hops: 65,
    });
    assert!(matches!(
        enabled.validate(),
        Err(ConfigValidationError::InvalidPropagationMaxHops(65))
    ));
}
