use crate::commands::{
    BuyerMarketUpdate, Command, CommandExecutionError, SellerMarketUpdate, WelfareRecipientUpdate,
};
use crate::config::SimConfig;
use crate::intents::Intent;
use crate::partitioning::SettlementIntentPartition;
use crate::state::{AgentState, SettlementState, WorldState};
use crate::storage::SegmentedAgentStorage;
use crate::subsystems::Subsystem;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, K_PRIME, Money, RngCoordinate, coordinate_prng_f32, mix64};
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

/// Canonical resolution record for an individual buyer participating in Phase 7 market clearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuyerMarketResolution {
    pub agent_id: AgentId,
    pub requested_units: f32,
    pub max_affordable_units: f32,
    pub effective_units: f32,
    pub bought_units: f32,
    pub debit: Money,
}

impl BuyerMarketResolution {
    #[inline]
    pub fn requested_demand(&self) -> f32 {
        self.requested_units
    }

    #[inline]
    pub fn effective_demand(&self) -> f32 {
        self.effective_units
    }

    #[inline]
    pub fn bought(&self) -> f32 {
        self.bought_units
    }
}

/// Canonical resolution record for an individual seller participating in Phase 7 market clearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SellerMarketResolution {
    pub agent_id: AgentId,
    pub submitted_units: f32,
    pub effective_units: f32,
    pub sold_units: f32,
    pub seller_share_f32: f32,
    pub seller_net_base: Money,
    pub seller_net: Money,
}

impl SellerMarketResolution {
    #[inline]
    pub fn submitted_supply(&self) -> f32 {
        self.submitted_units
    }

    #[inline]
    pub fn effective_supply(&self) -> f32 {
        self.effective_units
    }

    #[inline]
    pub fn sold(&self) -> f32 {
        self.sold_units
    }
}

/// Authoritative summary of Phase 7 market clearance for a single settlement locality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementMarketResolution {
    pub group_id: GroupId,
    pub food_price: Money,
    pub tax_rate: f32,
    pub total_effective_supply: f32,
    pub total_effective_demand: f32,
    pub total_sold: f32,
    pub total_revenue: Money,
    pub tax_withheld: Money,
    pub net_pool_proceeds: Money,
    pub proceeds_balance: Money,
    pub buyers: Vec<BuyerMarketResolution>,
    pub sellers: Vec<SellerMarketResolution>,
}

/// Explicit errors returned during Phase 7 Market Clearance.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase7Error {
    MissingSettlement(GroupId),
    DuplicatePartition(GroupId),
    MissingAgent(AgentId),
    PartitionGroupMismatch {
        agent_id: AgentId,
        intent_group_id: GroupId,
        partition_group_id: GroupId,
    },
    AgentGroupMismatch {
        agent_id: AgentId,
        agent_group_id: GroupId,
        partition_group_id: GroupId,
    },
    IneligibleParticipant(AgentId),
    DuplicateParticipant(AgentId),
    InvalidFoodPrice(Money),
    InvalidTaxRate(f32),
    InvalidRequestedDemand {
        agent_id: AgentId,
        requested_demand: f32,
    },
    InvalidSubmittedSupply {
        agent_id: AgentId,
        submitted_supply: f32,
    },
    NegativeAgentFood {
        agent_id: AgentId,
        food: f32,
    },
    NegativeAgentWealth {
        agent_id: AgentId,
        wealth: Money,
    },
    NegativeTreasury {
        group_id: GroupId,
        treasury: Money,
    },
    FinancialOverflow,
    FinancialImbalance {
        total_debit: Money,
        total_payout: Money,
        tax_withheld: Money,
    },
    NegativeProceedsReconciliationStall {
        group_id: GroupId,
        remaining_balance: Money,
    },
    CommandExecution(CommandExecutionError),
    InvariantViolation(String),
}

impl std::fmt::Display for Phase7Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSettlement(gid) => write!(f, "settlement missing: {}", gid),
            Self::DuplicatePartition(gid) => {
                write!(f, "duplicate partition for settlement: {}", gid)
            }
            Self::MissingAgent(aid) => write!(f, "market agent missing: {}", aid),
            Self::PartitionGroupMismatch {
                agent_id,
                intent_group_id,
                partition_group_id,
            } => write!(
                f,
                "agent {} intent group {} does not match partition group {}",
                agent_id, intent_group_id, partition_group_id
            ),
            Self::AgentGroupMismatch {
                agent_id,
                agent_group_id,
                partition_group_id,
            } => write!(
                f,
                "agent {} authoritative group {} does not match partition group {}",
                agent_id, agent_group_id, partition_group_id
            ),
            Self::IneligibleParticipant(aid) => {
                write!(f, "ineligible participant in market clearance: {}", aid)
            }
            Self::DuplicateParticipant(aid) => {
                write!(f, "duplicate participant in market clearance: {}", aid)
            }
            Self::InvalidFoodPrice(price) => write!(f, "invalid market food price: {}", price),
            Self::InvalidTaxRate(rate) => write!(f, "invalid market tax rate: {}", rate),
            Self::InvalidRequestedDemand {
                agent_id,
                requested_demand,
            } => write!(
                f,
                "invalid requested demand for agent {}: {}",
                agent_id, requested_demand
            ),
            Self::InvalidSubmittedSupply {
                agent_id,
                submitted_supply,
            } => write!(
                f,
                "invalid submitted supply for agent {}: {}",
                agent_id, submitted_supply
            ),
            Self::NegativeAgentFood { agent_id, food } => {
                write!(f, "agent {} has negative food: {}", agent_id, food)
            }
            Self::NegativeAgentWealth { agent_id, wealth } => {
                write!(f, "agent {} has negative wealth: {}", agent_id, wealth)
            }
            Self::NegativeTreasury { group_id, treasury } => {
                write!(
                    f,
                    "settlement {} has negative treasury: {}",
                    group_id, treasury
                )
            }
            Self::FinancialOverflow => write!(f, "financial arithmetic overflow in Phase 7"),
            Self::FinancialImbalance {
                total_debit,
                total_payout,
                tax_withheld,
            } => write!(
                f,
                "financial imbalance: total_debit {} != total_payout {} + tax_withheld {}",
                total_debit, total_payout, tax_withheld
            ),
            Self::NegativeProceedsReconciliationStall {
                group_id,
                remaining_balance,
            } => write!(
                f,
                "negative proceeds reconciliation stall for group {}: remaining balance {}",
                group_id, remaining_balance
            ),
            Self::CommandExecution(err) => write!(f, "command execution failed: {}", err),
            Self::InvariantViolation(msg) => write!(f, "phase 7 invariant violation: {}", msg),
        }
    }
}

