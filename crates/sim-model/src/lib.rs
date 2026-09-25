//! Domain models, subsystem mappings, configuration, Day 0 initialization, and Phase 1-3 execution for SimulaCiv.

pub mod commands;
pub mod config;
pub mod decision;
pub mod features;
pub mod initialization;
pub mod intents;
pub mod partitioning;
pub mod phases;
pub mod resolution;
pub mod state;
pub mod subsystems;

pub use commands::{
    BuyerMarketUpdate, Command, CommandExecutionError, SellerMarketUpdate, WelfareRecipientUpdate,
};
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
pub use intents::{
    Intent, IntentError, generate_intents, get_give_food_candidates, get_steal_food_candidates,
    phase4_generate_intents,
};
pub use partitioning::{Phase5Error, SettlementIntentPartition, phase5_partition_intents};
pub use phases::{execute_phases_1_and_2, phase1_resource_regrowth, phase2_biological_degradation};
pub use resolution::{
    BuyerMarketResolution, Phase6AError, Phase6BError, Phase7Error, Phase8Error,
    SellerMarketResolution, SettlementMarketResolution, SettlementTargetedResolution,
    SettlementWelfareResolution, SettlementWorkResolution, TargetedActionKind, TargetedOutcome,
    TargetedResolution, WelfareRecipientResolution, WorkAllocation, compare_keyed_interactions,
    compute_resolution_key, phase6a_work_resolution, phase6b_targeted_resolution,
    phase7_market_clearance, phase7_market_clearance_with_config, phase7_market_resolution,
    phase8_welfare_distribution, phase8_welfare_distribution_with_config,
    phase8_welfare_distribution_with_subconfigs, phase8_welfare_resolution,
};
pub use state::{AgentState, SettlementState, WorldState};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
