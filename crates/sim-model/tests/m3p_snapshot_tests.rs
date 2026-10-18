use sim_core::{AgentId, DenseSlot, SimulationDay};
use sim_model::{
    DayExecutionOptions, Event, M0RunContext, M3P_SNAPSHOT_SCHEMA_VERSION, M3pEvent,
    M3pReferenceState, SimConfig, SnapshotError, SnapshotMetadata, decode_m3p_snapshot,
    encode_m3p_snapshot, initialize_world, run_m3p_reference_day,
};

const CONFIG: &str = r#"
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
relationship_degree_k = 2
p_local = 0.0

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

fn config() -> SimConfig {
    SimConfig::parse_and_validate(CONFIG).expect("M3P snapshot config validates")
}

fn context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn options() -> DayExecutionOptions {
    DayExecutionOptions::default()
}

fn initial_state(config: &SimConfig) -> M3pReferenceState {
    let mut world = initialize_world(config).expect("M3P world initializes");
    world.current_day = SimulationDay(1);
    M3pReferenceState::new(world, config, &context(config)).expect("M3P reference state builds")
}

fn assert_logical_state_eq(left: &M3pReferenceState, right: &M3pReferenceState) {
    let normalize = |state: &M3pReferenceState| {
        let mut normalized = state.clone();
        normalized
            .world_mut()
            .agents
            .sort_by_key(|agent| agent.agent_id);
        for (slot, agent) in normalized.world_mut().agents.iter_mut().enumerate() {
            agent.dense_slot = DenseSlot(slot as u32);
        }
        normalized
    };
    assert_eq!(normalize(left), normalize(right));
}

#[test]
fn v2_snapshot_pause_resume_matches_across_compaction_day() {
    let config = config();
    let run_context = context(&config);
    let mut uninterrupted = initial_state(&config);
    let dead_target = uninterrupted
        .relationship_graph()
        .neighbors_of(AgentId(0))
        .unwrap()[0];
    uninterrupted
        .world_mut()
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == dead_target)
        .unwrap()
        .health = 0.0;

    let day_one = run_m3p_reference_day(&mut uninterrupted, &config, &run_context, &options())
        .expect("day before compaction completes");
    let snapshot = day_one
        .snapshot
        .as_ref()
        .expect("snapshot boundary enabled");
    assert_eq!(
        u32::from_le_bytes(snapshot.bytes[8..12].try_into().unwrap()),
        M3P_SNAPSHOT_SCHEMA_VERSION
    );
    assert!(day_one.events.iter().any(|record| matches!(
        &record.event,
        M3pEvent::Existing(Event::Observation(
            sim_model::ObservationEvent::SnapshotEmitted {
                schema_version: M3P_SNAPSHOT_SCHEMA_VERSION,
                ..
            }
        ))
    )));

    let restored = decode_m3p_snapshot(&snapshot.bytes, &config).expect("v2 restore validates");
    assert_eq!(restored.metadata, snapshot.metadata);
    assert_logical_state_eq(&restored.state, &uninterrupted);
    let mut resumed = restored.state;

    let day_two_uninterrupted =
        run_m3p_reference_day(&mut uninterrupted, &config, &run_context, &options())
            .expect("uninterrupted compaction day completes");
    let day_two_resumed = run_m3p_reference_day(&mut resumed, &config, &run_context, &options())
        .expect("restored compaction day completes");
    assert_eq!(day_two_resumed, day_two_uninterrupted);
    assert_logical_state_eq(&resumed, &uninterrupted);
    let post_compaction_snapshot = day_two_uninterrupted
        .snapshot
        .as_ref()
        .expect("compaction-day snapshot emitted");
    let restored_after_compaction = decode_m3p_snapshot(&post_compaction_snapshot.bytes, &config)
        .expect("post-compaction v2 snapshot restores");
    assert_logical_state_eq(&restored_after_compaction.state, &uninterrupted);
    assert!(day_two_resumed.events.iter().any(|record| matches!(
        record.event,
        M3pEvent::Cleanup {
            removed: 1,
            replaced_edges,
            dropped_edges: 0,
        } if replaced_edges > 0
    )));
}

#[test]
fn v2_restore_rejects_a_dangling_csr_target() {
    let config = config();
    let state = initial_state(&config);
    let metadata = SnapshotMetadata::new(
        1,
        config.world.master_seed,
        config.world.replicate_id,
        "model",
        "config",
    );
    let snapshot = encode_m3p_snapshot(&state, &metadata).expect("v2 snapshot encodes");
    let base_len = u64::from_le_bytes(snapshot.bytes[12..20].try_into().unwrap()) as usize;
    let mut cursor = 20 + base_len + 8 + 8;
    let source_count = u64::from_le_bytes(snapshot.bytes[cursor..cursor + 8].try_into().unwrap());
    cursor += 8;
    let mut corrupted = snapshot.bytes.clone();
    let mut changed = false;
    for _ in 0..source_count {
        cursor += 4;
        let degree = u32::from_le_bytes(corrupted[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        if degree > 0 {
            corrupted[cursor..cursor + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            changed = true;
            break;
        }
        cursor += degree * 4;
    }
    assert!(changed, "test graph must contain at least one edge");
    assert!(matches!(
        decode_m3p_snapshot(&corrupted, &config),
        Err(SnapshotError::InvalidState(_))
    ));
}

#[test]
fn v2_restore_rejects_an_allocator_cursor_beyond_the_exhaustion_sentinel() {
    let config = config();
    let state = initial_state(&config);
    let metadata = SnapshotMetadata::new(
        2,
        config.world.master_seed,
        config.world.replicate_id,
        "model",
        "config",
    );
    let snapshot = encode_m3p_snapshot(&state, &metadata).expect("v2 snapshot encodes");
    let base_len = u64::from_le_bytes(snapshot.bytes[12..20].try_into().unwrap()) as usize;
    let cursor_offset = 20 + base_len;
    let invalid_cursor = (u32::MAX as u64 + 2).to_le_bytes();
    let mut corrupted = snapshot.bytes;
    corrupted[cursor_offset..cursor_offset + 8].copy_from_slice(&invalid_cursor);
    assert!(matches!(
        decode_m3p_snapshot(&corrupted, &config),
        Err(SnapshotError::InvalidState(_))
    ));
}

#[test]
fn v1_snapshot_decoder_does_not_accept_the_m3p_v2_envelope() {
    let config = config();
    let state = initial_state(&config);
    let metadata = SnapshotMetadata::new(
        1,
        config.world.master_seed,
        config.world.replicate_id,
        "model",
        "config",
    );
    let snapshot = encode_m3p_snapshot(&state, &metadata).expect("v2 snapshot encodes");
    assert!(matches!(
        sim_model::decode_snapshot(&snapshot.bytes),
        Err(SnapshotError::UnsupportedVersion(
            M3P_SNAPSHOT_SCHEMA_VERSION
        ))
    ));
}
