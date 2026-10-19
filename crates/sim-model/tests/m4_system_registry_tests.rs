use sim_model::{
    RegisteredCorePhase, RegisteredSystemRunOptions, SegmentedAgentStorage, SimConfig,
    SystemExecutionOptions, SystemId, initialize_world, run_registered_storage_day_loop,
    run_registered_world_day_loop,
};

fn test_config() -> SimConfig {
    let mut config: SimConfig =
        toml::from_str(include_str!("fixtures/m3_propagation.toml")).unwrap();
    config.simulation = None;
    config.propagation = None;
    config
}

#[test]
fn generic_day_loops_visit_core_phases_in_frozen_order_for_both_layouts() {
    let config = test_config();
    let mut world = initialize_world(&config).unwrap();
    let expected_day = world.current_day.as_u32();
    let enabled_systems: Vec<SystemId> = Vec::new();
    let run_options = RegisteredSystemRunOptions {
        enabled_systems: &enabled_systems,
        execution: SystemExecutionOptions::default(),
    };
    let mut reference_trace = Vec::new();
    let reference_days = run_registered_world_day_loop(
        &mut world,
        &mut reference_trace,
        &run_options,
        2,
        |_, phase, _, _, trace, _| {
            trace.push(phase);
            Ok(())
        },
        |_, _, _, _, _| panic!("no registered systems are enabled"),
        |day, world, _| Ok((day, world.current_day.as_u32())),
    )
    .unwrap();

    let core_phases = [
        RegisteredCorePhase::Phase1,
        RegisteredCorePhase::Phase2,
        RegisteredCorePhase::Phase3,
        RegisteredCorePhase::Phase4,
        RegisteredCorePhase::Phase5,
        RegisteredCorePhase::Phase6A,
        RegisteredCorePhase::Phase6B,
        RegisteredCorePhase::Phase7,
        RegisteredCorePhase::Phase8,
        RegisteredCorePhase::Phase9,
        RegisteredCorePhase::Phase10,
        RegisteredCorePhase::Phase11,
    ];
    assert_eq!(reference_trace, [core_phases, core_phases].concat());
    assert_eq!(reference_days, [(0, expected_day), (1, expected_day)]);

    let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
    let mut storage_trace = Vec::new();
    let storage_days = run_registered_storage_day_loop(
        &mut storage,
        &mut storage_trace,
        &run_options,
        2,
        |_, phase, _, _, trace, _| {
            trace.push(phase);
            Ok(())
        },
        |_, _, _, _, _| panic!("no registered systems are enabled"),
        |day, _, _| Ok(day),
    )
    .unwrap();
    assert_eq!(storage_trace, [core_phases, core_phases].concat());
    assert_eq!(storage_days, [0, 1]);
}
