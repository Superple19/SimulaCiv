use rayon::ThreadPool;
use sim_core::{AgentId, RngCoordinate, coordinate_prng_f32};
use sim_model::hashing::{
    CanonicalHash, canonical_m3pprop_event_hash, canonical_m3pprop_state_hash_from_storage,
};
use sim_model::m3p::M3pEvent;
use sim_model::propagation::{
    M3_PROPAGATION_SUBSYSTEM_ID, PropagationAgentState, PropagationDailyCounts,
    apply_phase9c_storage_m3p, apply_phase9c_storage_m3p_parallel,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3pPropagationIncrementalTopologyStorageRunContext,
    M3pPropagationStorageRunContext, run_m3p_propagation_storage_days,
    run_m3p_propagation_storage_days_incremental_topology,
    run_m3p_propagation_storage_days_with_parallel_spread,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    M3pPropagationDailyMetrics, PropagationConfig, SimConfig, SimulationConfig, SimulationProfile,
    SimulationSystems, initialize_world,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::time::Instant;

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
base_metabolic_cost = 1.0
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
const SAMPLE_DAYS: u32 = 3;
const CHUNK_SIZE: NonZeroUsize = NonZeroUsize::new(256).expect("nonzero chunk size");

#[derive(Clone, Copy)]
enum RunnerKind {
    Serial,
    Incremental,
    Parallel,
}

enum RunnerContext {
    Serial(M3pPropagationStorageRunContext),
    Incremental(Box<M3pPropagationIncrementalTopologyStorageRunContext>),
    Parallel(M3pPropagationStorageRunContext),
}

struct TimedRun {
    world: HybridWorldState,
    context: RunnerContext,
}

#[derive(Debug, PartialEq)]
struct Trace {
    state: CanonicalHash,
    metrics: Vec<M3pPropagationDailyMetrics>,
    event_hashes: Vec<CanonicalHash>,
}

impl TimedRun {
    fn new(config: &SimConfig, kind: RunnerKind) -> Self {
        let world = initialize_world(config).expect("M3P sustained world initializes");
        let hybrid = HybridWorldState::hybrid(world);
        let run_context = M0RunContext::new(SEED, REPLICATE);
        let context = match kind {
            RunnerKind::Serial => RunnerContext::Serial(
                M3pPropagationStorageRunContext::new(&hybrid, config, &run_context)
                    .expect("serial M3P propagation context builds"),
            ),
            RunnerKind::Incremental => RunnerContext::Incremental(Box::new(
                M3pPropagationIncrementalTopologyStorageRunContext::new(
                    &hybrid,
                    config,
                    &run_context,
                )
                .expect("incremental M3P propagation context builds"),
            )),
            RunnerKind::Parallel => RunnerContext::Parallel(
                M3pPropagationStorageRunContext::new(&hybrid, config, &run_context)
                    .expect("parallel M3P propagation context builds"),
            ),
        };
        Self {
            world: hybrid,
            context,
        }
    }

    fn advance(
        &mut self,
        config: &SimConfig,
        kind: RunnerKind,
        pool: Option<&ThreadPool>,
    ) -> (f64, Trace) {
        let options = options();
        let started = Instant::now();
        let outcomes = match (&mut self.context, kind) {
            (RunnerContext::Serial(context), RunnerKind::Serial) => {
                run_m3p_propagation_storage_days(
                    &mut self.world,
                    config,
                    context,
                    SAMPLE_DAYS,
                    &options,
                )
            }
            (RunnerContext::Incremental(context), RunnerKind::Incremental) => {
                run_m3p_propagation_storage_days_incremental_topology(
                    &mut self.world,
                    config,
                    context,
                    SAMPLE_DAYS,
                    &options,
                )
            }
            (RunnerContext::Parallel(context), RunnerKind::Parallel) => {
                run_m3p_propagation_storage_days_with_parallel_spread(
                    &mut self.world,
                    config,
                    context,
                    SAMPLE_DAYS,
                    &options,
                    pool.expect("parallel runner uses a caller-owned pool"),
                    CHUNK_SIZE,
                )
            }
            _ => panic!("runner mode and context do not match"),
        }
        .expect("M3P propagation runner succeeds");
        let elapsed_us = started.elapsed().as_secs_f64() * 1_000_000.0 / f64::from(SAMPLE_DAYS);
        let (graph, propagation, next_agent_id) = match &self.context {
            RunnerContext::Serial(context) | RunnerContext::Parallel(context) => (
                context.relationship_graph(),
                context.propagation(),
                context.next_agent_id(),
            ),
            RunnerContext::Incremental(context) => (
                context.relationship_graph(),
                context.propagation(),
                context.next_agent_id(),
            ),
        };
        let state = canonical_m3pprop_state_hash_from_storage(
            self.world.storage().expect("hybrid storage exists"),
            self.world.world.current_day,
            &self.world.world.settlements,
            graph,
            next_agent_id,
            propagation,
        )
        .expect("M3PROP storage state digest succeeds");
        let metrics = outcomes
            .iter()
            .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
            .collect();
        let event_hashes = outcomes
            .iter()
            .map(|outcome| canonical_m3pprop_event_hash(&outcome.events).expect("event digest"))
            .collect();
        (
            elapsed_us,
            Trace {
                state,
                metrics,
                event_hashes,
            },
        )
    }
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
    config.profile = SimulationProfile::M3p;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200).expect("K fits u32");
    config.world.initial_settlement_resource = 1_000_000.0;
    config.environment.carrying_capacity = 10_000_000.0;
    config.environment.health_decay_rate = 0.2;
    config.economy.base_work_yield = 1.9;
    config.interaction.relationship_degree_k = Some(i64::from(degree));
    config.interaction.p_local = Some(0.5);
    config.population = Some(sim_model::PopulationConfig {
        birth_min_health: 0.5,
        birth_food_threshold: 0.5,
        birth_probability: 0.0001,
        birth_food_cost: 0.5,
        births_max_per_day: 0,
        compaction_interval_days: 50,
    });
    config.decision.action_biases = [2.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    config.decision.base_weight_matrix = [
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 0.0, 1.0],
        [0.0; 5],
        [0.0; 5],
    ];
    let mut systems = SimulationSystems::for_profile(SimulationProfile::M3p);
    systems.propagation = true;
    config.simulation = Some(SimulationConfig { systems });
    config.propagation = Some(PropagationConfig {
        seed_probability: 0.002,
        transmit_probability,
        forget_probability: 0.02,
        max_hops,
    });
    config
        .validate()
        .expect("M3P sustained propagation config validates");
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

fn assert_same(actual: &Trace, expected: &Trace) {
    assert_eq!(actual, expected);
}

fn new_runs(config: &SimConfig) -> (TimedRun, TimedRun, TimedRun) {
    (
        TimedRun::new(config, RunnerKind::Serial),
        TimedRun::new(config, RunnerKind::Incremental),
        TimedRun::new(config, RunnerKind::Parallel),
    )
}

fn paired_interleaved(config: &SimConfig, threads: usize) -> [(f64, f64); 3] {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("fixed-size pool builds");
    for _ in 0..WARMUPS {
        let (mut serial, mut incremental, mut parallel) = new_runs(config);
        let (_, serial_trace) = serial.advance(config, RunnerKind::Serial, None);
        let (_, incremental_trace) = incremental.advance(config, RunnerKind::Incremental, None);
        let (_, parallel_trace) = parallel.advance(config, RunnerKind::Parallel, Some(&pool));
        assert_same(&serial_trace, &incremental_trace);
        assert_same(&serial_trace, &parallel_trace);
    }
    let mut samples: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for sample in 0..SAMPLES {
        let (mut serial, mut incremental, mut parallel) = new_runs(config);
        let mut traces: [Option<Trace>; 3] = std::array::from_fn(|_| None);
        let order = match sample % 3 {
            0 => [
                RunnerKind::Serial,
                RunnerKind::Incremental,
                RunnerKind::Parallel,
            ],
            1 => [
                RunnerKind::Incremental,
                RunnerKind::Parallel,
                RunnerKind::Serial,
            ],
            _ => [
                RunnerKind::Parallel,
                RunnerKind::Serial,
                RunnerKind::Incremental,
            ],
        };
        for kind in order {
            let (elapsed, trace) = match kind {
                RunnerKind::Serial => serial.advance(config, kind, None),
                RunnerKind::Incremental => incremental.advance(config, kind, None),
                RunnerKind::Parallel => parallel.advance(config, kind, Some(&pool)),
            };
            let index = match kind {
                RunnerKind::Serial => 0,
                RunnerKind::Incremental => 1,
                RunnerKind::Parallel => 2,
            };
            samples[index].push(elapsed);
            traces[index] = Some(trace);
        }
        assert_same(traces[0].as_ref().unwrap(), traces[1].as_ref().unwrap());
        assert_same(traces[0].as_ref().unwrap(), traces[2].as_ref().unwrap());
    }
    std::array::from_fn(|index| summarize(&samples[index]))
}

fn paired_non_interleaved(config: &SimConfig, threads: usize) -> [(f64, f64); 3] {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .expect("fixed-size pool builds");
    for _ in 0..WARMUPS {
        let mut serial = TimedRun::new(config, RunnerKind::Serial);
        let mut incremental = TimedRun::new(config, RunnerKind::Incremental);
        let mut parallel = TimedRun::new(config, RunnerKind::Parallel);
        let (_, serial_trace) = serial.advance(config, RunnerKind::Serial, None);
        let (_, incremental_trace) = incremental.advance(config, RunnerKind::Incremental, None);
        let (_, parallel_trace) = parallel.advance(config, RunnerKind::Parallel, Some(&pool));
        assert_same(&serial_trace, &incremental_trace);
        assert_same(&serial_trace, &parallel_trace);
    }

    let kinds = [
        RunnerKind::Serial,
        RunnerKind::Incremental,
        RunnerKind::Parallel,
    ];
    let mut traces: [Vec<Trace>; 3] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    let mut samples: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for (index, kind) in kinds.into_iter().enumerate() {
        for _ in 0..SAMPLES {
            let mut run = TimedRun::new(config, kind);
            let (elapsed, trace) = run.advance(
                config,
                kind,
                if matches!(kind, RunnerKind::Parallel) {
                    Some(&pool)
                } else {
                    None
                },
            );
            samples[index].push(elapsed);
            traces[index].push(trace);
        }
    }
    for ((serial, incremental), parallel) in traces[0].iter().zip(&traces[1]).zip(&traces[2]) {
        assert_same(serial, incremental);
        assert_same(serial, parallel);
    }
    std::array::from_fn(|index| summarize(&samples[index]))
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
    newborn_ids: &[AgentId],
) -> (PropagationDailyCounts, Vec<usize>) {
    let mut ids = storage.agent_ids().to_vec();
    ids.sort_unstable();
    let newborns: BTreeSet<_> = newborn_ids.iter().copied().collect();
    let live: BTreeSet<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .filter_map(|(agent_id, alive)| {
            (alive && !newborns.contains(&agent_id)).then_some(agent_id)
        })
        .collect();
    let mut counts = PropagationDailyCounts::default();
    let mut seeded = BTreeSet::new();
    for agent_id in &ids {
        if !live.contains(agent_id) || propagation[agent_id].informed {
            continue;
        }
        let coordinate = RngCoordinate::new(
            SEED,
            REPLICATE,
            0,
            9,
            M3_PROPAGATION_SUBSYSTEM_ID,
            agent_id.0,
            1,
        );
        if coordinate_prng_f32(&coordinate) < config.seed_probability {
            let row = propagation.get_mut(agent_id).unwrap();
            row.informed = true;
            row.informed_day = 0;
            seeded.insert(*agent_id);
            counts.seeded += 1;
        }
    }
    let mut frontier = seeded.clone();
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
            for (edge_position, target_id) in graph
                .neighbors_of(*source_id)
                .expect("graph source exists")
                .iter()
                .enumerate()
            {
                if !live.contains(target_id) || propagation[target_id].informed {
                    continue;
                }
                let coordinate = RngCoordinate::new(
                    SEED,
                    REPLICATE,
                    0,
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
            let row = propagation.get_mut(target_id).unwrap();
            row.informed = true;
            row.informed_day = 0;
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

#[derive(Debug)]
struct PhaseMeasurements {
    serial_interleaved: (f64, f64),
    parallel_interleaved: Vec<(usize, f64, f64)>,
    serial_non_interleaved: (f64, f64),
    parallel_non_interleaved: Vec<(usize, f64, f64)>,
    counts: PropagationDailyCounts,
    newborn_count: usize,
    frontiers: Vec<usize>,
}

fn phase9c_samples(config: &SimConfig) -> PhaseMeasurements {
    let world = initialize_world(config).expect("world initializes");
    let mut hybrid = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3pPropagationStorageRunContext::new(&hybrid, config, &run_context)
        .expect("M3P propagation context builds");
    let day_outcome =
        run_m3p_propagation_storage_days(&mut hybrid, config, &mut context, 1, &options())
            .expect("M3P day 0 runs")
            .remove(0);
    let storage = hybrid.storage().unwrap();
    let newborn_ids: Vec<_> = day_outcome
        .events
        .iter()
        .filter_map(|record| match &record.event {
            M3pEvent::Birth { child, .. } => Some(*child),
            _ => None,
        })
        .collect();
    let prop_config = config.propagation.unwrap();
    let mut traced_state = initial_propagation(storage);
    let (traced_counts, frontiers) = trace_day_zero_frontiers(
        storage,
        context.relationship_graph(),
        &mut traced_state,
        prop_config,
        &newborn_ids,
    );
    assert_eq!(
        Some(traced_counts),
        day_outcome
            .metrics
            .as_ref()
            .map(|metric| metric.propagation),
        "benchmark frontier trace matches production Phase 9c metrics"
    );
    assert_eq!(&traced_state, context.propagation());
    let counts = traced_counts;

    let pools: Vec<_> = [1, 2, 4, 6]
        .into_iter()
        .map(|threads| {
            (
                threads,
                rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .expect("fixed-size pool builds"),
            )
        })
        .collect();
    let measure = |parallel: Option<&ThreadPool>| {
        let mut propagation = initial_propagation(storage);
        let started = Instant::now();
        let result = match parallel {
            Some(pool) => apply_phase9c_storage_m3p_parallel(
                storage,
                context.relationship_graph(),
                &mut propagation,
                prop_config,
                SEED,
                REPLICATE,
                0,
                &newborn_ids,
                pool,
                CHUNK_SIZE,
            ),
            None => apply_phase9c_storage_m3p(
                storage,
                context.relationship_graph(),
                &mut propagation,
                prop_config,
                SEED,
                REPLICATE,
                0,
                &newborn_ids,
            ),
        }
        .expect("isolated Phase 9c call succeeds");
        let elapsed = started.elapsed().as_secs_f64() * 1_000_000.0;
        assert_eq!(result, counts);
        assert_eq!(propagation, traced_state);
        elapsed
    };
    for _ in 0..WARMUPS {
        let _ = measure(None);
        for (_, pool) in &pools {
            let _ = measure(Some(pool));
        }
    }
    let mut serial_i = Vec::with_capacity(SAMPLES);
    let mut parallel_i: [Vec<f64>; 4] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for sample in 0..SAMPLES {
        if sample % 2 == 0 {
            serial_i.push(measure(None));
            for (index, (_, pool)) in pools.iter().enumerate() {
                parallel_i[index].push(measure(Some(pool)));
            }
        } else {
            for (index, (_, pool)) in pools.iter().enumerate().rev() {
                parallel_i[index].push(measure(Some(pool)));
            }
            serial_i.push(measure(None));
        }
    }
    let serial_interleaved = summarize(&serial_i);
    let parallel_interleaved = pools
        .iter()
        .zip(&parallel_i)
        .map(|((threads, _), samples)| {
            let (median, mad) = summarize(samples);
            (*threads, median, mad)
        })
        .collect();

    let mut serial_ni = Vec::with_capacity(SAMPLES);
    for _ in 0..WARMUPS {
        let _ = measure(None);
    }
    for _ in 0..SAMPLES {
        serial_ni.push(measure(None));
    }
    let serial_non_interleaved = summarize(&serial_ni);
    let mut parallel_non_interleaved = Vec::new();
    for (threads, pool) in &pools {
        for _ in 0..WARMUPS {
            let _ = measure(Some(pool));
        }
        let samples: Vec<_> = (0..SAMPLES).map(|_| measure(Some(pool))).collect();
        let (median, mad) = summarize(&samples);
        parallel_non_interleaved.push((*threads, median, mad));
    }
    PhaseMeasurements {
        serial_interleaved,
        parallel_interleaved,
        serial_non_interleaved,
        parallel_non_interleaved,
        counts,
        newborn_count: newborn_ids.len(),
        frontiers,
    }
}

fn run_cell(population: u64, degree: u32, max_hops: u32, transmit_probability: f32) {
    let config = config(population, degree, max_hops, transmit_probability);
    let phase = phase9c_samples(&config);
    println!(
        "M3P_PHASE|N={population}|K={}|k={degree}|hops={max_hops}|tx={transmit_probability:.2}|frontiers={:?}|levels={}|max_frontier={}|newborns={}|serial_i={:.3}±{:.3}|p1_i={:.3}±{:.3}|p2_i={:.3}±{:.3}|p4_i={:.3}±{:.3}|p6_i={:.3}±{:.3}|serial_ni={:.3}±{:.3}|p1_ni={:.3}±{:.3}|p2_ni={:.3}±{:.3}|p4_ni={:.3}±{:.3}|p6_ni={:.3}±{:.3}",
        population / 200,
        phase.frontiers,
        phase.counts.levels_used,
        phase.counts.max_frontier_size,
        phase.newborn_count,
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
        let interleaved = paired_interleaved(&config, threads);
        let non_interleaved = paired_non_interleaved(&config, threads);
        println!(
            "M3P_TICK|N={population}|K={}|k={degree}|hops={max_hops}|tx={transmit_probability:.2}|threads={threads}|I_serial={:.3}±{:.3}|I_incremental={:.3}±{:.3}|I_parallel={:.3}±{:.3}|I_speedup={:.5}|NI_serial={:.3}±{:.3}|NI_incremental={:.3}±{:.3}|NI_parallel={:.3}±{:.3}|NI_speedup={:.5}",
            population / 200,
            interleaved[0].0,
            interleaved[0].1,
            interleaved[1].0,
            interleaved[1].1,
            interleaved[2].0,
            interleaved[2].1,
            interleaved[0].0 / interleaved[2].0,
            non_interleaved[0].0,
            non_interleaved[0].1,
            non_interleaved[1].0,
            non_interleaved[1].1,
            non_interleaved[2].0,
            non_interleaved[2].1,
            non_interleaved[0].0 / non_interleaved[2].0,
        );
    }
}

fn main() {
    println!(
        "M3P_HOST|os={}|arch={}|logical_threads={}|warmups={WARMUPS}|samples={SAMPLES}|days_per_sample={SAMPLE_DAYS}|p_local=0.5|chunk={CHUNK_SIZE}|profile=m3p_sustained",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, usize::from),
    );
    for population in [10_000, 100_000] {
        let degree = 8;
        let transmit_probability = 0.2;
        for max_hops in [4, 16] {
            println!(
                "M3P_CELL_START|N={population}|k={degree}|hops={max_hops}|tx={transmit_probability:.2}"
            );
            run_cell(population, degree, max_hops, transmit_probability);
            println!(
                "M3P_CELL_DONE|N={population}|k={degree}|hops={max_hops}|tx={transmit_probability:.2}"
            );
        }
    }
}
