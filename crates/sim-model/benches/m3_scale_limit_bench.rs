use rayon::ThreadPoolBuilder;
use sim_core::{AgentId, GroupId, SimulationDay};
use sim_model::decision::{PrimaryActionChoice, phase4_primary_action_selection_storage_into};
use sim_model::events::{
    EventBuffer, event_from_daily_metrics, events_from_market_resolution,
    events_from_mortality_resolution, events_from_targeted_resolution,
    events_from_welfare_resolution, events_from_work_resolution, phase11_flush_events,
};
use sim_model::features::{
    AgentFeatures, Phase3ScarcityScratch, phase3_observation_and_features_storage_with_scratch,
    phase3_observation_and_features_storage_with_timing,
};
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::intents::{
    Intent, M3Phase4SlotProjection, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_storage_into,
};
use sim_model::metrics::M3DailyMetrics;
use sim_model::partitioning::{
    Phase5PartitionScratch, phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::{phase1_resource_regrowth, phase9_mortality_commitment_storage};
use sim_model::relationship_graph::RelationshipGraph;
use sim_model::resolution::{
    M3Phase6BStorageScratch, Phase8WelfareScratch, phase6a_work_resolution_storage,
    phase6b_targeted_resolution_storage_m3_with_timing,
    phase7_market_clearance_storage_with_config,
    phase8_welfare_distribution_storage_with_config_and_scratch,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3DayOutcome, M3ParallelStorageRunContext,
    M3StorageRunContext, run_m3_storage_days, run_m3_storage_days_with_parallel_phase6b,
    run_m3_storage_days_with_parallel_phase6b_timing, run_m3_storage_days_with_phase6b_timing,
};
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::storage::SegmentedAgentStorage;
use sim_model::{SimConfig, SimulationProfile, initialize_world};
use std::alloc::{GlobalAlloc, Layout, System};
use std::array;
use std::collections::HashMap;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const M2_40_CONFIG: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
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

const MASTER_SEED: u64 = 81_985_529_216_486_895;
const REPLICATE_ID: u32 = 7;
const EQUILIBRIUM_METABOLIC_COST: f32 = 0.3129;
const POPULATIONS: [u64; 4] = [100_000, 250_000, 500_000, 1_000_000];
const DEGREES: [u32; 2] = [8, 16];
const P_LOCAL: f32 = 0.5;
const POOL_THREADS: usize = 6;
const CHUNK_SIZE: usize = 256;
const DAYS_PER_SAMPLE: u32 = 3;
const WARMUPS: usize = 2;
const SAMPLES: usize = 7;
const CHECKPOINT_DAYS: [u32; 2] = [0, 50];
const RESIDUAL_SAMPLES: usize = 15;
const RESIDUAL_POPULATIONS: [u64; 10] = [
    25_000, 50_000, 100_000, 150_000, 200_000, 300_000, 400_000, 500_000, 750_000, 1_000_000,
];

struct AllocationTracker;

static TRACK_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static CAPTURED_ALLOCATION_BYTES: AtomicI64 = AtomicI64::new(0);

// This tracker is enabled only around untimed one-time and scratch probes.
unsafe impl GlobalAlloc for AllocationTracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: this delegates the unchanged layout to the system allocator.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() && TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            CAPTURED_ALLOCATION_BYTES.fetch_add(layout.size() as i64, Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            CAPTURED_ALLOCATION_BYTES.fetch_sub(layout.size() as i64, Ordering::Relaxed);
        }
        // SAFETY: `pointer` and `layout` are forwarded unchanged to the system allocator.
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: this forwards the original allocation and requested size unchanged.
        let new_pointer = unsafe { System.realloc(pointer, layout, new_size) };
        if !new_pointer.is_null() && TRACK_ALLOCATIONS.load(Ordering::Relaxed) {
            CAPTURED_ALLOCATION_BYTES
                .fetch_add(new_size as i64 - layout.size() as i64, Ordering::Relaxed);
        }
        new_pointer
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: AllocationTracker = AllocationTracker;

fn capture_allocations<T>(operation: impl FnOnce() -> T) -> (T, i64) {
    CAPTURED_ALLOCATION_BYTES.store(0, Ordering::Relaxed);
    TRACK_ALLOCATIONS.store(true, Ordering::Relaxed);
    let result = operation();
    TRACK_ALLOCATIONS.store(false, Ordering::Relaxed);
    (result, CAPTURED_ALLOCATION_BYTES.load(Ordering::Relaxed))
}

fn config_for(population: u64, degree: u32) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 config parses");
    config.profile = SimulationProfile::M3;
    config.world.initial_population = population;
    config.world.settlement_count = u32::try_from(population / 200).expect("K fits u32");
    config.world.master_seed = MASTER_SEED;
    config.world.replicate_id = REPLICATE_ID;
    config.environment.base_metabolic_cost = EQUILIBRIUM_METABOLIC_COST;
    config.interaction.relationship_degree_k = Some(i64::from(degree));
    config.interaction.p_local = Some(P_LOCAL);
    config.validate().expect("scale config validates");
    config
}

fn options() -> DayExecutionOptions {
    DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    }
}

fn initialize(config: &SimConfig) -> (HybridWorldState, Duration) {
    let started = Instant::now();
    let world = initialize_world(config).expect("world initializes");
    let hybrid = HybridWorldState::hybrid(world);
    (hybrid, started.elapsed())
}

fn memberships(storage: &SegmentedAgentStorage) -> Vec<(AgentId, GroupId)> {
    storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect()
}

#[derive(Clone, Copy)]
struct SetupMeasurement {
    initialization: Duration,
    graph: Duration,
    projection: Duration,
    graph_heap_bytes: i64,
    projection_heap_bytes: i64,
}

fn measure_setup(
    config: &SimConfig,
    base: &HybridWorldState,
    initialization: Duration,
) -> SetupMeasurement {
    let storage = base.storage().expect("hybrid storage exists");
    let memberships = memberships(storage);
    let graph_start = Instant::now();
    let (graph, graph_heap_bytes) = capture_allocations(|| {
        RelationshipGraph::generate(
            &memberships,
            MASTER_SEED,
            REPLICATE_ID,
            config.interaction.relationship_degree_k.unwrap() as u32,
            config.interaction.p_local.unwrap(),
        )
        .expect("relationship graph builds")
    });
    assert_eq!(graph_heap_bytes as usize, csr_capacity_bytes(&graph));
    let graph_time = graph_start.elapsed();

    let projection_start = Instant::now();
    let (_projection, projection_heap_bytes) = capture_allocations(|| {
        M3Phase4SlotProjection::new(&graph, storage).expect("slot projection builds")
    });
    let projection_time = projection_start.elapsed();
    assert_eq!(
        projection_heap_bytes as usize,
        (storage.len() + 1 + graph.neighbors().len()) * std::mem::size_of::<usize>()
    );
    SetupMeasurement {
        initialization,
        graph: graph_time,
        projection: projection_time,
        graph_heap_bytes,
        projection_heap_bytes,
    }
}

fn build_checkpoint(base: &HybridWorldState, config: &SimConfig, day: u32) -> HybridWorldState {
    let mut world = base.clone();
    if day == 0 {
        return world;
    }
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut context =
        M3StorageRunContext::new(&world, config, &run_context).expect("checkpoint context builds");
    for _ in 0..day {
        drop(
            run_m3_storage_days(&mut world, config, &mut context, 1, &options())
                .expect("checkpoint advances"),
        );
    }
    world
}

fn vector_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(std::mem::size_of::<T>())
}

fn soa_capacity_bytes(storage: &SegmentedAgentStorage) -> usize {
    vector_bytes::<bool>(storage.demography.alive.capacity())
        + vector_bytes::<SimulationDay>(storage.demography.birth_day.capacity())
        + vector_bytes::<f32>(storage.demography.health.capacity())
        + vector_bytes::<f32>(storage.economy.food.capacity())
        + vector_bytes::<sim_core::Money>(storage.economy.wealth.capacity())
        + vector_bytes::<GroupId>(storage.economy.group_id.capacity())
        + vector_bytes::<f32>(storage.personality.productivity.capacity())
        + vector_bytes::<f32>(storage.personality.cooperation.capacity())
        + vector_bytes::<f32>(storage.personality.aggression.capacity())
        + vector_bytes::<f32>(storage.personality.risk_tolerance.capacity())
        + vector_bytes::<AgentId>(storage.agent_ids.capacity())
        + vector_bytes::<sim_core::DenseSlot>(storage.dense_slots.capacity())
}

fn csr_capacity_bytes(graph: &RelationshipGraph) -> usize {
    vector_bytes::<AgentId>(graph.source_ids().len())
        + vector_bytes::<usize>(graph.row_offsets().len())
        + vector_bytes::<AgentId>(graph.neighbors().len())
}

fn membership_capacity_bytes(population: usize) -> usize {
    vector_bytes::<(AgentId, GroupId)>(population)
}

#[derive(Clone, Copy)]
struct ContextMemory {
    serial_scratch_bytes: i64,
    parallel_extra_scratch_bytes: i64,
}

fn serial_context_retained_bytes(checkpoint: &HybridWorldState, config: &SimConfig) -> i64 {
    let mut world = checkpoint.clone();
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let (context, retained_bytes) = capture_allocations(|| {
        let mut context = M3StorageRunContext::new(&world, config, &run_context)
            .expect("serial context builds for memory probe");
        for _ in 0..DAYS_PER_SAMPLE {
            drop(
                run_m3_storage_days(&mut world, config, &mut context, 1, &options())
                    .expect("serial memory probe day succeeds"),
            );
        }
        context
    });
    drop(context);
    retained_bytes
}

fn parallel_context_retained_bytes(
    checkpoint: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
) -> i64 {
    let mut world = checkpoint.clone();
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let chunk_size = NonZeroUsize::new(CHUNK_SIZE).expect("chunk size is nonzero");
    let (context, retained_bytes) = capture_allocations(|| {
        let mut context = M3ParallelStorageRunContext::new(&world, config, &run_context)
            .expect("parallel context builds for memory probe");
        for _ in 0..DAYS_PER_SAMPLE {
            drop(
                run_m3_storage_days_with_parallel_phase6b(
                    &mut world,
                    config,
                    &mut context,
                    1,
                    &options(),
                    pool,
                    chunk_size,
                )
                .expect("parallel memory probe day succeeds"),
            );
        }
        context
    });
    drop(context);
    retained_bytes
}

