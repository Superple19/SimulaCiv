use sim_core::{AgentId, GroupId};
use sim_model::hashing::{
    canonical_m3m_event_hash, canonical_m3m_metrics_hash, canonical_m3m_state_hash,
    canonical_m3m_state_hash_from_storage, canonical_m3mprop_event_hash,
    canonical_m3mprop_metrics_hash, canonical_m3mprop_state_hash,
    canonical_m3mprop_state_hash_from_storage, canonical_m3pm_event_hash,
    canonical_m3pm_metrics_hash, canonical_m3pm_state_hash, canonical_m3pm_state_hash_from_storage,
    canonical_m3pmprop_event_hash, canonical_m3pmprop_metrics_hash, canonical_m3pmprop_state_hash,
    canonical_m3pmprop_state_hash_from_storage,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3PropagationReferenceState, M3PropagationStorageRunContext,
    M3ReferenceContext, M3StorageRunContext, M3pPropagationReferenceState,
    M3pPropagationStorageRunContext, M3pReferenceState, M3pStorageRunContext, MigrationConfig,
    SimConfig, decode_m3p_snapshot, decode_migration_snapshot, initialize_world,
    run_m3_reference_day, run_m3p_propagation_storage_days, run_m3p_reference_day,
    run_m3p_storage_days, run_m3prop_reference_day, run_m3prop_storage_days,
};

const BASE: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 60
settlement_count = 3
initial_health = 1.0
initial_food = 20.0
initial_wealth = 10000
initial_settlement_resource = 1000000.0
initial_treasury = 1000

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
carrying_capacity = 3000000.0
regrowth_rate = 0.0
base_metabolic_cost = 0.1
health_decay_rate = 0.0

[economy]
base_work_yield = 0.0
food_price = 100
target_food = 20.0
target_reserve = 1
tax_rate = 0.0
welfare_payment = 0

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0
relationship_degree_k = 8
p_local = 0.5

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 100.0]
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

const M3M_GOLDENS: [(u32, &str, &str, &str); 2] = [
    (
        1,
        "064db4c0c8e2acefdb6f2cbc76f7d9dc2a03ff30dfa36fbaf2e8d95abcd99e3c",
        "0244744aeb6b994ce25b2e77a3c804043cf20002c214588845e6a615f541e451",
        "388fddfe5261628907bcee4653dbdbfc1cc86c25f9e09a77a7465957e590cf36",
    ),
    (
        2,
        "47552438c792724d1534fe3b84c2970f3ce97d3894c4a1416bae0ad6635f24fa",
        "404fba0ba82e0cbf283fde525302f92c1efb19a4dfffbde91f83123361236db2",
        "0c6cf42491b22fbbf9f8461272f1fb0a3da344457f3f47b3b804bfe24523fe0d",
    ),
];

fn config(profile: &str, propagation: bool) -> SimConfig {
    let population_dynamics = profile == "m3p";
    let source = format!(
        "profile = \"{profile}\"\n\n[simulation.systems]\nrelationship_graph = true\npopulation_dynamics = {population_dynamics}\npropagation = {propagation}\nmigration = true\n\n{BASE}\n\n[migration]\nmigration_probability = 1.0\nmigration_threshold = 0.0\nmigration_cost = 10\nmigration_cooldown_days = 3\n{}{}",
        if population_dynamics {
            "\n[population]\nbirth_min_health = 0.0\nbirth_food_threshold = 0.0\nbirth_probability = 0.2\nbirth_food_cost = 0.0\nbirths_max_per_day = 2\ncompaction_interval_days = 2\n"
        } else {
            ""
        },
        if propagation {
            "\n[propagation]\nseed_probability = 0.1\ntransmit_probability = 0.2\nforget_probability = 0.0\nmax_hops = 4\n"
        } else {
            ""
        }
    );
    SimConfig::parse_and_validate(&source).expect("migration test config validates")
}

