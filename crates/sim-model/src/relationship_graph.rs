use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};
use sim_core::{AgentId, GroupId, RngCoordinate, coordinate_prng_f32};

/// M3-only subsystem ID; the frozen M2 `Subsystem` mapping remains unchanged.
pub const M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID: u16 = 6;
const MAX_EXACT_F32_POPULATION: usize = 1 << 24;

/// Canonically ordered directed relationship graph in CSR form.
#[derive(Debug)]
pub struct RelationshipGraph {
    source_ids: Vec<AgentId>,
    row_offsets: Vec<usize>,
    neighbors: Vec<AgentId>,
    cross_partition_edges: AtomicU64,
    total_edges: u64,
    // An empty cell is the dirty marker; hash only when a digest or snapshot requests it.
    csr_hash: OnceLock<[u8; 32]>,
}

impl PartialEq for RelationshipGraph {
    fn eq(&self, other: &Self) -> bool {
        self.source_ids == other.source_ids
            && self.row_offsets == other.row_offsets
            && self.neighbors == other.neighbors
            && self.cross_partition_edges.load(Ordering::Relaxed)
                == other.cross_partition_edges.load(Ordering::Relaxed)
            && self.total_edges == other.total_edges
    }
}

impl Eq for RelationshipGraph {}

impl Clone for RelationshipGraph {
    fn clone(&self) -> Self {
        let csr_hash = OnceLock::new();
        if let Some(hash) = self.csr_hash.get() {
            let _ = csr_hash.set(*hash);
        }
        Self {
            source_ids: self.source_ids.clone(),
            row_offsets: self.row_offsets.clone(),
            neighbors: self.neighbors.clone(),
            cross_partition_edges: AtomicU64::new(
                self.cross_partition_edges.load(Ordering::Relaxed),
            ),
            total_edges: self.total_edges,
            csr_hash,
        }
    }
}

#[derive(Debug)]
pub(crate) struct RelationshipGraphAppendPlan {
    row_count: usize,
    additional_neighbors: usize,
    cross_partition_edges: u64,
    total_edges: u64,
}

/// Input, capacity, or representation error while building the M3 graph.
#[derive(Debug, Clone, PartialEq)]
pub enum RelationshipGraphError {
    InvalidDegree(u32),
    InvalidLocalityProbability(f32),
    PopulationTooLarge(usize),
    DuplicateAgentId(AgentId),
    InsufficientLocalCapacity {
        group_id: GroupId,
        available: usize,
        required: usize,
    },
    InsufficientCrossPartitionCapacity {
        group_id: GroupId,
        available: usize,
        required: usize,
    },
    EdgeCountOverflow,
    RankInvariantViolation,
}

impl std::fmt::Display for RelationshipGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDegree(k) => write!(f, "relationship degree must be in [0, 16], got {k}"),
            Self::InvalidLocalityProbability(p) => {
                write!(f, "p_local must be finite and in [0.0, 1.0], got {p}")
            }
            Self::PopulationTooLarge(n) => {
                write!(
                    f,
                    "relationship graph population must be less than 2^24, got {n}"
                )
            }
            Self::DuplicateAgentId(agent_id) => {
                write!(
                    f,
                    "duplicate AgentId {agent_id} in relationship graph input"
                )
            }
            Self::InsufficientLocalCapacity {
                group_id,
                available,
                required,
            } => write!(
                f,
                "group {group_id} has {available} other local agents, but degree {required} is required"
            ),
            Self::InsufficientCrossPartitionCapacity {
                group_id,
                available,
                required,
            } => write!(
                f,
                "group {group_id} has {available} cross-partition agents, but degree {required} is required"
            ),
            Self::EdgeCountOverflow => write!(f, "relationship graph edge count overflow"),
            Self::RankInvariantViolation => {
                write!(f, "relationship graph rank selection invariant failed")
            }
        }
    }
}

impl std::error::Error for RelationshipGraphError {}

