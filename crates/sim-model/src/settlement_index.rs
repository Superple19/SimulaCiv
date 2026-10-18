use crate::state::SettlementState;
use sim_core::GroupId;

const NO_SOURCE_INDEX: usize = usize::MAX;

#[derive(Debug)]
enum Representation {
    Dense(Vec<usize>),
    Sorted(Vec<(GroupId, usize)>),
}

/// Private, call-local lookup preserving first-source-match semantics.
#[derive(Debug)]
pub(crate) struct SettlementIndex {
    representation: Representation,
    duplicate_sources: Vec<bool>,
    sort_comparisons: usize,
}

impl SettlementIndex {
    pub(crate) fn new(settlements: &[SettlementState]) -> Self {
        if settlements.len() <= (u16::MAX as usize) + 1 {
            let mut dense = vec![NO_SOURCE_INDEX; settlements.len()];
            let mut is_dense = true;
            for (source_index, settlement) in settlements.iter().enumerate() {
                let id = usize::from(settlement.group_id.0);
                if id >= dense.len() || dense[id] != NO_SOURCE_INDEX {
                    is_dense = false;
                    break;
                }
                dense[id] = source_index;
            }
            if is_dense && dense.iter().all(|&index| index != NO_SOURCE_INDEX) {
                return Self {
                    representation: Representation::Dense(dense),
                    duplicate_sources: vec![false; settlements.len()],
                    sort_comparisons: 0,
                };
            }
        }

        let mut sorted: Vec<_> = settlements
            .iter()
            .enumerate()
            .map(|(index, settlement)| (settlement.group_id, index))
            .collect();
        let mut sort_comparisons = 0;
        sorted.sort_unstable_by(|left, right| {
            sort_comparisons += 1;
            left.cmp(right)
        });

        let mut duplicate_sources = vec![false; settlements.len()];
        let mut run_start = 0;
        while run_start < sorted.len() {
            let group_id = sorted[run_start].0;
            let mut run_end = run_start + 1;
            while run_end < sorted.len() && sorted[run_end].0 == group_id {
                run_end += 1;
            }
            for &(_, source_index) in &sorted[run_start + 1..run_end] {
                duplicate_sources[source_index] = true;
            }
            run_start = run_end;
        }

        Self {
            representation: Representation::Sorted(sorted),
            duplicate_sources,
            sort_comparisons,
        }
    }

    pub(crate) fn source_index(&self, group_id: GroupId) -> Option<usize> {
        self.source_index_with_comparisons(group_id).0
    }

    pub(crate) fn source_index_with_comparisons(
        &self,
        group_id: GroupId,
    ) -> (Option<usize>, usize) {
        match &self.representation {
            Representation::Dense(indices) => indices
                .get(usize::from(group_id.0))
                .copied()
                .filter(|&index| index != NO_SOURCE_INDEX)
                .map_or((None, 0), |index| (Some(index), 0)),
            Representation::Sorted(entries) => {
                let mut low = 0;
                let mut high = entries.len();
                let mut comparisons = 0;
                while low < high {
                    let middle = low + (high - low) / 2;
                    comparisons += 1;
                    if entries[middle].0 < group_id {
                        low = middle + 1;
                    } else {
                        high = middle;
                    }
                }
                if low < entries.len() {
                    comparisons += 1;
                    if entries[low].0 == group_id {
                        return (Some(entries[low].1), comparisons);
                    }
                }
                (None, comparisons)
            }
        }
    }

    pub(crate) fn is_duplicate_source(&self, source_index: usize) -> bool {
        self.duplicate_sources
            .get(source_index)
            .copied()
            .unwrap_or(false)
    }

    pub(crate) fn sort_comparisons(&self) -> usize {
        self.sort_comparisons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settlements(ids: &[u16]) -> Vec<SettlementState> {
        ids.iter()
            .map(|&id| SettlementState {
                group_id: GroupId(id),
                resource: 1.0,
                treasury: 1,
            })
            .collect()
    }

    #[test]
    fn indexed_results_and_missing_errors_match_linear_source_order() {
        let mut state = 0x9e37_79b9_u32;
        for len in 0..=80 {
            let mut ids = Vec::with_capacity(len);
            for _ in 0..len {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ids.push((state >> 16) as u16 % 37);
            }
            let input = settlements(&ids);
            let index = SettlementIndex::new(&input);

            for id in 0..48 {
                let group_id = GroupId(id);
                let linear = input
                    .iter()
                    .position(|settlement| settlement.group_id == group_id);
                assert_eq!(index.source_index(group_id), linear, "ids={ids:?}, id={id}");

                let old_error = linear.ok_or(group_id);
                let indexed_error = index.source_index(group_id).ok_or(group_id);
                assert_eq!(indexed_error, old_error, "ids={ids:?}, id={id}");
            }

            for (source_index, settlement) in input.iter().enumerate() {
                let old_duplicate = input[..source_index]
                    .iter()
                    .any(|prior| prior.group_id == settlement.group_id);
                assert_eq!(index.is_duplicate_source(source_index), old_duplicate);
            }
        }
    }

    #[test]
    fn dense_unsorted_ids_use_exact_source_indices() {
        let input = settlements(&[2, 0, 3, 1]);
        let index = SettlementIndex::new(&input);
        assert_eq!(index.source_index(GroupId(0)), Some(1));
        assert_eq!(index.source_index(GroupId(1)), Some(3));
        assert_eq!(index.source_index(GroupId(2)), Some(0));
        assert_eq!(index.source_index(GroupId(3)), Some(2));
        assert_eq!(index.source_index(GroupId(4)), None);
        assert_eq!(index.sort_comparisons(), 0);
    }

    #[test]
    fn duplicate_and_sparse_ids_keep_first_source_match() {
        let input = settlements(&[8, 3, 8, 42, 3]);
        let index = SettlementIndex::new(&input);
        assert_eq!(index.source_index(GroupId(8)), Some(0));
        assert_eq!(index.source_index(GroupId(3)), Some(1));
        assert_eq!(index.source_index(GroupId(42)), Some(3));
        assert!(index.is_duplicate_source(2));
        assert!(index.is_duplicate_source(4));
        assert!(index.sort_comparisons() > 0);
    }
}
