use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ParallelStorageRunContext, M3ReferenceContext,
    M3StorageRunContext, run_m3_reference_day, run_m3_storage_days,
    run_m3_storage_days_with_parallel_phase6b,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::HybridWorldState;
use sim_model::{EventRecord, M3DailyMetrics, M3DayOutcome, SimConfig, initialize_world};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

const M3_GOLDEN_CONFIG: &str = r#"
profile = "m3"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 30
settlement_count = 3
initial_health = 1.0
initial_food = 10.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
initial_treasury = 5000

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
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.15

[economy]
base_work_yield = 2.0
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

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 100.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

const MATRIX_CONFIG: &str = r#"
profile = "m3"

[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 300
settlement_count = 3
initial_health = 1.0
initial_food = 4.0
initial_wealth = 10000
initial_settlement_resource = 2000.0
initial_treasury = 5000

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
carrying_capacity = 10000.0
regrowth_rate = 0.1
base_metabolic_cost = 0.5
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
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

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 1.5, 1.5, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

const GOLDENS: [(u32, &str, &str, &str); 3] = [
    (
        100,
        "6507138d34f495ddf2f148dccc01c84a95bf7e22322a142dc9227197184d279a",
        "1b7dd9df9b441e5163aaf7393eb4ba010b3544b8d62c625e3a9fd614a38bac2b",
        "7003abb86b6c5598a8a362651f34b111493b33ac2900cd6ac533a0466d843769",
    ),
    (
        250,
        "43b75cf172e0f424631e1bf25ad19c8343e8a1e0993282696cf1a7d55d5b24ee",
        "783fc6ecd8a88cc967fcd6a217a8cddabce83f1cd6344184a7ff5fadeacef28c",
        "17fa7f486afc8ed2f73f03c8043384f944f8df04a48241f57b9cee34d036f881",
    ),
    (
        500,
        "41b85bf652cab71670404ba84bae8af3e0800a54e7d719274cef0e324863c9a0",
        "e7f0fba0680849fc910b072cab60db8327d9a471e747309fb56c8f0e5bd9a1ac",
        "621be56f6b6bf663ad14ed9b950e2add436e7973acbe6908ca69b5cdb9736c9c",
    ),
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct DayDigests {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Trace {
    days: Vec<DayDigests>,
    snapshots: BTreeMap<u32, Vec<u8>>,
}

fn golden_config() -> SimConfig {
    SimConfig::parse_and_validate(M3_GOLDEN_CONFIG).unwrap()
}

fn matrix_config(
    population: u64,
    settlements: u32,
    degree: i64,
    p_local: f32,
    seed: u64,
) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(MATRIX_CONFIG).unwrap();
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.master_seed = seed;
    config.interaction.relationship_degree_k = Some(degree);
    config.interaction.p_local = Some(p_local);
    config.validate().unwrap();
    config
}

fn options(snapshot_boundary: bool) -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary,
    }
}

fn day_digest(
    day: u32,
    state: CanonicalHash,
    metrics: &[M3DailyMetrics],
    events: &[EventRecord],
) -> DayDigests {
    DayDigests {
        day,
        state,
        metrics: canonical_m3_metrics_hash(metrics).unwrap(),
        events: canonical_event_hash(events).unwrap(),
    }
}

