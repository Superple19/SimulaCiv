use sim_core::{AgentId, SimulationDay};
use sim_model::{
    DayExecutionOptions, M0RunContext, M3P_BIRTH_SUBSYSTEM_ID, M3pEvent, M3pReferenceState,
    SimConfig, initialize_world, run_m3p_reference_day,
};

const BASE_CONFIG: &str = r#"
profile = "m3p"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 20
settlement_count = 2
initial_health = 1.0
initial_food = 5.0
initial_wealth = 1000
initial_settlement_resource = 1.0
initial_treasury = 0

[traits]
prod_min = 0.5
prod_max = 2.5
coop_min = 0.0
coop_max = 1.0
aggr_min = 0.0
aggr_max = 1.0
risk_min = 0.0
risk_max = 1.0

[environment]
carrying_capacity = 100.0
regrowth_rate = 0.0
base_metabolic_cost = 0.0
health_decay_rate = 0.0

[economy]
base_work_yield = 0.0
food_price = 1
target_food = 1.0
target_reserve = 1
tax_rate = 0.0
welfare_payment = 0

[interaction]
gift_amount = 0.0
theft_amount = 0.0
theft_success_probability = 0.5
starvation_threshold = 1.0
relationship_degree_k = 0
p_local = 0.5

[population]
birth_min_health = 0.0
birth_food_threshold = 0.5
birth_probability = 1.0
birth_food_cost = 0.5
births_max_per_day = 1
compaction_interval_days = 2

