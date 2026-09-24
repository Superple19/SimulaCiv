//! Domain models, subsystem mappings, configuration, Day 0 initialization, and Phase 1-3 execution for SimulaCiv.

pub mod config;
pub mod decision;
pub mod features;
pub mod initialization;
pub mod phases;
pub mod state;
pub mod subsystems;

pub use config::{
    ConfigError, ConfigValidationError, DecisionConfig, EconomyConfig, EnvironmentConfig,
    InteractionConfig, SimConfig, TraitConfig, WorldConfig,
};
pub use decision::{
    Action, DecisionError, PrimaryActionChoice, evaluate_utilities, phase4_action_selection,
    phase4_primary_action_selection, select_action, stable_softmax,
};
pub use features::{AgentFeatures, FeatureVector, Phase3Error, phase3_observation_and_features};
pub use initialization::{InitializationError, initialize_world};
pub use phases::{execute_phases_1_and_2, phase1_resource_regrowth, phase2_biological_degradation};
pub use state::{AgentState, SettlementState, WorldState};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