impl std::error::Error for Phase7Error {}

impl From<CommandExecutionError> for Phase7Error {
    fn from(err: CommandExecutionError) -> Self {
        Self::CommandExecution(err)
    }
}

struct PlannedSettlementMarket {
    group_id: GroupId,
    food_price: Money,
    tax_rate: f32,
    total_effective_supply: f32,
    total_effective_demand: f32,
    total_sold: f32,
    total_revenue: Money,
    tax_withheld: Money,
    net_pool_proceeds: Money,
    proceeds_balance: Money,
    buyers: Vec<BuyerMarketResolution>,
    sellers: Vec<SellerMarketResolution>,
    buyer_updates: Vec<BuyerMarketUpdate>,
    seller_updates: Vec<SellerMarketUpdate>,
}

/// Resolves Phase 7 Fixed-Price Pooled Settlement Market Clearance and Tax Settlement.
///
/// Execution order:
/// 1. Validate parameters (`food_price > 0`, finite and non-negative `tax_rate`).
/// 2. Canonical settlement ordering: ascending `GroupId`.
/// 3. For each settlement independently:
///    - Validate all `BuyFood` and `SellFood` participants and structural consistency.
///    - Order buyers and sellers strictly by ascending stable `AgentId`.
///    - Reconcile seller supply with live food: `effective_supply = min(submitted, food)`.
///    - Evaluate buyer affordability: `max_affordable_units = wealth / food_price`,
///      `effective_demand = min(requested, max_affordable_units as f32)`.
///    - Canonical sequential `f32` summation of `total_effective_supply` and `total_effective_demand`.
///    - Zero-volume check: if either is 0.0, clearance is a zero-op.
///    - Otherwise, clear pool:
///      - If `total_effective_supply >= total_effective_demand` (sufficient supply):
///        `seller_share = effective_supply / total_effective_supply`,
///        `sold = seller_share * total_effective_demand`, `bought = effective_demand`.
///      - Else (supply deficit):
///        `buyer_share = effective_demand / total_effective_demand`,
///        `bought = buyer_share * total_effective_supply`, `sold = effective_supply`.
///    - Buyer debit: `gross_f64 = bought as f64 * food_price as f64`, `debit = floor(gross_f64) as Money`,
///      `TotalRevenue = sum(debit)`.
///    - Tax withholding: `tax_f64 = TotalRevenue as f64 * tax_rate as f64`,
///      `tax_withheld = floor(tax_f64) as Money`, `net_pool_proceeds = TotalRevenue - tax_withheld`.
///    - Seller base proceeds: for participating sellers (`sold > 0.0`),
///      `seller_share_f32 = sold / total_sold`,
///      `seller_base_f64 = net_pool_proceeds as f64 * seller_share_f32 as f64`,
///      `seller_net_base = floor(seller_base_f64) as Money`.
///    - Signed reconciliation:
///      `seller_base_total = sum(seller_net_base)`,
///      `proceeds_balance = net_pool_proceeds - seller_base_total`.
///      - If `proceeds_balance > 0`: cycle through participating sellers in ascending `AgentId` adding `+1`.
///      - If `proceeds_balance < 0`: cycle through participating sellers with `seller_net > 0` subtracting `-1`.
///    - Verify financial invariants: `sum(seller_net) == net_pool_proceeds` and `TotalRevenue == sum(seller_net) + tax_withheld`.
/// 4. Atomic execution across all settlements: execute `Command::MarketClearance` only if all partitions pass Stage A.
pub fn phase7_market_clearance(
    world: &mut WorldState,
    partitions: &[SettlementIntentPartition],
    food_price: Money,
    tax_rate: f32,
) -> Result<Vec<SettlementMarketResolution>, Phase7Error> {
    if food_price <= 0 {
        return Err(Phase7Error::InvalidFoodPrice(food_price));
    }
    if !tax_rate.is_finite() || tax_rate < 0.0 || tax_rate > 1.0 {
        return Err(Phase7Error::InvalidTaxRate(tax_rate));
    }

    // Canonical settlement ordering: GroupId ascending
    let mut sorted_partitions: Vec<&SettlementIntentPartition> = partitions.iter().collect();
    sorted_partitions.sort_by_key(|p| p.group_id);

    let mut seen_groups = HashSet::new();
    for p in &sorted_partitions {
        if !seen_groups.insert(p.group_id) {
            return Err(Phase7Error::DuplicatePartition(p.group_id));
        }
    }

    let mut seen_participants = HashSet::new();
    let mut planned_settlements = Vec::with_capacity(sorted_partitions.len());

    // Stage A: Validation and Planning across all partitions
    for partition in sorted_partitions {
        let settlement = world
            .settlements
            .iter()
            .find(|s| s.group_id == partition.group_id)
            .ok_or(Phase7Error::MissingSettlement(partition.group_id))?;

        if settlement.treasury < 0 {
            return Err(Phase7Error::NegativeTreasury {
                group_id: settlement.group_id,
                treasury: settlement.treasury,
            });
        }

        let mut raw_buyers = Vec::new();
        let mut raw_sellers = Vec::new();

        for intent in &partition.intents {
            match intent {
                Intent::BuyFood {
                    agent_id,
                    group_id,
                    requested_demand,
                } => {
                    if *group_id != partition.group_id {
                        return Err(Phase7Error::PartitionGroupMismatch {
                            agent_id: *agent_id,
                            intent_group_id: *group_id,
                            partition_group_id: partition.group_id,
                        });
                    }
                    if !requested_demand.is_finite() || *requested_demand < 0.0 {
                        return Err(Phase7Error::InvalidRequestedDemand {
                            agent_id: *agent_id,
                            requested_demand: *requested_demand,
                        });
                    }
                    if !seen_participants.insert(*agent_id) {
                        return Err(Phase7Error::DuplicateParticipant(*agent_id));
                    }
                    let agent = world
                        .agents
                        .iter()
                        .find(|a| a.agent_id == *agent_id)
                        .ok_or(Phase7Error::MissingAgent(*agent_id))?;
                    if agent.group_id != partition.group_id {
                        return Err(Phase7Error::AgentGroupMismatch {
                            agent_id: *agent_id,
                            agent_group_id: agent.group_id,
                            partition_group_id: partition.group_id,
                        });
                    }
                    if !agent.is_behaviorally_eligible() {
                        return Err(Phase7Error::IneligibleParticipant(*agent_id));
                    }
                    if agent.wealth < 0 {
                        return Err(Phase7Error::NegativeAgentWealth {
                            agent_id: *agent_id,
                            wealth: agent.wealth,
                        });
                    }
                    raw_buyers.push((*agent_id, *requested_demand, agent.wealth));
                }
                Intent::SellFood {
                    agent_id,
                    group_id,
                    submitted_supply,
                } => {
                    if *group_id != partition.group_id {
                        return Err(Phase7Error::PartitionGroupMismatch {
                            agent_id: *agent_id,
                            intent_group_id: *group_id,
                            partition_group_id: partition.group_id,
                        });
                    }
                    if !submitted_supply.is_finite() || *submitted_supply < 0.0 {
                        return Err(Phase7Error::InvalidSubmittedSupply {
                            agent_id: *agent_id,
                            submitted_supply: *submitted_supply,
                        });
                    }
                    if !seen_participants.insert(*agent_id) {
                        return Err(Phase7Error::DuplicateParticipant(*agent_id));
                    }
                    let agent = world
                        .agents
                        .iter()
                        .find(|a| a.agent_id == *agent_id)
                        .ok_or(Phase7Error::MissingAgent(*agent_id))?;
                    if agent.group_id != partition.group_id {
                        return Err(Phase7Error::AgentGroupMismatch {
                            agent_id: *agent_id,
                            agent_group_id: agent.group_id,
                            partition_group_id: partition.group_id,
                        });
                    }
                    if !agent.is_behaviorally_eligible() {
                        return Err(Phase7Error::IneligibleParticipant(*agent_id));
                    }
                    if !agent.food.is_finite() || agent.food < 0.0 {
                        return Err(Phase7Error::NegativeAgentFood {
                            agent_id: *agent_id,
                            food: agent.food,
                        });
                    }
                    if agent.wealth < 0 {
                        return Err(Phase7Error::NegativeAgentWealth {
                            agent_id: *agent_id,
                            wealth: agent.wealth,
                        });
                    }
                    raw_sellers.push((*agent_id, *submitted_supply, agent.food));
                }
                _ => {}
            }
        }

        // Canonical participant ordering: strictly ascending AgentId
        raw_buyers.sort_by_key(|&(agent_id, _, _)| agent_id);
        raw_sellers.sort_by_key(|&(agent_id, _, _)| agent_id);

        let mut planned_buyers: Vec<BuyerMarketResolution> = Vec::with_capacity(raw_buyers.len());
        let mut total_effective_demand = 0.0f32;
        for &(agent_id, requested_demand, wealth) in &raw_buyers {
            let max_affordable_units = wealth / food_price;
            let max_affordable_food = max_affordable_units as f32;
            let effective_demand = requested_demand.min(max_affordable_food);
            total_effective_demand += effective_demand;
            planned_buyers.push(BuyerMarketResolution {
                agent_id,
                requested_units: requested_demand,
                max_affordable_units: max_affordable_food,
                effective_units: effective_demand,
                bought_units: 0.0,
                debit: 0,
            });
        }

        let mut planned_sellers: Vec<SellerMarketResolution> =
            Vec::with_capacity(raw_sellers.len());
        let mut total_effective_supply = 0.0f32;
        for &(agent_id, submitted_supply, live_food) in &raw_sellers {
            let effective_supply = submitted_supply.min(live_food);
            total_effective_supply += effective_supply;
            planned_sellers.push(SellerMarketResolution {
                agent_id,
                submitted_units: submitted_supply,
                effective_units: effective_supply,
                sold_units: 0.0,
                seller_share_f32: 0.0,
                seller_net_base: 0,
                seller_net: 0,
            });
        }

        let (total_sold, total_revenue, tax_withheld, net_pool_proceeds, proceeds_balance) =
            if total_effective_supply == 0.0 || total_effective_demand == 0.0 {
                // Zero-volume deterministic zero-op
                (0.0f32, 0, 0, 0, 0)
            } else {
                // Food-pool clearance
                if total_effective_supply >= total_effective_demand {
                    // Sufficient supply
                    for b in &mut planned_buyers {
                        b.bought_units = b.effective_units;
                    }
                    for s in &mut planned_sellers {
                        let seller_share = s.effective_units / total_effective_supply;
                        s.sold_units = seller_share * total_effective_demand;
                    }
                } else {
                    // Supply deficit
                    for b in &mut planned_buyers {
                        let buyer_share = b.effective_units / total_effective_demand;
                        b.bought_units = buyer_share * total_effective_supply;
                    }
                    for s in &mut planned_sellers {
                        s.sold_units = s.effective_units;
                    }
                }

                // Buyer debit conversion
                let mut rev: Money = 0;
                for b in &mut planned_buyers {
                    let gross_f64 = (b.bought_units as f64) * (food_price as f64);
                    if !gross_f64.is_finite() || gross_f64 < 0.0 {
                        return Err(Phase7Error::InvariantViolation(
                            "gross debit non-finite or negative".into(),
                        ));
                    }
                    let debit = gross_f64.floor() as Money;
                    if debit < 0 {
                        return Err(Phase7Error::InvariantViolation(
                            "negative buyer debit".into(),
                        ));
                    }
                    b.debit = debit;
                    rev = rev
                        .checked_add(debit)
                        .ok_or(Phase7Error::FinancialOverflow)?;
                }

                // Tax withholding
                let tax_f64 = (rev as f64) * (tax_rate as f64);
                if !tax_f64.is_finite() || tax_f64 < 0.0 {
                    return Err(Phase7Error::InvariantViolation(
                        "tax calculation non-finite or negative".into(),
                    ));
                }
                let tax = tax_f64.floor() as Money;
                if tax < 0 {
                    return Err(Phase7Error::InvariantViolation(
                        "negative tax withheld".into(),
                    ));
                }
                let net = rev.checked_sub(tax).ok_or(Phase7Error::FinancialOverflow)?;

                // Seller base proceeds
                let mut total_sold = 0.0f32;
                for s in &planned_sellers {
                    if s.sold_units > 0.0 {
                        total_sold += s.sold_units;
                    }
                }

                for s in &mut planned_sellers {
                    if s.sold_units > 0.0 {
                        let seller_share_f32 = s.sold_units / total_sold;
                        let seller_base_f64 = (net as f64) * (seller_share_f32 as f64);
                        if !seller_base_f64.is_finite() || seller_base_f64 < 0.0 {
                            return Err(Phase7Error::InvariantViolation(
                                "seller base proceeds non-finite or negative".into(),
                            ));
                        }
                        let base = seller_base_f64.floor() as Money;
                        s.seller_share_f32 = seller_share_f32;
                        s.seller_net_base = base;
                        s.seller_net = base;
                    }
                }

                // Signed seller-proceeds reconciliation
                let mut seller_base_total: Money = 0;
                for s in &planned_sellers {
                    seller_base_total = seller_base_total
                        .checked_add(s.seller_net_base)
                        .ok_or(Phase7Error::FinancialOverflow)?;
                }
                let initial_balance = net
                    .checked_sub(seller_base_total)
                    .ok_or(Phase7Error::FinancialOverflow)?;
                let mut balance = initial_balance;

                let participating_indices: Vec<usize> = planned_sellers
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.sold_units > 0.0)
                    .map(|(idx, _)| idx)
                    .collect();

                if !participating_indices.is_empty() {
                    if balance > 0 {
                        while balance > 0 {
                            for &idx in &participating_indices {
                                planned_sellers[idx].seller_net = planned_sellers[idx]
                                    .seller_net
                                    .checked_add(1)
                                    .ok_or(Phase7Error::FinancialOverflow)?;
                                balance = balance
                                    .checked_sub(1)
                                    .ok_or(Phase7Error::FinancialOverflow)?;
                                if balance == 0 {
                                    break;
                                }
                            }
                        }
                    } else if balance < 0 {
                        while balance < 0 {
                            let mut made_progress = false;
                            for &idx in &participating_indices {
                                if planned_sellers[idx].seller_net > 0 {
                                    planned_sellers[idx].seller_net = planned_sellers[idx]
                                        .seller_net
                                        .checked_sub(1)
                                        .ok_or(Phase7Error::FinancialOverflow)?;
                                    balance = balance
                                        .checked_add(1)
                                        .ok_or(Phase7Error::FinancialOverflow)?;
                                    made_progress = true;
                                }
                                if balance == 0 {
                                    break;
                                }
                            }
                            if balance < 0 && !made_progress {
                                return Err(Phase7Error::NegativeProceedsReconciliationStall {
                                    group_id: partition.group_id,
                                    remaining_balance: balance,
                                });
                            }
                        }
                    }
                }

                (total_sold, rev, tax, net, initial_balance)
            };

        // Validate financial conservation invariant
        let mut total_payout: Money = 0;
        for s in &planned_sellers {
            total_payout = total_payout
                .checked_add(s.seller_net)
                .ok_or(Phase7Error::FinancialOverflow)?;
        }
        if total_payout != net_pool_proceeds {
            return Err(Phase7Error::FinancialImbalance {
                total_debit: total_revenue,
                total_payout,
                tax_withheld,
            });
        }
        let total_credit = total_payout
            .checked_add(tax_withheld)
            .ok_or(Phase7Error::FinancialOverflow)?;
        if total_revenue != total_credit {
            return Err(Phase7Error::FinancialImbalance {
                total_debit: total_revenue,
                total_payout,
                tax_withheld,
            });
        }

        // Validate treasury credit overflow
        settlement
            .treasury
            .checked_add(tax_withheld)
            .ok_or(Phase7Error::FinancialOverflow)?;

        let buyer_updates: Vec<BuyerMarketUpdate> = planned_buyers
            .iter()
            .map(|b| BuyerMarketUpdate {
                agent_id: b.agent_id,
                bought: b.bought_units,
                debit: b.debit,
            })
            .collect();

        let seller_updates: Vec<SellerMarketUpdate> = planned_sellers
            .iter()
            .map(|s| SellerMarketUpdate {
                agent_id: s.agent_id,
                sold: s.sold_units,
                seller_net: s.seller_net,
            })
            .collect();

        planned_settlements.push(PlannedSettlementMarket {
            group_id: partition.group_id,
            food_price,
            tax_rate,
            total_effective_supply,
            total_effective_demand,
            total_sold,
            total_revenue,
            tax_withheld,
            net_pool_proceeds,
            proceeds_balance,
            buyers: planned_buyers,
            sellers: planned_sellers,
            buyer_updates,
            seller_updates,
        });
    }

    // Stage B: Atomic Execution across all settlements
    for plan in &planned_settlements {
        let cmd = Command::MarketClearance {
            group_id: plan.group_id,
            buyer_updates: plan.buyer_updates.clone(),
            seller_updates: plan.seller_updates.clone(),
            tax_withheld: plan.tax_withheld,
        };
        cmd.execute(world)?;
    }

    let resolutions = planned_settlements
        .into_iter()
        .map(|p| SettlementMarketResolution {
            group_id: p.group_id,
            food_price: p.food_price,
            tax_rate: p.tax_rate,
            total_effective_supply: p.total_effective_supply,
            total_effective_demand: p.total_effective_demand,
            total_sold: p.total_sold,
            total_revenue: p.total_revenue,
            tax_withheld: p.tax_withheld,
            net_pool_proceeds: p.net_pool_proceeds,
            proceeds_balance: p.proceeds_balance,
            buyers: p.buyers,
            sellers: p.sellers,
        })
        .collect();

    Ok(resolutions)
}