impl RelationshipGraph {
    /// Generates the normative pool-and-remove graph through the required rank-index algorithm.
    pub fn generate(
        memberships: &[(AgentId, GroupId)],
        master_seed: u64,
        replicate_id: u32,
        degree_k: u32,
        p_local: f32,
    ) -> Result<Self, RelationshipGraphError> {
        if degree_k > 16 {
            return Err(RelationshipGraphError::InvalidDegree(degree_k));
        }
        if !p_local.is_finite() || !(0.0..=1.0).contains(&p_local) {
            return Err(RelationshipGraphError::InvalidLocalityProbability(p_local));
        }
        validate_population_count(memberships.len())?;

        let mut agents = memberships.to_vec();
        if agents.windows(2).any(|pair| pair[0].0 > pair[1].0) {
            agents.sort_unstable_by_key(|&(agent_id, _)| agent_id);
        }
        for pair in agents.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(RelationshipGraphError::DuplicateAgentId(pair[0].0));
            }
        }

        let mut groups = BTreeMap::<GroupId, Vec<usize>>::new();
        for (rank, &(_, group_id)) in agents.iter().enumerate() {
            groups.entry(group_id).or_default().push(rank);
        }
        let mut group_ids = Vec::with_capacity(groups.len());
        let mut group_ranks = Vec::with_capacity(groups.len());
        let mut group_index_by_rank = vec![0; agents.len()];
        let mut group_rank_by_global_rank = vec![0; agents.len()];
        for (group_index, (group_id, ranks)) in groups.into_iter().enumerate() {
            for (group_rank, &global_rank) in ranks.iter().enumerate() {
                group_index_by_rank[global_rank] = group_index;
                group_rank_by_global_rank[global_rank] = group_rank;
            }
            group_ids.push(group_id);
            group_ranks.push(ranks);
        }

        let degree_k = degree_k as usize;
        validate_capacities(&group_ids, &group_ranks, agents.len(), degree_k, p_local)?;

        let mut source_ids = Vec::with_capacity(agents.len());
        let mut row_offsets = Vec::with_capacity(agents.len() + 1);
        let mut neighbors = Vec::with_capacity(agents.len().saturating_mul(degree_k));
        let mut cross_partition_edges = 0u64;
        row_offsets.push(0);

        for (source_rank, &(source_id, source_group_id)) in agents.iter().enumerate() {
            source_ids.push(source_id);
            let source_group_index = group_index_by_rank[source_rank];
            let source_group_ranks = &group_ranks[source_group_index];
            let mut local_excluded = vec![group_rank_by_global_rank[source_rank]];
            let mut cross_excluded = Vec::with_capacity(degree_k);
            let mut row = Vec::with_capacity(degree_k);

            for edge_index in 0..degree_k {
                let locality = RngCoordinate::new(
                    master_seed,
                    replicate_id,
                    0,
                    0,
                    M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID,
                    source_id.0,
                    (edge_index * 2) as u32,
                );
                let is_local = coordinate_prng_f32(&locality) < p_local;
                let excluded = if is_local {
                    &mut local_excluded
                } else {
                    &mut cross_excluded
                };
                let base_pool_size = if is_local {
                    source_group_ranks.len()
                } else {
                    agents.len() - source_group_ranks.len()
                };
                let remaining = base_pool_size - excluded.len();
                if remaining == 0 {
                    return Err(RelationshipGraphError::RankInvariantViolation);
                }

                let target_coordinate = RngCoordinate::new(
                    master_seed,
                    replicate_id,
                    0,
                    0,
                    M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID,
                    source_id.0,
                    (edge_index * 2 + 1) as u32,
                );
                let u = coordinate_prng_f32(&target_coordinate);
                let draw_rank = ((u * remaining as f32).floor() as usize).min(remaining - 1);
                let base_rank = adjust_rank_for_exclusions(draw_rank, excluded);

                let target_rank = if is_local {
                    *source_group_ranks
                        .get(base_rank)
                        .ok_or(RelationshipGraphError::RankInvariantViolation)?
                } else {
                    complement_rank_to_global_rank(source_group_ranks, agents.len(), base_rank)
                };
                if target_rank >= agents.len() || excluded.binary_search(&base_rank).is_ok() {
                    return Err(RelationshipGraphError::RankInvariantViolation);
                }

                let target = agents[target_rank];
                if target.1 != source_group_id {
                    cross_partition_edges = cross_partition_edges
                        .checked_add(1)
                        .ok_or(RelationshipGraphError::EdgeCountOverflow)?;
                }
                row.push(target.0);
                let insertion = excluded
                    .binary_search(&base_rank)
                    .unwrap_or_else(|position| position);
                excluded.insert(insertion, base_rank);
            }

            row.sort_unstable();
            neighbors.extend(row);
            row_offsets.push(neighbors.len());
        }

        let total_edges = u64::try_from(neighbors.len())
            .map_err(|_| RelationshipGraphError::EdgeCountOverflow)?;
        let csr_hash = hash_csr(&source_ids, &row_offsets, &neighbors);
        Ok(Self {
            source_ids,
            row_offsets,
            neighbors,
            cross_partition_edges: AtomicU64::new(cross_partition_edges),
            total_edges,
            csr_hash: OnceLock::from(csr_hash),
        })
    }

    /// Builds a canonical graph from explicit rows for the isolated M3P path.
    ///
    /// Unlike initial M2/M3 graph generation, dynamic M3P rows may be shorter than `k`.
    pub(crate) fn from_canonical_rows(
        rows: Vec<(AgentId, Vec<AgentId>)>,
        memberships: &[(AgentId, GroupId)],
    ) -> Result<Self, RelationshipGraphError> {
        Self::from_canonical_rows_with_hash(rows, memberships, true)
    }

    pub(crate) fn from_canonical_rows_dirty(
        rows: Vec<(AgentId, Vec<AgentId>)>,
        memberships: &[(AgentId, GroupId)],
    ) -> Result<Self, RelationshipGraphError> {
        Self::from_canonical_rows_with_hash(rows, memberships, false)
    }

    fn from_canonical_rows_with_hash(
        rows: Vec<(AgentId, Vec<AgentId>)>,
        memberships: &[(AgentId, GroupId)],
        compute_hash: bool,
    ) -> Result<Self, RelationshipGraphError> {
        if rows.len() != memberships.len() {
            return Err(RelationshipGraphError::RankInvariantViolation);
        }

        let mut sorted_memberships = memberships.to_vec();
        sorted_memberships.sort_unstable_by_key(|&(agent_id, _)| agent_id);
        for pair in sorted_memberships.windows(2) {
            if pair[0].0 == pair[1].0 {
                return Err(RelationshipGraphError::DuplicateAgentId(pair[0].0));
            }
        }

        let mut source_ids = Vec::with_capacity(rows.len());
        let mut row_offsets = Vec::with_capacity(rows.len() + 1);
        let mut neighbors = Vec::new();
        row_offsets.push(0);
        let mut cross_partition_edges = 0u64;

        for (row_index, (source_id, row_neighbors)) in rows.into_iter().enumerate() {
            if row_index > 0 && source_ids[row_index - 1] >= source_id {
                if source_ids[row_index - 1] == source_id {
                    return Err(RelationshipGraphError::DuplicateAgentId(source_id));
                }
                return Err(RelationshipGraphError::RankInvariantViolation);
            }
            if sorted_memberships.get(row_index).map(|entry| entry.0) != Some(source_id) {
                return Err(RelationshipGraphError::RankInvariantViolation);
            }
            if row_neighbors.len() > 16 {
                return Err(RelationshipGraphError::InvalidDegree(
                    row_neighbors.len() as u32
                ));
            }
            if row_neighbors.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(RelationshipGraphError::RankInvariantViolation);
            }

            let source_group_id = sorted_memberships[row_index].1;
            for &target_id in &row_neighbors {
                let target_index = sorted_memberships
                    .binary_search_by_key(&target_id, |entry| entry.0)
                    .map_err(|_| RelationshipGraphError::RankInvariantViolation)?;
                if target_id == source_id {
                    return Err(RelationshipGraphError::RankInvariantViolation);
                }
                if sorted_memberships[target_index].1 != source_group_id {
                    cross_partition_edges = cross_partition_edges
                        .checked_add(1)
                        .ok_or(RelationshipGraphError::EdgeCountOverflow)?;
                }
            }

            source_ids.push(source_id);
            neighbors.extend(row_neighbors);
            row_offsets.push(neighbors.len());
        }

        let total_edges = u64::try_from(neighbors.len())
            .map_err(|_| RelationshipGraphError::EdgeCountOverflow)?;
        let csr_hash = OnceLock::new();
        if compute_hash {
            let _ = csr_hash.set(hash_csr(&source_ids, &row_offsets, &neighbors));
        }
        Ok(Self {
            source_ids,
            row_offsets,
            neighbors,
            cross_partition_edges: AtomicU64::new(cross_partition_edges),
            total_edges,
            csr_hash,
        })
    }

    pub(crate) fn plan_append_rows(
        &self,
        rows: &[(AgentId, GroupId, Vec<AgentId>)],
        candidate_members: &[(AgentId, GroupId)],
        row_ends: &mut Vec<usize>,
    ) -> Result<RelationshipGraphAppendPlan, RelationshipGraphError> {
        if candidate_members
            .windows(2)
            .any(|pair| pair[0].0 >= pair[1].0)
        {
            return Err(RelationshipGraphError::RankInvariantViolation);
        }

        row_ends.clear();
        row_ends.reserve(rows.len());
        let mut row_end = self.neighbors.len();
        let mut last_source = self.source_ids.last().copied();
        let mut total_edges = self.total_edges;
        let mut cross_partition_edges = self.cross_partition_edges.load(Ordering::Relaxed);
        for (source_id, source_group, neighbors) in rows {
            if last_source.is_some_and(|previous| previous >= *source_id)
                || neighbors.len() > 16
                || neighbors.windows(2).any(|pair| pair[0] >= pair[1])
                || neighbors.contains(source_id)
            {
                return Err(RelationshipGraphError::RankInvariantViolation);
            }
            for target_id in neighbors {
                let target_index = candidate_members
                    .binary_search_by_key(target_id, |entry| entry.0)
                    .map_err(|_| RelationshipGraphError::RankInvariantViolation)?;
                if self.source_ids.binary_search(target_id).is_err() {
                    return Err(RelationshipGraphError::RankInvariantViolation);
                }
                if candidate_members[target_index].1 != *source_group {
                    cross_partition_edges = cross_partition_edges
                        .checked_add(1)
                        .ok_or(RelationshipGraphError::EdgeCountOverflow)?;
                }
            }
            total_edges = total_edges
                .checked_add(
                    u64::try_from(neighbors.len())
                        .map_err(|_| RelationshipGraphError::EdgeCountOverflow)?,
                )
                .ok_or(RelationshipGraphError::EdgeCountOverflow)?;
            row_end = row_end
                .checked_add(neighbors.len())
                .ok_or(RelationshipGraphError::EdgeCountOverflow)?;
            row_ends.push(row_end);
            last_source = Some(*source_id);
        }

        Ok(RelationshipGraphAppendPlan {
            row_count: rows.len(),
            additional_neighbors: row_end - self.neighbors.len(),
            cross_partition_edges,
            total_edges,
        })
    }

    pub(crate) fn reserve_appended_rows(&mut self, plan: &RelationshipGraphAppendPlan) {
        self.source_ids.reserve(plan.row_count);
        self.row_offsets.reserve(plan.row_count);
        self.neighbors.reserve(plan.additional_neighbors);
    }

    pub(crate) fn append_prevalidated_rows(
        &mut self,
        rows: &[(AgentId, GroupId, Vec<AgentId>)],
        row_ends: &[usize],
        plan: RelationshipGraphAppendPlan,
    ) {
        debug_assert_eq!(rows.len(), plan.row_count);
        debug_assert_eq!(rows.len(), row_ends.len());
        for ((source_id, _, neighbors), &row_end) in rows.iter().zip(row_ends) {
            self.source_ids.push(*source_id);
            self.neighbors.extend_from_slice(neighbors);
            self.row_offsets.push(row_end);
        }
        self.cross_partition_edges
            .store(plan.cross_partition_edges, Ordering::Relaxed);
        self.total_edges = plan.total_edges;
        let _ = self.csr_hash.take();
    }

    pub fn source_ids(&self) -> &[AgentId] {
        &self.source_ids
    }

    pub fn row_offsets(&self) -> &[usize] {
        &self.row_offsets
    }

    pub fn neighbors(&self) -> &[AgentId] {
        &self.neighbors
    }

    pub fn neighbors_of(&self, source: AgentId) -> Option<&[AgentId]> {
        let row = self.source_ids.binary_search(&source).ok()?;
        Some(&self.neighbors[self.row_offsets[row]..self.row_offsets[row + 1]])
    }

    pub fn edge_ratio_counts(&self) -> (u64, u64) {
        (
            self.cross_partition_edges.load(Ordering::Relaxed),
            self.total_edges,
        )
    }

    /// Refreshes the membership-derived edge count without changing canonical CSR or its hash.
    pub(crate) fn refresh_cross_partition_edge_count(&self, count: u64) {
        self.cross_partition_edges.store(count, Ordering::Relaxed);
    }

    pub fn csr_hash(&self) -> &[u8; 32] {
        self.csr_hash
            .get_or_init(|| hash_csr(&self.source_ids, &self.row_offsets, &self.neighbors))
    }
}

