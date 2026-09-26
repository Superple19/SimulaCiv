//! Domain models, subsystem mappings, configuration, Day 0 initialization, and Phase 1-3 execution for SimulaCiv.

pub mod commands;
pub mod config;
pub mod decision;
pub mod events;
pub mod features;
pub mod hashing;
pub mod initialization;
pub mod intents;
pub mod metrics;
pub mod partitioning;
pub mod phases;
pub mod resolution;
pub mod runner;
pub mod snapshot;
pub mod state;
pub mod storage;
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
pub use events::{
    Event, EventBuffer, EventError, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    StateTransitionEvent, event_from_daily_metrics, event_from_snapshot_emission,
    events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    partition_key_from_group, phase11_flush_events,
};
pub use features::{
    AgentFeatures, FeatureBuffer, FeatureVector, Phase3Error, phase3_observation_and_features,
    phase3_observation_and_features_into, phase3_observation_and_features_soa_into,
    phase3_observation_and_features_storage, phase3_observation_and_features_storage_into,
    phase3_observation_and_features_with_scratch,
};
pub use hashing::{
    CanonicalHash, CanonicalHashError, DOMAIN_EVENTS, DOMAIN_METRICS, DOMAIN_STATE,
    EVENT_CATEGORY_OBSERVATION, EVENT_CATEGORY_STATE_TRANSITION, OBSERVATION_EVENT_DAILY_METRICS,
    OBSERVATION_EVENT_SNAPSHOT_EMITTED, STATE_EVENT_FOOD_TRANSFERRED, STATE_EVENT_MARKET_CLEARED,
    STATE_EVENT_MORTALITY_COMMITTED, STATE_EVENT_WELFARE_DISTRIBUTED, STATE_EVENT_WORK_RESOLVED,
    canonical_event_bytes, canonical_event_hash, canonical_metrics_bytes, canonical_metrics_hash,
    canonical_state_bytes, canonical_state_hash,
};
pub use initialization::{InitializationError, initialize_world};
pub use intents::{
    Intent, IntentError, generate_intents, get_give_food_candidates, get_steal_food_candidates,
    phase4_generate_intents,
};
pub use metrics::{
    DailyMetrics, Phase10Error, compute_wealth_gini_dynamic, compute_wealth_gini_from_slices,
    compute_wealth_gini_soa, phase10_metrics, phase10_metrics_observation, phase10_observe,
    phase10_observe_compact_aos, phase10_observe_segmented, phase10_observe_soa_fresh,
    phase10_observe_storage, phase10_observe_storage_with_scratch, phase10_observe_with_config,
    phase10_observe_with_scratch,
};
pub use partitioning::{Phase5Error, SettlementIntentPartition, phase5_partition_intents};
pub use phases::{
    MortalityResolution, Phase9Error, Phase9MortalityResolution, execute_phases_1_and_2,
    phase1_resource_regrowth, phase2_biological_degradation, phase2_biological_degradation_dynamic,
    phase2_biological_degradation_soa, phase2_biological_degradation_storage,
    phase2_biological_degradation_with_scratch, phase9_mortality_commitment,
    phase9_mortality_commitment_storage, phase9_mortality_commitment_with_config,
    phase9_mortality_resolution, update_biological_degradation,
};
pub use resolution::{
    BuyerMarketResolution, Phase6AError, Phase6BError, Phase7Error, Phase8Error,
    SellerMarketResolution, SettlementMarketResolution, SettlementTargetedResolution,
    SettlementWelfareResolution, SettlementWorkResolution, TargetedActionKind, TargetedOutcome,
    TargetedResolution, WelfareRecipientResolution, WorkAllocation, compare_keyed_interactions,
    compute_resolution_key, phase6a_work_resolution, phase6b_targeted_resolution,
    phase7_market_clearance, phase7_market_clearance_with_config, phase7_market_resolution,
    phase8_welfare_distribution, phase8_welfare_distribution_storage,
    phase8_welfare_distribution_storage_with_config, phase8_welfare_distribution_with_config,
    phase8_welfare_distribution_with_subconfigs, phase8_welfare_resolution,
};
pub use runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, DayOutcome, M0RunContext,
    M0RunError, run_m0_day, run_m0_days,
};
pub use snapshot::{
    CanonicalSnapshot, RestoredSnapshot, SNAPSHOT_MAGIC, SNAPSHOT_SCHEMA_VERSION, SnapshotError,
    SnapshotMetadata, decode_snapshot, encode_snapshot, phase11_snapshot_if_boundary,
    restore_snapshot,
};
pub use state::{
    AgentDynamicSoAScratch, AgentDynamicState, AgentState, SettlementState, WorldState,
};
pub use storage::{
    AosStorageView, DemographyStorage, EconomyStorage, PersonalityStorage, SegmentedAgentStorage,
    WorldStorage, canonical_state_bytes_from_storage, canonical_state_hash_from_storage,
};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
