use sim_model::state::HybridWorldState;
use sim_model::{
    BranchExecutionOption, BranchIntervention, BranchRequest, ConfigParameter, ConfigValue,
    DayExecutionOptions, M0RunContext, M3PropagationStorageRunContext, M3StorageRunContext,
    M3pPropagationStorageRunContext, M3pStorageRunContext, MigrationConfig, SimConfig,
    SimulationConfig, SimulationProfile, SimulationRunnerSelection, SimulationSystems,
    initialize_world, run_counterfactual_branch_with_divergence_series, run_m3_storage_days,
    run_m3p_propagation_storage_days, run_m3p_storage_days, run_m3prop_storage_days,
};

fn config(profile: SimulationProfile, propagation: bool) -> SimConfig {
    let mut config =
        SimConfig::parse_and_validate(include_str!("fixtures/m3p_propagation_sustained.toml"))
            .unwrap();
    let population = profile == SimulationProfile::M3p;
    config.profile = profile;
    config.world.initial_population = 60;
    config.world.settlement_count = 3;
    config.world.master_seed = 0x5566;
    config.world.replicate_id = 7;
    config.simulation = Some(SimulationConfig {
        systems: SimulationSystems {
            relationship_graph: true,
            population_dynamics: population,
            propagation,
        },
    });
    if !population {
        config.population = None;
    }
    if !propagation {
        config.propagation = None;
    }
    config.enable_migration(MigrationConfig {
        migration_probability: 0.25,
        migration_threshold: 0.0,
        migration_cost: 10,
        migration_cooldown_days: 30,
    });
    config.validate().unwrap();
    config
}

fn snapshot(config: &SimConfig, run_context: &M0RunContext) -> sim_model::CanonicalSnapshot {
    let initial = initialize_world(config).unwrap();
    let mut hybrid = HybridWorldState::hybrid(initial);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: true,
    };
    match config.runner_selection() {
        SimulationRunnerSelection::M3 => {
            let mut context = M3StorageRunContext::new(&hybrid, config, run_context).unwrap();
            run_m3_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .unwrap()
                .remove(0)
                .snapshot
                .unwrap()
        }
        SimulationRunnerSelection::M3Propagation => {
            let mut context =
                M3PropagationStorageRunContext::new(&hybrid, config, run_context).unwrap();
            run_m3prop_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .unwrap()
                .remove(0)
                .snapshot
                .unwrap()
        }
        SimulationRunnerSelection::M3p => {
            let mut context = M3pStorageRunContext::new(&hybrid, config, run_context).unwrap();
            run_m3p_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .unwrap()
                .remove(0)
                .snapshot
                .unwrap()
        }
        SimulationRunnerSelection::M3pPropagation => {
            let mut context =
                M3pPropagationStorageRunContext::new(&hybrid, config, run_context).unwrap();
            run_m3p_propagation_storage_days(&mut hybrid, config, &mut context, 1, &options)
                .unwrap()
                .remove(0)
                .snapshot
                .unwrap()
        }
        SimulationRunnerSelection::M2 => unreachable!("migration requires an M3-family profile"),
    }
}

#[test]
fn m4_06_migration_profiles_keep_noop_branch_identity() {
    for (profile, propagation) in [
        (SimulationProfile::M3, false),
        (SimulationProfile::M3p, false),
        (SimulationProfile::M3, true),
        (SimulationProfile::M3p, true),
    ] {
        let config = config(profile, propagation);
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let branch_point = snapshot(&config, &run_context);
        let request = BranchRequest {
            snapshot: branch_point,
            config: config.clone(),
            run_context,
            intervention: BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(config.environment.base_metabolic_cost),
            },
            horizon_days: 3,
            execution: BranchExecutionOption::Serial,
            snapshot_days: Vec::new(),
        };
        let result = run_counterfactual_branch_with_divergence_series(&request).unwrap();
        assert_eq!(result.branch.base_days, result.branch.branch_days);
        assert_eq!(result.branch.daily_differences.len(), 3);
        assert!(
            result
                .branch
                .daily_differences
                .iter()
                .all(|day| day.changed_agent_records == 0)
        );
        assert!(
            result
                .daily_series
                .iter()
                .all(|day| day.changed_agent_records == 0 && !day.state_digests_differ)
        );
    }
}
