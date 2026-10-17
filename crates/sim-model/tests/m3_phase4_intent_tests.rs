use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::intents::generate_intents_into;
use sim_model::{
    Action, AgentState, CrossPartitionCountError, Intent, PrimaryActionChoice, RelationshipGraph,
    SettlementState, SimConfig, WorldState, count_cross_partition_interactions_per_day,
    generate_intents_m3_into, get_give_food_candidates_m3, get_steal_food_candidates_m3,
};

const M3_CONFIG: &str = r#"
profile = "m3"

[world]
master_seed = 12345
replicate_id = 7
initial_population = 4
settlement_count = 2
initial_health = 1.0
initial_food = 5.0
initial_wealth = 100
initial_settlement_resource = 10.0
initial_treasury = 100

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
base_work_yield = 1.0
food_price = 1
target_food = 5.0
target_reserve = 10
tax_rate = 0.0
welfare_payment = 0

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 10.0
relationship_degree_k = 1
p_local = 0.5

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
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

fn config() -> SimConfig {
    SimConfig::parse_and_validate(M3_CONFIG).expect("M3 config is valid")
}

fn agent(id: u32, group_id: u16, alive: bool, health: f32, food: f32) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(id),
        alive,
        birth_day: SimulationDay(0),
        health,
        food,
        wealth: 100,
        productivity: 1.0,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(group_id),
    }
}

fn world(agents: Vec<AgentState>) -> WorldState {
    WorldState {
        current_day: SimulationDay(12),
        agents,
        settlements: vec![
            SettlementState {
                group_id: GroupId(0),
                resource: 10.0,
                treasury: 100,
            },
            SettlementState {
                group_id: GroupId(1),
                resource: 10.0,
                treasury: 100,
            },
            SettlementState {
                group_id: GroupId(3),
                resource: 10.0,
                treasury: 100,
            },
            SettlementState {
                group_id: GroupId(9),
                resource: 10.0,
                treasury: 100,
            },
        ],
        initial_money_supply: 400,
    }
}

fn graph_for(world: &WorldState, degree_k: u32, p_local: f32) -> RelationshipGraph {
    let memberships: Vec<_> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    let config = config();
    RelationshipGraph::generate(
        &memberships,
        config.world.master_seed,
        config.world.replicate_id,
        degree_k,
        p_local,
    )
    .expect("test graph satisfies capacity")
}

fn choice(agent_id: u32, action: Action) -> PrimaryActionChoice {
    PrimaryActionChoice {
        agent_id: AgentId(agent_id),
        action,
    }
}

fn target(intent: &Intent) -> Option<AgentId> {
    match intent {
        Intent::GiveFood {
            target_agent_id, ..
        }
        | Intent::StealFood {
            target_agent_id, ..
        } => *target_agent_id,
        _ => None,
    }
}

#[test]
fn m3_targets_cross_settlement_neighbors_for_give_and_steal() {
    let world = world(vec![
        agent(4, 1, true, 1.0, 5.0),
        agent(2, 0, true, 1.0, 5.0),
        agent(3, 1, true, 1.0, 5.0),
        agent(1, 0, true, 1.0, 5.0),
    ]);
    let graph = graph_for(&world, 1, 0.0);
    let choices = [
        choice(1, Action::GiveFood),
        choice(2, Action::StealFood),
        choice(3, Action::Idle),
        choice(4, Action::Idle),
    ];
    let mut intents = Vec::new();
    generate_intents_m3_into(&world, &config(), &choices, &graph, &mut intents)
        .expect("M3 intent generation succeeds");

    assert_eq!(
        intents.iter().map(Intent::agent_id).collect::<Vec<_>>(),
        [AgentId(1), AgentId(2), AgentId(3), AgentId(4),]
    );
    let groups: std::collections::HashMap<_, _> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    for intent in intents
        .iter()
        .filter(|intent| matches!(intent, Intent::GiveFood { .. } | Intent::StealFood { .. }))
    {
        let selected = target(intent).expect("cross-settlement neighbor is eligible");
        assert_ne!(intent.group_id(), groups[&selected]);
    }
}