[decision]
decision_temperature = 1.0
action_biases = [1000.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 0.0
trait_weight_aggression = 0.0
trait_weight_risk_tolerance = 0.0
"#;

fn config_with(
    degree: u32,
    p_local: f32,
    birth_probability: f32,
    births_max_per_day: u32,
    compaction_interval_days: u32,
) -> SimConfig {
    let source = BASE_CONFIG
        .replace(
            "relationship_degree_k = 0",
            &format!("relationship_degree_k = {degree}"),
        )
        .replace("p_local = 0.5", &format!("p_local = {p_local}"))
        .replace(
            "birth_probability = 1.0",
            &format!("birth_probability = {birth_probability}"),
        )
        .replace(
            "births_max_per_day = 1",
            &format!("births_max_per_day = {births_max_per_day}"),
        )
        .replace(
            "compaction_interval_days = 2",
            &format!("compaction_interval_days = {compaction_interval_days}"),
        );
    SimConfig::parse_and_validate(&source).expect("M3P test config validates")
}

fn context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn options() -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

fn state_for_day(config: &SimConfig, day: u32) -> M3pReferenceState {
    let mut world = initialize_world(config).expect("M3P initializes shared day-zero state");
    world.current_day = SimulationDay(day);
    M3pReferenceState::new(world, config, &context(config)).expect("M3P reference state builds")
}

#[test]
fn m3p_births_transfer_food_copy_traits_emit_events_and_become_active_next_day() {
    let config = config_with(0, 0.5, 1.0, 1, 2);
    let run_context = context(&config);
    let mut state = state_for_day(&config, 1);
    let parent_before = state.world().agents[0].clone();
    let graph_hash_before = *state.relationship_graph().csr_hash();

    let day_one = run_m3p_reference_day(&mut state, &config, &run_context, &options())
        .expect("birth day executes");
    assert_eq!(day_one.executed_day, 1);
    let metrics = day_one.metrics.as_ref().expect("metrics enabled");
    assert_eq!(metrics.births, 1);
    assert_eq!(metrics.removed, 0);
    assert_eq!(metrics.replaced_edges, 0);
    assert_eq!(metrics.live_population, 21);
    assert_eq!(metrics.m3_metrics.metrics.population, 21);
    assert_eq!(metrics.next_agent_id, 21);

    let child_id = AgentId(20);
    let parent_after = state
        .world()
        .agents
        .iter()
        .find(|agent| agent.agent_id == parent_before.agent_id)
        .unwrap();
    assert_eq!(
        parent_after.food.to_bits(),
        (parent_before.food - 0.5).to_bits()
    );
    let child = state
        .world()
        .agents
        .iter()
        .find(|agent| agent.agent_id == child_id)
        .unwrap();
    assert_eq!(child.group_id, parent_before.group_id);
    assert!(child.alive);
    assert_eq!(child.birth_day, SimulationDay(1));
    assert_eq!(child.health, config.world.initial_health);
    assert_eq!(child.food, 0.5);
    assert_eq!(child.wealth, 0);
    assert_eq!(child.productivity, parent_before.productivity);
    assert_eq!(child.cooperation, parent_before.cooperation);
    assert_eq!(child.aggression, parent_before.aggression);
    assert_eq!(child.risk_tolerance, parent_before.risk_tolerance);
    assert!(
        state
            .relationship_graph()
            .neighbors_of(child_id)
            .unwrap()
            .is_empty()
    );
    assert_ne!(*state.relationship_graph().csr_hash(), graph_hash_before);

    let births: Vec<_> = day_one
        .events
        .iter()
        .filter_map(|record| match &record.event {
            M3pEvent::Birth {
                parent,
                child,
                group,
            } => Some((record.key, *parent, *child, *group)),
            _ => None,
        })
        .collect();
    assert_eq!(births.len(), 1);
    assert_eq!(births[0].1, parent_before.agent_id);
    assert_eq!(births[0].2, child_id);
    assert_eq!(births[0].3, parent_before.group_id);
    assert_eq!(births[0].0.local_sequence, 0);

    let day_two = run_m3p_reference_day(&mut state, &config, &run_context, &options())
        .expect("newborn participates from the next day");
    assert_eq!(day_two.executed_day, 2);
    let metrics = day_two.metrics.as_ref().unwrap();
    assert_eq!(metrics.live_population, 22);
    assert_eq!(metrics.births, 1);
    assert!(day_two.events.iter().any(|record| matches!(
        &record.event,
        M3pEvent::Existing(sim_model::Event::StateTransition(
            sim_model::StateTransitionEvent::WorkResolved { agent_id, .. }
        )) if *agent_id == child_id
    )));
    assert!(day_two.events.iter().any(|record| matches!(
        record.event,
        M3pEvent::Cleanup {
            removed: 0,
            replaced_edges: 0,
            dropped_edges: 0,
        }
    )));
}

#[test]
fn newborn_rows_use_fixed_prebirth_pool_and_allow_short_rows_and_zero_degree() {
    let config = config_with(8, 1.0, 1.0, 2, 2);
    let run_context = context(&config);
    let mut state = state_for_day(&config, 1);
    for agent in &mut state.world_mut().agents {
        if agent.agent_id != AgentId(0) && agent.agent_id != AgentId(2) {
            agent.health = 0.0;
        }
    }

    let outcome = run_m3p_reference_day(&mut state, &config, &run_context, &options())
        .expect("M3P day with a depleted candidate pool executes");
    assert_eq!(outcome.metrics.as_ref().unwrap().births, 2);
    assert_eq!(
        state
            .world()
            .agents
            .iter()
            .filter(|agent| agent.alive)
            .count(),
        4
    );
    assert!(
        outcome
            .events
            .iter()
            .filter(|record| matches!(&record.event, M3pEvent::Birth { .. }))
            .all(|record| record.key.local_sequence >= 18)
    );

    for (child, parent, other_parent) in [
        (AgentId(20), AgentId(0), AgentId(2)),
        (AgentId(21), AgentId(2), AgentId(0)),
    ] {
        let mut expected_neighbors = vec![parent, other_parent];
        expected_neighbors.sort_unstable();
        assert_eq!(
            state.relationship_graph().neighbors_of(child).unwrap(),
            expected_neighbors
        );
        assert!(
            state
                .relationship_graph()
                .neighbors_of(child)
                .unwrap()
                .iter()
                .all(|neighbor| neighbor.0 < 20)
        );
    }
    assert_eq!(state.next_agent_id(), 22);
}

#[test]
fn compaction_replaces_dead_targets_when_possible_and_drops_them_when_not() {
    let replacement_config = config_with(2, 0.0, 0.0, 0, 1);
    let replacement_context = context(&replacement_config);
    let mut replacement_state = state_for_day(&replacement_config, 1);
    let source_id = AgentId(0);
    let dead_target = replacement_state
        .relationship_graph()
        .neighbors_of(source_id)
        .unwrap()[0];
    replacement_state
        .world_mut()
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == dead_target)
        .unwrap()
        .health = 0.0;

    let replaced = run_m3p_reference_day(
        &mut replacement_state,
        &replacement_config,
        &replacement_context,
        &options(),
    )
    .expect("compaction replaces a dead endpoint from the remaining cross pool");
    let cleanup = replaced
        .events
        .iter()
        .find_map(|record| match &record.event {
            M3pEvent::Cleanup {
                removed,
                replaced_edges,
                dropped_edges,
            } => Some((*removed, *replaced_edges, *dropped_edges)),
            _ => None,
        })
        .unwrap();
    assert_eq!(cleanup.0, 1);
    assert!(cleanup.1 > 0);
    assert_eq!(cleanup.2, 0);
    assert!(
        !replacement_state
            .relationship_graph()
            .neighbors_of(source_id)
            .unwrap()
            .contains(&dead_target)
    );
    assert!(
        replacement_state
            .relationship_graph()
            .source_ids()
            .binary_search(&dead_target)
            .is_err()
    );
    assert_eq!(
        replacement_state.relationship_graph().source_ids(),
        &replacement_state
            .world()
            .agents
            .iter()
            .map(|agent| agent.agent_id)
            .collect::<Vec<_>>()[..]
    );

    let dropped_config = config_with(2, 0.0, 0.0, 0, 1);
    let dropped_context = context(&dropped_config);
    let mut dropped_state = state_for_day(&dropped_config, 1);
    for agent in &mut dropped_state.world_mut().agents {
        if agent.group_id == sim_core::GroupId(1) {
            agent.health = 0.0;
        }
    }
    let dropped = run_m3p_reference_day(
        &mut dropped_state,
        &dropped_config,
        &dropped_context,
        &options(),
    )
    .expect("compaction drops edges when the cross pool is empty");
    let cleanup = dropped
        .events
        .iter()
        .find_map(|record| match &record.event {
            M3pEvent::Cleanup {
                removed,
                replaced_edges,
                dropped_edges,
            } => Some((*removed, *replaced_edges, *dropped_edges)),
            _ => None,
        })
        .unwrap();
    assert_eq!(cleanup.0, 10);
    assert_eq!(cleanup.1, 0);
    assert_eq!(cleanup.2, 20);
    assert!(
        dropped_state
            .relationship_graph()
            .neighbors_of(AgentId(0))
            .unwrap()
            .is_empty()
    );
    assert!(
        dropped_state
            .relationship_graph()
            .neighbors()
            .iter()
            .all(|target| dropped_state
                .world()
                .agents
                .iter()
                .any(|agent| agent.agent_id == *target))
    );
}

