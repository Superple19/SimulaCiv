use crate::config::SimConfig;
use crate::decision::{Action, PrimaryActionChoice};
use crate::state::{AgentState, WorldState};
use crate::subsystems::Subsystem;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, RngCoordinate, coordinate_prng_f32};
use std::collections::HashSet;

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
    out.clear();
    let n = choices.len();
    if out.capacity() < n {
        out.reserve(n - out.capacity());
    }

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

        let agent = world
            .agents
            .iter()
            .find(|a| a.agent_id == choice.agent_id)
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
                let candidates = get_give_food_candidates(agent, world, config);
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
                let candidates = get_steal_food_candidates(agent, world);
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

/// Executes Phase 4 Intent generation directly against [`SegmentedAgentStorage`](crate::storage::SegmentedAgentStorage)
/// using DenseSlot direct sequential cursor access, reusable candidate scratch storage, and zero-allocation
/// duplicate validation.
pub fn generate_intents_storage_with_scratch(
    storage: &crate::storage::SegmentedAgentStorage,
    current_day: sim_core::SimulationDay,
    config: &SimConfig,
    choices: &[PrimaryActionChoice],
    candidate_scratch: &mut Vec<sim_core::AgentId>,
    out: &mut Vec<Intent>,
) -> Result<(), IntentError> {
    out.clear();
    candidate_scratch.clear();
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
    let foods = &storage.economy.food;
    let group_ids = &storage.economy.group_id;
    let agent_ids = &storage.agent_ids;
    let num_agents = storage.len();
    let is_storage_sorted = agent_ids.windows(2).all(|w| w[0] <= w[1]);
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
                if !is_storage_sorted {
                    candidate_scratch.sort_unstable();
                }

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
                if !is_storage_sorted {
                    candidate_scratch.sort_unstable();
                }

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
