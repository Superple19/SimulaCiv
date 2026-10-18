use sim_model::{
    ConfigError, ConfigValidationError, DayExecutionOptions, M0RunContext, PopulationConfig,
    SimConfig, SimulationProfile, initialize_world, run_m0_day,
};

const BASE_CONFIG: &str = r#"
[world]
master_seed = 1
replicate_id = 7
initial_population = 20
settlement_count = 2
initial_health = 1.0
initial_food = 5.0
initial_wealth = 1000
initial_settlement_resource = 1.0
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
carrying_capacity = 100.0
regrowth_rate = 0.0
base_metabolic_cost = 0.0
health_decay_rate = 0.0

[economy]
base_work_yield = 0.0
food_price = 1
target_food = 1.0
target_reserve = 1
tax_rate = 0.0
welfare_payment = 0

[interaction]
gift_amount = 0.0
theft_amount = 0.0
theft_success_probability = 0.5
starvation_threshold = 1.0

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
trait_weight_cooperation = 0.0
trait_weight_aggression = 0.0
trait_weight_risk_tolerance = 0.0
"#;

const M3_INTERACTION_FIELDS: &str = "relationship_degree_k = 1\np_local = 0.5";
const POPULATION_FIELDS: &str = "birth_min_health = 0.2\nbirth_food_threshold = 2.0\nbirth_probability = 0.4\nbirth_food_cost = 1.0\nbirths_max_per_day = 0\ncompaction_interval_days = 7";

fn config_text(
    profile: Option<&str>,
    interaction_fields: &str,
    population_fields: Option<&str>,
) -> String {
    let mut source = BASE_CONFIG.replace(
        "[decision]",
        &format!(
            "{interaction_fields}\n{}[decision]",
            population_fields
                .map_or_else(String::new, |fields| format!("\n[population]\n{fields}\n"))
        ),
    );
    if let Some(profile) = profile {
        source = source.replace("[world]", &format!("profile = \"{profile}\"\n[world]"));
    }
    source
}

fn valid_m3p_text() -> String {
    config_text(Some("m3p"), M3_INTERACTION_FIELDS, Some(POPULATION_FIELDS))
}

fn validation_error(source: &str) -> ConfigValidationError {
    match SimConfig::parse_and_validate(source) {
        Err(ConfigError::Validation(error)) => error,
        Err(ConfigError::Parse(error)) => panic!("unexpected parse error: {error}"),
        Ok(_) => panic!("expected validation error"),
    }
}

#[test]
fn m3p_requires_explicit_profile_interaction_and_population_settings() {
    assert_eq!(
        validation_error(&config_text(
            Some("m3p"),
            "p_local = 0.5",
            Some(POPULATION_FIELDS)
        )),
        ConfigValidationError::M3pMissingRelationshipDegreeK
    );
    assert_eq!(
        validation_error(&config_text(
            Some("m3p"),
            "relationship_degree_k = 1",
            Some(POPULATION_FIELDS)
        )),
        ConfigValidationError::M3pMissingLocalityProbability
    );
    assert_eq!(
        validation_error(&config_text(Some("m3p"), M3_INTERACTION_FIELDS, None)),
        ConfigValidationError::MissingPopulationSettings
    );

    for field in [
        "birth_min_health = 0.2\n",
        "birth_food_threshold = 2.0\n",
        "birth_probability = 0.4\n",
        "birth_food_cost = 1.0\n",
        "births_max_per_day = 0\n",
        "compaction_interval_days = 7\n",
    ] {
        let incomplete = valid_m3p_text().replace(field, "");
        assert!(
            matches!(
                SimConfig::parse_and_validate(&incomplete),
                Err(ConfigError::Parse(_))
            ),
            "missing required population field: {}",
            field.trim()
        );
    }
}

#[test]
fn m3p_population_fields_parse_serialize_and_validate() {
    let config = SimConfig::parse_and_validate(&valid_m3p_text()).expect("valid m3p config");
    assert_eq!(config.profile, SimulationProfile::M3p);
    assert_eq!(
        config.population,
        Some(PopulationConfig {
            birth_min_health: 0.2,
            birth_food_threshold: 2.0,
            birth_probability: 0.4,
            birth_food_cost: 1.0,
            births_max_per_day: 0,
            compaction_interval_days: 7,
        })
    );

    let serialized = toml::to_string(&config).expect("m3p config serializes");
    assert!(serialized.contains("profile = \"m3p\""));
    assert!(serialized.contains("[population]"));
    SimConfig::parse_and_validate(&serialized).expect("serialized m3p config validates");
}