fn run_context(config: &SimConfig) -> M0RunContext {
    M0RunContext::new(config.world.master_seed, config.world.replicate_id)
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn pressured_world(config: &SimConfig) -> sim_model::WorldState {
    let mut world = initialize_world(config).expect("world initializes");
    for settlement in &mut world.settlements {
        settlement.resource = match settlement.group_id.0 {
            0 => 1.0,
            1 => 1_000_000.0,
            _ => 2_000_000.0,
        };
    }
    world
}

fn total_money(world: &sim_model::WorldState) -> i64 {
    world.agents.iter().map(|agent| agent.wealth).sum::<i64>()
        + world
            .settlements
            .iter()
            .map(|settlement| settlement.treasury)
            .sum::<i64>()
}

#[test]
fn m3_migration_reference_storage_parity_conservation_and_digest() {
    let config = config("m3", false);
    let initial = pressured_world(&config);
    let initial_money = total_money(&initial);
    let run_context = run_context(&config);
    let mut reference_world = initial.clone();
    let reference_context = M3ReferenceContext::new(&initial, &config, &run_context).unwrap();
    let mut storage_world = HybridWorldState::hybrid(initial);
    let mut storage_context =
        M3StorageRunContext::new(&storage_world, &config, &run_context).unwrap();
    let mut reference = None;
    let mut storage = None;
    for _ in 0..2 {
        reference = Some(
            run_m3_reference_day(
                &mut reference_world,
                &config,
                &reference_context,
                &options(true),
            )
            .unwrap(),
        );
        storage = Some(run_m3_reference_day_storage(
            &mut storage_world,
            &config,
            &mut storage_context,
        ));
        let r = reference.as_ref().unwrap();
        let s = storage.as_ref().unwrap();
        assert_eq!(
            r.metrics, s.metrics,
            "daily metrics differ on {}",
            r.executed_day
        );
        assert_eq!(r.events, s.events, "events differ on {}", r.executed_day);
        assert_eq!(
            r.migration, s.migration,
            "migration differs on {}",
            r.executed_day
        );
        assert_eq!(
            r.snapshot, s.snapshot,
            "snapshot differs on {}",
            r.executed_day
        );
    }
    let reference = reference.unwrap();
    let storage = storage.unwrap();
    let migration = reference.migration.as_ref().unwrap();
    assert!(migration.metrics.unwrap().migrations > 0);
    let storage_state = storage_world.segmented_storage.as_ref().unwrap();
    assert_eq!(
        initial_money,
        storage_state.wealth().iter().sum::<i64>()
            + storage_world
                .world
                .settlements
                .iter()
                .map(|settlement| settlement.treasury)
                .sum::<i64>()
    );
    let reference_hash = canonical_m3m_state_hash(
        &reference_world,
        reference_context.relationship_graph(),
        &reference_context.migration_state().unwrap(),
    )
    .unwrap();
    let storage_hash = canonical_m3m_state_hash_from_storage(
        storage_state,
        storage_world.world.current_day,
        &storage_world.world.settlements,
        storage_context.relationship_graph(),
        storage_context.migration_state().unwrap(),
    )
    .unwrap();
    assert_eq!(reference_hash, storage_hash);
    assert_eq!(
        canonical_m3m_metrics_hash(
            &[reference.metrics.as_ref().unwrap().clone()],
            &[migration.metrics.unwrap()]
        )
        .unwrap(),
        canonical_m3m_metrics_hash(
            &[storage.metrics.as_ref().unwrap().clone()],
            &[storage.migration.as_ref().unwrap().metrics.unwrap()]
        )
        .unwrap()
    );
    assert_eq!(
        canonical_m3m_event_hash(&reference.events, &migration.events).unwrap(),
        canonical_m3m_event_hash(&storage.events, &storage.migration.as_ref().unwrap().events)
            .unwrap()
    );
}

#[test]
fn m4_06_m3m_golden_state_metrics_and_events() {
    let config = config("m3", false);
    let mut world = pressured_world(&config);
    let run_context = run_context(&config);
    let context = M3ReferenceContext::new(&world, &config, &run_context).unwrap();
    let mut metrics = Vec::new();
    let mut migration_metrics = Vec::new();
    let mut events = Vec::new();
    let mut migration_events = Vec::new();
    for expected in &M3M_GOLDENS {
        let outcome = run_m3_reference_day(&mut world, &config, &context, &options(true)).unwrap();
        metrics.push(outcome.metrics.unwrap());
        let migration = outcome.migration.unwrap();
        migration_metrics.push(migration.metrics.unwrap());
        events.extend(outcome.events);
        migration_events.extend(migration.events);
        assert_eq!(outcome.executed_day + 1, expected.0);
        assert_eq!(
            canonical_m3m_state_hash(
                &world,
                context.relationship_graph(),
                &context.migration_state().unwrap(),
            )
            .unwrap()
            .to_hex(),
            expected.1,
            "M4-06 M3M State golden at day {}",
            expected.0
        );
        assert_eq!(
            canonical_m3m_metrics_hash(&metrics, &migration_metrics)
                .unwrap()
                .to_hex(),
            expected.2,
            "M4-06 M3M Metrics golden at day {}",
            expected.0
        );
        assert_eq!(
            canonical_m3m_event_hash(&events, &migration_events)
                .unwrap()
                .to_hex(),
            expected.3,
            "M4-06 M3M Events golden at day {}",
            expected.0
        );
    }
}

#[test]
fn migration_event_group_ids_use_frozen_u32_little_endian_widths() {
    let bytes = sim_model::hashing::canonical_m3m_event_bytes(
        &[],
        &[sim_model::MigrationEventRecord {
            day: 7,
            local_sequence: 1_u64 << 63,
            agent_id: AgentId(0x1122_3344),
            from_group_id: GroupId(0x1234),
            to_group_id: GroupId(0x5678),
            cost: 9,
        }],
    )
    .unwrap();
    assert_eq!(
        &bytes[bytes.len() - 20..],
        &[
            0x44, 0x33, 0x22, 0x11, // AgentId u32 LE
            0x34, 0x12, 0x00, 0x00, // from GroupId u32 LE
            0x78, 0x56, 0x00, 0x00, // to GroupId u32 LE
            0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // Money i64 LE
        ]
    );
}

fn run_m3_reference_day_storage(
    world: &mut HybridWorldState,
    config: &SimConfig,
    context: &mut M3StorageRunContext,
) -> sim_model::M3DayOutcome {
    sim_model::run_m3_storage_days(world, config, context, 1, &options(true))
        .unwrap()
        .remove(0)
}

#[test]
fn migration_composes_with_m3p_and_propagation_storage_paths() {
    for (profile, propagation) in [("m3", true), ("m3p", false), ("m3p", true)] {
        let config = config(profile, propagation);
        let initial = pressured_world(&config);
        let run_context = run_context(&config);
        let mut storage_world = HybridWorldState::hybrid(initial.clone());

        if profile == "m3p" && propagation {
            let mut reference =
                M3pPropagationReferenceState::new(initial, &config, &run_context).unwrap();
            let mut storage_context =
                M3pPropagationStorageRunContext::new(&storage_world, &config, &run_context)
                    .unwrap();
            for _ in 0..3 {
                let reference_outcome = sim_model::run_m3pprop_reference_day(
                    &mut reference,
                    &config,
                    &run_context,
                    &options(true),
                )
                .unwrap();
                let storage_outcome = run_m3p_propagation_storage_days(
                    &mut storage_world,
                    &config,
                    &mut storage_context,
                    1,
                    &options(true),
                )
                .unwrap()
                .remove(0);
                assert_eq!(reference_outcome, storage_outcome);
                assert!(
                    reference_outcome
                        .migration
                        .as_ref()
                        .unwrap()
                        .metrics
                        .is_some()
                );
                let migration = reference_outcome.migration.as_ref().unwrap();
                let storage_state = storage_world.segmented_storage.as_ref().unwrap();
                assert_eq!(
                    canonical_m3pmprop_state_hash(&reference).unwrap(),
                    canonical_m3pmprop_state_hash_from_storage(
                        storage_state,
                        storage_world.world.current_day,
                        &storage_world.world.settlements,
                        storage_context.relationship_graph(),
                        storage_context.next_agent_id(),
                        storage_context.propagation(),
                        storage_context.migration_state().unwrap(),
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3pmprop_metrics_hash(
                        &[reference_outcome.metrics.as_ref().unwrap().clone()],
                        &[migration.metrics.unwrap()],
                    )
                    .unwrap(),
                    canonical_m3pmprop_metrics_hash(
                        &[storage_outcome.metrics.as_ref().unwrap().clone()],
                        &[storage_outcome.migration.as_ref().unwrap().metrics.unwrap()],
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3pmprop_event_hash(&reference_outcome.events, &migration.events,)
                        .unwrap(),
                    canonical_m3pmprop_event_hash(
                        &storage_outcome.events,
                        &storage_outcome.migration.as_ref().unwrap().events,
                    )
                    .unwrap()
                );
            }
        } else if profile == "m3p" {
            let mut reference = M3pReferenceState::new(initial, &config, &run_context).unwrap();
            let mut storage_context =
                M3pStorageRunContext::new(&storage_world, &config, &run_context).unwrap();
            let mut births = 0;
            for _ in 0..6 {
                let reference_outcome =
                    run_m3p_reference_day(&mut reference, &config, &run_context, &options(true))
                        .unwrap();
                let storage_outcome = run_m3p_storage_days(
                    &mut storage_world,
                    &config,
                    &mut storage_context,
                    1,
                    &options(true),
                )
                .unwrap()
                .remove(0);
                assert_eq!(reference_outcome, storage_outcome);
                assert!(
                    reference_outcome
                        .migration
                        .as_ref()
                        .unwrap()
                        .metrics
                        .is_some()
                );
                births += reference_outcome.metrics.as_ref().unwrap().births;
                let migration = reference_outcome.migration.as_ref().unwrap();
                let storage_state = storage_world.segmented_storage.as_ref().unwrap();
                assert_eq!(
                    canonical_m3pm_state_hash(&reference).unwrap(),
                    canonical_m3pm_state_hash_from_storage(
                        storage_state,
                        storage_world.world.current_day,
                        &storage_world.world.settlements,
                        storage_context.relationship_graph(),
                        storage_context.next_agent_id(),
                        storage_context.migration_state().unwrap(),
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3pm_metrics_hash(
                        &[reference_outcome.metrics.as_ref().unwrap().clone()],
                        &[migration.metrics.unwrap()],
                    )
                    .unwrap(),
                    canonical_m3pm_metrics_hash(
                        &[storage_outcome.metrics.as_ref().unwrap().clone()],
                        &[storage_outcome.migration.as_ref().unwrap().metrics.unwrap()],
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3pm_event_hash(&reference_outcome.events, &migration.events,)
                        .unwrap(),
                    canonical_m3pm_event_hash(
                        &storage_outcome.events,
                        &storage_outcome.migration.as_ref().unwrap().events,
                    )
                    .unwrap()
                );
            }
            assert!(births > 0, "M3P composition should exercise Phase 9b");
        } else {
            let mut reference =
                M3PropagationReferenceState::new(initial, &config, &run_context).unwrap();
            let mut storage_context =
                M3PropagationStorageRunContext::new(&storage_world, &config, &run_context).unwrap();
            for index in 0..3 {
                let reference_outcome =
                    run_m3prop_reference_day(&mut reference, &config, &run_context, &options(true))
                        .unwrap();
                let storage_result = run_m3prop_storage_days(
                    &mut storage_world,
                    &config,
                    &mut storage_context,
                    1,
                    &options(true),
                );
                let storage_outcome = storage_result
                    .unwrap_or_else(|error| {
                        panic!("M3 propagation storage iteration {index}: {error}")
                    })
                    .remove(0);
                assert_eq!(reference_outcome, storage_outcome);
                assert!(
                    reference_outcome
                        .migration
                        .as_ref()
                        .unwrap()
                        .metrics
                        .is_some()
                );
                let migration = reference_outcome.migration.as_ref().unwrap();
                let storage_state = storage_world.segmented_storage.as_ref().unwrap();
                assert_eq!(
                    canonical_m3mprop_state_hash(&reference).unwrap(),
                    canonical_m3mprop_state_hash_from_storage(
                        storage_state,
                        storage_world.world.current_day,
                        &storage_world.world.settlements,
                        storage_context.relationship_graph(),
                        storage_context.propagation(),
                        storage_context.migration_state().unwrap(),
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3mprop_metrics_hash(
                        &[reference_outcome.metrics.as_ref().unwrap().clone()],
                        &[migration.metrics.unwrap()],
                    )
                    .unwrap(),
                    canonical_m3mprop_metrics_hash(
                        &[storage_outcome.metrics.as_ref().unwrap().clone()],
                        &[storage_outcome.migration.as_ref().unwrap().metrics.unwrap()],
                    )
                    .unwrap()
                );
                assert_eq!(
                    canonical_m3mprop_event_hash(&reference_outcome.events, &migration.events,)
                        .unwrap(),
                    canonical_m3mprop_event_hash(
                        &storage_outcome.events,
                        &storage_outcome.migration.as_ref().unwrap().events,
                    )
                    .unwrap()
                );
            }
        }
    }
}

#[test]
fn migration_snapshot_v4_resume_preserves_m3p_graph_and_cooldown_state() {
    let config = config("m3p", false);
    let initial = pressured_world(&config);
    let run_context = run_context(&config);
    let mut uninterrupted_world = HybridWorldState::hybrid(initial.clone());
    let mut uninterrupted_context =
        M3pStorageRunContext::new(&uninterrupted_world, &config, &run_context).unwrap();
    let uninterrupted = run_m3p_storage_days(
        &mut uninterrupted_world,
        &config,
        &mut uninterrupted_context,
        4,
        &options(true),
    )
    .unwrap();
    let births = uninterrupted
        .iter()
        .map(|outcome| outcome.metrics.as_ref().unwrap().births)
        .sum::<u64>();
    assert!(births > 0, "the resume case must exercise newborn IDs");
    assert!(
        uninterrupted.iter().any(|outcome| outcome
            .events
            .iter()
            .any(|record| { matches!(&record.event, sim_model::M3pEvent::Cleanup { .. }) })),
        "the resume boundary must include a compaction day"
    );

    let mut paused_world = HybridWorldState::hybrid(initial);
    let mut paused_context =
        M3pStorageRunContext::new(&paused_world, &config, &run_context).unwrap();
    let paused = run_m3p_storage_days(
        &mut paused_world,
        &config,
        &mut paused_context,
        2,
        &options(true),
    )
    .unwrap();
    let snapshot = paused.last().unwrap().snapshot.as_ref().unwrap();
    let restored = decode_migration_snapshot(snapshot).unwrap();
    assert_eq!(
        restored.profile as u8,
        sim_model::MigrationProfileTag::M3pm as u8
    );
    let mut base = decode_m3p_snapshot(&restored.base_snapshot, &config)
        .unwrap()
        .state;
    base.set_migration_state(restored.migration_state.clone());
    assert_eq!(
        base.relationship_graph().csr_hash(),
        paused_context.relationship_graph().csr_hash()
    );
    assert_eq!(base.migration_state(), Some(&restored.migration_state));
    let resumed_world = HybridWorldState::hybrid(base.world().clone());
    let resumed_context =
        M3pStorageRunContext::from_reference_state(&resumed_world, &config, &run_context, &base)
            .unwrap();
    assert_eq!(
        resumed_context.relationship_graph().csr_hash(),
        paused_context.relationship_graph().csr_hash()
    );
    let mut resumed_world = resumed_world;
    let mut resumed_context = resumed_context;
    let resumed = run_m3p_storage_days(
        &mut resumed_world,
        &config,
        &mut resumed_context,
        2,
        &options(true),
    )
    .unwrap();
    assert_eq!(resumed, uninterrupted[2..]);
}

#[test]
fn migration_storage_plan_is_independent_of_dense_slot_order() {
    let config = config("m3", false);
    let world = pressured_world(&config);
    let mut reversed = world.clone();
    reversed.agents.reverse();
    let run_context = run_context(&config);
    let mut ordered = HybridWorldState::hybrid(world.clone());
    let mut shuffled = HybridWorldState::hybrid(reversed);
    let mut ordered_context = M3StorageRunContext::new(&ordered, &config, &run_context).unwrap();
    let mut shuffled_context = M3StorageRunContext::new(&shuffled, &config, &run_context).unwrap();
    let ordered_outcome = run_m3_reference_day_storage(&mut ordered, &config, &mut ordered_context);
    let shuffled_outcome =
        run_m3_reference_day_storage(&mut shuffled, &config, &mut shuffled_context);
    assert_eq!(ordered_outcome, shuffled_outcome);
    assert_eq!(
        canonical_m3m_state_hash_from_storage(
            ordered.segmented_storage.as_ref().unwrap(),
            ordered.world.current_day,
            &ordered.world.settlements,
            ordered_context.relationship_graph(),
            ordered_context.migration_state().unwrap(),
        )
        .unwrap(),
        canonical_m3m_state_hash_from_storage(
            shuffled.segmented_storage.as_ref().unwrap(),
            shuffled.world.current_day,
            &shuffled.world.settlements,
            shuffled_context.relationship_graph(),
            shuffled_context.migration_state().unwrap(),
        )
        .unwrap()
    );
}

#[test]
fn migration_config_rejects_settings_without_switch_and_invalid_probability() {
    let migration = MigrationConfig {
        migration_probability: f32::NAN,
        migration_threshold: 0.0,
        migration_cost: 0,
        migration_cooldown_days: 0,
    };
    let mut invalid = config("m3", true);
    invalid.migration = Some(migration);
    assert!(invalid.validate().is_err());

    let without_switch = BASE.to_owned()
        + "\n[migration]\nmigration_probability = 0.5\nmigration_threshold = 0.0\nmigration_cost = 0\nmigration_cooldown_days = 0\n";
    assert!(SimConfig::parse_and_validate(&without_switch).is_err());
}

#[test]
fn migration_draws_are_repeatable_and_cooldown_blocks_recent_movers() {
    let mut config = config("m3", false);
    let mut migration_config = config.migration.unwrap();
    migration_config.migration_probability = 0.5;
    config.enable_migration(migration_config);

    let initial = pressured_world(&config);
    let run_context = run_context(&config);
    let mut first_world = initial.clone();
    let mut second_world = initial.clone();
    let first_context = M3ReferenceContext::new(&initial, &config, &run_context).unwrap();
    let second_context = M3ReferenceContext::new(&initial, &config, &run_context).unwrap();
    let mut first_day_one_moves = Vec::new();
    for day in 0..3 {
        let first =
            run_m3_reference_day(&mut first_world, &config, &first_context, &options(false))
                .unwrap();
        let second =
            run_m3_reference_day(&mut second_world, &config, &second_context, &options(false))
                .unwrap();
        assert_eq!(first.migration, second.migration);
        if day == 1 {
            first_day_one_moves = first
                .migration
                .as_ref()
                .unwrap()
                .events
                .iter()
                .map(|event| event.agent_id)
                .collect();
        }
        if day == 2 {
            let day_two_moves = first
                .migration
                .as_ref()
                .unwrap()
                .events
                .iter()
                .map(|event| event.agent_id)
                .collect::<Vec<_>>();
            assert!(
                first_day_one_moves
                    .iter()
                    .all(|agent_id| !day_two_moves.contains(agent_id)),
                "a day-1 migrant moved again before the 3-day cooldown elapsed"
            );
        }
    }
}

#[test]
fn ineligible_agent_does_not_shift_other_agents_migration_draws() {
    let mut config = config("m3", false);
    let mut migration_config = config.migration.unwrap();
    migration_config.migration_probability = 0.5;
    config.enable_migration(migration_config);
    let initial = pressured_world(&config);
    let run_context = run_context(&config);
    let baseline_context = M3ReferenceContext::new(&initial, &config, &run_context).unwrap();
    let target = initial
        .agents
        .iter()
        .find(|agent| {
            agent.group_id.0 == 0
                && baseline_context
                    .relationship_graph()
                    .neighbors_of(agent.agent_id)
                    .unwrap()
                    .iter()
                    .any(|neighbor| {
                        initial.agents.iter().any(|candidate| {
                            candidate.agent_id == *neighbor && candidate.group_id.0 == 2
                        })
                    })
        })
        .unwrap()
        .agent_id;
    let mut ineligible_initial = initial.clone();
    ineligible_initial
        .agents
        .iter_mut()
        .find(|agent| agent.agent_id == target)
        .unwrap()
        .wealth = 0;
    let ineligible_context =
        M3ReferenceContext::new(&ineligible_initial, &config, &run_context).unwrap();
    let mut baseline_world = initial;
    let mut ineligible_world = ineligible_initial;
    let mut baseline_day_one = Vec::new();
    let mut ineligible_day_one = Vec::new();
    for day in 0..2 {
        let baseline = run_m3_reference_day(
            &mut baseline_world,
            &config,
            &baseline_context,
            &options(false),
        )
        .unwrap();
        let ineligible = run_m3_reference_day(
            &mut ineligible_world,
            &config,
            &ineligible_context,
            &options(false),
        )
        .unwrap();
        if day == 1 {
            baseline_day_one = baseline
                .migration
                .unwrap()
                .events
                .into_iter()
                .filter(|event| event.agent_id != target)
                .collect();
            ineligible_day_one = ineligible
                .migration
                .unwrap()
                .events
                .into_iter()
                .filter(|event| event.agent_id != target)
                .collect();
        }
    }
    assert_eq!(baseline_day_one, ineligible_day_one);
}

#[test]
fn migration_plan_failure_does_not_partially_commit_money_or_membership() {
    let config = config("m3", false);
    let run_context = run_context(&config);
    let mut reference_world = pressured_world(&config);
    reference_world.settlements[0].treasury = 0;
    reference_world.settlements[1].treasury = 0;
    reference_world.settlements[2].treasury = i64::MAX - 1;
    let mut storage_world = HybridWorldState::hybrid(reference_world.clone());
    let reference_context =
        M3ReferenceContext::new(&reference_world, &config, &run_context).unwrap();
    let mut storage_context =
        M3StorageRunContext::new(&storage_world, &config, &run_context).unwrap();

    let no_observation = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: false,
        snapshot_boundary: false,
    };
    run_m3_reference_day(
        &mut reference_world,
        &config,
        &reference_context,
        &no_observation,
    )
    .unwrap();
    sim_model::run_m3_storage_days(
        &mut storage_world,
        &config,
        &mut storage_context,
        1,
        &no_observation,
    )
    .unwrap();

    let reference_groups_before = reference_world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.group_id, agent.wealth))
        .collect::<Vec<_>>();
    let storage = storage_world.segmented_storage.as_ref().unwrap();
    let storage_groups_before = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .zip(storage.wealth().iter().copied())
        .map(|((agent_id, group), wealth)| (agent_id, group, wealth))
        .collect::<Vec<_>>();
    let reference_state_before = reference_context.migration_state();
    let storage_state_before = storage_context.migration_state().cloned();

    assert!(
        run_m3_reference_day(
            &mut reference_world,
            &config,
            &reference_context,
            &no_observation,
        )
        .is_err()
    );
    assert!(
        sim_model::run_m3_storage_days(
            &mut storage_world,
            &config,
            &mut storage_context,
            1,
            &no_observation,
        )
        .is_err()
    );

    assert_eq!(
        reference_groups_before,
        reference_world
            .agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id, agent.wealth))
            .collect::<Vec<_>>()
    );
    let storage = storage_world.segmented_storage.as_ref().unwrap();
    assert_eq!(
        storage_groups_before,
        storage
            .agent_ids()
            .iter()
            .copied()
            .zip(storage.group_ids().iter().copied())
            .zip(storage.wealth().iter().copied())
            .map(|((agent_id, group), wealth)| (agent_id, group, wealth))
            .collect::<Vec<_>>()
    );
    assert_eq!(reference_state_before, reference_context.migration_state());
    assert_eq!(
        storage_state_before,
        storage_context.migration_state().cloned()
    );
    assert_eq!(reference_world.settlements[2].treasury, i64::MAX - 1);
    assert_eq!(storage_world.world.settlements[2].treasury, i64::MAX - 1);
}
