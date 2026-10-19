//! Static registry metadata for the M3 extension systems.

use crate::config::{SimConfig, SimulationRunnerSelection, SimulationSystems};
use crate::hashing::{
    DOMAIN_EVENTS, DOMAIN_M3_METRICS, DOMAIN_M3_STATE, DOMAIN_M3P_EVENTS, DOMAIN_M3P_METRICS,
    DOMAIN_M3P_STATE, DOMAIN_M3PROP_EVENTS, DOMAIN_M3PROP_METRICS, DOMAIN_M3PROP_STATE,
};
use rayon::ThreadPool;
use std::collections::BTreeSet;
use std::fmt;
use std::num::NonZeroUsize;

/// Stable identity of a registered extension system, independent of its RNG subsystem IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemId {
    RelationshipGraph,
    PopulationDynamics,
    Propagation,
    #[cfg(test)]
    TestNoop,
}

/// Configuration key that selects a registered extension system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeatureSwitchKey {
    RelationshipGraph,
    PopulationDynamics,
    Propagation,
    #[cfg(test)]
    TestNoop,
}

/// A hook insertion point relative to the frozen core phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PhaseSlot {
    Initialization,
    Phase4,
    Phase6B,
    Phase9B,
    Phase9C,
}

/// Explicit execution choice for one registered system; pools remain caller-owned.
#[derive(Clone, Copy)]
pub enum SystemExecutionPolicy<'a> {
    Serial,
    IncrementalTopology,
    Parallel {
        pool: &'a ThreadPool,
        chunk_size: NonZeroUsize,
    },
}

/// Per-system execution choices passed into a generic day loop.
#[derive(Clone, Copy)]
pub struct SystemExecutionOptions<'a> {
    pub relationship_graph: SystemExecutionPolicy<'a>,
    pub population_dynamics: SystemExecutionPolicy<'a>,
    pub propagation: SystemExecutionPolicy<'a>,
}

pub struct RegisteredSystemRunOptions<'a> {
    pub enabled_systems: &'a [SystemId],
    pub execution: SystemExecutionOptions<'a>,
}

impl Default for SystemExecutionOptions<'_> {
    fn default() -> Self {
        Self {
            relationship_graph: SystemExecutionPolicy::Serial,
            population_dynamics: SystemExecutionPolicy::Serial,
            propagation: SystemExecutionPolicy::Serial,
        }
    }
}

impl SystemExecutionOptions<'_> {
    pub fn for_system(&self, system: SystemId) -> SystemExecutionPolicy<'_> {
        match system {
            SystemId::RelationshipGraph => self.relationship_graph,
            SystemId::PopulationDynamics => self.population_dynamics,
            SystemId::Propagation => self.propagation,
            #[cfg(test)]
            SystemId::TestNoop => SystemExecutionPolicy::Serial,
        }
    }
}

