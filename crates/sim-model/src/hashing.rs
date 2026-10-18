//! Canonical determinism oracles for SimulaCiv (Contract C03, C07, C09, C10 / §19.1).
//!
//! Provides deterministic SHA-256 hashing over explicitly specified canonical binary preimages for:
//! - Authoritative simulation state (`CanonicalStateHash`)
//! - Macroscopic metrics time-series (`CanonicalMetricsHash`)
//! - Ordered telemetry event streams (`CanonicalEventHash`)

use crate::events::{Event, EventKey, EventRecord, ObservationEvent, StateTransitionEvent};
use crate::m3p::{M3pDailyMetrics, M3pEvent, M3pEventRecord, M3pReferenceState};
use crate::m3prop::{M3PropDailyMetrics, M3PropagationReferenceState};
use crate::metrics::DailyMetrics;
use crate::metrics::M3DailyMetrics;
use crate::relationship_graph::RelationshipGraph;
use crate::resolution::TargetedActionKind;
use crate::state::WorldState;
use crate::storage::SegmentedAgentStorage;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sim_core::{AgentId, GroupId, Money, SimulationDay};

/// Exact ASCII domain prefix for canonical state preimages.
pub const DOMAIN_STATE: &[u8] = b"SIMCIV_STATE_V1";

/// Exact ASCII domain prefix for canonical metrics preimages.
pub const DOMAIN_METRICS: &[u8] = b"SIMCIV_METRICS_V1";

/// Exact ASCII domain prefix for canonical event preimages.
pub const DOMAIN_EVENTS: &[u8] = b"SIMCIV_EVENTS_V1";

/// Exact ASCII domain prefix for canonical M3 state preimages.
pub const DOMAIN_M3_STATE: &[u8] = b"SIMCIV_M3_STATE_V1";

/// Exact ASCII domain prefix for canonical M3 metrics preimages.
pub const DOMAIN_M3_METRICS: &[u8] = b"SIMCIV_M3_METRICS_V1";

/// Exact ASCII domain prefix for canonical M3P state preimages.
pub const DOMAIN_M3P_STATE: &[u8] = b"SIMCIV_M3P_STATE_V1";

/// Exact ASCII domain prefix for canonical M3P metrics preimages.
pub const DOMAIN_M3P_METRICS: &[u8] = b"SIMCIV_M3P_METRICS_V1";

/// Exact ASCII domain prefix for canonical M3P event preimages.
pub const DOMAIN_M3P_EVENTS: &[u8] = b"SIMCIV_M3P_EVENTS_V1";

/// Exact ASCII domain prefix for M3 propagation state preimages.
pub const DOMAIN_M3PROP_STATE: &[u8] = b"SIMCIV_M3PROP_STATE_V1";

/// Exact ASCII domain prefix for M3 propagation metrics preimages.
pub const DOMAIN_M3PROP_METRICS: &[u8] = b"SIMCIV_M3PROP_METRICS_V1";

/// Exact ASCII domain prefix for M3 propagation event preimages.
pub const DOMAIN_M3PROP_EVENTS: &[u8] = b"SIMCIV_M3PROP_EVENTS_V1";

/// Fixed M3P-only event variant tags, following the existing transition tags.
pub const M3P_EVENT_BIRTH: u8 = 0x05;
pub const M3P_EVENT_CLEANUP: u8 = 0x06;

const CANONICAL_EVENT_KEY_BYTES: usize = 21;

/// 256-bit canonical hash value produced by the determinism oracles.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CanonicalHash(pub [u8; 32]);

impl CanonicalHash {
    #[inline]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Formats the 32-byte digest as a deterministic 64-character lowercase hex string.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(64);
        for b in &self.0 {
            use std::fmt::Write;
            let _ = write!(s, "{:02x}", b);
        }
        s
    }
}

impl std::fmt::Debug for CanonicalHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CanonicalHash({})", self.to_hex())
    }
}

impl std::fmt::Display for CanonicalHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Errors returned during canonical preimage encoding or hashing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalHashError {
    DuplicateAgent(AgentId),
    DuplicateSettlement(GroupId),
    DuplicateMetricDay(u32),
    DuplicateEventKey(EventKey),
    NonFiniteFloat(&'static str),
    NegativeValue(&'static str),
    NegativeMoney(Money),
    LengthOverflow,
    ArithmeticOverflow,
    InvalidPhase(u8),
    InvalidState(String),
}

impl std::fmt::Display for CanonicalHashError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateAgent(aid) => write!(f, "duplicate agent ID in world state: {}", aid),
            Self::DuplicateSettlement(gid) => {
                write!(f, "duplicate settlement group ID in world state: {}", gid)
            }
            Self::DuplicateMetricDay(day) => {
                write!(f, "duplicate metric day in time-series: {}", day)
            }
            Self::DuplicateEventKey(k) => write!(
                f,
                "duplicate event key: (day={}, phase={}, partition={}, seq={})",
                k.day, k.phase, k.partition_key, k.local_sequence
            ),
            Self::NonFiniteFloat(name) => write!(f, "non-finite float in field '{}'", name),
            Self::NegativeValue(name) => write!(f, "negative value in field '{}'", name),
            Self::NegativeMoney(m) => write!(f, "negative money amount: {}", m),
            Self::LengthOverflow => write!(f, "collection length overflowed u32 representation"),
            Self::ArithmeticOverflow => write!(f, "arithmetic overflow during encoding"),
            Self::InvalidPhase(p) => write!(f, "invalid phase: {} (must be 1..=11)", p),
            Self::InvalidState(msg) => write!(f, "invalid state for hashing: {}", msg),
        }
    }
}

