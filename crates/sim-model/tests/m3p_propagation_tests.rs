use rayon::ThreadPoolBuilder;
use sim_model::hashing::{
    CanonicalHash, canonical_m3pprop_event_hash, canonical_m3pprop_metrics_hash,
    canonical_m3pprop_state_hash, canonical_m3pprop_state_hash_from_storage,
};
use sim_model::m3pprop::{
    M3pPropagationDailyMetrics, M3pPropagationReferenceState, decode_m3pprop_snapshot,
    run_m3pprop_reference_day,
};
use sim_model::propagation::{PropagationAgentState, apply_phase9c_m3p};
use sim_model::{
    ConfiguredDayOutcome, ConfiguredStorageRunContext, DayExecutionOptions, HybridWorldState,
    M0RunContext, M3pEvent, M3pEventRecord, M3pPropagationIncrementalTopologyStorageRunContext,
    M3pPropagationStorageRunContext, PropagationConfig, SimConfig, SimulationRunnerSelection,
    initialize_world, run_configured_storage_days, run_m3p_propagation_storage_days,
    run_m3p_propagation_storage_days_incremental_topology,
    run_m3p_propagation_storage_days_with_parallel_spread,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

const CONFIG: &str = include_str!("fixtures/m3p_propagation_sustained.toml");
const GOLDEN_DAYS: [u32; 3] = [100, 250, 300];
const GOLDENS: [(u32, &str, &str, &str); 3] = [
    (
        100,
        "f55fb8b83dd9f8a55ac3b069c4d8961610f53e26bb0c2f95c1bfe7a7e3ce6f87",
        "ddbb02722aeaa9fd12f01d8fcab1ab8f900c6d4fea69b33a1012087e1374001f",
        "3d26a0d9dfd04fd6968b8db7e491e3d27f8515e71026f69a77129048a52a8322",
    ),
    (
        250,
        "d37e352851577b63fee9f5c5eeba0cf14050bfb080f8386e9d0b1d79105be8dc",
        "609ee14aa776f5826a41a1f02c1ffb9a366c7002a679b273cf8c7ad3cd1c7d92",
        "fed10c2c8fa13c4afa70a11524f49ce8a3857694eec077228cfb30fd7d31c648",
    ),
    (
        300,
        "7449159638c995e448ba369c9b1fe36a9b9ba3ba00f22da0fd6801e3d33801ad",
        "0938e32b51e2fd6633208fceac200650206a24ebdac4a6bfed18e315934a95cf",
        "8e0ed108cf8bc3b98d2d372dcc98d0470c13b0bbb3e9020d9a47b36f25c04551",
    ),
];

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

fn config() -> SimConfig {
    SimConfig::parse_and_validate(CONFIG).expect("M3P propagation config validates")
}

#[test]
fn m3p_propagation_reference_golden_run_records_population_and_spread() {
    let config = config();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("M3P propagation world initializes");
    let mut state = M3pPropagationReferenceState::new(world, &config, &run_context)
        .expect("M3P propagation reference state initializes");
    let mut metrics = Vec::new();
    let mut events = Vec::new();
    let mut snapshots = Vec::new();
    let mut checkpoints = Vec::new();
    let mut cleanup_removed = 0u64;
    for _ in 0..300 {
        let executed_day = state.world().current_day.as_u32();
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: (executed_day + 1).is_multiple_of(50),
        };
        let outcome = run_m3pprop_reference_day(&mut state, &config, &run_context, &options)
            .expect("M3P propagation day runs");
        metrics.push(outcome.metrics.expect("metrics enabled"));
        for event in &outcome.events {
            if let M3pEvent::Cleanup { removed, .. } = &event.event {
                cleanup_removed += removed;
            }
        }
        events.extend(outcome.events);
        if let Some(snapshot) = outcome.snapshot {
            snapshots.push((snapshot.metadata.day, snapshot.bytes));
        }
        let day = state.world().current_day.as_u32();
        if GOLDEN_DAYS.contains(&day) {
            let checkpoint = Checkpoint {
                day,
                state: canonical_m3pprop_state_hash(&state).expect("state digest"),
                metrics: canonical_m3pprop_metrics_hash(&metrics).expect("metrics digest"),
                events: canonical_m3pprop_event_hash(&events).expect("event digest"),
            };
            checkpoints.push(checkpoint);
        }
    }
    assert!(
        metrics
            .iter()
            .map(|row| row.m3p_metrics.births)
            .sum::<u64>()
            > 0
    );
    assert!(cleanup_removed > 0);
    assert!(metrics.iter().any(|row| row.propagation.levels_used >= 3));
    assert_eq!(
        checkpoints
            .iter()
            .map(|checkpoint| checkpoint.day)
            .collect::<Vec<_>>(),
        GOLDEN_DAYS
    );
    let actual: Vec<_> = checkpoints
        .iter()
        .map(|row| {
            (
                row.day,
                row.state.to_hex(),
                row.metrics.to_hex(),
                row.events.to_hex(),
            )
        })
        .collect();
    let expected: Vec<_> = GOLDENS
        .iter()
        .map(|(day, state, metrics, events)| {
            (
                *day,
                (*state).to_owned(),
                (*metrics).to_owned(),
                (*events).to_owned(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(
        metrics
            .iter()
            .map(|row| row.m3p_metrics.births)
            .sum::<u64>(),
        6
    );
    assert_eq!(cleanup_removed, 1);
    assert_eq!(snapshots.len(), 6);
}

#[test]
fn m3p_propagation_storage_paths_match_reference_each_day_and_resume_v3() {
    let config = config();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let initial = initialize_world(&config).expect("M3P propagation world initializes");
    let mut reference = M3pPropagationReferenceState::new(initial.clone(), &config, &run_context)
        .expect("reference initializes");
    let mut serial_world = HybridWorldState::hybrid(initial.clone());
    let mut incremental_world = HybridWorldState::hybrid(initial);
    let mut serial_context =
        M3pPropagationStorageRunContext::new(&serial_world, &config, &run_context)
            .expect("serial context initializes");
    let mut incremental_context = M3pPropagationIncrementalTopologyStorageRunContext::new(
        &incremental_world,
        &config,
        &run_context,
    )
    .expect("incremental context initializes");

    let mut metrics = Vec::new();
    let mut events = Vec::new();
    let mut snapshots = Vec::new();
    let mut paused_reference = None;
    let mut paused_world = None;
    let mut paused_context = None;
    for _ in 0..300 {
        let day = reference.world().current_day.as_u32();
        let previous_ids: BTreeSet<_> = reference
            .world()
            .agents
            .iter()
            .map(|agent| agent.agent_id)
            .collect();
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: (day + 1).is_multiple_of(50),
        };
        let expected = run_m3pprop_reference_day(&mut reference, &config, &run_context, &options)
            .expect("reference day runs");
        let serial = run_m3p_propagation_storage_days(
            &mut serial_world,
            &config,
            &mut serial_context,
            1,
            &options,
        )
        .expect("serial M3P propagation day runs")
        .remove(0);
        let incremental = run_m3p_propagation_storage_days_incremental_topology(
            &mut incremental_world,
            &config,
            &mut incremental_context,
            1,
            &options,
        )
        .expect("incremental M3P propagation day runs")
        .remove(0);
        assert_eq!(serial, expected, "serial storage mismatch on day {day}");
        assert_eq!(
            incremental, expected,
            "incremental storage mismatch on day {day}"
        );

        for event in &expected.events {
            if let M3pEvent::Birth { child, .. } = &event.event {
                assert_eq!(
                    reference.propagation().get(child),
                    Some(&sim_model::PropagationAgentState::default()),
                    "newborn {child} must remain uninformed on birth day {day}"
                );
            }
        }
        let reference_hash = canonical_m3pprop_state_hash(&reference).unwrap();
        for (world, graph, propagation, next_agent_id) in [
            (
                &serial_world,
                serial_context.relationship_graph(),
                serial_context.propagation(),
                serial_context.next_agent_id(),
            ),
            (
                &incremental_world,
                incremental_context.relationship_graph(),
                incremental_context.propagation(),
                incremental_context.next_agent_id(),
            ),
        ] {
            assert_eq!(
                canonical_m3pprop_state_hash_from_storage(
                    world.storage().unwrap(),
                    world.world.current_day,
                    &world.world.settlements,
                    graph,
                    next_agent_id,
                    propagation,
                )
                .unwrap(),
                reference_hash,
                "M3P propagation storage digest mismatch on day {day}"
            );
        }
        assert_eq!(
            serial_context.propagation(),
            reference.propagation(),
            "serial propagation columns mismatch on day {day}"
        );
        assert_eq!(
            incremental_context.propagation(),
            reference.propagation(),
            "incremental propagation columns mismatch on day {day}"
        );
        let current_ids: BTreeSet<_> = reference
            .world()
            .agents
            .iter()
            .map(|agent| agent.agent_id)
            .collect();
        for removed_id in previous_ids.difference(&current_ids) {
            assert!(!reference.propagation().contains_key(removed_id));
            assert!(!serial_context.propagation().contains_key(removed_id));
            assert!(!incremental_context.propagation().contains_key(removed_id));
        }

        metrics.push(expected.metrics.clone().unwrap());
        events.extend(expected.events.clone());
        if let Some(snapshot) = expected.snapshot.as_ref() {
            assert_eq!(serial.snapshot.as_ref().unwrap().bytes, snapshot.bytes);
            assert_eq!(incremental.snapshot.as_ref().unwrap().bytes, snapshot.bytes);
            snapshots.push((snapshot.metadata.day, snapshot.bytes.clone()));
            if snapshot.metadata.day == 50 {
                let restored = decode_m3pprop_snapshot(&snapshot.bytes, &config).unwrap();
                assert_eq!(restored.metadata.day, 50);
                let restored_reference = restored.state;
                let restored_world = HybridWorldState::hybrid(restored_reference.world().clone());
                let restored_context =
                    M3pPropagationIncrementalTopologyStorageRunContext::from_reference_state(
                        &restored_world,
                        &config,
                        &run_context,
                        &restored_reference,
                    )
                    .unwrap();
                paused_reference = Some(restored_reference);
                paused_world = Some(restored_world);
                paused_context = Some(restored_context);
            }
        } else {
            assert!(serial.snapshot.is_none());
            assert!(incremental.snapshot.is_none());
        }

        if (100..=300).contains(&(day + 1)) && GOLDEN_DAYS.contains(&(day + 1)) {
            let checkpoint = GOLDENS
                .iter()
                .find(|(golden_day, ..)| *golden_day == day + 1)
                .unwrap();
            assert_eq!(reference_hash.to_hex(), checkpoint.1);
            assert_eq!(
                canonical_m3pprop_metrics_hash(&metrics).unwrap().to_hex(),
                checkpoint.2
            );
            assert_eq!(
                canonical_m3pprop_event_hash(&events).unwrap().to_hex(),
                checkpoint.3
            );
        }

        if (51..=100).contains(&(day + 1)) {
            let resumed_reference = paused_reference.as_mut().unwrap();
            let resumed_world = paused_world.as_mut().unwrap();
            let resumed_context = paused_context.as_mut().unwrap();
            let resumed =
                run_m3pprop_reference_day(resumed_reference, &config, &run_context, &options)
                    .expect("resumed reference day runs");
            let resumed_storage = run_m3p_propagation_storage_days_incremental_topology(
                resumed_world,
                &config,
                resumed_context,
                1,
                &options,
            )
            .expect("resumed incremental M3P propagation day runs")
            .remove(0);
            assert_eq!(resumed, expected, "resumed reference mismatch on day {day}");
            assert_eq!(
                resumed_storage, expected,
                "resumed storage mismatch on day {day}"
            );
        }
    }
    assert_eq!(snapshots.len(), 6);
}

#[derive(Debug, Clone, PartialEq)]
struct DailyRecord {
    state: CanonicalHash,
    metrics: M3pPropagationDailyMetrics,
    events: Vec<M3pEventRecord>,
}

fn matrix_config(seed: u64, settlements: u32, degree: i64, max_hops: u32) -> SimConfig {
    let mut config = config();
    config.world.master_seed = seed;
    config.world.settlement_count = settlements;
    config.interaction.relationship_degree_k = Some(degree);
    config.propagation.as_mut().unwrap().max_hops = max_hops;
    config
        .validate()
        .expect("matrix M3P propagation config validates");
    config
}

fn reference_record(
    state: &M3pPropagationReferenceState,
    outcome: &sim_model::M3pPropagationDayOutcome,
) -> DailyRecord {
    DailyRecord {
        state: canonical_m3pprop_state_hash(state).unwrap(),
        metrics: outcome.metrics.clone().unwrap(),
        events: outcome.events.clone(),
    }
}

fn storage_record(
    world: &HybridWorldState,
    context: &M3pPropagationStorageRunContext,
    outcome: &sim_model::M3pPropagationDayOutcome,
) -> DailyRecord {
    DailyRecord {
        state: canonical_m3pprop_state_hash_from_storage(
            world.storage().unwrap(),
            world.world.current_day,
            &world.world.settlements,
            context.relationship_graph(),
            context.next_agent_id(),
            context.propagation(),
        )
        .unwrap(),
        metrics: outcome.metrics.clone().unwrap(),
        events: outcome.events.clone(),
    }
}

#[test]
fn m3p_propagation_parallel_matrix_matches_reference_per_day() {
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    for seed in [3, 81_985_529_216_486_895] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for max_hops in [1, 4, 16] {
                    let config = matrix_config(seed, settlements, degree, max_hops);
                    let run_context =
                        M0RunContext::new(config.world.master_seed, config.world.replicate_id);
                    let logical_world = initialize_world(&config)
                        .expect("matrix M3P propagation world initializes");
                    let mut reference = M3pPropagationReferenceState::new(
                        logical_world.clone(),
                        &config,
                        &run_context,
                    )
                    .expect("matrix reference initializes");
                    let mut expected = Vec::with_capacity(200);
                    for _ in 0..200 {
                        let outcome = run_m3pprop_reference_day(
                            &mut reference,
                            &config,
                            &run_context,
                            &options,
                        )
                        .expect("matrix reference day runs");
                        expected.push(reference_record(&reference, &outcome));
                    }

                    let mut serial_world = HybridWorldState::hybrid(logical_world.clone());
                    let mut serial_context =
                        M3pPropagationStorageRunContext::new(&serial_world, &config, &run_context)
                            .expect("matrix serial context initializes");
                    for (day, expected_record) in expected.iter().enumerate() {
                        let outcome = run_m3p_propagation_storage_days(
                            &mut serial_world,
                            &config,
                            &mut serial_context,
                            1,
                            &options,
                        )
                        .expect("matrix serial storage day runs")
                        .remove(0);
                        assert_eq!(
                            storage_record(&serial_world, &serial_context, &outcome),
                            *expected_record,
                            "serial mismatch seed={seed} K={settlements} k={degree} hops={max_hops} day={day}"
                        );
                    }

                    for threads in [1, 2, 4] {
                        let pool = ThreadPoolBuilder::new()
                            .num_threads(threads)
                            .build()
                            .expect("explicit thread pool builds");
                        for chunk in [64, 256] {
                            let chunk_size = NonZeroUsize::new(chunk).unwrap();
                            let mut parallel_world =
                                HybridWorldState::hybrid(logical_world.clone());
                            let mut parallel_context = M3pPropagationStorageRunContext::new(
                                &parallel_world,
                                &config,
                                &run_context,
                            )
                            .expect("matrix parallel context initializes");
                            for (day, expected_record) in expected.iter().enumerate() {
                                let outcome =
                                    run_m3p_propagation_storage_days_with_parallel_spread(
                                        &mut parallel_world,
                                        &config,
                                        &mut parallel_context,
                                        1,
                                        &options,
                                        &pool,
                                        chunk_size,
                                    )
                                    .expect("matrix parallel storage day runs")
                                    .remove(0);
                                assert_eq!(
                                    storage_record(&parallel_world, &parallel_context, &outcome),
                                    *expected_record,
                                    "parallel mismatch seed={seed} K={settlements} k={degree} hops={max_hops} threads={threads} chunk={chunk} day={day}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn configured_m3p_propagation_dispatch_selects_the_explicit_runner() {
    let config = config();
    assert_eq!(
        config.runner_selection(),
        SimulationRunnerSelection::M3pPropagation
    );
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("M3P propagation world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let mut context = ConfiguredStorageRunContext::M3pPropagation(
        M3pPropagationStorageRunContext::new(&hybrid, &config, &run_context)
            .expect("M3P propagation context initializes"),
    );
    let outcome = run_configured_storage_days(
        &mut hybrid,
        &config,
        &mut context,
        1,
        &DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: false,
        },
    )
    .expect("configured M3P propagation dispatch runs");
    assert!(matches!(
        outcome.days.as_slice(),
        [ConfiguredDayOutcome::M3pPropagation(_)]
    ));
}

#[test]
fn m3p_newborn_is_not_seeded_or_targeted_on_its_birth_day() {
    let config = config();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("M3P propagation world initializes");
    let state = M3pPropagationReferenceState::new(world, &config, &run_context)
        .expect("M3P propagation reference state initializes");
    let newborn = state.world().agents.last().unwrap().agent_id;
    let mut propagation: BTreeMap<_, _> = state
        .world()
        .agents
        .iter()
        .map(|agent| (agent.agent_id, PropagationAgentState::default()))
        .collect();
    let counts = apply_phase9c_m3p(
        state.world(),
        state.relationship_graph(),
        &mut propagation,
        PropagationConfig {
            seed_probability: 1.0,
            transmit_probability: 0.0,
            forget_probability: 0.0,
            max_hops: 1,
        },
        config.world.master_seed,
        config.world.replicate_id,
        0,
        &[newborn],
    )
    .expect("M3P Phase 9c honors the birth-day newborn exclusion");
    assert_eq!(counts.seeded, propagation.len() as u64 - 1);
    assert_eq!(counts.informed_count, propagation.len() as u64 - 1);
    assert_eq!(propagation[&newborn], PropagationAgentState::default());
}
