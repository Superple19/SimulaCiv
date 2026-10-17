use sim_core::{AgentId, GroupId, RngCoordinate, coordinate_prng_f32, mix64};
use sim_model::{M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID, RelationshipGraph, RelationshipGraphError};

fn coordinate(
    master_seed: u64,
    replicate_id: u32,
    agent_id: u32,
    draw_index: u32,
) -> RngCoordinate {
    RngCoordinate::new(
        master_seed,
        replicate_id,
        0,
        0,
        M3_RELATIONSHIP_GRAPH_SUBSYSTEM_ID,
        agent_id,
        draw_index,
    )
}

fn oracle(
    memberships: &[(AgentId, GroupId)],
    master_seed: u64,
    replicate_id: u32,
    degree_k: u32,
    p_local: f32,
) -> Vec<(AgentId, Vec<AgentId>)> {
    let mut agents = memberships.to_vec();
    agents.sort_unstable_by_key(|&(agent_id, _)| agent_id);
    agents
        .iter()
        .map(|&(source, source_group)| {
            let mut local: Vec<_> = agents
                .iter()
                .filter_map(|&(agent_id, group_id)| {
                    (group_id == source_group && agent_id != source).then_some(agent_id)
                })
                .collect();
            let mut cross: Vec<_> = agents
                .iter()
                .filter_map(|&(agent_id, group_id)| (group_id != source_group).then_some(agent_id))
                .collect();
            let mut row = Vec::with_capacity(degree_k as usize);
            for edge_index in 0..degree_k {
                let is_local = coordinate_prng_f32(&coordinate(
                    master_seed,
                    replicate_id,
                    source.0,
                    edge_index * 2,
                )) < p_local;
                let pool = if is_local { &mut local } else { &mut cross };
                let count = pool.len();
                let u = coordinate_prng_f32(&coordinate(
                    master_seed,
                    replicate_id,
                    source.0,
                    edge_index * 2 + 1,
                ));
                let index = ((u * count as f32).floor() as usize).min(count - 1);
                row.push(pool.remove(index));
            }
            row.sort_unstable();
            (source, row)
        })
        .collect()
}

fn graph_rows(graph: &RelationshipGraph) -> Vec<(AgentId, Vec<AgentId>)> {
    graph
        .source_ids()
        .iter()
        .map(|&source| (source, graph.neighbors_of(source).unwrap().to_vec()))
        .collect()
}

fn round_robin_memberships(population: usize, settlement_count: usize) -> Vec<(AgentId, GroupId)> {
    (0..population)
        .map(|index| {
            (
                AgentId(index as u32),
                GroupId((index % settlement_count) as u16),
            )
        })
        .collect()
}

fn irregular_memberships() -> Vec<(AgentId, GroupId)> {
    let mut memberships = Vec::new();
    for (group_id, start, count) in [(2, 0, 17), (100, 17, 18), (65_000, 35, 19)] {
        memberships.extend(
            (start..start + count)
                .map(|index| (AgentId((index * 3 + 5) as u32), GroupId(group_id))),
        );
    }
    memberships
}

#[test]
fn rank_index_matches_literal_pool_and_remove_oracle_for_layouts_and_parameters() {
    let layouts = [
        round_robin_memberships(40, 2),
        round_robin_memberships(51, 3),
        irregular_memberships(),
    ];
    let seeds = [0, 1, 0x0123_4567_89ab_cdef];
    let localities = [0.0, 1.0, 0.125, 0.37, 0.5, 0.875];

    for memberships in layouts {
        for &master_seed in &seeds {
            for &p_local in &localities {
                for degree_k in 0..=16 {
                    let expected = oracle(&memberships, master_seed, 7, degree_k, p_local);
                    let graph = RelationshipGraph::generate(
                        &memberships,
                        master_seed,
                        7,
                        degree_k,
                        p_local,
                    )
                    .expect("capacity-valid graph generates");
                    assert_eq!(graph_rows(&graph), expected);

                    let mut shuffled = memberships.clone();
                    let mut shuffle_state = master_seed;
                    for end in (1..shuffled.len()).rev() {
                        shuffle_state = mix64(shuffle_state.wrapping_add(end as u64));
                        let index = (shuffle_state as usize) % (end + 1);
                        shuffled.swap(end, index);
                    }
                    let shuffled_graph =
                        RelationshipGraph::generate(&shuffled, master_seed, 7, degree_k, p_local)
                            .expect("shuffled input generates the same graph");
                    assert_eq!(shuffled_graph, graph);
                }
            }
        }
    }
}

fn grouped_sizes(sizes: &[(u16, usize)]) -> Vec<(AgentId, GroupId)> {
    let mut next_id = 0u32;
    let mut result = Vec::new();
    for &(group_id, count) in sizes {
        for _ in 0..count {
            result.push((AgentId(next_id), GroupId(group_id)));
            next_id += 1;
        }
    }
    result
}

