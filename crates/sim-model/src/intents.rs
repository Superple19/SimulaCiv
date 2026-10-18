use crate::config::SimConfig;
use crate::decision::{Action, PrimaryActionChoice};
use crate::relationship_graph::RelationshipGraph;
use crate::state::{AgentState, WorldState};
use crate::subsystems::Subsystem;
use rayon::ThreadPool;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, RngCoordinate, coordinate_prng_f32};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

/// Immutable decision-time Intent record generated during Phase 4.
///
/// Intent records are decision-time evidence only. Resolvers later validate authoritative
/// live state but do not recompute the behavioral decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Intent {
    Work {
        agent_id: AgentId,
        group_id: GroupId,
        requested_harvest: f32,
    },
    BuyFood {
        agent_id: AgentId,
        group_id: GroupId,
        requested_demand: f32,
    },
    SellFood {
        agent_id: AgentId,
        group_id: GroupId,
        submitted_supply: f32,
    },
    GiveFood {
        agent_id: AgentId,
        group_id: GroupId,
        target_agent_id: Option<AgentId>,
        requested_amount: f32,
    },
    StealFood {
        agent_id: AgentId,
        group_id: GroupId,
        target_agent_id: Option<AgentId>,
        requested_amount: f32,
    },
    Idle {
        agent_id: AgentId,
        group_id: GroupId,
    },
}

impl Intent {
    /// Returns the initiating agent's permanent identifier.
    #[inline]
    pub const fn agent_id(&self) -> AgentId {
        match self {
            Self::Work { agent_id, .. }
            | Self::BuyFood { agent_id, .. }
            | Self::SellFood { agent_id, .. }
            | Self::GiveFood { agent_id, .. }
            | Self::StealFood { agent_id, .. }
            | Self::Idle { agent_id, .. } => *agent_id,
        }
    }

    /// Returns the initiating agent's settlement / group identifier.
    #[inline]
    pub const fn group_id(&self) -> GroupId {
        match self {
            Self::Work { group_id, .. }
            | Self::BuyFood { group_id, .. }
            | Self::SellFood { group_id, .. }
            | Self::GiveFood { group_id, .. }
            | Self::StealFood { group_id, .. }
            | Self::Idle { group_id, .. } => *group_id,
        }
    }
}

/// Explicit error returned during Phase 4 Intent generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentError {
    /// A primary action choice references an AgentId that does not exist in authoritative world state.
    MissingAgent(AgentId),
    /// A primary action choice references an agent that is not behaviorally eligible (dead or health <= 0.0).
    IneligibleAgent(AgentId),
    /// Duplicate primary action choices provided for the same AgentId.
    DuplicateChoice(AgentId),
    /// An eligible agent in authoritative world state has no corresponding primary action choice.
    MissingChoiceForEligibleAgent(AgentId),
}

impl std::fmt::Display for IntentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingAgent(id) => write!(f, "choice references non-existent agent: {}", id),
            Self::IneligibleAgent(id) => write!(f, "choice references ineligible agent: {}", id),
            Self::DuplicateChoice(id) => write!(f, "duplicate choice for agent: {}", id),
            Self::MissingChoiceForEligibleAgent(id) => {
                write!(
                    f,
                    "eligible agent {} is missing a primary action choice",
                    id
                )
            }
        }
    }
}

impl std::error::Error for IntentError {}

/// Filters and returns eligible recipient candidates for GiveFood in strictly ascending AgentId order.
///
/// Eligible candidates satisfy all:
/// - candidate.group_id == giver.group_id
/// - candidate.alive == true
/// - candidate.health > 0.0
/// - candidate.agent_id != giver.agent_id
/// - candidate.food < starvation_threshold
pub fn get_give_food_candidates(
    giver: &AgentState,
    world: &WorldState,
    config: &SimConfig,
) -> Vec<AgentId> {
    let mut candidates: Vec<AgentId> = world
        .agents
        .iter()
        .filter(|c| {
            c.group_id == giver.group_id
                && c.alive
                && c.health > 0.0
                && c.agent_id != giver.agent_id
                && c.food < config.interaction.starvation_threshold
        })
        .map(|c| c.agent_id)
        .collect();
    candidates.sort();
    candidates
}

/// Filters and returns eligible victim candidates for StealFood in strictly ascending AgentId order.
///
/// Eligible candidates satisfy all:
/// - candidate.group_id == thief.group_id
/// - candidate.alive == true
/// - candidate.health > 0.0
/// - candidate.agent_id != thief.agent_id
/// - candidate.food > 0.0
pub fn get_steal_food_candidates(thief: &AgentState, world: &WorldState) -> Vec<AgentId> {
    let mut candidates: Vec<AgentId> = world
        .agents
        .iter()
        .filter(|c| {
            c.group_id == thief.group_id
                && c.alive
                && c.health > 0.0
                && c.agent_id != thief.agent_id
                && c.food > 0.0
        })
        .map(|c| c.agent_id)
        .collect();
    candidates.sort();
    candidates
}

/// Filters the initiator's graph neighbors for GiveFood in ascending AgentId order.
pub fn get_give_food_candidates_m3(
    giver: &AgentState,
    world: &WorldState,
    graph: &RelationshipGraph,
    config: &SimConfig,
) -> Vec<AgentId> {
    let agents_by_id = world_agent_lookup(world);
    get_give_food_candidates_m3_with_lookup(giver, graph, config, &agents_by_id)
}

/// Filters the initiator's graph neighbors for StealFood in ascending AgentId order.
pub fn get_steal_food_candidates_m3(
    thief: &AgentState,
    world: &WorldState,
    graph: &RelationshipGraph,
) -> Vec<AgentId> {
    let agents_by_id = world_agent_lookup(world);
    get_steal_food_candidates_m3_with_lookup(thief, graph, &agents_by_id)
}

fn get_give_food_candidates_m3_with_lookup(
    giver: &AgentState,
    graph: &RelationshipGraph,
    config: &SimConfig,
    agents_by_id: &HashMap<AgentId, &AgentState>,
) -> Vec<AgentId> {
    graph
        .neighbors_of(giver.agent_id)
        .into_iter()
        .flatten()
        .filter_map(|&agent_id| {
            let candidate = agents_by_id.get(&agent_id)?;
            (candidate.alive
                && candidate.health > 0.0
                && agent_id != giver.agent_id
                && candidate.food < config.interaction.starvation_threshold)
                .then_some(agent_id)
        })
        .collect()
}

fn get_steal_food_candidates_m3_with_lookup(
    thief: &AgentState,
    graph: &RelationshipGraph,
    agents_by_id: &HashMap<AgentId, &AgentState>,
) -> Vec<AgentId> {
    graph
        .neighbors_of(thief.agent_id)
        .into_iter()
        .flatten()
        .filter_map(|&agent_id| {
            let candidate = agents_by_id.get(&agent_id)?;
            (candidate.alive
                && candidate.health > 0.0
                && agent_id != thief.agent_id
                && candidate.food > 0.0)
                .then_some(agent_id)
        })
        .collect()
}