impl std::error::Error for CanonicalHashError {}

// =========================================================================
// 1. CanonicalStateHash
// =========================================================================

/// Encodes the authoritative `WorldState` into canonical binary preimage bytes.
///
/// Preimage layout:
/// - `DOMAIN_STATE` (`"SIMCIV_STATE_V1"`, 15 bytes)
/// - `current_day`: `u32 LE` (4 bytes)
/// - `agent_count`: `u32 LE` (4 bytes)
/// - sorted agents in strictly ascending `AgentId` order (43 bytes per agent):
///   - `agent_id`: `u32 LE`
///   - `alive`: `u8` (0x01 or 0x00)
///   - `birth_day`: `u32 LE`
///   - `health`: `u32 LE` (`to_bits()`)
///   - `food`: `u32 LE` (`to_bits()`)
///   - `wealth`: `i64 LE`
///   - `productivity`: `u32 LE` (`to_bits()`)
///   - `cooperation`: `u32 LE` (`to_bits()`)
///   - `aggression`: `u32 LE` (`to_bits()`)
///   - `risk_tolerance`: `u32 LE` (`to_bits()`)
///   - `group_id`: `u16 LE`
/// - `settlement_count`: `u32 LE` (4 bytes)
/// - sorted settlements in strictly ascending `GroupId` order (14 bytes per settlement):
///   - `group_id`: `u16 LE`
///   - `resource`: `u32 LE` (`to_bits()`)
///   - `treasury`: `i64 LE`
///
/// Excludes: `DenseSlot`, `SnapshotMetadata`, `master_seed`, `replicate_id`, etc.
pub fn canonical_state_bytes(world: &WorldState) -> Result<Vec<u8>, CanonicalHashError> {
    let agent_count_u32 =
        u32::try_from(world.agents.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let settlement_count_u32 =
        u32::try_from(world.settlements.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;

    // 1. Canonicalize agents: strictly ascending AgentId
    let mut sorted_agents: Vec<&crate::state::AgentState> = world.agents.iter().collect();
    sorted_agents.sort_by_key(|a| a.agent_id);

    for window in sorted_agents.windows(2) {
        if window[0].agent_id == window[1].agent_id {
            return Err(CanonicalHashError::DuplicateAgent(window[0].agent_id));
        }
    }

    for a in &sorted_agents {
        if !a.health.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("health"));
        }
        if !a.food.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("food"));
        }
        if a.food < 0.0 {
            return Err(CanonicalHashError::NegativeValue("food"));
        }
        if a.wealth < 0 {
            return Err(CanonicalHashError::NegativeMoney(a.wealth));
        }
        if !a.productivity.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("productivity"));
        }
        if !a.cooperation.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("cooperation"));
        }
        if !a.aggression.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("aggression"));
        }
        if !a.risk_tolerance.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("risk_tolerance"));
        }
    }

    // 2. Canonicalize settlements: strictly ascending GroupId
    let mut sorted_settlements: Vec<&crate::state::SettlementState> =
        world.settlements.iter().collect();
    sorted_settlements.sort_by_key(|s| s.group_id);

    for window in sorted_settlements.windows(2) {
        if window[0].group_id == window[1].group_id {
            return Err(CanonicalHashError::DuplicateSettlement(window[0].group_id));
        }
    }

    for s in &sorted_settlements {
        if !s.resource.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("resource"));
        }
        if s.resource < 0.0 {
            return Err(CanonicalHashError::NegativeValue("resource"));
        }
        if s.treasury < 0 {
            return Err(CanonicalHashError::NegativeMoney(s.treasury));
        }
    }

    // 3. Assemble binary payload
    let total_capacity = DOMAIN_STATE.len()
        + 4 // current_day
        + 4 // agent_count
        + (sorted_agents.len() * 43)
        + 4 // settlement_count
        + (sorted_settlements.len() * 14);

    let mut buf = Vec::with_capacity(total_capacity);
    buf.extend_from_slice(DOMAIN_STATE);
    buf.extend_from_slice(&world.current_day.0.to_le_bytes());
    buf.extend_from_slice(&agent_count_u32.to_le_bytes());

    for a in sorted_agents {
        buf.extend_from_slice(&a.agent_id.0.to_le_bytes());
        buf.push(if a.alive { 0x01 } else { 0x00 });
        buf.extend_from_slice(&a.birth_day.0.to_le_bytes());
        buf.extend_from_slice(&a.health.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.food.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.wealth.to_le_bytes());
        buf.extend_from_slice(&a.productivity.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.cooperation.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.aggression.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.risk_tolerance.to_bits().to_le_bytes());
        buf.extend_from_slice(&a.group_id.0.to_le_bytes());
    }

    buf.extend_from_slice(&settlement_count_u32.to_le_bytes());

    for s in sorted_settlements {
        buf.extend_from_slice(&s.group_id.0.to_le_bytes());
        buf.extend_from_slice(&s.resource.to_bits().to_le_bytes());
        buf.extend_from_slice(&s.treasury.to_le_bytes());
    }

    Ok(buf)
}