#[test]
fn capacity_boundaries_cover_local_cross_mixed_and_single_settlement_cases() {
    let k = 2;
    for (sizes, p_local) in [
        (&[(4, 3), (19, 3)][..], 1.0),
        (&[(4, 2), (19, 2)][..], 0.0),
        (&[(4, 3), (19, 3)][..], 0.5),
    ] {
        RelationshipGraph::generate(&grouped_sizes(sizes), 1, 0, k, p_local)
            .expect("exact capacity boundary is accepted");
    }

    assert!(matches!(
        RelationshipGraph::generate(&grouped_sizes(&[(4, 2), (19, 3)]), 1, 0, k, 1.0),
        Err(RelationshipGraphError::InsufficientLocalCapacity {
            group_id: GroupId(4),
            available: 1,
            required: 2
        })
    ));
    assert!(matches!(
        RelationshipGraph::generate(&grouped_sizes(&[(4, 1), (19, 4)]), 1, 0, k, 0.0),
        Err(RelationshipGraphError::InsufficientCrossPartitionCapacity {
            group_id: GroupId(19),
            available: 1,
            required: 2
        })
    ));
    assert!(matches!(
        RelationshipGraph::generate(&grouped_sizes(&[(4, 2), (19, 3)]), 1, 0, k, 0.5),
        Err(RelationshipGraphError::InsufficientLocalCapacity { .. })
    ));

    RelationshipGraph::generate(&grouped_sizes(&[(7, 3)]), 1, 0, k, 1.0)
        .expect("single settlement supports local-only sampling");
    assert!(matches!(
        RelationshipGraph::generate(&grouped_sizes(&[(7, 3)]), 1, 0, k, 0.0),
        Err(RelationshipGraphError::InsufficientCrossPartitionCapacity {
            group_id: GroupId(7),
            available: 0,
            required: 2
        })
    ));
    assert!(matches!(
        RelationshipGraph::generate(&grouped_sizes(&[(7, 3)]), 1, 0, k, 0.5),
        Err(RelationshipGraphError::InsufficientCrossPartitionCapacity { .. })
    ));
    RelationshipGraph::generate(&grouped_sizes(&[(7, 1)]), 1, 0, 0, 0.5)
        .expect("zero degree skips capacity checks");
}

#[test]
fn invalid_graph_parameters_and_duplicate_agents_are_rejected() {
    let memberships = grouped_sizes(&[(1, 3), (9, 3)]);
    assert!(matches!(
        RelationshipGraph::generate(&memberships, 1, 0, 17, 0.5),
        Err(RelationshipGraphError::InvalidDegree(17))
    ));
    for p_local in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
        assert!(matches!(
            RelationshipGraph::generate(&memberships, 1, 0, 1, p_local),
            Err(RelationshipGraphError::InvalidLocalityProbability(_))
        ));
    }
    assert!(matches!(
        RelationshipGraph::generate(
            &[(AgentId(1), GroupId(1)), (AgentId(1), GroupId(2))],
            1,
            0,
            0,
            0.5
        ),
        Err(RelationshipGraphError::DuplicateAgentId(AgentId(1)))
    ));
}

#[test]
fn csr_shape_edge_ratio_and_small_configuration_hash_are_canonical() {
    let memberships = [
        (AgentId(1), GroupId(0)),
        (AgentId(2), GroupId(1)),
        (AgentId(3), GroupId(0)),
        (AgentId(4), GroupId(1)),
        (AgentId(5), GroupId(1)),
    ];
    let graph = RelationshipGraph::generate(&memberships, 0x1234, 3, 1, 0.5).unwrap();
    assert_eq!(
        graph.source_ids(),
        &[AgentId(1), AgentId(2), AgentId(3), AgentId(4), AgentId(5)]
    );
    assert_eq!(graph.row_offsets().len(), memberships.len() + 1);
    assert_eq!(*graph.row_offsets().first().unwrap(), 0);
    assert_eq!(
        *graph.row_offsets().last().unwrap(),
        graph.neighbors().len()
    );
    let groups: std::collections::BTreeMap<_, _> = memberships.iter().copied().collect();
    let expected_cross_edges = graph
        .source_ids()
        .iter()
        .flat_map(|source| {
            graph
                .neighbors_of(*source)
                .unwrap()
                .iter()
                .map(|neighbor| groups[source] != groups[neighbor])
        })
        .filter(|&cross| cross)
        .count() as u64;
    assert_eq!(
        graph.edge_ratio_counts(),
        (expected_cross_edges, memberships.len() as u64)
    );
    for source in graph.source_ids() {
        let row = graph.neighbors_of(*source).unwrap();
        assert!(row.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!row.contains(source));
    }
    assert_eq!(
        hex(graph.csr_hash()),
        "91f50ce25b9fe63c117947313b8d7ba837c7573927c0773823818874b842b42a"
    );
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut output, byte| {
        write!(&mut output, "{byte:02x}").unwrap();
        output
    })
}
