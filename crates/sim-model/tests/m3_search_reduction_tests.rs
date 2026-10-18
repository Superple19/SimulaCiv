use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, M3SearchReductionRunContext,
    M3StorageRunContext, run_m3_reference_day, run_m3_storage_days,
    run_m3_storage_days_with_search_reduction,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::{EventRecord, M3DailyMetrics, SimConfig, SimulationProfile, initialize_world};

const GOLDEN_CONFIG: &str = r#"
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
struct Checkpoint {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
    snapshot: Option<Vec<u8>>,
}

#[derive(Default)]
struct Accumulator {
    metrics: Vec<M3DailyMetrics>,
    events: Vec<EventRecord>,
}

impl Accumulator {
    fn record(&mut self, metrics: Option<M3DailyMetrics>, events: &[EventRecord]) {
        self.metrics.push(metrics.expect("metrics enabled"));
        self.events.extend_from_slice(events);
    }

    fn checkpoint(&self, day: u32, state: CanonicalHash, snapshot: Option<Vec<u8>>) -> Checkpoint {
        Checkpoint {
            day,
            state,
            metrics: canonical_m3_metrics_hash(&self.metrics).unwrap(),
            events: canonical_event_hash(&self.events).unwrap(),
            snapshot,
        }
    }
}

struct TripleRun {
    reference_world: WorldState,
    reference_context: M3ReferenceContext,
    serial_world: HybridWorldState,
    serial_context: M3StorageRunContext,
    search_world: HybridWorldState,
    search_context: M3SearchReductionRunContext,
    reference_trace: Accumulator,
    serial_trace: Accumulator,
    search_trace: Accumulator,
    checkpoints: Vec<Checkpoint>,
    snapshots: Vec<(u32, Vec<u8>)>,
}