#[test]
fn m2_and_m3_reject_population_settings_but_legacy_configs_remain_unchanged() {
    let legacy_m2 = SimConfig::parse_and_validate(&config_text(None, "", None))
        .expect("legacy M2 config remains valid");
    assert_eq!(legacy_m2.profile, SimulationProfile::M2);
    let m2_serialized = toml::to_string(&legacy_m2).expect("legacy M2 config serializes");
    assert!(!m2_serialized.contains("[population]"));

    let legacy_m3 =
        SimConfig::parse_and_validate(&config_text(Some("m3"), M3_INTERACTION_FIELDS, None))
            .expect("existing M3 config remains valid");
    assert_eq!(legacy_m3.profile, SimulationProfile::M3);
    let m3_serialized = toml::to_string(&legacy_m3).expect("existing M3 config serializes");
    assert!(!m3_serialized.contains("[population]"));

    for profile in [None, Some("m2"), Some("m3")] {
        let interaction = if profile == Some("m3") {
            M3_INTERACTION_FIELDS
        } else {
            ""
        };
        assert_eq!(
            validation_error(&config_text(profile, interaction, Some(POPULATION_FIELDS))),
            ConfigValidationError::PopulationSettingsRequireM3pProfile
        );
    }
}

#[test]
fn m3p_birth_parameter_ranges_are_enforced() {
    for value in ["-0.01", "1.01", "nan", "inf", "-inf"] {
        let text = valid_m3p_text().replace(
            "birth_min_health = 0.2",
            &format!("birth_min_health = {value}"),
        );
        assert!(matches!(
            validation_error(&text),
            ConfigValidationError::InvalidBirthMinHealth(_)
        ));
    }

    for (field, default) in [("birth_food_threshold", "2.0"), ("birth_food_cost", "1.0")] {
        for value in ["-0.01", "nan", "inf", "-inf"] {
            let text = valid_m3p_text().replace(
                &format!("{field} = {default}"),
                &format!("{field} = {value}"),
            );
            let error = validation_error(&text);
            let is_expected = matches!(
                (field, &error),
                (
                    "birth_food_threshold",
                    ConfigValidationError::InvalidBirthFoodThreshold(_)
                ) | (
                    "birth_food_cost",
                    ConfigValidationError::InvalidBirthFoodCost(_)
                )
            );
            assert!(is_expected, "field={field} value={value} error={error:?}");
        }
    }

    let threshold_below_cost =
        valid_m3p_text().replace("birth_food_threshold = 2.0", "birth_food_threshold = 0.5");
    assert_eq!(
        validation_error(&threshold_below_cost),
        ConfigValidationError::BirthFoodThresholdBelowCost {
            threshold: 0.5,
            cost: 1.0,
        }
    );

    for value in ["-0.01", "1.01", "nan", "inf", "-inf"] {
        let text = valid_m3p_text().replace(
            "birth_probability = 0.4",
            &format!("birth_probability = {value}"),
        );
        let error = validation_error(&text);
        assert!(matches!(
            error,
            ConfigValidationError::InvalidBirthProbability(_)
        ));
    }

    let zero_interval = valid_m3p_text().replace(
        "compaction_interval_days = 7",
        "compaction_interval_days = 0",
    );
    assert_eq!(
        validation_error(&zero_interval),
        ConfigValidationError::InvalidCompactionIntervalDays(0)
    );
}

#[test]
fn m3p_inclusive_boundaries_and_integer_types_are_validated() {
    let boundaries = valid_m3p_text()
        .replace("birth_min_health = 0.2", "birth_min_health = 0.0")
        .replace("birth_food_threshold = 2.0", "birth_food_threshold = 0.0")
        .replace("birth_probability = 0.4", "birth_probability = 1.0")
        .replace("birth_food_cost = 1.0", "birth_food_cost = 0.0")
        .replace("births_max_per_day = 0", "births_max_per_day = 4294967295")
        .replace(
            "compaction_interval_days = 7",
            "compaction_interval_days = 4294967295",
        );
    SimConfig::parse_and_validate(&boundaries).expect("inclusive m3p boundaries are valid");

    for (field, value) in [
        ("births_max_per_day", "-1"),
        ("compaction_interval_days", "-1"),
        ("compaction_interval_days", "4294967296"),
    ] {
        let text = valid_m3p_text().replace(
            &format!(
                "{field} = {}",
                if field == "births_max_per_day" {
                    "0"
                } else {
                    "7"
                }
            ),
            &format!("{field} = {value}"),
        );
        assert!(
            matches!(
                SimConfig::parse_and_validate(&text),
                Err(ConfigError::Parse(_))
            ),
            "field={field} value={value}"
        );
    }
}

#[test]
fn existing_runners_do_not_accept_the_m3p_profile() {
    let config = SimConfig::parse_and_validate(&valid_m3p_text()).expect("valid m3p config");
    let mut world =
        initialize_world(&config).expect("m3p can initialize its common day-zero state");
    let initial = world.clone();
    let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
    let error = run_m0_day(
        &mut world,
        &config,
        &context,
        &DayExecutionOptions::default(),
    )
    .expect_err("M2/M0 runner must not run m3p semantics");
    assert!(matches!(
        error,
        sim_model::M0RunError::InvariantViolation(_)
    ));
    assert_eq!(world, initial);
}