fn validate_population_count(n: usize) -> Result<(), RelationshipGraphError> {
    if n >= MAX_EXACT_F32_POPULATION {
        Err(RelationshipGraphError::PopulationTooLarge(n))
    } else {
        Ok(())
    }
}

fn validate_capacities(
    group_ids: &[GroupId],
    group_ranks: &[Vec<usize>],
    population: usize,
    degree_k: usize,
    p_local: f32,
) -> Result<(), RelationshipGraphError> {
    if degree_k == 0 {
        return Ok(());
    }
    let requires_local = p_local != 0.0;
    let requires_cross = p_local != 1.0;
    for (group_id, ranks) in group_ids.iter().zip(group_ranks) {
        if requires_local && ranks.len().saturating_sub(1) < degree_k {
            return Err(RelationshipGraphError::InsufficientLocalCapacity {
                group_id: *group_id,
                available: ranks.len().saturating_sub(1),
                required: degree_k,
            });
        }
        let cross_capacity = population - ranks.len();
        if requires_cross && cross_capacity < degree_k {
            return Err(RelationshipGraphError::InsufficientCrossPartitionCapacity {
                group_id: *group_id,
                available: cross_capacity,
                required: degree_k,
            });
        }
    }
    Ok(())
}

pub(crate) fn adjust_rank_for_exclusions(rank: usize, excluded: &[usize]) -> usize {
    let mut adjusted = rank;
    for &excluded_rank in excluded {
        if excluded_rank <= adjusted {
            adjusted += 1;
        } else {
            break;
        }
    }
    adjusted
}

