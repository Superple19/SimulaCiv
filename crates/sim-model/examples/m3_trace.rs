use sim_core::AgentId;
use sim_model::config::PopulationConfig;
use sim_model::decision::{
    Action, PrimaryActionChoice, phase4_primary_action_selection_storage_into,
};
use sim_model::features::phase3_observation_and_features_storage_with_scratch;
use sim_model::hashing::{
    CanonicalHash, canonical_event_hash, canonical_m3_metrics_hash,
    canonical_m3_state_hash_from_storage, canonical_m3p_event_hash, canonical_m3p_metrics_hash,
    canonical_m3p_state_hash_from_storage,
};
use sim_model::intents::{
    Intent, M3Phase4SlotProjection, count_cross_partition_interactions_storage_per_day,
    generate_intents_m3_storage_into,
};
use sim_model::partitioning::{
    Phase5PartitionScratch, phase5_partition_intents_from_vec_with_scratch,
};
use sim_model::phases::phase1_resource_regrowth;
use sim_model::resolution::{
    M3Phase6BStorageScratch, TargetedActionKind, TargetedOutcome, compare_keyed_interactions,
    compute_m3_resolution_key, phase6a_work_resolution_storage,
    phase6b_targeted_resolution_storage_m3_with_scratch,
};
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3DayOutcome, M3ParallelStorageRunContext,
    M3SearchReductionRunContext, M3StorageRunContext, M3pStorageRunContext, run_m3_storage_days,
    run_m3_storage_days_with_parallel_phase6b, run_m3_storage_days_with_search_reduction,
    run_m3p_storage_days,
};
use sim_model::state::HybridWorldState;
use sim_model::{
    AgentFeatures, M3DailyMetrics, M3pDailyMetrics, M3pDayOutcome, M3pEvent, Phase3ScarcityScratch,
    RelationshipGraph, SimConfig, SimulationProfile, initialize_world,
};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

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

const SUSTAINED_METABOLIC_COST: f32 = 0.48;
const EQUILIBRIUM_METABOLIC_COST: f32 = 0.3129;

#[derive(Debug, Clone, Copy)]
enum Runner {
    Serial,
    Parallel { threads: usize, chunk: NonZeroUsize },
    SearchReduction,
}

impl Runner {
    fn label(self) -> String {
        match self {
            Self::Serial => "serial".into(),
            Self::Parallel { threads, chunk } => format!("parallel:{threads}:{chunk}"),
            Self::SearchReduction => "search_reduction".into(),
        }
    }
}

struct Args {
    config: String,
    population: u64,
    settlements: u32,
    degree: u32,
    p_local: f32,
    days: u32,
    seed: u64,
    replicate: u32,
    runner: Runner,
    output: PathBuf,
}

impl Args {
    fn parse() -> Result<Self, Box<dyn Error>> {
        let mut config = None;
        let mut population = None;
        let mut settlements = None;
        let mut degree = None;
        let mut p_local = None;
        let mut days = None;
        let mut seed = None;
        let mut replicate = None;
        let mut runner = None;
        let mut output = None;
        let mut arguments = std::env::args().skip(1);
        while let Some(flag) = arguments.next() {
            let value = arguments
                .next()
                .ok_or_else(|| format!("missing value after {flag}; use --help for usage"))?;
            match flag.as_str() {
                "--config" => config = Some(value),
                "--n" => population = Some(value.parse()?),
                "--settlements" => settlements = Some(value.parse()?),
                "--k" => degree = Some(value.parse()?),
                "--p-local" => p_local = Some(value.parse()?),
                "--days" => days = Some(value.parse()?),
                "--seed" => seed = Some(value.parse()?),
                "--replicate" => replicate = Some(value.parse()?),
                "--runner" => runner = Some(parse_runner(&value)?),
                "--output" => output = Some(PathBuf::from(value)),
                _ => return Err(format!("unknown argument {flag}; use --help for usage").into()),
            }
        }
        Ok(Self {
            config: required("--config", config)?,
            population: required("--n", population)?,
            settlements: required("--settlements", settlements)?,
            degree: required("--k", degree)?,
            p_local: required("--p-local", p_local)?,
            days: required("--days", days)?,
            seed: required("--seed", seed)?,
            replicate: required("--replicate", replicate)?,
            runner: required("--runner", runner)?,
            output: required("--output", output)?,
        })
    }
}

fn required<T>(name: &str, value: Option<T>) -> Result<T, Box<dyn Error>> {
    value.ok_or_else(|| format!("missing {name}").into())
}

