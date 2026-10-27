use rayon::ThreadPoolBuilder;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, run_hybrid_authority_days,
    run_hybrid_authority_days_with_rayon_phase4,
};
use sim_model::state::HybridWorldState;
use sim_model::{SimConfig, initialize_world};
use std::num::NonZeroUsize;

const BASE_CONFIG: &str = r#"
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
base_weight_matrix = [[0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0, 0.0]]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

const POPULATION: u64 = 20_000;
const SETTLEMENTS: u32 = 2;
const DAYS: u32 = 3;
const CHUNK: NonZeroUsize = NonZeroUsize::new(256).unwrap();

fn config(scale_m2_40_resources: bool) -> Result<SimConfig, Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(BASE_CONFIG)?;
    config.world.initial_population = POPULATION;
    config.world.settlement_count = SETTLEMENTS;
    if scale_m2_40_resources {
        config.environment.carrying_capacity = 1_000.0 * POPULATION as f32;
        config.world.initial_settlement_resource = 200.0 * POPULATION as f32;
    }
    config.validate()?;
    Ok(config)
}

fn run(config: &SimConfig, parallel: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut hybrid = HybridWorldState::hybrid(initialize_world(config)?);
    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };
    if parallel {
        let pool = ThreadPoolBuilder::new().num_threads(6).build()?;
        run_hybrid_authority_days_with_rayon_phase4(
            &mut hybrid,
            config,
            &context,
            DAYS,
            &options,
            &pool,
            CHUNK,
        )?;
    } else {
        run_hybrid_authority_days(&mut hybrid, config, &context, DAYS, &options)?;
    }
    Ok(())
}

fn status(result: Result<(), Box<dyn std::error::Error>>) -> String {
    match result {
        Ok(()) => "ok".to_string(),
        Err(error) => format!("error: {error:?}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    for (label, scaled) in [("adapter-unscaled", false), ("M2-40-scaled", true)] {
        let config = config(scaled)?;
        let serial = status(run(&config, false));
        let rayon = status(run(&config, true));
        println!(
            "{label}|N={POPULATION}|K={SETTLEMENTS}|resource={}|capacity={}|serial={serial}|rayon={rayon}",
            config.world.initial_settlement_resource, config.environment.carrying_capacity
        );
    }
    Ok(())
}
