//! Domain models, subsystem mappings, configuration, Day 0 initialization, and Phase 1-3 execution for SimulaCiv.

pub mod commands;
pub mod config;
pub mod decision;
pub mod features;
pub mod initialization;
pub mod intents;
pub mod metrics;
pub mod partitioning;
pub mod phases;
pub mod resolution;
pub mod snapshot;
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
pub use metrics::{
    DailyMetrics, Phase10Error, phase10_metrics, phase10_metrics_observation, phase10_observe,
    phase10_observe_with_config,
};
pub use partitioning::{Phase5Error, SettlementIntentPartition, phase5_partition_intents};
pub use phases::{
    MortalityResolution, Phase9Error, Phase9MortalityResolution, execute_phases_1_and_2,
    phase1_resource_regrowth, phase2_biological_degradation, phase9_mortality_commitment,
    phase9_mortality_commitment_with_config, phase9_mortality_resolution,
};
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
pub use snapshot::{
    CanonicalSnapshot, RestoredSnapshot, SNAPSHOT_MAGIC, SNAPSHOT_SCHEMA_VERSION, SnapshotError,
    SnapshotMetadata, decode_snapshot, encode_snapshot, phase11_snapshot_if_boundary,
    restore_snapshot,
};
pub use state::{AgentState, SettlementState, WorldState};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