/// Computes the `CanonicalStateHash` for the given authoritative `WorldState`.
pub fn canonical_state_hash(world: &WorldState) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_state_bytes(world)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M1 canonical state fields followed by the canonical M3 relationship CSR hash.
pub fn canonical_m3_state_bytes(
    world: &WorldState,
    graph: &RelationshipGraph,
) -> Result<Vec<u8>, CanonicalHashError> {
    let m1_bytes = canonical_state_bytes(world)?;
    let fields = &m1_bytes[DOMAIN_STATE.len()..];
    let capacity = DOMAIN_M3_STATE
        .len()
        .checked_add(fields.len())
        .and_then(|n| n.checked_add(32))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3_STATE);
    bytes.extend_from_slice(fields);
    bytes.extend_from_slice(graph.csr_hash());
    Ok(bytes)
}

/// Computes the canonical M3 state digest, including the relationship graph hash.
pub fn canonical_m3_state_hash(
    world: &WorldState,
    graph: &RelationshipGraph,
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3_state_bytes(world, graph)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes canonical M1 state fields, the CSR hash, and the M3P allocator cursor.
pub fn canonical_m3p_state_bytes(state: &M3pReferenceState) -> Result<Vec<u8>, CanonicalHashError> {
    let m3_bytes = canonical_m3_state_bytes(state.world(), state.relationship_graph())?;
    let fields = &m3_bytes[DOMAIN_M3_STATE.len()..];
    let capacity = DOMAIN_M3P_STATE
        .len()
        .checked_add(fields.len())
        .and_then(|len| len.checked_add(8))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3P_STATE);
    bytes.extend_from_slice(fields);
    bytes.extend_from_slice(&state.next_agent_id().to_le_bytes());
    Ok(bytes)
}

/// Computes the M3P State digest.
pub fn canonical_m3p_state_hash(
    state: &M3pReferenceState,
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3p_state_bytes(state)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3 fields and the relationship graph followed by canonical propagation columns.
pub fn canonical_m3prop_state_bytes(
    state: &M3PropagationReferenceState,
) -> Result<Vec<u8>, CanonicalHashError> {
    let base = canonical_m3_state_bytes(state.world(), state.relationship_graph())?;
    let expected_ids: Vec<_> = state
        .world()
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    let mut sorted_ids = expected_ids.clone();
    sorted_ids.sort_unstable();
    if sorted_ids.windows(2).any(|pair| pair[0] == pair[1])
        || state
            .propagation()
            .keys()
            .copied()
            .ne(sorted_ids.iter().copied())
    {
        return Err(CanonicalHashError::InvalidState(
            "M3PROP propagation rows do not match canonical AgentIds".into(),
        ));
    }
    let capacity = DOMAIN_M3PROP_STATE
        .len()
        .checked_add(base.len() - DOMAIN_M3_STATE.len())
        .and_then(|length| length.checked_add(sorted_ids.len().checked_mul(9)?))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3PROP_STATE);
    bytes.extend_from_slice(&base[DOMAIN_M3_STATE.len()..]);
    for agent_id in sorted_ids {
        let propagated = state
            .propagation()
            .get(&agent_id)
            .expect("validated propagation key");
        if !propagated.informed && propagated.informed_day != 0 {
            return Err(CanonicalHashError::InvalidState(
                "uninformed M3PROP row has nonzero informed_day".into(),
            ));
        }
        bytes.extend_from_slice(&agent_id.0.to_le_bytes());
        bytes.push(u8::from(propagated.informed));
        bytes.extend_from_slice(&propagated.informed_day.to_le_bytes());
    }
    Ok(bytes)
}

/// Computes the canonical M3PROP State digest.
pub fn canonical_m3prop_state_hash(
    state: &M3PropagationReferenceState,
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3prop_state_bytes(state)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes authoritative storage state under the M3 domain, followed by the canonical CSR hash.
pub fn canonical_m3_state_bytes_from_storage(
    storage: &SegmentedAgentStorage,
    current_day: SimulationDay,
    settlements: &[crate::state::SettlementState],
    graph: &RelationshipGraph,
) -> Result<Vec<u8>, CanonicalHashError> {
    let m1_bytes = storage.canonical_state_bytes(current_day, settlements)?;
    let fields = &m1_bytes[DOMAIN_STATE.len()..];
    let capacity = DOMAIN_M3_STATE
        .len()
        .checked_add(fields.len())
        .and_then(|n| n.checked_add(32))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3_STATE);
    bytes.extend_from_slice(fields);
    bytes.extend_from_slice(graph.csr_hash());
    Ok(bytes)
}

