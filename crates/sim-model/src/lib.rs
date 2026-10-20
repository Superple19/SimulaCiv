//! Domain models, subsystem mappings, configuration, Day 0 initialization, and Phase 1-3 execution for SimulaCiv.
#![deny(clippy::disallowed_methods)]

pub mod commands;
pub mod config;
pub mod counterfactual;
pub mod decision;
pub mod events;
pub mod features;
pub mod hashing;
pub mod initialization;
pub mod intents;
pub mod m3p;
pub mod m3pprop;
pub mod m3prop;
pub mod metrics;
pub mod partitioning;
pub mod phases;
pub mod propagation;
pub mod relationship_graph;
pub mod resolution;
pub mod runner;
pub mod snapshot;
pub mod state;
pub mod storage;
pub mod subsystems;
pub mod system_registry;

pub use commands::{
    BuyerMarketUpdate, Command, CommandExecutionError, SellerMarketUpdate, WelfareRecipientUpdate,
};
pub use config::{
    ConfigError, ConfigValidationError, DecisionConfig, EconomyConfig, EnvironmentConfig,
    InteractionConfig, PopulationConfig, PropagationConfig, SimConfig, SimulationConfig,
    SimulationProfile, SimulationRunManifest, SimulationRunnerSelection, SimulationSystems,
    TraitConfig, WorldConfig,
};
pub use counterfactual::{
    AgentField, AgentFieldValue, AgentMatchKey, BranchDayRecord, BranchDivergenceResult,
    BranchError, BranchEventsRecord, BranchExecutionOption, BranchIntervention,
    BranchMetricsRecord, BranchRequest, BranchResult, ConfigParameter, ConfigValue,
    DailyDifference, DivergenceClassification, DivergenceDayRecord, DivergenceSummary,
    FirstDivergence, ForcedActionOutcome, RecordedSlot, TargetDeathRelation,
    run_counterfactual_branch, run_counterfactual_branch_with_divergence_series,
};
pub use decision::{
    Action, DecisionError, PrimaryActionChoice, evaluate_utilities, phase4_action_selection,
    phase4_primary_action_selection, phase4_primary_action_selection_into,
    phase4_primary_action_selection_storage_into,
    phase4_primary_action_selection_storage_into_baseline,
    phase4_primary_action_selection_storage_into_rayon, select_action, stable_softmax,
};
pub use events::{
    Event, EventBuffer, EventError, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent,
    StateTransitionEvent, event_from_daily_metrics, event_from_snapshot_emission,
    events_from_market_resolution, events_from_mortality_resolution,
    events_from_targeted_resolution, events_from_welfare_resolution, events_from_work_resolution,
    partition_key_from_group, phase11_flush_events,
};
pub use features::{
    AgentFeatures, FeatureBuffer, FeatureVector, Phase3Error, Phase3ScarcityScratch,
    Phase3StorageStageTimings, phase3_observation_and_features,
    phase3_observation_and_features_into, phase3_observation_and_features_soa_into,
    phase3_observation_and_features_storage, phase3_observation_and_features_storage_into,
    phase3_observation_and_features_storage_with_scratch,
    phase3_observation_and_features_storage_with_timing,
    phase3_observation_and_features_with_scratch,
};
pub use hashing::{
    CanonicalHash, CanonicalHashError, DOMAIN_EVENTS, DOMAIN_M3_METRICS, DOMAIN_M3_STATE,
    DOMAIN_M3P_EVENTS, DOMAIN_M3P_METRICS, DOMAIN_M3P_STATE, DOMAIN_M3PROP_EVENTS,
    DOMAIN_M3PROP_METRICS, DOMAIN_M3PROP_STATE, DOMAIN_METRICS, DOMAIN_STATE,
    EVENT_CATEGORY_OBSERVATION, EVENT_CATEGORY_STATE_TRANSITION, M3P_EVENT_BIRTH,
    M3P_EVENT_CLEANUP, OBSERVATION_EVENT_DAILY_METRICS, OBSERVATION_EVENT_SNAPSHOT_EMITTED,
    STATE_EVENT_FOOD_TRANSFERRED, STATE_EVENT_MARKET_CLEARED, STATE_EVENT_MORTALITY_COMMITTED,
    STATE_EVENT_WELFARE_DISTRIBUTED, STATE_EVENT_WORK_RESOLVED, canonical_event_bytes,
    canonical_event_hash, canonical_m3_metrics_bytes, canonical_m3_metrics_hash,
    canonical_m3_state_bytes, canonical_m3_state_bytes_from_storage, canonical_m3_state_hash,
    canonical_m3_state_hash_from_storage, canonical_m3p_event_bytes, canonical_m3p_event_hash,
    canonical_m3p_metrics_bytes, canonical_m3p_metrics_hash, canonical_m3p_state_bytes,
    canonical_m3p_state_bytes_from_storage, canonical_m3p_state_hash,
    canonical_m3p_state_hash_from_storage, canonical_m3pprop_event_bytes,
    canonical_m3pprop_event_hash, canonical_m3pprop_metrics_bytes, canonical_m3pprop_metrics_hash,
    canonical_m3pprop_state_bytes, canonical_m3pprop_state_bytes_from_storage,
    canonical_m3pprop_state_hash, canonical_m3pprop_state_hash_from_storage,
    canonical_m3prop_event_bytes, canonical_m3prop_event_hash, canonical_m3prop_metrics_bytes,
    canonical_m3prop_metrics_hash, canonical_m3prop_state_bytes,
    canonical_m3prop_state_bytes_from_storage, canonical_m3prop_state_hash,
    canonical_m3prop_state_hash_from_storage, canonical_metrics_bytes, canonical_metrics_hash,
    canonical_state_bytes, canonical_state_hash,
};
pub use initialization::{InitializationError, initialize_world};
pub use intents::{
    CrossPartitionCountError, Intent, IntentError, M3Phase4IntentTimings, M3Phase4SlotProjection,
    Phase4CandidateIndexScratch, count_cross_partition_interactions_per_day,
    count_cross_partition_interactions_storage_per_day, generate_intents, generate_intents_m3_into,
    generate_intents_m3_storage_into, generate_intents_m3_storage_into_with_timing,
    generate_intents_storage_into, generate_intents_storage_into_baseline,
    generate_intents_storage_into_variant_c, generate_intents_storage_into_variant_d,
    generate_intents_storage_with_candidate_index,
    generate_intents_storage_with_candidate_index_rayon, generate_intents_storage_with_scratch,
    get_give_food_candidates, get_give_food_candidates_m3, get_steal_food_candidates,
    get_steal_food_candidates_m3, phase4_generate_intents, phase4_generate_intents_into,
    phase4_generate_intents_storage_into,
};
pub use m3p::{
    M3P_BIRTH_SUBSYSTEM_ID, M3P_SNAPSHOT_SCHEMA_VERSION, M3pDailyMetrics, M3pDayOutcome, M3pEvent,
    M3pEventRecord, M3pReferenceState, RestoredM3pSnapshot, decode_m3p_snapshot,
    encode_m3p_snapshot, run_m3p_reference_day,
};
pub use m3pprop::{
    M3pPropagationDailyMetrics, M3pPropagationDayOutcome, M3pPropagationReferenceState,
    RestoredM3pPropagationSnapshot, decode_m3pprop_snapshot, encode_m3pprop_snapshot,
    run_m3pprop_reference_day,
};
pub use m3prop::{
    M3PROP_SNAPSHOT_SCHEMA_VERSION, M3PropDailyMetrics, M3PropDayOutcome, M3PropPopulationMetrics,
    M3PropagationReferenceState, RestoredM3PropagationSnapshot, decode_m3prop_snapshot,
    encode_m3prop_snapshot, run_m3prop_reference_day,
};
pub use metrics::{
    DailyMetrics, M3DailyMetrics, Phase10Error, compute_wealth_gini_dynamic,
    compute_wealth_gini_from_slices, compute_wealth_gini_soa, phase10_metrics,
    phase10_metrics_observation, phase10_observe, phase10_observe_compact_aos,
    phase10_observe_segmented, phase10_observe_soa_fresh, phase10_observe_storage,
    phase10_observe_storage_with_scratch, phase10_observe_with_config,
    phase10_observe_with_scratch,
};
pub use partitioning::{
    Phase5Error, Phase5PartitionScratch, SettlementIntentPartition, phase5_partition_intents,
    phase5_partition_intents_baseline, phase5_partition_intents_from_vec,
    phase5_partition_intents_from_vec_with_scratch,
};
pub use phases::{
    MortalityResolution, Phase9Error, Phase9MortalityResolution, execute_phases_1_and_2,
    phase1_resource_regrowth, phase2_biological_degradation, phase2_biological_degradation_dynamic,
    phase2_biological_degradation_soa, phase2_biological_degradation_storage,
    phase2_biological_degradation_with_scratch, phase9_mortality_commitment,
    phase9_mortality_commitment_storage, phase9_mortality_commitment_with_config,
    phase9_mortality_resolution, update_biological_degradation,
};
pub use propagation::{
    M3_PROPAGATION_SUBSYSTEM_ID, PropagationAgentState, PropagationDailyCounts, apply_phase9c,
    apply_phase9c_m3p, apply_phase9c_storage, apply_phase9c_storage_m3p,
    apply_phase9c_storage_m3p_parallel, apply_phase9c_storage_parallel,
};
pub use relationship_graph::{
    M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID, RelationshipGraph, RelationshipGraphError,
};
pub use resolution::{
    BuyerMarketResolution, M3Phase6BLevelStats, M3Phase6BParallelScratch, M3Phase6BStageTimings,
    M3Phase6BStorageScratch, Phase6AError, Phase6BError, Phase6BResolutionScratch, Phase7Error,
    Phase8Error, Phase8WelfareScratch, SellerMarketResolution, SettlementMarketResolution,
    SettlementTargetedResolution, SettlementWelfareResolution, SettlementWorkResolution,
    TargetedActionKind, TargetedOutcome, TargetedResolution, WelfareRecipientResolution,
    WorkAllocation, compare_keyed_interactions, compute_m3_resolution_key, compute_resolution_key,
    phase6a_work_resolution, phase6a_work_resolution_storage, phase6b_targeted_resolution,
    phase6b_targeted_resolution_m3, phase6b_targeted_resolution_storage,
    phase6b_targeted_resolution_storage_baseline,
    phase6b_targeted_resolution_storage_m3_parallel_with_scratch,
    phase6b_targeted_resolution_storage_m3_parallel_with_timing,
    phase6b_targeted_resolution_storage_m3_with_scratch,
    phase6b_targeted_resolution_storage_m3_with_timing,
    phase6b_targeted_resolution_storage_with_scratch, phase7_market_clearance,
    phase7_market_clearance_storage, phase7_market_clearance_storage_with_config,
    phase7_market_clearance_with_config, phase7_market_resolution, phase8_welfare_distribution,
    phase8_welfare_distribution_storage, phase8_welfare_distribution_storage_full_scan,
    phase8_welfare_distribution_storage_with_config,
    phase8_welfare_distribution_storage_with_config_and_scratch,
    phase8_welfare_distribution_storage_with_scratch, phase8_welfare_distribution_with_config,
    phase8_welfare_distribution_with_subconfigs, phase8_welfare_resolution,
};
pub use runner::{
    ConfiguredDayOutcome, ConfiguredRunOutcome, ConfiguredStorageRunContext,
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, DayOutcome, M0RunContext,
    M0RunError, M3DayOutcome, M3ParallelStorageRunContext, M3PropagationStorageRunContext,
    M3ReferenceContext, M3SearchReductionRunContext, M3StorageRunContext,
    M3pIncrementalTopologyStorageRunContext, M3pPropagationIncrementalTopologyStorageRunContext,
    M3pPropagationStorageRunContext, M3pStorageRunContext, Phase4Backend, Phase4ExecutionPolicy,
    RegisteredCorePhase, run_configured_storage_days, run_hybrid_authority_day,
    run_hybrid_authority_day_with_candidate_index_scratch,
    run_hybrid_authority_day_with_candidate_scratch, run_hybrid_authority_day_with_scratch,
    run_hybrid_authority_days, run_hybrid_authority_days_with_phase3_linear_scan,
    run_hybrid_authority_days_with_phase4_policy, run_hybrid_authority_days_with_phase5_baseline,
    run_hybrid_authority_days_with_phase6b_baseline,
    run_hybrid_authority_days_with_phase8_full_scan, run_hybrid_authority_days_with_rayon_phase4,
    run_hybrid_scope_isolated_day, run_hybrid_scope_isolated_day_with_scratch,
    run_hybrid_scope_isolated_days, run_m0_day, run_m0_days, run_m3_reference_day,
    run_m3_storage_days, run_m3_storage_days_with_parallel_phase6b,
    run_m3_storage_days_with_parallel_phase6b_timing, run_m3_storage_days_with_phase6b_timing,
    run_m3_storage_days_with_search_reduction, run_m3p_propagation_storage_days,
    run_m3p_propagation_storage_days_incremental_topology,
    run_m3p_propagation_storage_days_with_parallel_spread, run_m3p_storage_days,
    run_m3p_storage_days_incremental_topology, run_m3prop_storage_days,
    run_m3prop_storage_days_with_parallel_spread, run_native_soa_day,
    run_native_soa_day_with_storage, run_native_soa_days, run_registered_storage_day_loop,
    run_registered_world_day_loop,
};
pub use snapshot::{
    CanonicalSnapshot, RestoredSnapshot, SNAPSHOT_MAGIC, SNAPSHOT_SCHEMA_VERSION, SnapshotError,
    SnapshotMetadata, decode_snapshot, encode_snapshot, phase11_snapshot_if_boundary,
    restore_snapshot,
};
pub use state::{
    AgentDynamicSoAScratch, AgentDynamicState, AgentState, AuthorityMode, HybridWorldState,
    SettlementState, WorldState,
};
pub use storage::{
    AosStorageView, DemographyStorage, EconomyStorage, PersonalityStorage, SegmentedAgentStorage,
    WorldStorage, canonical_state_bytes_from_storage, canonical_state_hash_from_storage,
};
pub use subsystems::{InvalidSubsystemIdError, Subsystem};
pub use system_registry::{
    ByteOrder, DescriptorProfile, DigestContribution, DigestKind, FeatureSwitchKey, PhaseAccess,
    PhaseSlot, RegisteredHook, RegisteredSystemRunOptions, SYSTEM_REGISTRY, SnapshotSection,
    SystemDescriptor, SystemExecutionOptions, SystemExecutionPolicy, SystemId, SystemRegistryError,
    system_ids_for_features, system_ids_for_runner_selection, validate_configured_system_set,
    validate_system_registry,
};