fn oracle_trace(config: &SimConfig, days: u32, snapshot_days: &[u32]) -> Trace {
    let reference_world = initialize_world(config).unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let reference_context =
        M3ReferenceContext::new(&reference_world, config, &run_context).unwrap();
    let mut reference_world = reference_world;

    let storage_world = HybridWorldState::hybrid(reference_world.clone());
    let mut storage_world = storage_world;
    let mut storage_context =
        M3StorageRunContext::new(&storage_world, config, &run_context).unwrap();
    assert_eq!(
        reference_context.relationship_graph().csr_hash(),
        storage_context.relationship_graph().csr_hash()
    );

    let mut reference_metrics = Vec::with_capacity(days as usize);
    let mut storage_metrics = Vec::with_capacity(days as usize);
    let mut reference_events = Vec::new();
    let mut storage_events = Vec::new();
    let mut trace = Trace {
        days: Vec::with_capacity(days as usize),
        snapshots: BTreeMap::new(),
    };

    for day in 1..=days {
        let snapshot_boundary = snapshot_days.contains(&day);
        let day_options = options(snapshot_boundary);
        let reference_outcome = run_m3_reference_day(
            &mut reference_world,
            config,
            &reference_context,
            &day_options,
        )
        .unwrap();
        let mut storage_outcomes = run_m3_storage_days(
            &mut storage_world,
            config,
            &mut storage_context,
            1,
            &day_options,
        )
        .unwrap();
        let storage_outcome = storage_outcomes.pop().unwrap();

        assert_eq!(storage_outcome.executed_day, reference_outcome.executed_day);
        assert_eq!(storage_outcome.metrics, reference_outcome.metrics);
        assert_eq!(storage_outcome.events, reference_outcome.events);
        assert_eq!(
            storage_outcome
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.bytes),
            reference_outcome
                .snapshot
                .as_ref()
                .map(|snapshot| &snapshot.bytes)
        );

        let reference_state =
            canonical_m3_state_hash(&reference_world, reference_context.relationship_graph())
                .unwrap();
        let storage = storage_world.segmented_storage.as_ref().unwrap();
        let storage_state = canonical_m3_state_hash_from_storage(
            storage,
            storage_world.world.current_day,
            &storage_world.world.settlements,
            storage_context.relationship_graph(),
        )
        .unwrap();
        assert_eq!(storage_state, reference_state);

        let reference_metric = reference_outcome.metrics.unwrap();
        let storage_metric = storage_outcome.metrics.unwrap();
        reference_metrics.push(reference_metric);
        storage_metrics.push(storage_metric);
        reference_events.extend(reference_outcome.events);
        storage_events.extend(storage_outcome.events);

        let reference_digest =
            day_digest(day, reference_state, &reference_metrics, &reference_events);
        let storage_digest = day_digest(day, storage_state, &storage_metrics, &storage_events);
        assert_eq!(storage_digest, reference_digest);
        trace.days.push(reference_digest);
        if let Some(snapshot) = storage_outcome.snapshot {
            trace.snapshots.insert(day, snapshot.bytes);
        }
    }
    trace
}

struct ParallelRun {
    hybrid_world: HybridWorldState,
    context: M3ParallelStorageRunContext,
    metrics: Vec<M3DailyMetrics>,
    events: Vec<EventRecord>,
    trace: Trace,
}

impl ParallelRun {
    fn new(config: &SimConfig, shuffled_slots: bool) -> Self {
        let mut world = initialize_world(config).unwrap();
        if shuffled_slots {
            world.agents.reverse();
        }
        let hybrid_world = HybridWorldState::hybrid(world);
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let context =
            M3ParallelStorageRunContext::new(&hybrid_world, config, &run_context).unwrap();
        Self {
            hybrid_world,
            context,
            metrics: Vec::new(),
            events: Vec::new(),
            trace: Trace {
                days: Vec::new(),
                snapshots: BTreeMap::new(),
            },
        }
    }

    fn advance(
        &mut self,
        config: &SimConfig,
        pool: &ThreadPool,
        chunk_size: usize,
        snapshot_boundary: bool,
    ) -> DayDigests {
        let mut outcomes = run_m3_storage_days_with_parallel_phase6b(
            &mut self.hybrid_world,
            config,
            &mut self.context,
            1,
            &options(snapshot_boundary),
            pool,
            NonZeroUsize::new(chunk_size).unwrap(),
        )
        .unwrap();
        let outcome: M3DayOutcome = outcomes.pop().unwrap();
        let metric = outcome.metrics.unwrap();
        self.metrics.push(metric);
        self.events.extend(outcome.events);

        let storage = self.hybrid_world.segmented_storage.as_ref().unwrap();
        let day = self.hybrid_world.world.current_day.as_u32();
        let state = canonical_m3_state_hash_from_storage(
            storage,
            self.hybrid_world.world.current_day,
            &self.hybrid_world.world.settlements,
            self.context.relationship_graph(),
        )
        .unwrap();
        let digest = day_digest(day, state, &self.metrics, &self.events);
        if let Some(snapshot) = outcome.snapshot {
            self.trace.snapshots.insert(day, snapshot.bytes);
        }
        self.trace.days.push(digest.clone());
        digest
    }

    fn checkpoint(&self, day: u32) -> ParallelCheckpoint {
        ParallelCheckpoint {
            snapshot: self.trace.snapshots[&day].clone(),
            graph_hash: *self.context.relationship_graph().csr_hash(),
            metrics: self.metrics.clone(),
            events: self.events.clone(),
            trace: self.trace.clone(),
        }
    }

    fn from_checkpoint(config: &SimConfig, checkpoint: ParallelCheckpoint) -> Self {
        let restored = restore_snapshot(&checkpoint.snapshot).unwrap();
        let run_context = M0RunContext::new(
            restored.metadata.master_seed,
            restored.metadata.replicate_id,
        );
        let hybrid_world = HybridWorldState::hybrid(restored.world);
        let context =
            M3ParallelStorageRunContext::new(&hybrid_world, config, &run_context).unwrap();
        assert_eq!(
            context.relationship_graph().csr_hash(),
            &checkpoint.graph_hash
        );
        Self {
            hybrid_world,
            context,
            metrics: checkpoint.metrics,
            events: checkpoint.events,
            trace: checkpoint.trace,
        }
    }
}