#[test]
fn m3_candidates_apply_live_filters_and_keep_agent_id_order() {
    let world = world(vec![
        agent(7, 3, false, 1.0, 5.0),
        agent(10, 3, true, 1.0, 20.0),
        agent(5, 3, true, 1.0, 10.0),
        agent(3, 3, true, 1.0, 0.0),
        agent(9, 3, true, 0.0, 5.0),
        agent(1, 3, true, 1.0, 5.0),
    ]);
    let graph = graph_for(&world, 5, 1.0);
    let initiator = world
        .agents
        .iter()
        .find(|agent| agent.agent_id == AgentId(10))
        .unwrap();

    assert_eq!(
        get_give_food_candidates_m3(initiator, &world, &graph, &config()),
        [AgentId(1), AgentId(3)]
    );
    assert_eq!(
        get_steal_food_candidates_m3(initiator, &world, &graph),
        [AgentId(1), AgentId(5)]
    );
}

#[test]
fn m3_emits_none_when_no_graph_neighbor_passes_action_filters() {
    let give_world = world(vec![
        agent(10, 0, true, 1.0, 20.0),
        agent(1, 0, true, 1.0, 10.0),
        agent(2, 0, true, 1.0, 11.0),
    ]);
    let give_graph = graph_for(&give_world, 2, 1.0);
    let give_choices = [
        choice(1, Action::Idle),
        choice(2, Action::Idle),
        choice(10, Action::GiveFood),
    ];
    let mut give_intents = Vec::new();
    generate_intents_m3_into(
        &give_world,
        &config(),
        &give_choices,
        &give_graph,
        &mut give_intents,
    )
    .unwrap();
    assert_eq!(target(&give_intents[2]), None);

    let steal_world = world(vec![
        agent(10, 0, true, 1.0, 20.0),
        agent(1, 0, true, 1.0, 0.0),
        agent(2, 0, true, 1.0, 0.0),
    ]);
    let steal_graph = graph_for(&steal_world, 2, 1.0);
    let steal_choices = [
        choice(1, Action::Idle),
        choice(2, Action::Idle),
        choice(10, Action::StealFood),
    ];
    let mut steal_intents = Vec::new();
    generate_intents_m3_into(
        &steal_world,
        &config(),
        &steal_choices,
        &steal_graph,
        &mut steal_intents,
    )
    .unwrap();
    assert_eq!(target(&steal_intents[2]), None);
}

#[test]
fn complete_same_settlement_graph_matches_existing_m2_intents() {
    let world = world(vec![
        agent(50, 3, true, 1.0, 20.0),
        agent(5, 3, true, 1.0, 5.0),
        agent(20, 3, true, 1.0, 10.0),
        agent(10, 3, true, 1.0, 0.0),
        agent(40, 3, true, 1.0, 9.0),
    ]);
    let graph = graph_for(&world, 4, 1.0);
    for source in graph.source_ids() {
        let mut expected: Vec<_> = world
            .agents
            .iter()
            .filter(|agent| agent.group_id == GroupId(3) && agent.agent_id != *source)
            .map(|agent| agent.agent_id)
            .collect();
        expected.sort_unstable();
        assert_eq!(graph.neighbors_of(*source).unwrap(), expected);
    }
    let choices = [
        choice(50, Action::GiveFood),
        choice(5, Action::StealFood),
        choice(20, Action::GiveFood),
        choice(10, Action::StealFood),
        choice(40, Action::Idle),
    ];
    let mut m2_intents = Vec::new();
    let mut m3_intents = Vec::new();
    let config = config();
    generate_intents_into(&world, &config, &choices, &mut m2_intents).unwrap();
    generate_intents_m3_into(&world, &config, &choices, &graph, &mut m3_intents).unwrap();
    assert_eq!(m3_intents, m2_intents);
}

