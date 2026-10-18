use sim_model::hashing::{
    CanonicalHash, canonical_m3p_state_bytes, canonical_m3p_state_bytes_from_storage,
    canonical_m3p_state_hash, canonical_m3p_state_hash_from_storage,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{
    DayExecutionOptions, Intent, M0RunContext, M3Phase4SlotProjection, M3Phase6BStorageScratch,
    M3pDailyMetrics, M3pDayOutcome, M3pReferenceState, M3pStorageRunContext, Phase3ScarcityScratch,
    Phase5PartitionScratch, SettlementTargetedResolution, SimConfig, canonical_m3p_event_hash,
    canonical_m3p_metrics_hash, decode_m3p_snapshot, generate_intents_m3_into,
    generate_intents_m3_storage_into, initialize_world, phase1_resource_regrowth,
    phase2_biological_degradation, phase3_observation_and_features_into,
    phase3_observation_and_features_storage_with_scratch, phase4_primary_action_selection_into,
    phase4_primary_action_selection_storage_into, phase5_partition_intents_baseline,
    phase5_partition_intents_from_vec_with_scratch, phase6a_work_resolution,
    phase6a_work_resolution_storage, phase6b_targeted_resolution_m3,
    phase6b_targeted_resolution_storage_m3_with_scratch, run_m3p_reference_day,
    run_m3p_storage_days,
};
use std::time::Instant;

const CONFIG: &str = r#"
profile = "m3p"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 300
settlement_count = 3
initial_health = 1.0
initial_food = 15.0
initial_wealth = 100
initial_settlement_resource = 1000000.0
initial_treasury = 100

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
carrying_capacity = 1000000.0
regrowth_rate = 0.1
base_metabolic_cost = 1.0
health_decay_rate = 0.2

[economy]
base_work_yield = 1.0
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
relationship_degree_k = 8
p_local = 0.5

[population]
birth_min_health = 0.5
birth_food_threshold = 0.5
birth_probability = 0.05
birth_food_cost = 0.5
births_max_per_day = 1
compaction_interval_days = 50

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
    SimConfig::parse_and_validate(CONFIG).expect("M3P storage config validates")
}

fn edge_drop_config() -> SimConfig {
    let mut config = config();
    config.world.settlement_count = 10;
    config.interaction.relationship_degree_k = Some(16);
    config.economy.base_work_yield = 0.7;
    config.validate().expect("edge-drop config validates");
    config
}

fn matrix_config(seed: u64, settlements: u32, degree: i64, p_local: f32) -> SimConfig {
    let mut config = config();
    config.world.master_seed = seed;
    config.world.settlement_count = settlements;
    config.interaction.relationship_degree_k = Some(degree);
    config.interaction.p_local = Some(p_local);
    config.validate().expect("matrix config validates");
    config
}

fn context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn state(config: &SimConfig) -> M3pReferenceState {
    let world = initialize_world(config).expect("M3P world initializes");
    M3pReferenceState::new(world, config, &context(config)).expect("M3P reference initializes")
}

fn snapshot_world(world: &WorldState, storage: &SegmentedAgentStorage) -> WorldState {
    let mut snapshot_world = world.clone();
    snapshot_world.agents = storage.to_agents();
    snapshot_world
}

#[derive(Debug, PartialEq)]
struct PhaseOutputs {
    intents: Vec<Intent>,
    work: Vec<sim_model::SettlementWorkResolution>,
    targeted: Vec<SettlementTargetedResolution>,
}

fn phase_outputs_reference(
    reference: &M3pReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
) -> PhaseOutputs {
    let mut world = reference.world().clone();
    let mut effective_config = config.clone();
    effective_config.world.master_seed = run_context.master_seed;
    effective_config.world.replicate_id = run_context.replicate_id;
    phase1_resource_regrowth(&mut world, &effective_config);
    phase2_biological_degradation(&mut world, &effective_config);

    let mut features = Vec::with_capacity(world.agents.len());
    phase3_observation_and_features_into(&world, &effective_config, &mut features).unwrap();
    let mut choices = Vec::with_capacity(world.agents.len());
    phase4_primary_action_selection_into(&world, &effective_config, &features, &mut choices)
        .unwrap();
    let mut intents = Vec::with_capacity(world.agents.len());
    generate_intents_m3_into(
        &world,
        &effective_config,
        &choices,
        reference.relationship_graph(),
        &mut intents,
    )
    .unwrap();
    let output_intents = intents.clone();
    let partitions = phase5_partition_intents_baseline(&intents).unwrap();
    let work = phase6a_work_resolution(&mut world, &partitions).unwrap();
    let targeted =
        phase6b_targeted_resolution_m3(&mut world, &effective_config, &partitions).unwrap();
    PhaseOutputs {
        intents: output_intents,
        work,
        targeted,
    }
}