impl TripleRun {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).unwrap();
        Self::from_worlds(
            config,
            world.clone(),
            HybridWorldState::hybrid(world.clone()),
        )
    }

    fn from_worlds(config: &SimConfig, world: WorldState, search_world: HybridWorldState) -> Self {
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let reference_context = M3ReferenceContext::new(&world, config, &run_context).unwrap();
        let serial_world = HybridWorldState::hybrid(world.clone());
        let serial_context = M3StorageRunContext::new(&serial_world, config, &run_context).unwrap();
        let search_context =
            M3SearchReductionRunContext::new(&search_world, config, &run_context).unwrap();
        assert_eq!(
            reference_context.relationship_graph().csr_hash(),
            serial_context.relationship_graph().csr_hash()
        );
        Self {
            reference_world: world,
            reference_context,
            serial_world,
            serial_context,
            search_world,
            search_context,
            reference_trace: Accumulator::default(),
            serial_trace: Accumulator::default(),
            search_trace: Accumulator::default(),
            checkpoints: Vec::new(),
            snapshots: Vec::new(),
        }
    }

    fn advance(&mut self, config: &SimConfig, snapshot: bool, checkpoint: bool) {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary: snapshot,
        };
        let reference = run_m3_reference_day(
            &mut self.reference_world,
            config,
            &self.reference_context,
            &options,
        )
        .unwrap();
        let serial = run_m3_storage_days(
            &mut self.serial_world,
            config,
            &mut self.serial_context,
            1,
            &options,
        )
        .unwrap()
        .pop()
        .unwrap();
        let search = run_m3_storage_days_with_search_reduction(
            &mut self.search_world,
            config,
            &mut self.search_context,
            1,
            &options,
        )
        .unwrap()
        .pop()
        .unwrap();

        assert_eq!(serial.executed_day, reference.executed_day);
        assert_eq!(search.executed_day, reference.executed_day);
        assert_eq!(serial.metrics, reference.metrics);
        assert_eq!(search.metrics, reference.metrics);
        assert_eq!(serial.events, reference.events);
        assert_eq!(search.events, reference.events);
        assert_eq!(
            serial.snapshot.as_ref().map(|snapshot| &snapshot.bytes),
            reference.snapshot.as_ref().map(|snapshot| &snapshot.bytes)
        );
        assert_eq!(
            search.snapshot.as_ref().map(|snapshot| &snapshot.bytes),
            reference.snapshot.as_ref().map(|snapshot| &snapshot.bytes)
        );

        let reference_state = canonical_m3_state_hash(
            &self.reference_world,
            self.reference_context.relationship_graph(),
        )
        .unwrap();
        let serial_state = canonical_m3_state_hash_from_storage(
            self.serial_world.segmented_storage.as_ref().unwrap(),
            self.serial_world.world.current_day,
            &self.serial_world.world.settlements,
            self.serial_context.relationship_graph(),
        )
        .unwrap();
        let search_state = canonical_m3_state_hash_from_storage(
            self.search_world.segmented_storage.as_ref().unwrap(),
            self.search_world.world.current_day,
            &self.search_world.world.settlements,
            self.serial_context.relationship_graph(),
        )
        .unwrap();
        assert_eq!(serial_state, reference_state);
        assert_eq!(search_state, reference_state);

        self.reference_trace
            .record(reference.metrics.clone(), &reference.events);
        self.serial_trace.record(serial.metrics, &serial.events);
        self.search_trace.record(search.metrics, &search.events);

        if let Some(snapshot) = reference.snapshot {
            self.snapshots
                .push((reference.executed_day + 1, snapshot.bytes));
        }
        if checkpoint {
            let day = self.reference_world.current_day.as_u32();
            let snapshot = self
                .snapshots
                .iter()
                .find(|(snapshot_day, _)| *snapshot_day == day)
                .map(|(_, bytes)| bytes.clone());
            let reference = self
                .reference_trace
                .checkpoint(day, reference_state, snapshot.clone());
            let serial = self
                .serial_trace
                .checkpoint(day, serial_state, snapshot.clone());
            let search = self.search_trace.checkpoint(day, search_state, snapshot);
            assert_eq!(serial, reference);
            assert_eq!(search, reference);
            self.checkpoints.push(reference);
        }
    }

    fn restore_from_snapshot(&mut self, config: &SimConfig, bytes: &[u8]) {
        let old_graph_hash = *self.serial_context.relationship_graph().csr_hash();
        let restored = restore_snapshot(bytes).unwrap();
        let run_context = M0RunContext::new(
            restored.metadata.master_seed,
            restored.metadata.replicate_id,
        );
        self.reference_world = restored.world.clone();
        self.reference_context =
            M3ReferenceContext::new(&self.reference_world, config, &run_context).unwrap();
        self.serial_world = HybridWorldState::hybrid(restored.world.clone());
        self.serial_context =
            M3StorageRunContext::new(&self.serial_world, config, &run_context).unwrap();
        self.search_world = HybridWorldState::hybrid(restored.world);
        self.search_context =
            M3SearchReductionRunContext::new(&self.search_world, config, &run_context).unwrap();
        assert_eq!(
            self.reference_context.relationship_graph().csr_hash(),
            &old_graph_hash
        );
        assert_eq!(
            self.serial_context.relationship_graph().csr_hash(),
            &old_graph_hash
        );
        let restored_graph = sim_model::RelationshipGraph::generate(
            &self
                .search_world
                .segmented_storage
                .as_ref()
                .unwrap()
                .agent_ids()
                .iter()
                .copied()
                .zip(
                    self.search_world
                        .segmented_storage
                        .as_ref()
                        .unwrap()
                        .group_ids()
                        .iter()
                        .copied(),
                )
                .collect::<Vec<_>>(),
            run_context.master_seed,
            run_context.replicate_id,
            config.interaction.relationship_degree_k.unwrap() as u32,
            config.interaction.p_local.unwrap(),
        )
        .unwrap();
        assert_eq!(restored_graph.csr_hash(), &old_graph_hash);
    }
}

fn golden_config() -> SimConfig {
    SimConfig::parse_and_validate(GOLDEN_CONFIG).unwrap()
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

fn sustained_config(population: u64, settlements: u32) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).unwrap();
    config.profile = SimulationProfile::M3;
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
    config.environment.base_metabolic_cost = 0.48;
    config.validate().unwrap();
    config
}

