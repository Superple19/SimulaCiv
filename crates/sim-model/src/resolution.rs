use crate::commands::Command;
use crate::config::SimConfig;
use crate::intents::Intent;
use crate::partitioning::SettlementIntentPartition;
use crate::state::WorldState;
use crate::subsystems::Subsystem;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, K_PRIME, RngCoordinate, coordinate_prng_f32, mix64};
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

/// Canonical targeted action kinds for Phase 6B.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TargetedActionKind {
    GiveFood = 3,
    StealFood = 4,
}

impl TargetedActionKind {
    #[inline]
    pub const fn action_kind(self) -> u64 {
        self as u64
    }
}

/// Explicit outcomes of targeted interaction resolution in Phase 6B.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TargetedOutcome {
    ZeroTarget,
    InitiatorIneligible,
    TargetIneligible,
    TheftFailed,
    Applied { amount: f32 },
}

/// Detailed resolution record for a single targeted interaction intent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetedResolution {
    pub group_id: GroupId,
    pub initiator_agent_id: AgentId,
    pub target_agent_id: Option<AgentId>,
    pub action_kind: TargetedActionKind,
    pub resolution_key: Option<u64>,
    pub theft_success_draw: Option<f32>,
    pub outcome: TargetedOutcome,
}

/// Settlement-level summary and report of Phase 6B targeted resolutions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementTargetedResolution {
    pub group_id: GroupId,
    pub zero_target: Vec<TargetedResolution>,
    pub keyed_stream: Vec<TargetedResolution>,
    pub resolutions: Vec<TargetedResolution>,
}

/// Explicit errors returned during Phase 6B structural validation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase6BError {
    MissingSettlement(GroupId),
    MissingInitiator(AgentId),
    MissingTarget(AgentId),
    PartitionGroupMismatch {
        agent_id: AgentId,
        intent_group_id: GroupId,
        partition_group_id: GroupId,
    },
    InvalidRequestedAmount {
        agent_id: AgentId,
        requested_amount: f32,
    },
    InvalidTheftSuccessProbability(f32),
    DuplicatePartition(GroupId),
}

impl std::fmt::Display for Phase6BError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSettlement(gid) => write!(f, "missing settlement: {}", gid),
            Self::MissingInitiator(aid) => write!(f, "missing initiator agent: {}", aid),
            Self::MissingTarget(aid) => write!(f, "missing target agent: {}", aid),
            Self::PartitionGroupMismatch {
                agent_id,
                intent_group_id,
                partition_group_id,
            } => write!(
                f,
                "agent {} partition group mismatch: intent={}, partition={}",
                agent_id, intent_group_id, partition_group_id
            ),
            Self::InvalidRequestedAmount {
                agent_id,
                requested_amount,
            } => write!(
                f,
                "invalid requested amount {} for agent {}",
                requested_amount, agent_id
            ),
            Self::InvalidTheftSuccessProbability(p) => {
                write!(f, "invalid theft success probability: {}", p)
            }
            Self::DuplicatePartition(gid) => {
                write!(f, "duplicate partition for group: {}", gid)
            }
        }
    }
}

impl std::error::Error for Phase6BError {}

/// Computes the exact 64-bit ResolutionKey for Phase 6B GiveFood / StealFood intents.
///
/// Packing:
/// W0 = MasterSeed
/// W1 = ((ReplicateId as u64) << 32) | (Day as u64)
/// W2 = ((6u64) << 48) | ((5u64) << 32) | (group_id as u64)
/// W3 = ((target_agent_id as u64) << 32) | (initiator_agent_id as u64)
/// W4 = action_kind as u64
///
/// Folding:
/// h0 = W0.wrapping_add(K_PRIME)
/// h1 = Mix64(h0 ^ W1)
/// h2 = Mix64(h1 ^ W2)
/// h3 = Mix64(h2 ^ W3)
/// h4 = Mix64(h3 ^ W4)
/// ResolutionKey = Mix64(h4)
#[inline]
pub fn compute_resolution_key(
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    group_id: u16,
    target_agent_id: u32,
    initiator_agent_id: u32,
    action_kind: u64,
) -> u64 {
    let w0 = master_seed;
    let w1 = ((replicate_id as u64) << 32) | (day as u64);
    let w2 = ((6u64) << 48) | ((5u64) << 32) | (group_id as u64);
    let w3 = ((target_agent_id as u64) << 32) | (initiator_agent_id as u64);
    let w4 = action_kind;

    let h0 = w0.wrapping_add(K_PRIME);
    let h1 = mix64(h0 ^ w1);
    let h2 = mix64(h1 ^ w2);
    let h3 = mix64(h2 ^ w3);
    let h4 = mix64(h3 ^ w4);
    mix64(h4)
}