fn phase_outputs_storage(
    hybrid_world: &HybridWorldState,
    graph: &sim_model::RelationshipGraph,
    config: &SimConfig,
) -> PhaseOutputs {
    let mut hybrid_world = hybrid_world.clone();
    let HybridWorldState {
        world,
        segmented_storage,
        authority_mode: _,
    } = &mut hybrid_world;
    let storage = segmented_storage.as_mut().unwrap();
    phase1_resource_regrowth(world, config);
    storage.phase2_degradation_with_config(config);

    let mut phase3_scratch = Phase3ScarcityScratch::with_capacity(world.settlements.len());
    let mut features = Vec::with_capacity(storage.len());
    phase3_observation_and_features_storage_with_scratch(
        storage,
        &world.settlements,
        config,
        &mut features,
        &mut phase3_scratch,
    )
    .unwrap();
    let mut choices = Vec::with_capacity(storage.len());
    phase4_primary_action_selection_storage_into(
        storage,
        world.current_day,
        config,
        &features,
        &mut choices,
    )
    .unwrap();
    let projection = M3Phase4SlotProjection::new(graph, storage).unwrap();
    let mut candidate_scratch = Vec::with_capacity(16);
    let mut intents = Vec::with_capacity(storage.len());
    generate_intents_m3_storage_into(
        storage,
        world.current_day,
        config,
        &choices,
        &projection,
        &mut candidate_scratch,
        &mut intents,
    )
    .unwrap();
    let output_intents = intents.clone();
    let mut phase5_scratch = Phase5PartitionScratch::with_capacity(world.settlements.len());
    let partitions =
        phase5_partition_intents_from_vec_with_scratch(&mut intents, &mut phase5_scratch).unwrap();
    let work =
        phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions).unwrap();
    let targeted = phase6b_targeted_resolution_storage_m3_with_scratch(
        storage,
        &world.settlements,
        world.current_day,
        config,
        &partitions,
        &mut M3Phase6BStorageScratch::default(),
    )
    .unwrap();
    PhaseOutputs {
        intents: output_intents,
        work,
        targeted,
    }
}

#[derive(Debug, PartialEq)]
struct DayParity {
    state: CanonicalHash,
    metrics: Option<M3pDailyMetrics>,
    events: Vec<sim_model::M3pEventRecord>,
    snapshot: Option<Vec<u8>>,
}

fn record_parity(
    reference: &M3pReferenceState,
    reference_outcome: M3pDayOutcome,
    hybrid_world: &HybridWorldState,
    storage_context: &M3pStorageRunContext,
    storage_outcome: M3pDayOutcome,
) -> DayParity {
    assert_eq!(reference_outcome.executed_day, storage_outcome.executed_day);
    assert_eq!(reference_outcome.metrics, storage_outcome.metrics);
    assert_eq!(reference_outcome.events, storage_outcome.events);
    let reference_metrics_digest = reference_outcome
        .metrics
        .as_ref()
        .map(|metric| canonical_m3p_metrics_hash(std::slice::from_ref(metric)).unwrap());
    let storage_metrics_digest = storage_outcome
        .metrics
        .as_ref()
        .map(|metric| canonical_m3p_metrics_hash(std::slice::from_ref(metric)).unwrap());
    assert_eq!(reference_metrics_digest, storage_metrics_digest);
    let reference_events_digest = canonical_m3p_event_hash(&reference_outcome.events).unwrap();
    let storage_events_digest = canonical_m3p_event_hash(&storage_outcome.events).unwrap();
    assert_eq!(reference_events_digest, storage_events_digest);
    let reference_snapshot = reference_outcome.snapshot.map(|snapshot| snapshot.bytes);
    let storage_snapshot = storage_outcome.snapshot.map(|snapshot| snapshot.bytes);
    assert_eq!(reference_snapshot, storage_snapshot);
    let storage = hybrid_world.storage().unwrap();
    let state_hash = canonical_m3p_state_hash_from_storage(
        storage,
        hybrid_world.world.current_day,
        &hybrid_world.world.settlements,
        storage_context.relationship_graph(),
        storage_context.next_agent_id(),
    )
    .unwrap();
    let reference_hash = canonical_m3p_state_hash(reference).unwrap();
    assert_eq!(reference_hash, state_hash);
    DayParity {
        state: state_hash,
        metrics: storage_outcome.metrics,
        events: storage_outcome.events,
        snapshot: storage_snapshot,
    }
}

