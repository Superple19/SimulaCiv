use sim_model::{ConfigError, ConfigValidationError, SimConfig};

const VALID_TOML: &str = r#"
[world]
master_seed = 42
replicate_id = 0
initial_population = 1000
settlement_count = 4
initial_health = 1.0
initial_food = 20.0
initial_wealth = 10000
initial_settlement_resource = 5000.0
initial_treasury = 0

[traits]
prod_min = 0.5
prod_max = 2.5
coop_min = 0.0
coop_max = 1.0
aggr_min = 0.0
aggr_max = 1.0
risk_min = 0.0
risk_max = 1.0

[environment]
carrying_capacity = 10000.0
regrowth_rate = 0.05
base_metabolic_cost = 1.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 10.0
target_reserve = 5000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 4.0
theft_success_probability = 0.7
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

#[test]
fn test_valid_minimal_m0_toml_parses_and_validates() {
    let cfg = SimConfig::parse_and_validate(VALID_TOML).expect("valid config should parse");
    assert_eq!(cfg.world.master_seed, 42);
    assert_eq!(cfg.world.settlement_count, 4);
    assert_eq!(cfg.world.initial_population, 1000);
    assert_eq!(cfg.decision.decision_temperature, 1.0);
}

#[test]
fn test_flat_toml_is_rejected() {
    let flat_toml = r#"
master_seed = 99
replicate_id = 1
initial_population = 500
settlement_count = 2
initial_health = 0.9
initial_food = 15.0
initial_wealth = 5000
initial_settlement_resource = 2500.0
initial_treasury = 100

prod_min = 0.8
prod_max = 1.5
coop_min = 0.1
coop_max = 0.9
aggr_min = 0.2
aggr_max = 0.8
risk_min = 0.3
risk_max = 0.7

carrying_capacity = 5000.0
regrowth_rate = 0.02
base_metabolic_cost = 0.8
health_decay_rate = 0.03

base_work_yield = 1.5
food_price = 80
target_food = 8.0
target_reserve = 3000
tax_rate = 0.05
welfare_payment = 20

gift_amount = 1.0
theft_amount = 2.0
theft_success_probability = 0.5
starvation_threshold = 3.0

decision_temperature = 0.8
action_biases = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
base_weight_matrix = [
  [0.1, 0.1, 0.1, 0.1, 0.1],
  [0.2, 0.2, 0.2, 0.2, 0.2],
  [0.3, 0.3, 0.3, 0.3, 0.3],
  [0.4, 0.4, 0.4, 0.4, 0.4],
  [0.5, 0.5, 0.5, 0.5, 0.5],
  [0.6, 0.6, 0.6, 0.6, 0.6]
]
trait_weight_cooperation = 0.5
trait_weight_aggression = 0.6
trait_weight_risk_tolerance = 0.7
"#;
    let err = SimConfig::parse_and_validate(flat_toml).unwrap_err();
    match err {
        ConfigError::Parse(_) => {}
        other => panic!("expected ConfigError::Parse for flat TOML, got {:?}", other),
    }
}

#[test]
fn test_initial_population_zero_validates() {
    let toml_pop_zero = VALID_TOML.replace("initial_population = 1000", "initial_population = 0");
    let cfg = SimConfig::parse_and_validate(&toml_pop_zero)
        .expect("initial_population = 0 should validate");
    assert_eq!(cfg.world.initial_population, 0);
}

#[test]
fn test_initial_population_exceeding_u32_is_rejected() {
    let toml_large_pop = VALID_TOML.replace(
        "initial_population = 1000",
        "initial_population = 5000000000",
    );
    let err = SimConfig::parse_and_validate(&toml_large_pop).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidPopulation(p)) => {
            assert_eq!(p, 5_000_000_000);
        }
        other => panic!("expected InvalidPopulation, got {:?}", other),
    }
}

#[test]
fn test_initial_settlement_resource_zero_validates() {
    let toml_zero_res = VALID_TOML.replace(
        "initial_settlement_resource = 5000.0",
        "initial_settlement_resource = 0.0",
    );
    let cfg = SimConfig::parse_and_validate(&toml_zero_res)
        .expect("initial_settlement_resource = 0.0 should validate");
    assert_eq!(cfg.world.initial_settlement_resource, 0.0);
}