/// Legacy byte-compatible profile whose codec a descriptor contributes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DescriptorProfile {
    M3,
    M3p,
    M3Propagation,
    M3pPropagation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DigestKind {
    State,
    Metrics,
    Events,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    LittleEndian,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotSection {
    pub profile: DescriptorProfile,
    pub schema_version: u32,
    pub order: u16,
    pub name: &'static str,
    pub encoding: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DigestContribution {
    pub profile: DescriptorProfile,
    pub kind: DigestKind,
    pub domain_tag: &'static [u8],
    pub byte_order: ByteOrder,
    pub order: u16,
    pub field_sequence: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhaseAccess {
    pub slot: PhaseSlot,
    pub reads: &'static [&'static str],
    pub writes: &'static [&'static str],
    /// A direct deterministic ordering edge for permitted write overlap at this slot.
    pub ordered_after: &'static [SystemId],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemDescriptor {
    pub id: SystemId,
    pub name: &'static str,
    pub feature_switch_key: FeatureSwitchKey,
    pub dependencies: &'static [SystemId],
    pub phase_slots: &'static [PhaseSlot],
    pub rng_subsystem_ids: &'static [u16],
    pub owned_state_columns: &'static [&'static str],
    pub snapshot_sections: &'static [SnapshotSection],
    pub digest_contributions: &'static [DigestContribution],
    pub metrics_fields: &'static [&'static str],
    pub event_kinds: &'static [&'static str],
    pub access: &'static [PhaseAccess],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegisteredHook {
    pub slot: PhaseSlot,
    pub system_id: SystemId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemRegistryError {
    DuplicateSystemId(SystemId),
    DuplicateSystemName(&'static str),
    DuplicateRngSubsystemId(u16),
    MissingDependencyDescriptor {
        system: SystemId,
        dependency: SystemId,
    },
    MissingEnabledDependency {
        system: SystemId,
        dependency: SystemId,
    },
    UnknownEnabledSystem(SystemId),
    DuplicateEnabledSystem(SystemId),
    AccessSlotNotDeclared {
        system: SystemId,
        slot: PhaseSlot,
    },
    UnknownOrderingTarget {
        system: SystemId,
        target: SystemId,
    },
    UndeclaredWriteOverlap {
        first: SystemId,
        second: SystemId,
        slot: PhaseSlot,
        column: &'static str,
    },
    FeatureProfileMapMismatch,
}

impl fmt::Display for SystemRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateSystemId(system) => write!(f, "duplicate system id {system:?}"),
            Self::DuplicateSystemName(name) => write!(f, "duplicate system name {name}"),
            Self::DuplicateRngSubsystemId(id) => {
                write!(f, "duplicate RNG subsystem id {id}")
            }
            Self::MissingDependencyDescriptor { system, dependency } => write!(
                f,
                "system {system:?} depends on unregistered system {dependency:?}"
            ),
            Self::MissingEnabledDependency { system, dependency } => write!(
                f,
                "enabled system {system:?} requires disabled system {dependency:?}"
            ),
            Self::UnknownEnabledSystem(system) => {
                write!(f, "enabled system {system:?} has no descriptor")
            }
            Self::DuplicateEnabledSystem(system) => {
                write!(f, "system {system:?} was enabled more than once")
            }
            Self::AccessSlotNotDeclared { system, slot } => write!(
                f,
                "system {system:?} declares access at undeclared phase slot {slot:?}"
            ),
            Self::UnknownOrderingTarget { system, target } => write!(
                f,
                "system {system:?} orders after unregistered system {target:?}"
            ),
            Self::UndeclaredWriteOverlap {
                first,
                second,
                slot,
                column,
            } => write!(
                f,
                "systems {first:?} and {second:?} both write {column} at {slot:?} without an explicit order"
            ),
            Self::FeatureProfileMapMismatch => write!(
                f,
                "feature switches and legacy runner selection map to different system sets"
            ),
        }
    }
}

impl std::error::Error for SystemRegistryError {}

const M3P_GRAPH_SNAPSHOT: &[SnapshotSection] = &[SnapshotSection {
    profile: DescriptorProfile::M3p,
    schema_version: 2,
    order: 3,
    name: "relationship_graph_csr_and_hash",
    encoding: "canonical AgentId CSR rows and 32-byte CSR hash",
}];
const M3P_POPULATION_SNAPSHOT: &[SnapshotSection] = &[SnapshotSection {
    profile: DescriptorProfile::M3p,
    schema_version: 2,
    order: 1,
    name: "next_agent_id",
    encoding: "u64 little-endian allocator cursor after the base snapshot",
}];
const PROPAGATION_SNAPSHOT_SECTIONS: &[SnapshotSection] = &[
    SnapshotSection {
        profile: DescriptorProfile::M3Propagation,
        schema_version: 3,
        order: 1,
        name: "propagation_manifest_config_and_rows",
        encoding: "frozen v3 feature manifest, parameters, and AgentId-sorted propagation rows",
    },
    SnapshotSection {
        profile: DescriptorProfile::M3pPropagation,
        schema_version: 3,
        order: 1,
        name: "propagation_manifest_config_and_rows",
        encoding: "frozen v3 feature manifest, parameters, and AgentId-sorted propagation rows",
    },
];