fn advance_pair(
    reference: &mut M3pReferenceState,
    hybrid_world: &mut HybridWorldState,
    storage_context: &mut M3pStorageRunContext,
    config: &SimConfig,
    snapshot_boundary: bool,
) -> (DayParity, M3pDayOutcome) {
    let run_context = context(config);
    let reference_phases = phase_outputs_reference(reference, config, &run_context);
    let storage_phases =
        phase_outputs_storage(hybrid_world, storage_context.relationship_graph(), config);
    assert_eq!(reference_phases, storage_phases);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    };
    let reference_outcome =
        run_m3p_reference_day(reference, config, &run_context, &options).unwrap();
    let storage_outcome = run_m3p_storage_days(hybrid_world, config, storage_context, 1, &options)
        .unwrap()
        .pop()
        .unwrap();
    let parity = record_parity(
        reference,
        reference_outcome.clone(),
        hybrid_world,
        storage_context,
        storage_outcome,
    );
    (parity, reference_outcome)
}

fn compare_days(config: &SimConfig, days: u32, reverse_initial_slots: bool) {
    let mut reference = state(config);
    let run_context = context(config);
    let mut hybrid_world = HybridWorldState::hybrid(reference.world().clone());
    if reverse_initial_slots {
        let mut agents = reference.world().agents.clone();
        agents.reverse();
        hybrid_world.segmented_storage = Some(SegmentedAgentStorage::from_agents(&agents));
    }
    let mut storage_context =
        M3pStorageRunContext::new(&hybrid_world, config, &run_context).unwrap();
    assert_eq!(
        reference.relationship_graph().csr_hash(),
        storage_context.relationship_graph().csr_hash()
    );

    for _ in 0..days {
        let next_day = hybrid_world.world.current_day.as_u32() + 1;
        let checkpoint = [100, 250, 500].contains(&next_day);
        let (parity, _) = advance_pair(
            &mut reference,
            &mut hybrid_world,
            &mut storage_context,
            config,
            checkpoint,
        );
        assert!(parity.state != CanonicalHash::from_bytes([0; 32]));
        if checkpoint {
            assert!(parity.snapshot.is_some());
        }
    }
}

fn run_matrix_slice(seed: u64, settlements: u32) {
    let started = Instant::now();
    for degree in [4, 8, 16] {
        for p_local in [0.0, 0.5, 1.0] {
            let config = matrix_config(seed, settlements, degree, p_local);
            compare_days(&config, 300, false);
        }
    }
    println!(
        "M3P storage matrix slice seed={seed} K={settlements}: 6 cells x 300 days, {:?}",
        started.elapsed()
    );
}

#[test]
fn m3p_storage_matrix_seed0_k3() {
    run_matrix_slice(0, 3);
}

#[test]
fn m3p_storage_matrix_seed0_k10() {
    run_matrix_slice(0, 10);
}

#[test]
fn m3p_storage_matrix_seed8198_k3() {
    run_matrix_slice(81985529216486895, 3);
}

#[test]
fn m3p_storage_matrix_seed8198_k10() {
    run_matrix_slice(81985529216486895, 10);
}

#[test]
fn m3p_storage_golden_configs_and_shuffled_slots_match_reference() {
    let main = config();
    compare_days(&main, 500, false);
    compare_days(&main, 300, true);
    compare_days(&edge_drop_config(), 500, false);
}