#[test]
fn search_reduction_matches_m3_goldens_and_snapshot_resume() {
    let config = golden_config();
    let mut uninterrupted = TripleRun::new(&config);
    for day in 1..=500 {
        let boundary = [100, 250, 500].contains(&day);
        uninterrupted.advance(&config, boundary, boundary);
    }
    assert_eq!(uninterrupted.checkpoints.len(), GOLDENS.len());
    for ((day, state, metrics, events), checkpoint) in
        GOLDENS.iter().zip(&uninterrupted.checkpoints)
    {
        assert_eq!(*day, checkpoint.day);
        assert_eq!(*state, checkpoint.state.to_hex());
        assert_eq!(*metrics, checkpoint.metrics.to_hex());
        assert_eq!(*events, checkpoint.events.to_hex());
        assert!(checkpoint.snapshot.is_some());
    }

    let mut paused = TripleRun::new(&config);
    for day in 1..=250 {
        let boundary = [100, 250].contains(&day);
        paused.advance(&config, boundary, boundary);
    }
    let snapshot = paused
        .snapshots
        .iter()
        .find(|(day, _)| *day == 250)
        .unwrap()
        .1
        .clone();
    paused.restore_from_snapshot(&config, &snapshot);
    for day in 251..=500 {
        let boundary = day == 500;
        paused.advance(&config, boundary, boundary);
    }
    assert_eq!(paused.checkpoints, uninterrupted.checkpoints);
    assert_eq!(
        paused.reference_trace.metrics,
        uninterrupted.reference_trace.metrics
    );
    assert_eq!(
        paused.reference_trace.events,
        uninterrupted.reference_trace.events
    );
    assert_eq!(
        paused.search_trace.metrics,
        uninterrupted.search_trace.metrics
    );
    assert_eq!(
        paused.search_trace.events,
        uninterrupted.search_trace.events
    );
}

#[test]
fn search_reduction_matches_the_full_frozen_100_day_matrix() {
    let started = std::time::Instant::now();
    let mut cells = 0;
    for seed in [0, 1, 81_985_529_216_486_895] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for p_local in [0.0, 0.5, 1.0] {
                    let config = matrix_config(300, settlements, degree, p_local, seed);
                    let mut run = TripleRun::new(&config);
                    for day in 1..=100 {
                        run.advance(&config, false, [25, 50, 75, 100].contains(&day));
                    }
                    cells += 1;
                }
            }
        }
    }
    for population in [1_000, 3_000] {
        let settlements = (population / 17) as u32;
        for p_local in [0.0, 0.5, 1.0] {
            let config = matrix_config(population, settlements, 8, p_local, 81_985_529_216_486_895);
            let mut run = TripleRun::new(&config);
            for day in 1..=100 {
                run.advance(&config, false, [25, 50, 75, 100].contains(&day));
            }
            cells += 1;
        }
    }
    assert_eq!(cells, 60);
    eprintln!(
        "M3-02b serial/reference/search matrix: {cells} cells in {:?}",
        started.elapsed()
    );
}

#[test]
fn search_reduction_matches_sustained_workload_for_200_days() {
    for (population, settlements) in [(300, 3), (3_000, 15)] {
        let config = sustained_config(population, settlements);
        let mut run = TripleRun::new(&config);
        for day in 1..=200 {
            run.advance(&config, false, [100, 200].contains(&day));
        }
        assert_eq!(run.checkpoints.len(), 2);
    }
}

#[test]
fn search_reduction_preserves_shuffled_storage_slot_parity() {
    let config = golden_config();
    let world = initialize_world(&config).unwrap();
    let mut reversed = world.clone();
    reversed.agents.reverse();
    let mut run = TripleRun::from_worlds(&config, world, HybridWorldState::hybrid(reversed));
    for day in 1..=100 {
        run.advance(&config, false, [25, 50, 75, 100].contains(&day));
    }
    assert_eq!(run.checkpoints.len(), 4);
}

#[test]
fn search_reduction_runner_rejects_m2_and_non_hybrid_authority() {
    let config = golden_config();
    let world = initialize_world(&config).unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let hybrid = HybridWorldState::hybrid(world.clone());
    let mut context = M3SearchReductionRunContext::new(&hybrid, &config, &run_context).unwrap();
    let options = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: false,
        snapshot_boundary: false,
    };

    let mut m2_config = config.clone();
    m2_config.profile = SimulationProfile::M2;
    let mut hybrid_for_m2 = hybrid.clone();
    assert!(
        run_m3_storage_days_with_search_reduction(
            &mut hybrid_for_m2,
            &m2_config,
            &mut context,
            0,
            &options,
        )
        .is_err()
    );

    let mut legacy = HybridWorldState::legacy(world);
    assert!(
        run_m3_storage_days_with_search_reduction(&mut legacy, &config, &mut context, 0, &options,)
            .is_err()
    );
}
