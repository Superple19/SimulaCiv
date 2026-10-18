use sim_model::config::PopulationConfig;
use sim_model::runner::{
    DayExecutionOptions, M0RunContext, M3pStorageRunContext, run_m3p_storage_days,
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

fn sustained_config(population: u64, settlements: u32) -> SimConfig {
    let mut config = SimConfig::from_toml_str(M2_40_CONFIG).expect("M2-40 parses");
    config.profile = SimulationProfile::M3p;
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.initial_settlement_resource = 1_000_000.0;
    config.environment.carrying_capacity = 10_000_000.0;
    config.environment.base_metabolic_cost = 1.0;
    config.environment.health_decay_rate = 0.2;
    config.economy.base_work_yield = 1.9;
    config.interaction.relationship_degree_k = Some(8);
    config.interaction.p_local = Some(0.5);
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
    config.validate().expect("sustained M3P config validates");
    config
}

#[test]
fn m3p_sustained_ci_population_stays_within_ten_percent_for_1000_days() {
    let config = sustained_config(1_000, 5);
    let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let world = initialize_world(&config).expect("M3P sustained world initializes");
    let mut hybrid_world = HybridWorldState::hybrid(world);
    let mut context = M3pStorageRunContext::new(&hybrid_world, &config, &run_context)
        .expect("M3P sustained context initializes");
    let options = DayExecutionOptions {
        metrics_enabled: true,
        events_enabled: true,
        snapshot_boundary: false,
    };

    for _ in 0..1_000 {
        let outcome = run_m3p_storage_days(&mut hybrid_world, &config, &mut context, 1, &options)
            .expect("M3P sustained day executes")
            .remove(0);
        let day = outcome.executed_day;
        let alive = outcome
            .metrics
            .expect("metrics are enabled")
            .live_population;
        if (100..=1_000).contains(&day) {
            assert!(
                (900..=1_100).contains(&alive),
                "live population {alive} is outside [900, 1100] on day {day}"
            );
        }
    }
}