fn world_agent_lookup(world: &WorldState) -> HashMap<AgentId, &AgentState> {
    let mut agents_by_id = HashMap::with_capacity(world.agents.len());
    for agent in &world.agents {
        agents_by_id.entry(agent.agent_id).or_insert(agent);
    }
    agents_by_id
}

/// Error returned when a targeted intent references an agent absent from the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossPartitionCountError {
    MissingTargetAgent(AgentId),
}

impl std::fmt::Display for CrossPartitionCountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTargetAgent(id) => {
                write!(f, "cross-partition count target does not exist: {id}")
            }
        }
    }
}

impl std::error::Error for CrossPartitionCountError {}

/// Counts targeted GiveFood and StealFood intents crossing group boundaries for one day.
pub fn count_cross_partition_interactions_per_day(
    intents: &[Intent],
    world: &WorldState,
) -> Result<u64, CrossPartitionCountError> {
    let agents_by_id = world_agent_lookup(world);
    let mut count = 0u64;
    for intent in intents {
        let (initiator_group_id, target_agent_id) = match intent {
            Intent::GiveFood {
                group_id,
                target_agent_id: Some(target_agent_id),
                ..
            }
            | Intent::StealFood {
                group_id,
                target_agent_id: Some(target_agent_id),
                ..
            } => (*group_id, *target_agent_id),
            _ => continue,
        };
        let target_group_id = agents_by_id
            .get(&target_agent_id)
            .ok_or(CrossPartitionCountError::MissingTargetAgent(
                target_agent_id,
            ))?
            .group_id;
        if initiator_group_id != target_group_id {
            count += 1;
        }
    }
    Ok(count)
}

/// Counts cross-partition targeted intents using the target's current storage group.
pub fn count_cross_partition_interactions_storage_per_day(
    intents: &[Intent],
    storage: &crate::storage::SegmentedAgentStorage,
) -> Result<u64, CrossPartitionCountError> {
    let mut count = 0u64;
    for intent in intents {
        let (initiator_group_id, target_agent_id) = match intent {
            Intent::GiveFood {
                group_id,
                target_agent_id: Some(target_agent_id),
                ..
            }
            | Intent::StealFood {
                group_id,
                target_agent_id: Some(target_agent_id),
                ..
            } => (*group_id, *target_agent_id),
            _ => continue,
        };
        let slot = storage.slot_of(target_agent_id).ok_or(
            CrossPartitionCountError::MissingTargetAgent(target_agent_id),
        )?;
        if initiator_group_id != storage.group_ids()[slot] {
            count += 1;
        }
    }
    Ok(count)
}

/// Generates immutable Intent records for all primary action choices into a reusable buffer.
///
/// Enforces:
/// 1. Exactly one choice per behaviorally eligible agent.
/// 2. Zero choices for ineligible agents.
/// 3. No duplicate choices for an AgentId.
/// 4. Every referenced agent must exist and be behaviorally eligible.
/// 5. Deterministic target selection for GiveFood and StealFood via coordinate PRNG (DrawIndex = 1).
/// 6. Strict preservation of declared action even with zero quantities or no targets (no-fallback invariant).
/// 7. Output returned in strictly ascending initiator AgentId order.
/// 8. Zero mutation of authoritative world state.
pub fn generate_intents_into(
    world: &WorldState,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    generate_intents_with_graph(world, config, choices, out, None)
}

/// Generates Phase 4 intents with graph-neighbor target selection for the M3 reference profile.
pub fn generate_intents_m3_into(
    world: &WorldState,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    graph: &RelationshipGraph,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    generate_intents_with_graph(world, config, choices, out, Some(graph))
}

/// Reusable Phase 4 projection from canonical graph rows to current SoA storage slots.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct M3Phase4SlotProjection {
    row_offsets: Vec<usize>,
    neighbor_slots: Vec<usize>,
    max_degree: usize,
    storage_is_sorted: bool,
}

impl M3Phase4SlotProjection {
    pub fn new(
        graph: &RelationshipGraph,
        storage: &crate::storage::SegmentedAgentStorage,
    ) -> Result<Self, IntentError> {
        let mut projection = Self::default();
        projection.rebuild(graph, storage)?;
        Ok(projection)
    }

    /// Rebuilds the transient projection after storage construction, replacement, or restore.
    pub fn rebuild(
        &mut self,
        graph: &RelationshipGraph,
        storage: &crate::storage::SegmentedAgentStorage,
    ) -> Result<(), IntentError> {
        self.row_offsets.clear();
        self.row_offsets.resize(storage.len() + 1, 0);
        self.neighbor_slots.clear();

        if graph.source_ids().len() != storage.len() {
            let unmatched = graph
                .source_ids()
                .iter()
                .find(|&&agent_id| storage.slot_of(agent_id).is_none())
                .copied()
                .or_else(|| {
                    storage
                        .agent_ids()
                        .iter()
                        .find(|&&agent_id| graph.neighbors_of(agent_id).is_none())
                        .copied()
                })
                .or_else(|| storage.agent_ids().first().copied())
                .or_else(|| graph.source_ids().first().copied())
                .expect("different graph and storage lengths imply a non-empty side");
            return Err(IntentError::MissingAgent(unmatched));
        }
        for &source_id in graph.source_ids() {
            if storage.slot_of(source_id).is_none() {
                return Err(IntentError::MissingAgent(source_id));
            }
        }

        for slot in 0..storage.len() {
            let agent_id = storage.agent_ids()[slot];
            let neighbors = graph
                .neighbors_of(agent_id)
                .ok_or(IntentError::MissingAgent(agent_id))?;
            self.row_offsets[slot + 1] = self.row_offsets[slot] + neighbors.len();
        }

        self.neighbor_slots.reserve(self.row_offsets[storage.len()]);
        for &agent_id in storage.agent_ids() {
            let neighbors = graph
                .neighbors_of(agent_id)
                .ok_or(IntentError::MissingAgent(agent_id))?;
            for &neighbor_id in neighbors {
                let slot = storage
                    .slot_of(neighbor_id)
                    .ok_or(IntentError::MissingAgent(neighbor_id))?;
                self.neighbor_slots.push(slot);
            }
        }

        self.max_degree = self
            .row_offsets
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .max()
            .unwrap_or(0);
        self.storage_is_sorted = storage
            .agent_ids()
            .windows(2)
            .all(|pair| pair[0] <= pair[1]);

        #[cfg(debug_assertions)]
        self.debug_verify_sample(graph, storage);

        Ok(())
    }

