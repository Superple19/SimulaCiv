use crate::intents::Intent;
use crate::partitioning::SettlementIntentPartition;
use crate::state::WorldState;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId};
use std::collections::HashSet;

/// Outcome of a single agent's Work intent during Phase 6A.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkAllocation {
    pub agent_id: AgentId,
    pub group_id: GroupId,
    pub requested_harvest: f32,
    pub allocated_harvest: f32,
}

/// Settlement-level summary of Phase 6A Work resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementWorkResolution {
    pub group_id: GroupId,
    pub resource_before: f32,
    pub resource_after: f32,
    pub total_requested: f32,
    pub total_allocated: f32,
    pub allocations: Vec<WorkAllocation>,
}

/// Explicit errors returned during Phase 6A Work resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase6AError {
    /// A partition references a settlement GroupId that does not exist in authoritative world state.
    MissingSettlement(GroupId),
    /// A Work intent references an AgentId that does not exist in authoritative world state.
    MissingAgent(AgentId),
    /// A Work intent references an agent whose authoritative group_id does not match the intent's group_id.
    GroupMismatch {
        agent_id: AgentId,
        agent_group_id: GroupId,
        intent_group_id: GroupId,
    },
    /// A Work intent's group_id does not match the enclosing partition's group_id.
    PartitionGroupMismatch {
        agent_id: AgentId,
        intent_group_id: GroupId,
        partition_group_id: GroupId,
    },
    /// Multiple Work intents were supplied for the same worker AgentId.
    DuplicateWorker(AgentId),
    /// A worker is not behaviorally eligible (dead or health <= 0.0).
    IneligibleWorker(AgentId),
    /// A Work intent's requested_harvest is negative or non-finite.
    InvalidRequestedHarvest {
        agent_id: AgentId,
        requested_harvest: f32,
    },
    /// A settlement's authoritative resource is negative or non-finite.
    InvalidSettlementResource { group_id: GroupId, resource: f32 },
    /// Total allocated harvest exceeds the settlement's available resource.
    TotalAllocatedExceedsResource {
        group_id: GroupId,
        total_allocated: f32,
        resource: f32,
    },
}

impl std::fmt::Display for Phase6AError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSettlement(gid) => write!(f, "missing settlement: {}", gid),
            Self::MissingAgent(aid) => write!(f, "missing agent: {}", aid),
            Self::GroupMismatch {
                agent_id,
                agent_group_id,
                intent_group_id,
            } => write!(
                f,
                "agent {} group mismatch: state={}, intent={}",
                agent_id, agent_group_id, intent_group_id
            ),
            Self::PartitionGroupMismatch {
                agent_id,
                intent_group_id,
                partition_group_id,
            } => write!(
                f,
                "agent {} partition mismatch: intent={}, partition={}",
                agent_id, intent_group_id, partition_group_id
            ),
            Self::DuplicateWorker(aid) => write!(f, "duplicate work intent for agent: {}", aid),
            Self::IneligibleWorker(aid) => write!(f, "ineligible worker agent: {}", aid),
            Self::InvalidRequestedHarvest {
                agent_id,
                requested_harvest,
            } => write!(
                f,
                "invalid requested harvest {} for agent {}",
                requested_harvest, agent_id
            ),
            Self::InvalidSettlementResource { group_id, resource } => {
                write!(
                    f,
                    "invalid resource {} for settlement {}",
                    resource, group_id
                )
            }
            Self::TotalAllocatedExceedsResource {
                group_id,
                total_allocated,
                resource,
            } => write!(
                f,
                "total allocated {} exceeds settlement {} resource {}",
                total_allocated, group_id, resource
            ),
        }
    }
}

impl std::error::Error for Phase6AError {}

struct PlannedWorkerAllocation {
    agent_id: AgentId,
    requested_harvest: f32,
    allocated_harvest: f32,
}

struct PlannedSettlementResolution {
    group_id: GroupId,
    resource_before: f32,
    resource_after: f32,
    total_requested: f32,
    total_allocated: f32,
    workers: Vec<PlannedWorkerAllocation>,
}

