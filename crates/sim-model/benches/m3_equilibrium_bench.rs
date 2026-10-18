use sim_model::events::{Event, StateTransitionEvent};
use sim_model::resolution::TargetedActionKind;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3StorageRunContext, run_m3_storage_days,
};
use sim_model::state::HybridWorldState;
use sim_model::{SimConfig, SimulationProfile, initialize_world};

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

const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const SEARCH_DAYS: u32 = 300;
const FIRST_CHECK_DAY: u32 = 50;

#[derive(Clone, Copy)]
struct FoodConfig {
    initial_resource: f32,
    carrying_capacity: f32,
    regrowth_rate: f32,
    metabolic_cost: f32,
    health_decay_rate: f32,
    work_yield: f32,
}

const SUSTAINED_FOOD: FoodConfig = FoodConfig {
    initial_resource: 2_000.0,
    carrying_capacity: 10_000.0,
    regrowth_rate: 0.1,
    metabolic_cost: 0.48,
    health_decay_rate: 0.05,
    work_yield: 2.0,
};

#[derive(Clone)]
struct Candidate {
    name: String,
    food: FoodConfig,
    changed_knobs: u8,
    relative_change: f64,
}

#[derive(Default)]
struct EquilibriumSummary {
    minimum_alive: usize,
    alive_day_50: usize,
    alive_day_300: usize,
    maximum_daily_deaths: usize,
    total_deaths_days_50_300: usize,
    mean_food_day_50: f64,
    mean_food_day_300: f64,
    keyed_zero_days: u32,
    keyed_total: u64,
    alive_total: u64,
    give_food_some_days: u32,
    window_days: u32,
}

impl EquilibriumSummary {
    fn mean_keyed(&self) -> f64 {
        self.keyed_total as f64 / f64::from(self.window_days)
    }

    fn mean_alive(&self) -> f64 {
        self.alive_total as f64 / f64::from(self.window_days)
    }

    fn food_drift_percent(&self) -> f64 {
        if self.mean_food_day_50 == 0.0 {
            f64::INFINITY
        } else {
            (self.mean_food_day_300 - self.mean_food_day_50).abs() / self.mean_food_day_50 * 100.0
        }
    }

    fn passed_gates(&self, population: usize) -> u8 {
        let max_daily_deaths = population / 1_000;
        let max_total_deaths = population / 100;
        let mut passed = 0;
        passed += u8::from(self.maximum_daily_deaths <= max_daily_deaths);
        passed += u8::from(self.total_deaths_days_50_300 <= max_total_deaths);
        passed += u8::from(self.food_drift_percent() <= 5.0);
        passed += u8::from(self.keyed_zero_days == 0);
        passed += u8::from(self.mean_keyed() >= self.mean_alive() * 0.1);
        passed += u8::from(self.give_food_some_days > 0);
        passed
    }

    fn accepted(&self, population: usize) -> bool {
        self.passed_gates(population) == 6
    }
}

fn candidates() -> Vec<Candidate> {
    let mut result = vec![Candidate {
        name: "sustained_base_0.48".to_string(),
        food: SUSTAINED_FOOD,
        changed_knobs: 0,
        relative_change: 0.0,
    }];

    for value in [
        0.475, 0.47, 0.46, 0.45, 0.44, 0.42, 0.4, 0.35, 0.32, 0.313, 0.3129, 0.3128, 0.31275,
        0.3125, 0.312, 0.31, 0.305, 0.303, 0.30275, 0.3025, 0.302, 0.3015, 0.301, 0.3005, 0.3,
    ] {
        result.push(single_knob_candidate(
            format!("base_metabolic_cost={value}"),
            SUSTAINED_FOOD.metabolic_cost,
            value,
            |food| FoodConfig {
                metabolic_cost: value,
                ..food
            },
        ));
    }
    for value in [0.04, 0.03, 0.02, 0.01, 0.005] {
        result.push(single_knob_candidate(
            format!("health_decay_rate={value}"),
            SUSTAINED_FOOD.health_decay_rate,
            value,
            |food| FoodConfig {
                health_decay_rate: value,
                ..food
            },
        ));
    }
    for value in [0.11, 0.15, 0.2, 0.3, 0.5] {
        result.push(single_knob_candidate(
            format!("regrowth_rate={value}"),
            SUSTAINED_FOOD.regrowth_rate,
            value,
            |food| FoodConfig {
                regrowth_rate: value,
                ..food
            },
        ));
    }
    for value in [11_000.0, 12_000.0, 15_000.0, 20_000.0, 30_000.0] {
        result.push(single_knob_candidate(
            format!("carrying_capacity={value}"),
            SUSTAINED_FOOD.carrying_capacity,
            value,
            |food| FoodConfig {
                carrying_capacity: value,
                ..food
            },
        ));
    }
    for value in [2_100.0, 2_200.0, 2_500.0, 3_000.0, 4_000.0, 6_000.0] {
        result.push(single_knob_candidate(
            format!("initial_settlement_resource={value}"),
            SUSTAINED_FOOD.initial_resource,
            value,
            |food| FoodConfig {
                initial_resource: value,
                ..food
            },
        ));
    }
    for value in [2.1, 2.25, 2.5, 3.0, 4.0] {
        result.push(single_knob_candidate(
            format!("base_work_yield={value}"),
            SUSTAINED_FOOD.work_yield,
            value,
            |food| FoodConfig {
                work_yield: value,
                ..food
            },
        ));
    }
    result
}

