use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use sim_core::{AgentId, GroupId, RngCoordinate, coordinate_prng_f32};

/// M3-only subsystem ID; the frozen M2 `Subsystem` mapping remains unchanged.
pub const M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID: u16 = 6;
const MAX_EXACT_F32_POPULATION: usize = 1 << 24;

/// Canonically ordered directed relationship graph in CSR form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationshipGraph {
    source_ids: Vec<AgentId>,
    row_offsets: Vec<usize>,
    neighbors: Vec<AgentId>,
    cross_partition_edges: u64,
    total_edges: u64,
    csr_hash: [u8; 32],
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
            cross_partition_edges,
            total_edges,
            csr_hash,
        })
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
        (self.cross_partition_edges, self.total_edges)
    }

    pub fn csr_hash(&self) -> &[u8; 32] {
        &self.csr_hash
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

fn adjust_rank_for_exclusions(rank: usize, excluded: &[usize]) -> usize {
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

fn complement_rank_to_global_rank(
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