#[test]
fn test_negative_initial_settlement_resource_is_rejected() {
    let toml_neg_res = VALID_TOML.replace(
        "initial_settlement_resource = 5000.0",
        "initial_settlement_resource = -10.0",
    );
    let err = SimConfig::parse_and_validate(&toml_neg_res).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidSettlementResource {
            resource,
            ..
        }) => {
            assert_eq!(resource, -10.0);
        }
        other => panic!(
            "expected InvalidSettlementResource for negative value, got {:?}",
            other
        ),
    }
}

#[test]
fn test_replicate_id_defaults_to_zero() {
    let toml_without_replicate = VALID_TOML.replace("replicate_id = 0\n", "");
    let cfg = SimConfig::parse_and_validate(&toml_without_replicate)
        .expect("config without replicate_id should default to 0");
    assert_eq!(cfg.world.replicate_id, 0);
}

#[test]
fn test_invalid_settlement_count_zero() {
    let bad_toml = VALID_TOML.replace("settlement_count = 4", "settlement_count = 0");
    let err = SimConfig::parse_and_validate(&bad_toml).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidSettlementCount(0)) => {}
        other => panic!("expected InvalidSettlementCount(0), got {:?}", other),
    }
}

#[test]
fn test_invalid_probability_greater_than_one() {
    let bad_toml = VALID_TOML.replace(
        "theft_success_probability = 0.7",
        "theft_success_probability = 1.5",
    );
    let err = SimConfig::parse_and_validate(&bad_toml).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidProbability {
            name: "theft_success_probability",
            value,
        }) => {
            assert!((value - 1.5).abs() < f32::EPSILON);
        }
        other => panic!("expected InvalidProbability, got {:?}", other),
    }
}

#[test]
fn test_invalid_negative_money_input() {
    let bad_toml = VALID_TOML.replace("initial_wealth = 10000", "initial_wealth = -500");
    let err = SimConfig::parse_and_validate(&bad_toml).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::NegativeWealth(-500)) => {}
        other => panic!("expected NegativeWealth(-500), got {:?}", other),
    }

    let bad_treasury = VALID_TOML.replace("initial_treasury = 0", "initial_treasury = -1");
    let err = SimConfig::parse_and_validate(&bad_treasury).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::NegativeTreasury(-1)) => {}
        other => panic!("expected NegativeTreasury(-1), got {:?}", other),
    }

    let bad_price = VALID_TOML.replace("food_price = 100", "food_price = 0");
    let err = SimConfig::parse_and_validate(&bad_price).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidFoodPrice(0)) => {}
        other => panic!("expected InvalidFoodPrice(0), got {:?}", other),
    }

    let bad_reserve = VALID_TOML.replace("target_reserve = 5000", "target_reserve = 0");
    let err = SimConfig::parse_and_validate(&bad_reserve).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidTargetReserve(0)) => {}
        other => panic!("expected InvalidTargetReserve(0), got {:?}", other),
    }
}

#[test]
fn test_invalid_decision_temperature_zero() {
    let bad_toml = VALID_TOML.replace("decision_temperature = 1.0", "decision_temperature = 0.0");
    let err = SimConfig::parse_and_validate(&bad_toml).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidDecisionTemperature(t)) => {
            assert_eq!(t, 0.0);
        }
        other => panic!("expected InvalidDecisionTemperature(0.0), got {:?}", other),
    }

    let negative_temp =
        VALID_TOML.replace("decision_temperature = 1.0", "decision_temperature = -0.5");
    let err2 = SimConfig::parse_and_validate(&negative_temp).unwrap_err();
    match err2 {
        ConfigError::Validation(ConfigValidationError::InvalidDecisionTemperature(t)) => {
            assert_eq!(t, -0.5);
        }
        other => panic!("expected InvalidDecisionTemperature(-0.5), got {:?}", other),
    }
}