fn print_help() {
    println!(
        "Usage: cargo run --release -p sim-model --example m3_trace -- \\
         --config <m2_40|sustained|equilibrium|m3p_sustained|config.toml> --n <N> --settlements <K> \\
         --k <degree> --p-local <0..1> --days <days> --seed <u64> \\
         --replicate <u32> --runner <serial|parallel:threads:chunk|search_reduction> \\
         --output <trace.jsonl>"
    );
}

fn parse_runner(value: &str) -> Result<Runner, Box<dyn Error>> {
    if value == "serial" {
        return Ok(Runner::Serial);
    }
    if value == "search_reduction" {
        return Ok(Runner::SearchReduction);
    }
    if let Some(rest) = value.strip_prefix("parallel:") {
        let (threads, chunk) = rest
            .split_once(':')
            .ok_or("parallel runner must be parallel:<threads>:<chunk>")?;
        let threads = threads.parse::<usize>()?;
        if threads == 0 {
            return Err("parallel runner thread count must be nonzero".into());
        }
        let chunk = NonZeroUsize::new(chunk.parse::<usize>()?)
            .ok_or("parallel runner chunk size must be nonzero")?;
        return Ok(Runner::Parallel { threads, chunk });
    }
    Err(format!("unsupported runner {value:?}").into())
}

