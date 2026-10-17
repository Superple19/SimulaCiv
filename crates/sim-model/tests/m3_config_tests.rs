use sim_model::{ConfigValidationError, SimConfig, SimulationProfile};

const BASE_CONFIG: &str = r#"
[world]
master_seed = 1
initial_population = 20
settlement_count = 2
initial_health = 1.0
initial_food = 1.0
initial_wealth = 0
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

fn config_with(profile: Option<&str>, interaction_fields: &str) -> SimConfig {
    let mut source =
        BASE_CONFIG.replace("[decision]", &format!("{interaction_fields}\n[decision]"));
    if let Some(profile) = profile {
        source = source.replace("[world]", &format!("profile = \"{profile}\"\n[world]"));
    }
    SimConfig::from_toml_str(&source).expect("configuration parses")
}

#[test]
fn absent_profile_and_m3_fields_keep_the_m2_defaults_and_serialize_without_optional_fields() {
    let config = config_with(None, "");
    assert_eq!(config.profile, SimulationProfile::M2);
    assert_eq!(config.interaction.relationship_degree_k, None);
    assert_eq!(config.interaction.p_local, None);
    config
        .validate()
        .expect("legacy configuration remains valid");

    let serialized = toml::to_string(&config).expect("configuration serializes");
    assert!(serialized.contains("profile = \"m2\""));
    assert!(!serialized.contains("relationship_degree_k"));
    assert!(!serialized.contains("p_local"));

    let explicit_m2 = config_with(Some("m2"), "");
    explicit_m2
        .validate()
        .expect("explicit M2 profile without M3 fields is valid");
    assert!(
        SimConfig::from_toml_str(&BASE_CONFIG.replace("[world]", "profile = \"m4\"\n[world]"))
            .is_err()
    );
}

#[test]
fn m3_requires_both_fields_and_accepts_closed_boundaries() {
    assert!(matches!(
        config_with(Some("m3"), "p_local = 0.5").validate(),
        Err(ConfigValidationError::MissingRelationshipDegreeK)
    ));
    assert!(matches!(
        config_with(Some("m3"), "relationship_degree_k = 1").validate(),
        Err(ConfigValidationError::MissingLocalityProbability)
    ));

    for (k, p_local) in [(0, "0.0"), (16, "1.0")] {
        let fields = format!("relationship_degree_k = {k}\np_local = {p_local}");
        config_with(Some("m3"), &fields)
            .validate()
            .expect("M3 inclusive config boundaries are valid");
    }
}

#[test]
fn relationship_degree_is_limited_to_zero_through_sixteen() {
    for k in [-1, 17] {
        assert!(matches!(
            config_with(
                Some("m3"),
                &format!("relationship_degree_k = {k}\np_local = 0.5")
            )
            .validate(),
            Err(ConfigValidationError::InvalidRelationshipDegree(_))
        ));
    }
}

#[test]
fn locality_probability_must_be_finite_and_within_zero_through_one() {
    for p_local in ["nan", "inf", "-inf", "-0.01", "1.01"] {
        assert!(
            matches!(
                config_with(
                    Some("m3"),
                    &format!("relationship_degree_k = 1\np_local = {p_local}")
                )
                .validate(),
                Err(ConfigValidationError::InvalidLocalityProbability(_))
            ),
            "p_local={p_local}"
        );
    }
}

#[test]
fn m2_rejects_each_m3_setting_instead_of_ignoring_it() {
    for fields in ["relationship_degree_k = 1", "p_local = 0.5"] {
        assert!(matches!(
            config_with(None, fields).validate(),
            Err(ConfigValidationError::M3SettingsRequireM3Profile)
        ));
    }

    let explicit_m2 = config_with(Some("m2"), "relationship_degree_k = 1\np_local = 0.5");
    assert!(matches!(
        explicit_m2.validate(),
        Err(ConfigValidationError::M3SettingsRequireM3Profile)
    ));
}