pub(crate) fn complement_rank_to_global_rank(
    group_ranks: &[usize],
    population: usize,
    complement_rank: usize,
) -> usize {
    let (mut low, mut high) = (0, population);
    while low < high {
        let middle = low + (high - low) / 2;
        let complement_prefix = middle + 1 - group_ranks.partition_point(|&rank| rank <= middle);
        if complement_prefix > complement_rank {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

fn hash_csr(source_ids: &[AgentId], row_offsets: &[usize], neighbors: &[AgentId]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"SIMCIV_M3_RELATIONSHIP_CSR_V1");
    hasher.update((source_ids.len() as u64).to_le_bytes());
    for (row, source_id) in source_ids.iter().enumerate() {
        hasher.update(source_id.0.to_le_bytes());
        let start = row_offsets[row];
        let end = row_offsets[row + 1];
        hasher.update(((end - start) as u32).to_le_bytes());
        for neighbor in &neighbors[start..end] {
            hasher.update(neighbor.0.to_le_bytes());
        }
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::{MAX_EXACT_F32_POPULATION, RelationshipGraphError, validate_population_count};

    #[test]
    fn population_count_must_be_below_f32_exact_integer_limit() {
        assert!(validate_population_count(MAX_EXACT_F32_POPULATION - 1).is_ok());
        assert_eq!(
            validate_population_count(MAX_EXACT_F32_POPULATION),
            Err(RelationshipGraphError::PopulationTooLarge(
                MAX_EXACT_F32_POPULATION
            ))
        );
    }
}
