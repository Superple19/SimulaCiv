use sim_core::AgentId;
use sim_model::config::{SimulationConfig, SimulationProfile, SimulationSystems};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchExecutionOption, BranchIntervention, BranchRequest,
    ConfigParameter, ConfigValue, run_counterfactual_branch,
    run_counterfactual_branch_with_divergence_series,
};
use sim_model::m3p::{M3pEvent, M3pReferenceState, encode_m3p_snapshot};
use sim_model::m3pprop::{M3pPropagationReferenceState, encode_m3pprop_snapshot};
use sim_model::m3prop::{M3PropagationReferenceState, encode_m3prop_snapshot};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, M0RunContext};
use sim_model::snapshot::{SnapshotMetadata, encode_snapshot};
use sim_model::{Action, SimConfig, SimulationRunnerSelection, initialize_world};
use std::fmt::Write;

fn config(population: bool, propagation: bool, seed: u64) -> SimConfig {
    let fixture = if population {
        include_str!("fixtures/m3p_propagation_sustained.toml")
    } else {
        include_str!("fixtures/m3_propagation.toml")
    };
    let mut config = SimConfig::parse_and_validate(fixture).unwrap();
    config.world.initial_population = 2_000;
    config.world.master_seed = seed;
    config.simulation = None;
    if population {
        config.profile = SimulationProfile::M3p;
    } else {
        config.profile = SimulationProfile::M3;
        config.population = None;
    }
    if propagation {
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics: population,
                propagation: true,
            },
        });
    } else {
        config.propagation = None;
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
        snapshot_days: vec![],
    }
}

fn assert_partition_invariant(result: &sim_model::BranchDivergenceResult) {
    assert_eq!(result.branch.base_days.len(), result.daily_series.len());
    assert_eq!(result.branch.branch_days.len(), result.daily_series.len());
    for ((base, branch), day) in result
        .branch
        .base_days
        .iter()
        .zip(&result.branch.branch_days)
        .zip(&result.daily_series)
    {
        let component_differs = day.graph_changed
            || day.allocator_changed
            || day.propagation_changed
            || day.settlement_global_changed;
        assert_eq!(
            base.state_digest != branch.state_digest,
            day.changed_agent_records > 0 || component_differs,
            "state partition mismatch on day {}: {day:?}",
            day.day
        );
    }
}

fn encode_golden(result: &sim_model::BranchDivergenceResult) -> String {
    let mut output = String::from(
        "# day|D|CRN-loss|L|u|base-alive|branch-alive|state|graph|allocator|propagation|settlement-global|normalized-bits|unmatched-fraction-bits\n",
    );
    for day in &result.daily_series {
        writeln!(
            output,
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:016x}|{:016x}",
            day.day,
            day.changed_agent_records,
            day.newborn_crn_loss,
            day.matched_alive_entities,
            day.unmatched_birth_records,
            day.base_alive_population,
            day.branch_alive_population,
            u8::from(day.state_digests_differ),
            u8::from(day.graph_changed),
            u8::from(day.allocator_changed),
            u8::from(day.propagation_changed),
            u8::from(day.settlement_global_changed),
            day.normalized_divergence.to_bits(),
            day.unmatched_birth_fraction.to_bits(),
        )
        .unwrap();
    }
    let summary = &result.summary;
    writeln!(
        output,
        "summary|{:?}|{:?}|{:?}|{:?}|{:?}|{}|{:016x}|{:?}|{}|{:?}|{:?}|{:?}",
        summary.first_divergent_day,
        summary.cascade_onset_day,
        summary.plateau_days,
        summary.growth_slope.map(f64::to_bits),
        summary.doubling_time_days.map(f64::to_bits),
        summary.peak_changed_agent_records,
        summary.peak_normalized_fraction.to_bits(),
        summary.days_to_peak,
        summary.newborn_crn_loss,
        summary.base_target_death_day,
        summary.branch_target_death_day,
        summary.onset_vs_branch_target_death,
    )
    .unwrap();
    output
}

#[test]
fn identity_series_and_state_partition_hold_for_all_four_profiles() {
    for (population, propagation) in [(false, false), (false, true), (true, false), (true, true)] {
        let config = config(population, propagation, 81_985_529_216_486_895);
        let request = request(
            config.clone(),
            BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(config.environment.base_metabolic_cost),
            },
            30,
        );
        let expected = run_counterfactual_branch(&request).unwrap();
        let measured = run_counterfactual_branch_with_divergence_series(&request).unwrap();
        assert_eq!(measured.branch, expected);
        assert!(measured.daily_series.iter().all(|day| {
            day.changed_agent_records == 0
                && day.unmatched_birth_records == 0
                && day.normalized_divergence == 0.0
                && day.unmatched_birth_fraction == 0.0
        }));
        assert_partition_invariant(&measured);
    }
}

#[test]
fn intervention_series_match_m4_02_and_m4_03_golden() {
    let seed = 81_985_529_216_486_895;
    let m3p = config(true, false, seed);
    let cases = [
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BaseMetabolicCost,
            value: ConfigValue::F32(m3p.environment.base_metabolic_cost * 1.01),
        },
        BranchIntervention::AgentFieldOverride {
            agent_id: AgentId(0),
            field: AgentField::Health,
            value: AgentFieldValue::Health(0.0),
        },
        BranchIntervention::ForcedAction {
            agent_id: AgentId(0),
            action: Action::Idle,
        },
    ];
    let mut golden = None;
    for (case_index, intervention) in cases.into_iter().enumerate() {
        let request = request(m3p.clone(), intervention, 60);
        let expected = run_counterfactual_branch(&request).unwrap();
        let measured = run_counterfactual_branch_with_divergence_series(&request).unwrap();
        assert_eq!(measured.branch, expected);
        assert_eq!(measured.daily_series.len(), 60);
        assert_partition_invariant(&measured);
        if case_index == 1 {
            golden = Some(measured);
        }
    }
    let golden = golden.unwrap();
    assert_eq!(golden.summary.branch_target_death_day, Some(0));
    assert_eq!(
        encode_golden(&golden),
        include_str!("fixtures/m4_03_divergence_golden.txt").replace("\r\n", "\n")
    );
}

#[test]
fn shared_birth_lineage_and_snapshot_boundary_are_reported_without_m4_02_changes() {
    let mut request = request(
        config(true, false, 81_985_529_216_486_895),
        BranchIntervention::ConfigOverride {
            parameter: ConfigParameter::BirthProbability,
            value: ConfigValue::F32(0.0001),
        },
        60,
    );
    request.snapshot_days = vec![50];
    let expected = run_counterfactual_branch(&request).unwrap();
    let measured = run_counterfactual_branch_with_divergence_series(&request).unwrap();
    assert_eq!(measured.branch, expected);
    assert_partition_invariant(&measured);
    assert_eq!(measured.summary.newborn_crn_loss, 0);
    assert!(
        measured
            .branch
            .daily_differences
            .iter()
            .map(|day| day.matched_births.len())
            .sum::<usize>()
            > 0
    );
    assert!(
        measured
            .branch
            .base_days
            .iter()
            .any(|day| day.day == 50 && day.snapshot.is_some())
    );
    assert!(measured.branch.base_days.iter().any(|day| {
        matches!(
            &day.events,
            sim_model::BranchEventsRecord::Population(records)
                if records.iter().any(|record| matches!(record.event, M3pEvent::Cleanup { .. }))
        )
    }));
}
