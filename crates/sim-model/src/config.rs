use serde::{Deserialize, Serialize};
use sim_core::Money;

/// Run and world initialization configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorldConfig {
    pub master_seed: u64,
    #[serde(default)]
    pub replicate_id: u32,
    pub initial_population: u64,
    pub settlement_count: u32,
    pub initial_health: f32,
    pub initial_food: f32,
    pub initial_wealth: Money,
    pub initial_settlement_resource: f32,
    pub initial_treasury: Money,
}

/// Agent personality trait distribution bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraitConfig {
    pub prod_min: f32,
    pub prod_max: f32,
    pub coop_min: f32,
    pub coop_max: f32,
    pub aggr_min: f32,
    pub aggr_max: f32,
    pub risk_min: f32,
    pub risk_max: f32,
}

/// Environmental dynamics and biological degradation parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub carrying_capacity: f32,
    pub regrowth_rate: f32,
    pub base_metabolic_cost: f32,
    pub health_decay_rate: f32,
}

/// Economic parameters, market pricing, and fiscal policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EconomyConfig {
    pub base_work_yield: f32,
    pub food_price: Money,
    pub target_food: f32,
    pub target_reserve: Money,
    pub tax_rate: f32,
    pub welfare_payment: Money,
}

/// Targeted interaction parameters (mutual aid, theft, starvation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InteractionConfig {
    pub gift_amount: f32,
    pub theft_amount: f32,
    pub theft_success_probability: f32,
    pub starvation_threshold: f32,
}

/// Utility decision model weights and action biases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionConfig {
    pub decision_temperature: f32,
    pub action_biases: [f32; 6],
    pub base_weight_matrix: [[f32; 5]; 6],
    pub trait_weight_cooperation: f32,
    pub trait_weight_aggression: f32,
    pub trait_weight_risk_tolerance: f32,
}

/// Master configuration container for the SimulaCiv M0 reference model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimConfig {
    pub world: WorldConfig,
    pub traits: TraitConfig,
    pub environment: EnvironmentConfig,
    pub economy: EconomyConfig,
    pub interaction: InteractionConfig,
    pub decision: DecisionConfig,
}