/// Standalone canonical comparator for keyed interactions, supporting collision tie-break verification.
///
/// Primary sort: ResolutionKey ascending.
/// Secondary tie-break: lexicographic (target_agent_id, initiator_agent_id, action_kind) ascending.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn compare_keyed_interactions(
    key_a: u64,
    target_a: AgentId,
    initiator_a: AgentId,
    action_a: TargetedActionKind,
    key_b: u64,
    target_b: AgentId,
    initiator_b: AgentId,
    action_b: TargetedActionKind,
) -> std::cmp::Ordering {
    key_a
        .cmp(&key_b)
        .then_with(|| target_a.cmp(&target_b))
        .then_with(|| initiator_a.cmp(&initiator_b))
        .then_with(|| (action_a as u8).cmp(&(action_b as u8)))
}

/// Internal representation of a keyed interaction intent awaiting sequential resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PlannedKeyedItem {
    key: u64,
    initiator_agent_id: AgentId,
    target_agent_id: AgentId,
    action_kind: TargetedActionKind,
    requested_amount: f32,
}

/// Executes Phase 6B: Targeted Interaction Resolution.
///
/// 1. Structural validation pass over all supplied partitions and config.
/// 2. Deterministic settlement ordering (ascending GroupId).
/// 3. For each settlement:
///    - Zero-target preclassification (ordered by ascending initiator AgentId).
///    - Keyed stream assembly and sorting (by ResolutionKey ascending, tie-breaking lexicographically).
///    - Dynamic live-state validation immediately preceding each interaction.
///    - Immediate sequential authoritative commit via `Command::ModifyFood`.
///    - Live food state strictly conserved for transfers; later intents observe updated food.
pub fn phase6b_targeted_resolution(
    world: &mut WorldState,
    config: &SimConfig,
    partitions: &[SettlementIntentPartition],
) -> Result<Vec<SettlementTargetedResolution>, Phase6BError> {
    // 1. Structural validation pass
    if !config.interaction.theft_success_probability.is_finite()
        || config.interaction.theft_success_probability < 0.0
        || config.interaction.theft_success_probability > 1.0
    {
        return Err(Phase6BError::InvalidTheftSuccessProbability(
            config.interaction.theft_success_probability,
        ));
    }

    let mut seen_groups = HashSet::new();
    for partition in partitions {
        if !seen_groups.insert(partition.group_id) {
            return Err(Phase6BError::DuplicatePartition(partition.group_id));
        }

        if !world
            .settlements
            .iter()
            .any(|s| s.group_id == partition.group_id)
        {
            return Err(Phase6BError::MissingSettlement(partition.group_id));
        }

        for intent in &partition.intents {
            match *intent {
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
                | Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                } => {
                    if group_id != partition.group_id {
                        return Err(Phase6BError::PartitionGroupMismatch {
                            agent_id,
                            intent_group_id: group_id,
                            partition_group_id: partition.group_id,
                        });
                    }

                    if !requested_amount.is_finite() || requested_amount < 0.0 {
                        return Err(Phase6BError::InvalidRequestedAmount {
                            agent_id,
                            requested_amount,
                        });
                    }

                    if !world.agents.iter().any(|a| a.agent_id == agent_id) {
                        return Err(Phase6BError::MissingInitiator(agent_id));
                    }

                    if let Some(target_id) = target_agent_id
                        && !world.agents.iter().any(|a| a.agent_id == target_id)
                    {
                        return Err(Phase6BError::MissingTarget(target_id));
                    }
                }
                _ => {}
            }
        }
    }

    // 2. Canonical settlement ordering: GroupId ascending
    let mut sorted_partitions: Vec<&SettlementIntentPartition> = partitions.iter().collect();
    sorted_partitions.sort_by_key(|p| p.group_id);

    let mut settlement_resolutions = Vec::with_capacity(sorted_partitions.len());

    for partition in sorted_partitions {
        let group_id = partition.group_id;

        let mut zero_target_records = Vec::new();
        let mut keyed_items = Vec::new();

        for intent in &partition.intents {
            match *intent {
                Intent::GiveFood {
                    agent_id,
                    group_id: intent_group,
                    target_agent_id,
                    requested_amount,
                } => match target_agent_id {
                    None => {
                        zero_target_records.push(TargetedResolution {
                            group_id: intent_group,
                            initiator_agent_id: agent_id,
                            target_agent_id: None,
                            action_kind: TargetedActionKind::GiveFood,
                            resolution_key: None,
                            theft_success_draw: None,
                            outcome: TargetedOutcome::ZeroTarget,
                        });
                    }
                    Some(target) => {
                        let key = compute_resolution_key(
                            config.world.master_seed,
                            config.world.replicate_id,
                            world.current_day.as_u32(),
                            intent_group.as_u16(),
                            target.as_u32(),
                            agent_id.as_u32(),
                            TargetedActionKind::GiveFood.action_kind(),
                        );
                        keyed_items.push(PlannedKeyedItem {
                            key,
                            initiator_agent_id: agent_id,
                            target_agent_id: target,
                            action_kind: TargetedActionKind::GiveFood,
                            requested_amount,
                        });
                    }
                },
                Intent::StealFood {
                    agent_id,
                    group_id: intent_group,
                    target_agent_id,
                    requested_amount,
                } => match target_agent_id {
                    None => {
                        zero_target_records.push(TargetedResolution {
                            group_id: intent_group,
                            initiator_agent_id: agent_id,
                            target_agent_id: None,
                            action_kind: TargetedActionKind::StealFood,
                            resolution_key: None,
                            theft_success_draw: None,
                            outcome: TargetedOutcome::ZeroTarget,
                        });
                    }
                    Some(target) => {
                        let key = compute_resolution_key(
                            config.world.master_seed,
                            config.world.replicate_id,
                            world.current_day.as_u32(),
                            intent_group.as_u16(),
                            target.as_u32(),
                            agent_id.as_u32(),
                            TargetedActionKind::StealFood.action_kind(),
                        );
                        keyed_items.push(PlannedKeyedItem {
                            key,
                            initiator_agent_id: agent_id,
                            target_agent_id: target,
                            action_kind: TargetedActionKind::StealFood,
                            requested_amount,
                        });
                    }
                },
                _ => {}
            }
        }

        // Canonical zero-target ordering: ascending initiator AgentId, then action_kind
        zero_target_records.sort_by_key(|r| (r.initiator_agent_id, r.action_kind));

        // Keyed stream ordering: ascending ResolutionKey, then tie-break (target, initiator, action_kind)
        keyed_items.sort_by(|a, b| {
            compare_keyed_interactions(
                a.key,
                a.target_agent_id,
                a.initiator_agent_id,
                a.action_kind,
                b.key,
                b.target_agent_id,
                b.initiator_agent_id,
                b.action_kind,
            )
        });

        // Sequential resolution with immediate authoritative commit
        let mut keyed_resolutions = Vec::with_capacity(keyed_items.len());

        for item in keyed_items {
            let key = item.key;
            let initiator_id = item.initiator_agent_id;
            let target_id = item.target_agent_id;

            // Live-state initiator validation
            let initiator_idx = world
                .agents
                .iter()
                .position(|a| a.agent_id == initiator_id)
                .expect("initiator verified in structural validation");
            let initiator = &world.agents[initiator_idx];

            if !initiator.alive || initiator.health <= 0.0 || initiator.group_id != group_id {
                keyed_resolutions.push(TargetedResolution {
                    group_id,
                    initiator_agent_id: initiator_id,
                    target_agent_id: Some(target_id),
                    action_kind: item.action_kind,
                    resolution_key: Some(key),
                    theft_success_draw: None,
                    outcome: TargetedOutcome::InitiatorIneligible,
                });
                continue;
            }

            match item.action_kind {
                TargetedActionKind::GiveFood => {
                    let target_idx = world
                        .agents
                        .iter()
                        .position(|a| a.agent_id == target_id)
                        .expect("target verified in structural validation");
                    let target = &world.agents[target_idx];
                    let giver = &world.agents[initiator_idx];

                    let target_valid = target.group_id == giver.group_id
                        && target.alive
                        && target.health > 0.0
                        && target.agent_id != giver.agent_id
                        && target.food < config.interaction.starvation_threshold
                        && giver.food > 0.0;

                    if !target_valid {
                        keyed_resolutions.push(TargetedResolution {
                            group_id,
                            initiator_agent_id: initiator_id,
                            target_agent_id: Some(target_id),
                            action_kind: TargetedActionKind::GiveFood,
                            resolution_key: Some(key),
                            theft_success_draw: None,
                            outcome: TargetedOutcome::TargetIneligible,
                        });
                        continue;
                    }

                    let actual_given = item.requested_amount.min(giver.food);
                    if actual_given <= 0.0 {
                        keyed_resolutions.push(TargetedResolution {
                            group_id,
                            initiator_agent_id: initiator_id,
                            target_agent_id: Some(target_id),
                            action_kind: TargetedActionKind::GiveFood,
                            resolution_key: Some(key),
                            theft_success_draw: None,
                            outcome: TargetedOutcome::Applied { amount: 0.0 },
                        });
                        continue;
                    }

                    let cmd = Command::ModifyFood {
                        from: initiator_id,
                        to: target_id,
                        amount: actual_given,
                    };
                    cmd.execute(world)
                        .expect("command execution must succeed on validated state");

                    keyed_resolutions.push(TargetedResolution {
                        group_id,
                        initiator_agent_id: initiator_id,
                        target_agent_id: Some(target_id),
                        action_kind: TargetedActionKind::GiveFood,
                        resolution_key: Some(key),
                        theft_success_draw: None,
                        outcome: TargetedOutcome::Applied {
                            amount: actual_given,
                        },
                    });
                }
                TargetedActionKind::StealFood => {
                    let target_idx = world
                        .agents
                        .iter()
                        .position(|a| a.agent_id == target_id)
                        .expect("target verified in structural validation");
                    let target = &world.agents[target_idx];
                    let thief = &world.agents[initiator_idx];

                    let target_valid = target.group_id == thief.group_id
                        && target.alive
                        && target.health > 0.0
                        && target.agent_id != thief.agent_id
                        && target.food > 0.0;

                    if !target_valid {
                        keyed_resolutions.push(TargetedResolution {
                            group_id,
                            initiator_agent_id: initiator_id,
                            target_agent_id: Some(target_id),
                            action_kind: TargetedActionKind::StealFood,
                            resolution_key: Some(key),
                            theft_success_draw: None,
                            outcome: TargetedOutcome::TargetIneligible,
                        });
                        continue;
                    }

                    // TheftSuccess RNG draw
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        world.current_day.as_u32(),
                        6,
                        Subsystem::TheftSuccess.id(),
                        initiator_id.as_u32(),
                        0,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let p = config.interaction.theft_success_probability;

                    // Theft Success Threshold Rule: u < p
                    let theft_succeeded = u < p;
                    if !theft_succeeded {
                        keyed_resolutions.push(TargetedResolution {
                            group_id,
                            initiator_agent_id: initiator_id,
                            target_agent_id: Some(target_id),
                            action_kind: TargetedActionKind::StealFood,
                            resolution_key: Some(key),
                            theft_success_draw: Some(u),
                            outcome: TargetedOutcome::TheftFailed,
                        });
                        continue;
                    }

                    // Success: actual_stolen = min(requested_amount, victim.food)
                    let victim = &world.agents[target_idx];
                    let actual_stolen = item.requested_amount.min(victim.food);
                    if actual_stolen <= 0.0 {
                        keyed_resolutions.push(TargetedResolution {
                            group_id,
                            initiator_agent_id: initiator_id,
                            target_agent_id: Some(target_id),
                            action_kind: TargetedActionKind::StealFood,
                            resolution_key: Some(key),
                            theft_success_draw: Some(u),
                            outcome: TargetedOutcome::Applied { amount: 0.0 },
                        });
                        continue;
                    }

                    let cmd = Command::ModifyFood {
                        from: target_id,
                        to: initiator_id,
                        amount: actual_stolen,
                    };
                    cmd.execute(world)
                        .expect("command execution must succeed on validated state");

                    keyed_resolutions.push(TargetedResolution {
                        group_id,
                        initiator_agent_id: initiator_id,
                        target_agent_id: Some(target_id),
                        action_kind: TargetedActionKind::StealFood,
                        resolution_key: Some(key),
                        theft_success_draw: Some(u),
                        outcome: TargetedOutcome::Applied {
                            amount: actual_stolen,
                        },
                    });
                }
            }
        }

        let mut all_resolutions = zero_target_records.clone();
        all_resolutions.extend(keyed_resolutions.clone());

        settlement_resolutions.push(SettlementTargetedResolution {
            group_id,
            zero_target: zero_target_records,
            keyed_stream: keyed_resolutions,
            resolutions: all_resolutions,
        });
    }

    Ok(settlement_resolutions)
}
