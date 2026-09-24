//! Domain models, subsystem mappings, and configuration for SimulaCiv.

pub mod config;
pub mod subsystems;

pub use config::{
    ConfigError, ConfigValidationError, DecisionConfig, EconomyConfig, EnvironmentConfig,
    InteractionConfig, SimConfig, TraitConfig, WorldConfig,
};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