/// Computes the M3 state digest directly from authoritative segmented storage.
pub fn canonical_m3_state_hash_from_storage(
    storage: &SegmentedAgentStorage,
    current_day: SimulationDay,
    settlements: &[crate::state::SettlementState],
    graph: &RelationshipGraph,
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3_state_bytes_from_storage(storage, current_day, settlements, graph)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3P State from authoritative segmented storage, followed by the allocator cursor.
pub fn canonical_m3p_state_bytes_from_storage(
    storage: &SegmentedAgentStorage,
    current_day: SimulationDay,
    settlements: &[crate::state::SettlementState],
    graph: &RelationshipGraph,
    next_agent_id: u64,
) -> Result<Vec<u8>, CanonicalHashError> {
    let m3_bytes = canonical_m3_state_bytes_from_storage(storage, current_day, settlements, graph)?;
    let fields = &m3_bytes[DOMAIN_M3_STATE.len()..];
    let capacity = DOMAIN_M3P_STATE
        .len()
        .checked_add(fields.len())
        .and_then(|len| len.checked_add(8))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3P_STATE);
    bytes.extend_from_slice(fields);
    bytes.extend_from_slice(&next_agent_id.to_le_bytes());
    Ok(bytes)
}

/// Computes the M3P State digest directly from authoritative segmented storage.
pub fn canonical_m3p_state_hash_from_storage(
    storage: &SegmentedAgentStorage,
    current_day: SimulationDay,
    settlements: &[crate::state::SettlementState],
    graph: &RelationshipGraph,
    next_agent_id: u64,
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3p_state_bytes_from_storage(
        storage,
        current_day,
        settlements,
        graph,
        next_agent_id,
    )?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

// =========================================================================
// 2. CanonicalMetricsHash
// =========================================================================

/// Encodes a time-series of `DailyMetrics` into canonical binary preimage bytes.
///
/// Preimage layout:
/// - `DOMAIN_METRICS` (`"SIMCIV_METRICS_V1"`, 17 bytes)
/// - `record_count`: `u32 LE` (4 bytes)
/// - sorted records in strictly ascending `day` order (36 bytes per record):
///   - `day`: `u32 LE`
///   - `population`: `u64 LE`
///   - `wealth_gini`: `u64 LE` (`to_bits()`)
///   - `total_food_reserves`: `u64 LE` (`to_bits()`)
///   - `total_treasury`: `i64 LE`
pub fn canonical_metrics_bytes(metrics: &[DailyMetrics]) -> Result<Vec<u8>, CanonicalHashError> {
    let count_u32 = u32::try_from(metrics.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;

    // 1. Canonicalize records: strictly ascending day
    let mut sorted_metrics: Vec<&DailyMetrics> = metrics.iter().collect();
    sorted_metrics.sort_by_key(|m| m.day);

    for window in sorted_metrics.windows(2) {
        if window[0].day == window[1].day {
            return Err(CanonicalHashError::DuplicateMetricDay(window[0].day));
        }
    }

    for m in &sorted_metrics {
        if !m.wealth_gini.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("wealth_gini"));
        }
        if !(0.0..=1.0).contains(&m.wealth_gini) {
            return Err(CanonicalHashError::InvalidState(format!(
                "wealth_gini outside [0, 1]: {}",
                m.wealth_gini
            )));
        }
        if !m.total_food_reserves.is_finite() {
            return Err(CanonicalHashError::NonFiniteFloat("total_food_reserves"));
        }
        if m.total_food_reserves < 0.0 {
            return Err(CanonicalHashError::NegativeValue("total_food_reserves"));
        }
        if m.total_treasury < 0 {
            return Err(CanonicalHashError::NegativeMoney(m.total_treasury));
        }
    }

    // 2. Assemble binary payload
    let total_capacity = DOMAIN_METRICS.len() + 4 + (sorted_metrics.len() * 36);
    let mut buf = Vec::with_capacity(total_capacity);
    buf.extend_from_slice(DOMAIN_METRICS);
    buf.extend_from_slice(&count_u32.to_le_bytes());

    for m in sorted_metrics {
        buf.extend_from_slice(&m.day.to_le_bytes());
        buf.extend_from_slice(&m.population.to_le_bytes());
        buf.extend_from_slice(&m.wealth_gini.to_bits().to_le_bytes());
        buf.extend_from_slice(&m.total_food_reserves.to_bits().to_le_bytes());
        buf.extend_from_slice(&m.total_treasury.to_le_bytes());
    }

    Ok(buf)
}