/// Convenience alias for [`phase7_market_clearance`].
pub use phase7_market_clearance as phase7_market_resolution;

/// Convenience wrapper executing Phase 7 market clearance using parameters from [`crate::config::EconomyConfig`].
pub fn phase7_market_clearance_with_config(
    world: &mut WorldState,
    partitions: &[SettlementIntentPartition],
    config: &crate::config::EconomyConfig,
) -> Result<Vec<SettlementMarketResolution>, Phase7Error> {
    phase7_market_clearance(world, partitions, config.food_price, config.tax_rate)
}

/// Canonical resolution record for an individual welfare recipient in Phase 8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WelfareRecipientResolution {
    pub agent_id: AgentId,
    pub payout: Money,
}

/// Authoritative summary of Phase 8 institutional welfare distribution for a single settlement locality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementWelfareResolution {
    pub group_id: GroupId,
    pub treasury_before: Money,
    pub treasury_after: Money,
    pub eligible_count: usize,
    pub payment_per_agent: Money,
    pub remainder: Money,
    pub total_distributed: Money,
    pub recipients: Vec<WelfareRecipientResolution>,
}

/// Explicit errors returned during Phase 8 Institutional Welfare Distribution.
#[derive(Debug, Clone, PartialEq)]
pub enum Phase8Error {
    InvalidStarvationThreshold(f32),
    NegativeWelfarePayment(Money),
    NegativeTreasury {
        group_id: GroupId,
        treasury: Money,
    },
    NegativeAgentWealth {
        agent_id: AgentId,
        wealth: Money,
    },
    InvalidAgentState {
        agent_id: AgentId,
        msg: String,
    },
    DuplicateSettlement(GroupId),
    DuplicateAgent(AgentId),
    MissingSettlement(GroupId),
    MissingAgent(AgentId),
    WealthOverflow(AgentId),
    FinancialOverflow,
    FinancialImbalance {
        treasury_before: Money,
        treasury_after: Money,
        total_distributed: Money,
    },
    CommandExecution(CommandExecutionError),
    InvariantViolation(String),
}

