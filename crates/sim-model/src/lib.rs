//! Domain models, subsystem mappings, configuration, and Day 0 initialization for SimulaCiv.

pub mod config;
pub mod initialization;
pub mod state;
pub mod subsystems;

pub use config::{
    ConfigError, ConfigValidationError, DecisionConfig, EconomyConfig, EnvironmentConfig,
    InteractionConfig, SimConfig, TraitConfig, WorldConfig,
};
pub use initialization::{InitializationError, initialize_world};
pub use state::{AgentState, SettlementState, WorldState};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