/// Computes the `CanonicalMetricsHash` for the given macro metrics time-series.
pub fn canonical_metrics_hash(
    metrics: &[DailyMetrics],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_metrics_bytes(metrics)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3 metric records in M1 day order with the frozen M3 counters appended.
pub fn canonical_m3_metrics_bytes(
    metrics: &[M3DailyMetrics],
) -> Result<Vec<u8>, CanonicalHashError> {
    let count = u32::try_from(metrics.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let mut sorted: Vec<&M3DailyMetrics> = metrics.iter().collect();
    sorted.sort_by_key(|metric| metric.metrics.day);
    for window in sorted.windows(2) {
        if window[0].metrics.day == window[1].metrics.day {
            return Err(CanonicalHashError::DuplicateMetricDay(
                window[0].metrics.day,
            ));
        }
    }

    let m1_metrics: Vec<DailyMetrics> =
        sorted.iter().map(|metric| metric.metrics.clone()).collect();
    let m1_bytes = canonical_metrics_bytes(&m1_metrics)?;
    let capacity = DOMAIN_M3_METRICS
        .len()
        .checked_add(4)
        .and_then(|n| n.checked_add(sorted.len().checked_mul(60)?))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3_METRICS);
    bytes.extend_from_slice(&count.to_le_bytes());
    for (index, metric) in sorted.into_iter().enumerate() {
        let start = index
            .checked_mul(36)
            .and_then(|offset| DOMAIN_METRICS.len().checked_add(4)?.checked_add(offset))
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        let end = start
            .checked_add(36)
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        bytes.extend_from_slice(&m1_bytes[start..end]);
        bytes.extend_from_slice(&metric.cross_partition_edge_count.to_le_bytes());
        bytes.extend_from_slice(&metric.total_directed_edge_count.to_le_bytes());
        bytes.extend_from_slice(&metric.cross_partition_interaction_count.to_le_bytes());
    }
    Ok(bytes)
}

/// Computes the canonical M3 metrics digest.
pub fn canonical_m3_metrics_hash(
    metrics: &[M3DailyMetrics],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3_metrics_bytes(metrics)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3P daily metrics in M1/M3 day order with population fields appended.
pub fn canonical_m3p_metrics_bytes(
    metrics: &[M3pDailyMetrics],
) -> Result<Vec<u8>, CanonicalHashError> {
    let count = u32::try_from(metrics.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let mut sorted: Vec<&M3pDailyMetrics> = metrics.iter().collect();
    sorted.sort_by_key(|metric| metric.m3_metrics.metrics.day);
    for window in sorted.windows(2) {
        if window[0].m3_metrics.metrics.day == window[1].m3_metrics.metrics.day {
            return Err(CanonicalHashError::DuplicateMetricDay(
                window[0].m3_metrics.metrics.day,
            ));
        }
    }

    let m3_metrics: Vec<M3DailyMetrics> = sorted
        .iter()
        .map(|metric| metric.m3_metrics.clone())
        .collect();
    let m3_bytes = canonical_m3_metrics_bytes(&m3_metrics)?;
    let capacity = DOMAIN_M3P_METRICS
        .len()
        .checked_add(4)
        .and_then(|len| len.checked_add(sorted.len().checked_mul(100)?))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3P_METRICS);
    bytes.extend_from_slice(&count.to_le_bytes());
    for (index, metric) in sorted.into_iter().enumerate() {
        let start = DOMAIN_M3_METRICS
            .len()
            .checked_add(4)
            .and_then(|offset| offset.checked_add(index.checked_mul(60)?))
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        let end = start
            .checked_add(60)
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        bytes.extend_from_slice(&m3_bytes[start..end]);
        bytes.extend_from_slice(&metric.births.to_le_bytes());
        bytes.extend_from_slice(&metric.removed.to_le_bytes());
        bytes.extend_from_slice(&metric.replaced_edges.to_le_bytes());
        bytes.extend_from_slice(&metric.live_population.to_le_bytes());
        bytes.extend_from_slice(&metric.next_agent_id.to_le_bytes());
    }
    Ok(bytes)
}

/// Computes the canonical M3P Metrics digest.
pub fn canonical_m3p_metrics_hash(
    metrics: &[M3pDailyMetrics],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3p_metrics_bytes(metrics)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3 metrics in day order followed by the six propagation counters.
pub fn canonical_m3prop_metrics_bytes(
    metrics: &[M3PropDailyMetrics],
) -> Result<Vec<u8>, CanonicalHashError> {
    let count = u32::try_from(metrics.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let mut sorted: Vec<&M3PropDailyMetrics> = metrics.iter().collect();
    sorted.sort_by_key(|metric| metric.m3_metrics.metrics.day);
    for window in sorted.windows(2) {
        if window[0].m3_metrics.metrics.day == window[1].m3_metrics.metrics.day {
            return Err(CanonicalHashError::DuplicateMetricDay(
                window[0].m3_metrics.metrics.day,
            ));
        }
    }
    let base: Vec<_> = sorted
        .iter()
        .map(|metric| metric.m3_metrics.clone())
        .collect();
    let base_bytes = canonical_m3_metrics_bytes(&base)?;
    let capacity = DOMAIN_M3PROP_METRICS
        .len()
        .checked_add(4)
        .and_then(|length| length.checked_add(sorted.len().checked_mul(108)?))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3PROP_METRICS);
    bytes.extend_from_slice(&count.to_le_bytes());
    for (index, metric) in sorted.into_iter().enumerate() {
        let start = DOMAIN_M3_METRICS
            .len()
            .checked_add(4)
            .and_then(|offset| offset.checked_add(index.checked_mul(60)?))
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        let end = start
            .checked_add(60)
            .ok_or(CanonicalHashError::ArithmeticOverflow)?;
        bytes.extend_from_slice(&base_bytes[start..end]);
        for value in [
            metric.propagation.informed_count,
            metric.propagation.newly_informed,
            metric.propagation.forgotten,
            metric.propagation.seeded,
            metric.propagation.levels_used,
            metric.propagation.max_frontier_size,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

/// Computes the canonical M3PROP Metrics digest.
pub fn canonical_m3prop_metrics_hash(
    metrics: &[M3PropDailyMetrics],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3prop_metrics_bytes(metrics)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

// =========================================================================
// 3. CanonicalEventHash
// =========================================================================

/// Fixed canonical category tags for Contract C07 events.
pub const EVENT_CATEGORY_STATE_TRANSITION: u8 = 0x00;
pub const EVENT_CATEGORY_OBSERVATION: u8 = 0x01;

/// Fixed canonical variant tags for StateTransition events.
pub const STATE_EVENT_WORK_RESOLVED: u8 = 0x00;
pub const STATE_EVENT_FOOD_TRANSFERRED: u8 = 0x01;
pub const STATE_EVENT_MARKET_CLEARED: u8 = 0x02;
pub const STATE_EVENT_WELFARE_DISTRIBUTED: u8 = 0x03;
pub const STATE_EVENT_MORTALITY_COMMITTED: u8 = 0x04;

/// Fixed canonical variant tags for Observation events.
pub const OBSERVATION_EVENT_DAILY_METRICS: u8 = 0x00;
pub const OBSERVATION_EVENT_SNAPSHOT_EMITTED: u8 = 0x01;

/// Encodes a stream of `EventRecord`s into canonical binary preimage bytes.
///
/// Preimage layout:
/// - `DOMAIN_EVENTS` (`"SIMCIV_EVENTS_V1"`, 16 bytes)
/// - `record_count`: `u32 LE` (4 bytes)
/// - for each record in strictly ascending `EventKey` order:
///   - `key.day`: `u32 LE` (4 bytes)
///   - `key.phase`: `u8` (1 byte)
///   - `key.partition_key`: `u64 LE` (8 bytes)
///   - `key.local_sequence`: `u64 LE` (8 bytes)
///   - `event_category_tag`: `u8` (1 byte)
///   - `event_variant_tag`: `u8` (1 byte)
///   - variant payload...
pub fn canonical_event_bytes(events: &[EventRecord]) -> Result<Vec<u8>, CanonicalHashError> {
    let count_u32 = u32::try_from(events.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;

    // 1. Canonicalize events: strictly ascending EventKey
    let mut sorted_events: Vec<&EventRecord> = events.iter().collect();
    sorted_events.sort_by_key(|e| e.key);

    for window in sorted_events.windows(2) {
        if window[0].key == window[1].key {
            return Err(CanonicalHashError::DuplicateEventKey(window[0].key));
        }
    }

    // 2. Validate all records
    for r in &sorted_events {
        if r.key.phase < 1 || r.key.phase > 11 {
            return Err(CanonicalHashError::InvalidPhase(r.key.phase));
        }

        match &r.event {
            Event::StateTransition(st) => match st {
                StateTransitionEvent::WorkResolved {
                    requested_harvest,
                    allocated_harvest,
                    ..
                } => {
                    if !requested_harvest.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("requested_harvest"));
                    }
                    if !allocated_harvest.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("allocated_harvest"));
                    }
                    if *requested_harvest < 0.0 || *allocated_harvest < 0.0 {
                        return Err(CanonicalHashError::NegativeValue("harvest"));
                    }
                }
                StateTransitionEvent::FoodTransferred { amount, .. } => {
                    if !amount.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("amount"));
                    }
                    if *amount < 0.0 {
                        return Err(CanonicalHashError::NegativeValue("amount"));
                    }
                }
                StateTransitionEvent::MarketCleared {
                    food_price,
                    tax_rate,
                    total_effective_supply,
                    total_effective_demand,
                    total_sold,
                    total_revenue,
                    tax_withheld,
                    net_pool_proceeds,
                    ..
                } => {
                    if !tax_rate.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("tax_rate"));
                    }
                    if !total_effective_supply.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("total_effective_supply"));
                    }
                    if !total_effective_demand.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("total_effective_demand"));
                    }
                    if !total_sold.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("total_sold"));
                    }
                    if *food_price < 0 {
                        return Err(CanonicalHashError::NegativeMoney(*food_price));
                    }
                    if *total_revenue < 0 {
                        return Err(CanonicalHashError::NegativeMoney(*total_revenue));
                    }
                    if *tax_withheld < 0 {
                        return Err(CanonicalHashError::NegativeMoney(*tax_withheld));
                    }
                    if *net_pool_proceeds < 0 {
                        return Err(CanonicalHashError::NegativeMoney(*net_pool_proceeds));
                    }
                    // Note: proceeds_balance is signed (negative/zero/positive all permitted)
                }
                StateTransitionEvent::WelfareDistributed { payout, .. } => {
                    if *payout < 0 {
                        return Err(CanonicalHashError::NegativeMoney(*payout));
                    }
                }
                StateTransitionEvent::MortalityCommitted { .. } => {}
            },
            Event::Observation(obs) => match obs {
                ObservationEvent::DailyMetricsObserved(m) => {
                    if !m.wealth_gini.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("wealth_gini"));
                    }
                    if !(0.0..=1.0).contains(&m.wealth_gini) {
                        return Err(CanonicalHashError::InvalidState(format!(
                            "wealth_gini outside [0, 1]: {}",
                            m.wealth_gini
                        )));
                    }
                    if !m.total_food_reserves.is_finite() {
                        return Err(CanonicalHashError::NonFiniteFloat("total_food_reserves"));
                    }
                    if m.total_food_reserves < 0.0 {
                        return Err(CanonicalHashError::NegativeValue("total_food_reserves"));
                    }
                    if m.total_treasury < 0 {
                        return Err(CanonicalHashError::NegativeMoney(m.total_treasury));
                    }
                }
                ObservationEvent::SnapshotEmitted { .. } => {}
            },
        }
    }

    // 3. Assemble binary payload
    let mut buf = Vec::with_capacity(DOMAIN_EVENTS.len() + 4 + sorted_events.len() * 32);
    buf.extend_from_slice(DOMAIN_EVENTS);
    buf.extend_from_slice(&count_u32.to_le_bytes());

    for r in sorted_events {
        // Canonical key (21 bytes)
        buf.extend_from_slice(&r.key.day.to_le_bytes());
        buf.push(r.key.phase);
        buf.extend_from_slice(&r.key.partition_key.to_le_bytes());
        buf.extend_from_slice(&r.key.local_sequence.to_le_bytes());

        match &r.event {
            Event::StateTransition(st) => {
                buf.push(EVENT_CATEGORY_STATE_TRANSITION);
                match st {
                    StateTransitionEvent::WorkResolved {
                        group_id,
                        agent_id,
                        requested_harvest,
                        allocated_harvest,
                    } => {
                        buf.push(STATE_EVENT_WORK_RESOLVED);
                        buf.extend_from_slice(&group_id.0.to_le_bytes());
                        buf.extend_from_slice(&agent_id.0.to_le_bytes());
                        buf.extend_from_slice(&requested_harvest.to_bits().to_le_bytes());
                        buf.extend_from_slice(&allocated_harvest.to_bits().to_le_bytes());
                    }
                    StateTransitionEvent::FoodTransferred {
                        group_id,
                        initiator_agent_id,
                        target_agent_id,
                        action_kind,
                        amount,
                        success,
                    } => {
                        buf.push(STATE_EVENT_FOOD_TRANSFERRED);
                        buf.extend_from_slice(&group_id.0.to_le_bytes());
                        buf.extend_from_slice(&initiator_agent_id.0.to_le_bytes());
                        match target_agent_id {
                            None => buf.push(0x00),
                            Some(tid) => {
                                buf.push(0x01);
                                buf.extend_from_slice(&tid.0.to_le_bytes());
                            }
                        }
                        let action_code = match action_kind {
                            TargetedActionKind::GiveFood => 3u8,
                            TargetedActionKind::StealFood => 4u8,
                        };
                        buf.push(action_code);
                        buf.extend_from_slice(&amount.to_bits().to_le_bytes());
                        buf.push(if *success { 0x01 } else { 0x00 });
                    }
                    StateTransitionEvent::MarketCleared {
                        group_id,
                        food_price,
                        tax_rate,
                        total_effective_supply,
                        total_effective_demand,
                        total_sold,
                        total_revenue,
                        tax_withheld,
                        net_pool_proceeds,
                        proceeds_balance,
                        buyers_count,
                        sellers_count,
                    } => {
                        buf.push(STATE_EVENT_MARKET_CLEARED);
                        buf.extend_from_slice(&group_id.0.to_le_bytes());
                        buf.extend_from_slice(&food_price.to_le_bytes());
                        buf.extend_from_slice(&tax_rate.to_bits().to_le_bytes());
                        buf.extend_from_slice(&total_effective_supply.to_bits().to_le_bytes());
                        buf.extend_from_slice(&total_effective_demand.to_bits().to_le_bytes());
                        buf.extend_from_slice(&total_sold.to_bits().to_le_bytes());
                        buf.extend_from_slice(&total_revenue.to_le_bytes());
                        buf.extend_from_slice(&tax_withheld.to_le_bytes());
                        buf.extend_from_slice(&net_pool_proceeds.to_le_bytes());
                        buf.extend_from_slice(&proceeds_balance.to_le_bytes());
                        let buyers_u64 = u64::try_from(*buyers_count)
                            .map_err(|_| CanonicalHashError::LengthOverflow)?;
                        let sellers_u64 = u64::try_from(*sellers_count)
                            .map_err(|_| CanonicalHashError::LengthOverflow)?;
                        buf.extend_from_slice(&buyers_u64.to_le_bytes());
                        buf.extend_from_slice(&sellers_u64.to_le_bytes());
                    }
                    StateTransitionEvent::WelfareDistributed {
                        group_id,
                        agent_id,
                        payout,
                    } => {
                        buf.push(STATE_EVENT_WELFARE_DISTRIBUTED);
                        buf.extend_from_slice(&group_id.0.to_le_bytes());
                        buf.extend_from_slice(&agent_id.0.to_le_bytes());
                        buf.extend_from_slice(&payout.to_le_bytes());
                    }
                    StateTransitionEvent::MortalityCommitted { agent_id } => {
                        buf.push(STATE_EVENT_MORTALITY_COMMITTED);
                        buf.extend_from_slice(&agent_id.0.to_le_bytes());
                    }
                }
            }
            Event::Observation(obs) => {
                buf.push(EVENT_CATEGORY_OBSERVATION);
                match obs {
                    ObservationEvent::DailyMetricsObserved(m) => {
                        buf.push(OBSERVATION_EVENT_DAILY_METRICS);
                        buf.extend_from_slice(&m.day.to_le_bytes());
                        buf.extend_from_slice(&m.population.to_le_bytes());
                        buf.extend_from_slice(&m.wealth_gini.to_bits().to_le_bytes());
                        buf.extend_from_slice(&m.total_food_reserves.to_bits().to_le_bytes());
                        buf.extend_from_slice(&m.total_treasury.to_le_bytes());
                    }
                    ObservationEvent::SnapshotEmitted {
                        day,
                        master_seed,
                        replicate_id,
                        schema_version,
                    } => {
                        buf.push(OBSERVATION_EVENT_SNAPSHOT_EMITTED);
                        buf.extend_from_slice(&day.to_le_bytes());
                        buf.extend_from_slice(&master_seed.to_le_bytes());
                        buf.extend_from_slice(&replicate_id.to_le_bytes());
                        buf.extend_from_slice(&schema_version.to_le_bytes());
                    }
                }
            }
        }
    }

    Ok(buf)
}

