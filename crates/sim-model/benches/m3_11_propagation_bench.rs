use sim_core::{AgentId, RngCoordinate, coordinate_prng_f32};
use sim_model::hashing::{
    CanonicalHash, canonical_m3prop_event_hash, canonical_m3prop_state_hash_from_storage,
};
use sim_model::m3prop::{M3PropagationReferenceState, run_m3prop_reference_day};
use sim_model::propagation::{
    M3_PROPAGATION_SUBSYSTEM_ID, PropagationAgentState, PropagationDailyCounts,
    apply_phase9c_storage, apply_phase9c_storage_parallel,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3PropagationStorageRunContext, M3StorageRunContext,
    run_m3_storage_days, run_m3prop_storage_days, run_m3prop_storage_days_with_parallel_spread,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    PropagationConfig, SimConfig, SimulationConfig, SimulationProfile, SimulationSystems,
    initialize_world,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::time::Instant;

#[path = "../examples/common/a5.rs"]
mod a5;
#[path = "../examples/common/process_controls.rs"]
mod process_controls;

const M2_40_CONFIG: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10000
settlement_count = 50
initial_health = 1.0
initial_food = 25.0
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
base_metabolic_cost = 0.3129
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
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const WARMUPS: usize = 2;
const SAMPLES: usize = 11;
const CHUNK_SIZE: NonZeroUsize = NonZeroUsize::new(256).expect("nonzero chunk size");
const SAMPLE_DAYS: u32 = 3;
const GOLDEN_CONFIG: &str = include_str!("../tests/fixtures/m3_propagation.toml");

struct TimedRun {
    world: HybridWorldState,
    context: M3PropagationStorageRunContext,
}

impl TimedRun {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).expect("world initializes");
        let hybrid_world = HybridWorldState::hybrid(world);
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let context = M3PropagationStorageRunContext::new(&hybrid_world, config, &run_context)
            .expect("propagation context builds");
        Self {
            world: hybrid_world,
            context,
        }
    }

    fn advance(&mut self, config: &SimConfig, pool: Option<&rayon::ThreadPool>) -> (f64, Trace) {
        let options = options();
        let started = Instant::now();
        let outcomes = match pool {
            Some(pool) => run_m3prop_storage_days_with_parallel_spread(
                &mut self.world,
                config,
                &mut self.context,
                SAMPLE_DAYS,
                &options,
                pool,
                CHUNK_SIZE,
            ),
            None => run_m3prop_storage_days(
                &mut self.world,
                config,
                &mut self.context,
                SAMPLE_DAYS,
                &options,
            ),
        }
        .expect("propagation runner succeeds");
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(SAMPLE_DAYS);
        let storage = self.world.storage().expect("hybrid storage exists");
        let state_hash = canonical_m3prop_state_hash_from_storage(
            storage,
            self.world.world.current_day,
            &self.world.world.settlements,
            self.context.relationship_graph(),
            self.context.propagation(),
        )
        .expect("storage propagation hash succeeds");
        let metrics = outcomes
            .iter()
            .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
            .collect();
        let event_hashes = outcomes
            .iter()
            .map(|outcome| canonical_m3prop_event_hash(&outcome.events).expect("event hash"))
            .collect();
        (
            elapsed_us,
            Trace {
                state_hash,
                metrics,
                event_hashes,
            },
        )
    }
}

#[derive(Debug, PartialEq)]
struct Trace {
    state_hash: CanonicalHash,
    metrics: Vec<sim_model::M3PropDailyMetrics>,
    event_hashes: Vec<CanonicalHash>,
}

fn options() -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

fn config(population: u64, degree: u32, max_hops: u32, transmit_probability: f32) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 parses");
    config.profile = SimulationProfile::M3;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200).expect("K fits u32");
    config.interaction.relationship_degree_k = Some(i64::from(degree));
    config.interaction.p_local = Some(0.5);
    let mut systems = SimulationSystems::for_profile(SimulationProfile::M3);
    systems.propagation = true;
    config.simulation = Some(SimulationConfig { systems });
    config.propagation = Some(PropagationConfig {
        seed_probability: 0.002,
        transmit_probability,
        forget_probability: 0.02,
        max_hops,
    });
    config.validate().expect("benchmark config validates");
    config
}

