use crate::intents::Intent;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId};
use std::collections::{HashMap, HashSet};

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

/// Transient lookup scratch for ordered Phase5 bucketing. It is reusable across ticks but is not
/// part of simulation state, snapshots, hashes, or events.
#[derive(Debug, Default)]
pub struct Phase5PartitionScratch {
    groups: HashMap<GroupId, Vec<Intent>>,
}

impl Phase5PartitionScratch {
    pub fn with_capacity(group_count: usize) -> Self {
        Self {
            groups: HashMap::with_capacity(group_count),
        }
    }
}

/// Reference Phase 5 path: reject duplicate initiators in input order, then sort canonically.
///
/// This remains available for fallback, differential tests, and benchmarks.
pub fn phase5_partition_intents_baseline(
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

/// Groups Phase 4 intents into independent settlement buckets by GroupId.
///
/// Ordered AgentId input uses stable GroupId bucketing; arbitrary-order input uses the reference
/// canonical sort path. The contract is:
/// - partition key is strictly `group_id: GroupId`;
/// - all Intent variants and payloads are preserved exactly;
/// - partitions are ordered by ascending GroupId;
/// - intents within a partition are ordered by ascending initiator AgentId;
/// - empty input returns no partitions;
/// - duplicate initiators return `DuplicateInitiator` in original input order;
/// - the function consumes no PRNG draws and mutates no simulation state.
pub fn phase5_partition_intents(
    intents: &[Intent],
) -> Result<Vec<SettlementIntentPartition>, Phase5Error> {
    match strictly_increasing_agent_ids(intents)? {
        true => {
            let mut scratch = Phase5PartitionScratch::default();
            Ok(bucket_agent_id_ordered(
                intents.iter().cloned(),
                &mut scratch.groups,
            ))
        }
        false => phase5_partition_intents_baseline(intents),
    }
}

/// Phase5 path for caller-owned intent scratch. Ordered input is moved into the returned owned
/// partitions, leaving the input Vec empty while retaining its capacity. Unordered input follows
/// the canonical reference path and remains unchanged.
pub fn phase5_partition_intents_from_vec(
    intents: &mut Vec<Intent>,
) -> Result<Vec<SettlementIntentPartition>, Phase5Error> {
    phase5_partition_intents_from_vec_with_scratch(intents, &mut Phase5PartitionScratch::default())
}

/// Reuses caller-owned Phase5 lookup capacity while moving ordered intents into owned partitions.
pub fn phase5_partition_intents_from_vec_with_scratch(
    intents: &mut Vec<Intent>,
    scratch: &mut Phase5PartitionScratch,
) -> Result<Vec<SettlementIntentPartition>, Phase5Error> {
    match strictly_increasing_agent_ids(intents)? {
        true => Ok(bucket_agent_id_ordered(
            intents.drain(..),
            &mut scratch.groups,
        )),
        false => phase5_partition_intents_baseline(intents),
    }
}

/// Returns true for strictly increasing AgentIds, false at the first descending pair, and keeps
/// the reference duplicate error for an equal adjacent pair in an ordered prefix.
fn strictly_increasing_agent_ids(intents: &[Intent]) -> Result<bool, Phase5Error> {
    for pair in intents.windows(2) {
        match pair[0].agent_id().cmp(&pair[1].agent_id()) {
            std::cmp::Ordering::Less => {}
            std::cmp::Ordering::Equal => {
                return Err(Phase5Error::DuplicateInitiator(pair[1].agent_id()));
            }
            std::cmp::Ordering::Greater => return Ok(false),
        }
    }
    Ok(true)
}

/// Stable GroupId bucketing for a strictly increasing AgentId stream. HashMap iteration order is
/// discarded before output: the returned partitions are explicitly sorted by GroupId.
fn bucket_agent_id_ordered(
    intents: impl IntoIterator<Item = Intent>,
    groups: &mut HashMap<GroupId, Vec<Intent>>,
) -> Vec<SettlementIntentPartition> {
    groups.clear();
    for intent in intents {
        groups.entry(intent.group_id()).or_default().push(intent);
    }

    let mut partitions = Vec::with_capacity(groups.len());
    partitions.extend(
        groups
            .drain()
            .map(|(group_id, intents)| SettlementIntentPartition { group_id, intents }),
    );
    partitions.sort_unstable_by_key(|partition| partition.group_id);
    partitions
}