/// Explicit configuration validation errors.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigValidationError {
    NonFiniteValue {
        field: &'static str,
        value: f32,
    },
    InvalidSettlementCount(u32),
    InvalidPopulation(u64),
    InvalidHealth(f32),
    NegativeFood(f32),
    NegativeWealth(Money),
    NegativeTreasury(Money),
    InvalidSettlementResource {
        resource: f32,
        carrying_capacity: f32,
    },
    InvalidTraitRange {
        name: &'static str,
        min: f32,
        max: f32,
    },
    TraitBoundsOutOfBounds {
        name: &'static str,
        min: f32,
        max: f32,
    },
    ProductivityOutOfBounds {
        min: f32,
        max: f32,
    },
    InvalidCarryingCapacity(f32),
    NegativeRegrowthRate(f32),
    NegativeMetabolicCost(f32),
    NegativeHealthDecayRate(f32),
    NegativeWorkYield(f32),
    InvalidFoodPrice(Money),
    InvalidTargetFood(f32),
    InvalidTargetReserve(Money),
    InvalidTaxRate(f32),
    NegativeWelfarePayment(Money),
    NegativeGiftAmount(f32),
    NegativeTheftAmount(f32),
    InvalidProbability {
        name: &'static str,
        value: f32,
    },
    InvalidStarvationThreshold(f32),
    InvalidDecisionTemperature(f32),
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteValue { field, value } => {
                write!(f, "field '{}' contains non-finite value {}", field, value)
            }
            Self::InvalidSettlementCount(c) => {
                write!(f, "settlement_count must be > 0 and <= 65536, got {}", c)
            }
            Self::InvalidPopulation(p) => {
                write!(
                    f,
                    "initial_population must be representable as AgentId (u32), got {}",
                    p
                )
            }
            Self::InvalidHealth(h) => {
                write!(f, "initial_health must be within [0.0, 1.0], got {}", h)
            }
            Self::NegativeFood(fd) => {
                write!(f, "initial_food cannot be negative, got {}", fd)
            }
            Self::NegativeWealth(w) => {
                write!(f, "initial_wealth cannot be negative, got {}", w)
            }
            Self::NegativeTreasury(t) => {
                write!(f, "initial_treasury cannot be negative, got {}", t)
            }
            Self::InvalidSettlementResource {
                resource,
                carrying_capacity,
            } => {
                write!(
                    f,
                    "initial_settlement_resource ({}) must be >= 0.0 and <= carrying_capacity ({})",
                    resource, carrying_capacity
                )
            }
            Self::InvalidTraitRange { name, min, max } => {
                write!(f, "trait '{}' min ({}) exceeds max ({})", name, min, max)
            }
            Self::TraitBoundsOutOfBounds { name, min, max } => {
                write!(
                    f,
                    "trait '{}' bounds [{}, {}] must fall within [0.0, 1.0]",
                    name, min, max
                )
            }
            Self::ProductivityOutOfBounds { min, max } => {
                write!(
                    f,
                    "productivity bounds [{}, {}] must fall within documented range [0.5, 2.5]",
                    min, max
                )
            }
            Self::InvalidCarryingCapacity(c) => {
                write!(f, "carrying_capacity must be > 0.0, got {}", c)
            }
            Self::NegativeRegrowthRate(r) => {
                write!(f, "regrowth_rate cannot be negative, got {}", r)
            }
            Self::NegativeMetabolicCost(m) => {
                write!(f, "base_metabolic_cost cannot be negative, got {}", m)
            }
            Self::NegativeHealthDecayRate(d) => {
                write!(f, "health_decay_rate cannot be negative, got {}", d)
            }
            Self::NegativeWorkYield(y) => {
                write!(f, "base_work_yield cannot be negative, got {}", y)
            }
            Self::InvalidFoodPrice(p) => {
                write!(f, "food_price must be > 0, got {}", p)
            }
            Self::InvalidTargetFood(tf) => {
                write!(f, "target_food must be > 0.0, got {}", tf)
            }
            Self::InvalidTargetReserve(tr) => {
                write!(f, "target_reserve must be > 0, got {}", tr)
            }
            Self::InvalidTaxRate(r) => {
                write!(f, "tax_rate must be within [0.0, 1.0], got {}", r)
            }
            Self::NegativeWelfarePayment(w) => {
                write!(f, "welfare_payment cannot be negative, got {}", w)
            }
            Self::NegativeGiftAmount(g) => {
                write!(f, "gift_amount cannot be negative, got {}", g)
            }
            Self::NegativeTheftAmount(t) => {
                write!(f, "theft_amount cannot be negative, got {}", t)
            }
            Self::InvalidProbability { name, value } => {
                write!(
                    f,
                    "probability '{}' must be within [0.0, 1.0], got {}",
                    name, value
                )
            }
            Self::InvalidStarvationThreshold(s) => {
                write!(f, "starvation_threshold must be > 0.0, got {}", s)
            }
            Self::InvalidDecisionTemperature(t) => {
                write!(f, "decision_temperature must be > 0.0, got {}", t)
            }
        }
    }
}

impl std::error::Error for ConfigValidationError {}

/// Unified configuration loading and validation error.
#[derive(Debug)]
pub enum ConfigError {
    Parse(String),
    Validation(ConfigValidationError),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "configuration parse error: {}", msg),
            Self::Validation(err) => write!(f, "configuration validation error: {}", err),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(_) => None,
            Self::Validation(err) => Some(err),
        }
    }
}

#[inline]
fn check_finite(field: &'static str, value: f32) -> Result<(), ConfigValidationError> {
    if !value.is_finite() {
        Err(ConfigValidationError::NonFiniteValue { field, value })
    } else {
        Ok(())
    }
}