fn summarize(values: &[f64]) -> (f64, f64) {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let mut deviations: Vec<_> = values.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    (median, deviations[deviations.len() / 2])
}

fn assert_trace_equal(serial: &Trace, parallel: &Trace) {
    assert_eq!(serial.metrics, parallel.metrics);
    assert_eq!(serial.event_hashes, parallel.event_hashes);
    assert_eq!(serial.state_hash, parallel.state_hash);
}

fn paired_interleaved(config: &SimConfig, threads: usize) -> (f64, f64, f64, f64) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("fixed-size pool builds");
    for _ in 0..WARMUPS {
        let mut serial = TimedRun::new(config);
        let mut parallel = TimedRun::new(config);
        let (_, serial_trace) = serial.advance(config, None);
        let (_, parallel_trace) = parallel.advance(config, Some(&pool));
        assert_trace_equal(&serial_trace, &parallel_trace);
    }
    let mut serial_samples = Vec::with_capacity(SAMPLES);
    let mut parallel_samples = Vec::with_capacity(SAMPLES);
    for sample in 0..SAMPLES {
        let mut serial = TimedRun::new(config);
        let mut parallel = TimedRun::new(config);
        if sample % 2 == 0 {
            let (serial_us, serial_trace) = serial.advance(config, None);
            let (parallel_us, parallel_trace) = parallel.advance(config, Some(&pool));
            assert_trace_equal(&serial_trace, &parallel_trace);
            serial_samples.push(serial_us);
            parallel_samples.push(parallel_us);
        } else {
            let (parallel_us, parallel_trace) = parallel.advance(config, Some(&pool));
            let (serial_us, serial_trace) = serial.advance(config, None);
            assert_trace_equal(&serial_trace, &parallel_trace);
            serial_samples.push(serial_us);
            parallel_samples.push(parallel_us);
        }
    }
    let (serial_median, serial_mad) = summarize(&serial_samples);
    let (parallel_median, parallel_mad) = summarize(&parallel_samples);
    (serial_median, serial_mad, parallel_median, parallel_mad)
}

fn paired_non_interleaved(config: &SimConfig, threads: usize) -> (f64, f64, f64, f64) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("fixed-size pool builds");
    for _ in 0..WARMUPS {
        let mut serial = TimedRun::new(config);
        let mut parallel = TimedRun::new(config);
        let (_, serial_trace) = serial.advance(config, None);
        let (_, parallel_trace) = parallel.advance(config, Some(&pool));
        assert_trace_equal(&serial_trace, &parallel_trace);
    }
    let mut serial_samples = Vec::with_capacity(SAMPLES);
    let mut serial_traces = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let mut serial = TimedRun::new(config);
        let (elapsed, trace) = serial.advance(config, None);
        serial_samples.push(elapsed);
        serial_traces.push(trace);
    }
    let mut parallel_samples = Vec::with_capacity(SAMPLES);
    let mut parallel_traces = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let mut parallel = TimedRun::new(config);
        let (elapsed, trace) = parallel.advance(config, Some(&pool));
        parallel_samples.push(elapsed);
        parallel_traces.push(trace);
    }
    for (serial_trace, parallel_trace) in serial_traces.iter().zip(&parallel_traces) {
        assert_trace_equal(serial_trace, parallel_trace);
    }
    let (serial_median, serial_mad) = summarize(&serial_samples);
    let (parallel_median, parallel_mad) = summarize(&parallel_samples);
    (serial_median, serial_mad, parallel_median, parallel_mad)
}

fn initial_propagation(
    storage: &sim_model::storage::SegmentedAgentStorage,
) -> BTreeMap<AgentId, PropagationAgentState> {
    storage
        .agent_ids()
        .iter()
        .copied()
        .map(|agent_id| (agent_id, PropagationAgentState::default()))
        .collect()
}