fn load_config(args: &Args) -> Result<SimConfig, Box<dyn Error>> {
    let mut config = match args.config.as_str() {
        "m2_40" => SimConfig::from_toml_str(M2_40_CONFIG)?,
        "sustained" => {
            let mut config = SimConfig::from_toml_str(M2_40_CONFIG)?;
            config.environment.base_metabolic_cost = SUSTAINED_METABOLIC_COST;
            config
        }
        "equilibrium" => {
            let mut config = SimConfig::from_toml_str(M2_40_CONFIG)?;
            config.environment.base_metabolic_cost = EQUILIBRIUM_METABOLIC_COST;
            config
        }
        "m3p_sustained" => {
            let mut config = SimConfig::from_toml_str(M2_40_CONFIG)?;
            config.profile = SimulationProfile::M3p;
            config.world.initial_settlement_resource = 1_000_000.0;
            config.environment.carrying_capacity = 10_000_000.0;
            config.environment.base_metabolic_cost = 1.0;
            config.environment.health_decay_rate = 0.2;
            config.economy.base_work_yield = 1.9;
            config.population = Some(PopulationConfig {
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
            config
        }
        path => SimConfig::from_toml_str(&fs::read_to_string(path)?)?,
    };
    if config.profile != SimulationProfile::M3p {
        config.profile = SimulationProfile::M3;
    }
    config.world.initial_population = args.population;
    config.world.settlement_count = args.settlements;
    config.world.master_seed = args.seed;
    config.world.replicate_id = args.replicate;
    config.interaction.relationship_degree_k = Some(args.degree.into());
    config.interaction.p_local = Some(args.p_local);
    config.validate()?;
    if args.days == 0 {
        return Err("--days must be greater than zero".into());
    }
    Ok(config)
}

#[derive(Debug, Clone, PartialEq)]
struct Digests {
    state: CanonicalHash,
    metrics: CanonicalHash,
    events: CanonicalHash,
}

fn digests(
    world: &HybridWorldState,
    graph: &RelationshipGraph,
    outcomes: &[M3DayOutcome],
) -> Result<Digests, Box<dyn Error>> {
    let storage = world.storage().ok_or("hybrid storage is missing")?;
    let metrics: Vec<M3DailyMetrics> = outcomes
        .iter()
        .map(|day| day.metrics.clone().ok_or("runner omitted metrics"))
        .collect::<Result<_, _>>()?;
    let events: Vec<_> = outcomes
        .iter()
        .flat_map(|day| day.events.iter().cloned())
        .collect();
    Ok(Digests {
        state: canonical_m3_state_hash_from_storage(
            storage,
            world.world.current_day,
            &world.world.settlements,
            graph,
        )?,
        metrics: canonical_m3_metrics_hash(&metrics)?,
        events: canonical_event_hash(&events)?,
    })
}

fn m3p_digests(
    world: &HybridWorldState,
    context: &M3pStorageRunContext,
    outcomes: &[M3pDayOutcome],
) -> Result<Digests, Box<dyn Error>> {
    let storage = world.storage().ok_or("hybrid storage is missing")?;
    let metrics: Vec<M3pDailyMetrics> = outcomes
        .iter()
        .map(|day| day.metrics.clone().ok_or("M3P runner omitted metrics"))
        .collect::<Result<_, _>>()?;
    let events: Vec<_> = outcomes
        .iter()
        .flat_map(|day| day.events.iter().cloned())
        .collect();
    Ok(Digests {
        state: canonical_m3p_state_hash_from_storage(
            storage,
            world.world.current_day,
            &world.world.settlements,
            context.relationship_graph(),
            context.next_agent_id(),
        )?,
        metrics: canonical_m3p_metrics_hash(&metrics)?,
        events: canonical_m3p_event_hash(&events)?,
    })
}

#[derive(Debug, Default)]
struct OutcomeCounts {
    zero_target: u64,
    initiator_ineligible: u64,
    target_ineligible: u64,
    theft_failed: u64,
    applied_positive: u64,
    applied_zero: u64,
}

#[derive(Debug)]
struct Observation {
    actions: [u64; Action::COUNT],
    give_some: u64,
    give_none: u64,
    steal_some: u64,
    steal_none: u64,
    cross_partition: u64,
    keyed: u64,
    outcomes: OutcomeCounts,
    conflict_fraction: Option<f64>,
    mean_contention: Option<f64>,
    level_count: usize,
    max_level_size: usize,
}

#[derive(Clone, Copy)]
struct KeyedEntry {
    key: u64,
    target: AgentId,
    initiator: AgentId,
    action: TargetedActionKind,
    target_slot: usize,
    initiator_slot: usize,
}

struct Observer {
    projection: M3Phase4SlotProjection,
    features: Vec<AgentFeatures>,
    choices: Vec<PrimaryActionChoice>,
    intents: Vec<Intent>,
    candidates: Vec<AgentId>,
    phase3: Phase3ScarcityScratch,
    phase5: Phase5PartitionScratch,
    phase6b: M3Phase6BStorageScratch,
}

impl Observer {
    fn new(world: &HybridWorldState, graph: &RelationshipGraph) -> Result<Self, Box<dyn Error>> {
        let storage = world.storage().ok_or("hybrid storage is missing")?;
        Ok(Self {
            projection: M3Phase4SlotProjection::new(graph, storage)?,
            features: Vec::with_capacity(storage.len()),
            choices: Vec::with_capacity(storage.len()),
            intents: Vec::with_capacity(storage.len()),
            candidates: Vec::with_capacity(16),
            phase3: Phase3ScarcityScratch::with_capacity(world.world.settlements.len()),
            phase5: Phase5PartitionScratch::with_capacity(world.world.settlements.len()),
            phase6b: M3Phase6BStorageScratch::with_capacity(storage.len()),
        })
    }

    fn observe(
        &mut self,
        base: &HybridWorldState,
        config: &SimConfig,
    ) -> Result<Observation, Box<dyn Error>> {
        let mut shadow = base.clone();
        let world = &mut shadow.world;
        let storage = shadow
            .segmented_storage
            .as_mut()
            .ok_or("hybrid storage is missing")?;
        let day = world.current_day;
        phase1_resource_regrowth(world, config);
        storage.phase2_degradation_with_config(config);
        phase3_observation_and_features_storage_with_scratch(
            storage,
            &world.settlements,
            config,
            &mut self.features,
            &mut self.phase3,
        )?;
        phase4_primary_action_selection_storage_into(
            storage,
            day,
            config,
            &self.features,
            &mut self.choices,
        )?;
        let mut actions = [0u64; Action::COUNT];
        for choice in &self.choices {
            actions[choice.action.index()] += 1;
        }
        generate_intents_m3_storage_into(
            storage,
            day,
            config,
            &self.choices,
            &self.projection,
            &mut self.candidates,
            &mut self.intents,
        )?;
        let mut give_some = 0;
        let mut give_none = 0;
        let mut steal_some = 0;
        let mut steal_none = 0;
        for intent in &self.intents {
            match intent {
                Intent::GiveFood {
                    target_agent_id: Some(_),
                    ..
                } => give_some += 1,
                Intent::GiveFood {
                    target_agent_id: None,
                    ..
                } => give_none += 1,
                Intent::StealFood {
                    target_agent_id: Some(_),
                    ..
                } => steal_some += 1,
                Intent::StealFood {
                    target_agent_id: None,
                    ..
                } => steal_none += 1,
                _ => {}
            }
        }
        let cross_partition =
            count_cross_partition_interactions_storage_per_day(&self.intents, storage)?;
        let mut keyed = Vec::new();
        for intent in &self.intents {
            let (initiator, target, action) = match intent {
                Intent::GiveFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => (*agent_id, *target, TargetedActionKind::GiveFood),
                Intent::StealFood {
                    agent_id,
                    target_agent_id: Some(target),
                    ..
                } => (*agent_id, *target, TargetedActionKind::StealFood),
                _ => continue,
            };
            keyed.push(KeyedEntry {
                key: compute_m3_resolution_key(
                    config.world.master_seed,
                    config.world.replicate_id,
                    day.as_u32(),
                    target.0,
                    initiator.0,
                    action.action_kind(),
                ),
                target,
                initiator,
                action,
                target_slot: storage.slot_of(target).ok_or("keyed target has no slot")?,
                initiator_slot: storage
                    .slot_of(initiator)
                    .ok_or("keyed initiator has no slot")?,
            });
        }
        keyed.sort_by(|a, b| {
            compare_keyed_interactions(
                a.key,
                a.target,
                a.initiator,
                a.action,
                b.key,
                b.target,
                b.initiator,
                b.action,
            )
        });
        let partitions =
            phase5_partition_intents_from_vec_with_scratch(&mut self.intents, &mut self.phase5)?;
        let _work = phase6a_work_resolution_storage(storage, &mut world.settlements, &partitions)?;
        let resolutions = phase6b_targeted_resolution_storage_m3_with_scratch(
            storage,
            &world.settlements,
            day,
            config,
            &partitions,
            &mut self.phase6b,
        )?;
        let resolved_keyed = resolutions
            .iter()
            .map(|group| group.keyed_stream.len() as u64)
            .sum::<u64>();
        if resolved_keyed != keyed.len() as u64 {
            return Err("intent and resolver keyed-entry counts differ".into());
        }

        let mut outcomes = OutcomeCounts::default();
        for group in &resolutions {
            for resolution in &group.resolutions {
                match resolution.outcome {
                    TargetedOutcome::ZeroTarget => outcomes.zero_target += 1,
                    TargetedOutcome::InitiatorIneligible => outcomes.initiator_ineligible += 1,
                    TargetedOutcome::TargetIneligible => outcomes.target_ineligible += 1,
                    TargetedOutcome::TheftFailed => outcomes.theft_failed += 1,
                    TargetedOutcome::Applied { amount } if amount > 0.0 => {
                        outcomes.applied_positive += 1
                    }
                    TargetedOutcome::Applied { .. } => outcomes.applied_zero += 1,
                }
            }
        }
        let mut touches = HashMap::<AgentId, u64>::with_capacity(keyed.len() * 2);
        for entry in &keyed {
            *touches.entry(entry.initiator).or_default() += 1;
            *touches.entry(entry.target).or_default() += 1;
        }
        let conflicting = keyed
            .iter()
            .filter(|entry| touches[&entry.initiator] > 1 || touches[&entry.target] > 1)
            .count();
        let conflict_fraction =
            (!keyed.is_empty()).then_some(conflicting as f64 / keyed.len() as f64);
        let mean_contention = (!touches.is_empty())
            .then(|| touches.values().sum::<u64>() as f64 / touches.len() as f64);
        let mut last_level = vec![0usize; storage.len()];
        let mut level_sizes = Vec::<usize>::new();
        for entry in &keyed {
            let level = last_level[entry.initiator_slot].max(last_level[entry.target_slot]) + 1;
            if level_sizes.len() < level {
                level_sizes.resize(level, 0);
            }
            level_sizes[level - 1] += 1;
            last_level[entry.initiator_slot] = level;
            last_level[entry.target_slot] = level;
        }
        Ok(Observation {
            actions,
            give_some,
            give_none,
            steal_some,
            steal_none,
            cross_partition,
            keyed: keyed.len() as u64,
            outcomes,
            conflict_fraction,
            mean_contention,
            level_count: level_sizes.len(),
            max_level_size: level_sizes.into_iter().max().unwrap_or(0),
        })
    }
}

#[derive(Debug)]
struct FoodStats {
    mean: Option<f64>,
    min: Option<f32>,
    max: Option<f32>,
}

fn food_stats(world: &HybridWorldState) -> Result<(u64, FoodStats), Box<dyn Error>> {
    let storage = world.storage().ok_or("hybrid storage is missing")?;
    let mut alive = 0u64;
    let mut sum = 0.0f64;
    let mut min: Option<f32> = None;
    let mut max: Option<f32> = None;
    for (slot, &is_alive) in storage.alive().iter().enumerate() {
        if is_alive {
            let food = storage.food()[slot];
            alive += 1;
            sum += f64::from(food);
            min = Some(min.map_or(food, |current| current.min(food)));
            max = Some(max.map_or(food, |current| current.max(food)));
        }
    }
    Ok((
        alive,
        FoodStats {
            mean: (alive > 0).then_some(sum / alive as f64),
            min,
            max,
        },
    ))
}

fn run_one_day(
    world: &mut HybridWorldState,
    config: &SimConfig,
    runner: Runner,
    pool: Option<&rayon::ThreadPool>,
    context: &mut RunnerContext,
) -> Result<M3DayOutcome, Box<dyn Error>> {
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let outcomes = match (runner, context) {
        (Runner::Serial, RunnerContext::Serial(context)) => {
            run_m3_storage_days(world, config, context, 1, &options)?
        }
        (Runner::SearchReduction, RunnerContext::SearchReduction(context)) => {
            run_m3_storage_days_with_search_reduction(world, config, context, 1, &options)?
        }
        (Runner::Parallel { chunk, .. }, RunnerContext::Parallel(context)) => {
            run_m3_storage_days_with_parallel_phase6b(
                world,
                config,
                context,
                1,
                &options,
                pool.ok_or("parallel pool was not built")?,
                chunk,
            )?
        }
        _ => return Err("runner and runtime context do not match".into()),
    };
    Ok(outcomes
        .into_iter()
        .next()
        .ok_or("runner returned no day")?)
}

enum RunnerContext {
    Serial(Box<M3StorageRunContext>),
    SearchReduction(Box<M3SearchReductionRunContext>),
    Parallel(Box<M3ParallelStorageRunContext>),
}

impl RunnerContext {
    fn new(
        world: &HybridWorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
        runner: Runner,
    ) -> Result<Self, Box<dyn Error>> {
        Ok(match runner {
            Runner::Serial => Self::Serial(Box::new(M3StorageRunContext::new(
                world,
                config,
                run_context,
            )?)),
            Runner::SearchReduction => Self::SearchReduction(Box::new(
                M3SearchReductionRunContext::new(world, config, run_context)?,
            )),
            Runner::Parallel { .. } => Self::Parallel(Box::new(M3ParallelStorageRunContext::new(
                world,
                config,
                run_context,
            )?)),
        })
    }
}

fn plain_run(
    base: &HybridWorldState,
    config: &SimConfig,
    graph: &RelationshipGraph,
    runner: Runner,
    days: u32,
    run_context: &M0RunContext,
    pool: Option<&rayon::ThreadPool>,
) -> Result<(HybridWorldState, Vec<M3DayOutcome>, Digests), Box<dyn Error>> {
    let mut world = base.clone();
    let mut context = RunnerContext::new(&world, config, run_context, runner)?;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let outcomes = match (runner, &mut context) {
        (Runner::Serial, RunnerContext::Serial(context)) => {
            run_m3_storage_days(&mut world, config, context, days, &options)?
        }
        (Runner::SearchReduction, RunnerContext::SearchReduction(context)) => {
            run_m3_storage_days_with_search_reduction(&mut world, config, context, days, &options)?
        }
        (Runner::Parallel { chunk, .. }, RunnerContext::Parallel(context)) => {
            run_m3_storage_days_with_parallel_phase6b(
                &mut world,
                config,
                context,
                days,
                &options,
                pool.ok_or("parallel pool was not built")?,
                chunk,
            )?
        }
        _ => return Err("runner and runtime context do not match".into()),
    };
    let final_digests = digests(&world, graph, &outcomes)?;
    Ok((world, outcomes, final_digests))
}

fn json_string(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + 2);
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(output, "\\u{:04x}", c as u32);
            }
            c => output.push(c),
        }
    }
    output.push('"');
    output
}