    pub(crate) fn stage_appended_rows(
        &self,
        rows: &[(AgentId, GroupId, Vec<AgentId>)],
        storage: &crate::storage::SegmentedAgentStorage,
        row_ends: &mut Vec<usize>,
        neighbor_slots: &mut Vec<usize>,
    ) -> Result<usize, IntentError> {
        debug_assert_eq!(self.row_offsets.len(), storage.len() + 1);
        row_ends.clear();
        neighbor_slots.clear();
        let mut row_end = self.row_offsets[storage.len()];
        let mut max_degree = self.max_degree;
        for (_, _, neighbors) in rows {
            max_degree = max_degree.max(neighbors.len());
            for &neighbor_id in neighbors {
                let slot = storage
                    .slot_of(neighbor_id)
                    .ok_or(IntentError::MissingAgent(neighbor_id))?;
                neighbor_slots.push(slot);
            }
            row_end = row_end
                .checked_add(neighbors.len())
                .expect("M3P population and degree bounds fit projection offsets");
            row_ends.push(row_end);
        }
        Ok(max_degree)
    }

    pub(crate) fn reserve_appended_rows(&mut self, row_count: usize, neighbor_count: usize) {
        self.row_offsets.reserve(row_count);
        self.neighbor_slots.reserve(neighbor_count);
    }

    pub(crate) fn append_staged_rows(
        &mut self,
        row_ends: &[usize],
        neighbor_slots: &[usize],
        max_degree: usize,
    ) {
        self.row_offsets.extend_from_slice(row_ends);
        self.neighbor_slots.extend_from_slice(neighbor_slots);
        self.max_degree = max_degree;
    }

    fn neighbors_of_slot(&self, slot: usize) -> &[usize] {
        &self.neighbor_slots[self.row_offsets[slot]..self.row_offsets[slot + 1]]
    }

    #[cfg(debug_assertions)]
    fn debug_verify_sample(
        &self,
        graph: &RelationshipGraph,
        storage: &crate::storage::SegmentedAgentStorage,
    ) {
        let source_count = graph.source_ids().len();
        if source_count == 0 {
            return;
        }
        for graph_row in [0, source_count / 2, source_count - 1] {
            let source_id = graph.source_ids()[graph_row];
            let source_slot = storage
                .slot_of(source_id)
                .expect("projection source must exist in storage");
            let graph_neighbors = graph
                .neighbors_of(source_id)
                .expect("projection source row was just built");
            let projected_neighbors = self.neighbors_of_slot(source_slot);
            debug_assert_eq!(projected_neighbors.len(), graph_neighbors.len());
            if graph_neighbors.is_empty() {
                continue;
            }
            for neighbor_index in [0, graph_neighbors.len() / 2, graph_neighbors.len() - 1] {
                let expected_slot = storage
                    .slot_of(graph_neighbors[neighbor_index])
                    .expect("projected neighbor must exist in storage");
                debug_assert_eq!(projected_neighbors[neighbor_index], expected_slot);
            }
        }
    }
}