fn trace_day_zero_frontiers(
    storage: &sim_model::storage::SegmentedAgentStorage,
    graph: &sim_model::RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    day: u32,
) -> (PropagationDailyCounts, Vec<usize>) {
    let mut agent_ids = storage.agent_ids().to_vec();
    agent_ids.sort_unstable();
    let live: BTreeSet<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .filter_map(|(id, alive)| alive.then_some(id))
        .collect();
    let mut counts = PropagationDailyCounts::default();
    let mut seeded = BTreeSet::new();
    for agent_id in &agent_ids {
        if !live.contains(agent_id) || propagation[agent_id].informed {
            continue;
        }
        let coordinate = RngCoordinate::new(
            SEED,
            REPLICATE,
            day,
            9,
            M3_PROPAGATION_SUBSYSTEM_ID,
            agent_id.0,
            1,
        );
        if coordinate_prng_f32(&coordinate) < config.seed_probability {
            let state = propagation
                .get_mut(agent_id)
                .expect("validated propagation id");
            state.informed = true;
            state.informed_day = day;
            seeded.insert(*agent_id);
            counts.seeded += 1;
        }
    }
    let mut frontier: BTreeSet<_> = seeded.clone();
    let mut frontier_sizes = vec![frontier.len()];
    let mut newly_informed = seeded;
    counts.max_frontier_size = frontier.len() as u64;
    for level in 1..=config.max_hops {
        if frontier.is_empty() {
            break;
        }
        counts.levels_used += 1;
        let mut next_frontier = BTreeSet::new();
        for source_id in &frontier {
            let neighbors = graph.neighbors_of(*source_id).expect("graph source exists");
            for (edge_position, target_id) in neighbors.iter().enumerate() {
                if !live.contains(target_id) || propagation[target_id].informed {
                    continue;
                }
                let coordinate = RngCoordinate::new(
                    SEED,
                    REPLICATE,
                    day,
                    9,
                    M3_PROPAGATION_SUBSYSTEM_ID,
                    source_id.0,
                    2 + level * 64 + edge_position as u32,
                );
                if coordinate_prng_f32(&coordinate) < config.transmit_probability {
                    next_frontier.insert(*target_id);
                }
            }
        }
        for target_id in &next_frontier {
            let state = propagation.get_mut(target_id).expect("validated target id");
            state.informed = true;
            state.informed_day = day;
        }
        newly_informed.extend(next_frontier.iter().copied());
        frontier_sizes.push(next_frontier.len());
        counts.max_frontier_size = counts.max_frontier_size.max(next_frontier.len() as u64);
        frontier = next_frontier;
    }
    counts.newly_informed = newly_informed.len() as u64;
    counts.informed_count = live
        .iter()
        .filter(|agent_id| propagation[agent_id].informed)
        .count() as u64;
    (counts, frontier_sizes)
}

struct PhaseMeasurements {
    serial_interleaved: (f64, f64),
    parallel_interleaved: Vec<(usize, f64, f64)>,
    serial_non_interleaved: (f64, f64),
    parallel_non_interleaved: Vec<(usize, f64, f64)>,
    counts: PropagationDailyCounts,
    frontiers: Vec<usize>,
}

