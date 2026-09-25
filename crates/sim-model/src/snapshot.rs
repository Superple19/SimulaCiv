//! Canonical snapshot and restore reference implementation (Contract C08 / Phase 11).
//!
//! Provides deterministic canonical binary encoding, decoding, validation, and
//! state restoration for pause/resume equivalence in SimulaCiv M0.

use crate::state::{AgentState, SettlementState, WorldState};
use sim_core::{AgentId, DenseSlot, GroupId, Money, ReplicateId, SimulationDay};

/// Canonical snapshot file magic constant: `SIMCIVM0`.
pub const SNAPSHOT_MAGIC: [u8; 8] = *b"SIMCIVM0";

/// Supported canonical snapshot schema version.
pub const SNAPSHOT_SCHEMA_VERSION: u32 = 1;

/// Fixed byte length of a single serialized agent record.
///
/// Layout (43 bytes total):
/// 1.  agent_id:       u32 (4 bytes LE)
/// 2.  alive:          u8  (1 byte: 0x01 = true, 0x00 = false)
/// 3.  birth_day:      u32 (4 bytes LE)
/// 4.  health:         f32 (4 bytes LE from health.to_bits())
/// 5.  food:           f32 (4 bytes LE from food.to_bits())
/// 6.  wealth:         i64 (8 bytes LE)
/// 7.  productivity:   f32 (4 bytes LE from productivity.to_bits())
/// 8.  cooperation:    f32 (4 bytes LE from cooperation.to_bits())
/// 9.  aggression:     f32 (4 bytes LE from aggression.to_bits())
/// 10. risk_tolerance: f32 (4 bytes LE from risk_tolerance.to_bits())
/// 11. group_id:       u16 (2 bytes LE)
pub const SERIALIZED_AGENT_RECORD_BYTES: usize = 43;

/// Fixed byte length of a single serialized settlement record.
///
/// Layout (14 bytes total):
/// 1. group_id: u16 (2 bytes LE)
/// 2. resource: f32 (4 bytes LE from resource.to_bits())
/// 3. treasury: i64 (8 bytes LE)
pub const SERIALIZED_SETTLEMENT_RECORD_BYTES: usize = 14;

/// Compact structured error type for snapshot encoding and decoding failures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotError {
    InvalidMagic([u8; 8]),
    UnsupportedVersion(u32),
    UnexpectedEof,
    TrailingBytes(usize),
    InvalidBoolean(u8),
    InvalidUtf8,
    LengthOverflow,
    DuplicateAgent(AgentId),
    DuplicateSettlement(GroupId),
    UnsortedAgents,
    UnsortedSettlements,
    NonFiniteFloat {
        field: &'static str,
        agent_id: Option<AgentId>,
    },
    NegativeFood {
        agent_id: AgentId,
    },
    NegativeWealth {
        agent_id: AgentId,
        wealth: Money,
    },
    NegativeResource {
        group_id: GroupId,
    },
    NegativeTreasury {
        group_id: GroupId,
        treasury: Money,
    },
    ArithmeticOverflow,
    InvalidState(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic(m) => write!(f, "invalid snapshot magic: {:?}", m),
            Self::UnsupportedVersion(v) => write!(f, "unsupported snapshot schema version: {}", v),
            Self::UnexpectedEof => write!(f, "unexpected end of snapshot payload"),
            Self::TrailingBytes(n) => {
                write!(f, "unexpected trailing bytes after snapshot: {} bytes", n)
            }
            Self::InvalidBoolean(b) => write!(f, "invalid boolean byte in snapshot: 0x{:02x}", b),
            Self::InvalidUtf8 => write!(f, "invalid UTF-8 in snapshot metadata string"),
            Self::LengthOverflow => write!(f, "string or collection length overflow"),
            Self::DuplicateAgent(id) => write!(f, "duplicate agent ID in snapshot: {}", id),
            Self::DuplicateSettlement(gid) => {
                write!(f, "duplicate settlement group ID in snapshot: {}", gid)
            }
            Self::UnsortedAgents => {
                write!(
                    f,
                    "decoded agents are not in strictly ascending AgentId order"
                )
            }
            Self::UnsortedSettlements => write!(
                f,
                "decoded settlements are not in strictly ascending GroupId order"
            ),
            Self::NonFiniteFloat { field, agent_id } => {
                if let Some(id) = agent_id {
                    write!(f, "non-finite float for field '{}' on agent {}", field, id)
                } else {
                    write!(f, "non-finite float for field '{}'", field)
                }
            }
            Self::NegativeFood { agent_id } => write!(f, "negative food for agent {}", agent_id),
            Self::NegativeWealth { agent_id, wealth } => {
                write!(f, "negative wealth for agent {}: {}", agent_id, wealth)
            }
            Self::NegativeResource { group_id } => {
                write!(f, "negative resource for group {}", group_id)
            }
            Self::NegativeTreasury { group_id, treasury } => {
                write!(f, "negative treasury for group {}: {}", group_id, treasury)
            }
            Self::ArithmeticOverflow => write!(f, "arithmetic overflow during snapshot processing"),
            Self::InvalidState(msg) => write!(f, "invalid state in snapshot: {}", msg),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Minimal explicit metadata identifying a snapshot state and trajectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMetadata {
    pub day: u32,
    pub master_seed: u64,
    pub replicate_id: u32,
    pub model_version: String,
    pub config_version: String,
}

impl SnapshotMetadata {
    pub fn new(
        day: u32,
        master_seed: u64,
        replicate_id: u32,
        model_version: impl Into<String>,
        config_version: impl Into<String>,
    ) -> Self {
        Self {
            day,
            master_seed,
            replicate_id,
            model_version: model_version.into(),
            config_version: config_version.into(),
        }
    }

    #[inline]
    pub fn simulation_day(&self) -> SimulationDay {
        SimulationDay(self.day)
    }

    #[inline]
    pub fn replicate(&self) -> ReplicateId {
        ReplicateId(self.replicate_id)
    }
}

/// Canonical binary snapshot payload wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalSnapshot {
    pub metadata: SnapshotMetadata,
    pub bytes: Vec<u8>,
}