impl SimConfig {
    /// Deserializes a `SimConfig` from a canonical structured TOML string.
    pub fn from_toml_str(toml_str: &str) -> Result<Self, ConfigError> {
        toml::from_str::<SimConfig>(toml_str).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Deserializes and validates a `SimConfig` from a TOML string.
    pub fn parse_and_validate(toml_str: &str) -> Result<Self, ConfigError> {
        let config = Self::from_toml_str(toml_str)?;
        config.validate().map_err(ConfigError::Validation)?;
        Ok(config)
    }

    /// Validates all configuration invariants according to the SimulaCiv M0 specification.
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // 1. Non-finite floating-point checks
        check_finite("initial_health", self.world.initial_health)?;
        check_finite("initial_food", self.world.initial_food)?;
        check_finite(
            "initial_settlement_resource",
            self.world.initial_settlement_resource,
        )?;
        check_finite("prod_min", self.traits.prod_min)?;
        check_finite("prod_max", self.traits.prod_max)?;
        check_finite("coop_min", self.traits.coop_min)?;
        check_finite("coop_max", self.traits.coop_max)?;
        check_finite("aggr_min", self.traits.aggr_min)?;
        check_finite("aggr_max", self.traits.aggr_max)?;
        check_finite("risk_min", self.traits.risk_min)?;
        check_finite("risk_max", self.traits.risk_max)?;
        check_finite("carrying_capacity", self.environment.carrying_capacity)?;
        check_finite("regrowth_rate", self.environment.regrowth_rate)?;
        check_finite("base_metabolic_cost", self.environment.base_metabolic_cost)?;
        check_finite("health_decay_rate", self.environment.health_decay_rate)?;
        check_finite("base_work_yield", self.economy.base_work_yield)?;
        check_finite("target_food", self.economy.target_food)?;
        check_finite("tax_rate", self.economy.tax_rate)?;
        check_finite("gift_amount", self.interaction.gift_amount)?;
        check_finite("theft_amount", self.interaction.theft_amount)?;
        check_finite(
            "theft_success_probability",
            self.interaction.theft_success_probability,
        )?;
        check_finite(
            "starvation_threshold",
            self.interaction.starvation_threshold,
        )?;
        check_finite("decision_temperature", self.decision.decision_temperature)?;

        for (i, &bias) in self.decision.action_biases.iter().enumerate() {
            if !bias.is_finite() {
                return Err(ConfigValidationError::NonFiniteValue {
                    field: "action_biases",
                    value: bias,
                });
            }
            let _ = i;
        }

        for row in &self.decision.base_weight_matrix {
            for &val in row {
                if !val.is_finite() {
                    return Err(ConfigValidationError::NonFiniteValue {
                        field: "base_weight_matrix",
                        value: val,
                    });
                }
            }
        }

        check_finite(
            "trait_weight_cooperation",
            self.decision.trait_weight_cooperation,
        )?;
        check_finite(
            "trait_weight_aggression",
            self.decision.trait_weight_aggression,
        )?;
        check_finite(
            "trait_weight_risk_tolerance",
            self.decision.trait_weight_risk_tolerance,
        )?;

        // 2. World parameter validations
        if self.world.settlement_count == 0 || self.world.settlement_count > (u16::MAX as u32) + 1 {
            return Err(ConfigValidationError::InvalidSettlementCount(
                self.world.settlement_count,
            ));
        }

        if self.world.initial_population > (u32::MAX as u64) {
            return Err(ConfigValidationError::InvalidPopulation(
                self.world.initial_population,
            ));
        }

        if self.world.initial_health < 0.0 || self.world.initial_health > 1.0 {
            return Err(ConfigValidationError::InvalidHealth(
                self.world.initial_health,
            ));
        }

        if self.world.initial_food < 0.0 {
            return Err(ConfigValidationError::NegativeFood(self.world.initial_food));
        }

        if self.world.initial_wealth < 0 {
            return Err(ConfigValidationError::NegativeWealth(
                self.world.initial_wealth,
            ));
        }

        if self.world.initial_treasury < 0 {
            return Err(ConfigValidationError::NegativeTreasury(
                self.world.initial_treasury,
            ));
        }

        // 3. Environment parameter validations
        if self.environment.carrying_capacity <= 0.0 {
            return Err(ConfigValidationError::InvalidCarryingCapacity(
                self.environment.carrying_capacity,
            ));
        }

        if self.world.initial_settlement_resource < 0.0
            || self.world.initial_settlement_resource > self.environment.carrying_capacity
        {
            return Err(ConfigValidationError::InvalidSettlementResource {
                resource: self.world.initial_settlement_resource,
                carrying_capacity: self.environment.carrying_capacity,
            });
        }

        if self.environment.regrowth_rate < 0.0 {
            return Err(ConfigValidationError::NegativeRegrowthRate(
                self.environment.regrowth_rate,
            ));
        }

        if self.environment.base_metabolic_cost < 0.0 {
            return Err(ConfigValidationError::NegativeMetabolicCost(
                self.environment.base_metabolic_cost,
            ));
        }

        if self.environment.health_decay_rate < 0.0 {
            return Err(ConfigValidationError::NegativeHealthDecayRate(
                self.environment.health_decay_rate,
            ));
        }

        // 4. Trait distribution validations
        if self.traits.prod_min > self.traits.prod_max {
            return Err(ConfigValidationError::InvalidTraitRange {
                name: "productivity",
                min: self.traits.prod_min,
                max: self.traits.prod_max,
            });
        }
        if self.traits.prod_min < 0.5 || self.traits.prod_max > 2.5 {
            return Err(ConfigValidationError::ProductivityOutOfBounds {
                min: self.traits.prod_min,
                max: self.traits.prod_max,
            });
        }

        if self.traits.coop_min > self.traits.coop_max {
            return Err(ConfigValidationError::InvalidTraitRange {
                name: "cooperation",
                min: self.traits.coop_min,
                max: self.traits.coop_max,
            });
        }
        if self.traits.coop_min < 0.0 || self.traits.coop_max > 1.0 {
            return Err(ConfigValidationError::TraitBoundsOutOfBounds {
                name: "cooperation",
                min: self.traits.coop_min,
                max: self.traits.coop_max,
            });
        }

        if self.traits.aggr_min > self.traits.aggr_max {
            return Err(ConfigValidationError::InvalidTraitRange {
                name: "aggression",
                min: self.traits.aggr_min,
                max: self.traits.aggr_max,
            });
        }
        if self.traits.aggr_min < 0.0 || self.traits.aggr_max > 1.0 {
            return Err(ConfigValidationError::TraitBoundsOutOfBounds {
                name: "aggression",
                min: self.traits.aggr_min,
                max: self.traits.aggr_max,
            });
        }

        if self.traits.risk_min > self.traits.risk_max {
            return Err(ConfigValidationError::InvalidTraitRange {
                name: "risk_tolerance",
                min: self.traits.risk_min,
                max: self.traits.risk_max,
            });
        }
        if self.traits.risk_min < 0.0 || self.traits.risk_max > 1.0 {
            return Err(ConfigValidationError::TraitBoundsOutOfBounds {
                name: "risk_tolerance",
                min: self.traits.risk_min,
                max: self.traits.risk_max,
            });
        }

        // 5. Economy parameter validations
        if self.economy.base_work_yield < 0.0 {
            return Err(ConfigValidationError::NegativeWorkYield(
                self.economy.base_work_yield,
            ));
        }

        if self.economy.food_price <= 0 {
            return Err(ConfigValidationError::InvalidFoodPrice(
                self.economy.food_price,
            ));
        }

        if self.economy.target_food <= 0.0 {
            return Err(ConfigValidationError::InvalidTargetFood(
                self.economy.target_food,
            ));
        }

        if self.economy.target_reserve <= 0 {
            return Err(ConfigValidationError::InvalidTargetReserve(
                self.economy.target_reserve,
            ));
        }

        if self.economy.tax_rate < 0.0 || self.economy.tax_rate > 1.0 {
            return Err(ConfigValidationError::InvalidTaxRate(self.economy.tax_rate));
        }

        if self.economy.welfare_payment < 0 {
            return Err(ConfigValidationError::NegativeWelfarePayment(
                self.economy.welfare_payment,
            ));
        }

        // 6. Interaction parameter validations
        if self.interaction.gift_amount < 0.0 {
            return Err(ConfigValidationError::NegativeGiftAmount(
                self.interaction.gift_amount,
            ));
        }

        if self.interaction.theft_amount < 0.0 {
            return Err(ConfigValidationError::NegativeTheftAmount(
                self.interaction.theft_amount,
            ));
        }

        if self.interaction.theft_success_probability < 0.0
            || self.interaction.theft_success_probability > 1.0
        {
            return Err(ConfigValidationError::InvalidProbability {
                name: "theft_success_probability",
                value: self.interaction.theft_success_probability,
            });
        }

        if self.interaction.starvation_threshold <= 0.0 {
            return Err(ConfigValidationError::InvalidStarvationThreshold(
                self.interaction.starvation_threshold,
            ));
        }

        // 7. Decision model parameter validations
        if self.decision.decision_temperature <= 0.0 {
            return Err(ConfigValidationError::InvalidDecisionTemperature(
                self.decision.decision_temperature,
            ));
        }

        Ok(())
    }
}
