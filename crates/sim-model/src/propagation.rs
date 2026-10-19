use crate::config::PropagationConfig;
use crate::relationship_graph::RelationshipGraph;
use crate::state::WorldState;
use crate::storage::SegmentedAgentStorage;
use rayon::ThreadPool;
use rayon::prelude::*;
use sim_core::{AgentId, RngCoordinate, coordinate_prng_f32};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;

pub const M3_PROPAGATION_SUBSYSTEM_ID: u16 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PropagationAgentState {
    pub informed: bool,
    pub informed_day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PropagationDailyCounts {
    pub informed_count: u64,
    pub newly_informed: u64,
    pub forgotten: u64,
    pub seeded: u64,
    pub levels_used: u64,
    pub max_frontier_size: u64,
}

pub fn apply_phase9c(
    world: &WorldState,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
) -> Result<PropagationDailyCounts, String> {
    let agent_layout: Vec<_> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.alive))
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        None,
        &[],
    )
}

/// Applies Phase 9c for M3P while keeping same-day newborns inactive.
#[allow(clippy::too_many_arguments)]
pub fn apply_phase9c_m3p(
    world: &WorldState,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    newborn_ids: &[AgentId],
) -> Result<PropagationDailyCounts, String> {
    let agent_layout: Vec<_> = world
        .agents
        .iter()
        .map(|agent| (agent.agent_id, agent.alive))
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        None,
        newborn_ids,
    )
}

pub fn apply_phase9c_storage(
    storage: &SegmentedAgentStorage,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
) -> Result<PropagationDailyCounts, String> {
    if storage.agent_ids().len() != storage.alive().len() {
        return Err("storage propagation columns have inconsistent lengths".into());
    }
    let agent_layout: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        None,
        &[],
    )
}