#[test]
fn m3_intent_generation_preserves_reference_errors_and_partial_output() {
    let world = world(vec![
        agent(1, 0, true, 1.0, 5.0),
        agent(2, 0, true, 1.0, 5.0),
        agent(3, 1, false, 1.0, 5.0),
        agent(4, 1, true, 1.0, 5.0),
    ]);
    let graph = graph_for(&world, 1, 0.0);

    for choices in [
        vec![choice(1, Action::Idle), choice(1, Action::Idle)],
        vec![choice(99, Action::Idle)],
        vec![choice(3, Action::Idle)],
        vec![choice(1, Action::Idle)],
    ] {
        let mut m2_intents = Vec::new();
        let mut m3_intents = Vec::new();
        let m2_result = generate_intents_into(&world, &config(), &choices, &mut m2_intents);
        let m3_result =
            generate_intents_m3_into(&world, &config(), &choices, &graph, &mut m3_intents);
        assert_eq!(m3_result, m2_result);
        assert_eq!(m3_intents, m2_intents);
    }
}

#[test]
fn graph_built_from_shuffled_memberships_produces_identical_m3_intents() {
    let world = world(vec![
        agent(12, 9, true, 1.0, 4.0),
        agent(10, 3, true, 1.0, 5.0),
        agent(11, 9, true, 1.0, 6.0),
        agent(13, 3, true, 1.0, 7.0),
        agent(14, 9, true, 1.0, 8.0),
        agent(15, 3, true, 1.0, 9.0),
    ]);
    let memberships: Vec<_> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id))
        .collect();
    let mut shuffled = memberships.clone();
    shuffled.reverse();
    let config = config();
    let make_graph = |memberships: &[(AgentId, GroupId)]| {
        RelationshipGraph::generate(
            memberships,
            config.world.master_seed,
            config.world.replicate_id,
            2,
            0.5,
        )
        .unwrap()
    };
    let graph_a = make_graph(&memberships);
    let graph_b = make_graph(&shuffled);
    assert_eq!(graph_a, graph_b);

    let choices = [
        choice(10, Action::GiveFood),
        choice(11, Action::StealFood),
        choice(12, Action::GiveFood),
        choice(13, Action::StealFood),
        choice(14, Action::GiveFood),
        choice(15, Action::StealFood),
    ];
    let mut intents_a = Vec::new();
    let mut intents_b = Vec::new();
    generate_intents_m3_into(&world, &config, &choices, &graph_a, &mut intents_a).unwrap();
    generate_intents_m3_into(&world, &config, &choices, &graph_b, &mut intents_b).unwrap();
    assert_eq!(intents_a, intents_b);
}

#[test]
fn cross_partition_count_ignores_local_and_zero_target_intents() {
    let world = world(vec![
        agent(1, 0, true, 1.0, 5.0),
        agent(2, 0, true, 1.0, 5.0),
        agent(3, 1, true, 1.0, 5.0),
        agent(4, 1, true, 1.0, 5.0),
    ]);
    let intents = [
        Intent::GiveFood {
            agent_id: AgentId(1),
            group_id: GroupId(0),
            target_agent_id: Some(AgentId(3)),
            requested_amount: 1.0,
        },
        Intent::StealFood {
            agent_id: AgentId(3),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(4)),
            requested_amount: 1.0,
        },
        Intent::GiveFood {
            agent_id: AgentId(2),
            group_id: GroupId(0),
            target_agent_id: None,
            requested_amount: 1.0,
        },
        Intent::StealFood {
            agent_id: AgentId(4),
            group_id: GroupId(1),
            target_agent_id: Some(AgentId(1)),
            requested_amount: 1.0,
        },
        Intent::Idle {
            agent_id: AgentId(2),
            group_id: GroupId(0),
        },
    ];
    assert_eq!(
        count_cross_partition_interactions_per_day(&intents, &world),
        Ok(2)
    );

    let missing_target = [Intent::GiveFood {
        agent_id: AgentId(1),
        group_id: GroupId(0),
        target_agent_id: Some(AgentId(99)),
        requested_amount: 1.0,
    }];
    assert_eq!(
        count_cross_partition_interactions_per_day(&missing_target, &world),
        Err(CrossPartitionCountError::MissingTargetAgent(AgentId(99)))
    );
}