const GRAPH_DIGESTS: &[DigestContribution] = &[
    DigestContribution {
        profile: DescriptorProfile::M3,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M1 canonical State fields then relationship CSR hash",
    },
    DigestContribution {
        profile: DescriptorProfile::M3p,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3P_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M1 canonical State fields then relationship CSR hash",
    },
    DigestContribution {
        profile: DescriptorProfile::M3Propagation,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3PROP_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M3 base State fields then relationship CSR hash",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3PROP_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M3P base State fields then relationship CSR hash",
    },
    DigestContribution {
        profile: DescriptorProfile::M3,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M1 daily metrics then edge ratio and cross-partition interactions",
    },
    DigestContribution {
        profile: DescriptorProfile::M3p,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3P_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M3 daily metrics and population fields",
    },
    DigestContribution {
        profile: DescriptorProfile::M3Propagation,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3PROP_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M3 base metrics followed by propagation metrics",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3PROP_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "M3P base metrics followed by propagation metrics",
    },
    DigestContribution {
        profile: DescriptorProfile::M3,
        kind: DigestKind::Events,
        domain_tag: DOMAIN_EVENTS,
        byte_order: ByteOrder::LittleEndian,
        order: 1,
        field_sequence: "existing canonical EventKey and event payload order",
    },
];

const POPULATION_DIGESTS: &[DigestContribution] = &[
    DigestContribution {
        profile: DescriptorProfile::M3p,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3P_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "next_agent_id u64 after M1 State fields and CSR hash",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3PROP_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "next_agent_id u64 in M3P base State fields",
    },
    DigestContribution {
        profile: DescriptorProfile::M3p,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3P_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "births, removed, replaced_edges, live_population, next_agent_id",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3PROP_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "M3P population fields before propagation metrics",
    },
    DigestContribution {
        profile: DescriptorProfile::M3p,
        kind: DigestKind::Events,
        domain_tag: DOMAIN_M3P_EVENTS,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "canonical Birth and Cleanup event tags and payloads",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::Events,
        domain_tag: DOMAIN_M3PROP_EVENTS,
        byte_order: ByteOrder::LittleEndian,
        order: 2,
        field_sequence: "canonical Birth and Cleanup event tags and payloads",
    },
];

const PROPAGATION_DIGESTS: &[DigestContribution] = &[
    DigestContribution {
        profile: DescriptorProfile::M3Propagation,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3PROP_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "AgentId, informed, informed_day rows after M3 base state",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::State,
        domain_tag: DOMAIN_M3PROP_STATE,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "AgentId, informed, informed_day rows after M3P base state",
    },
    DigestContribution {
        profile: DescriptorProfile::M3Propagation,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3PROP_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "six propagation daily counters after M3 metrics",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::Metrics,
        domain_tag: DOMAIN_M3PROP_METRICS,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "six propagation daily counters after M3P metrics",
    },
    DigestContribution {
        profile: DescriptorProfile::M3Propagation,
        kind: DigestKind::Events,
        domain_tag: DOMAIN_M3PROP_EVENTS,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "existing canonical event payloads under propagation domain",
    },
    DigestContribution {
        profile: DescriptorProfile::M3pPropagation,
        kind: DigestKind::Events,
        domain_tag: DOMAIN_M3PROP_EVENTS,
        byte_order: ByteOrder::LittleEndian,
        order: 3,
        field_sequence: "existing canonical event payloads under propagation domain",
    },
];

const GRAPH_PHASE_SLOTS: &[PhaseSlot] = &[
    PhaseSlot::Initialization,
    PhaseSlot::Phase4,
    PhaseSlot::Phase6B,
];
const POPULATION_PHASE_SLOTS: &[PhaseSlot] = &[PhaseSlot::Phase9B];
const PROPAGATION_PHASE_SLOTS: &[PhaseSlot] = &[PhaseSlot::Phase9C];

