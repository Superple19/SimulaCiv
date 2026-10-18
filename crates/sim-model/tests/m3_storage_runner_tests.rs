use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash, canonical_m3_state_hash,
    canonical_m3_state_hash_from_storage,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3ReferenceContext, M3StorageRunContext,
    run_m3_reference_day, run_m3_storage_days,
};
use sim_model::snapshot::restore_snapshot;
use sim_model::state::{HybridWorldState, WorldState};
use sim_model::{SimConfig, SimulationProfile, initialize_world};

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

#[derive(Debug, PartialEq, Eq)]
struct Checkpoint {
    day: u32,
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
    snapshot: Option<Vec<u8>>,
}

struct PairedRun {
    reference_world: WorldState,
    reference_context: M3ReferenceContext,
    storage_world: HybridWorldState,
    storage_context: M3StorageRunContext,
    metrics: Vec<sim_model::M3DailyMetrics>,
    events: Vec<sim_model::EventRecord>,
    checkpoints: Vec<Checkpoint>,
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

impl PairedRun {
    fn new(config: &SimConfig) -> Self {
        let world = initialize_world(config).unwrap();
        Self::from_worlds(config, world.clone(), HybridWorldState::hybrid(world))
    }

    fn from_worlds(
        config: &SimConfig,
        reference_world: WorldState,
        storage_world: HybridWorldState,
    ) -> Self {
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let reference_context =
            M3ReferenceContext::new(&reference_world, config, &run_context).unwrap();
        let storage_context =
            M3StorageRunContext::new(&storage_world, config, &run_context).unwrap();
        assert_eq!(
            reference_context.relationship_graph().csr_hash(),
            storage_context.relationship_graph().csr_hash()
        );
        Self {
            reference_world,
            reference_context,
            storage_world,
            storage_context,
            metrics: Vec::new(),
            events: Vec::new(),
            checkpoints: Vec::new(),
        }
    }

    fn advance(&mut self, config: &SimConfig, snapshot_boundary: bool, checkpoint: bool) {
        let options = DayExecutionOptions {
            metrics_enabled: true,
            events_enabled: true,
            snapshot_boundary,
        };
        let reference_outcome = run_m3_reference_day(
            &mut self.reference_world,
            config,
            &self.reference_context,
            &options,
        )
        .unwrap();
        let storage_outcomes = run_m3_storage_days(
            &mut self.storage_world,
            config,
            &mut self.storage_context,
            1,
            &options,
        )
        .unwrap();
        let storage_outcome = storage_outcomes.into_iter().next().unwrap();
        assert_eq!(storage_outcome.executed_day, reference_outcome.executed_day);
        assert_eq!(storage_outcome.metrics, reference_outcome.metrics);
        assert_eq!(storage_outcome.events, reference_outcome.events);
        assert_eq!(
            storage_outcome.snapshot.as_ref().map(|s| &s.bytes),
            reference_outcome.snapshot.as_ref().map(|s| &s.bytes)
        );

        let reference_state = canonical_m3_state_hash(
            &self.reference_world,
            self.reference_context.relationship_graph(),
        )
        .unwrap();
        let storage = self.storage_world.segmented_storage.as_ref().unwrap();
        let storage_state = canonical_m3_state_hash_from_storage(
            storage,
            self.storage_world.world.current_day,
            &self.storage_world.world.settlements,
            self.storage_context.relationship_graph(),
        )
        .unwrap();
        assert_eq!(storage_state, reference_state);

        let metric = storage_outcome.metrics.unwrap();
        self.metrics.push(metric);
        self.events.extend(storage_outcome.events);
        if checkpoint {
            self.checkpoints.push(Checkpoint {
                day: self.storage_world.world.current_day.as_u32(),
                state: storage_state,
                metrics: canonical_m3_metrics_hash(&self.metrics).unwrap(),
                events: canonical_event_hash(&self.events).unwrap(),
                snapshot: storage_outcome.snapshot.map(|snapshot| snapshot.bytes),
            });
        }
    }