fn optional_number(value: Option<f64>) -> String {
    value.map_or_else(|| "null".into(), |number| number.to_string())
}

fn optional_float(value: Option<f32>) -> String {
    value.map_or_else(|| "null".into(), |number| number.to_string())
}

fn git_commit_hash() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn write_header(
    writer: &mut impl Write,
    config: &SimConfig,
    runner: Runner,
) -> Result<(), Box<dyn Error>> {
    let config_toml = toml::to_string_pretty(config)?;
    let logical_processors = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let rustc = Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unavailable".into());
    let processor = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default();
    let commit = git_commit_hash().map_or_else(|| "null".into(), |hash| json_string(&hash));
    writeln!(
        writer,
        "{{\"type\":\"header\",\"config_toml\":{},\"runner\":{},\"host\":{{\"os\":{},\"arch\":{},\"logical_processors\":{},\"processor\":{},\"rustc\":{}}},\"commit\":{}}}",
        json_string(&config_toml),
        json_string(&runner.label()),
        json_string(std::env::consts::OS),
        json_string(std::env::consts::ARCH),
        logical_processors,
        json_string(&processor),
        json_string(&rustc),
        commit,
    )?;
    Ok(())
}

fn write_day(
    writer: &mut impl Write,
    day: u32,
    observation: &Observation,
    alive: u64,
    food: &FoodStats,
    wall_time_us: f64,
    final_digests: Option<&Digests>,
) -> Result<(), Box<dyn Error>> {
    let actions = Action::ALL
        .iter()
        .enumerate()
        .map(|(index, action)| {
            format!(
                "{}:{}",
                json_string(&action.to_string()),
                observation.actions[index]
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    write!(
        writer,
        "{{\"type\":\"day\",\"day\":{day},\"alive\":{alive},\"food_alive\":{{\"mean\":{},\"min\":{},\"max\":{}}},\"action_counts\":{{{actions}}},\"give_food\":{{\"some\":{},\"none\":{}}},\"steal_food\":{{\"some\":{},\"none\":{}}},\"keyed_entries\":{},\"cross_partition_interactions\":{},\"phase6b_outcomes\":{{\"zero_target\":{},\"initiator_ineligible\":{},\"target_ineligible\":{},\"theft_failed\":{},\"applied_positive\":{},\"applied_zero\":{}}},\"conflict_fraction\":{},\"mean_contention\":{},\"level_count\":{},\"max_level_size\":{},\"day_wall_time_us\":{}",
        optional_number(food.mean),
        optional_float(food.min),
        optional_float(food.max),
        observation.give_some,
        observation.give_none,
        observation.steal_some,
        observation.steal_none,
        observation.keyed,
        observation.cross_partition,
        observation.outcomes.zero_target,
        observation.outcomes.initiator_ineligible,
        observation.outcomes.target_ineligible,
        observation.outcomes.theft_failed,
        observation.outcomes.applied_positive,
        observation.outcomes.applied_zero,
        optional_number(observation.conflict_fraction),
        optional_number(observation.mean_contention),
        observation.level_count,
        observation.max_level_size,
        wall_time_us,
    )?;
    if let Some(digests) = final_digests {
        write!(
            writer,
            ",\"final_digests\":{{\"state\":{},\"metrics\":{},\"events\":{}}}",
            json_string(&digests.state.to_string()),
            json_string(&digests.metrics.to_string()),
            json_string(&digests.events.to_string()),
        )?;
    }
    writeln!(writer, "}}")?;
    Ok(())
}

fn write_m3p_day(
    writer: &mut impl Write,
    outcome: &M3pDayOutcome,
    observation: &Observation,
    alive: u64,
    food: &FoodStats,
    wall_time_us: f64,
    final_digests: Option<&Digests>,
) -> Result<(), Box<dyn Error>> {
    let metrics = outcome
        .metrics
        .as_ref()
        .ok_or("M3P runner omitted metrics")?;
    let actions = Action::ALL
        .iter()
        .enumerate()
        .map(|(index, action)| {
            format!(
                "{}:{}",
                json_string(&action.to_string()),
                observation.actions[index]
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut population_events = Vec::new();
    let mut dropped_edges = 0u64;
    for record in &outcome.events {
        match record.event {
            M3pEvent::Birth {
                parent,
                child,
                group,
            } => population_events.push(format!(
                "{{\"type\":\"birth\",\"parent\":{},\"child\":{},\"group\":{}}}",
                parent.0, child.0, group.0
            )),
            M3pEvent::Cleanup {
                removed,
                replaced_edges,
                dropped_edges: dropped,
            } => {
                dropped_edges += dropped;
                population_events.push(format!(
                    "{{\"type\":\"cleanup\",\"removed\":{removed},\"replaced_edges\":{replaced_edges},\"dropped_edges\":{dropped}}}"
                ));
            }
            M3pEvent::Existing(_) => {}
        }
    }
    write!(
        writer,
        "{{\"type\":\"day\",\"day\":{},\"alive\":{alive},\"food_alive\":{{\"mean\":{},\"min\":{},\"max\":{}}},\"action_counts\":{{{actions}}},\"give_food\":{{\"some\":{},\"none\":{}}},\"steal_food\":{{\"some\":{},\"none\":{}}},\"keyed_entries\":{},\"cross_partition_interactions\":{},\"phase6b_outcomes\":{{\"zero_target\":{},\"initiator_ineligible\":{},\"target_ineligible\":{},\"theft_failed\":{},\"applied_positive\":{},\"applied_zero\":{}}},\"conflict_fraction\":{},\"mean_contention\":{},\"level_count\":{},\"max_level_size\":{},\"day_wall_time_us\":{},\"births\":{},\"removed\":{},\"replaced_edges\":{},\"dropped_edges\":{dropped_edges},\"live_population\":{},\"next_agent_id\":{},\"population_events\":[{}]",
        outcome.executed_day,
        optional_number(food.mean),
        optional_float(food.min),
        optional_float(food.max),
        observation.give_some,
        observation.give_none,
        observation.steal_some,
        observation.steal_none,
        observation.keyed,
        metrics.m3_metrics.cross_partition_interaction_count,
        observation.outcomes.zero_target,
        observation.outcomes.initiator_ineligible,
        observation.outcomes.target_ineligible,
        observation.outcomes.theft_failed,
        observation.outcomes.applied_positive,
        observation.outcomes.applied_zero,
        optional_number(observation.conflict_fraction),
        optional_number(observation.mean_contention),
        observation.level_count,
        observation.max_level_size,
        wall_time_us,
        metrics.births,
        metrics.removed,
        metrics.replaced_edges,
        metrics.live_population,
        metrics.next_agent_id,
        population_events.join(","),
    )?;
    if let Some(digests) = final_digests {
        write!(
            writer,
            ",\"final_digests\":{{\"state\":{},\"metrics\":{},\"events\":{}}}",
            json_string(&digests.state.to_string()),
            json_string(&digests.metrics.to_string()),
            json_string(&digests.events.to_string()),
        )?;
    }
    writeln!(writer, "}}")?;
    Ok(())
}

fn run_m3p_trace(
    args: &Args,
    config: &SimConfig,
    run_context: &M0RunContext,
    initial: &HybridWorldState,
) -> Result<(), Box<dyn Error>> {
    if !matches!(args.runner, Runner::Serial) {
        return Err("M3P trace currently supports only the serial storage runner".into());
    }
    let mut traced_world = initial.clone();
    let mut context = M3pStorageRunContext::new(&traced_world, config, run_context)?;
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    let mut outcomes = Vec::with_capacity(args.days as usize);
    let mut observations = Vec::with_capacity(args.days as usize);
    let mut elapsed = Vec::with_capacity(args.days as usize);
    let mut food_by_day = Vec::with_capacity(args.days as usize);

    for _ in 0..args.days {
        let mut observer = Observer::new(&traced_world, context.relationship_graph())?;
        let observation = observer.observe(&traced_world, config)?;
        let started = Instant::now();
        let outcome = run_m3p_storage_days(&mut traced_world, config, &mut context, 1, &options)?
            .into_iter()
            .next()
            .ok_or("M3P runner returned no day")?;
        let metrics = outcome
            .metrics
            .as_ref()
            .ok_or("M3P runner omitted metrics")?;
        assert_eq!(
            observation.cross_partition, metrics.m3_metrics.cross_partition_interaction_count,
            "trace cross-partition count differs from M3P metrics"
        );
        assert_eq!(
            context.relationship_graph().edge_ratio_counts(),
            (
                metrics.m3_metrics.cross_partition_edge_count,
                metrics.m3_metrics.total_directed_edge_count
            ),
            "trace relationship graph differs from M3P metrics"
        );
        elapsed.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        food_by_day.push(food_stats(&traced_world)?);
        outcomes.push(outcome);
        observations.push(observation);
    }

    let traced_digests = m3p_digests(&traced_world, &context, &outcomes)?;
    let mut plain_world = initial.clone();
    let mut plain_context = M3pStorageRunContext::new(&plain_world, config, run_context)?;
    let plain_outcomes = run_m3p_storage_days(
        &mut plain_world,
        config,
        &mut plain_context,
        args.days,
        &options,
    )?;
    let plain_digests = m3p_digests(&plain_world, &plain_context, &plain_outcomes)?;
    assert_eq!(
        traced_digests, plain_digests,
        "trace collection changed M3P digests"
    );
    assert_eq!(
        outcomes
            .iter()
            .map(|day| (&day.metrics, &day.events))
            .collect::<Vec<_>>(),
        plain_outcomes
            .iter()
            .map(|day| (&day.metrics, &day.events))
            .collect::<Vec<_>>(),
        "trace collection changed M3P daily metrics or events"
    );

    if let Some(parent) = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    write_header(&mut writer, config, args.runner)?;
    for index in 0..args.days as usize {
        let (alive, food) = &food_by_day[index];
        let final_digests = (index + 1 == args.days as usize).then_some(&traced_digests);
        write_m3p_day(
            &mut writer,
            &outcomes[index],
            &observations[index],
            *alive,
            food,
            elapsed[index],
            final_digests,
        )?;
    }
    writer.flush()?;
    println!("trace: {}", args.output.display());
    println!("state: {}", traced_digests.state);
    println!("metrics: {}", traced_digests.metrics);
    println!("events: {}", traced_digests.events);
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().nth(1).as_deref() == Some("--help") {
        print_help();
        return Ok(());
    }
    let args = Args::parse()?;
    let config = load_config(&args)?;
    let run_context = M0RunContext::new(args.seed, args.replicate);
    let initial = HybridWorldState::hybrid(initialize_world(&config)?);
    if config.profile == SimulationProfile::M3p {
        return run_m3p_trace(&args, &config, &run_context, &initial);
    }
    let storage = initial.storage().ok_or("hybrid storage is missing")?;
    let memberships: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.group_ids().iter().copied())
        .collect();
    let graph = RelationshipGraph::generate(
        &memberships,
        args.seed,
        args.replicate,
        args.degree,
        args.p_local,
    )?;
    let pool = match args.runner {
        Runner::Parallel { threads, .. } => Some(
            rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()?,
        ),
        _ => None,
    };
    let mut observer = Observer::new(&initial, &graph)?;
    let mut traced_world = initial.clone();
    let mut runner_context = RunnerContext::new(&initial, &config, &run_context, args.runner)?;
    let mut outcomes = Vec::with_capacity(args.days as usize);
    let mut observations = Vec::with_capacity(args.days as usize);
    let mut elapsed = Vec::with_capacity(args.days as usize);
    let mut food_by_day = Vec::with_capacity(args.days as usize);

    for _ in 0..args.days {
        let observation = observer.observe(&traced_world, &config)?;
        let started = Instant::now();
        let outcome = run_one_day(
            &mut traced_world,
            &config,
            args.runner,
            pool.as_ref(),
            &mut runner_context,
        )?;
        let metrics = outcome.metrics.as_ref().ok_or("runner omitted metrics")?;
        assert_eq!(
            observation.cross_partition, metrics.cross_partition_interaction_count,
            "trace cross-partition count differs from runner metrics"
        );
        assert_eq!(
            graph.edge_ratio_counts(),
            (
                metrics.cross_partition_edge_count,
                metrics.total_directed_edge_count
            ),
            "trace relationship graph differs from runner metrics"
        );
        elapsed.push(started.elapsed().as_secs_f64() * 1_000_000.0);
        food_by_day.push(food_stats(&traced_world)?);
        outcomes.push(outcome);
        observations.push(observation);
    }
    let traced_digests = digests(&traced_world, &graph, &outcomes)?;
    let (plain_world, plain_outcomes, plain_digests) = plain_run(
        &initial,
        &config,
        &graph,
        args.runner,
        args.days,
        &run_context,
        pool.as_ref(),
    )?;
    assert_eq!(
        traced_digests, plain_digests,
        "trace collection changed final digests"
    );
    assert_eq!(
        outcomes
            .iter()
            .map(|day| (&day.metrics, &day.events))
            .collect::<Vec<_>>(),
        plain_outcomes
            .iter()
            .map(|day| (&day.metrics, &day.events))
            .collect::<Vec<_>>(),
        "trace collection changed daily records"
    );
    let plain_state = digests(&plain_world, &graph, &plain_outcomes)?;
    assert_eq!(
        traced_digests, plain_state,
        "plain-run digest verification failed"
    );

    if let Some(parent) = args
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(File::create(&args.output)?);
    write_header(&mut writer, &config, args.runner)?;
    for index in 0..args.days as usize {
        let (alive, food) = &food_by_day[index];
        let final_digests = (index + 1 == args.days as usize).then_some(&traced_digests);
        write_day(
            &mut writer,
            outcomes[index].executed_day,
            &observations[index],
            *alive,
            food,
            elapsed[index],
            final_digests,
        )?;
    }
    writer.flush()?;
    println!("trace: {}", args.output.display());
    println!("state: {}", traced_digests.state);
    println!("metrics: {}", traced_digests.metrics);
    println!("events: {}", traced_digests.events);
    Ok(())
}