const GRAPH_ACCESS: &[PhaseAccess] = &[
    PhaseAccess {
        slot: PhaseSlot::Initialization,
        reads: &[
            "agent.agent_id",
            "agent.group_id",
            "run.seed",
            "run.replicate_id",
        ],
        writes: &["relationship_graph.csr", "relationship_graph.csr_hash"],
        ordered_after: &[],
    },
    PhaseAccess {
        slot: PhaseSlot::Phase4,
        reads: &[
            "relationship_graph.csr",
            "agent.alive",
            "agent.health",
            "agent.food",
            "agent.group_id",
        ],
        writes: &["phase4.intent_targets"],
        ordered_after: &[],
    },
    PhaseAccess {
        slot: PhaseSlot::Phase6B,
        reads: &[
            "phase4.intent_targets",
            "agent.alive",
            "agent.health",
            "agent.food",
        ],
        writes: &["agent.food"],
        ordered_after: &[],
    },
];
const POPULATION_ACCESS: &[PhaseAccess] = &[PhaseAccess {
    slot: PhaseSlot::Phase9B,
    reads: &[
        "agent.alive",
        "agent.health",
        "agent.food",
        "agent.group_id",
        "relationship_graph.csr",
        "population.birth_config",
    ],
    writes: &[
        "agent.records",
        "agent.food",
        "relationship_graph.csr",
        "population.next_agent_id",
        "phase11.events",
        "phase10.metrics",
    ],
    ordered_after: &[SystemId::RelationshipGraph],
}];
const PROPAGATION_ACCESS: &[PhaseAccess] = &[PhaseAccess {
    slot: PhaseSlot::Phase9C,
    reads: &[
        "agent.alive",
        "relationship_graph.csr",
        "propagation.informed",
        "propagation.informed_day",
    ],
    writes: &[
        "propagation.informed",
        "propagation.informed_day",
        "phase10.metrics",
    ],
    ordered_after: &[SystemId::PopulationDynamics],
}];

pub static SYSTEM_REGISTRY: &[SystemDescriptor] = &[
    SystemDescriptor {
        id: SystemId::RelationshipGraph,
        name: "relationship_graph",
        feature_switch_key: FeatureSwitchKey::RelationshipGraph,
        dependencies: &[],
        phase_slots: GRAPH_PHASE_SLOTS,
        rng_subsystem_ids: &[6],
        owned_state_columns: &["relationship_graph.csr", "relationship_graph.csr_hash"],
        snapshot_sections: M3P_GRAPH_SNAPSHOT,
        digest_contributions: GRAPH_DIGESTS,
        metrics_fields: &[
            "cross_partition_edge_count",
            "total_directed_edge_count",
            "cross_partition_interaction_count",
        ],
        event_kinds: &[],
        access: GRAPH_ACCESS,
    },
    SystemDescriptor {
        id: SystemId::PopulationDynamics,
        name: "population_dynamics",
        feature_switch_key: FeatureSwitchKey::PopulationDynamics,
        dependencies: &[SystemId::RelationshipGraph],
        phase_slots: POPULATION_PHASE_SLOTS,
        rng_subsystem_ids: &[7],
        owned_state_columns: &["population.next_agent_id"],
        snapshot_sections: M3P_POPULATION_SNAPSHOT,
        digest_contributions: POPULATION_DIGESTS,
        metrics_fields: &[
            "births",
            "removed",
            "replaced_edges",
            "live_population",
            "next_agent_id",
        ],
        event_kinds: &["Birth", "Cleanup"],
        access: POPULATION_ACCESS,
    },
    SystemDescriptor {
        id: SystemId::Propagation,
        name: "propagation",
        feature_switch_key: FeatureSwitchKey::Propagation,
        dependencies: &[SystemId::RelationshipGraph],
        phase_slots: PROPAGATION_PHASE_SLOTS,
        rng_subsystem_ids: &[8],
        owned_state_columns: &["propagation.informed", "propagation.informed_day"],
        snapshot_sections: PROPAGATION_SNAPSHOT_SECTIONS,
        digest_contributions: PROPAGATION_DIGESTS,
        metrics_fields: &[
            "informed_count",
            "newly_informed",
            "forgotten",
            "seeded",
            "levels_used",
            "max_frontier_size",
        ],
        event_kinds: &[],
        access: PROPAGATION_ACCESS,
    },
];

const M2_SYSTEMS: &[SystemId] = &[];
const M3_SYSTEMS: &[SystemId] = &[SystemId::RelationshipGraph];
const M3P_SYSTEMS: &[SystemId] = &[SystemId::RelationshipGraph, SystemId::PopulationDynamics];
const M3_PROPAGATION_SYSTEMS: &[SystemId] = &[SystemId::RelationshipGraph, SystemId::Propagation];
const M3P_PROPAGATION_SYSTEMS: &[SystemId] = &[
    SystemId::RelationshipGraph,
    SystemId::PopulationDynamics,
    SystemId::Propagation,
];