fn phase9c_samples(config: &SimConfig) -> PhaseMeasurements {
    let world = initialize_world(config).expect("world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut base_config = config.clone();
    base_config
        .simulation
        .as_mut()
        .expect("systems exist")
        .systems
        .propagation = false;
    base_config.propagation = None;
    base_config.validate().expect("M3 base config validates");
    let mut base_context = M3StorageRunContext::new(&hybrid, &base_config, &run_context)
        .expect("base M3 context builds");
    run_m3_storage_days(&mut hybrid, &base_config, &mut base_context, 1, &options())
        .expect("base day runs");
    let storage = hybrid.storage().expect("storage exists");
    let graph = base_context.relationship_graph();
    let prop_config = config.propagation.expect("propagation config exists");
    let mut oracle_state = initial_propagation(storage);
    let (oracle_counts, frontier_sizes) =
        trace_day_zero_frontiers(storage, graph, &mut oracle_state, prop_config, 0);
    let mut serial_state = initial_propagation(storage);
    let actual_counts = apply_phase9c_storage(
        storage,
        graph,
        &mut serial_state,
        prop_config,
        SEED,
        REPLICATE,
        0,
    )
    .expect("serial Phase 9c succeeds");
    assert_eq!(oracle_counts, actual_counts);
    assert_eq!(oracle_state, serial_state);

    let mut serial_samples = Vec::with_capacity(SAMPLES);
    let mut parallel_samples = Vec::new();
    let mut pools = Vec::new();
    for threads in [1, 2, 4, 6] {
        pools.push((
            threads,
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap(),
        ));
        parallel_samples.push(Vec::with_capacity(SAMPLES));
    }
    for _ in 0..WARMUPS {
        let mut state = initial_propagation(storage);
        apply_phase9c_storage(storage, graph, &mut state, prop_config, SEED, REPLICATE, 0)
            .expect("serial warmup");
        for (_, pool) in &pools {
            let mut state = initial_propagation(storage);
            apply_phase9c_storage_parallel(
                storage,
                graph,
                &mut state,
                prop_config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK_SIZE,
            )
            .expect("parallel warmup");
        }
    }
    for _ in 0..SAMPLES {
        let mut serial_state = initial_propagation(storage);
        let started = Instant::now();
        let serial_counts = apply_phase9c_storage(
            storage,
            graph,
            &mut serial_state,
            prop_config,
            SEED,
            REPLICATE,
            0,
        )
        .expect("serial sample");
        serial_samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        assert_eq!(serial_counts, oracle_counts);
        assert_eq!(serial_state, oracle_state);
        for (index, (_, pool)) in pools.iter().enumerate() {
            let mut parallel_state = initial_propagation(storage);
            let started = Instant::now();
            let counts = apply_phase9c_storage_parallel(
                storage,
                graph,
                &mut parallel_state,
                prop_config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK_SIZE,
            )
            .expect("parallel sample");
            parallel_samples[index].push(started.elapsed().as_secs_f64() * 1_000_000.0);
            assert_eq!(serial_counts, counts);
            assert_eq!(serial_state, parallel_state);
        }
    }
    let (serial_median, serial_mad) = summarize(&serial_samples);
    let mut interleaved_rows = Vec::new();
    for ((threads, _), samples) in pools.iter().zip(&parallel_samples) {
        let (parallel_median, parallel_mad) = summarize(samples);
        interleaved_rows.push((*threads, parallel_median, parallel_mad));
    }

    let mut serial_non_interleaved_samples = Vec::with_capacity(SAMPLES);
    for _ in 0..WARMUPS {
        let mut state = initial_propagation(storage);
        apply_phase9c_storage(storage, graph, &mut state, prop_config, SEED, REPLICATE, 0)
            .expect("serial non-interleaved warmup");
    }
    for _ in 0..SAMPLES {
        let mut state = initial_propagation(storage);
        let started = Instant::now();
        let counts =
            apply_phase9c_storage(storage, graph, &mut state, prop_config, SEED, REPLICATE, 0)
                .expect("serial non-interleaved sample");
        serial_non_interleaved_samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        assert_eq!(counts, oracle_counts);
        assert_eq!(state, oracle_state);
    }
    let (serial_non_interleaved_median, serial_non_interleaved_mad) =
        summarize(&serial_non_interleaved_samples);
    let mut non_interleaved_rows = Vec::new();
    for (threads, pool) in &pools {
        for _ in 0..WARMUPS {
            let mut state = initial_propagation(storage);
            apply_phase9c_storage_parallel(
                storage,
                graph,
                &mut state,
                prop_config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK_SIZE,
            )
            .expect("parallel non-interleaved warmup");
        }
        let mut samples = Vec::with_capacity(SAMPLES);
        for _ in 0..SAMPLES {
            let mut state = initial_propagation(storage);
            let started = Instant::now();
            let counts = apply_phase9c_storage_parallel(
                storage,
                graph,
                &mut state,
                prop_config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK_SIZE,
            )
            .expect("parallel non-interleaved sample");
            samples.push(started.elapsed().as_secs_f64() * 1_000_000.0);
            assert_eq!(counts, oracle_counts);
            assert_eq!(state, oracle_state);
        }
        let (median, mad) = summarize(&samples);
        non_interleaved_rows.push((*threads, median, mad));
    }
    PhaseMeasurements {
        serial_interleaved: (serial_median, serial_mad),
        parallel_interleaved: interleaved_rows,
        serial_non_interleaved: (serial_non_interleaved_median, serial_non_interleaved_mad),
        parallel_non_interleaved: non_interleaved_rows,
        counts: oracle_counts,
        frontiers: frontier_sizes,
    }
}

fn run_cell(population: u64, degree: u32, max_hops: u32, transmit_probability: f32) {
    let config = config(population, degree, max_hops, transmit_probability);
    let phase = phase9c_samples(&config);
    println!(
        "PHASE|N={population}|K={}|k={degree}|max_hops={max_hops}|transmit={transmit_probability:.2}|frontiers={:?}|levels_used={}|max_frontier_size={}|informed={}|newly_informed={}|serial_i_us={:.3}|serial_i_mad={:.3}|p1_i_us={:.3}|p1_i_mad={:.3}|p2_i_us={:.3}|p2_i_mad={:.3}|p4_i_us={:.3}|p4_i_mad={:.3}|p6_i_us={:.3}|p6_i_mad={:.3}|serial_ni_us={:.3}|serial_ni_mad={:.3}|p1_ni_us={:.3}|p1_ni_mad={:.3}|p2_ni_us={:.3}|p2_ni_mad={:.3}|p4_ni_us={:.3}|p4_ni_mad={:.3}|p6_ni_us={:.3}|p6_ni_mad={:.3}",
        population / 200,
        phase.frontiers,
        phase.counts.levels_used,
        phase.counts.max_frontier_size,
        phase.counts.informed_count,
        phase.counts.newly_informed,
        phase.serial_interleaved.0,
        phase.serial_interleaved.1,
        phase.parallel_interleaved[0].1,
        phase.parallel_interleaved[0].2,
        phase.parallel_interleaved[1].1,
        phase.parallel_interleaved[1].2,
        phase.parallel_interleaved[2].1,
        phase.parallel_interleaved[2].2,
        phase.parallel_interleaved[3].1,
        phase.parallel_interleaved[3].2,
        phase.serial_non_interleaved.0,
        phase.serial_non_interleaved.1,
        phase.parallel_non_interleaved[0].1,
        phase.parallel_non_interleaved[0].2,
        phase.parallel_non_interleaved[1].1,
        phase.parallel_non_interleaved[1].2,
        phase.parallel_non_interleaved[2].1,
        phase.parallel_non_interleaved[2].2,
        phase.parallel_non_interleaved[3].1,
        phase.parallel_non_interleaved[3].2,
    );
    for threads in [1, 2, 4, 6] {
        let (is_serial, is_serial_mad, is_parallel, is_parallel_mad) =
            paired_interleaved(&config, threads);
        let (ni_serial, ni_serial_mad, ni_parallel, ni_parallel_mad) =
            paired_non_interleaved(&config, threads);
        println!(
            "TICK|N={population}|K={}|k={degree}|max_hops={max_hops}|transmit={transmit_probability:.2}|threads={threads}|interleaved_serial={is_serial:.3}|interleaved_serial_mad={is_serial_mad:.3}|interleaved_parallel={is_parallel:.3}|interleaved_parallel_mad={is_parallel_mad:.3}|interleaved_speedup={:.5}|noninterleaved_serial={ni_serial:.3}|noninterleaved_serial_mad={ni_serial_mad:.3}|noninterleaved_parallel={ni_parallel:.3}|noninterleaved_parallel_mad={ni_parallel_mad:.3}|noninterleaved_speedup={:.5}",
            population / 200,
            is_serial / is_parallel,
            ni_serial / ni_parallel,
        );
    }
}

struct PhaseSetup {
    world: HybridWorldState,
    context: M3StorageRunContext,
    prop_config: PropagationConfig,
    oracle_counts: PropagationDailyCounts,
    oracle_state: BTreeMap<AgentId, PropagationAgentState>,
}

impl PhaseSetup {
    fn new(config: &SimConfig) -> Self {
        let mut world =
            HybridWorldState::hybrid(initialize_world(config).expect("world initializes"));
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let mut base_config = config.clone();
        base_config
            .simulation
            .as_mut()
            .expect("systems exist")
            .systems
            .propagation = false;
        base_config.propagation = None;
        base_config.validate().expect("base config validates");
        let mut context = M3StorageRunContext::new(&world, &base_config, &run_context)
            .expect("base context builds");
        run_m3_storage_days(&mut world, &base_config, &mut context, 1, &options())
            .expect("base day runs");
        let storage = world.storage().expect("storage exists");
        let mut oracle_state = initial_propagation(storage);
        let (oracle_counts, _) = trace_day_zero_frontiers(
            storage,
            context.relationship_graph(),
            &mut oracle_state,
            config.propagation.expect("propagation config exists"),
            0,
        );
        Self {
            world,
            context,
            prop_config: config.propagation.expect("propagation config exists"),
            oracle_counts,
            oracle_state,
        }
    }

    fn run(
        &self,
        pool: Option<&rayon::ThreadPool>,
    ) -> (
        f64,
        PropagationDailyCounts,
        BTreeMap<AgentId, PropagationAgentState>,
    ) {
        let storage = self.world.storage().expect("storage exists");
        let mut state = initial_propagation(storage);
        let started = Instant::now();
        let counts = match pool {
            Some(pool) => apply_phase9c_storage_parallel(
                storage,
                self.context.relationship_graph(),
                &mut state,
                self.prop_config,
                SEED,
                REPLICATE,
                0,
                pool,
                CHUNK_SIZE,
            ),
            None => apply_phase9c_storage(
                storage,
                self.context.relationship_graph(),
                &mut state,
                self.prop_config,
                SEED,
                REPLICATE,
                0,
            ),
        };
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0;
        let counts = counts.expect("Phase 9c succeeds");
        assert_eq!(
            counts, self.oracle_counts,
            "Phase 9c counts match the serial oracle"
        );
        assert_eq!(
            state, self.oracle_state,
            "Phase 9c state matches the serial oracle"
        );
        (elapsed_us, counts, state)
    }
}

fn controls_record() -> (bool, bool) {
    let status = process_controls::apply_high_priority_affinity();
    if cfg!(windows) {
        assert!(
            status.priority_set && status.affinity_set,
            "A-5 process controls failed"
        );
    }
    (status.priority_set, status.affinity_set)
}

fn a5_record(
    cell: &str,
    config_hash: &str,
    paths: &[&str],
    series: &a5::Series,
    controls: (bool, bool),
) -> String {
    let records = (0..a5::SAMPLES)
        .map(|sample| {
            let values = paths
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    format!(
                        "\"{name}_us_per_call\":{:.9}",
                        series.path_samples_us[index][sample]
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{{values},\"timed_pair_elapsed_ns\":{:.0}}}",
                series.sample_seconds[sample] * 1_000_000_000.0
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let spreads = paths
        .iter()
        .enumerate()
        .map(|(index, name)| {
            format!(
                "\"{name}\":{:.9}",
                a5::mad_over_median(&series.path_samples_us[index])
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let repetitions = series.repetitions_per_path.first().copied().unwrap_or(1);
    let recalibrations = a5::recalibrations_json(series);
    format!(
        "{{\"kind\":\"series\",\"cell_id\":\"{cell}\",\"complete\":true,\"smoke\":false,\"config_hash\":\"{config_hash}\",\"process_controls\":{{\"priority_set\":{},\"affinity_set\":{}}},\"series\":{{\"warmups\":{},\"samples\":{},\"repetitions_per_sample\":{repetitions},\"sample_records\":[{records}],\"path_mad_over_median\":{{{spreads}}},\"recalibrations\":{recalibrations}}}}}",
        controls.0,
        controls.1,
        a5::WARMUPS,
        a5::SAMPLES
    )
}

fn append_record(path: &std::path::Path, record: &str) {
    use std::io::Write;
    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open A-5 JSONL");
    writeln!(output, "{record}").expect("append A-5 record");
    output.flush().expect("flush A-5 record");
}

fn a5_phase9c(
    population: u64,
    smoke: bool,
    controls: (bool, bool),
) -> (String, String, a5::Series, (bool, bool)) {
    let config = config(population, 16, 16, 0.2);
    let setup = PhaseSetup::new(&config);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(6)
        .build()
        .expect("six-thread pool builds");
    let id = format!("N{population}-k16-hops16-transmit0.2-pool6-day0");
    if smoke {
        let (_, _, serial_state) = setup.run(None);
        let (_, _, parallel_state) = setup.run(Some(&pool));
        assert_eq!(serial_state, parallel_state, "Phase 9c smoke parity");
        return (
            id,
            "smoke".into(),
            a5::Series {
                repetitions_per_path: vec![1],
                sample_seconds: vec![],
                path_samples_us: vec![],
                recalibrations: vec![],
            },
            controls,
        );
    }
    let series = a5::paired_series(2, |sample, repeat| {
        let (serial_us, _, serial_state, parallel_us, _, parallel_state) =
            if (sample + repeat) % 2 == 0 {
                let serial = setup.run(None);
                let parallel = setup.run(Some(&pool));
                (
                    serial.0, serial.1, serial.2, parallel.0, parallel.1, parallel.2,
                )
            } else {
                let parallel = setup.run(Some(&pool));
                let serial = setup.run(None);
                (
                    serial.0, serial.1, serial.2, parallel.0, parallel.1, parallel.2,
                )
            };
        assert_eq!(serial_state, parallel_state, "Phase 9c paired parity");
        (
            vec![serial_us, parallel_us],
            (serial_us + parallel_us) / 1_000_000.0,
        )
    });
    let hash = a5::config_hash(&format!(
        "{id}|seed={SEED}|replicate={REPLICATE}|K={}|phase=9c|pool=6|chunk=256|timer=apply_phase9c_storage",
        population / 200
    ));
    (id, hash, series, controls)
}

fn a5_tick(
    cell: &str,
    population: u64,
    interleaved: bool,
    smoke: bool,
    controls: (bool, bool),
) -> (String, String, a5::Series, (bool, bool)) {
    let config = config(population, 16, 16, 0.2);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(6)
        .build()
        .expect("six-thread pool builds");
    let series = if interleaved {
        if smoke {
            let mut serial = TimedRun::new(&config);
            let mut parallel = TimedRun::new(&config);
            let (_, serial_trace) = serial.advance(&config, None);
            let (_, parallel_trace) = parallel.advance(&config, Some(&pool));
            assert_trace_equal(&serial_trace, &parallel_trace);
            return (
                cell.into(),
                "smoke".into(),
                a5::Series {
                    repetitions_per_path: vec![1],
                    sample_seconds: vec![],
                    path_samples_us: vec![],
                    recalibrations: vec![],
                },
                controls,
            );
        }
        a5::paired_series(2, |sample, repeat| {
            let order = (sample + repeat) % 2;
            let (serial_us, serial_trace, parallel_us, parallel_trace) = if order == 0 {
                let mut serial = TimedRun::new(&config);
                let serial = serial.advance(&config, None);
                let mut parallel = TimedRun::new(&config);
                let parallel = parallel.advance(&config, Some(&pool));
                (serial.0, serial.1, parallel.0, parallel.1)
            } else {
                let mut parallel = TimedRun::new(&config);
                let parallel = parallel.advance(&config, Some(&pool));
                let mut serial = TimedRun::new(&config);
                let serial = serial.advance(&config, None);
                (serial.0, serial.1, parallel.0, parallel.1)
            };
            assert_trace_equal(&serial_trace, &parallel_trace);
            (
                vec![serial_us, parallel_us],
                (serial_us + parallel_us) / 1_000_000.0,
            )
        })
    } else {
        if smoke {
            let mut serial = TimedRun::new(&config);
            let mut parallel = TimedRun::new(&config);
            let (_, serial_trace) = serial.advance(&config, None);
            let (_, parallel_trace) = parallel.advance(&config, Some(&pool));
            assert_trace_equal(&serial_trace, &parallel_trace);
            return (
                cell.into(),
                "smoke".into(),
                a5::Series {
                    repetitions_per_path: vec![1],
                    sample_seconds: vec![],
                    path_samples_us: vec![],
                    recalibrations: vec![],
                },
                (true, true),
            );
        }
        let mut oracle = TimedRun::new(&config);
        let (_, expected) = oracle.advance(&config, None);
        a5::noninterleaved_series(2, |path, _, _| {
            let mut run = TimedRun::new(&config);
            let (elapsed, trace) = run.advance(&config, if path == 0 { None } else { Some(&pool) });
            assert_trace_equal(&expected, &trace);
            (elapsed, elapsed / 1_000_000.0)
        })
    };
    let hash = a5::config_hash(&format!(
        "{cell}|seed={SEED}|replicate={REPLICATE}|N={population}|K={}|k=16|hops=16|tx=0.2|threads=6|days=3|metrics=events=true|timer=public storage API",
        population / 200
    ));
    (cell.into(), hash, series, controls)
}

fn run_a5(output: &std::path::Path) {
    let controls = controls_record();
    let cells = [
        ("N100000-k16-hops16-transmit0.2-pool6-day0", 100_000, true),
        ("large-frontier-interleaved-pool6", 100_000, true),
        ("large-frontier-noninterleaved-pool6", 100_000, false),
    ];
    for (cell, population, _) in cells {
        let existing = std::fs::read_to_string(output).unwrap_or_default();
        let hash_scope = if cell.starts_with("N") {
            format!(
                "{cell}|seed={SEED}|replicate={REPLICATE}|K={}|phase=9c|pool=6|chunk=256|timer=apply_phase9c_storage",
                population / 200
            )
        } else {
            format!(
                "{cell}|seed={SEED}|replicate={REPLICATE}|N={population}|K={}|k=16|hops=16|tx=0.2|threads=6|days=3|metrics=events=true|timer=public storage API",
                population / 200
            )
        };
        let expected_hash = a5::config_hash(&hash_scope);
        if let Some(line) = existing
            .lines()
            .find(|line| line.contains(&format!("\"cell_id\":\"{cell}\"")))
        {
            assert!(
                line.contains(&format!("\"config_hash\":\"{expected_hash}\"")),
                "existing A-5 cell has a stale config hash: {cell}"
            );
            println!("A5_SKIP|cell={cell}");
            continue;
        }
        let result = match cell {
            "N100000-k16-hops16-transmit0.2-pool6-day0" => a5_phase9c(population, false, controls),
            "large-frontier-interleaved-pool6" => a5_tick(cell, population, true, false, controls),
            _ => a5_tick(cell, population, false, false, controls),
        };
        append_record(
            output,
            &a5_record(
                &result.0,
                &result.1,
                &["serial", "rayon"],
                &result.2,
                result.3,
            ),
        );
        println!("A5_CELL_COMPLETE|cell={cell}");
    }
}

fn a5_smoke(cell: &str) {
    let controls = controls_record();
    match cell {
        "N100000-k16-hops16-transmit0.2-pool6-day0" => {
            let _ = a5_phase9c(2_000, true, controls);
        }
        "large-frontier-interleaved-pool6" => {
            let _ = a5_tick(cell, 2_000, true, true, controls);
        }
        "large-frontier-noninterleaved-pool6" => {
            let _ = a5_tick(cell, 2_000, false, true, controls);
        }
        _ => panic!("unknown M3-11 A-5 cell {cell}"),
    }
    println!("A5_SMOKE_OK|cell={cell}|parity=true|timing_recorded=false");
}

fn golden_frontiers() {
    let config = SimConfig::parse_and_validate(GOLDEN_CONFIG).expect("golden config validates");
    let world = initialize_world(&config).expect("golden world initializes");
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let mut state = M3PropagationReferenceState::new(world, &config, &run_context)
        .expect("reference propagation state builds");
    let mut day = 0;
    while day < 300 {
        let outcome = run_m3prop_reference_day(&mut state, &config, &run_context, &options())
            .expect("golden day runs");
        let metrics = outcome.metrics.expect("golden metrics enabled");
        println!(
            "GOLDEN_DAY|day={}|levels_used={}|max_frontier_size={}",
            outcome.executed_day,
            metrics.propagation.levels_used,
            metrics.propagation.max_frontier_size
        );
        day = state.world().current_day.as_u32();
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("--a5") {
        let output = args
            .get(2)
            .map(std::path::PathBuf::from)
            .expect("--a5 needs JSONL output path");
        run_a5(&output);
        return;
    }
    if args.get(1).map(String::as_str) == Some("--a5-smoke") {
        a5_smoke(args.get(2).expect("--a5-smoke needs a cell id"));
        return;
    }
    println!(
        "HOST|os={}|arch={}|logical_threads={}|warmups={WARMUPS}|samples={SAMPLES}|days_per_sample={SAMPLE_DAYS}|p_local=0.5|chunk={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, usize::from),
        CHUNK_SIZE,
    );
    golden_frontiers();
    for population in [10_000, 100_000] {
        for degree in [8, 16] {
            for max_hops in [1, 2, 4, 8, 16] {
                for transmit_probability in [0.05, 0.2] {
                    println!(
                        "CELL_START|N={population}|k={degree}|max_hops={max_hops}|transmit={transmit_probability:.2}"
                    );
                    run_cell(population, degree, max_hops, transmit_probability);
                    println!(
                        "CELL_DONE|N={population}|k={degree}|max_hops={max_hops}|transmit={transmit_probability:.2}"
                    );
                }
            }
        }
    }
}