impl std::fmt::Display for Phase8Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStarvationThreshold(t) => {
                write!(f, "invalid starvation threshold: {}", t)
            }
            Self::NegativeWelfarePayment(w) => {
                write!(f, "negative welfare payment: {}", w)
            }
            Self::NegativeTreasury { group_id, treasury } => {
                write!(
                    f,
                    "settlement {} has negative treasury: {}",
                    group_id, treasury
                )
            }
            Self::NegativeAgentWealth { agent_id, wealth } => {
                write!(f, "agent {} has negative wealth: {}", agent_id, wealth)
            }
            Self::InvalidAgentState { agent_id, msg } => {
                write!(f, "invalid agent state for {}: {}", agent_id, msg)
            }
            Self::DuplicateSettlement(gid) => {
                write!(f, "duplicate settlement in world state: {}", gid)
            }
            Self::DuplicateAgent(aid) => {
                write!(f, "duplicate agent in world state: {}", aid)
            }
            Self::MissingSettlement(gid) => write!(f, "settlement missing: {}", gid),
            Self::MissingAgent(aid) => write!(f, "agent missing: {}", aid),
            Self::WealthOverflow(aid) => write!(f, "wealth overflow for agent {}", aid),
            Self::FinancialOverflow => write!(f, "financial arithmetic overflow in Phase 8"),
            Self::FinancialImbalance {
                treasury_before,
                treasury_after,
                total_distributed,
            } => write!(
                f,
                "financial imbalance in Phase 8: treasury_before {} != treasury_after {} + total_distributed {}",
                treasury_before, treasury_after, total_distributed
            ),
            Self::CommandExecution(err) => write!(f, "command execution failed: {}", err),
            Self::InvariantViolation(msg) => write!(f, "phase 8 invariant violation: {}", msg),
        }
    }
}