#[test]
fn m3p_birth_batch_id_exhaustion_is_atomic() {
    let mut config = config_with(0, 0.5, 1.0, 1, 2);
    config.population.as_mut().unwrap().birth_food_threshold = 0.0;
    config.population.as_mut().unwrap().birth_food_cost = 0.0;
    let run_context = context(&config);
    let source_state = state_for_day(&config, 1);
    let exhausted = M3pReferenceState::from_parts(
        source_state.world().clone(),
        source_state.relationship_graph().clone(),
        u32::MAX as u64 + 1,
        &config,
        &run_context,
    )
    .expect("exhausted cursor sentinel is a valid state");
    let mut failed_state = exhausted.clone();

    let mut no_birth_config = config.clone();
    no_birth_config
        .population
        .as_mut()
        .unwrap()
        .birth_probability = 0.0;
    let mut baseline_state = M3pReferenceState::from_parts(
        source_state.world().clone(),
        source_state.relationship_graph().clone(),
        u32::MAX as u64 + 1,
        &no_birth_config,
        &run_context,
    )
    .expect("baseline cursor sentinel is valid");
    let error = run_m3p_reference_day(&mut failed_state, &config, &run_context, &options())
        .expect_err("birth cannot allocate from the exhausted sentinel");
    assert!(matches!(
        error,
        sim_model::M0RunError::InvariantViolation(_)
    ));
    run_m3p_reference_day(
        &mut baseline_state,
        &no_birth_config,
        &run_context,
        &options(),
    )
    .expect("no-birth reference completes");
    assert_eq!(failed_state.next_agent_id(), u32::MAX as u64 + 1);
    assert_eq!(failed_state.world().agents, baseline_state.world().agents);
    assert_eq!(
        failed_state.relationship_graph(),
        baseline_state.relationship_graph()
    );
}

#[test]
fn m3p_phase9b_uses_the_m3p_only_subsystem_id() {
    assert_eq!(M3P_BIRTH_SUBSYSTEM_ID, 7);
}