/// Executes Phase 6A: Work Resolution.
///
/// For each settlement partition:
/// 1. Collects all `Intent::Work` intents.
/// 2. Validates workers, requested harvest values, and settlement resource state.
/// 3. Computes harvest allocations using canonical AgentId evaluation order:
///    - If R >= total_requested: allocation = requested_harvest
///    - If R < total_requested: allocation = R * (requested_harvest / total_requested)
///    - If total_requested == 0.0: allocation = 0.0
/// 4. Validates invariants: total_allocated <= R and R_after >= 0.0.
/// 5. Commits mutations atomically: worker food additions and settlement resource deductions.
///
/// If any validation error occurs, authoritative world state remains completely unmodified.
pub fn phase6a_work_resolution(
    world: &mut WorldState,
    partitions: &[SettlementIntentPartition],
) -> Result<Vec<SettlementWorkResolution>, Phase6AError> {
    let mut plans = Vec::with_capacity(partitions.len());
    let mut seen_workers = HashSet::new();

    // Stage A: Validation and Planning across all partitions
    for partition in partitions {
        let settlement = world
            .settlements
            .iter()
            .find(|s| s.group_id == partition.group_id)
            .ok_or(Phase6AError::MissingSettlement(partition.group_id))?;

        if !settlement.resource.is_finite() || settlement.resource < 0.0 {
            return Err(Phase6AError::InvalidSettlementResource {
                group_id: settlement.group_id,
                resource: settlement.resource,
            });
        }

        // Collect and validate all Work intents in this partition
        let mut work_intents = Vec::new();
        for intent in &partition.intents {
            if let Intent::Work {
                agent_id,
                group_id,
                requested_harvest,
            } = *intent
            {
                if group_id != partition.group_id {
                    return Err(Phase6AError::PartitionGroupMismatch {
                        agent_id,
                        intent_group_id: group_id,
                        partition_group_id: partition.group_id,
                    });
                }

                if !requested_harvest.is_finite() || requested_harvest < 0.0 {
                    return Err(Phase6AError::InvalidRequestedHarvest {
                        agent_id,
                        requested_harvest,
                    });
                }

                if !seen_workers.insert(agent_id) {
                    return Err(Phase6AError::DuplicateWorker(agent_id));
                }

                let agent = world
                    .agents
                    .iter()
                    .find(|a| a.agent_id == agent_id)
                    .ok_or(Phase6AError::MissingAgent(agent_id))?;

                if agent.group_id != group_id {
                    return Err(Phase6AError::GroupMismatch {
                        agent_id,
                        agent_group_id: agent.group_id,
                        intent_group_id: group_id,
                    });
                }

                if !agent.is_behaviorally_eligible() {
                    return Err(Phase6AError::IneligibleWorker(agent_id));
                }

                work_intents.push((agent_id, requested_harvest));
            }
        }

        // Canonical aggregation order: strictly ascending initiator AgentId
        work_intents.sort_by_key(|&(agent_id, _)| agent_id);

        let mut total_requested = 0.0f32;
        for &(_, requested) in &work_intents {
            total_requested += requested;
        }

        let resource = settlement.resource;
        let mut planned_workers = Vec::with_capacity(work_intents.len());
        let mut total_allocated = 0.0f32;

        if work_intents.is_empty() || total_requested == 0.0 {
            for &(agent_id, requested_harvest) in &work_intents {
                planned_workers.push(PlannedWorkerAllocation {
                    agent_id,
                    requested_harvest,
                    allocated_harvest: 0.0,
                });
            }
        } else if resource >= total_requested {
            // Sufficient-resource branch
            for &(agent_id, requested_harvest) in &work_intents {
                planned_workers.push(PlannedWorkerAllocation {
                    agent_id,
                    requested_harvest,
                    allocated_harvest: requested_harvest,
                });
                total_allocated += requested_harvest;
            }
        } else {
            // Insufficient-resource branch: proportional rationing
            for &(agent_id, requested_harvest) in &work_intents {
                let share = requested_harvest / total_requested;
                let allocation = resource * share;
                planned_workers.push(PlannedWorkerAllocation {
                    agent_id,
                    requested_harvest,
                    allocated_harvest: allocation,
                });
                total_allocated += allocation;
            }
        }

        if total_allocated > resource {
            return Err(Phase6AError::TotalAllocatedExceedsResource {
                group_id: partition.group_id,
                total_allocated,
                resource,
            });
        }

        let resource_after = resource - total_allocated;

        plans.push(PlannedSettlementResolution {
            group_id: partition.group_id,
            resource_before: resource,
            resource_after,
            total_requested,
            total_allocated,
            workers: planned_workers,
        });
    }

    // Stage B: Atomic Commit to authoritative WorldState
    let mut resolutions = Vec::with_capacity(plans.len());

    for plan in plans {
        // Commit worker food additions in canonical AgentId order
        let mut allocations = Vec::with_capacity(plan.workers.len());
        for worker_plan in plan.workers {
            let agent = world
                .agents
                .iter_mut()
                .find(|a| a.agent_id == worker_plan.agent_id)
                .expect("worker already validated in Stage A");

            agent.food += worker_plan.allocated_harvest;

            allocations.push(WorkAllocation {
                agent_id: worker_plan.agent_id,
                group_id: plan.group_id,
                requested_harvest: worker_plan.requested_harvest,
                allocated_harvest: worker_plan.allocated_harvest,
            });
        }

        // Commit settlement resource deduction
        let settlement = world
            .settlements
            .iter_mut()
            .find(|s| s.group_id == plan.group_id)
            .expect("settlement already validated in Stage A");

        settlement.resource = plan.resource_after;

        resolutions.push(SettlementWorkResolution {
            group_id: plan.group_id,
            resource_before: plan.resource_before,
            resource_after: plan.resource_after,
            total_requested: plan.total_requested,
            total_allocated: plan.total_allocated,
            allocations,
        });
    }

    Ok(resolutions)
}