impl std::error::Error for Phase8Error {}

impl From<CommandExecutionError> for Phase8Error {
    fn from(err: CommandExecutionError) -> Self {
        Self::CommandExecution(err)
    }
}

struct PlannedSettlementWelfare {
    group_id: GroupId,
    treasury_before: Money,
    treasury_after: Money,
    eligible_count: usize,
    payment_per_agent: Money,
    remainder: Money,
    total_distributed: Money,
    recipients: Vec<WelfareRecipientResolution>,
    recipient_updates: Vec<WelfareRecipientUpdate>,
}

/// Resolves Phase 8 Institutional Welfare Distribution for all settlements in the world.
///
/// Execution order:
/// 1. Validate parameters: `starvation_threshold.is_finite() && starvation_threshold > 0.0`,
///    `welfare_payment >= 0`.
/// 2. Canonical settlement ordering: ascending `GroupId`.
/// 3. For each settlement independently:
///    - Validate treasury is non-negative.
///    - Collect all living, behaviorally eligible agents belonging to this settlement
///      whose live food is strictly below `starvation_threshold`:
///      `agent.group_id == group_id && agent.alive && agent.health > 0.0 && agent.food < starvation_threshold`.
///    - Validate agent states (finite non-negative health/food, non-negative wealth).
///    - Order eligible recipients strictly by ascending stable `AgentId`.
///    - If `eligible_count == 0` or `welfare_payment == 0`:
///      - Zero treasury debit, zero payouts, zero wealth mutations.
///    - Else:
///      - Calculate `required = eligible_count * welfare_payment` with checked multiplication.
///      - If `treasury >= required` (fully funded):
///        - `payment_per_agent = welfare_payment`, `remainder = 0`.
///        - `total_distributed = required`.
///        - `treasury_after = treasury - total_distributed`.
///        - Each recipient gets `welfare_payment`.
///      - Else (`treasury < required`, underfunded):
///        - `payment_per_agent = treasury / eligible_count` (integer division).
///        - `remainder = treasury % eligible_count` (integer modulo).
///        - `total_distributed = treasury`.
///        - `treasury_after = 0`.
///        - First `remainder` recipients in ascending `AgentId` get `payment_per_agent + 1`.
///        - Remaining recipients get `payment_per_agent`.
///    - Verify financial invariants: `treasury_before == treasury_after + total_distributed`
///      and `sum(payouts) == total_distributed`.
///    - Verify no recipient wealth overflows (`agent.wealth.checked_add(payout)`).
/// 4. Atomic commit across all settlements: execute `Command::WelfareDistribution` only if all
///    settlements validate Stage A.
pub fn phase8_welfare_distribution(
    world: &mut WorldState,
    starvation_threshold: f32,
    welfare_payment: Money,
) -> Result<Vec<SettlementWelfareResolution>, Phase8Error> {
    if !starvation_threshold.is_finite() || starvation_threshold <= 0.0 {
        return Err(Phase8Error::InvalidStarvationThreshold(
            starvation_threshold,
        ));
    }
    if welfare_payment < 0 {
        return Err(Phase8Error::NegativeWelfarePayment(welfare_payment));
    }

    // Canonical settlement ordering: GroupId ascending
    let mut group_ids: Vec<GroupId> = world.settlements.iter().map(|s| s.group_id).collect();
    group_ids.sort();

    for w in group_ids.windows(2) {
        if w[0] == w[1] {
            return Err(Phase8Error::DuplicateSettlement(w[0]));
        }
    }

    let mut seen_agents = HashSet::with_capacity(world.agents.len());
    for agent in &world.agents {
        if !seen_agents.insert(agent.agent_id) {
            return Err(Phase8Error::DuplicateAgent(agent.agent_id));
        }
        if !agent.health.is_finite() {
            return Err(Phase8Error::InvalidAgentState {
                agent_id: agent.agent_id,
                msg: format!("non-finite health: {}", agent.health),
            });
        }
        if !agent.food.is_finite() || agent.food < 0.0 {
            return Err(Phase8Error::InvalidAgentState {
                agent_id: agent.agent_id,
                msg: format!("invalid food: {}", agent.food),
            });
        }
        if agent.wealth < 0 {
            return Err(Phase8Error::NegativeAgentWealth {
                agent_id: agent.agent_id,
                wealth: agent.wealth,
            });
        }
    }

    let mut planned_settlements = Vec::with_capacity(group_ids.len());

    // Stage A: Planning and validation across all settlements
    for gid in group_ids {
        let settlement = world
            .settlements
            .iter()
            .find(|s| s.group_id == gid)
            .ok_or(Phase8Error::MissingSettlement(gid))?;

        if settlement.treasury < 0 {
            return Err(Phase8Error::NegativeTreasury {
                group_id: gid,
                treasury: settlement.treasury,
            });
        }

        let treasury_before = settlement.treasury;

        // Collect eligible agents for this settlement:
        // living, health > 0, food < starvation_threshold, group_id == settlement.group_id
        let mut eligible_agents: Vec<&AgentState> = world
            .agents
            .iter()
            .filter(|a| {
                a.group_id == gid && a.alive && a.health > 0.0 && a.food < starvation_threshold
            })
            .collect();

        // Canonical ordering: strictly ascending AgentId
        eligible_agents.sort_by_key(|a| a.agent_id);

        let eligible_count = eligible_agents.len();

        let (
            payment_per_agent,
            remainder,
            total_distributed,
            treasury_after,
            recipients,
            recipient_updates,
        ) = if eligible_count == 0 || welfare_payment == 0 {
            let mut recs = Vec::with_capacity(eligible_count);
            let mut updates = Vec::with_capacity(eligible_count);
            if welfare_payment == 0 && eligible_count > 0 {
                for a in &eligible_agents {
                    recs.push(WelfareRecipientResolution {
                        agent_id: a.agent_id,
                        payout: 0,
                    });
                    updates.push(WelfareRecipientUpdate {
                        agent_id: a.agent_id,
                        payout: 0,
                    });
                }
            }
            (0, 0, 0, treasury_before, recs, updates)
        } else {
            let required = (eligible_count as Money)
                .checked_mul(welfare_payment)
                .ok_or(Phase8Error::FinancialOverflow)?;

            if treasury_before >= required {
                // Fully funded
                let payment_per_agent = welfare_payment;
                let remainder = 0;
                let total_distributed = required;
                let treasury_after = treasury_before
                    .checked_sub(total_distributed)
                    .ok_or(Phase8Error::FinancialOverflow)?;

                let mut recs = Vec::with_capacity(eligible_count);
                let mut updates = Vec::with_capacity(eligible_count);

                for a in &eligible_agents {
                    a.wealth
                        .checked_add(welfare_payment)
                        .ok_or(Phase8Error::WealthOverflow(a.agent_id))?;

                    recs.push(WelfareRecipientResolution {
                        agent_id: a.agent_id,
                        payout: welfare_payment,
                    });
                    updates.push(WelfareRecipientUpdate {
                        agent_id: a.agent_id,
                        payout: welfare_payment,
                    });
                }

                (
                    payment_per_agent,
                    remainder,
                    total_distributed,
                    treasury_after,
                    recs,
                    updates,
                )
            } else {
                // Underfunded
                let count_money = eligible_count as Money;
                let payment_per_agent = treasury_before / count_money;
                let remainder = treasury_before % count_money;
                let total_distributed = treasury_before;
                let treasury_after = 0;

                let mut recs = Vec::with_capacity(eligible_count);
                let mut updates = Vec::with_capacity(eligible_count);

                for (idx, a) in eligible_agents.iter().enumerate() {
                    let extra = if (idx as Money) < remainder { 1 } else { 0 };
                    let payout = payment_per_agent
                        .checked_add(extra)
                        .ok_or(Phase8Error::FinancialOverflow)?;

                    a.wealth
                        .checked_add(payout)
                        .ok_or(Phase8Error::WealthOverflow(a.agent_id))?;

                    recs.push(WelfareRecipientResolution {
                        agent_id: a.agent_id,
                        payout,
                    });
                    updates.push(WelfareRecipientUpdate {
                        agent_id: a.agent_id,
                        payout,
                    });
                }

                (
                    payment_per_agent,
                    remainder,
                    total_distributed,
                    treasury_after,
                    recs,
                    updates,
                )
            }
        };

        // Invariant check: conservation of treasury
        if treasury_before != treasury_after + total_distributed {
            return Err(Phase8Error::FinancialImbalance {
                treasury_before,
                treasury_after,
                total_distributed,
            });
        }

        // Invariant check: sum of payouts == total_distributed
        let mut sum_payouts: Money = 0;
        for r in &recipients {
            sum_payouts = sum_payouts
                .checked_add(r.payout)
                .ok_or(Phase8Error::FinancialOverflow)?;
        }
        if sum_payouts != total_distributed {
            return Err(Phase8Error::FinancialImbalance {
                treasury_before,
                treasury_after,
                total_distributed,
            });
        }

        planned_settlements.push(PlannedSettlementWelfare {
            group_id: gid,
            treasury_before,
            treasury_after,
            eligible_count,
            payment_per_agent,
            remainder,
            total_distributed,
            recipients,
            recipient_updates,
        });
    }

    // Stage B: Atomic commit across all settlements
    let mut resolutions = Vec::with_capacity(planned_settlements.len());
    for plan in planned_settlements {
        if plan.total_distributed > 0 || !plan.recipient_updates.is_empty() {
            let cmd = Command::WelfareDistribution {
                group_id: plan.group_id,
                recipient_updates: plan.recipient_updates,
                treasury_debit: plan.total_distributed,
            };
            cmd.execute(world)?;
        }

        resolutions.push(SettlementWelfareResolution {
            group_id: plan.group_id,
            treasury_before: plan.treasury_before,
            treasury_after: plan.treasury_after,
            eligible_count: plan.eligible_count,
            payment_per_agent: plan.payment_per_agent,
            remainder: plan.remainder,
            total_distributed: plan.total_distributed,
            recipients: plan.recipients,
        });
    }

    Ok(resolutions)
}

