use rayon::ThreadPoolBuilder;
use sim_model::hashing::canonical_m3prop_state_hash_from_storage;
use sim_model::m3prop::{M3PropagationReferenceState, run_m3prop_reference_day};
use sim_model::state::HybridWorldState;
use sim_model::{
    DayExecutionOptions, M0RunContext, M3PropagationStorageRunContext, SimConfig, initialize_world,
    run_m3prop_storage_days,
};
use std::num::NonZeroUsize;

const CONFIG: &str = include_str!("fixtures/m3_propagation.toml");

fn config() -> SimConfig {
    SimConfig::parse_and_validate(CONFIG).unwrap()
}

#[test]
fn m3_storage_phase9c_matches_worldstate_reference_each_day_and_snapshot() {
    let config = config();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let initial = initialize_world(&config).unwrap();
    let mut reference =
        M3PropagationReferenceState::new(initial.clone(), &config, &run_context).unwrap();
    let mut storage_world = HybridWorldState::hybrid(initial);
    let mut storage_context =
        M3PropagationStorageRunContext::new(&storage_world, &config, &run_context).unwrap();
    let mut expected_outcomes = Vec::new();

    for day in 1..=300 {
        let snapshot_boundary = [100, 250].contains(&day);
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary,
        };
        let reference_outcome =
            run_m3prop_reference_day(&mut reference, &config, &run_context, &options).unwrap();
        let storage_outcome = run_m3prop_storage_days(
            &mut storage_world,
            &config,
            &mut storage_context,
            1,
            &options,
        )
        .unwrap()
        .remove(0);
        assert_eq!(
            storage_outcome.metrics, reference_outcome.metrics,
            "day {day}"
        );
        assert_eq!(
            storage_outcome.events, reference_outcome.events,
            "day {day}"
        );
        assert_eq!(
            storage_outcome
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.bytes),
            reference_outcome
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.bytes),
            "snapshot day {day}"
        );
        let storage = storage_world.segmented_storage.as_ref().unwrap();
        assert_eq!(
            canonical_m3prop_state_hash_from_storage(
                storage,
                storage_world.world.current_day,
                &storage_world.world.settlements,
                storage_context.relationship_graph(),
                storage_context.propagation(),
            )
            .unwrap(),
            reference.state_hash().unwrap(),
            "state day {day}"
        );
        expected_outcomes.push(reference_outcome);
    }

    for threads in [1, 2, 4] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        for chunk in [64, 256] {
            let mut hybrid = HybridWorldState::hybrid(initialize_world(&config).unwrap());
            let mut context =
                M3PropagationStorageRunContext::new(&hybrid, &config, &run_context).unwrap();
            for expected in &expected_outcomes {
                let parallel = sim_model::run_m3prop_storage_days_with_parallel_spread(
                    &mut hybrid,
                    &config,
                    &mut context,
                    1,
                    &DayExecutionOptions {
                        metrics_enabled: true,
                        events_enabled: true,
                        snapshot_boundary: expected.snapshot.is_some(),
                    },
                    &pool,
                    NonZeroUsize::new(chunk).unwrap(),
                )
                .unwrap()
                .remove(0);
                assert_eq!(&parallel, expected, "pool {threads}, chunk {chunk}");
            }
        }
    }
}