pub fn system_ids_for_runner_selection(
    selection: SimulationRunnerSelection,
) -> &'static [SystemId] {
    match selection {
        SimulationRunnerSelection::M2 => M2_SYSTEMS,
        SimulationRunnerSelection::M3 => M3_SYSTEMS,
        SimulationRunnerSelection::M3p => M3P_SYSTEMS,
        SimulationRunnerSelection::M3Propagation => M3_PROPAGATION_SYSTEMS,
        SimulationRunnerSelection::M3pPropagation => M3P_PROPAGATION_SYSTEMS,
    }
}

pub fn validate_system_registry(
    registry: &[SystemDescriptor],
    enabled_systems: &[SystemId],
) -> Result<Vec<RegisteredHook>, SystemRegistryError> {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut rng_ids = BTreeSet::new();

    for descriptor in registry {
        if !ids.insert(descriptor.id) {
            return Err(SystemRegistryError::DuplicateSystemId(descriptor.id));
        }
        if !names.insert(descriptor.name) {
            return Err(SystemRegistryError::DuplicateSystemName(descriptor.name));
        }
        for &subsystem_id in descriptor.rng_subsystem_ids {
            if !rng_ids.insert(subsystem_id) {
                return Err(SystemRegistryError::DuplicateRngSubsystemId(subsystem_id));
            }
        }
        let mut phase_slots = BTreeSet::new();
        for &slot in descriptor.phase_slots {
            if !phase_slots.insert(slot) {
                return Err(SystemRegistryError::AccessSlotNotDeclared {
                    system: descriptor.id,
                    slot,
                });
            }
        }
        for access in descriptor.access {
            if !phase_slots.contains(&access.slot) {
                return Err(SystemRegistryError::AccessSlotNotDeclared {
                    system: descriptor.id,
                    slot: access.slot,
                });
            }
        }
    }

    for descriptor in registry {
        for &dependency in descriptor.dependencies {
            if !ids.contains(&dependency) {
                return Err(SystemRegistryError::MissingDependencyDescriptor {
                    system: descriptor.id,
                    dependency,
                });
            }
        }
        for access in descriptor.access {
            for &target in access.ordered_after {
                if !ids.contains(&target) {
                    return Err(SystemRegistryError::UnknownOrderingTarget {
                        system: descriptor.id,
                        target,
                    });
                }
            }
        }
    }

    let mut enabled = BTreeSet::new();
    for &system_id in enabled_systems {
        if !enabled.insert(system_id) {
            return Err(SystemRegistryError::DuplicateEnabledSystem(system_id));
        }
        if !ids.contains(&system_id) {
            return Err(SystemRegistryError::UnknownEnabledSystem(system_id));
        }
    }

    let selected: Vec<_> = registry
        .iter()
        .filter(|descriptor| enabled.contains(&descriptor.id))
        .collect();
    for descriptor in &selected {
        for &dependency in descriptor.dependencies {
            if !enabled.contains(&dependency) {
                return Err(SystemRegistryError::MissingEnabledDependency {
                    system: descriptor.id,
                    dependency,
                });
            }
        }
    }

    for first_index in 0..selected.len() {
        for second_index in (first_index + 1)..selected.len() {
            let first = selected[first_index];
            let second = selected[second_index];
            for first_access in first.access {
                for second_access in second.access {
                    if first_access.slot != second_access.slot {
                        continue;
                    }
                    for &column in first_access.writes {
                        if second_access.writes.contains(&column)
                            && !first_access.ordered_after.contains(&second.id)
                            && !second_access.ordered_after.contains(&first.id)
                        {
                            return Err(SystemRegistryError::UndeclaredWriteOverlap {
                                first: first.id,
                                second: second.id,
                                slot: first_access.slot,
                                column,
                            });
                        }
                    }
                }
            }
        }
    }

    let mut hooks: Vec<_> = selected
        .into_iter()
        .flat_map(|descriptor| {
            descriptor
                .phase_slots
                .iter()
                .copied()
                .map(move |slot| RegisteredHook {
                    slot,
                    system_id: descriptor.id,
                })
        })
        .collect();
    hooks.sort_unstable_by_key(|hook| (hook.slot, hook.system_id));
    Ok(hooks)
}