fn context_memory(
    checkpoint: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    graph_bytes: usize,
    projection_bytes: usize,
    population: usize,
) -> ContextMemory {
    let common_bytes =
        graph_bytes as i64 + projection_bytes as i64 + membership_capacity_bytes(population) as i64;
    let serial_scratch_bytes = serial_context_retained_bytes(checkpoint, config) - common_bytes;
    let parallel_total_scratch =
        parallel_context_retained_bytes(checkpoint, config, pool) - common_bytes;
    ContextMemory {
        serial_scratch_bytes,
        parallel_extra_scratch_bytes: parallel_total_scratch - serial_scratch_bytes,
    }
}

#[cfg(windows)]
#[repr(C)]
struct ProcessMemoryCountersEx {
    size: u32,
    page_fault_count: u32,
    // Peak WS, current WS, quota peaks/usage, pagefile usage/peak, private usage.
    values: [usize; 9],
}

#[cfg(windows)]
#[link(name = "Psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut std::ffi::c_void,
        counters: *mut ProcessMemoryCountersEx,
        size: u32,
    ) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
}

#[cfg(windows)]
fn current_working_set_bytes() -> Option<usize> {
    let mut counters = ProcessMemoryCountersEx {
        size: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
        page_fault_count: 0,
        values: [0; 9],
    };
    // SAFETY: the process pseudo-handle is valid and the buffer has the declared C layout/size.
    let succeeded =
        unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.size) };
    (succeeded != 0).then_some(counters.values[1])
}

#[cfg(not(windows))]
fn current_working_set_bytes() -> Option<usize> {
    None
}

