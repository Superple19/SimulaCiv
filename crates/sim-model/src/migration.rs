//! Deterministic M4-06 settlement migration system.

use crate::config::MigrationConfig;
use crate::relationship_graph::RelationshipGraph;
use crate::snapshot::{CanonicalSnapshot, SNAPSHOT_MAGIC, SnapshotError};
use crate::state::{SettlementState, WorldState};
use crate::storage::SegmentedAgentStorage;
use sim_core::{AgentId, GroupId, Money, RngCoordinate, coordinate_prng_f32};
use std::collections::{BTreeMap, HashMap};

pub const MIGRATION_RNG_SUBSYSTEM_ID: u16 = 9;
pub const MIGRATION_SNAPSHOT_SCHEMA_VERSION: u32 = 4;
const MIGRATION_PHASE_ID: u8 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MigrationProfileTag {
    M3m = 0x31,
    M3pm = 0x32,
    M3mPropagation = 0x33,
    M3pmPropagation = 0x34,
}

impl MigrationProfileTag {
    const fn carries_graph_section(self) -> bool {
        matches!(self, Self::M3m | Self::M3mPropagation)
    }

    const fn base_snapshot_version(self) -> u32 {
        match self {
            Self::M3m => crate::snapshot::SNAPSHOT_SCHEMA_VERSION,
            Self::M3pm => crate::m3p::M3P_SNAPSHOT_SCHEMA_VERSION,
            Self::M3mPropagation | Self::M3pmPropagation => {
                crate::m3prop::M3PROP_SNAPSHOT_SCHEMA_VERSION
            }
        }
    }
}