    fn restore_from_snapshot(&mut self, config: &SimConfig, bytes: &[u8]) {
        let old_graph_hash = *self.storage_context.relationship_graph().csr_hash();
        let restored = restore_snapshot(bytes).unwrap();
        let run_context = M0RunContext::new(
            restored.metadata.master_seed,
            restored.metadata.replicate_id,
        );
        let reference_context =
            M3ReferenceContext::new(&restored.world, config, &run_context).unwrap();
        let storage_world = HybridWorldState::hybrid(restored.world.clone());
        let storage_context =
            M3StorageRunContext::new(&storage_world, config, &run_context).unwrap();
        assert_eq!(
            reference_context.relationship_graph().csr_hash(),
            &old_graph_hash
        );
        assert_eq!(
            storage_context.relationship_graph().csr_hash(),
            &old_graph_hash
        );
        self.reference_world = restored.world;
        self.reference_context = reference_context;
        self.storage_world = storage_world;
        self.storage_context = storage_context;
    }
}

#[test]
fn storage_runner_matches_m3_goldens_and_snapshot_resume() {
    let config = golden_config();
    let mut uninterrupted = PairedRun::new(&config);
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

    let mut paused = PairedRun::new(&config);
    for day in 1..=250 {
        let boundary = [100, 250].contains(&day);
        paused.advance(&config, boundary, boundary);
    }
    let snapshot_250 = paused
        .checkpoints
        .iter()
        .find(|checkpoint| checkpoint.day == 250)
        .unwrap()
        .snapshot
        .clone()
        .unwrap();
    paused.restore_from_snapshot(&config, &snapshot_250);
    assert_eq!(paused.storage_world.world.current_day.as_u32(), 250);
    for day in 251..=500 {
        let boundary = day == 500;
        paused.advance(&config, boundary, boundary);
    }
    assert_eq!(paused.checkpoints, uninterrupted.checkpoints);
    assert_eq!(paused.metrics, uninterrupted.metrics);
    assert_eq!(paused.events, uninterrupted.events);
}

#[test]
fn storage_runner_matches_the_frozen_100_day_ci_matrix() {
    let started = std::time::Instant::now();
    let mut cells = 0;
    for seed in [0, 1, 81_985_529_216_486_895] {
        for settlements in [3, 10] {
            for degree in [4, 8, 16] {
                for p_local in [0.0, 0.5, 1.0] {
                    let config = matrix_config(300, settlements, degree, p_local, seed);
                    run_matrix_cell(&config);
                    cells += 1;
                }
            }
        }
    }
    for population in [1_000, 3_000] {
        let settlements = (population / 17) as u32;
        for p_local in [0.0, 0.5, 1.0] {
            let config = matrix_config(population, settlements, 8, p_local, 81_985_529_216_486_895);
            run_matrix_cell(&config);
            cells += 1;
        }
    }
    assert_eq!(cells, 60);
    eprintln!(
        "M3-01 storage/reference matrix: {cells} cells in {:?}",
        started.elapsed()
    );
}

fn run_matrix_cell(config: &SimConfig) {
    let mut run = PairedRun::new(config);
    for day in 1..=100 {
        run.advance(config, false, [1, 25, 50, 75, 100].contains(&day));
    }
    assert_eq!(run.checkpoints.len(), 5);
}

#[test]
fn shuffled_storage_slots_preserve_m3_digests() {
    let config = golden_config();
    let world = initialize_world(&config).unwrap();
    let mut reversed_world = world.clone();
    reversed_world.agents.reverse();
    let mut ordered = PairedRun::from_worlds(
        &config,
        world.clone(),
        HybridWorldState::hybrid(world.clone()),
    );
    let mut shuffled =
        PairedRun::from_worlds(&config, world, HybridWorldState::hybrid(reversed_world));
    for _ in 0..30 {
        ordered.advance(&config, false, true);
        shuffled.advance(&config, false, true);
    }
    assert_eq!(ordered.checkpoints, shuffled.checkpoints);
    assert_eq!(ordered.metrics, shuffled.metrics);
    assert_eq!(ordered.events, shuffled.events);
}

#[test]
fn m3_storage_runner_rejects_m2_and_non_hybrid_authority() {
    let config = golden_config();
    let world = initialize_world(&config).unwrap();
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let hybrid = HybridWorldState::hybrid(world.clone());
    let mut storage_context = M3StorageRunContext::new(&hybrid, &config, &run_context).unwrap();
    let options = DayExecutionOptions {
        metrics_enabled: false,
        events_enabled: false,
        snapshot_boundary: false,
    };

    let mut m2_config = config.clone();
    m2_config.profile = SimulationProfile::M2;
    let mut hybrid_for_m2 = hybrid.clone();
    assert!(
        run_m3_storage_days(
            &mut hybrid_for_m2,
            &m2_config,
            &mut storage_context,
            0,
            &options
        )
        .is_err()
    );

    let mut legacy = HybridWorldState::legacy(world);
    assert!(run_m3_storage_days(&mut legacy, &config, &mut storage_context, 0, &options).is_err());
}
