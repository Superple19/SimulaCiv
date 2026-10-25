use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
use sim_model::state::AgentState;
use sim_model::storage::SegmentedAgentStorage;

fn agent(id: u32, dense_slot: u32) -> AgentState {
    AgentState {
        agent_id: AgentId(id),
        dense_slot: DenseSlot(dense_slot),
        alive: true,
        birth_day: SimulationDay(0),
        health: 1.0,
        food: 2.0,
        wealth: 3,
        productivity: 0.5,
        cooperation: 0.5,
        aggression: 0.5,
        risk_tolerance: 0.5,
        group_id: GroupId(4),
    }
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "duplicate AgentId 1")]
fn push_debug_asserts_on_duplicate_agent_id() {
    let first = agent(1, 0);
    let mut storage = SegmentedAgentStorage::from_agents(std::slice::from_ref(&first));
    storage.push(&first);
}

#[test]
fn deserialize_round_trip_rebuilds_slot_map() {
    let storage = SegmentedAgentStorage::from_agents(&[agent(7, 0), agent(3, 1)]);
    let serialized = serde_json::to_vec(&storage).expect("storage serializes");
    let restored: SegmentedAgentStorage =
        serde_json::from_slice(&serialized).expect("storage deserializes");

    assert_eq!(restored.slot_of(AgentId(7)), Some(0));
    assert_eq!(restored.slot_of(AgentId(3)), Some(1));
    assert_eq!(restored.to_agents(), storage.to_agents());
}

#[test]
fn deserialize_rejects_duplicate_agent_ids() {
    let storage = SegmentedAgentStorage::from_agents(&[agent(7, 0), agent(3, 1)]);
    let mut serialized = serde_json::to_value(&storage).expect("storage serializes");
    serialized["agent_ids"][1] = serde_json::json!(7);

    let error = serde_json::from_value::<SegmentedAgentStorage>(serialized)
        .expect_err("duplicate AgentIds cannot form a consistent slot map");
    assert!(error.to_string().contains("duplicate AgentId 7"));
}

#[test]
fn deserialize_rejects_mismatched_column_lengths() {
    let storage = SegmentedAgentStorage::from_agents(&[agent(7, 0), agent(3, 1)]);
    let mut serialized = serde_json::to_value(&storage).expect("storage serializes");
    serialized["demography"]["health"]
        .as_array_mut()
        .expect("health serializes as an array")
        .pop();

    let error = serde_json::from_value::<SegmentedAgentStorage>(serialized)
        .expect_err("every storage column must align with agent_ids");
    assert!(
        error
            .to_string()
            .contains("column `demography.health` has length 1, expected 2")
    );
}