impl CanonicalSnapshot {
    pub fn new(metadata: SnapshotMetadata, bytes: Vec<u8>) -> Self {
        Self { metadata, bytes }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl AsRef<[u8]> for CanonicalSnapshot {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl std::ops::Deref for CanonicalSnapshot {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

/// Restored snapshot container containing restored metadata and a freshly reconstructed WorldState.
#[derive(Debug, Clone, PartialEq)]
pub struct RestoredSnapshot {
    pub metadata: SnapshotMetadata,
    pub world: WorldState,
}

/// Encodes an authoritative WorldState and SnapshotMetadata into canonical binary snapshot bytes.
///
/// Invariants:
/// - Takes `world: &WorldState` read-only; performs zero state mutation.
/// - Consumes zero RNG.
/// - Canonicalizes agents in strictly ascending `AgentId` order (physical `world.agents` layout is ignored).
/// - Canonicalizes settlements in strictly ascending `GroupId` order.
/// - Excludes volatile runtime storage indices (`DenseSlot`).
/// - Encodes floats using exact IEEE 754 bit-patterns (`value.to_bits()`).
pub fn encode_snapshot(
    world: &WorldState,
    metadata: &SnapshotMetadata,
) -> Result<CanonicalSnapshot, SnapshotError> {
    // 1. Validate and canonicalize agents in strictly ascending AgentId order
    let mut sorted_agents: Vec<&AgentState> = world.agents.iter().collect();
    sorted_agents.sort_by_key(|a| a.agent_id);

    for (i, agent) in sorted_agents.iter().enumerate() {
        if i > 0 && agent.agent_id == sorted_agents[i - 1].agent_id {
            return Err(SnapshotError::DuplicateAgent(agent.agent_id));
        }
        if !agent.health.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "health",
                agent_id: Some(agent.agent_id),
            });
        }
        if !agent.food.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "food",
                agent_id: Some(agent.agent_id),
            });
        }
        if agent.food < 0.0 {
            return Err(SnapshotError::NegativeFood {
                agent_id: agent.agent_id,
            });
        }
        if agent.wealth < 0 {
            return Err(SnapshotError::NegativeWealth {
                agent_id: agent.agent_id,
                wealth: agent.wealth,
            });
        }
        if !agent.productivity.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "productivity",
                agent_id: Some(agent.agent_id),
            });
        }
        if !agent.cooperation.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "cooperation",
                agent_id: Some(agent.agent_id),
            });
        }
        if !agent.aggression.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "aggression",
                agent_id: Some(agent.agent_id),
            });
        }
        if !agent.risk_tolerance.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "risk_tolerance",
                agent_id: Some(agent.agent_id),
            });
        }
    }

    // 2. Validate and canonicalize settlements in strictly ascending GroupId order
    let mut sorted_settlements: Vec<&SettlementState> = world.settlements.iter().collect();
    sorted_settlements.sort_by_key(|s| s.group_id);

    for (i, settlement) in sorted_settlements.iter().enumerate() {
        if i > 0 && settlement.group_id == sorted_settlements[i - 1].group_id {
            return Err(SnapshotError::DuplicateSettlement(settlement.group_id));
        }
        if !settlement.resource.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "resource",
                agent_id: None,
            });
        }
        if settlement.resource < 0.0 {
            return Err(SnapshotError::NegativeResource {
                group_id: settlement.group_id,
            });
        }
        if settlement.treasury < 0 {
            return Err(SnapshotError::NegativeTreasury {
                group_id: settlement.group_id,
                treasury: settlement.treasury,
            });
        }
    }

    // Check string and collection length overflow
    let model_ver_len: u32 = metadata
        .model_version
        .len()
        .try_into()
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let config_ver_len: u32 = metadata
        .config_version
        .len()
        .try_into()
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let agent_count: u32 = sorted_agents
        .len()
        .try_into()
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let settlement_count: u32 = sorted_settlements
        .len()
        .try_into()
        .map_err(|_| SnapshotError::LengthOverflow)?;

    // 3. Serialize into canonical binary buffer
    let estimated_cap = 44
        + metadata.model_version.len()
        + metadata.config_version.len()
        + (sorted_agents.len() * SERIALIZED_AGENT_RECORD_BYTES)
        + (sorted_settlements.len() * SERIALIZED_SETTLEMENT_RECORD_BYTES);
    let mut buf = Vec::with_capacity(estimated_cap);

    // Header
    buf.extend_from_slice(&SNAPSHOT_MAGIC);
    buf.extend_from_slice(&SNAPSHOT_SCHEMA_VERSION.to_le_bytes());
    buf.extend_from_slice(&metadata.day.to_le_bytes());
    buf.extend_from_slice(&metadata.master_seed.to_le_bytes());
    buf.extend_from_slice(&metadata.replicate_id.to_le_bytes());

    buf.extend_from_slice(&model_ver_len.to_le_bytes());
    buf.extend_from_slice(metadata.model_version.as_bytes());

    buf.extend_from_slice(&config_ver_len.to_le_bytes());
    buf.extend_from_slice(metadata.config_version.as_bytes());

    // Agent count and records (strictly ascending AgentId)
    buf.extend_from_slice(&agent_count.to_le_bytes());
    for agent in sorted_agents {
        buf.extend_from_slice(&agent.agent_id.0.to_le_bytes());
        buf.push(if agent.alive { 0x01 } else { 0x00 });
        buf.extend_from_slice(&agent.birth_day.0.to_le_bytes());
        buf.extend_from_slice(&agent.health.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.food.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.wealth.to_le_bytes());
        buf.extend_from_slice(&agent.productivity.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.cooperation.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.aggression.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.risk_tolerance.to_bits().to_le_bytes());
        buf.extend_from_slice(&agent.group_id.0.to_le_bytes());
    }

    // Settlement count and records (strictly ascending GroupId)
    buf.extend_from_slice(&settlement_count.to_le_bytes());
    for settlement in sorted_settlements {
        buf.extend_from_slice(&settlement.group_id.0.to_le_bytes());
        buf.extend_from_slice(&settlement.resource.to_bits().to_le_bytes());
        buf.extend_from_slice(&settlement.treasury.to_le_bytes());
    }

    Ok(CanonicalSnapshot::new(metadata.clone(), buf))
}