/// Applies storage Phase 9c for M3P with newborns inactive until the next day.
#[allow(clippy::too_many_arguments)]
pub fn apply_phase9c_storage_m3p(
    storage: &SegmentedAgentStorage,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    newborn_ids: &[AgentId],
) -> Result<PropagationDailyCounts, String> {
    if storage.agent_ids().len() != storage.alive().len() {
        return Err("storage propagation columns have inconsistent lengths".into());
    }
    let agent_layout: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        None,
        newborn_ids,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn apply_phase9c_storage_parallel(
    storage: &SegmentedAgentStorage,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<PropagationDailyCounts, String> {
    if storage.agent_ids().len() != storage.alive().len() {
        return Err("storage propagation columns have inconsistent lengths".into());
    }
    let agent_layout: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        Some((pool, chunk_size)),
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub fn apply_phase9c_storage_m3p_parallel(
    storage: &SegmentedAgentStorage,
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    newborn_ids: &[AgentId],
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<PropagationDailyCounts, String> {
    if storage.agent_ids().len() != storage.alive().len() {
        return Err("storage propagation columns have inconsistent lengths".into());
    }
    let agent_layout: Vec<_> = storage
        .agent_ids()
        .iter()
        .copied()
        .zip(storage.alive().iter().copied())
        .collect();
    apply_phase9c_for_layout(
        &agent_layout,
        graph,
        propagation,
        config,
        master_seed,
        replicate_id,
        day,
        Some((pool, chunk_size)),
        newborn_ids,
    )
}

#[allow(clippy::too_many_arguments)]
fn apply_phase9c_for_layout(
    agent_layout: &[(AgentId, bool)],
    graph: &RelationshipGraph,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    parallel: Option<(&ThreadPool, NonZeroUsize)>,
    newborn_ids: &[AgentId],
) -> Result<PropagationDailyCounts, String> {
    if [
        config.seed_probability,
        config.transmit_probability,
        config.forget_probability,
    ]
    .into_iter()
    .any(|probability| !probability.is_finite() || !(0.0..=1.0).contains(&probability))
        || !(1..=64).contains(&config.max_hops)
    {
        return Err("invalid M3 propagation configuration".into());
    }
    let mut agent_ids: Vec<_> = agent_layout.iter().map(|(agent_id, _)| *agent_id).collect();
    agent_ids.sort_unstable();
    if agent_ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("duplicate AgentId in propagation state".into());
    }
    if graph.source_ids() != agent_ids {
        return Err("propagation graph sources do not match world AgentIds".into());
    }
    if propagation.keys().copied().ne(agent_ids.iter().copied()) {
        return Err("propagation columns do not match world AgentIds".into());
    }

    let newborns =
        (!newborn_ids.is_empty()).then(|| newborn_ids.iter().copied().collect::<BTreeSet<_>>());
    if let Some(newborns) = &newborns
        && newborns
            .iter()
            .any(|agent_id| agent_ids.binary_search(agent_id).is_err())
    {
        return Err("M3P newborn list contains an unknown AgentId".into());
    }

    let live: BTreeSet<_> = if let Some(newborns) = &newborns {
        agent_layout
            .iter()
            .filter(|(agent_id, alive)| *alive && !newborns.contains(agent_id))
            .map(|(agent_id, _)| *agent_id)
            .collect()
    } else {
        agent_layout
            .iter()
            .filter(|(_, alive)| *alive)
            .map(|(agent_id, _)| *agent_id)
            .collect()
    };
    let mut counts = PropagationDailyCounts::default();
    for agent_id in &agent_ids {
        if !live.contains(agent_id) || !propagation[agent_id].informed {
            continue;
        }
        let coordinate = RngCoordinate::new(
            master_seed,
            replicate_id,
            day,
            9,
            M3_PROPAGATION_SUBSYSTEM_ID,
            agent_id.0,
            0,
        );
        if coordinate_prng_f32(&coordinate) < config.forget_probability {
            let state = propagation
                .get_mut(agent_id)
                .expect("validated propagation key");
            state.informed = false;
            state.informed_day = 0;
            counts.forgotten += 1;
        }
    }

    let mut seeded = BTreeSet::new();
    for agent_id in &agent_ids {
        if !live.contains(agent_id) || propagation[agent_id].informed {
            continue;
        }
        let coordinate = RngCoordinate::new(
            master_seed,
            replicate_id,
            day,
            9,
            M3_PROPAGATION_SUBSYSTEM_ID,
            agent_id.0,
            1,
        );
        if coordinate_prng_f32(&coordinate) < config.seed_probability {
            let state = propagation
                .get_mut(agent_id)
                .expect("validated propagation key");
            state.informed = true;
            state.informed_day = day;
            seeded.insert(*agent_id);
            counts.seeded += 1;
        }
    }

    let mut frontier: BTreeSet<_> = agent_ids
        .iter()
        .copied()
        .filter(|agent_id| {
            live.contains(agent_id)
                && propagation[agent_id].informed
                && (seeded.contains(agent_id)
                    || (day > 0 && propagation[agent_id].informed_day == day - 1))
        })
        .collect();
    let mut newly_informed = seeded;
    counts.max_frontier_size = frontier.len() as u64;

    for level in 1..=config.max_hops {
        if frontier.is_empty() {
            break;
        }
        counts.levels_used += 1;
        let frontier_agents: Vec<_> = frontier.iter().copied().collect();
        let candidate_chunks: Vec<Vec<AgentId>> = if let Some((pool, chunk_size)) = parallel {
            pool.install(|| {
                frontier_agents
                    .par_chunks(chunk_size.get())
                    .map(|sources| {
                        evaluate_sources(
                            sources,
                            graph,
                            &live,
                            propagation,
                            config,
                            master_seed,
                            replicate_id,
                            day,
                            level,
                        )
                    })
                    .collect()
            })
        } else {
            vec![evaluate_sources(
                &frontier_agents,
                graph,
                &live,
                propagation,
                config,
                master_seed,
                replicate_id,
                day,
                level,
            )]
        };
        let next_frontier: BTreeSet<_> = candidate_chunks.into_iter().flatten().collect();
        for target_id in &next_frontier {
            let state = propagation
                .get_mut(target_id)
                .expect("validated propagation key");
            state.informed = true;
            state.informed_day = day;
        }
        newly_informed.extend(next_frontier.iter().copied());
        counts.max_frontier_size = counts.max_frontier_size.max(next_frontier.len() as u64);
        frontier = next_frontier;
    }

    counts.newly_informed = newly_informed.len() as u64;
    counts.informed_count = live
        .iter()
        .filter(|agent_id| propagation[agent_id].informed)
        .count() as u64;
    Ok(counts)
}

#[allow(clippy::too_many_arguments)]
fn evaluate_sources(
    sources: &[AgentId],
    graph: &RelationshipGraph,
    live: &BTreeSet<AgentId>,
    propagation: &BTreeMap<AgentId, PropagationAgentState>,
    config: PropagationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    level: u32,
) -> Vec<AgentId> {
    let mut targets = Vec::new();
    for source_id in sources {
        let Some(neighbors) = graph.neighbors_of(*source_id) else {
            continue;
        };
        for (edge_position, target_id) in neighbors.iter().enumerate() {
            if !live.contains(target_id) || propagation[target_id].informed {
                continue;
            }
            let coordinate = RngCoordinate::new(
                master_seed,
                replicate_id,
                day,
                9,
                M3_PROPAGATION_SUBSYSTEM_ID,
                source_id.0,
                2 + level * 64 + edge_position as u32,
            );
            if coordinate_prng_f32(&coordinate) < config.transmit_probability {
                targets.push(*target_id);
            }
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::{PropagationAgentState, PropagationDailyCounts, apply_phase9c};
    use crate::config::PropagationConfig;
    use crate::relationship_graph::RelationshipGraph;
    use crate::state::{AgentState, SettlementState, WorldState};
    use sim_core::{AgentId, DenseSlot, GroupId, SimulationDay};
    use std::collections::BTreeMap;

    fn make_world(alive: &[bool]) -> (WorldState, RelationshipGraph) {
        let agents: Vec<_> = alive
            .iter()
            .enumerate()
            .map(|(index, &is_alive)| AgentState {
                agent_id: AgentId(index as u32),
                dense_slot: DenseSlot(index as u32),
                alive: is_alive,
                birth_day: SimulationDay(0),
                health: 1.0,
                food: 1.0,
                wealth: 0,
                productivity: 1.0,
                cooperation: 0.5,
                aggression: 0.0,
                risk_tolerance: 0.5,
                group_id: GroupId(0),
            })
            .collect();
        let rows = vec![
            (AgentId(0), vec![AgentId(1)]),
            (AgentId(1), vec![AgentId(0), AgentId(2)]),
            (AgentId(2), vec![AgentId(1), AgentId(3)]),
            (AgentId(3), vec![AgentId(2)]),
        ];
        let memberships: Vec<_> = agents
            .iter()
            .map(|agent| (agent.agent_id, agent.group_id))
            .collect();
        let graph = RelationshipGraph::from_canonical_rows(rows, &memberships).unwrap();
        (
            WorldState {
                current_day: SimulationDay(1),
                agents,
                settlements: vec![SettlementState {
                    group_id: GroupId(0),
                    resource: 1.0,
                    treasury: 0,
                }],
                initial_money_supply: 0,
            },
            graph,
        )
    }

    fn propagation() -> BTreeMap<AgentId, PropagationAgentState> {
        (0..4)
            .map(|id| (AgentId(id), PropagationAgentState::default()))
            .collect()
    }

    fn config(
        seed_probability: f32,
        transmit_probability: f32,
        forget_probability: f32,
        max_hops: u32,
    ) -> PropagationConfig {
        PropagationConfig {
            seed_probability,
            transmit_probability,
            forget_probability,
            max_hops,
        }
    }

    #[test]
    fn seed_forget_and_empty_frontier_rules_are_explicit() {
        assert_eq!(
            PropagationAgentState::default(),
            PropagationAgentState {
                informed: false,
                informed_day: 0
            }
        );
        let (world, graph) = make_world(&[true; 4]);
        let mut state = propagation();
        let seeded = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(1.0, 0.0, 0.0, 2),
            7,
            3,
            1,
        )
        .unwrap();
        assert_eq!(seeded.seeded, 4);
        assert_eq!(seeded.newly_informed, 4);
        assert_eq!(seeded.levels_used, 1);

        let forgotten = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 0.0, 1.0, 2),
            7,
            3,
            2,
        )
        .unwrap();
        assert_eq!(forgotten.forgotten, 4);
        assert_eq!(forgotten.seeded, 0);
        assert_eq!(forgotten.informed_count, 0);
        assert!(
            state
                .values()
                .all(|row| !row.informed && row.informed_day == 0)
        );

        let empty = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 1.0, 0.0, 2),
            7,
            3,
            3,
        )
        .unwrap();
        assert_eq!(empty, PropagationDailyCounts::default());
    }

    #[test]
    fn spread_is_synchronous_and_respects_max_hops_and_or_merges() {
        let (world, graph) = make_world(&[true; 4]);
        let mut state = propagation();
        state.insert(
            AgentId(0),
            PropagationAgentState {
                informed: true,
                informed_day: 0,
            },
        );
        let counts = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 1.0, 0.0, 2),
            7,
            3,
            1,
        )
        .unwrap();
        assert!(state[&AgentId(1)].informed);
        assert!(state[&AgentId(2)].informed);
        assert!(!state[&AgentId(3)].informed);
        assert_eq!(counts.levels_used, 2);
        assert_eq!(counts.newly_informed, 2);
        assert_eq!(counts.informed_count, 3);

        let (world, graph) = make_world(&[true; 4]);
        let mut state = propagation();
        state.get_mut(&AgentId(0)).unwrap().informed = true;
        state.get_mut(&AgentId(0)).unwrap().informed_day = 0;
        let counts = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 1.0, 0.0, 1),
            7,
            3,
            1,
        )
        .unwrap();
        assert_eq!(counts.levels_used, 1);
        assert!(state[&AgentId(1)].informed);
        assert!(!state[&AgentId(2)].informed);

        let (world, graph) = make_world(&[true; 4]);
        let mut state = propagation();
        for source in [AgentId(0), AgentId(2)] {
            let row = state.get_mut(&source).unwrap();
            row.informed = true;
            row.informed_day = 0;
        }
        let counts = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 1.0, 0.0, 1),
            7,
            3,
            1,
        )
        .unwrap();
        assert_eq!(counts.newly_informed, 2);
        assert_eq!(counts.informed_count, 4);
        assert!(state[&AgentId(1)].informed);
        assert!(state[&AgentId(3)].informed);
    }

    #[test]
    fn dead_agents_do_not_send_or_receive() {
        let (world, graph) = make_world(&[false, true, false, true]);
        let mut state = propagation();
        for source in [AgentId(0), AgentId(1), AgentId(2)] {
            let row = state.get_mut(&source).unwrap();
            row.informed = true;
            row.informed_day = 0;
        }
        let counts = apply_phase9c(
            &world,
            &graph,
            &mut state,
            config(0.0, 1.0, 0.0, 4),
            7,
            3,
            1,
        )
        .unwrap();
        assert!(state[&AgentId(0)].informed);
        assert!(state[&AgentId(2)].informed);
        assert!(state[&AgentId(1)].informed);
        assert!(!state[&AgentId(3)].informed);
        assert_eq!(counts.informed_count, 1);
        assert_eq!(counts.newly_informed, 0);
    }
}