fn generate_intents_with_graph(
    world: &WorldState,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
    graph: Option<&RelationshipGraph>,
) -> Result<(), IntentError> {
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let agents_by_id = graph.map(|_| world_agent_lookup(world));

    let mut seen_choice_agents = if n > 32 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    for (i, choice) in choices.iter().enumerate() {
        let duplicate = if let Some(ref mut set) = seen_choice_agents {
            !set.insert(choice.agent_id)
        } else {
            choices[..i].iter().any(|c| c.agent_id == choice.agent_id)
        };
        if duplicate {
            return Err(IntentError::DuplicateChoice(choice.agent_id));
        }

        let agent = match &agents_by_id {
            Some(agents_by_id) => agents_by_id.get(&choice.agent_id).copied(),
            None => world.agents.iter().find(|a| a.agent_id == choice.agent_id),
        }
        .ok_or(IntentError::MissingAgent(choice.agent_id))?;

        if !agent.is_behaviorally_eligible() {
            return Err(IntentError::IneligibleAgent(choice.agent_id));
        }

        let intent = match choice.action {
            Action::Work => {
                let requested_harvest =
                    (config.economy.base_work_yield * agent.productivity) * agent.health;
                Intent::Work {
                    agent_id: agent.agent_id,
                    group_id: agent.group_id,
                    requested_harvest,
                }
            }
            Action::BuyFood => {
                let requested_demand = (config.economy.target_food - agent.food).max(0.0);
                Intent::BuyFood {
                    agent_id: agent.agent_id,
                    group_id: agent.group_id,
                    requested_demand,
                }
            }
            Action::SellFood => {
                let submitted_supply = (agent.food - config.economy.target_food).max(0.0);
                Intent::SellFood {
                    agent_id: agent.agent_id,
                    group_id: agent.group_id,
                    submitted_supply,
                }
            }
            Action::GiveFood => {
                let candidates = match (graph, &agents_by_id) {
                    (Some(graph), Some(agents_by_id)) => {
                        get_give_food_candidates_m3_with_lookup(agent, graph, config, agents_by_id)
                    }
                    _ => get_give_food_candidates(agent, world, config),
                };
                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        world.current_day.as_u32(),
                        4,
                        Subsystem::MutualAidTarget.id(),
                        agent.agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.gift_amount.min(agent.food);
                Intent::GiveFood {
                    agent_id: agent.agent_id,
                    group_id: agent.group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::StealFood => {
                let candidates = match (graph, &agents_by_id) {
                    (Some(graph), Some(agents_by_id)) => {
                        get_steal_food_candidates_m3_with_lookup(agent, graph, agents_by_id)
                    }
                    _ => get_steal_food_candidates(agent, world),
                };
                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        world.current_day.as_u32(),
                        4,
                        Subsystem::TheftTarget.id(),
                        agent.agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.theft_amount;
                Intent::StealFood {
                    agent_id: agent.agent_id,
                    group_id: agent.group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::Idle => Intent::Idle {
                agent_id: agent.agent_id,
                group_id: agent.group_id,
            },
        };

        out.push(intent);
    }

    // Enforce exactly one choice for every behaviorally eligible agent in world.agents
    for agent in &world.agents {
        if agent.is_behaviorally_eligible() {
            let choice_present = if let Some(ref set) = seen_choice_agents {
                set.contains(&agent.agent_id)
            } else {
                choices.iter().any(|c| c.agent_id == agent.agent_id)
            };
            if !choice_present {
                return Err(IntentError::MissingChoiceForEligibleAgent(agent.agent_id));
            }
        }
    }

    // Canonical output ordering: strictly ascending initiator AgentId
    out.sort_by_key(|i| i.agent_id());
    Ok(())
}

/// Generates immutable Intent records for all primary action choices.
///
/// Allocates a new vector and delegates to [`generate_intents_into`].
pub fn generate_intents(
    world: &WorldState,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
) -> Result<Vec<Intent>, IntentError> {
    let mut intents = Vec::with_capacity(choices.len());
    generate_intents_into(world, config, choices, &mut intents)?;
    Ok(intents)
}

pub use generate_intents as phase4_generate_intents;
pub use generate_intents_into as phase4_generate_intents_into;

/// Baseline Phase 4 Intent generation directly against [`SegmentedAgentStorage`](crate::storage::SegmentedAgentStorage)
/// using per-agent `slot_of()` HashMap lookup, per-candidate `Vec` allocation, and `HashSet` duplicate tracking (M2-27 reference).
pub fn generate_intents_storage_into_baseline(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let mut seen_choice_agents = if n > 64 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    let alives = &storage.demography.alive;
    let healths = &storage.demography.health;
    let foods = &storage.economy.food;
    let group_ids = &storage.economy.group_id;
    let agent_ids = &storage.agent_ids;

    for choice in choices {
        let slot = storage
            .slot_of(choice.agent_id)
            .ok_or(IntentError::MissingAgent(choice.agent_id))?;

        if !alives[slot] || healths[slot] <= 0.0 {
            return Err(IntentError::IneligibleAgent(choice.agent_id));
        }

        if let Some(ref mut set) = seen_choice_agents {
            if !set.insert(choice.agent_id) {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        } else {
            let count = choices
                .iter()
                .filter(|c| c.agent_id == choice.agent_id)
                .count();
            if count > 1 {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        }

        let agent_id = agent_ids[slot];
        let group_id = group_ids[slot];
        let food = foods[slot];

        let intent = match choice.action {
            Action::Work => {
                let requested_harvest = (config.economy.base_work_yield
                    * storage.personality.productivity[slot])
                    * healths[slot];
                Intent::Work {
                    agent_id,
                    group_id,
                    requested_harvest,
                }
            }
            Action::BuyFood => {
                let requested_demand = (config.economy.target_food - food).max(0.0);
                Intent::BuyFood {
                    agent_id,
                    group_id,
                    requested_demand,
                }
            }
            Action::SellFood => {
                let submitted_supply = (food - config.economy.target_food).max(0.0);
                Intent::SellFood {
                    agent_id,
                    group_id,
                    submitted_supply,
                }
            }
            Action::GiveFood => {
                let mut candidates: Vec<sim_core::AgentId> = (0..storage.len())
                    .filter(|&s| {
                        group_ids[s] == group_id
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != agent_id
                            && foods[s] < config.interaction.starvation_threshold
                    })
                    .map(|s| agent_ids[s])
                    .collect();
                candidates.sort();

                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::MutualAidTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.gift_amount.min(food);
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::StealFood => {
                let mut candidates: Vec<sim_core::AgentId> = (0..storage.len())
                    .filter(|&s| {
                        group_ids[s] == group_id
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != agent_id
                            && foods[s] > 0.0
                    })
                    .map(|s| agent_ids[s])
                    .collect();
                candidates.sort();

                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::TheftTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.theft_amount;
                Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::Idle => Intent::Idle { agent_id, group_id },
        };

        out.push(intent);
    }

    for s in 0..storage.len() {
        if alives[s] && healths[s] > 0.0 {
            let aid = agent_ids[s];
            let choice_present = if let Some(ref set) = seen_choice_agents {
                set.contains(&aid)
            } else {
                choices.iter().any(|c| c.agent_id == aid)
            };
            if !choice_present {
                return Err(IntentError::MissingChoiceForEligibleAgent(aid));
            }
        }
    }

    out.sort_by_key(|i| i.agent_id());
    Ok(())
}

/// Variant C: Native + DenseSlot direct.
/// Eliminates `slot_of()` HashMap lookups via direct sequential cursor,
/// while retaining per-candidate `Vec` allocations and `HashSet` duplicate tracking.
pub fn generate_intents_storage_into_variant_c(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let mut seen_choice_agents = if n > 64 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    let alives = &storage.demography.alive;
    let healths = &storage.demography.health;
    let foods = &storage.economy.food;
    let group_ids = &storage.economy.group_id;
    let agent_ids = &storage.agent_ids;
    let num_agents = storage.len();
    let mut cursor = 0;

    for choice in choices {
        let slot = if cursor < num_agents && agent_ids[cursor] == choice.agent_id {
            let s = cursor;
            cursor += 1;
            s
        } else {
            let mut found = None;
            while cursor < num_agents && agent_ids[cursor] <= choice.agent_id {
                if agent_ids[cursor] == choice.agent_id {
                    found = Some(cursor);
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            if let Some(s) = found {
                s
            } else {
                storage
                    .slot_of(choice.agent_id)
                    .ok_or(IntentError::MissingAgent(choice.agent_id))?
            }
        };

        if !alives[slot] || healths[slot] <= 0.0 {
            return Err(IntentError::IneligibleAgent(choice.agent_id));
        }

        if let Some(ref mut set) = seen_choice_agents {
            if !set.insert(choice.agent_id) {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        } else {
            let count = choices
                .iter()
                .filter(|c| c.agent_id == choice.agent_id)
                .count();
            if count > 1 {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        }

        let agent_id = agent_ids[slot];
        let group_id = group_ids[slot];
        let food = foods[slot];

        let intent = match choice.action {
            Action::Work => {
                let requested_harvest = (config.economy.base_work_yield
                    * storage.personality.productivity[slot])
                    * healths[slot];
                Intent::Work {
                    agent_id,
                    group_id,
                    requested_harvest,
                }
            }
            Action::BuyFood => {
                let requested_demand = (config.economy.target_food - food).max(0.0);
                Intent::BuyFood {
                    agent_id,
                    group_id,
                    requested_demand,
                }
            }
            Action::SellFood => {
                let submitted_supply = (food - config.economy.target_food).max(0.0);
                Intent::SellFood {
                    agent_id,
                    group_id,
                    submitted_supply,
                }
            }
            Action::GiveFood => {
                let mut candidates: Vec<sim_core::AgentId> = (0..storage.len())
                    .filter(|&s| {
                        group_ids[s] == group_id
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != agent_id
                            && foods[s] < config.interaction.starvation_threshold
                    })
                    .map(|s| agent_ids[s])
                    .collect();
                candidates.sort();

                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::MutualAidTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.gift_amount.min(food);
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::StealFood => {
                let mut candidates: Vec<sim_core::AgentId> = (0..storage.len())
                    .filter(|&s| {
                        group_ids[s] == group_id
                            && alives[s]
                            && healths[s] > 0.0
                            && agent_ids[s] != agent_id
                            && foods[s] > 0.0
                    })
                    .map(|s| agent_ids[s])
                    .collect();
                candidates.sort();

                let target_agent_id = if candidates.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::TheftTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidates.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidates[index])
                };
                let requested_amount = config.interaction.theft_amount;
                Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::Idle => Intent::Idle { agent_id, group_id },
        };

        out.push(intent);
    }

    for s in 0..storage.len() {
        if alives[s] && healths[s] > 0.0 {
            let aid = agent_ids[s];
            let choice_present = if let Some(ref set) = seen_choice_agents {
                set.contains(&aid)
            } else {
                choices.iter().any(|c| c.agent_id == aid)
            };
            if !choice_present {
                return Err(IntentError::MissingChoiceForEligibleAgent(aid));
            }
        }
    }

    out.sort_by_key(|i| i.agent_id());
    Ok(())
}

/// Variant D: Native + DenseSlot direct + candidate scratch reuse.
/// Eliminates `slot_of()` HashMap lookups and reuses candidate scratch buffer across
/// GiveFood / StealFood selections, eliminating per-agent dynamic allocations.
pub fn generate_intents_storage_into_variant_d(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let mut seen_choice_agents = if n > 64 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    let alives = &storage.demography.alive;
    let healths = &storage.demography.health;
    let foods = &storage.economy.food;
    let group_ids = &storage.economy.group_id;
    let agent_ids = &storage.agent_ids;
    let num_agents = storage.len();
    let mut cursor = 0;

    for choice in choices {
        let slot = if cursor < num_agents && agent_ids[cursor] == choice.agent_id {
            let s = cursor;
            cursor += 1;
            s
        } else {
            let mut found = None;
            while cursor < num_agents && agent_ids[cursor] <= choice.agent_id {
                if agent_ids[cursor] == choice.agent_id {
                    found = Some(cursor);
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            if let Some(s) = found {
                s
            } else {
                storage
                    .slot_of(choice.agent_id)
                    .ok_or(IntentError::MissingAgent(choice.agent_id))?
            }
        };

        if !alives[slot] || healths[slot] <= 0.0 {
            return Err(IntentError::IneligibleAgent(choice.agent_id));
        }

        if let Some(ref mut set) = seen_choice_agents {
            if !set.insert(choice.agent_id) {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        } else {
            let count = choices
                .iter()
                .filter(|c| c.agent_id == choice.agent_id)
                .count();
            if count > 1 {
                return Err(IntentError::DuplicateChoice(choice.agent_id));
            }
        }

        let agent_id = agent_ids[slot];
        let group_id = group_ids[slot];
        let food = foods[slot];

        let intent = match choice.action {
            Action::Work => {
                let requested_harvest = (config.economy.base_work_yield
                    * storage.personality.productivity[slot])
                    * healths[slot];
                Intent::Work {
                    agent_id,
                    group_id,
                    requested_harvest,
                }
            }
            Action::BuyFood => {
                let requested_demand = (config.economy.target_food - food).max(0.0);
                Intent::BuyFood {
                    agent_id,
                    group_id,
                    requested_demand,
                }
            }
            Action::SellFood => {
                let submitted_supply = (food - config.economy.target_food).max(0.0);
                Intent::SellFood {
                    agent_id,
                    group_id,
                    submitted_supply,
                }
            }
            Action::GiveFood => {
                candidate_scratch.clear();
                for s in 0..num_agents {
                    if group_ids[s] == group_id
                        && alives[s]
                        && healths[s] > 0.0
                        && agent_ids[s] != agent_id
                        && foods[s] < config.interaction.starvation_threshold
                    {
                        candidate_scratch.push(agent_ids[s]);
                    }
                }
                candidate_scratch.sort_unstable();

                let target_agent_id = if candidate_scratch.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::MutualAidTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidate_scratch.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidate_scratch[index])
                };
                let requested_amount = config.interaction.gift_amount.min(food);
                Intent::GiveFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::StealFood => {
                candidate_scratch.clear();
                for s in 0..num_agents {
                    if group_ids[s] == group_id
                        && alives[s]
                        && healths[s] > 0.0
                        && agent_ids[s] != agent_id
                        && foods[s] > 0.0
                    {
                        candidate_scratch.push(agent_ids[s]);
                    }
                }
                candidate_scratch.sort_unstable();

                let target_agent_id = if candidate_scratch.is_empty() {
                    None
                } else {
                    let coord = RngCoordinate::new(
                        config.world.master_seed,
                        config.world.replicate_id,
                        current_day.as_u32(),
                        4,
                        Subsystem::TheftTarget.id(),
                        agent_id.as_u32(),
                        1,
                    );
                    let u = coordinate_prng_f32(&coord);
                    let c = candidate_scratch.len();
                    let index = ((u * (c as f32)).floor() as usize).min(c - 1);
                    Some(candidate_scratch[index])
                };
                let requested_amount = config.interaction.theft_amount;
                Intent::StealFood {
                    agent_id,
                    group_id,
                    target_agent_id,
                    requested_amount,
                }
            }
            Action::Idle => Intent::Idle { agent_id, group_id },
        };

        out.push(intent);
    }

    for s in 0..storage.len() {
        if alives[s] && healths[s] > 0.0 {
            let aid = agent_ids[s];
            let choice_present = if let Some(ref set) = seen_choice_agents {
                set.contains(&aid)
            } else {
                choices.iter().any(|c| c.agent_id == aid)
            };
            if !choice_present {
                return Err(IntentError::MissingChoiceForEligibleAgent(aid));
            }
        }
    }

    out.sort_by_key(|i| i.agent_id());
    Ok(())
}

/// Reusable Phase4-local GroupId/action candidate buckets; not part of simulation state.
#[derive(Default)]
pub struct Phase4CandidateIndexScratch {
    groups: Vec<Phase4CandidateGroup>,
    group_to_index: HashMap<sim_core::GroupId, usize>,
    active_groups: usize,
    storage_is_sorted: bool,
}

#[derive(Default)]
struct Phase4CandidateGroup {
    give_food: Vec<sim_core::AgentId>,
    steal_food: Vec<sim_core::AgentId>,
}

impl Phase4CandidateIndexScratch {
    /// Creates reusable candidate buckets sized for the expected number of groups.
    pub fn with_capacity(group_capacity: usize) -> Self {
        Self {
            groups: Vec::with_capacity(group_capacity),
            group_to_index: HashMap::with_capacity(group_capacity),
            active_groups: 0,
            storage_is_sorted: true,
        }
    }

    /// Rebuilds all action buckets from one immutable Phase4 storage state.
    pub fn build(&mut self, storage: &crate::storage::SegmentedAgentStorage, config: &SimConfig) {
        self.group_to_index.clear();
        self.active_groups = 0;

        let alive = storage.alive();
        let health = storage.health();
        let food = storage.food();
        let groups = storage.group_ids();
        let agent_ids = storage.agent_ids();
        let mut storage_is_sorted = true;

        for slot in 0..storage.len() {
            if slot > 0 && agent_ids[slot - 1] > agent_ids[slot] {
                storage_is_sorted = false;
            }
            if !alive[slot] || health[slot] <= 0.0 {
                continue;
            }

            let give_food_candidate = food[slot] < config.interaction.starvation_threshold;
            let steal_food_candidate = food[slot] > 0.0;
            if !give_food_candidate && !steal_food_candidate {
                continue;
            }

            let bucket = self.group_bucket_mut(groups[slot]);
            if give_food_candidate {
                bucket.give_food.push(agent_ids[slot]);
            }
            if steal_food_candidate {
                bucket.steal_food.push(agent_ids[slot]);
            }
        }

        if !storage_is_sorted {
            for bucket in &mut self.groups[..self.active_groups] {
                bucket.give_food.sort_unstable();
                bucket.steal_food.sort_unstable();
            }
        }
        self.storage_is_sorted = storage_is_sorted;
    }

    fn group_bucket_mut(&mut self, group_id: sim_core::GroupId) -> &mut Phase4CandidateGroup {
        let index = if let Some(&index) = self.group_to_index.get(&group_id) {
            index
        } else {
            let index = self.active_groups;
            if index == self.groups.len() {
                self.groups.push(Phase4CandidateGroup::default());
            }
            let bucket = &mut self.groups[index];
            bucket.give_food.clear();
            bucket.steal_food.clear();
            self.active_groups += 1;
            self.group_to_index.insert(group_id, index);
            index
        };
        &mut self.groups[index]
    }

    fn candidates(&self, group_id: sim_core::GroupId, action: Action) -> &[sim_core::AgentId] {
        let Some(&index) = self.group_to_index.get(&group_id) else {
            return &[];
        };
        match action {
            Action::GiveFood => &self.groups[index].give_food,
            Action::StealFood => &self.groups[index].steal_food,
            _ => &[],
        }
    }

    fn storage_is_sorted(&self) -> bool {
        self.storage_is_sorted
    }
}

enum Phase4CandidateSource<'a> {
    FullScan(&'a mut Vec<sim_core::AgentId>, bool),
    PreIndexed(&'a Phase4CandidateIndexScratch),
    M3Projected(
        &'a M3Phase4SlotProjection,
        &'a mut Vec<sim_core::AgentId>,
        Option<M3Phase4GlobalEligibility>,
    ),
}

impl Phase4CandidateSource<'_> {
    #[allow(clippy::too_many_arguments)]
    fn target_agent(
        &mut self,
        storage: &crate::storage::SegmentedAgentStorage,
        action: Action,
        initiator: sim_core::AgentId,
        initiator_slot: usize,
        current_day: sim_core::SimulationDay,
        config: &SimConfig,
        timing: Option<&mut Duration>,
    ) -> Option<sim_core::AgentId> {
        let started = timing.as_ref().map(|_| Instant::now());
        let group_id = storage.group_ids()[initiator_slot];
        let (candidate_ids, exclude_start, exclude_end) = match self {
            Self::FullScan(candidate_scratch, storage_is_sorted) => {
                candidate_scratch.clear();
                let alive = storage.alive();
                let health = storage.health();
                let food = storage.food();
                let groups = storage.group_ids();
                let agent_ids = storage.agent_ids();

                match action {
                    Action::GiveFood => {
                        for slot in 0..storage.len() {
                            if groups[slot] == group_id
                                && alive[slot]
                                && health[slot] > 0.0
                                && agent_ids[slot] != initiator
                                && food[slot] < config.interaction.starvation_threshold
                            {
                                candidate_scratch.push(agent_ids[slot]);
                            }
                        }
                    }
                    Action::StealFood => {
                        for slot in 0..storage.len() {
                            if groups[slot] == group_id
                                && alive[slot]
                                && health[slot] > 0.0
                                && agent_ids[slot] != initiator
                                && food[slot] > 0.0
                            {
                                candidate_scratch.push(agent_ids[slot]);
                            }
                        }
                    }
                    _ => unreachable!("candidate targets only apply to GiveFood or StealFood"),
                }

                if !*storage_is_sorted {
                    candidate_scratch.sort_unstable();
                }
                (candidate_scratch.as_slice(), 0, 0)
            }
            Self::PreIndexed(index) => {
                let candidates = index.candidates(group_id, action);
                let exclude_start = candidates.partition_point(|id| *id < initiator);
                let exclude_end = candidates.partition_point(|id| *id <= initiator);
                (candidates, exclude_start, exclude_end)
            }
            Self::M3Projected(projection, candidate_scratch, global_eligibility) => {
                candidate_scratch.clear();
                let globally_eligible = global_eligibility.is_none_or(|eligibility| match action {
                    Action::GiveFood => eligibility.give_food,
                    Action::StealFood => eligibility.steal_food,
                    _ => unreachable!("candidate targets only apply to targeted actions"),
                });
                if globally_eligible {
                    let alive = storage.alive();
                    let health = storage.health();
                    let food = storage.food();
                    let agent_ids = storage.agent_ids();
                    for &candidate_slot in projection.neighbors_of_slot(initiator_slot) {
                        let candidate_id = agent_ids[candidate_slot];
                        let eligible = candidate_id != initiator
                            && alive[candidate_slot]
                            && health[candidate_slot] > 0.0
                            && match action {
                                Action::GiveFood => {
                                    food[candidate_slot] < config.interaction.starvation_threshold
                                }
                                Action::StealFood => food[candidate_slot] > 0.0,
                                _ => {
                                    unreachable!("candidate targets only apply to targeted actions")
                                }
                            };
                        if eligible {
                            candidate_scratch.push(candidate_id);
                        }
                    }
                }
                (candidate_scratch.as_slice(), 0, 0)
            }
        };

        let target = phase4_select_candidate(
            candidate_ids,
            exclude_start,
            exclude_end,
            initiator,
            action,
            current_day,
            config,
        );
        if let (Some(timing), Some(started)) = (timing, started) {
            *timing += started.elapsed();
        }
        target
    }
}

fn intent_for_valid_storage_choice(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choice: &PrimaryActionChoice,
    slot: usize,
    candidate_source: &mut Phase4CandidateSource<'_>,
    timing: Option<&mut Duration>,
) -> Intent {
    let agent_id = storage.agent_ids()[slot];
    let group_id = storage.group_ids()[slot];
    let food = storage.food()[slot];
    let health = storage.health()[slot];

    match choice.action {
        Action::Work => {
            let requested_harvest =
                (config.economy.base_work_yield * storage.personality.productivity[slot]) * health;
            Intent::Work {
                agent_id,
                group_id,
                requested_harvest,
            }
        }
        Action::BuyFood => {
            let requested_demand = (config.economy.target_food - food).max(0.0);
            Intent::BuyFood {
                agent_id,
                group_id,
                requested_demand,
            }
        }
        Action::SellFood => {
            let submitted_supply = (food - config.economy.target_food).max(0.0);
            Intent::SellFood {
                agent_id,
                group_id,
                submitted_supply,
            }
        }
        Action::GiveFood => {
            let target_agent_id = candidate_source.target_agent(
                storage,
                Action::GiveFood,
                agent_id,
                slot,
                current_day,
                config,
                timing,
            );
            let requested_amount = config.interaction.gift_amount.min(food);
            Intent::GiveFood {
                agent_id,
                group_id,
                target_agent_id,
                requested_amount,
            }
        }
        Action::StealFood => {
            let target_agent_id = candidate_source.target_agent(
                storage,
                Action::StealFood,
                agent_id,
                slot,
                current_day,
                config,
                timing,
            );
            let requested_amount = config.interaction.theft_amount;
            Intent::StealFood {
                agent_id,
                group_id,
                target_agent_id,
                requested_amount,
            }
        }
        Action::Idle => Intent::Idle { agent_id, group_id },
    }
}

fn phase4_select_candidate(
    candidate_ids: &[sim_core::AgentId],
    exclude_start: usize,
    exclude_end: usize,
    initiator: sim_core::AgentId,
    action: Action,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
) -> Option<sim_core::AgentId> {
    let excluded = exclude_end - exclude_start;
    let candidate_count = candidate_ids.len() - excluded;
    if candidate_count == 0 {
        return None;
    }

    let subsystem = match action {
        Action::GiveFood => Subsystem::MutualAidTarget,
        Action::StealFood => Subsystem::TheftTarget,
        _ => unreachable!("candidate targets only apply to GiveFood or StealFood"),
    };
    let coord = RngCoordinate::new(
        config.world.master_seed,
        config.world.replicate_id,
        current_day.as_u32(),
        4,
        subsystem.id(),
        initiator.as_u32(),
        1,
    );
    let u = coordinate_prng_f32(&coord);
    let logical_index = ((u * (candidate_count as f32)).floor() as usize).min(candidate_count - 1);
    let index = if logical_index < exclude_start {
        logical_index
    } else {
        logical_index + excluded
    };
    Some(candidate_ids[index])
}

/// Generates immutable Intent records with a tick-built reusable Phase4 candidate index.
pub fn generate_intents_storage_with_candidate_index(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    candidate_index: &mut Phase4CandidateIndexScratch,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    candidate_index.build(storage, config);
    generate_intents_storage_impl(
        storage,
        current_day,
        config,
        choices,
        Phase4CandidateSource::PreIndexed(candidate_index),
        out,
        None,
    )
}

/// Generates Phase 4 M3 intents from the caller-owned storage-slot graph projection.
pub fn generate_intents_m3_storage_into(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    projection: &M3Phase4SlotProjection,
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    candidate_scratch.clear();
    candidate_scratch.reserve(projection.max_degree);
    generate_intents_storage_impl(
        storage,
        current_day,
        config,
        choices,
        Phase4CandidateSource::M3Projected(projection, candidate_scratch, None),
        out,
        None,
    )
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct M3Phase4GlobalEligibility {
    pub give_food: bool,
    pub steal_food: bool,
}

/// M3-02b path using a once-per-day proof that one target class is globally empty.
#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_intents_m3_storage_with_global_eligibility_into(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    projection: &M3Phase4SlotProjection,
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    global_eligibility: M3Phase4GlobalEligibility,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    candidate_scratch.clear();
    candidate_scratch.reserve(projection.max_degree);
    generate_intents_storage_impl(
        storage,
        current_day,
        config,
        choices,
        Phase4CandidateSource::M3Projected(projection, candidate_scratch, Some(global_eligibility)),
        out,
        None,
    )
}

/// Opt-in timing for the unchanged M3 storage intent generator.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct M3Phase4IntentTimings {
    pub candidate_filter_and_draw: Duration,
    pub total: Duration,
}

/// Runs the M3 storage intent generator with result-neutral timing observations.
pub fn generate_intents_m3_storage_into_with_timing(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    projection: &M3Phase4SlotProjection,
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    out: &mut Vec<Intent>,
) -> Result<M3Phase4IntentTimings, IntentError> {
    let mut timings = M3Phase4IntentTimings::default();
    candidate_scratch.clear();
    candidate_scratch.reserve(projection.max_degree);
    generate_intents_storage_impl(
        storage,
        current_day,
        config,
        choices,
        Phase4CandidateSource::M3Projected(projection, candidate_scratch, None),
        out,
        Some(&mut timings),
    )?;
    Ok(timings)
}

/// Experimental Rayon Phase4 intent generation using the tick-built candidate index.
/// Validation and its error precedence remain serial; only independent per-agent intent
/// construction runs in parallel, then chunks are merged in input order.
#[allow(clippy::too_many_arguments)]
pub fn generate_intents_storage_with_candidate_index_rayon(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    candidate_index: &mut Phase4CandidateIndexScratch,
    out: &mut Vec<Intent>,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<(), IntentError> {
    candidate_index.build(storage, config);
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let is_choices_sorted = choices.windows(2).all(|w| w[0].agent_id <= w[1].agent_id);
    if is_choices_sorted {
        for w in choices.windows(2) {
            if w[0].agent_id == w[1].agent_id {
                return Err(IntentError::DuplicateChoice(w[0].agent_id));
            }
        }
    }

    let alives = storage.alive();
    let healths = storage.health();
    let agent_ids = storage.agent_ids();
    let num_agents = storage.len();
    let is_storage_sorted = candidate_index.storage_is_sorted();
    let mut cursor = 0;
    let mut slots = Vec::with_capacity(n);
    let mut seen_choice_agents = if !is_choices_sorted && n > 64 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    for (i, choice) in choices.iter().enumerate() {
        let slot = if cursor < num_agents && agent_ids[cursor] == choice.agent_id {
            let slot = cursor;
            cursor += 1;
            Some(slot)
        } else {
            let mut found = None;
            while cursor < num_agents && agent_ids[cursor] <= choice.agent_id {
                if agent_ids[cursor] == choice.agent_id {
                    found = Some(cursor);
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            found.or_else(|| storage.slot_of(choice.agent_id))
        };
        let Some(slot) = slot else {
            append_rayon_intent_chunks(
                storage,
                current_day,
                config,
                &choices[..i],
                &slots,
                candidate_index,
                out,
                pool,
                chunk_size.get(),
            );
            return Err(IntentError::MissingAgent(choice.agent_id));
        };

        let error = if !alives[slot] || healths[slot] <= 0.0 {
            Some(IntentError::IneligibleAgent(choice.agent_id))
        } else if !is_choices_sorted
            && seen_choice_agents.as_mut().map_or_else(
                || {
                    choices[..i]
                        .iter()
                        .any(|previous| previous.agent_id == choice.agent_id)
                },
                |seen| !seen.insert(choice.agent_id),
            )
        {
            Some(IntentError::DuplicateChoice(choice.agent_id))
        } else {
            None
        };

        if let Some(error) = error {
            append_rayon_intent_chunks(
                storage,
                current_day,
                config,
                &choices[..i],
                &slots,
                candidate_index,
                out,
                pool,
                chunk_size.get(),
            );
            return Err(error);
        }
        slots.push(slot);
    }

    append_rayon_intent_chunks(
        storage,
        current_day,
        config,
        choices,
        &slots,
        candidate_index,
        out,
        pool,
        chunk_size.get(),
    );

    if is_choices_sorted && is_storage_sorted {
        let mut choice_index = 0;
        for slot in 0..num_agents {
            if alives[slot] && healths[slot] > 0.0 {
                if choice_index < choices.len() && choices[choice_index].agent_id == agent_ids[slot]
                {
                    choice_index += 1;
                } else {
                    return Err(IntentError::MissingChoiceForEligibleAgent(agent_ids[slot]));
                }
            }
        }
    } else if is_choices_sorted {
        for slot in 0..num_agents {
            if alives[slot]
                && healths[slot] > 0.0
                && choices
                    .binary_search_by_key(&agent_ids[slot], |choice| choice.agent_id)
                    .is_err()
            {
                return Err(IntentError::MissingChoiceForEligibleAgent(agent_ids[slot]));
            }
        }
    } else {
        for slot in 0..num_agents {
            if alives[slot] && healths[slot] > 0.0 {
                let choice_present = seen_choice_agents.as_ref().map_or_else(
                    || {
                        choices
                            .iter()
                            .any(|choice| choice.agent_id == agent_ids[slot])
                    },
                    |seen| seen.contains(&agent_ids[slot]),
                );
                if !choice_present {
                    return Err(IntentError::MissingChoiceForEligibleAgent(agent_ids[slot]));
                }
            }
        }
    }

    out.sort_by_key(|intent| intent.agent_id());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_rayon_intent_chunks(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    slots: &[usize],
    candidate_index: &Phase4CandidateIndexScratch,
    out: &mut Vec<Intent>,
    pool: &ThreadPool,
    chunk_size: usize,
) {
    let mut chunks = pool.install(|| {
        choices
            .par_chunks(chunk_size)
            .zip(slots.par_chunks(chunk_size))
            .enumerate()
            .map(|(chunk_index, (chunk_choices, chunk_slots))| {
                let mut source = Phase4CandidateSource::PreIndexed(candidate_index);
                let intents = chunk_choices
                    .iter()
                    .zip(chunk_slots)
                    .map(|(choice, &slot)| {
                        intent_for_valid_storage_choice(
                            storage,
                            current_day,
                            config,
                            choice,
                            slot,
                            &mut source,
                            None,
                        )
                    })
                    .collect::<Vec<_>>();
                (chunk_index, intents)
            })
            .collect::<Vec<_>>()
    });
    chunks.sort_unstable_by_key(|(chunk_index, _)| *chunk_index);
    for (_, intents) in chunks {
        out.extend(intents);
    }
}

/// Executes Phase 4 Intent generation directly against [`SegmentedAgentStorage`](crate::storage::SegmentedAgentStorage)
/// using the reference full-storage candidate scan.
pub fn generate_intents_storage_with_scratch(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    generate_intents_storage_impl(
        storage,
        current_day,
        config,
        choices,
        Phase4CandidateSource::FullScan(
            candidate_scratch,
            storage
                .agent_ids()
                .windows(2)
                .all(|window| window[0] <= window[1]),
        ),
        out,
        None,
    )
}

fn generate_intents_storage_impl(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    mut candidate_source: Phase4CandidateSource<'_>,
    out: &mut Vec<Intent>,
    mut timings: Option<&mut M3Phase4IntentTimings>,
) -> Result<(), IntentError> {
    let total_started = timings.as_ref().map(|_| Instant::now());
    out.clear();
    match &mut candidate_source {
        Phase4CandidateSource::FullScan(candidate_scratch, _)
        | Phase4CandidateSource::M3Projected(_, candidate_scratch, _) => candidate_scratch.clear(),
        Phase4CandidateSource::PreIndexed(_) => {}
    }
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

    let is_choices_sorted = choices.windows(2).all(|w| w[0].agent_id <= w[1].agent_id);
    let mut seen_choice_agents = if !is_choices_sorted && n > 64 {
        Some(HashSet::with_capacity(n))
    } else {
        None
    };

    if is_choices_sorted {
        for w in choices.windows(2) {
            if w[0].agent_id == w[1].agent_id {
                return Err(IntentError::DuplicateChoice(w[0].agent_id));
            }
        }
    }

    let alives = &storage.demography.alive;
    let healths = &storage.demography.health;
    let agent_ids = &storage.agent_ids;
    let num_agents = storage.len();
    let is_storage_sorted = match &candidate_source {
        Phase4CandidateSource::FullScan(_, is_sorted) => *is_sorted,
        Phase4CandidateSource::PreIndexed(index) => index.storage_is_sorted(),
        Phase4CandidateSource::M3Projected(projection, _, _) => projection.storage_is_sorted,
    };
    let mut cursor = 0;

    for (i, choice) in choices.iter().enumerate() {
        let slot = if cursor < num_agents && agent_ids[cursor] == choice.agent_id {
            let s = cursor;
            cursor += 1;
            s
        } else {
            let mut found = None;
            while cursor < num_agents && agent_ids[cursor] <= choice.agent_id {
                if agent_ids[cursor] == choice.agent_id {
                    found = Some(cursor);
                    cursor += 1;
                    break;
                }
                cursor += 1;
            }
            if let Some(s) = found {
                s
            } else {
                storage
                    .slot_of(choice.agent_id)
                    .ok_or(IntentError::MissingAgent(choice.agent_id))?
            }
        };

        if !alives[slot] || healths[slot] <= 0.0 {
            return Err(IntentError::IneligibleAgent(choice.agent_id));
        }

        if !is_choices_sorted {
            if let Some(ref mut set) = seen_choice_agents {
                if !set.insert(choice.agent_id) {
                    return Err(IntentError::DuplicateChoice(choice.agent_id));
                }
            } else {
                let duplicate = choices[..i].iter().any(|c| c.agent_id == choice.agent_id);
                if duplicate {
                    return Err(IntentError::DuplicateChoice(choice.agent_id));
                }
            }
        }

        out.push(intent_for_valid_storage_choice(
            storage,
            current_day,
            config,
            choice,
            slot,
            &mut candidate_source,
            timings
                .as_deref_mut()
                .map(|timing| &mut timing.candidate_filter_and_draw),
        ));
    }

    if is_choices_sorted && is_storage_sorted {
        let mut c_idx = 0;
        for s in 0..num_agents {
            if alives[s] && healths[s] > 0.0 {
                let aid = agent_ids[s];
                if c_idx < choices.len() && choices[c_idx].agent_id == aid {
                    c_idx += 1;
                } else {
                    return Err(IntentError::MissingChoiceForEligibleAgent(aid));
                }
            }
        }
    } else if is_choices_sorted {
        for s in 0..num_agents {
            if alives[s]
                && healths[s] > 0.0
                && choices
                    .binary_search_by_key(&agent_ids[s], |choice| choice.agent_id)
                    .is_err()
            {
                return Err(IntentError::MissingChoiceForEligibleAgent(agent_ids[s]));
            }
        }
    } else {
        for s in 0..num_agents {
            if alives[s] && healths[s] > 0.0 {
                let aid = agent_ids[s];
                let choice_present = if let Some(ref set) = seen_choice_agents {
                    set.contains(&aid)
                } else {
                    choices.iter().any(|c| c.agent_id == aid)
                };
                if !choice_present {
                    return Err(IntentError::MissingChoiceForEligibleAgent(aid));
                }
            }
        }
    }

    out.sort_by_key(|i| i.agent_id());
    if let (Some(timings), Some(started)) = (timings, total_started) {
        timings.total += started.elapsed();
    }
    Ok(())
}

/// Generates immutable Intent records for all primary action choices directly against [`SegmentedAgentStorage`](crate::storage::SegmentedAgentStorage).
pub fn generate_intents_storage_into(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    let mut candidate_scratch = Vec::with_capacity(storage.len());
    generate_intents_storage_with_scratch(
        storage,
        current_day,
        config,
        choices,
        &mut candidate_scratch,
        out,
    )
}

pub use generate_intents_storage_into as phase4_generate_intents_storage_into;