/// Computes the `CanonicalEventHash` for the given event slice.
pub fn canonical_event_hash(events: &[EventRecord]) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_event_bytes(events)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes M3P events, reusing the frozen payload encoding for existing events.
pub fn canonical_m3p_event_bytes(events: &[M3pEventRecord]) -> Result<Vec<u8>, CanonicalHashError> {
    let count = u32::try_from(events.len()).map_err(|_| CanonicalHashError::LengthOverflow)?;
    let mut sorted: Vec<&M3pEventRecord> = events.iter().collect();
    sorted.sort_by_key(|record| record.key);
    for window in sorted.windows(2) {
        if window[0].key == window[1].key {
            return Err(CanonicalHashError::DuplicateEventKey(window[0].key));
        }
    }

    let capacity = DOMAIN_M3P_EVENTS
        .len()
        .checked_add(4)
        .and_then(|len| len.checked_add(sorted.len().checked_mul(64)?))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3P_EVENTS);
    bytes.extend_from_slice(&count.to_le_bytes());

    for record in sorted {
        if !(1..=11).contains(&record.key.phase) {
            return Err(CanonicalHashError::InvalidPhase(record.key.phase));
        }
        bytes.extend_from_slice(&record.key.day.to_le_bytes());
        bytes.push(record.key.phase);
        bytes.extend_from_slice(&record.key.partition_key.to_le_bytes());
        bytes.extend_from_slice(&record.key.local_sequence.to_le_bytes());
        match &record.event {
            M3pEvent::Existing(event) => {
                let encoded =
                    canonical_event_bytes(&[EventRecord::new(record.key, event.clone())])?;
                let event_body_start = DOMAIN_EVENTS
                    .len()
                    .checked_add(4)
                    .and_then(|offset| offset.checked_add(CANONICAL_EVENT_KEY_BYTES))
                    .ok_or(CanonicalHashError::ArithmeticOverflow)?;
                bytes.extend_from_slice(&encoded[event_body_start..]);
            }
            M3pEvent::Birth {
                parent,
                child,
                group,
            } => {
                bytes.push(EVENT_CATEGORY_STATE_TRANSITION);
                bytes.push(M3P_EVENT_BIRTH);
                bytes.extend_from_slice(&parent.0.to_le_bytes());
                bytes.extend_from_slice(&child.0.to_le_bytes());
                bytes.extend_from_slice(&group.0.to_le_bytes());
            }
            M3pEvent::Cleanup {
                removed,
                replaced_edges,
                dropped_edges,
            } => {
                bytes.push(EVENT_CATEGORY_STATE_TRANSITION);
                bytes.push(M3P_EVENT_CLEANUP);
                bytes.extend_from_slice(&removed.to_le_bytes());
                bytes.extend_from_slice(&replaced_edges.to_le_bytes());
                bytes.extend_from_slice(&dropped_edges.to_le_bytes());
            }
        }
    }
    Ok(bytes)
}

/// Computes the canonical M3P Events digest.
pub fn canonical_m3p_event_hash(
    events: &[M3pEventRecord],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3p_event_bytes(events)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}

/// Encodes the existing canonical event stream under the M3PROP event domain.
pub fn canonical_m3prop_event_bytes(events: &[EventRecord]) -> Result<Vec<u8>, CanonicalHashError> {
    let base = canonical_event_bytes(events)?;
    let capacity = DOMAIN_M3PROP_EVENTS
        .len()
        .checked_add(base.len().saturating_sub(DOMAIN_EVENTS.len()))
        .ok_or(CanonicalHashError::ArithmeticOverflow)?;
    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(DOMAIN_M3PROP_EVENTS);
    bytes.extend_from_slice(&base[DOMAIN_EVENTS.len()..]);
    Ok(bytes)
}

/// Computes the canonical M3PROP Events digest.
pub fn canonical_m3prop_event_hash(
    events: &[EventRecord],
) -> Result<CanonicalHash, CanonicalHashError> {
    let bytes = canonical_m3prop_event_bytes(events)?;
    Ok(CanonicalHash(Sha256::digest(&bytes).into()))
}