#[test]
fn test_invalid_nan_infinite_values() {
    let nan_toml = VALID_TOML.replace("initial_health = 1.0", "initial_health = nan");
    let err = SimConfig::parse_and_validate(&nan_toml).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::NonFiniteValue { field, .. }) => {
            assert_eq!(field, "initial_health");
        }
        other => panic!("expected NonFiniteValue for NaN, got {:?}", other),
    }

    let inf_toml = VALID_TOML.replace("carrying_capacity = 10000.0", "carrying_capacity = inf");
    let err_inf = SimConfig::parse_and_validate(&inf_toml).unwrap_err();
    match err_inf {
        ConfigError::Validation(ConfigValidationError::NonFiniteValue { field, .. }) => {
            assert_eq!(field, "carrying_capacity");
        }
        other => panic!("expected NonFiniteValue for inf, got {:?}", other),
    }
}

#[test]
fn test_wrong_matrix_vector_dimensions() {
    // action_biases with 5 elements instead of 6
    let wrong_vector = VALID_TOML.replace(
        "action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]",
        "action_biases = [0.0, 0.0, 0.0, 0.0, 0.0]",
    );
    let err_vec = SimConfig::parse_and_validate(&wrong_vector).unwrap_err();
    match err_vec {
        ConfigError::Parse(msg) => {
            assert!(
                msg.contains("invalid length") || msg.contains("expected an array of length 6"),
                "unexpected parse message: {}",
                msg
            );
        }
        other => panic!(
            "expected parse error for wrong vector dimension, got {:?}",
            other
        ),
    }

    // base_weight_matrix with 4 rows instead of 6
    let wrong_matrix_rows = VALID_TOML.replace(
        r#"base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]"#,
        r#"base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]"#,
    );
    let err_rows = SimConfig::parse_and_validate(&wrong_matrix_rows).unwrap_err();
    match err_rows {
        ConfigError::Parse(msg) => {
            assert!(
                msg.contains("invalid length") || msg.contains("expected an array of length 6"),
                "unexpected parse message: {}",
                msg
            );
        }
        other => panic!(
            "expected parse error for wrong row dimension, got {:?}",
            other
        ),
    }

    // base_weight_matrix with row of 3 columns instead of 5
    let wrong_matrix_cols = VALID_TOML.replace("[0.0, 0.0, 0.0, 0.0, 0.0],", "[0.0, 0.0, 0.0],");
    let err_cols = SimConfig::parse_and_validate(&wrong_matrix_cols).unwrap_err();
    match err_cols {
        ConfigError::Parse(msg) => {
            assert!(
                msg.contains("invalid length") || msg.contains("expected an array of length 5"),
                "unexpected parse message: {}",
                msg
            );
        }
        other => panic!(
            "expected parse error for wrong col dimension, got {:?}",
            other
        ),
    }
}

#[test]
fn test_productivity_out_of_bounds() {
    let low_prod = VALID_TOML.replace("prod_min = 0.5", "prod_min = 0.4");
    let err = SimConfig::parse_and_validate(&low_prod).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::ProductivityOutOfBounds { min, .. }) => {
            assert_eq!(min, 0.4);
        }
        other => panic!("expected ProductivityOutOfBounds, got {:?}", other),
    }

    let high_prod = VALID_TOML.replace("prod_max = 2.5", "prod_max = 2.6");
    let err2 = SimConfig::parse_and_validate(&high_prod).unwrap_err();
    match err2 {
        ConfigError::Validation(ConfigValidationError::ProductivityOutOfBounds { max, .. }) => {
            assert_eq!(max, 2.6);
        }
        other => panic!("expected ProductivityOutOfBounds, got {:?}", other),
    }
}

#[test]
fn test_trait_min_greater_than_max() {
    let inverted = VALID_TOML.replace(
        "coop_min = 0.0\ncoop_max = 1.0",
        "coop_min = 0.9\ncoop_max = 0.1",
    );
    let err = SimConfig::parse_and_validate(&inverted).unwrap_err();
    match err {
        ConfigError::Validation(ConfigValidationError::InvalidTraitRange {
            name: "cooperation",
            min,
            max,
        }) => {
            assert_eq!(min, 0.9);
            assert_eq!(max, 0.1);
        }
        other => panic!("expected InvalidTraitRange, got {:?}", other),
    }
}
