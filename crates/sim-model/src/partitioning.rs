use crate::intents::Intent;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId};
use std::collections::HashSet;

/// Grouped collection of immutable intents belonging to a single settlement locality.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettlementIntentPartition {
    pub group_id: GroupId,
    pub intents: Vec<Intent>,
}

/// Explicit error returned during Phase 5 settlement locality partitioning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase5Error {
    /// Multiple intents were supplied with the same initiator AgentId.
    DuplicateInitiator(AgentId),
}

impl std::fmt::Display for Phase5Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateInitiator(id) => {
                write!(f, "duplicate initiator agent {} in Phase 5 input", id)
            }
        }
    }
}

impl std::error::Error for Phase5Error {}

/// Groups already-addressed Phase 4 intents into independent settlement buckets by GroupId.
///
/// Invariants:
/// 1. Partition key is strictly `group_id: GroupId`.
/// 2. All six Intent variants participate.
/// 3. Targets (including `target_agent_id = None`) do not alter the partition key.
/// 4. Output partitions are sorted in strictly ascending `GroupId` order.
/// 5. Within each partition, intents are sorted in strictly ascending initiator `AgentId` order.
/// 6. Each input intent is preserved bit-identically and placed into exactly one partition.
/// 7. Sum of partition intent counts equals input intent count.
/// 8. Empty input produces empty output partition list.
/// 9. Rejects duplicate initiator `AgentId`s explicitly.
/// 10. Consumes zero PRNG draws and performs zero state mutations.
pub fn phase5_partition_intents(
    intents: &[Intent],
) -> Result<Vec<SettlementIntentPartition>, Phase5Error> {
    if intents.is_empty() {
        return Ok(Vec::new());
    }

    // 1. Detect duplicate initiators in exact input order
    let mut seen_initiators = HashSet::with_capacity(intents.len());
    for intent in intents {
        let initiator = intent.agent_id();
        if !seen_initiators.insert(initiator) {
            return Err(Phase5Error::DuplicateInitiator(initiator));
        }
    }

    // 2. Collect and sort contiguous intents once by (GroupId ascending, AgentId ascending)
    let mut sorted_intents = intents.to_vec();
    sorted_intents.sort_by(|a, b| {
        a.group_id()
            .cmp(&b.group_id())
            .then_with(|| a.agent_id().cmp(&b.agent_id()))
    });

    // 3. Form partitions contiguously without BTreeMap or per-bucket sorting
    let mut partitions = Vec::new();
    let mut current_group: Option<GroupId> = None;
    let mut current_intents: Vec<Intent> = Vec::new();

    for intent in sorted_intents {
        let gid = intent.group_id();
        match current_group {
            Some(curr) if curr == gid => {
                current_intents.push(intent);
            }
            Some(curr) => {
                partitions.push(SettlementIntentPartition {
                    group_id: curr,
                    intents: std::mem::take(&mut current_intents),
                });
                current_group = Some(gid);
                current_intents.push(intent);
            }
            None => {
                current_group = Some(gid);
                current_intents.push(intent);
            }
        }
    }

    if let Some(curr) = current_group {
        partitions.push(SettlementIntentPartition {
            group_id: curr,
            intents: current_intents,
        });
    }

    Ok(partitions)
}