/// Convenience alias for [`phase8_welfare_distribution`].
pub use phase8_welfare_distribution as phase8_welfare_resolution;

/// Convenience wrapper executing Phase 8 institutional welfare distribution using parameters from [`crate::config::SimConfig`].
pub fn phase8_welfare_distribution_with_config(
    world: &mut WorldState,
    config: &crate::config::SimConfig,
) -> Result<Vec<SettlementWelfareResolution>, Phase8Error> {
    phase8_welfare_distribution(
        world,
        config.interaction.starvation_threshold,
        config.economy.welfare_payment,
    )
}

/// Convenience wrapper executing Phase 8 institutional welfare distribution using sub-configurations.
pub fn phase8_welfare_distribution_with_subconfigs(
    world: &mut WorldState,
    interaction: &crate::config::InteractionConfig,
    economy: &crate::config::EconomyConfig,
) -> Result<Vec<SettlementWelfareResolution>, Phase8Error> {
    phase8_welfare_distribution(
        world,
        interaction.starvation_threshold,
        economy.welfare_payment,
    )
}

/// Executes Phase 8: Institutional Welfare Distribution natively on authoritative [`SegmentedAgentStorage`].
///
/// Directly filters eligible agents using contiguous `economy.group_id`, `demography.alive`,
/// `demography.health`, and `economy.food` columns, canonicalizes recipient ordering by ascending
/// `AgentId`, applies integer division and remainder distribution, and updates `settlements` and
/// `economy.wealth` in-place without heap allocations or command lookup overhead.
pub fn phase8_welfare_distribution_storage(
    storage: &mut SegmentedAgentStorage,
    settlements: &mut [SettlementState],
    starvation_threshold: f32,
    welfare_payment: Money,
) -> Result<Vec<SettlementWelfareResolution>, Phase8Error> {
    if !starvation_threshold.is_finite() || starvation_threshold <= 0.0 {
        return Err(Phase8Error::InvalidStarvationThreshold(
            starvation_threshold,
        ));
    }
    if welfare_payment < 0 {
        return Err(Phase8Error::NegativeWelfarePayment(welfare_payment));
    }

    // Canonical settlement ordering: GroupId ascending
    let mut group_ids: Vec<GroupId> = settlements.iter().map(|s| s.group_id).collect();
    group_ids.sort();

    for w in group_ids.windows(2) {
        if w[0] == w[1] {
            return Err(Phase8Error::DuplicateSettlement(w[0]));
        }
    }

    let n = storage.len();
    let mut seen_agents = HashSet::with_capacity(n);
    for i in 0..n {
        let aid = storage.agent_ids[i];
        if !seen_agents.insert(aid) {
            return Err(Phase8Error::DuplicateAgent(aid));
        }
        let h = storage.demography.health[i];
        if !h.is_finite() {
            return Err(Phase8Error::InvalidAgentState {
                agent_id: aid,
                msg: format!("non-finite health: {}", h),
            });
        }
        let f = storage.economy.food[i];
        if !f.is_finite() || f < 0.0 {
            return Err(Phase8Error::InvalidAgentState {
                agent_id: aid,
                msg: format!("invalid food: {}", f),
            });
        }
        let w = storage.economy.wealth[i];
        if w < 0 {
            return Err(Phase8Error::NegativeAgentWealth {
                agent_id: aid,
                wealth: w,
            });
        }
    }

    struct PlannedSoAWelfare {
        group_id: GroupId,
        treasury_before: Money,
        treasury_after: Money,
        eligible_count: usize,
        payment_per_agent: Money,
        remainder: Money,
        total_distributed: Money,
        recipients: Vec<WelfareRecipientResolution>,
        slot_payouts: Vec<(usize, Money)>,
    }

    let mut planned_settlements = Vec::with_capacity(group_ids.len());

    // Stage A: Planning and validation across all settlements
    for gid in group_ids {
        let settlement = settlements
            .iter()
            .find(|s| s.group_id == gid)
            .ok_or(Phase8Error::MissingSettlement(gid))?;

        if settlement.treasury < 0 {
            return Err(Phase8Error::NegativeTreasury {
                group_id: gid,
                treasury: settlement.treasury,
            });
        }

        let treasury_before = settlement.treasury;

        // Collect eligible agents for this settlement:
        // living, health > 0, food < starvation_threshold, group_id == settlement.group_id
        let mut eligible: Vec<(AgentId, usize)> = Vec::new();
        for i in 0..n {
            if storage.economy.group_id[i] == gid
                && storage.demography.alive[i]
                && storage.demography.health[i] > 0.0
                && storage.economy.food[i] < starvation_threshold
            {
                eligible.push((storage.agent_ids[i], i));
            }
        }

        // Canonical ordering: strictly ascending AgentId
        eligible.sort_by_key(|&(aid, _slot)| aid);

        let eligible_count = eligible.len();

        let (
            payment_per_agent,
            remainder,
            total_distributed,
            treasury_after,
            recipients,
            slot_payouts,
        ) = if eligible_count == 0 || welfare_payment == 0 {
            let mut recs = Vec::with_capacity(eligible_count);
            let mut payouts = Vec::with_capacity(eligible_count);
            if welfare_payment == 0 && eligible_count > 0 {
                for &(aid, slot) in &eligible {
                    recs.push(WelfareRecipientResolution {
                        agent_id: aid,
                        payout: 0,
                    });
                    payouts.push((slot, 0));
                }
            }
            (0, 0, 0, treasury_before, recs, payouts)
        } else {
            let required = (eligible_count as Money)
                .checked_mul(welfare_payment)
                .ok_or(Phase8Error::FinancialOverflow)?;

            if treasury_before >= required {
                // Fully funded
                let payment_per_agent = welfare_payment;
                let remainder = 0;
                let total_distributed = required;
                let treasury_after = treasury_before
                    .checked_sub(total_distributed)
                    .ok_or(Phase8Error::FinancialOverflow)?;

                let mut recs = Vec::with_capacity(eligible_count);
                let mut payouts = Vec::with_capacity(eligible_count);

                for &(aid, slot) in &eligible {
                    storage.economy.wealth[slot]
                        .checked_add(welfare_payment)
                        .ok_or(Phase8Error::WealthOverflow(aid))?;

                    recs.push(WelfareRecipientResolution {
                        agent_id: aid,
                        payout: welfare_payment,
                    });
                    payouts.push((slot, welfare_payment));
                }

                (
                    payment_per_agent,
                    remainder,
                    total_distributed,
                    treasury_after,
                    recs,
                    payouts,
                )
            } else {
                // Underfunded
                let count_money = eligible_count as Money;
                let payment_per_agent = treasury_before / count_money;
                let remainder = treasury_before % count_money;
                let total_distributed = treasury_before;
                let treasury_after = 0;

                let mut recs = Vec::with_capacity(eligible_count);
                let mut payouts = Vec::with_capacity(eligible_count);

                for (idx, &(aid, slot)) in eligible.iter().enumerate() {
                    let extra = if (idx as Money) < remainder { 1 } else { 0 };
                    let payout = payment_per_agent
                        .checked_add(extra)
                        .ok_or(Phase8Error::FinancialOverflow)?;

                    storage.economy.wealth[slot]
                        .checked_add(payout)
                        .ok_or(Phase8Error::WealthOverflow(aid))?;

                    recs.push(WelfareRecipientResolution {
                        agent_id: aid,
                        payout,
                    });
                    payouts.push((slot, payout));
                }

                (
                    payment_per_agent,
                    remainder,
                    total_distributed,
                    treasury_after,
                    recs,
                    payouts,
                )
            }
        };

        // Invariant check: conservation of treasury
        if treasury_before != treasury_after + total_distributed {
            return Err(Phase8Error::FinancialImbalance {
                treasury_before,
                treasury_after,
                total_distributed,
            });
        }

        // Invariant check: sum of payouts == total_distributed
        let mut sum_payouts: Money = 0;
        for r in &recipients {
            sum_payouts = sum_payouts
                .checked_add(r.payout)
                .ok_or(Phase8Error::FinancialOverflow)?;
        }
        if sum_payouts != total_distributed {
            return Err(Phase8Error::FinancialImbalance {
                treasury_before,
                treasury_after,
                total_distributed,
            });
        }

        planned_settlements.push(PlannedSoAWelfare {
            group_id: gid,
            treasury_before,
            treasury_after,
            eligible_count,
            payment_per_agent,
            remainder,
            total_distributed,
            recipients,
            slot_payouts,
        });
    }

    // Stage B: Atomic commit across all settlements
    let mut resolutions = Vec::with_capacity(planned_settlements.len());
    for plan in planned_settlements {
        if plan.total_distributed > 0 || !plan.slot_payouts.is_empty() {
            let settlement = settlements
                .iter_mut()
                .find(|s| s.group_id == plan.group_id)
                .expect("settlement existence checked during planning");
            settlement.treasury -= plan.total_distributed;

            for (slot, payout) in plan.slot_payouts {
                storage.economy.wealth[slot] += payout;
            }
        }

        resolutions.push(SettlementWelfareResolution {
            group_id: plan.group_id,
            treasury_before: plan.treasury_before,
            treasury_after: plan.treasury_after,
            eligible_count: plan.eligible_count,
            payment_per_agent: plan.payment_per_agent,
            remainder: plan.remainder,
            total_distributed: plan.total_distributed,
            recipients: plan.recipients,
        });
    }

    Ok(resolutions)
}