impl TryFrom<u8> for MigrationProfileTag {
    type Error = SnapshotError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x31 => Ok(Self::M3m),
            0x32 => Ok(Self::M3pm),
            0x33 => Ok(Self::M3mPropagation),
            0x34 => Ok(Self::M3pmPropagation),
            _ => Err(SnapshotError::InvalidState(
                "unknown M4-06 snapshot profile tag".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationState {
    last_move_day: BTreeMap<AgentId, Option<u32>>,
}

impl MigrationState {
    pub fn new(agent_ids: impl IntoIterator<Item = AgentId>) -> Result<Self, MigrationError> {
        let mut last_move_day = BTreeMap::new();
        for agent_id in agent_ids {
            if last_move_day.insert(agent_id, None).is_some() {
                return Err(MigrationError::DuplicateAgent(agent_id));
            }
        }
        Ok(Self { last_move_day })
    }

    pub fn last_move_day(&self, agent_id: AgentId) -> Option<Option<u32>> {
        self.last_move_day.get(&agent_id).copied()
    }

    pub fn records(&self) -> impl Iterator<Item = (AgentId, Option<u32>)> + '_ {
        self.last_move_day
            .iter()
            .map(|(&agent_id, &day)| (agent_id, day))
    }

    pub fn sync_agent_ids(
        &mut self,
        agent_ids: impl IntoIterator<Item = AgentId>,
    ) -> Result<(), MigrationError> {
        let mut ids = agent_ids.into_iter().collect::<Vec<_>>();
        ids.sort_unstable();
        if let Some(pair) = ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(MigrationError::DuplicateAgent(pair[0]));
        }
        self.last_move_day
            .retain(|agent_id, _| ids.binary_search(agent_id).is_ok());
        for agent_id in ids {
            self.last_move_day.entry(agent_id).or_insert(None);
        }
        Ok(())
    }

    pub fn validate_agent_ids(
        &self,
        agent_ids: impl IntoIterator<Item = AgentId>,
    ) -> Result<(), MigrationError> {
        let mut ids = agent_ids.into_iter().collect::<Vec<_>>();
        ids.sort_unstable();
        if let Some(pair) = ids.windows(2).find(|pair| pair[0] == pair[1]) {
            return Err(MigrationError::DuplicateAgent(pair[0]));
        }
        if ids.len() != self.last_move_day.len()
            || ids.iter().copied().ne(self.last_move_day.keys().copied())
        {
            return Err(MigrationError::AgentSetMismatch);
        }
        Ok(())
    }

    pub fn validate_current_day(&self, current_day: u32) -> Result<(), MigrationError> {
        for (&agent_id, last_move_day) in &self.last_move_day {
            if let Some(day) = last_move_day
                && *day > current_day
            {
                return Err(MigrationError::InvalidLastMoveDay {
                    agent_id,
                    day: *day,
                });
            }
        }
        Ok(())
    }

    pub(crate) fn set_last_move_day(&mut self, agent_id: AgentId, day: u32) {
        self.last_move_day.insert(agent_id, Some(day));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationMetrics {
    pub day: u32,
    pub migrations: u64,
    pub migration_cost: Money,
    pub cross_partition_edge_count_delta: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationEventRecord {
    pub day: u32,
    pub local_sequence: u64,
    pub agent_id: AgentId,
    pub from_group_id: GroupId,
    pub to_group_id: GroupId,
    pub cost: Money,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationDayData {
    pub metrics: Option<MigrationMetrics>,
    pub events: Vec<MigrationEventRecord>,
    pub current_cross_partition_edge_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    DuplicateAgent(AgentId),
    MissingAgent(AgentId),
    MissingSettlement(GroupId),
    AgentSetMismatch,
    ArithmeticOverflow,
    InvalidLastMoveDay { agent_id: AgentId, day: u32 },
    InvalidSnapshot,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateAgent(agent_id) => write!(f, "duplicate migration AgentId {agent_id}"),
            Self::MissingAgent(agent_id) => {
                write!(f, "migration graph refers to missing AgentId {agent_id}")
            }
            Self::MissingSettlement(group_id) => {
                write!(f, "migration refers to missing GroupId {group_id}")
            }
            Self::AgentSetMismatch => write!(
                f,
                "migration state AgentId set does not match canonical agents"
            ),
            Self::ArithmeticOverflow => write!(f, "checked arithmetic overflow in migration plan"),
            Self::InvalidLastMoveDay { agent_id, day } => {
                write!(
                    f,
                    "migration last_move_day {day} is after current day for AgentId {agent_id}"
                )
            }
            Self::InvalidSnapshot => write!(f, "invalid M4-06 migration snapshot"),
        }
    }
}

impl std::error::Error for MigrationError {}

#[derive(Debug, Clone, Copy)]
struct AgentView {
    agent_id: AgentId,
    slot: usize,
    alive: bool,
    birth_day: u32,
    wealth: Money,
    group_id: GroupId,
}

#[derive(Debug, Clone, Copy)]
struct PlannedMove {
    agent_id: AgentId,
    slot: usize,
    from_group_id: GroupId,
    to_group_id: GroupId,
    wealth_before: Money,
    wealth_after: Money,
}

#[derive(Debug, Default, Clone)]
pub struct MigrationScratch {
    agents: Vec<AgentView>,
    agent_lookup: HashMap<AgentId, usize>,
    live_counts: HashMap<GroupId, u64>,
    settlements: HashMap<GroupId, (f32, Money)>,
    settlement_indices: HashMap<GroupId, usize>,
    credits: BTreeMap<GroupId, Money>,
    treasury_updates: Vec<(usize, Money)>,
    candidate_groups: Vec<GroupId>,
    moves: Vec<PlannedMove>,
    edges_before: u64,
    edges_after: u64,
    edge_delta: i64,
    total_cost: Money,
}

impl MigrationScratch {
    pub fn with_capacity(agent_capacity: usize, settlement_capacity: usize) -> Self {
        Self {
            agents: Vec::with_capacity(agent_capacity),
            agent_lookup: HashMap::with_capacity(agent_capacity),
            live_counts: HashMap::with_capacity(settlement_capacity),
            settlements: HashMap::with_capacity(settlement_capacity),
            settlement_indices: HashMap::with_capacity(settlement_capacity),
            credits: BTreeMap::new(),
            treasury_updates: Vec::with_capacity(settlement_capacity),
            candidate_groups: Vec::with_capacity(16),
            moves: Vec::with_capacity(agent_capacity),
            edges_before: 0,
            edges_after: 0,
            edge_delta: 0,
            total_cost: 0,
        }
    }

    fn reset(&mut self) {
        self.agents.clear();
        self.agent_lookup.clear();
        self.live_counts.clear();
        self.settlements.clear();
        self.settlement_indices.clear();
        self.credits.clear();
        self.treasury_updates.clear();
        self.candidate_groups.clear();
        self.moves.clear();
        self.edges_before = 0;
        self.edges_after = 0;
        self.edge_delta = 0;
        self.total_cost = 0;
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_migration_world(
    world: &mut WorldState,
    graph: &RelationshipGraph,
    config: MigrationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    state: &mut MigrationState,
    scratch: &mut MigrationScratch,
) -> Result<MigrationDayData, MigrationError> {
    scratch.reset();
    for (slot, agent) in world.agents.iter().enumerate() {
        scratch.agents.push(AgentView {
            agent_id: agent.agent_id,
            slot,
            alive: agent.alive,
            birth_day: agent.birth_day.as_u32(),
            wealth: agent.wealth,
            group_id: agent.group_id,
        });
    }
    prepare_migration_plan(
        &world.settlements,
        graph,
        config,
        master_seed,
        replicate_id,
        day,
        state,
        scratch,
    )?;
    for planned in &scratch.moves {
        let agent = world
            .agents
            .get_mut(planned.slot)
            .ok_or(MigrationError::MissingAgent(planned.agent_id))?;
        if agent.agent_id != planned.agent_id
            || agent.group_id != planned.from_group_id
            || agent.wealth != planned.wealth_before
        {
            return Err(MigrationError::AgentSetMismatch);
        }
    }
    validate_treasury_credits(&world.settlements, scratch)?;
    for planned in &scratch.moves {
        let agent = &mut world.agents[planned.slot];
        agent.group_id = planned.to_group_id;
        agent.wealth = planned.wealth_after;
        state.set_last_move_day(planned.agent_id, day);
    }
    for (index, treasury_after) in &scratch.treasury_updates {
        world.settlements[*index].treasury = *treasury_after;
    }
    graph.refresh_cross_partition_edge_count(scratch.edges_after);
    Ok(finish_migration_data(day, config, scratch))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_migration_storage(
    storage: &mut SegmentedAgentStorage,
    settlements: &mut [SettlementState],
    graph: &RelationshipGraph,
    config: MigrationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    state: &mut MigrationState,
    scratch: &mut MigrationScratch,
) -> Result<MigrationDayData, MigrationError> {
    scratch.reset();
    let n = storage.len();
    let ids = storage.agent_ids();
    let alive = storage.alive();
    let birth_days = storage.birth_days();
    let wealth = storage.wealth();
    let groups = storage.group_ids();
    if [alive.len(), birth_days.len(), wealth.len(), groups.len()]
        .into_iter()
        .any(|len| len != n)
    {
        return Err(MigrationError::AgentSetMismatch);
    }
    for slot in 0..n {
        scratch.agents.push(AgentView {
            agent_id: ids[slot],
            slot,
            alive: alive[slot],
            birth_day: birth_days[slot].as_u32(),
            wealth: wealth[slot],
            group_id: groups[slot],
        });
    }
    prepare_migration_plan(
        settlements,
        graph,
        config,
        master_seed,
        replicate_id,
        day,
        state,
        scratch,
    )?;
    for planned in &scratch.moves {
        let slot = storage
            .slot_of(planned.agent_id)
            .ok_or(MigrationError::MissingAgent(planned.agent_id))?;
        if slot != planned.slot
            || storage.group_ids()[slot] != planned.from_group_id
            || storage.wealth()[slot] != planned.wealth_before
        {
            return Err(MigrationError::AgentSetMismatch);
        }
    }
    validate_treasury_credits(settlements, scratch)?;
    for planned in &scratch.moves {
        let slot = planned.slot;
        storage.group_ids_mut()[slot] = planned.to_group_id;
        storage.wealth_mut()[slot] = planned.wealth_after;
        state.set_last_move_day(planned.agent_id, day);
    }
    for (index, treasury_after) in &scratch.treasury_updates {
        settlements[*index].treasury = *treasury_after;
    }
    graph.refresh_cross_partition_edge_count(scratch.edges_after);
    Ok(finish_migration_data(day, config, scratch))
}

#[allow(clippy::too_many_arguments)]
fn prepare_migration_plan(
    settlements: &[SettlementState],
    graph: &RelationshipGraph,
    config: MigrationConfig,
    master_seed: u64,
    replicate_id: u32,
    day: u32,
    state: &mut MigrationState,
    scratch: &mut MigrationScratch,
) -> Result<(), MigrationError> {
    scratch.agents.sort_unstable_by_key(|agent| agent.agent_id);
    for (index, agent) in scratch.agents.iter().enumerate() {
        if scratch.agent_lookup.insert(agent.agent_id, index).is_some() {
            return Err(MigrationError::DuplicateAgent(agent.agent_id));
        }
        if agent.alive {
            let count = scratch.live_counts.entry(agent.group_id).or_default();
            *count = count
                .checked_add(1)
                .ok_or(MigrationError::ArithmeticOverflow)?;
        }
    }
    for (index, settlement) in settlements.iter().enumerate() {
        if scratch
            .settlements
            .insert(
                settlement.group_id,
                (settlement.resource, settlement.treasury),
            )
            .is_some()
        {
            return Err(MigrationError::MissingSettlement(settlement.group_id));
        }
        scratch
            .settlement_indices
            .insert(settlement.group_id, index);
    }
    if graph.source_ids().len() != scratch.agents.len()
        || graph
            .source_ids()
            .iter()
            .copied()
            .ne(scratch.agents.iter().map(|agent| agent.agent_id))
    {
        return Err(MigrationError::AgentSetMismatch);
    }

    scratch.edges_before = cross_partition_edges(graph, scratch)?;
    for &agent_id in graph.source_ids() {
        let index = *scratch
            .agent_lookup
            .get(&agent_id)
            .ok_or(MigrationError::MissingAgent(agent_id))?;
        let source = scratch.agents[index];
        if !source.alive {
            continue;
        }
        let draw = coordinate_prng_f32(&RngCoordinate::new(
            master_seed,
            replicate_id,
            day,
            MIGRATION_PHASE_ID,
            MIGRATION_RNG_SUBSYSTEM_ID,
            agent_id.0,
            0,
        ));
        if !source.alive || source.birth_day == day {
            continue;
        }
        if let Some(Some(last_day)) = state.last_move_day(agent_id) {
            if last_day > day {
                return Err(MigrationError::InvalidLastMoveDay {
                    agent_id,
                    day: last_day,
                });
            }
            if day - last_day < config.migration_cooldown_days {
                continue;
            }
        }
        if source.wealth < config.migration_cost {
            continue;
        }

        let current_pc = per_capita_resource(source.group_id, scratch)?;
        scratch.candidate_groups.clear();
        for neighbor_id in graph
            .neighbors_of(agent_id)
            .ok_or(MigrationError::MissingAgent(agent_id))?
        {
            let neighbor_index = *scratch
                .agent_lookup
                .get(neighbor_id)
                .ok_or(MigrationError::MissingAgent(*neighbor_id))?;
            let group_id = scratch.agents[neighbor_index].group_id;
            if group_id != source.group_id
                && scratch.live_counts.get(&group_id).copied().unwrap_or(0) > 0
                && !scratch.candidate_groups.contains(&group_id)
            {
                scratch.candidate_groups.push(group_id);
            }
        }
        scratch.candidate_groups.sort_unstable();
        let mut destination = None;
        for &group_id in &scratch.candidate_groups {
            let per_capita = per_capita_resource(group_id, scratch)?;
            if destination.is_none_or(|(best_group, best_pc): (GroupId, f32)| {
                per_capita > best_pc || (per_capita == best_pc && group_id < best_group)
            }) {
                destination = Some((group_id, per_capita));
            }
        }
        let Some((to_group_id, destination_pc)) = destination else {
            continue;
        };
        let pressure = (destination_pc - current_pc) / current_pc.max(f32::EPSILON);
        if pressure <= config.migration_threshold || draw >= config.migration_probability {
            continue;
        }
        let wealth_after = source
            .wealth
            .checked_sub(config.migration_cost)
            .ok_or(MigrationError::ArithmeticOverflow)?;
        let credit = scratch.credits.entry(to_group_id).or_default();
        *credit = credit
            .checked_add(config.migration_cost)
            .ok_or(MigrationError::ArithmeticOverflow)?;
        scratch.moves.push(PlannedMove {
            agent_id,
            slot: source.slot,
            from_group_id: source.group_id,
            to_group_id,
            wealth_before: source.wealth,
            wealth_after,
        });
    }
    if scratch.moves.len() > u64::MAX as usize {
        return Err(MigrationError::ArithmeticOverflow);
    }
    scratch.total_cost = config
        .migration_cost
        .checked_mul(
            i64::try_from(scratch.moves.len()).map_err(|_| MigrationError::ArithmeticOverflow)?,
        )
        .ok_or(MigrationError::ArithmeticOverflow)?;
    for movement in &scratch.moves {
        let index = scratch.agent_lookup[&movement.agent_id];
        scratch.agents[index].group_id = movement.to_group_id;
    }
    scratch.edges_after = cross_partition_edges(graph, scratch)?;
    scratch.edge_delta = i64::try_from(scratch.edges_after)
        .ok()
        .and_then(|after| {
            i64::try_from(scratch.edges_before)
                .ok()
                .and_then(|before| after.checked_sub(before))
        })
        .ok_or(MigrationError::ArithmeticOverflow)?;
    Ok(())
}

fn per_capita_resource(
    group_id: GroupId,
    scratch: &MigrationScratch,
) -> Result<f32, MigrationError> {
    let (resource, _) = scratch
        .settlements
        .get(&group_id)
        .copied()
        .ok_or(MigrationError::MissingSettlement(group_id))?;
    let count = scratch.live_counts.get(&group_id).copied().unwrap_or(0);
    if count == 0 {
        return Err(MigrationError::MissingSettlement(group_id));
    }
    Ok(resource / count as f32)
}

fn validate_treasury_credits(
    settlements: &[SettlementState],
    scratch: &mut MigrationScratch,
) -> Result<(), MigrationError> {
    for (&group_id, &credit) in &scratch.credits {
        let index = *scratch
            .settlement_indices
            .get(&group_id)
            .ok_or(MigrationError::MissingSettlement(group_id))?;
        let settlement = &settlements[index];
        let treasury_after = settlement
            .treasury
            .checked_add(credit)
            .ok_or(MigrationError::ArithmeticOverflow)?;
        scratch.treasury_updates.push((index, treasury_after));
    }
    Ok(())
}

fn cross_partition_edges(
    graph: &RelationshipGraph,
    scratch: &MigrationScratch,
) -> Result<u64, MigrationError> {
    let mut count = 0_u64;
    for &source_id in graph.source_ids() {
        let source_index = *scratch
            .agent_lookup
            .get(&source_id)
            .ok_or(MigrationError::MissingAgent(source_id))?;
        let source_group = scratch.agents[source_index].group_id;
        for target_id in graph
            .neighbors_of(source_id)
            .ok_or(MigrationError::MissingAgent(source_id))?
        {
            let target_index = *scratch
                .agent_lookup
                .get(target_id)
                .ok_or(MigrationError::MissingAgent(*target_id))?;
            if source_group != scratch.agents[target_index].group_id {
                count = count
                    .checked_add(1)
                    .ok_or(MigrationError::ArithmeticOverflow)?;
            }
        }
    }
    Ok(count)
}

fn finish_migration_data(
    day: u32,
    config: MigrationConfig,
    scratch: &MigrationScratch,
) -> MigrationDayData {
    let events = scratch
        .moves
        .iter()
        .enumerate()
        .map(|(sequence, movement)| MigrationEventRecord {
            day,
            local_sequence: (1_u64 << 63) + sequence as u64,
            agent_id: movement.agent_id,
            from_group_id: movement.from_group_id,
            to_group_id: movement.to_group_id,
            cost: config.migration_cost,
        })
        .collect();
    MigrationDayData {
        metrics: Some(MigrationMetrics {
            day,
            migrations: scratch.moves.len() as u64,
            migration_cost: scratch.total_cost,
            cross_partition_edge_count_delta: scratch.edge_delta,
        }),
        events,
        current_cross_partition_edge_count: scratch.edges_after,
    }
}

pub fn encode_migration_snapshot(
    profile: MigrationProfileTag,
    base: &CanonicalSnapshot,
    state: &MigrationState,
    agent_ids: impl IntoIterator<Item = AgentId>,
    graph: &RelationshipGraph,
) -> Result<CanonicalSnapshot, SnapshotError> {
    validate_base_snapshot(profile, &base.bytes)?;
    let mut ids = agent_ids.into_iter().collect::<Vec<_>>();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1])
        || ids.iter().copied().ne(state.last_move_day.keys().copied())
    {
        return Err(SnapshotError::InvalidState(
            "migration snapshot AgentIds do not match base state".into(),
        ));
    }
    let count = u32::try_from(ids.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let base_len = u64::try_from(base.bytes.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    if graph.source_ids() != ids {
        return Err(SnapshotError::InvalidState(
            "migration snapshot graph does not match canonical AgentIds".into(),
        ));
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SNAPSHOT_MAGIC);
    bytes.extend_from_slice(&MIGRATION_SNAPSHOT_SCHEMA_VERSION.to_le_bytes());
    bytes.push(profile as u8);
    bytes.extend_from_slice(&base_len.to_le_bytes());
    bytes.extend_from_slice(&base.bytes);
    if profile.carries_graph_section() {
        bytes.push(1);
        bytes.extend_from_slice(&count.to_le_bytes());
        for &source_id in graph.source_ids() {
            let neighbors = graph
                .neighbors_of(source_id)
                .ok_or_else(|| SnapshotError::InvalidState("missing migration CSR row".into()))?;
            let degree =
                u32::try_from(neighbors.len()).map_err(|_| SnapshotError::LengthOverflow)?;
            bytes.extend_from_slice(&source_id.0.to_le_bytes());
            bytes.extend_from_slice(&degree.to_le_bytes());
            for neighbor_id in neighbors {
                bytes.extend_from_slice(&neighbor_id.0.to_le_bytes());
            }
        }
        bytes.extend_from_slice(graph.csr_hash());
    } else {
        bytes.push(0);
    }
    bytes.extend_from_slice(&count.to_le_bytes());
    for agent_id in ids {
        bytes.extend_from_slice(&agent_id.0.to_le_bytes());
        match state.last_move_day[&agent_id] {
            None => bytes.push(0),
            Some(day) => {
                bytes.push(1);
                bytes.extend_from_slice(&day.to_le_bytes());
            }
        }
    }
    Ok(CanonicalSnapshot::new(base.metadata.clone(), bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredMigrationSnapshot {
    pub profile: MigrationProfileTag,
    pub base_snapshot: CanonicalSnapshot,
    pub migration_state: MigrationState,
    graph_rows: Option<Vec<(AgentId, Vec<AgentId>)>>,
    graph_hash: Option<[u8; 32]>,
}

impl RestoredMigrationSnapshot {
    pub fn relationship_graph(
        &self,
        memberships: &[(AgentId, GroupId)],
    ) -> Result<Option<RelationshipGraph>, SnapshotError> {
        let Some(rows) = &self.graph_rows else {
            return Ok(None);
        };
        let graph = RelationshipGraph::from_canonical_rows(rows.clone(), memberships)
            .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
        if Some(graph.csr_hash()) != self.graph_hash.as_ref() {
            return Err(SnapshotError::InvalidState(
                "migration snapshot CSR hash does not match decoded base memberships".into(),
            ));
        }
        Ok(Some(graph))
    }
}

pub fn decode_migration_snapshot(
    snapshot: &CanonicalSnapshot,
) -> Result<RestoredMigrationSnapshot, SnapshotError> {
    let bytes = &snapshot.bytes;
    let mut cursor = 0_usize;
    let magic: [u8; 8] = read_slice(bytes, &mut cursor, 8)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SnapshotError::InvalidMagic(magic));
    }
    let version = read_u32(bytes, &mut cursor)?;
    if version != MIGRATION_SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedVersion(version));
    }
    let profile = MigrationProfileTag::try_from(read_u8(bytes, &mut cursor)?)?;
    let base_len = usize::try_from(read_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let base_bytes = read_slice(bytes, &mut cursor, base_len)?.to_vec();
    validate_base_snapshot(profile, &base_bytes)?;
    let graph_tag = read_u8(bytes, &mut cursor)?;
    let graph_required = profile.carries_graph_section();
    if graph_tag != u8::from(graph_required) {
        return Err(SnapshotError::InvalidState(
            "M4-06 snapshot graph section does not match base profile".into(),
        ));
    }
    let (graph_rows, graph_hash) = if graph_required {
        let source_count = read_u32(bytes, &mut cursor)? as usize;
        let mut rows = Vec::with_capacity(source_count);
        let mut source_ids = Vec::with_capacity(source_count);
        for _ in 0..source_count {
            let source_id = AgentId(read_u32(bytes, &mut cursor)?);
            let degree = read_u32(bytes, &mut cursor)? as usize;
            if degree > 16 {
                return Err(SnapshotError::InvalidState(
                    "migration CSR row exceeds maximum degree".into(),
                ));
            }
            let mut neighbors = Vec::with_capacity(degree);
            for _ in 0..degree {
                neighbors.push(AgentId(read_u32(bytes, &mut cursor)?));
            }
            if neighbors.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(SnapshotError::InvalidState(
                    "migration CSR neighbors are not AgentId-sorted".into(),
                ));
            }
            source_ids.push(source_id);
            rows.push((source_id, neighbors));
        }
        if source_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(SnapshotError::UnsortedAgents);
        }
        let stored_hash: [u8; 32] = read_slice(bytes, &mut cursor, 32)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?;
        let memberships = source_ids
            .iter()
            .copied()
            .map(|agent_id| (agent_id, GroupId(0)))
            .collect::<Vec<_>>();
        let graph = RelationshipGraph::from_canonical_rows(rows.clone(), &memberships)
            .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
        if graph.csr_hash() != &stored_hash {
            return Err(SnapshotError::InvalidState(
                "migration snapshot CSR hash does not match canonical rows".into(),
            ));
        }
        (Some(rows), Some(stored_hash))
    } else {
        (None, None)
    };
    let count = read_u32(bytes, &mut cursor)? as usize;
    let mut records = BTreeMap::new();
    let mut previous = None;
    for _ in 0..count {
        let agent_id = AgentId(read_u32(bytes, &mut cursor)?);
        if previous.is_some_and(|previous| previous >= agent_id) {
            return Err(SnapshotError::UnsortedAgents);
        }
        previous = Some(agent_id);
        let last_move_day = match read_u8(bytes, &mut cursor)? {
            0 => None,
            1 => Some(read_u32(bytes, &mut cursor)?),
            value => return Err(SnapshotError::InvalidBoolean(value)),
        };
        records.insert(agent_id, last_move_day);
    }
    if cursor != bytes.len() {
        return Err(SnapshotError::TrailingBytes(bytes.len() - cursor));
    }
    let migration_state = MigrationState {
        last_move_day: records,
    };
    migration_state
        .validate_current_day(snapshot.metadata.day)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    Ok(RestoredMigrationSnapshot {
        profile,
        base_snapshot: CanonicalSnapshot::new(snapshot.metadata.clone(), base_bytes),
        migration_state,
        graph_rows,
        graph_hash,
    })
}

fn validate_base_snapshot(profile: MigrationProfileTag, bytes: &[u8]) -> Result<(), SnapshotError> {
    if bytes.get(..SNAPSHOT_MAGIC.len()) != Some(SNAPSHOT_MAGIC.as_slice()) {
        return Err(SnapshotError::InvalidState(
            "migration envelope does not contain a canonical base snapshot".into(),
        ));
    }
    let version_bytes: [u8; 4] = bytes
        .get(SNAPSHOT_MAGIC.len()..SNAPSHOT_MAGIC.len() + 4)
        .ok_or(SnapshotError::UnexpectedEof)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    let version = u32::from_le_bytes(version_bytes);
    if version != profile.base_snapshot_version() {
        return Err(SnapshotError::InvalidState(format!(
            "migration profile {:?} requires base snapshot schema {}, got {version}",
            profile,
            profile.base_snapshot_version()
        )));
    }
    Ok(())
}

fn read_slice<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], SnapshotError> {
    let end = cursor
        .checked_add(length)
        .ok_or(SnapshotError::LengthOverflow)?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or(SnapshotError::UnexpectedEof)?;
    *cursor = end;
    Ok(slice)
}

fn read_u8(bytes: &[u8], cursor: &mut usize) -> Result<u8, SnapshotError> {
    Ok(read_slice(bytes, cursor, 1)?[0])
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, SnapshotError> {
    Ok(u32::from_le_bytes(
        read_slice(bytes, cursor, 4)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SnapshotError> {
    Ok(u64::from_le_bytes(
        read_slice(bytes, cursor, 8)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}