struct ParallelCheckpoint {
    snapshot: Vec<u8>,
    graph_hash: [u8; 32],
    metrics: Vec<M3DailyMetrics>,
    events: Vec<EventRecord>,
    trace: Trace,
}

fn make_pools() -> Vec<(usize, ThreadPool)> {
    [1, 2, 4]
        .into_iter()
        .map(|count| {
            (
                count,
                ThreadPoolBuilder::new().num_threads(count).build().unwrap(),
            )
        })
        .collect()
}

fn assert_trace(actual: &Trace, expected: &Trace) {
    assert_eq!(actual.days, expected.days);
    assert_eq!(actual.snapshots, expected.snapshots);
}

fn assert_m3_goldens(trace: &Trace) {
    for &(day, state, metrics, events) in &GOLDENS {
        let digest = &trace.days[(day - 1) as usize];
        assert_eq!(digest.day, day);
        assert_eq!(digest.state.to_hex(), state);
        assert_eq!(digest.metrics.to_hex(), metrics);
        assert_eq!(digest.events.to_hex(), events);
        assert!(trace.snapshots.contains_key(&day));
    }
}

#[test]
fn parallel_runner_matches_golden_full_matrix_and_thread_chunk_sweeps() {
    let pools = make_pools();
    let chunks = [64, 128, 256, 512, 1024];
    let golden_config = golden_config();
    let golden_oracle = oracle_trace(&golden_config, 500, &[100, 250, 500]);
    assert_m3_goldens(&golden_oracle);

    let mut parallel_resume_checkpoint = None;
    for (thread_count, pool) in &pools {
        for chunk_size in chunks {
            let mut run = ParallelRun::new(&golden_config, false);
            for day in 1..=500 {
                run.advance(
                    &golden_config,
                    pool,
                    chunk_size,
                    [100, 250, 500].contains(&day),
                );
                if *thread_count == 4 && chunk_size == 256 && day == 250 {
                    parallel_resume_checkpoint = Some(run.checkpoint(250));
                }
            }
            assert_trace(&run.trace, &golden_oracle);
        }
    }

    let mut resumed = ParallelRun::from_checkpoint(
        &golden_config,
        parallel_resume_checkpoint.expect("golden 4-thread/256-chunk checkpoint"),
    );
    for day in 251..=500 {
        resumed.advance(&golden_config, &pools[2].1, 256, day == 500);
    }
    assert_trace(&resumed.trace, &golden_oracle);

    let mut shuffled = ParallelRun::new(&golden_config, true);
    for day in 1..=500 {
        shuffled.advance(
            &golden_config,
            &pools[2].1,
            256,
            [100, 250, 500].contains(&day),
        );
    }
    assert_trace(&shuffled.trace, &golden_oracle);

    let mut sweep_cells = Vec::<(SimConfig, Trace)>::new();
    let mut matrix_cells = 0;
    for seed in [0, 1, 81_985_529_216_486_895] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for p_local in [0.0, 0.5, 1.0] {
                    let config = matrix_config(300, settlements, degree, p_local, seed);
                    let expected = oracle_trace(&config, 100, &[]);
                    for thread_count in [2, 4] {
                        let pool = &pools[(thread_count / 2) as usize].1;
                        let mut run = ParallelRun::new(&config, false);
                        for _ in 0..100 {
                            run.advance(&config, pool, 256, false);
                        }
                        assert_trace(&run.trace, &expected);
                    }
                    if seed == 81_985_529_216_486_895 && settlements == 3 && degree == 8 {
                        sweep_cells.push((config, expected));
                    }
                    matrix_cells += 1;
                }
            }
        }
    }
    for population in [1_000, 3_000] {
        let settlements = (population / 17) as u32;
        for p_local in [0.0, 0.5, 1.0] {
            let config = matrix_config(population, settlements, 8, p_local, 81_985_529_216_486_895);
            let expected = oracle_trace(&config, 100, &[]);
            for thread_count in [2, 4] {
                let pool = &pools[(thread_count / 2) as usize].1;
                let mut run = ParallelRun::new(&config, false);
                for _ in 0..100 {
                    run.advance(&config, pool, 256, false);
                }
                assert_trace(&run.trace, &expected);
            }
            matrix_cells += 1;
        }
    }
    assert_eq!(matrix_cells, 60);

    for (config, expected) in sweep_cells {
        for (thread_count, pool) in &pools {
            for chunk_size in chunks {
                if *thread_count != 1 && chunk_size == 256 {
                    continue;
                }
                let mut run = ParallelRun::new(&config, false);
                for _ in 0..100 {
                    run.advance(&config, pool, chunk_size, false);
                }
                assert_trace(&run.trace, &expected);
            }
        }
    }
}