fn single_knob_candidate(
    name: String,
    base_value: f32,
    new_value: f32,
    update: impl FnOnce(FoodConfig) -> FoodConfig,
) -> Candidate {
    Candidate {
        name,
        food: update(SUSTAINED_FOOD),
        changed_knobs: 1,
        relative_change: f64::from((new_value - base_value).abs() / base_value.abs()),
    }
}

fn build_config(population: u64, settlements: u32, p_local: f32, food: FoodConfig) -> SimConfig {
    let mut config = SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 config parses");
    config.profile = SimulationProfile::M3;
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.environment.base_metabolic_cost = food.metabolic_cost;
    config.environment.regrowth_rate = food.regrowth_rate;
    config.environment.carrying_capacity = food.carrying_capacity;
    config.environment.health_decay_rate = food.health_decay_rate;
    config.world.initial_settlement_resource = food.initial_resource;
    config.economy.base_work_yield = food.work_yield;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(p_local);
    config.validate().expect("equilibrium config validates");
    config
}

fn mean_alive_food(storage: &sim_model::storage::SegmentedAgentStorage) -> f64 {
    let mut sum = 0.0f64;
    let mut alive = 0u64;
    for (&is_alive, &food) in storage.alive().iter().zip(storage.food()) {
        if is_alive {
            sum += f64::from(food);
            alive += 1;
        }
    }
    if alive == 0 { 0.0 } else { sum / alive as f64 }
}

fn daily_target_counts(outcome: &sim_model::runner::M3DayOutcome) -> (u64, u64) {
    let mut keyed = 0;
    let mut give_food_some = 0;
    for record in &outcome.events {
        if let Event::StateTransition(StateTransitionEvent::FoodTransferred {
            action_kind,
            target_agent_id,
            ..
        }) = &record.event
            && target_agent_id.is_some()
        {
            keyed += 1;
            if *action_kind == TargetedActionKind::GiveFood {
                give_food_some += 1;
            }
        }
    }
    (keyed, give_food_some)
}

fn run_days(
    population: u64,
    settlements: u32,
    p_local: f32,
    food: FoodConfig,
) -> EquilibriumSummary {
    let config = build_config(population, settlements, p_local, food);
    let world = initialize_world(&config).expect("world initializes");
    let mut hybrid_world = HybridWorldState::hybrid(world);
    let run_context = M0RunContext::new(SEED, REPLICATE);
    let mut context = M3StorageRunContext::new(&hybrid_world, &config, &run_context)
        .expect("M3 storage context builds");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    let mut summary = EquilibriumSummary {
        minimum_alive: population as usize,
        ..EquilibriumSummary::default()
    };
    let mut previous_alive = population as usize;
    for day in 1..=SEARCH_DAYS {
        let outcomes = run_m3_storage_days(&mut hybrid_world, &config, &mut context, 1, &options)
            .expect("M3 day runs");
        let outcome = outcomes.first().expect("one daily outcome");
        let storage = hybrid_world.storage().expect("hybrid storage exists");
        let alive = storage.alive().iter().filter(|&&value| value).count();
        let deaths = previous_alive.saturating_sub(alive);
        previous_alive = alive;
        summary.minimum_alive = summary.minimum_alive.min(alive);

        if day == 50 {
            summary.alive_day_50 = alive;
            summary.mean_food_day_50 = mean_alive_food(storage);
        }
        if day == SEARCH_DAYS {
            summary.alive_day_300 = alive;
            summary.mean_food_day_300 = mean_alive_food(storage);
        }
        if day >= FIRST_CHECK_DAY {
            summary.maximum_daily_deaths = summary.maximum_daily_deaths.max(deaths);
            summary.total_deaths_days_50_300 += deaths;
            summary.window_days += 1;
            summary.alive_total += alive as u64;
            let (keyed, give_food_some) = daily_target_counts(outcome);
            summary.keyed_total += keyed;
            summary.give_food_some_days += u32::from(give_food_some > 0);
            summary.keyed_zero_days += u32::from(keyed == 0);
        }
    }
    summary
}