fn sampled_peak_working_set(operation: impl FnOnce()) -> Option<usize> {
    let Some(initial) = current_working_set_bytes() else {
        operation();
        return None;
    };
    let finished = Arc::new(AtomicBool::new(false));
    let peak = Arc::new(AtomicUsize::new(initial));
    let monitor_finished = Arc::clone(&finished);
    let monitor_peak = Arc::clone(&peak);
    let monitor = thread::spawn(move || {
        while !monitor_finished.load(Ordering::Relaxed) {
            if let Some(current) = current_working_set_bytes() {
                monitor_peak.fetch_max(current, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(2));
        }
    });
    operation();
    finished.store(true, Ordering::Relaxed);
    monitor.join().expect("working-set monitor joins");
    Some(peak.load(Ordering::Relaxed))
}

fn warm_serial_context(checkpoint: &HybridWorldState, config: &SimConfig) {
    let mut world = checkpoint.clone();
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut context = M3StorageRunContext::new(&world, config, &run_context)
        .expect("serial memory warm context builds");
    for _ in 0..DAYS_PER_SAMPLE {
        drop(
            run_m3_storage_days(&mut world, config, &mut context, 1, &options())
                .expect("serial memory warm day succeeds"),
        );
    }
}

fn warm_parallel_context(
    checkpoint: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
) {
    let mut world = checkpoint.clone();
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut context = M3ParallelStorageRunContext::new(&world, config, &run_context)
        .expect("parallel memory warm context builds");
    let chunk_size = NonZeroUsize::new(CHUNK_SIZE).expect("chunk size is nonzero");
    for _ in 0..DAYS_PER_SAMPLE {
        drop(
            run_m3_storage_days_with_parallel_phase6b(
                &mut world,
                config,
                &mut context,
                1,
                &options(),
                pool,
                chunk_size,
            )
            .expect("parallel memory warm day succeeds"),
        );
    }
}

#[derive(Clone, Copy)]
struct MemoryMeasurement {
    soa_bytes: usize,
    graph_csr_bytes: usize,
    projection_bytes: usize,
    membership_bytes: usize,
    serial_scratch_bytes: i64,
    parallel_extra_scratch_bytes: i64,
    os_peak_working_set_bytes: Option<usize>,
}

fn measure_memory(
    base: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    setup: SetupMeasurement,
) -> MemoryMeasurement {
    let storage = base.storage().expect("hybrid storage exists");
    let population = storage.len();
    let graph_csr_bytes = setup.graph_heap_bytes.max(0) as usize;
    let projection_bytes = setup.projection_heap_bytes.max(0) as usize;
    let membership_bytes = membership_capacity_bytes(population);
    let measured_scratch = context_memory(
        base,
        config,
        pool,
        graph_csr_bytes,
        projection_bytes,
        population,
    );
    let os_peak_working_set_bytes = sampled_peak_working_set(|| {
        warm_serial_context(base, config);
        warm_parallel_context(base, config, pool);
    });
    MemoryMeasurement {
        soa_bytes: soa_capacity_bytes(storage),
        graph_csr_bytes,
        projection_bytes,
        membership_bytes,
        serial_scratch_bytes: measured_scratch.serial_scratch_bytes,
        parallel_extra_scratch_bytes: measured_scratch.parallel_extra_scratch_bytes,
        os_peak_working_set_bytes,
    }
}

fn report_setup(setup: SetupMeasurement, population: u64, degree: u32, settlements: u32) {
    emit(&format!(
        "SCALE_SETUP|N={population}|K={settlements}|k={degree}|initialize_us={:.3}|graph_us={:.3}|projection_us={:.3}|graph_csr_bytes={}|projection_bytes={}",
        micros(setup.initialization),
        micros(setup.graph),
        micros(setup.projection),
        setup.graph_heap_bytes,
        setup.projection_heap_bytes,
    ));
}

fn report_memory(memory: MemoryMeasurement, population: u64, degree: u32, settlements: u32) {
    let serial_total = memory.soa_bytes as i64
        + memory.graph_csr_bytes as i64
        + memory.projection_bytes as i64
        + memory.membership_bytes as i64
        + memory.serial_scratch_bytes;
    let parallel_total = serial_total + memory.parallel_extra_scratch_bytes;
    emit(&format!(
        "SCALE_MEMORY|N={population}|K={settlements}|k={degree}|soa_vec_capacity_bytes={}|graph_csr_vec_bytes={}|projection_vec_bytes={}|membership_vec_bytes={}|serial_scratch_retained_bytes={}|parallel_phase6b_extra_scratch_bytes={}|serial_total_bytes={serial_total}|parallel_total_bytes={parallel_total}|os_sampled_peak_working_set_bytes={}",
        memory.soa_bytes,
        memory.graph_csr_bytes,
        memory.projection_bytes,
        memory.membership_bytes,
        memory.serial_scratch_bytes,
        memory.parallel_extra_scratch_bytes,
        memory
            .os_peak_working_set_bytes
            .map_or_else(|| "unavailable".to_string(), |bytes| bytes.to_string()),
    ));
}

#[derive(Clone, Copy)]
struct Digests {
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

fn digests(
    world: &HybridWorldState,
    graph: &RelationshipGraph,
    outcomes: &[M3DayOutcome],
) -> Digests {
    let storage = world.storage().expect("hybrid storage exists");
    let metrics = outcomes
        .iter()
        .map(|outcome| outcome.metrics.clone().expect("metrics enabled"))
        .collect::<Vec<_>>();
    let events = outcomes
        .iter()
        .flat_map(|outcome| outcome.events.iter().cloned())
        .collect::<Vec<_>>();
    Digests {
        state: canonical_m3_state_hash_from_storage(
            storage,
            world.world.current_day,
            &world.world.settlements,
            graph,
        )
        .expect("M3 storage state digest succeeds"),
        metrics: canonical_m3_metrics_hash(&metrics).expect("M3 metrics digest succeeds"),
        events: canonical_event_hash(&events).expect("event digest succeeds"),
    }
}

fn emit(line: &str) {
    println!("{line}");
    io::stdout().flush().expect("benchmark output flushes");
}

const PHASE_NAMES: [&str; 13] = [
    "P1_regrowth",
    "P2_degradation",
    "P3_features",
    "P4_action_selection",
    "P4_intent_projection",
    "P5_partition",
    "P6A_work_resolution",
    "P6B_total",
    "P7_market",
    "P8_welfare",
    "P9_mortality",
    "P10_metrics",
    "P11_events",
];

const P6B_STAGE_NAMES: [&str; 5] = [
    "validation_slot_lookup",
    "key_generation",
    "global_sort",
    "dependency_level_build",
    "evaluation_writeback_output",
];

#[derive(Clone, Copy)]
struct ProfileSample {
    phase_us: [f64; PHASE_NAMES.len()],
    p6b_stage_us: [f64; P6B_STAGE_NAMES.len()],
    settlement_scans: SettlementScanCounts,
    alive_agents: u64,
    keyed_entries: u64,
}

#[derive(Clone, Copy, Default)]
struct ScanTally {
    calls: usize,
    elements: usize,
}

#[derive(Clone, Copy, Default)]
struct SettlementScanCounts {
    phase3_lookup_calls: usize,
    phase3_lookup_comparisons: usize,
    phase3_index_build_entries: usize,
    phase3_index_sort_comparisons: usize,
    phase3_output_sorted_before_sort: bool,
    phase6a_find: ScanTally,
    phase6b_any: ScanTally,
    phase7_find: ScanTally,
    phase10_duplicate_any: ScanTally,
    phase11_group_find: ScanTally,
}

fn settlement_find_tally(
    settlements: &[sim_model::SettlementState],
    groups: &[GroupId],
) -> ScanTally {
    let mut tally = ScanTally::default();
    for &group_id in groups {
        tally.calls += 1;
        tally.elements += settlements
            .iter()
            .position(|settlement| settlement.group_id == group_id)
            .map_or(settlements.len(), |index| index + 1);
    }
    tally
}

fn phase11_group_find_tally(work_groups: &[GroupId], targeted_groups: &[GroupId]) -> ScanTally {
    let mut tally = ScanTally::default();
    let mut event_groups = Vec::with_capacity(work_groups.len() + targeted_groups.len());
    for &group_id in work_groups.iter().chain(targeted_groups) {
        tally.calls += 1;
        let index = event_groups.iter().position(|&seen| seen == group_id);
        let scanned = index.map_or(event_groups.len(), |index| index + 1);
        tally.elements += scanned;
        if index.is_none() {
            event_groups.push(group_id);
        }
    }
    tally
}

fn phase3_lookup_counts(
    storage: &SegmentedAgentStorage,
    scratch: &Phase3ScarcityScratch,
) -> (usize, usize, bool) {
    let mut calls = 0;
    let mut comparisons = 0;
    let mut previous = None;
    let mut sorted_before_sort = true;
    for slot in 0..storage.len() {
        if storage.demography.alive[slot] && storage.demography.health[slot] > 0.0 {
            let agent_id = storage.agent_ids[slot];
            if previous.is_some_and(|previous| previous > agent_id) {
                sorted_before_sort = false;
            }
            previous = Some(agent_id);
            calls += 1;
            comparisons += scratch.group_id_lookup_comparisons(storage.economy.group_id[slot]);
        }
    }
    (calls, comparisons, sorted_before_sort)
}

fn record_phase6_event_count(
    counts: &mut Vec<(u16, u64)>,
    slots: &mut HashMap<GroupId, usize>,
    group_id: GroupId,
    count: u64,
) -> u64 {
    if let Some(&slot) = slots.get(&group_id) {
        let previous = counts[slot].1;
        counts[slot].1 += count;
        previous
    } else {
        let slot = counts.len();
        counts.push((group_id.0, count));
        slots.insert(group_id, slot);
        0
    }
}

struct ManualProfile {
    world: WorldState,
    storage: SegmentedAgentStorage,
    graph: RelationshipGraph,
    run_context: M0RunContext,
    projection: M3Phase4SlotProjection,
    features: Vec<AgentFeatures>,
    choices: Vec<PrimaryActionChoice>,
    intents: Vec<Intent>,
    candidate_scratch: Vec<AgentId>,
    phase3_scratch: Phase3ScarcityScratch,
    phase5_scratch: Phase5PartitionScratch,
    phase6b_scratch: M3Phase6BStorageScratch,
    phase8_scratch: Phase8WelfareScratch,
}

impl ManualProfile {
    fn from_checkpoint(checkpoint: &HybridWorldState, config: &SimConfig) -> Self {
        let world = checkpoint.world.clone();
        let storage = checkpoint
            .storage()
            .expect("checkpoint has hybrid storage")
            .clone();
        let membership = memberships(&storage);
        let graph = RelationshipGraph::generate(
            &membership,
            MASTER_SEED,
            REPLICATE_ID,
            config.interaction.relationship_degree_k.unwrap() as u32,
            config.interaction.p_local.unwrap(),
        )
        .expect("profile relationship graph builds");
        let projection =
            M3Phase4SlotProjection::new(&graph, &storage).expect("profile slot projection builds");
        let population = storage.len();
        let settlement_count = world.settlements.len();
        let candidate_capacity = config.interaction.relationship_degree_k.unwrap() as usize;
        Self {
            world,
            storage,
            graph,
            run_context: M0RunContext::new(MASTER_SEED, REPLICATE_ID),
            projection,
            features: Vec::with_capacity(population),
            choices: Vec::with_capacity(population),
            intents: Vec::with_capacity(population),
            candidate_scratch: Vec::with_capacity(candidate_capacity),
            phase3_scratch: Phase3ScarcityScratch::with_capacity(settlement_count),
            phase5_scratch: Phase5PartitionScratch::with_capacity(settlement_count),
            phase6b_scratch: M3Phase6BStorageScratch::with_capacity(population),
            phase8_scratch: Phase8WelfareScratch::with_capacity(settlement_count),
        }
    }

    fn step(
        &mut self,
        config: &SimConfig,
        collect_settlement_scans: bool,
    ) -> (M3DayOutcome, ProfileSample) {
        let executed_day = self.world.current_day;
        let executed_day_number = executed_day.as_u32();
        let next_day = executed_day.as_u32().checked_add(1).expect("day fits u32");
        let mut effective = config.clone();
        effective.world.master_seed = self.run_context.master_seed;
        effective.world.replicate_id = self.run_context.replicate_id;
        let mut phase_us = [0.0; PHASE_NAMES.len()];

        let started = Instant::now();
        phase1_resource_regrowth(&mut self.world, &effective);
        phase_us[0] = micros(started.elapsed());

        let started = Instant::now();
        self.storage.phase2_degradation_with_config(&effective);
        phase_us[1] = micros(started.elapsed());

        let started = Instant::now();
        self.features.clear();
        phase3_observation_and_features_storage_with_scratch(
            &self.storage,
            &self.world.settlements,
            &effective,
            &mut self.features,
            &mut self.phase3_scratch,
        )
        .expect("profile phase 3 succeeds");
        phase_us[2] = micros(started.elapsed());
        let phase3_scan_snapshot = collect_settlement_scans.then(|| {
            (
                phase3_lookup_counts(&self.storage, &self.phase3_scratch),
                self.phase3_scratch.last_index_build_entries(),
                self.phase3_scratch.last_index_sort_comparisons(),
            )
        });

        let started = Instant::now();
        self.choices.clear();
        phase4_primary_action_selection_storage_into(
            &self.storage,
            executed_day,
            &effective,
            &self.features,
            &mut self.choices,
        )
        .expect("profile phase 4 selection succeeds");
        phase_us[3] = micros(started.elapsed());

        let started = Instant::now();
        self.intents.clear();
        generate_intents_m3_storage_into(
            &self.storage,
            executed_day,
            &effective,
            &self.choices,
            &self.projection,
            &mut self.candidate_scratch,
            &mut self.intents,
        )
        .expect("profile phase 4 intent generation succeeds");
        phase_us[4] = micros(started.elapsed());
        let cross_partition =
            count_cross_partition_interactions_storage_per_day(&self.intents, &self.storage)
                .expect("cross-partition count succeeds");

        let started = Instant::now();
        let partitions = phase5_partition_intents_from_vec_with_scratch(
            &mut self.intents,
            &mut self.phase5_scratch,
        )
        .expect("profile phase 5 succeeds");
        phase_us[5] = micros(started.elapsed());

        let started = Instant::now();
        let work_resolutions = phase6a_work_resolution_storage(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
        )
        .expect("profile phase 6A succeeds");
        phase_us[6] = micros(started.elapsed());

        let started = Instant::now();
        let (targeted_resolutions, stage_timings) =
            phase6b_targeted_resolution_storage_m3_with_timing(
                &mut self.storage,
                &self.world.settlements,
                executed_day,
                &effective,
                &partitions,
                &mut self.phase6b_scratch,
            )
            .expect("profile phase 6B succeeds");
        phase_us[7] = micros(started.elapsed());
        let p6b_stage_us = [
            micros(stage_timings.validation_slot_lookup),
            micros(stage_timings.key_generation),
            micros(stage_timings.global_sort),
            micros(stage_timings.dependency_level_build),
            micros(stage_timings.evaluation_writeback_output),
        ];

        let started = Instant::now();
        let market_resolutions = phase7_market_clearance_storage_with_config(
            &mut self.storage,
            &mut self.world.settlements,
            &partitions,
            &effective.economy,
        )
        .expect("profile phase 7 succeeds");
        phase_us[8] = micros(started.elapsed());

        let started = Instant::now();
        let welfare_resolutions = phase8_welfare_distribution_storage_with_config_and_scratch(
            &mut self.storage,
            &mut self.world.settlements,
            &effective,
            &mut self.phase8_scratch,
        )
        .expect("profile phase 8 succeeds");
        phase_us[9] = micros(started.elapsed());

        let started = Instant::now();
        let mortality_resolution = phase9_mortality_commitment_storage(&mut self.storage)
            .expect("profile phase 9 succeeds");
        phase_us[10] = micros(started.elapsed());

        let event_build_started = Instant::now();
        let mut event_buffer = EventBuffer::with_capacity(
            self.storage.len().saturating_mul(2) + self.world.settlements.len() + 4,
        );
        let mut phase6_counts: Vec<(u16, u64)> = Vec::with_capacity(work_resolutions.len());
        let mut phase6_count_slots = HashMap::with_capacity(
            work_resolutions
                .len()
                .saturating_add(targeted_resolutions.len()),
        );
        for work in &work_resolutions {
            let work_events = events_from_work_resolution(executed_day_number, work);
            record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                work.group_id,
                work_events.len() as u64,
            );
            event_buffer.push_all(work_events);
        }
        for targeted in &targeted_resolutions {
            let mut targeted_events =
                events_from_targeted_resolution(executed_day_number, targeted);
            let offset = record_phase6_event_count(
                &mut phase6_counts,
                &mut phase6_count_slots,
                targeted.group_id,
                targeted_events.len() as u64,
            );
            for event in &mut targeted_events {
                event.key.local_sequence += offset;
            }
            event_buffer.push_all(targeted_events);
        }
        for market in &market_resolutions {
            event_buffer.push_all(events_from_market_resolution(executed_day_number, market));
        }
        for welfare in &welfare_resolutions {
            event_buffer.push_all(events_from_welfare_resolution(executed_day_number, welfare));
        }
        event_buffer.push_all(events_from_mortality_resolution(
            executed_day_number,
            &mortality_resolution,
        ));
        let event_build_us = micros(event_build_started.elapsed());

        let started = Instant::now();
        let base_metrics = self
            .storage
            .phase10_metrics(&self.world.settlements, executed_day.as_u32())
            .expect("profile phase 10 succeeds");
        event_buffer.push(event_from_daily_metrics(&base_metrics));
        phase_us[11] = micros(started.elapsed());
        let (cross_partition_edge_count, total_directed_edge_count) =
            self.graph.edge_ratio_counts();
        let metrics = M3DailyMetrics {
            metrics: base_metrics,
            cross_partition_edge_count,
            total_directed_edge_count,
            cross_partition_interaction_count: cross_partition,
        };

        let started = Instant::now();
        let events = phase11_flush_events(&mut event_buffer).expect("profile phase 11 succeeds");
        let event_flush_us = micros(started.elapsed());
        phase_us[12] = event_build_us + event_flush_us;
        self.world.current_day = SimulationDay(next_day);

        let settlement_scans = if collect_settlement_scans {
            let partition_groups: Vec<_> = partitions
                .iter()
                .map(|partition| partition.group_id)
                .collect();
            let (phase3_lookup, phase3_index_build_entries, phase3_index_sort_comparisons) =
                phase3_scan_snapshot.expect("Phase 3 scan snapshot was collected");
            let phase10_calls = self.world.settlements.len();
            let phase10_elements = (0..phase10_calls).sum();
            let work_groups: Vec<_> = work_resolutions
                .iter()
                .map(|resolution| resolution.group_id)
                .collect();
            let targeted_groups: Vec<_> = targeted_resolutions
                .iter()
                .map(|resolution| resolution.group_id)
                .collect();
            let phase6a_one_pass =
                settlement_find_tally(&self.world.settlements, &partition_groups);
            let phase7_one_pass = settlement_find_tally(&self.world.settlements, &partition_groups);
            SettlementScanCounts {
                phase3_lookup_calls: phase3_lookup.0,
                phase3_lookup_comparisons: phase3_lookup.1,
                phase3_index_build_entries,
                phase3_index_sort_comparisons,
                phase3_output_sorted_before_sort: phase3_lookup.2,
                phase6a_find: ScanTally {
                    calls: phase6a_one_pass.calls * 2,
                    elements: phase6a_one_pass.elements * 2,
                },
                phase6b_any: settlement_find_tally(&self.world.settlements, &partition_groups),
                phase7_find: ScanTally {
                    calls: phase7_one_pass.calls * 2,
                    elements: phase7_one_pass.elements * 2,
                },
                phase10_duplicate_any: ScanTally {
                    calls: phase10_calls,
                    elements: phase10_elements,
                },
                phase11_group_find: phase11_group_find_tally(&work_groups, &targeted_groups),
            }
        } else {
            SettlementScanCounts::default()
        };
        let alive_agents = metrics.metrics.population as u64;
        let keyed_entries = targeted_resolutions
            .iter()
            .map(|resolution| resolution.keyed_stream.len() as u64)
            .sum();

        (
            M3DayOutcome {
                executed_day: executed_day.as_u32(),
                metrics: Some(metrics),
                events,
                snapshot: None,
            },
            ProfileSample {
                phase_us,
                p6b_stage_us,
                settlement_scans,
                alive_agents,
                keyed_entries,
            },
        )
    }
}

fn micros(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000_000.0
}

#[derive(Clone, Copy)]
struct Summary {
    median: f64,
    mad: f64,
}

fn summarize(values: &mut [f64]) -> Summary {
    values.sort_by(f64::total_cmp);
    let median = values[values.len() / 2];
    let mut deviations: Vec<_> = values.iter().map(|value| (value - median).abs()).collect();
    deviations.sort_by(f64::total_cmp);
    Summary {
        median,
        mad: deviations[deviations.len() / 2],
    }
}

#[derive(Clone, Copy)]
struct PhaseProfile {
    phases: [Summary; PHASE_NAMES.len()],
    p6b_stages: [Summary; P6B_STAGE_NAMES.len()],
    mean_alive_agents: f64,
    mean_keyed_entries: f64,
}

fn profile_serial_cell(checkpoint: &HybridWorldState, config: &SimConfig) -> PhaseProfile {
    profile_serial_cell_with_samples(checkpoint, config, WARMUPS, SAMPLES)
}

fn profile_serial_cell_with_samples(
    checkpoint: &HybridWorldState,
    config: &SimConfig,
    warmups: usize,
    samples: usize,
) -> PhaseProfile {
    let mut phase_values: [Vec<f64>; PHASE_NAMES.len()] = array::from_fn(|_| Vec::new());
    let mut p6b_values: [Vec<f64>; P6B_STAGE_NAMES.len()] = array::from_fn(|_| Vec::new());
    let mut alive_total = 0u64;
    let mut keyed_total = 0u64;
    let mut measurement_days = 0u64;
    let options = options();

    for sample_index in 0..(warmups + samples) {
        let mut manual = ManualProfile::from_checkpoint(checkpoint, config);
        let mut runner_world = checkpoint.clone();
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut runner_context = M3StorageRunContext::new(&runner_world, config, &run_context)
            .expect("serial profile oracle context builds");

        for _ in 0..DAYS_PER_SAMPLE {
            let (manual_outcome, profile) = manual.step(config, false);
            let runner_outcomes =
                run_m3_storage_days(&mut runner_world, config, &mut runner_context, 1, &options)
                    .expect("serial profile oracle day succeeds");
            assert_eq!(runner_outcomes.len(), 1);
            assert_eq!(manual_outcome, runner_outcomes[0]);

            let manual_state = canonical_m3_state_hash_from_storage(
                &manual.storage,
                manual.world.current_day,
                &manual.world.settlements,
                &manual.graph,
            )
            .expect("manual profile state hash succeeds");
            let runner_state = canonical_m3_state_hash_from_storage(
                runner_world.storage().expect("oracle storage exists"),
                runner_world.world.current_day,
                &runner_world.world.settlements,
                runner_context.relationship_graph(),
            )
            .expect("runner profile state hash succeeds");
            assert_eq!(manual_state, runner_state);

            if sample_index >= warmups {
                for (values, value) in phase_values.iter_mut().zip(profile.phase_us) {
                    values.push(value);
                }
                for (values, value) in p6b_values.iter_mut().zip(profile.p6b_stage_us) {
                    values.push(value);
                }
                alive_total += profile.alive_agents;
                keyed_total += profile.keyed_entries;
                measurement_days += 1;
            }
        }
    }

    PhaseProfile {
        phases: phase_values.each_mut().map(|values| summarize(values)),
        p6b_stages: p6b_values.each_mut().map(|values| summarize(values)),
        mean_alive_agents: alive_total as f64 / measurement_days as f64,
        mean_keyed_entries: keyed_total as f64 / measurement_days as f64,
    }
}

fn m3_day_digests(
    world: &HybridWorldState,
    graph: &RelationshipGraph,
    outcomes: &[M3DayOutcome],
) -> Digests {
    digests(world, graph, outcomes)
}

struct PairedTiming {
    serial: f64,
    parallel: f64,
    digests: Digests,
}

fn paired_timing(
    checkpoint: &HybridWorldState,
    config: &SimConfig,
    pool: &rayon::ThreadPool,
    sample_index: usize,
) -> PairedTiming {
    let mut serial_world = checkpoint.clone();
    let mut parallel_world = checkpoint.clone();
    let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut serial_context = M3StorageRunContext::new(&serial_world, config, &run_context)
        .expect("serial paired context builds");
    let mut parallel_context =
        M3ParallelStorageRunContext::new(&parallel_world, config, &run_context)
            .expect("parallel paired context builds");
    let options = options();
    let chunk_size = NonZeroUsize::new(CHUNK_SIZE).expect("chunk size is nonzero");

    let (serial_us, serial_digests, parallel_us, parallel_digests) =
        if sample_index.is_multiple_of(2) {
            let started = Instant::now();
            let outcomes = run_m3_storage_days(
                &mut serial_world,
                config,
                &mut serial_context,
                DAYS_PER_SAMPLE,
                &options,
            )
            .expect("paired serial sample succeeds");
            let elapsed = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
            let digests = m3_day_digests(
                &serial_world,
                serial_context.relationship_graph(),
                &outcomes,
            );
            drop(outcomes);

            let started = Instant::now();
            let outcomes = run_m3_storage_days_with_parallel_phase6b(
                &mut parallel_world,
                config,
                &mut parallel_context,
                DAYS_PER_SAMPLE,
                &options,
                pool,
                chunk_size,
            )
            .expect("paired parallel sample succeeds");
            let elapsed_parallel = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
            let parallel_digest = m3_day_digests(
                &parallel_world,
                parallel_context.relationship_graph(),
                &outcomes,
            );
            (elapsed, digests, elapsed_parallel, parallel_digest)
        } else {
            let started = Instant::now();
            let outcomes = run_m3_storage_days_with_parallel_phase6b(
                &mut parallel_world,
                config,
                &mut parallel_context,
                DAYS_PER_SAMPLE,
                &options,
                pool,
                chunk_size,
            )
            .expect("paired parallel sample succeeds");
            let elapsed_parallel = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
            let parallel_digest = m3_day_digests(
                &parallel_world,
                parallel_context.relationship_graph(),
                &outcomes,
            );
            drop(outcomes);

            let started = Instant::now();
            let outcomes = run_m3_storage_days(
                &mut serial_world,
                config,
                &mut serial_context,
                DAYS_PER_SAMPLE,
                &options,
            )
            .expect("paired serial sample succeeds");
            let elapsed = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
            let digests = m3_day_digests(
                &serial_world,
                serial_context.relationship_graph(),
                &outcomes,
            );
            (elapsed, digests, elapsed_parallel, parallel_digest)
        };

    assert_eq!(serial_digests.state, parallel_digests.state);
    assert_eq!(serial_digests.metrics, parallel_digests.metrics);
    assert_eq!(serial_digests.events, parallel_digests.events);
    PairedTiming {
        serial: serial_us,
        parallel: parallel_us,
        digests: serial_digests,
    }
}

struct CellMeasurements {
    population: u64,
    degree: u32,
    horizon: u32,
    serial_full_tick: Summary,
    parallel_full_tick: Summary,
    phase_profile: PhaseProfile,
    final_digests: Digests,
}

fn measure_cell(
    population: u64,
    degree: u32,
    horizon: u32,
    config: &SimConfig,
    checkpoint: &HybridWorldState,
    pool: &rayon::ThreadPool,
) -> CellMeasurements {
    emit(&format!(
        "SCALE_CELL_BEGIN|N={population}|K={}|k={degree}|p_local={P_LOCAL}|horizon={horizon}",
        config.world.settlement_count
    ));
    let phase_profile = profile_serial_cell(checkpoint, config);
    let mut serial_samples = Vec::with_capacity(SAMPLES);
    let mut parallel_samples = Vec::with_capacity(SAMPLES);
    let mut final_digests: Option<Digests> = None;
    for sample_index in 0..(WARMUPS + SAMPLES) {
        let pair = paired_timing(checkpoint, config, pool, sample_index);
        if let Some(previous) = final_digests {
            assert_eq!(previous.state, pair.digests.state);
            assert_eq!(previous.metrics, pair.digests.metrics);
            assert_eq!(previous.events, pair.digests.events);
        } else {
            final_digests = Some(pair.digests);
        }
        if sample_index >= WARMUPS {
            serial_samples.push(pair.serial);
            parallel_samples.push(pair.parallel);
        }
    }
    let final_digests = final_digests.expect("at least one paired sample");
    CellMeasurements {
        population,
        degree,
        horizon,
        serial_full_tick: summarize(&mut serial_samples),
        parallel_full_tick: summarize(&mut parallel_samples),
        phase_profile,
        final_digests,
    }
}

fn report_cell(cell: &CellMeasurements) {
    let serial_speed_per_agent = cell.serial_full_tick.median / cell.population as f64;
    let speedup = cell.serial_full_tick.median / cell.parallel_full_tick.median;
    emit(&format!(
        "SCALE_FULL_TICK|N={}|K={}|k={}|p_local={P_LOCAL}|horizon={}|serial_median_us={:.3}|serial_mad_us={:.3}|serial_us_per_agent={serial_speed_per_agent:.6}|parallel_median_us={:.3}|parallel_mad_us={:.3}|parallel_us_per_agent={:.6}|speedup={speedup:.4}",
        cell.population,
        cell.population / 200,
        cell.degree,
        cell.horizon,
        cell.serial_full_tick.median,
        cell.serial_full_tick.mad,
        cell.parallel_full_tick.median,
        cell.parallel_full_tick.mad,
        cell.parallel_full_tick.median / cell.population as f64,
    ));
    for (name, summary) in PHASE_NAMES.iter().zip(cell.phase_profile.phases) {
        emit(&format!(
            "SCALE_PHASE|N={}|K={}|k={}|horizon={}|phase={name}|median_us={:.3}|mad_us={:.3}|share_of_serial_tick_pct={:.3}",
            cell.population,
            cell.population / 200,
            cell.degree,
            cell.horizon,
            summary.median,
            summary.mad,
            summary.median / cell.serial_full_tick.median * 100.0,
        ));
    }
    for (name, summary) in P6B_STAGE_NAMES.iter().zip(cell.phase_profile.p6b_stages) {
        emit(&format!(
            "SCALE_P6B_STAGE|N={}|K={}|k={}|horizon={}|stage={name}|median_us={:.3}|mad_us={:.3}|share_of_serial_tick_pct={:.3}",
            cell.population,
            cell.population / 200,
            cell.degree,
            cell.horizon,
            summary.median,
            summary.mad,
            summary.median / cell.serial_full_tick.median * 100.0,
        ));
    }
    emit(&format!(
        "SCALE_PARITY|N={}|K={}|k={}|horizon={}|state={}|metrics={}|events={}|serial_parallel_equal=true",
        cell.population,
        cell.population / 200,
        cell.degree,
        cell.horizon,
        cell.final_digests.state,
        cell.final_digests.metrics,
        cell.final_digests.events,
    ));
}

fn alpha(lower_population: u64, lower_time: f64, upper_population: u64, upper_time: f64) -> f64 {
    libm::log(upper_time / lower_time)
        / libm::log(upper_population as f64 / lower_population as f64)
}

fn measured_alpha(
    lower_population: u64,
    lower_time: f64,
    upper_population: u64,
    upper_time: f64,
) -> Option<f64> {
    (lower_time > 0.0 && upper_time > 0.0)
        .then(|| alpha(lower_population, lower_time, upper_population, upper_time))
}

fn report_alpha(results: &[CellMeasurements]) -> Vec<(u64, u32, u32, u64, String, f64)> {
    let mut report = Vec::new();
    for degree in DEGREES {
        for horizon in CHECKPOINT_DAYS {
            let mut cells: Vec<_> = results
                .iter()
                .filter(|cell| cell.degree == degree && cell.horizon == horizon)
                .collect();
            cells.sort_by_key(|cell| cell.population);
            for adjacent in cells.windows(2) {
                let lower = adjacent[0];
                let upper = adjacent[1];
                let tick_alpha = alpha(
                    lower.population,
                    lower.serial_full_tick.median,
                    upper.population,
                    upper.serial_full_tick.median,
                );
                emit(&format!(
                    "SCALE_ALPHA|k={degree}|horizon={horizon}|phase=full_tick|from_N={}|to_N={}|alpha={tick_alpha:.5}",
                    lower.population, upper.population
                ));
                report.push((
                    degree as u64,
                    horizon,
                    lower.population as u32,
                    upper.population,
                    "full_tick".to_string(),
                    tick_alpha,
                ));
                for (phase_index, phase_name) in PHASE_NAMES.iter().enumerate() {
                    let value = alpha(
                        lower.population,
                        lower.phase_profile.phases[phase_index].median,
                        upper.population,
                        upper.phase_profile.phases[phase_index].median,
                    );
                    emit(&format!(
                        "SCALE_ALPHA|k={degree}|horizon={horizon}|phase={phase_name}|from_N={}|to_N={}|alpha={value:.5}",
                        lower.population, upper.population
                    ));
                    report.push((
                        degree as u64,
                        horizon,
                        lower.population as u32,
                        upper.population,
                        (*phase_name).to_string(),
                        value,
                    ));
                }
                for (stage_index, stage_name) in P6B_STAGE_NAMES.iter().enumerate() {
                    let Some(value) = measured_alpha(
                        lower.population,
                        lower.phase_profile.p6b_stages[stage_index].median,
                        upper.population,
                        upper.phase_profile.p6b_stages[stage_index].median,
                    ) else {
                        emit(&format!(
                            "SCALE_ALPHA|k={degree}|horizon={horizon}|phase=P6B_{stage_name}|from_N={}|to_N={}|alpha=not_applicable_serial_stage",
                            lower.population, upper.population
                        ));
                        continue;
                    };
                    emit(&format!(
                        "SCALE_ALPHA|k={degree}|horizon={horizon}|phase=P6B_{stage_name}|from_N={}|to_N={}|alpha={value:.5}",
                        lower.population, upper.population
                    ));
                    report.push((
                        degree as u64,
                        horizon,
                        lower.population as u32,
                        upper.population,
                        format!("P6B_{stage_name}"),
                        value,
                    ));
                }
            }
        }
    }
    report
}

fn report_first_breaks(
    results: &[CellMeasurements],
    setups: &[(u64, u32, SetupMeasurement, MemoryMeasurement)],
    alphas: &[(u64, u32, u32, u64, String, f64)],
    host_ram_bytes: u64,
) {
    let mut found = false;
    for (degree, horizon, from_n, to_n, phase, value) in alphas {
        if *value > 1.15 {
            found = true;
            emit(&format!(
                "SCALE_FIRST_BREAK|criterion=phase_alpha_gt_1.15|k={degree}|horizon={horizon}|phase={phase}|from_N={from_n}|to_N={to_n}|alpha={value:.5}"
            ));
        }
    }
    for degree in DEGREES {
        for horizon in CHECKPOINT_DAYS {
            let Some(baseline) = results.iter().find(|cell| {
                cell.population == 100_000 && cell.degree == degree && cell.horizon == horizon
            }) else {
                continue;
            };
            let baseline_per_agent = baseline.serial_full_tick.median / baseline.population as f64;
            for cell in results.iter().filter(|cell| {
                cell.degree == degree && cell.horizon == horizon && cell.population > 100_000
            }) {
                let per_agent = cell.serial_full_tick.median / cell.population as f64;
                let increase = per_agent / baseline_per_agent - 1.0;
                if increase > 0.25 {
                    found = true;
                    emit(&format!(
                        "SCALE_FIRST_BREAK|criterion=per_agent_tick_gt_25pct|k={degree}|horizon={horizon}|N={}|increase_pct={:.3}",
                        cell.population,
                        increase * 100.0
                    ));
                }
            }
        }
    }
    for (population, degree, setup, _) in setups {
        let Some(day0) = results.iter().find(|cell| {
            cell.population == *population && cell.degree == *degree && cell.horizon == 0
        }) else {
            continue;
        };
        let ticks = (micros(setup.graph + setup.projection)) / day0.serial_full_tick.median;
        if ticks > 10.0 {
            found = true;
            emit(&format!(
                "SCALE_FIRST_BREAK|criterion=graph_projection_setup_gt_10_ticks|N={population}|k={degree}|tick_equivalents={ticks:.4}"
            ));
        }
    }
    let memory_limit = host_ram_bytes / 2;
    for (population, degree, setup, memory) in setups {
        let serial_total = memory.soa_bytes
            + memory.graph_csr_bytes
            + memory.projection_bytes
            + memory.membership_bytes
            + memory.serial_scratch_bytes.max(0) as usize;
        let parallel_total = serial_total + memory.parallel_extra_scratch_bytes.max(0) as usize;
        let measured_os = memory.os_peak_working_set_bytes.unwrap_or(0);
        let used = serial_total.max(parallel_total).max(measured_os);
        if used as u64 > memory_limit {
            found = true;
            emit(&format!(
                "SCALE_FIRST_BREAK|criterion=memory_gt_50pct_host_ram|N={population}|k={degree}|used_bytes={used}|host_ram_bytes={host_ram_bytes}|graph_projection_us={:.3}",
                micros(setup.graph + setup.projection)
            ));
        }
    }
    if !found {
        emit("SCALE_FIRST_BREAK|none_through_N=1000000|runtime_or_capacity_errors=none");
    }
}

#[derive(Clone, Copy)]
struct DiagnosticCell {
    population: u64,
    settlements: u32,
    profile: PhaseProfile,
}

fn config_for_diagnostic(population: u64, settlements: u32) -> SimConfig {
    let mut config = config_for(population, 8);
    config.world.settlement_count = settlements;
    config.validate().expect("diagnostic config validates");
    let smallest_group = population / u64::from(settlements);
    let largest_group = population.div_ceil(u64::from(settlements));
    assert!(smallest_group > 8, "local capacity supports k=8");
    assert!(
        population - largest_group >= 8,
        "cross capacity supports k=8"
    );
    config
}

fn profile_diagnostic_cell(population: u64, settlements: u32, series: &str) -> DiagnosticCell {
    let config = config_for_diagnostic(population, settlements);
    let (base, _) = initialize(&config);
    let profile = profile_serial_cell(&base, &config);
    let cell = DiagnosticCell {
        population,
        settlements,
        profile,
    };
    report_diagnostic_phases(series, &cell);
    cell
}

fn report_diagnostic_phases(series: &str, cell: &DiagnosticCell) {
    for (name, summary) in PHASE_NAMES.iter().zip(cell.profile.phases) {
        emit(&format!(
            "DIAG_PHASE|series={series}|N={}|K={}|phase={name}|median_us={:.3}|mad_us={:.3}",
            cell.population, cell.settlements, summary.median, summary.mad
        ));
    }
}

fn report_scaled_population_alphas(cells: &[DiagnosticCell]) {
    for adjacent in cells.windows(2) {
        for (index, name) in PHASE_NAMES.iter().enumerate() {
            let lower = adjacent[0].profile.phases[index].median;
            let upper = adjacent[1].profile.phases[index].median;
            emit(&format!(
                "DIAG_ALPHA_N|series=K=N/200|from_N={}|from_K={}|to_N={}|to_K={}|phase={name}|alpha={:.5}",
                adjacent[0].population,
                adjacent[0].settlements,
                adjacent[1].population,
                adjacent[1].settlements,
                alpha(adjacent[0].population, lower, adjacent[1].population, upper)
            ));
        }
    }
}

fn report_n_alphas(cells: &[DiagnosticCell]) {
    for adjacent in cells.windows(2) {
        for (index, name) in PHASE_NAMES.iter().enumerate() {
            let lower = adjacent[0].profile.phases[index].median;
            let upper = adjacent[1].profile.phases[index].median;
            emit(&format!(
                "DIAG_ALPHA_N|K={}|phase={name}|from_N={}|to_N={}|alpha={:.5}",
                adjacent[0].settlements,
                adjacent[0].population,
                adjacent[1].population,
                alpha(adjacent[0].population, lower, adjacent[1].population, upper)
            ));
        }
    }
}

fn report_k_times(cells: &[DiagnosticCell]) {
    for cell in cells {
        for (name, summary) in PHASE_NAMES.iter().zip(cell.profile.phases) {
            emit(&format!(
                "DIAG_TIME_K|N={}|K={}|phase={name}|median_us={:.3}|mad_us={:.3}",
                cell.population, cell.settlements, summary.median, summary.mad
            ));
        }
    }
}

fn report_lookup_counts(population: u64, settlements: u32, config: &SimConfig) {
    let (base, _) = initialize(config);
    let mut manual = ManualProfile::from_checkpoint(&base, config);
    let (_, sample) = manual.step(config, true);
    let scans = sample.settlement_scans;
    emit(&format!(
        "DIAG_LOOKUPS|N={population}|K={settlements}|partition_count={}|after_phase3_index_queries={}|after_phase3_group_comparisons={}|after_phase3_index_build_entries={}|after_phase3_index_sort_comparisons={}|phase3_out_sorted_before_sort={}|before_phase6a_find_calls={}|before_phase6a_elements={}|after_phase6a_index_queries={}|before_phase6b_any_calls={}|before_phase6b_elements={}|after_phase6b_index_queries={}|before_phase7_find_calls={}|before_phase7_elements={}|after_phase7_index_queries={}|before_phase10_duplicate_any_calls={}|before_phase10_elements={}|after_phase10_duplicate_flag_checks={}|before_phase11_group_find_calls={}|before_phase11_elements={}|after_phase11_map_lookups={}|after_phase11_max_distinct_group_keys={}",
        scans.phase6a_find.calls / 2,
        scans.phase3_lookup_calls,
        scans.phase3_lookup_comparisons,
        scans.phase3_index_build_entries,
        scans.phase3_index_sort_comparisons,
        scans.phase3_output_sorted_before_sort,
        scans.phase6a_find.calls,
        scans.phase6a_find.elements,
        scans.phase6a_find.calls,
        scans.phase6b_any.calls,
        scans.phase6b_any.elements,
        scans.phase6b_any.calls,
        scans.phase7_find.calls,
        scans.phase7_find.elements,
        scans.phase7_find.calls,
        scans.phase10_duplicate_any.calls,
        scans.phase10_duplicate_any.elements,
        settlements,
        scans.phase11_group_find.calls,
        scans.phase11_group_find.elements,
        scans.phase11_group_find.calls,
        scans.phase6a_find.calls / 2,
    ));
}

fn phase3_subphase_profile(
    storage: &SegmentedAgentStorage,
    config: &SimConfig,
    settlements: &[sim_model::SettlementState],
) -> ([Summary; 4], bool) {
    let mut values: [Vec<f64>; 4] = array::from_fn(|_| Vec::new());
    let mut timed_features = Vec::with_capacity(storage.len());
    let mut plain_features = Vec::with_capacity(storage.len());
    let mut timed_scratch = Phase3ScarcityScratch::with_capacity(settlements.len());
    let mut plain_scratch = Phase3ScarcityScratch::with_capacity(settlements.len());
    let output_sorted_before_sort = storage.agent_ids().windows(2).all(|w| w[0] <= w[1]);

    for sample_index in 0..(WARMUPS + SAMPLES) {
        for _ in 0..DAYS_PER_SAMPLE {
            let started = Instant::now();
            let timings = phase3_observation_and_features_storage_with_timing(
                storage,
                settlements,
                config,
                &mut timed_features,
                &mut timed_scratch,
            )
            .expect("timed Phase 3 succeeds");
            let total = micros(started.elapsed());
            if sample_index >= WARMUPS {
                values[0].push(micros(timings.scarcity_prepare));
                values[1].push(micros(timings.agent_loop));
                values[2].push(micros(timings.final_sort));
                values[3].push(total);
            }
        }
        phase3_observation_and_features_storage_with_scratch(
            storage,
            settlements,
            config,
            &mut plain_features,
            &mut plain_scratch,
        )
        .expect("untimed Phase 3 succeeds");
        assert_eq!(
            timed_features, plain_features,
            "timed and plain Phase 3 parity"
        );
    }

    (
        values.each_mut().map(|stage| summarize(stage)),
        output_sorted_before_sort,
    )
}

fn diagnose_phase3() {
    for population in [250_000, 1_000_000] {
        let config = config_for(population, 8);
        let (base, _) = initialize(&config);
        let storage = base.storage().expect("diagnostic storage exists");
        let (profile, presorted) =
            phase3_subphase_profile(storage, &config, &base.world.settlements);
        for (stage, summary) in [
            "scarcity_prepare",
            "agent_loop",
            "final_sort",
            "timed_phase_total",
        ]
        .into_iter()
        .zip(profile)
        {
            emit(&format!(
                "DIAG_P3|N={population}|K={}|stage={stage}|median_us={:.3}|mad_us={:.3}|out_sorted_before_sort={presorted}",
                config.world.settlement_count, summary.median, summary.mad
            ));
        }
    }
}

fn p6b_stage_values(timings: sim_model::resolution::M3Phase6BStageTimings) -> [f64; 5] {
    [
        micros(timings.validation_slot_lookup),
        micros(timings.key_generation),
        micros(timings.global_sort),
        micros(timings.dependency_level_build),
        micros(timings.evaluation_writeback_output),
    ]
}

fn diagnose_parallel_phase6b() {
    let population = 1_000_000;
    let degree = 16;
    let config = config_for(population, degree);
    let (checkpoint, _) = initialize(&config);
    let pool = ThreadPoolBuilder::new()
        .num_threads(POOL_THREADS)
        .build()
        .expect("diagnostic Rayon pool builds");
    pool.install(|| ());
    let chunk_size = NonZeroUsize::new(CHUNK_SIZE).expect("chunk size is nonzero");
    let options = options();
    let mut serial_values: [Vec<f64>; 5] = array::from_fn(|_| Vec::new());
    let mut parallel_values: [Vec<f64>; 5] = array::from_fn(|_| Vec::new());
    let mut serial_tick_values = Vec::new();
    let mut parallel_tick_values = Vec::new();

    for sample_index in 0..(WARMUPS + SAMPLES) {
        let mut serial_world = checkpoint.clone();
        let mut parallel_world = checkpoint.clone();
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut serial_context = M3StorageRunContext::new(&serial_world, &config, &run_context)
            .expect("serial diagnostic context builds");
        let mut parallel_context =
            M3ParallelStorageRunContext::new(&parallel_world, &config, &run_context)
                .expect("parallel diagnostic context builds");

        let (serial_digests, serial_timing, parallel_digests, parallel_timing) =
            if sample_index.is_multiple_of(2) {
                let (serial_outcomes, serial_timing) = run_m3_storage_days_with_phase6b_timing(
                    &mut serial_world,
                    &config,
                    &mut serial_context,
                    DAYS_PER_SAMPLE,
                    &options,
                )
                .expect("serial timed diagnostic run succeeds");
                let serial_digests = m3_day_digests(
                    &serial_world,
                    serial_context.relationship_graph(),
                    &serial_outcomes,
                );
                let (parallel_outcomes, parallel_timing) =
                    run_m3_storage_days_with_parallel_phase6b_timing(
                        &mut parallel_world,
                        &config,
                        &mut parallel_context,
                        DAYS_PER_SAMPLE,
                        &options,
                        &pool,
                        chunk_size,
                    )
                    .expect("parallel timed diagnostic run succeeds");
                let parallel_digests = m3_day_digests(
                    &parallel_world,
                    parallel_context.relationship_graph(),
                    &parallel_outcomes,
                );
                (
                    serial_digests,
                    serial_timing,
                    parallel_digests,
                    parallel_timing,
                )
            } else {
                let (parallel_outcomes, parallel_timing) =
                    run_m3_storage_days_with_parallel_phase6b_timing(
                        &mut parallel_world,
                        &config,
                        &mut parallel_context,
                        DAYS_PER_SAMPLE,
                        &options,
                        &pool,
                        chunk_size,
                    )
                    .expect("parallel timed diagnostic run succeeds");
                let parallel_digests = m3_day_digests(
                    &parallel_world,
                    parallel_context.relationship_graph(),
                    &parallel_outcomes,
                );
                let (serial_outcomes, serial_timing) = run_m3_storage_days_with_phase6b_timing(
                    &mut serial_world,
                    &config,
                    &mut serial_context,
                    DAYS_PER_SAMPLE,
                    &options,
                )
                .expect("serial timed diagnostic run succeeds");
                let serial_digests = m3_day_digests(
                    &serial_world,
                    serial_context.relationship_graph(),
                    &serial_outcomes,
                );
                (
                    serial_digests,
                    serial_timing,
                    parallel_digests,
                    parallel_timing,
                )
            };

        assert_eq!(serial_digests.state, parallel_digests.state);
        assert_eq!(serial_digests.metrics, parallel_digests.metrics);
        assert_eq!(serial_digests.events, parallel_digests.events);

        let mut serial_tick_world = checkpoint.clone();
        let mut parallel_tick_world = checkpoint.clone();
        let mut serial_tick_context =
            M3StorageRunContext::new(&serial_tick_world, &config, &run_context)
                .expect("serial untimed context builds");
        let mut parallel_tick_context =
            M3ParallelStorageRunContext::new(&parallel_tick_world, &config, &run_context)
                .expect("parallel untimed context builds");
        let (serial_tick_digests, serial_tick, parallel_tick_digests, parallel_tick) =
            if sample_index.is_multiple_of(2) {
                let started = Instant::now();
                let outcomes = run_m3_storage_days(
                    &mut serial_tick_world,
                    &config,
                    &mut serial_tick_context,
                    DAYS_PER_SAMPLE,
                    &options,
                )
                .expect("serial untimed diagnostic run succeeds");
                let serial_tick = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
                let serial_tick_digests = m3_day_digests(
                    &serial_tick_world,
                    serial_tick_context.relationship_graph(),
                    &outcomes,
                );
                let started = Instant::now();
                let outcomes = run_m3_storage_days_with_parallel_phase6b(
                    &mut parallel_tick_world,
                    &config,
                    &mut parallel_tick_context,
                    DAYS_PER_SAMPLE,
                    &options,
                    &pool,
                    chunk_size,
                )
                .expect("parallel untimed diagnostic run succeeds");
                let parallel_tick = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
                let parallel_tick_digests = m3_day_digests(
                    &parallel_tick_world,
                    parallel_tick_context.relationship_graph(),
                    &outcomes,
                );
                (
                    serial_tick_digests,
                    serial_tick,
                    parallel_tick_digests,
                    parallel_tick,
                )
            } else {
                let started = Instant::now();
                let outcomes = run_m3_storage_days_with_parallel_phase6b(
                    &mut parallel_tick_world,
                    &config,
                    &mut parallel_tick_context,
                    DAYS_PER_SAMPLE,
                    &options,
                    &pool,
                    chunk_size,
                )
                .expect("parallel untimed diagnostic run succeeds");
                let parallel_tick = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
                let parallel_tick_digests = m3_day_digests(
                    &parallel_tick_world,
                    parallel_tick_context.relationship_graph(),
                    &outcomes,
                );
                let started = Instant::now();
                let outcomes = run_m3_storage_days(
                    &mut serial_tick_world,
                    &config,
                    &mut serial_tick_context,
                    DAYS_PER_SAMPLE,
                    &options,
                )
                .expect("serial untimed diagnostic run succeeds");
                let serial_tick = micros(started.elapsed()) / f64::from(DAYS_PER_SAMPLE);
                let serial_tick_digests = m3_day_digests(
                    &serial_tick_world,
                    serial_tick_context.relationship_graph(),
                    &outcomes,
                );
                (
                    serial_tick_digests,
                    serial_tick,
                    parallel_tick_digests,
                    parallel_tick,
                )
            };
        assert_eq!(serial_tick_digests.state, parallel_tick_digests.state);
        assert_eq!(serial_tick_digests.metrics, parallel_tick_digests.metrics);
        assert_eq!(serial_tick_digests.events, parallel_tick_digests.events);
        if sample_index >= WARMUPS {
            serial_tick_values.push(serial_tick);
            parallel_tick_values.push(parallel_tick);
            for timing in serial_timing {
                for (values, elapsed) in serial_values.iter_mut().zip(p6b_stage_values(timing)) {
                    values.push(elapsed);
                }
            }
            for timing in parallel_timing {
                for (values, elapsed) in parallel_values.iter_mut().zip(p6b_stage_values(timing)) {
                    values.push(elapsed);
                }
            }
        }
    }

    for ((name, serial), parallel) in P6B_STAGE_NAMES
        .iter()
        .zip(serial_values.each_mut().map(|v| summarize(v)))
        .zip(parallel_values.each_mut().map(|v| summarize(v)))
    {
        emit(&format!(
            "DIAG_P6B_PARALLEL|N={population}|K={}|k={degree}|threads={POOL_THREADS}|chunk={CHUNK_SIZE}|stage={name}|serial_median_us={:.3}|serial_mad_us={:.3}|parallel_median_us={:.3}|parallel_mad_us={:.3}|speedup={:.4}",
            config.world.settlement_count,
            serial.median,
            serial.mad,
            parallel.median,
            parallel.mad,
            serial.median / parallel.median,
        ));
    }
    let serial_tick = summarize(&mut serial_tick_values);
    let parallel_tick = summarize(&mut parallel_tick_values);
    emit(&format!(
        "DIAG_FULL_TICK|N={population}|K={}|k={degree}|serial_median_us={:.3}|serial_mad_us={:.3}|parallel_median_us={:.3}|parallel_mad_us={:.3}|speedup={:.4}|parity=true",
        config.world.settlement_count,
        serial_tick.median,
        serial_tick.mad,
        parallel_tick.median,
        parallel_tick.mad,
        serial_tick.median / parallel_tick.median,
    ));
    emit("DIAG_P6B_PARITY|equal_state_metrics_events=true|samples=7|days_per_sample=3");
}

#[derive(Clone, Copy)]
struct ResidualCell {
    population: u64,
    profile: PhaseProfile,
}

fn residual_mad_ratio(summary: Summary) -> f64 {
    if summary.median > 0.0 {
        summary.mad / summary.median
    } else {
        f64::INFINITY
    }
}

fn residual_mad_percent(summary: Summary) -> String {
    if summary.median > 0.0 {
        format!("{:.3}", residual_mad_ratio(summary) * 100.0)
    } else {
        "na".to_owned()
    }
}

fn report_residual_profile(layout: &str, cell: ResidualCell) {
    emit(&format!(
        "RESIDUAL_CELL|layout={layout}|N={}|K={}|mean_alive={:.1}|mean_keyed={:.1}|warmups={WARMUPS}|samples={RESIDUAL_SAMPLES}|days_per_sample={DAYS_PER_SAMPLE}|observations={}",
        cell.population,
        cell.population / 200,
        cell.profile.mean_alive_agents,
        cell.profile.mean_keyed_entries,
        RESIDUAL_SAMPLES * DAYS_PER_SAMPLE as usize,
    ));
    for (phase, summary) in PHASE_NAMES.iter().zip(cell.profile.phases) {
        let ns_per_alive = summary.median * 1_000.0 / cell.profile.mean_alive_agents;
        let reliable = residual_mad_ratio(summary) <= 0.05;
        let mad_percent = residual_mad_percent(summary);
        let ns_per_keyed = if *phase == "P6B_total" && cell.profile.mean_keyed_entries > 0.0 {
            format!(
                "{:.4}",
                summary.median * 1_000.0 / cell.profile.mean_keyed_entries
            )
        } else {
            "na".to_owned()
        };
        emit(&format!(
            "RESIDUAL_PHASE|layout={layout}|N={}|phase={phase}|median_us={:.3}|mad_us={:.3}|mad_over_median_pct={mad_percent}|reliable={reliable}|ns_per_alive={ns_per_alive:.4}|ns_per_keyed={ns_per_keyed}",
            cell.population, summary.median, summary.mad,
        ));
    }
    for (phase, summary) in P6B_STAGE_NAMES.iter().zip(cell.profile.p6b_stages) {
        let reliable = residual_mad_ratio(summary) <= 0.05;
        let mad_percent = residual_mad_percent(summary);
        let ns_per_keyed = if cell.profile.mean_keyed_entries > 0.0 {
            format!(
                "{:.4}",
                summary.median * 1_000.0 / cell.profile.mean_keyed_entries
            )
        } else {
            "na".to_owned()
        };
        emit(&format!(
            "RESIDUAL_P6B_STAGE|layout={layout}|N={}|stage={phase}|median_us={:.3}|mad_us={:.3}|mad_over_median_pct={mad_percent}|reliable={reliable}|ns_per_keyed={ns_per_keyed}",
            cell.population, summary.median, summary.mad,
        ));
    }
}

#[cfg(target_arch = "x86_64")]
fn report_residual_cpu_cache() {
    use std::arch::x86_64::__cpuid;
    let max_leaf = __cpuid(0x8000_0000).eax;
    if max_leaf < 0x8000_0006 {
        emit(&format!(
            "RESIDUAL_CACHE|extended_leaf_max=0x{max_leaf:08x}|unavailable=true"
        ));
        return;
    }
    let leaf5 = __cpuid(0x8000_0005);
    let leaf6 = __cpuid(0x8000_0006);
    let l1d_bytes = ((leaf5.ecx >> 24) & 0xff) as u64 * 1024;
    let l2_bytes = ((leaf6.ecx >> 16) & 0xffff) as u64 * 1024;
    let l3_bytes = ((leaf6.edx >> 18) & 0x3fff) as u64 * 512 * 1024;
    let l1_dtlb_4k = (leaf5.ebx >> 16) & 0xff;
    let l2_dtlb_4k = (leaf6.ebx >> 16) & 0xfff;
    emit(&format!(
        "RESIDUAL_CACHE|extended_leaf_max=0x{max_leaf:08x}|l1d_bytes_per_core={l1d_bytes}|l2_bytes_per_core={l2_bytes}|l3_bytes_per_ccd={l3_bytes}|l1_dtlb_4k_entries={l1_dtlb_4k}|l1_dtlb_4k_reach_bytes={}|l2_dtlb_4k_entries_cpuid_raw={l2_dtlb_4k}|l2_dtlb_4k_reach_cpuid_raw_bytes={}",
        u64::from(l1_dtlb_4k) * 4096,
        u64::from(l2_dtlb_4k) * 4096,
    ));
}

#[cfg(not(target_arch = "x86_64"))]
fn report_residual_cpu_cache() {
    emit("RESIDUAL_CACHE|unavailable=true|reason=unsupported_architecture");
}

fn verify_shuffled_layout_parity(
    config: &SimConfig,
    normal: &HybridWorldState,
    shuffled: &HybridWorldState,
) {
    fn one_day(config: &SimConfig, checkpoint: &HybridWorldState) -> Digests {
        let mut world = checkpoint.clone();
        let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
        let mut context = M3StorageRunContext::new(&world, config, &run_context)
            .expect("residual layout context builds");
        let outcomes = run_m3_storage_days(&mut world, config, &mut context, 1, &options())
            .expect("residual layout parity day succeeds");
        m3_day_digests(&world, context.relationship_graph(), &outcomes)
    }

    let normal_digests = one_day(config, normal);
    let shuffled_digests = one_day(config, shuffled);
    let normal_hashes = (
        normal_digests.state.to_hex(),
        normal_digests.metrics.to_hex(),
        normal_digests.events.to_hex(),
    );
    let shuffled_hashes = (
        shuffled_digests.state.to_hex(),
        shuffled_digests.metrics.to_hex(),
        shuffled_digests.events.to_hex(),
    );
    assert_eq!(normal_hashes, shuffled_hashes);
    emit(&format!(
        "RESIDUAL_LAYOUT_PARITY|N={}|K={}|state={}|metrics={}|events={}|equal=true",
        config.world.initial_population,
        config.world.settlement_count,
        normal_hashes.0,
        normal_hashes.1,
        normal_hashes.2,
    ));
}

fn report_residual_layout_comparison(
    config: &SimConfig,
    normal_world: &HybridWorldState,
    normal_profile: PhaseProfile,
) {
    let mut reversed_world = normal_world.world.clone();
    reversed_world.agents.reverse();
    let shuffled = HybridWorldState::hybrid(reversed_world);
    verify_shuffled_layout_parity(config, normal_world, &shuffled);

    let shuffled_profile =
        profile_serial_cell_with_samples(&shuffled, config, WARMUPS, RESIDUAL_SAMPLES);
    let population = config.world.initial_population;
    report_residual_profile(
        "reversed",
        ResidualCell {
            population,
            profile: shuffled_profile,
        },
    );
    for (phase, (normal, reversed)) in PHASE_NAMES.iter().zip(
        normal_profile
            .phases
            .into_iter()
            .zip(shuffled_profile.phases),
    ) {
        emit(&format!(
            "RESIDUAL_LAYOUT_PHASE|N={population}|phase={phase}|normal_us={:.3}|normal_mad_pct={:.3}|reversed_us={:.3}|reversed_mad_pct={:.3}|shuffled_over_normal={:.4}",
            normal.median,
            residual_mad_ratio(normal) * 100.0,
            reversed.median,
            residual_mad_ratio(reversed) * 100.0,
            reversed.median / normal.median,
        ));
    }
}

fn report_residual_alphas(cells: &[ResidualCell]) {
    for pair in [(250_000, 500_000), (500_000, 1_000_000)] {
        let lower = cells.iter().find(|cell| cell.population == pair.0).unwrap();
        let upper = cells.iter().find(|cell| cell.population == pair.1).unwrap();
        for (index, phase) in PHASE_NAMES.iter().enumerate() {
            let lower_time = lower.profile.phases[index];
            let upper_time = upper.profile.phases[index];
            let reliable =
                residual_mad_ratio(lower_time) <= 0.05 && residual_mad_ratio(upper_time) <= 0.05;
            let alpha_value = if reliable {
                format!(
                    "{:.5}",
                    alpha(pair.0, lower_time.median, pair.1, upper_time.median)
                )
            } else {
                "excluded".to_owned()
            };
            emit(&format!(
                "RESIDUAL_ALPHA|from_N={}|to_N={}|phase={phase}|alpha={alpha_value}|from_mad_pct={:.3}|to_mad_pct={:.3}|reliable={reliable}",
                pair.0,
                pair.1,
                residual_mad_ratio(lower_time) * 100.0,
                residual_mad_ratio(upper_time) * 100.0,
            ));
        }
    }
}

fn run_residual_sweep() {
    emit(&format!(
        "RESIDUAL_HOST|os={}|arch={}|pid={}|seed={MASTER_SEED}|replicate={REPLICATE_ID}|profile=equilibrium|metabolic_cost={EQUILIBRIUM_METABOLIC_COST}|k=8|p_local={P_LOCAL}|warmups={WARMUPS}|samples={RESIDUAL_SAMPLES}|days_per_sample={DAYS_PER_SAMPLE}|timed_days_per_cell={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id(),
        RESIDUAL_SAMPLES * DAYS_PER_SAMPLE as usize,
    ));
    report_residual_cpu_cache();

    let mut cells = Vec::with_capacity(RESIDUAL_POPULATIONS.len() + 1);
    for population in RESIDUAL_POPULATIONS {
        let config = config_for(population, 8);
        let (base, initialization) = initialize(&config);
        emit(&format!(
            "RESIDUAL_SETUP|N={population}|K={}|initialization_us={:.3}",
            config.world.settlement_count,
            micros(initialization),
        ));
        let profile = profile_serial_cell_with_samples(&base, &config, WARMUPS, RESIDUAL_SAMPLES);
        let cell = ResidualCell {
            population,
            profile,
        };
        report_residual_profile("normal", cell);
        if matches!(population, 100_000 | 1_000_000) {
            report_residual_layout_comparison(&config, &base, profile);
        }
        cells.push(cell);
        drop(base);
    }

    let anchor_population = 250_000;
    let anchor_config = config_for(anchor_population, 8);
    let (anchor_world, initialization) = initialize(&anchor_config);
    emit(&format!(
        "RESIDUAL_SETUP|series=alpha_anchor|N={anchor_population}|K={}|initialization_us={:.3}",
        anchor_config.world.settlement_count,
        micros(initialization),
    ));
    let anchor_profile =
        profile_serial_cell_with_samples(&anchor_world, &anchor_config, WARMUPS, RESIDUAL_SAMPLES);
    report_residual_profile(
        "alpha_anchor",
        ResidualCell {
            population: anchor_population,
            profile: anchor_profile,
        },
    );
    cells.push(ResidualCell {
        population: anchor_population,
        profile: anchor_profile,
    });
    report_residual_alphas(&cells);
    emit(
        "RESIDUAL_COMPLETE|fine_sweep_cells=10|alpha_anchor_cells=1|layout_cells=2|samples_per_cell=15",
    );
}

fn run_diagnosis() {
    if std::env::args().any(|arg| arg == "--p3-only") {
        diagnose_phase3();
        return;
    }
    if std::env::args().any(|arg| arg == "--parallel-only") {
        diagnose_parallel_phase6b();
        return;
    }
    emit(&format!(
        "DIAG_HOST|os={}|arch={}|seed={MASTER_SEED}|replicate={REPLICATE_ID}|metabolic_cost={EQUILIBRIUM_METABOLIC_COST}|p_local={P_LOCAL}|k=8|warmups={WARMUPS}|samples={SAMPLES}|days_per_sample={DAYS_PER_SAMPLE}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));

    let fixed_k = [100_000, 250_000, 500_000, 1_000_000]
        .into_iter()
        .map(|population| profile_diagnostic_cell(population, 500, "fixed_K"))
        .collect::<Vec<_>>();
    report_n_alphas(&fixed_k);

    let fixed_n = [250, 500, 1_250, 2_500, 5_000]
        .into_iter()
        .map(|settlements| profile_diagnostic_cell(250_000, settlements, "fixed_N"))
        .collect::<Vec<_>>();
    report_k_times(&fixed_n);

    report_diagnostic_phases("K=N/200", &fixed_k[0]);
    report_diagnostic_phases("K=N/200", &fixed_n[2]);
    let scaled_500k = profile_diagnostic_cell(500_000, 2_500, "K=N/200");

    let scaled_1m_config = config_for_diagnostic(1_000_000, 5_000);
    let (scaled_1m_world, _) = initialize(&scaled_1m_config);
    let pool = ThreadPoolBuilder::new()
        .num_threads(POOL_THREADS)
        .build()
        .expect("diagnostic Rayon pool builds");
    let scaled_1m_measurement =
        measure_cell(1_000_000, 8, 0, &scaled_1m_config, &scaled_1m_world, &pool);
    let scaled_1m = DiagnosticCell {
        population: 1_000_000,
        settlements: 5_000,
        profile: scaled_1m_measurement.phase_profile,
    };
    report_diagnostic_phases("K=N/200", &scaled_1m);
    emit(&format!(
        "DIAG_FULL_TICK|N=1000000|K=5000|k=8|day=0|serial_median_us={:.3}|serial_mad_us={:.3}|parallel_median_us={:.3}|parallel_mad_us={:.3}|paired_samples={SAMPLES}|warmups={WARMUPS}|days_per_sample={DAYS_PER_SAMPLE}|digest_parity=true",
        scaled_1m_measurement.serial_full_tick.median,
        scaled_1m_measurement.serial_full_tick.mad,
        scaled_1m_measurement.parallel_full_tick.median,
        scaled_1m_measurement.parallel_full_tick.mad,
    ));
    let scaled_k = [fixed_k[0], fixed_n[2], scaled_500k, scaled_1m];
    report_scaled_population_alphas(&scaled_k);

    diagnose_phase3();
    let config_500 = config_for_diagnostic(1_000_000, 500);
    report_lookup_counts(1_000_000, 500, &config_500);
    let config_5000 = config_for(1_000_000, 8);
    report_lookup_counts(1_000_000, 5_000, &config_5000);
    diagnose_parallel_phase6b();
    emit(
        "DIAG_COMPLETE|fixed_K_cells=4|fixed_N_cells=5|K=N/200_cells=4|phase3_N=250000,1000000|parallel_N=1000000_k=16",
    );
}

fn main() {
    if std::env::args().any(|arg| arg == "--residual") {
        run_residual_sweep();
        return;
    }
    if std::env::args().any(|arg| arg == "--diagnose") {
        run_diagnosis();
        return;
    }
    emit(&format!(
        "SCALE_HOST|os={}|arch={}|seed={MASTER_SEED}|replicate={REPLICATE_ID}|profile=equilibrium|metabolic_cost={EQUILIBRIUM_METABOLIC_COST}|p_local={P_LOCAL}|pool_threads={POOL_THREADS}|chunk={CHUNK_SIZE}|warmups={WARMUPS}|samples={SAMPLES}|days_per_sample={DAYS_PER_SAMPLE}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    let pool = ThreadPoolBuilder::new()
        .num_threads(POOL_THREADS)
        .build()
        .expect("Rayon pool builds");
    pool.install(|| ());
    let warm_config = config_for(10_000, 8);
    let (mut warm_world, _) = initialize(&warm_config);
    let warm_run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
    let mut warm_context =
        M3ParallelStorageRunContext::new(&warm_world, &warm_config, &warm_run_context)
            .expect("parallel pool warm context builds");
    drop(
        run_m3_storage_days_with_parallel_phase6b(
            &mut warm_world,
            &warm_config,
            &mut warm_context,
            1,
            &options(),
            &pool,
            NonZeroUsize::new(CHUNK_SIZE).expect("chunk size is nonzero"),
        )
        .expect("parallel pool prewarm succeeds"),
    );

    let mut results = Vec::new();
    let mut setups = Vec::new();
    for population in POPULATIONS {
        for degree in DEGREES {
            let config = config_for(population, degree);
            let (base, initialization) = initialize(&config);
            let setup = measure_setup(&config, &base, initialization);
            report_setup(setup, population, degree, config.world.settlement_count);
            let memory = measure_memory(&base, &config, &pool, setup);
            report_memory(memory, population, degree, config.world.settlement_count);
            setups.push((population, degree, setup, memory));

            for horizon in CHECKPOINT_DAYS {
                let checkpoint = build_checkpoint(&base, &config, horizon);
                let cell = measure_cell(population, degree, horizon, &config, &checkpoint, &pool);
                report_cell(&cell);
                results.push(cell);
                drop(checkpoint);
            }
            drop(base);
        }
    }

    let alphas = report_alpha(&results);
    report_first_breaks(&results, &setups, &alphas, 102_663_720_960);
    emit(
        "SCALE_COMPLETE|largest_N=1000000|cells=16|parity=all_serial_parallel_final_digests_equal",
    );
}