pub fn system_ids_for_features(
    systems: SimulationSystems,
) -> Result<Vec<SystemId>, SystemRegistryError> {
    let mut ids = Vec::with_capacity(3);
    if systems.relationship_graph {
        ids.push(SystemId::RelationshipGraph);
    }
    if systems.population_dynamics {
        ids.push(SystemId::PopulationDynamics);
    }
    if systems.propagation {
        ids.push(SystemId::Propagation);
    }
    validate_system_registry(SYSTEM_REGISTRY, &ids)?;
    Ok(ids)
}

/// Validates the descriptor set before configured runner dispatch without changing its error boundary.
pub fn validate_configured_system_set(config: &SimConfig) -> Result<(), SystemRegistryError> {
    let from_features = system_ids_for_features(config.effective_systems())?;
    let from_selection = system_ids_for_runner_selection(config.runner_selection());
    if from_features != from_selection {
        return Err(SystemRegistryError::FeatureProfileMapMismatch);
    }
    validate_system_registry(SYSTEM_REGISTRY, &from_features).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::{
        DigestKind, FeatureSwitchKey, PhaseAccess, PhaseSlot, SYSTEM_REGISTRY, SystemDescriptor,
        SystemId, SystemRegistryError, system_ids_for_features, system_ids_for_runner_selection,
        validate_system_registry,
    };
    use crate::config::{
        SimConfig, SimulationConfig, SimulationProfile, SimulationRunnerSelection,
        SimulationSystems,
    };

    static NOOP_DESCRIPTOR: SystemDescriptor = SystemDescriptor {
        id: SystemId::TestNoop,
        name: "test_noop",
        feature_switch_key: FeatureSwitchKey::TestNoop,
        dependencies: &[],
        phase_slots: &[],
        rng_subsystem_ids: &[],
        owned_state_columns: &[],
        snapshot_sections: &[],
        digest_contributions: &[],
        metrics_fields: &[],
        event_kinds: &[],
        access: &[],
    };

    static DUPLICATE_RNG_DESCRIPTOR: SystemDescriptor = SystemDescriptor {
        id: SystemId::PopulationDynamics,
        name: "duplicate_rng",
        feature_switch_key: FeatureSwitchKey::PopulationDynamics,
        dependencies: &[],
        phase_slots: &[],
        rng_subsystem_ids: &[6],
        owned_state_columns: &[],
        snapshot_sections: &[],
        digest_contributions: &[],
        metrics_fields: &[],
        event_kinds: &[],
        access: &[],
    };

    static OVERLAP_ACCESS_A: &[PhaseAccess] = &[PhaseAccess {
        slot: PhaseSlot::Phase4,
        reads: &[],
        writes: &["core.agent.food"],
        ordered_after: &[],
    }];
    static OVERLAP_ACCESS_B: &[PhaseAccess] = &[PhaseAccess {
        slot: PhaseSlot::Phase4,
        reads: &[],
        writes: &["core.agent.food"],
        ordered_after: &[],
    }];
    static OVERLAP_DESCRIPTOR_A: SystemDescriptor = SystemDescriptor {
        id: SystemId::RelationshipGraph,
        name: "overlap_a",
        feature_switch_key: FeatureSwitchKey::RelationshipGraph,
        dependencies: &[],
        phase_slots: &[PhaseSlot::Phase4],
        rng_subsystem_ids: &[],
        owned_state_columns: &[],
        snapshot_sections: &[],
        digest_contributions: &[],
        metrics_fields: &[],
        event_kinds: &[],
        access: OVERLAP_ACCESS_A,
    };
    static OVERLAP_DESCRIPTOR_B: SystemDescriptor = SystemDescriptor {
        id: SystemId::PopulationDynamics,
        name: "overlap_b",
        feature_switch_key: FeatureSwitchKey::PopulationDynamics,
        dependencies: &[],
        phase_slots: &[PhaseSlot::Phase4],
        rng_subsystem_ids: &[],
        owned_state_columns: &[],
        snapshot_sections: &[],
        digest_contributions: &[],
        metrics_fields: &[],
        event_kinds: &[],
        access: OVERLAP_ACCESS_B,
    };

    #[test]
    fn descriptor_catalog_contains_the_frozen_system_contributions() {
        assert_eq!(SYSTEM_REGISTRY.len(), 3);
        assert_eq!(SYSTEM_REGISTRY[0].rng_subsystem_ids, &[6]);
        assert_eq!(SYSTEM_REGISTRY[1].rng_subsystem_ids, &[7]);
        assert_eq!(SYSTEM_REGISTRY[2].rng_subsystem_ids, &[8]);
        assert!(
            SYSTEM_REGISTRY[0]
                .digest_contributions
                .iter()
                .any(|contribution| contribution.kind == DigestKind::State)
        );
        assert!(SYSTEM_REGISTRY[1].event_kinds.contains(&"Birth"));
        assert!(
            SYSTEM_REGISTRY[2]
                .snapshot_sections
                .iter()
                .any(|section| section.schema_version == 3)
        );
    }

    #[test]
    fn registry_validation_reports_required_structural_errors() {
        assert_eq!(
            validate_system_registry(
                &[SYSTEM_REGISTRY[0], SYSTEM_REGISTRY[0]],
                &[SystemId::RelationshipGraph],
            ),
            Err(SystemRegistryError::DuplicateSystemId(
                SystemId::RelationshipGraph
            ))
        );

        assert_eq!(
            validate_system_registry(
                &[SYSTEM_REGISTRY[0], DUPLICATE_RNG_DESCRIPTOR],
                &[SystemId::RelationshipGraph, SystemId::PopulationDynamics],
            ),
            Err(SystemRegistryError::DuplicateRngSubsystemId(6))
        );

        assert_eq!(
            validate_system_registry(&[SYSTEM_REGISTRY[1]], &[SystemId::PopulationDynamics],),
            Err(SystemRegistryError::MissingDependencyDescriptor {
                system: SystemId::PopulationDynamics,
                dependency: SystemId::RelationshipGraph,
            })
        );
        assert_eq!(
            validate_system_registry(SYSTEM_REGISTRY, &[SystemId::PopulationDynamics],),
            Err(SystemRegistryError::MissingEnabledDependency {
                system: SystemId::PopulationDynamics,
                dependency: SystemId::RelationshipGraph,
            })
        );

        assert_eq!(
            validate_system_registry(
                &[OVERLAP_DESCRIPTOR_A, OVERLAP_DESCRIPTOR_B],
                &[SystemId::RelationshipGraph, SystemId::PopulationDynamics],
            ),
            Err(SystemRegistryError::UndeclaredWriteOverlap {
                first: SystemId::RelationshipGraph,
                second: SystemId::PopulationDynamics,
                slot: PhaseSlot::Phase4,
                column: "core.agent.food",
            })
        );
    }

    #[test]
    fn registry_hook_order_is_independent_of_descriptor_input_order() {
        let enabled = [
            SystemId::RelationshipGraph,
            SystemId::PopulationDynamics,
            SystemId::Propagation,
        ];
        let forward = validate_system_registry(SYSTEM_REGISTRY, &enabled).unwrap();
        let reverse = SYSTEM_REGISTRY.iter().rev().copied().collect::<Vec<_>>();
        let reversed = validate_system_registry(&reverse, &enabled).unwrap();
        assert_eq!(forward, reversed);
        assert!(forward.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn test_only_noop_descriptor_has_no_runtime_contributions() {
        assert!(NOOP_DESCRIPTOR.phase_slots.is_empty());
        assert!(NOOP_DESCRIPTOR.rng_subsystem_ids.is_empty());
        assert!(NOOP_DESCRIPTOR.owned_state_columns.is_empty());
        assert!(NOOP_DESCRIPTOR.snapshot_sections.is_empty());
        assert!(NOOP_DESCRIPTOR.digest_contributions.is_empty());
        assert!(NOOP_DESCRIPTOR.metrics_fields.is_empty());
        assert!(NOOP_DESCRIPTOR.event_kinds.is_empty());
        assert!(NOOP_DESCRIPTOR.access.is_empty());
        let with_noop = [
            SYSTEM_REGISTRY[0],
            SYSTEM_REGISTRY[1],
            SYSTEM_REGISTRY[2],
            NOOP_DESCRIPTOR,
        ];
        let real_set = [SystemId::RelationshipGraph];
        let noop_set = [SystemId::RelationshipGraph, SystemId::TestNoop];
        assert_eq!(
            validate_system_registry(&with_noop, &real_set),
            validate_system_registry(&with_noop, &noop_set)
        );
    }

    #[test]
    fn test_only_noop_toggle_preserves_legacy_m3_outputs() {
        use crate::hashing::canonical_m3_state_hash;
        use crate::initialization::initialize_world;
        use crate::runner::{
            DayExecutionOptions, M0RunContext, M3ReferenceContext, run_m3_reference_day,
        };

        let mut config: SimConfig =
            toml::from_str(include_str!("../tests/fixtures/m3_propagation.toml")).unwrap();
        config.profile = SimulationProfile::M3;
        config.simulation = None;
        config.propagation = None;
        let initial_world = initialize_world(&config).unwrap();
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);

        let descriptors = [
            SYSTEM_REGISTRY[0],
            SYSTEM_REGISTRY[1],
            SYSTEM_REGISTRY[2],
            NOOP_DESCRIPTOR,
        ];
        let without_noop = [SystemId::RelationshipGraph];
        let with_noop = [SystemId::RelationshipGraph, SystemId::TestNoop];
        assert_eq!(
            validate_system_registry(&descriptors, &without_noop),
            validate_system_registry(&descriptors, &with_noop)
        );

        let mut world_without = initial_world.clone();
        let mut world_with = initial_world;
        let context_without =
            M3ReferenceContext::new(&world_without, &config, &run_context).unwrap();
        let context_with = M3ReferenceContext::new(&world_with, &config, &run_context).unwrap();
        let options = DayExecutionOptions::default();
        let outcome_without =
            run_m3_reference_day(&mut world_without, &config, &context_without, &options).unwrap();
        let outcome_with =
            run_m3_reference_day(&mut world_with, &config, &context_with, &options).unwrap();

        assert_eq!(outcome_without, outcome_with);
        assert_eq!(world_without, world_with);
        assert_eq!(
            canonical_m3_state_hash(&world_without, context_without.relationship_graph()).unwrap(),
            canonical_m3_state_hash(&world_with, context_with.relationship_graph()).unwrap()
        );
    }

    #[test]
    fn feature_profiles_and_runner_selection_choose_the_same_registry_set() {
        let base: SimConfig =
            toml::from_str(include_str!("../tests/fixtures/m3_propagation.toml")).unwrap();
        for (profile, expected_selection) in [
            (SimulationProfile::M2, SimulationRunnerSelection::M2),
            (SimulationProfile::M3, SimulationRunnerSelection::M3),
            (SimulationProfile::M3p, SimulationRunnerSelection::M3p),
        ] {
            let mut config = base.clone();
            config.profile = profile;
            config.simulation = None;
            config.propagation = None;
            assert_eq!(
                config.effective_systems(),
                SimulationSystems::for_profile(profile)
            );
            assert_eq!(config.runner_selection(), expected_selection);
            assert_eq!(
                system_ids_for_features(SimulationSystems::for_profile(profile)).unwrap(),
                system_ids_for_runner_selection(config.runner_selection())
            );
        }

        for (profile, systems, selection) in [
            (
                SimulationProfile::M3,
                SimulationSystems {
                    relationship_graph: true,
                    population_dynamics: false,
                    propagation: true,
                },
                SimulationRunnerSelection::M3Propagation,
            ),
            (
                SimulationProfile::M3p,
                SimulationSystems {
                    relationship_graph: true,
                    population_dynamics: true,
                    propagation: true,
                },
                SimulationRunnerSelection::M3pPropagation,
            ),
        ] {
            let mut config = base.clone();
            config.profile = profile;
            config.simulation = Some(SimulationConfig { systems });
            assert_eq!(config.effective_systems(), systems);
            assert_eq!(config.runner_selection(), selection);
            assert_eq!(
                system_ids_for_features(systems).unwrap(),
                system_ids_for_runner_selection(config.runner_selection())
            );
        }
    }
}