/// Convenience wrapper executing Phase 8 institutional welfare distribution natively using parameters from [`crate::config::SimConfig`].
pub fn phase8_welfare_distribution_storage_with_config(
    storage: &mut SegmentedAgentStorage,
    settlements: &mut [SettlementState],
    config: &crate::config::SimConfig,
) -> Result<Vec<SettlementWelfareResolution>, Phase8Error> {
    phase8_welfare_distribution_storage(
        storage,
        settlements,
        config.interaction.starvation_threshold,
        config.economy.welfare_payment,
    )
}

pub use crate::phases::{
    MortalityResolution, Phase9Error, Phase9MortalityResolution, phase9_mortality_commitment,
    phase9_mortality_commitment_with_config, phase9_mortality_resolution,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initialize_world;

    const TEST_CONFIG_TOML: &str = r#"
[world]
master_seed = 81985529216486895
replicate_id = 7
initial_population = 10
settlement_count = 2
initial_health = 0.8
initial_food = 5.0
initial_wealth = 1000
initial_settlement_resource = 500.0
initial_treasury = 1000

[traits]
prod_min = 0.8
prod_max = 1.2
coop_min = 0.3
coop_max = 0.7
aggr_min = 0.1
aggr_max = 0.5
risk_min = 0.2
risk_max = 0.6

[environment]
carrying_capacity = 1000.0
regrowth_rate = 0.1
base_metabolic_cost = 2.0
health_decay_rate = 0.05

[economy]
base_work_yield = 2.0
food_price = 100
target_food = 10.0
target_reserve = 1000
tax_rate = 0.1
welfare_payment = 50

[interaction]
gift_amount = 2.0
theft_amount = 3.0
theft_success_probability = 0.5
starvation_threshold = 5.0

[decision]
decision_temperature = 1.0
action_biases = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
base_weight_matrix = [
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0],
  [0.0, 0.0, 0.0, 0.0, 0.0]
]
trait_weight_cooperation = 1.0
trait_weight_aggression = 1.0
trait_weight_risk_tolerance = 1.0
"#;

    #[test]
    fn test_phase8_storage_parity() {
        let config = SimConfig::parse_and_validate(TEST_CONFIG_TOML).unwrap();
        let mut world = initialize_world(&config).unwrap();

        // Mutate agent states: some eligible, some not
        world.agents[0].alive = false; // dead -> ineligible
        world.agents[1].health = 0.0; // health 0 -> ineligible
        world.agents[2].food = 2.0; // food < 5.0 -> eligible
        world.agents[3].food = 6.0; // food >= 5.0 -> ineligible
        world.agents[4].food = 1.0; // food < 5.0 -> eligible
        world.settlements[0].treasury = 75; // underfunded: 2 eligible, required 100, gives 37 + 38

        let mut world_aos = world.clone();
        let mut storage = SegmentedAgentStorage::from_agents(&world.agents);
        let mut settlements_soa = world.settlements.clone();

        let res_aos = phase8_welfare_distribution_with_config(&mut world_aos, &config).unwrap();
        let res_soa = phase8_welfare_distribution_storage_with_config(
            &mut storage,
            &mut settlements_soa,
            &config,
        )
        .unwrap();

        assert_eq!(res_aos, res_soa);
        assert_eq!(world_aos.settlements, settlements_soa);

        for (i, agent) in world_aos.agents.iter().enumerate() {
            assert_eq!(
                agent.wealth,
                storage.wealth()[i],
                "agent {} wealth mismatch",
                i
            );
        }

        let reconstructed = storage.to_agents();
        assert_eq!(world_aos.agents, reconstructed);
    }
}