#[test]
fn m3p_storage_snapshot_pause_resume_rebuilds_graph_and_projection_at_compaction() {
    let config = config();
    let run_context = context(&config);
    let mut uninterrupted = state(&config);
    let mut paused = state(&config);
    let mut hybrid_world = HybridWorldState::hybrid(paused.world().clone());
    let mut storage_context =
        M3pStorageRunContext::new(&hybrid_world, &config, &run_context).unwrap();
    let mut compaction_snapshot = None;

    for day in 0..=50 {
        let boundary = day == 50;
        let (parity, paused_outcome) = advance_pair(
            &mut paused,
            &mut hybrid_world,
            &mut storage_context,
            &config,
            boundary,
        );
        let uninterrupted_outcome = run_m3p_reference_day(
            &mut uninterrupted,
            &config,
            &run_context,
            &DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: boundary,
            },
        )
        .unwrap();
        assert_eq!(paused_outcome, uninterrupted_outcome);
        if boundary {
            assert!(
                parity
                    .events
                    .iter()
                    .any(|record| matches!(&record.event, sim_model::M3pEvent::Cleanup { .. }))
            );
            compaction_snapshot = parity.snapshot;
        }
    }

    let restored = decode_m3p_snapshot(compaction_snapshot.as_deref().unwrap(), &config).unwrap();
    let mut resumed = restored.state;
    assert_eq!(
        canonical_m3p_state_hash(&resumed).unwrap(),
        canonical_m3p_state_hash(&uninterrupted).unwrap()
    );
    let mut resumed_hybrid = HybridWorldState::hybrid(resumed.world().clone());
    let mut resumed_context = M3pStorageRunContext::from_reference_state(
        &resumed_hybrid,
        &config,
        &run_context,
        &resumed,
    )
    .unwrap();

    for _ in 0..60 {
        let boundary = [100, 250, 500].contains(&(resumed.world().current_day.as_u32() + 1));
        let (resumed_parity, resumed_outcome) = advance_pair(
            &mut resumed,
            &mut resumed_hybrid,
            &mut resumed_context,
            &config,
            boundary,
        );
        let uninterrupted_outcome = run_m3p_reference_day(
            &mut uninterrupted,
            &config,
            &run_context,
            &DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: boundary,
            },
        )
        .unwrap();
        assert_eq!(resumed_outcome, uninterrupted_outcome);
        assert_eq!(
            canonical_m3p_state_hash(&resumed).unwrap(),
            canonical_m3p_state_hash(&uninterrupted).unwrap()
        );
        if boundary {
            assert!(resumed_parity.snapshot.is_some());
        }
    }
}

#[test]
fn m3p_storage_runner_matches_reference_id_exhaustion_error() {
    let mut config = config();
    config.population.as_mut().unwrap().birth_probability = 1.0;
    let run_context = context(&config);
    let source = state(&config);
    let exhausted = M3pReferenceState::from_parts(
        source.world().clone(),
        source.relationship_graph().clone(),
        u32::MAX as u64 + 1,
        &config,
        &run_context,
    )
    .unwrap();
    let mut reference = exhausted.clone();
    let mut hybrid_world = HybridWorldState::hybrid(exhausted.world().clone());
    let mut storage_context = M3pStorageRunContext::from_reference_state(
        &hybrid_world,
        &config,
        &run_context,
        &exhausted,
    )
    .unwrap();
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let reference_error =
        run_m3p_reference_day(&mut reference, &config, &run_context, &options).unwrap_err();
    let storage_error = run_m3p_storage_days(
        &mut hybrid_world,
        &config,
        &mut storage_context,
        1,
        &options,
    )
    .unwrap_err();
    assert_eq!(reference_error, storage_error);
}

#[test]
fn m3p_storage_state_digest_matches_reference_for_shuffled_slots_and_mutated_graph() {
    let config = config();
    let run_context = context(&config);
    let mut reference = state(&config);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    for _ in 0..60 {
        run_m3p_reference_day(&mut reference, &config, &run_context, &options)
            .expect("reference day executes");
    }

    let world = reference.world();
    let mut shuffled_agents = world.agents.clone();
    shuffled_agents.reverse();
    let storage = SegmentedAgentStorage::from_agents(&shuffled_agents);
    let storage_world = snapshot_world(world, &storage);
    let reference_bytes = canonical_m3p_state_bytes(&reference).unwrap();
    let storage_bytes = canonical_m3p_state_bytes_from_storage(
        &storage,
        storage_world.current_day,
        &storage_world.settlements,
        reference.relationship_graph(),
        reference.next_agent_id(),
    )
    .unwrap();
    assert_eq!(reference_bytes, storage_bytes);
    assert_eq!(
        canonical_m3p_state_hash(&reference).unwrap(),
        canonical_m3p_state_hash_from_storage(
            &storage,
            storage_world.current_day,
            &storage_world.settlements,
            reference.relationship_graph(),
            reference.next_agent_id(),
        )
        .unwrap()
    );
}
