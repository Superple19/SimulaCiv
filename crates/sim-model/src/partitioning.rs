use crate::intents::Intent;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId};
use std::collections::{BTreeMap, HashSet};

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

    let mut seen_initiators = HashSet::with_capacity(intents.len());
    let mut buckets: BTreeMap<GroupId, Vec<Intent>> = BTreeMap::new();

    for intent in intents {
        let initiator = intent.agent_id();
        if !seen_initiators.insert(initiator) {
            return Err(Phase5Error::DuplicateInitiator(initiator));
        }

        let group_id = intent.group_id();
        buckets.entry(group_id).or_default().push(intent.clone());
    }

    let mut partitions = Vec::with_capacity(buckets.len());
    for (group_id, mut partition_intents) in buckets {
        partition_intents.sort_by_key(|i| i.agent_id());
        partitions.push(SettlementIntentPartition {
            group_id,
            intents: partition_intents,
        });
    }

    Ok(partitions)
}