fn print_summary(label: &str, population: usize, summary: &EquilibriumSummary) {
    println!(
        "EQUILIBRIUM_ROW|candidate={label}|min_alive={}|alive_day50={}|alive_day300={}|max_deaths_per_day={}|deaths_days50_300={}|food_day50={:.6}|food_day300={:.6}|food_drift_pct={:.4}|keyed_mean={:.2}|alive_mean={:.2}|keyed_zero_days={}|give_food_some_days={}|gates={}/6|accepted={}",
        summary.minimum_alive,
        summary.alive_day_50,
        summary.alive_day_300,
        summary.maximum_daily_deaths,
        summary.total_deaths_days_50_300,
        summary.mean_food_day_50,
        summary.mean_food_day_300,
        summary.food_drift_percent(),
        summary.mean_keyed(),
        summary.mean_alive(),
        summary.keyed_zero_days,
        summary.give_food_some_days,
        summary.passed_gates(population),
        summary.accepted(population),
    );
}

fn run_grid() -> Option<FoodConfig> {
    println!(
        "EQUILIBRIUM_GRID|N=10000|K=50|k=8|p_local=0.5|seed={SEED}|replicate={REPLICATE}|days={SEARCH_DAYS}|window=50..300"
    );
    println!(
        "EQUILIBRIUM_LIMITS|daily_deaths<=10|total_deaths<=100|food_drift_pct<=5|keyed_zero_days=0|keyed_mean>=10pct_alive|give_food_some_days>0"
    );

    let mut results = Vec::new();
    for candidate in candidates() {
        let summary = run_days(10_000, 50, 0.5, candidate.food);
        println!(
            "EQUILIBRIUM_CHANGE|candidate={}|changed_knobs={}|relative_change_pct={:.3}",
            candidate.name,
            candidate.changed_knobs,
            candidate.relative_change * 100.0
        );
        print_summary(&candidate.name, 10_000, &summary);
        results.push((candidate, summary));
    }

    let mut accepted: Vec<_> = results
        .iter()
        .filter(|(_, summary)| summary.accepted(10_000))
        .collect();
    accepted.sort_by(|(a, _), (b, _)| {
        a.changed_knobs
            .cmp(&b.changed_knobs)
            .then_with(|| a.relative_change.total_cmp(&b.relative_change))
            .then_with(|| a.name.cmp(&b.name))
    });

    if let Some((candidate, _)) = accepted.first() {
        println!(
            "EQUILIBRIUM_SELECTED|candidate={}|changed_knobs={}|relative_change_pct={:.3}",
            candidate.name,
            candidate.changed_knobs,
            candidate.relative_change * 100.0
        );
        return Some(candidate.food);
    }

    let mut closest: Vec<_> = results.iter().collect();
    closest.sort_by(|(_, a), (_, b)| {
        b.passed_gates(10_000)
            .cmp(&a.passed_gates(10_000))
            .then_with(|| a.food_drift_percent().total_cmp(&b.food_drift_percent()))
            .then_with(|| a.total_deaths_days_50_300.cmp(&b.total_deaths_days_50_300))
    });
    if let Some((candidate, summary)) = closest.first() {
        println!(
            "EQUILIBRIUM_NONE_PASSED|closest={}|gates={}/6|fallback=sustained_base_0.48",
            candidate.name,
            summary.passed_gates(10_000)
        );
    }
    None
}

fn main() {
    let Some(equilibrium_food) = run_grid() else {
        return;
    };

    println!(
        "EQUILIBRIUM_VERIFY|N=100000|K=500|k=8|seed={SEED}|replicate={REPLICATE}|days={SEARCH_DAYS}"
    );
    for p_local in [0.0, 0.5, 1.0] {
        let summary = run_days(100_000, 500, p_local, equilibrium_food);
        print_summary(&format!("p_local={p_local}"), 100_000, &summary);
    }
}