/// Decodes canonical binary snapshot bytes and restores an authoritative WorldState and SnapshotMetadata.
///
/// Invariants:
/// - Rejects invalid magic, unsupported schema version, unexpected EOF, trailing bytes.
/// - Rejects malformed boolean bytes, non-UTF-8 strings, duplicate or unsorted entities.
/// - Assigns DenseSlot deterministically as `DenseSlot(0)..DenseSlot(n - 1)`.
/// - Constructs a brand new WorldState atomically; never partially mutates existing state.
struct SnapshotReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> SnapshotReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], SnapshotError> {
        let end = self
            .cursor
            .checked_add(len)
            .ok_or(SnapshotError::UnexpectedEof)?;
        if end > self.bytes.len() {
            return Err(SnapshotError::UnexpectedEof);
        }
        let slice = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    fn remaining_len(&self) -> usize {
        self.bytes.len() - self.cursor
    }
}

/// Decodes canonical binary snapshot bytes and restores an authoritative WorldState and SnapshotMetadata.
///
/// Invariants:
/// - Rejects invalid magic, unsupported schema version, unexpected EOF, trailing bytes.
/// - Rejects malformed boolean bytes, non-UTF-8 strings, duplicate or unsorted entities.
/// - Assigns DenseSlot deterministically as `DenseSlot(0)..DenseSlot(n - 1)`.
/// - Constructs a brand new WorldState atomically; never partially mutates existing state.
pub fn decode_snapshot(bytes: &[u8]) -> Result<RestoredSnapshot, SnapshotError> {
    let mut reader = SnapshotReader::new(bytes);

    // 1. Magic
    let magic_bytes = reader.read_exact(8)?;
    if magic_bytes != SNAPSHOT_MAGIC {
        let mut arr = [0u8; 8];
        arr.copy_from_slice(magic_bytes);
        return Err(SnapshotError::InvalidMagic(arr));
    }

    // 2. Schema version
    let schema_bytes = reader.read_exact(4)?;
    let schema_version = u32::from_le_bytes(schema_bytes.try_into().unwrap());
    if schema_version != SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedVersion(schema_version));
    }

    // 3. Metadata fields
    let day = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap());
    let master_seed = u64::from_le_bytes(reader.read_exact(8)?.try_into().unwrap());
    let replicate_id = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap());

    let model_ver_len = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap()) as usize;
    let model_ver_bytes = reader.read_exact(model_ver_len)?;
    let model_version = std::str::from_utf8(model_ver_bytes)
        .map_err(|_| SnapshotError::InvalidUtf8)?
        .to_string();

    let config_ver_len = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap()) as usize;
    let config_ver_bytes = reader.read_exact(config_ver_len)?;
    let config_version = std::str::from_utf8(config_ver_bytes)
        .map_err(|_| SnapshotError::InvalidUtf8)?
        .to_string();

    let metadata = SnapshotMetadata {
        day,
        master_seed,
        replicate_id,
        model_version,
        config_version,
    };

    // 4. Agent count and records
    let agent_count = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap()) as usize;
    let total_agent_bytes = agent_count
        .checked_mul(SERIALIZED_AGENT_RECORD_BYTES)
        .ok_or(SnapshotError::UnexpectedEof)?;
    if reader.remaining_len() < total_agent_bytes {
        return Err(SnapshotError::UnexpectedEof);
    }

    let mut agents = Vec::with_capacity(agent_count);
    let mut prev_agent_id: Option<AgentId> = None;
    let mut sum_wealth: Money = 0;

    for i in 0..agent_count {
        let agent_id = AgentId(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if let Some(prev) = prev_agent_id {
            if agent_id == prev {
                return Err(SnapshotError::DuplicateAgent(agent_id));
            } else if agent_id < prev {
                return Err(SnapshotError::UnsortedAgents);
            }
        }
        prev_agent_id = Some(agent_id);

        let alive_byte = reader.read_exact(1)?[0];
        let alive = match alive_byte {
            0x00 => false,
            0x01 => true,
            b => return Err(SnapshotError::InvalidBoolean(b)),
        };

        let birth_day = SimulationDay(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        let health = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !health.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "health",
                agent_id: Some(agent_id),
            });
        }

        let food = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !food.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "food",
                agent_id: Some(agent_id),
            });
        }
        if food < 0.0 {
            return Err(SnapshotError::NegativeFood { agent_id });
        }

        let wealth = Money::from_le_bytes(reader.read_exact(8)?.try_into().unwrap());
        if wealth < 0 {
            return Err(SnapshotError::NegativeWealth { agent_id, wealth });
        }
        sum_wealth = sum_wealth
            .checked_add(wealth)
            .ok_or(SnapshotError::ArithmeticOverflow)?;

        let productivity = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !productivity.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "productivity",
                agent_id: Some(agent_id),
            });
        }

        let cooperation = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !cooperation.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "cooperation",
                agent_id: Some(agent_id),
            });
        }

        let aggression = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !aggression.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "aggression",
                agent_id: Some(agent_id),
            });
        }

        let risk_tolerance = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !risk_tolerance.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "risk_tolerance",
                agent_id: Some(agent_id),
            });
        }

        let group_id = GroupId(u16::from_le_bytes(
            reader.read_exact(2)?.try_into().unwrap(),
        ));

        // DenseSlot restored densely from 0..n-1
        let dense_slot = DenseSlot(i as u32);

        agents.push(AgentState {
            agent_id,
            dense_slot,
            alive,
            birth_day,
            health,
            food,
            wealth,
            productivity,
            cooperation,
            aggression,
            risk_tolerance,
            group_id,
        });
    }

    // 5. Settlement count and records
    let settlement_count = u32::from_le_bytes(reader.read_exact(4)?.try_into().unwrap()) as usize;
    let total_settlement_bytes = settlement_count
        .checked_mul(SERIALIZED_SETTLEMENT_RECORD_BYTES)
        .ok_or(SnapshotError::UnexpectedEof)?;
    if reader.remaining_len() < total_settlement_bytes {
        return Err(SnapshotError::UnexpectedEof);
    }

    let mut settlements = Vec::with_capacity(settlement_count);
    let mut prev_group_id: Option<GroupId> = None;
    let mut sum_treasury: Money = 0;

    for _ in 0..settlement_count {
        let group_id = GroupId(u16::from_le_bytes(
            reader.read_exact(2)?.try_into().unwrap(),
        ));
        if let Some(prev) = prev_group_id {
            if group_id == prev {
                return Err(SnapshotError::DuplicateSettlement(group_id));
            } else if group_id < prev {
                return Err(SnapshotError::UnsortedSettlements);
            }
        }
        prev_group_id = Some(group_id);

        let resource = f32::from_bits(u32::from_le_bytes(
            reader.read_exact(4)?.try_into().unwrap(),
        ));
        if !resource.is_finite() {
            return Err(SnapshotError::NonFiniteFloat {
                field: "resource",
                agent_id: None,
            });
        }
        if resource < 0.0 {
            return Err(SnapshotError::NegativeResource { group_id });
        }

        let treasury = Money::from_le_bytes(reader.read_exact(8)?.try_into().unwrap());
        if treasury < 0 {
            return Err(SnapshotError::NegativeTreasury { group_id, treasury });
        }
        sum_treasury = sum_treasury
            .checked_add(treasury)
            .ok_or(SnapshotError::ArithmeticOverflow)?;

        settlements.push(SettlementState {
            group_id,
            resource,
            treasury,
        });
    }

    // 6. Check trailing bytes
    if reader.remaining_len() > 0 {
        return Err(SnapshotError::TrailingBytes(reader.remaining_len()));
    }

    // 7. Reconstruct initial_money_supply from conserved sum
    let initial_money_supply = sum_wealth
        .checked_add(sum_treasury)
        .ok_or(SnapshotError::ArithmeticOverflow)?;

    let world = WorldState {
        current_day: SimulationDay(day),
        agents,
        settlements,
        initial_money_supply,
    };

    Ok(RestoredSnapshot { metadata, world })
}

/// Restores an authoritative WorldState and SnapshotMetadata from canonical snapshot bytes.
///
/// Convenience wrapper delegating to [`decode_snapshot`].
#[inline]
pub fn restore_snapshot(bytes: &[u8]) -> Result<RestoredSnapshot, SnapshotError> {
    decode_snapshot(bytes)
}

/// Phase 11 conditional snapshot hook.
///
/// Emits a canonical snapshot if at an epoch boundary; otherwise returns `Ok(None)`.
///
/// Invariants:
/// - `at_epoch_boundary == false` -> `Ok(None)`.
/// - `at_epoch_boundary == true` -> `encode_snapshot(world, metadata).map(Some)`.
/// - Does not mutate `world`.
/// - Consumes zero RNG.
pub fn phase11_snapshot_if_boundary(
    world: &WorldState,
    metadata: &SnapshotMetadata,
    at_epoch_boundary: bool,
) -> Result<Option<CanonicalSnapshot>, SnapshotError> {
    if !at_epoch_boundary {
        return Ok(None);
    }
    encode_snapshot(world, metadata).map(Some)
}
