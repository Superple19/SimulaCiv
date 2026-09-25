//! Canonical event stream and buffer implementation (Contract C07 / Phase 11).
//!
//! Provides deterministic event representation, telemetry buffering, canonical
//! sorting, and Phase 11 flushing for SimulaCiv M0.

use crate::metrics::DailyMetrics;
use crate::phases::Phase9MortalityResolution;
use crate::resolution::{
    SettlementMarketResolution, SettlementTargetedResolution, SettlementWelfareResolution,
    SettlementWorkResolution, TargetedActionKind, TargetedOutcome,
};
use crate::snapshot::SnapshotMetadata;
use serde::{Deserialize, Serialize};
use sim_core::{AgentId, GroupId, Money};
use std::collections::HashSet;

/// Partition key value assigned to global, non-settlement-local observations.
pub const GLOBAL_PARTITION_KEY: u64 = u64::MAX;

/// Converts a settlement `GroupId` to its canonical scalar partition key.
#[inline]
pub const fn partition_key_from_group(group_id: GroupId) -> u64 {
    group_id.0 as u64
}

/// Explicit canonical key defining deterministic total ordering over simulation events.
///
/// Order: `day -> phase -> partition_key -> local_sequence`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventKey {
    pub day: u32,
    pub phase: u8,
    pub partition_key: u64,
    pub local_sequence: u64,
}

impl EventKey {
    #[inline]
    pub const fn new(day: u32, phase: u8, partition_key: u64, local_sequence: u64) -> Self {
        Self {
            day,
            phase,
            partition_key,
            local_sequence,
        }
    }
}

/// Category 1: Authoritative state transitions committed by simulation phases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StateTransitionEvent {
    WorkResolved {
        group_id: GroupId,
        agent_id: AgentId,
        requested_harvest: f32,
        allocated_harvest: f32,
    },
    FoodTransferred {
        group_id: GroupId,
        initiator_agent_id: AgentId,
        target_agent_id: Option<AgentId>,
        action_kind: TargetedActionKind,
        amount: f32,
        success: bool,
    },
    MarketCleared {
        group_id: GroupId,
        food_price: Money,
        tax_rate: f32,
        total_effective_supply: f32,
        total_effective_demand: f32,
        total_sold: f32,
        total_revenue: Money,
        tax_withheld: Money,
        net_pool_proceeds: Money,
        proceeds_balance: Money,
        buyers_count: usize,
        sellers_count: usize,
    },
    WelfareDistributed {
        group_id: GroupId,
        agent_id: AgentId,
        payout: Money,
    },
    MortalityCommitted {
        agent_id: AgentId,
    },
}

/// Category 2: Non-mutating macroscopic system observations and artifact emissions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObservationEvent {
    DailyMetricsObserved(DailyMetrics),
    SnapshotEmitted {
        day: u32,
        master_seed: u64,
        replicate_id: u32,
        schema_version: u32,
    },
}

/// Top-level event payload enum.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    StateTransition(StateTransitionEvent),
    Observation(ObservationEvent),
}

/// Canonical event envelope containing a deterministic key and event payload.
///
/// Records are immutable and exclude volatile execution metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventRecord {
    pub key: EventKey,
    pub event: Event,
}

impl EventRecord {
    #[inline]
    pub const fn new(key: EventKey, event: Event) -> Self {
        Self { key, event }
    }
}

/// Telemetry buffer for staging uncommitted event records prior to Phase 11 flush.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventBuffer {
    pending: Vec<EventRecord>,
}

impl EventBuffer {
    #[inline]
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            pending: Vec::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn push(&mut self, record: EventRecord) {
        self.pending.push(record);
    }

    #[inline]
    pub fn push_all(&mut self, records: impl IntoIterator<Item = EventRecord>) {
        self.pending.extend(records);
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    #[inline]
    pub fn pending_records(&self) -> &[EventRecord] {
        &self.pending
    }
}

/// Errors returned during event validation and Phase 11 canonical flush.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    InvalidPhase(u8),
    DuplicateKey(EventKey),
    NonFiniteFloat(&'static str),
    NegativeMoney(Money),
    InvalidState(String),
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPhase(p) => write!(f, "invalid event phase: {} (must be 1..=11)", p),
            Self::DuplicateKey(k) => write!(
                f,
                "duplicate event key detected: (day={}, phase={}, partition={}, seq={})",
                k.day, k.phase, k.partition_key, k.local_sequence
            ),
            Self::NonFiniteFloat(name) => write!(f, "non-finite float in event field '{}'", name),
            Self::NegativeMoney(m) => write!(f, "negative money amount in event: {}", m),
            Self::InvalidState(msg) => write!(f, "invalid event state: {}", msg),
        }
    }
}

impl std::error::Error for EventError {}

/// Executes Phase 11 canonical event flush:
/// 1. Validates all pending records (phase bounds, finite floats, non-negative money).
/// 2. Detects duplicate complete `EventKey`s.
/// 3. Sorts records in canonical ascending `EventKey` order.
/// 4. Empties the pending buffer atomically on success.
///
/// Invariants:
/// - If validation fails or duplicate keys exist, `buffer` remains completely unchanged.
/// - Successful flush empties `buffer.pending`.
/// - Flushed records are ordered canonically regardless of insertion order.
pub fn phase11_flush_events(buffer: &mut EventBuffer) -> Result<Vec<EventRecord>, EventError> {
    // 1. Validate all records and detect duplicate keys without mutating buffer
    let mut seen_keys = HashSet::with_capacity(buffer.pending.len());

    for record in &buffer.pending {
        if record.key.phase < 1 || record.key.phase > 11 {
            return Err(EventError::InvalidPhase(record.key.phase));
        }

        if !seen_keys.insert(record.key) {
            return Err(EventError::DuplicateKey(record.key));
        }

        match &record.event {
            Event::StateTransition(st) => match st {
                StateTransitionEvent::WorkResolved {
                    requested_harvest,
                    allocated_harvest,
                    ..
                } => {
                    if !requested_harvest.is_finite() {
                        return Err(EventError::NonFiniteFloat("requested_harvest"));
                    }
                    if !allocated_harvest.is_finite() {
                        return Err(EventError::NonFiniteFloat("allocated_harvest"));
                    }
                    if *requested_harvest < 0.0 || *allocated_harvest < 0.0 {
                        return Err(EventError::InvalidState(
                            "work harvest amount cannot be negative".to_string(),
                        ));
                    }
                }
                StateTransitionEvent::FoodTransferred { amount, .. } => {
                    if !amount.is_finite() {
                        return Err(EventError::NonFiniteFloat("amount"));
                    }
                    if *amount < 0.0 {
                        return Err(EventError::InvalidState(
                            "food transfer amount cannot be negative".to_string(),
                        ));
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
                        return Err(EventError::NonFiniteFloat("tax_rate"));
                    }
                    if !total_effective_supply.is_finite() {
                        return Err(EventError::NonFiniteFloat("total_effective_supply"));
                    }
                    if !total_effective_demand.is_finite() {
                        return Err(EventError::NonFiniteFloat("total_effective_demand"));
                    }
                    if !total_sold.is_finite() {
                        return Err(EventError::NonFiniteFloat("total_sold"));
                    }
                    if *food_price < 0 {
                        return Err(EventError::NegativeMoney(*food_price));
                    }
                    if *total_revenue < 0 {
                        return Err(EventError::NegativeMoney(*total_revenue));
                    }
                    if *tax_withheld < 0 {
                        return Err(EventError::NegativeMoney(*tax_withheld));
                    }
                    if *net_pool_proceeds < 0 {
                        return Err(EventError::NegativeMoney(*net_pool_proceeds));
                    }
                }
                StateTransitionEvent::WelfareDistributed { payout, .. } => {
                    if *payout < 0 {
                        return Err(EventError::NegativeMoney(*payout));
                    }
                }
                StateTransitionEvent::MortalityCommitted { .. } => {}
            },
            Event::Observation(obs) => match obs {
                ObservationEvent::DailyMetricsObserved(m) => {
                    if !m.wealth_gini.is_finite() {
                        return Err(EventError::NonFiniteFloat("wealth_gini"));
                    }
                    if !m.total_food_reserves.is_finite() {
                        return Err(EventError::NonFiniteFloat("total_food_reserves"));
                    }
                    if m.total_treasury < 0 {
                        return Err(EventError::NegativeMoney(m.total_treasury));
                    }
                }
                ObservationEvent::SnapshotEmitted { .. } => {}
            },
        }
    }

    // 2. All validations passed; atomically drain and sort
    let mut flushed = std::mem::take(&mut buffer.pending);
    flushed.sort_by_key(|r| r.key);

    Ok(flushed)
}

// =========================================================================
// Provenance Adapters
// =========================================================================

/// Constructs a canonical Phase 7 MarketCleared event from a completed settlement market resolution.
pub fn events_from_market_resolution(
    day: u32,
    resolution: &SettlementMarketResolution,
) -> Vec<EventRecord> {
    vec![EventRecord {
        key: EventKey::new(day, 7, partition_key_from_group(resolution.group_id), 0),
        event: Event::StateTransition(StateTransitionEvent::MarketCleared {
            group_id: resolution.group_id,
            food_price: resolution.food_price,
            tax_rate: resolution.tax_rate,
            total_effective_supply: resolution.total_effective_supply,
            total_effective_demand: resolution.total_effective_demand,
            total_sold: resolution.total_sold,
            total_revenue: resolution.total_revenue,
            tax_withheld: resolution.tax_withheld,
            net_pool_proceeds: resolution.net_pool_proceeds,
            proceeds_balance: resolution.proceeds_balance,
            buyers_count: resolution.buyers.len(),
            sellers_count: resolution.sellers.len(),
        }),
    }]
}

/// Constructs canonical Phase 8 WelfareDistributed events from a completed welfare resolution.
///
/// Recipient events are sequenced canonically by strictly ascending `AgentId`.
pub fn events_from_welfare_resolution(
    day: u32,
    resolution: &SettlementWelfareResolution,
) -> Vec<EventRecord> {
    let mut sorted_recipients = resolution.recipients.clone();
    sorted_recipients.sort_by_key(|r| r.agent_id);

    sorted_recipients
        .into_iter()
        .enumerate()
        .map(|(seq, r)| EventRecord {
            key: EventKey::new(
                day,
                8,
                partition_key_from_group(resolution.group_id),
                seq as u64,
            ),
            event: Event::StateTransition(StateTransitionEvent::WelfareDistributed {
                group_id: resolution.group_id,
                agent_id: r.agent_id,
                payout: r.payout,
            }),
        })
        .collect()
}

/// Constructs canonical Phase 9 MortalityCommitted events from a completed mortality resolution.
///
/// Events are sequenced canonically by strictly ascending `AgentId` on `GLOBAL_PARTITION_KEY`.
pub fn events_from_mortality_resolution(
    day: u32,
    resolution: &Phase9MortalityResolution,
) -> Vec<EventRecord> {
    let mut sorted_deceased = resolution.newly_deceased.clone();
    sorted_deceased.sort();

    sorted_deceased
        .into_iter()
        .enumerate()
        .map(|(seq, agent_id)| EventRecord {
            key: EventKey::new(day, 9, GLOBAL_PARTITION_KEY, seq as u64),
            event: Event::StateTransition(StateTransitionEvent::MortalityCommitted { agent_id }),
        })
        .collect()
}

/// Constructs a canonical Phase 10 DailyMetricsObserved observation event.
pub fn event_from_daily_metrics(metrics: &DailyMetrics) -> EventRecord {
    EventRecord {
        key: EventKey::new(metrics.day, 10, GLOBAL_PARTITION_KEY, 0),
        event: Event::Observation(ObservationEvent::DailyMetricsObserved(metrics.clone())),
    }
}

/// Constructs a canonical Phase 11 SnapshotEmitted observation event.
pub fn event_from_snapshot_emission(
    metadata: &SnapshotMetadata,
    schema_version: u32,
) -> EventRecord {
    EventRecord {
        key: EventKey::new(metadata.day, 11, GLOBAL_PARTITION_KEY, 0),
        event: Event::Observation(ObservationEvent::SnapshotEmitted {
            day: metadata.day,
            master_seed: metadata.master_seed,
            replicate_id: metadata.replicate_id,
            schema_version,
        }),
    }
}

/// Constructs canonical Phase 6A WorkResolved events from a completed work resolution.
pub fn events_from_work_resolution(
    day: u32,
    resolution: &SettlementWorkResolution,
) -> Vec<EventRecord> {
    let mut sorted_allocations = resolution.allocations.clone();
    sorted_allocations.sort_by_key(|a| a.agent_id);

    sorted_allocations
        .into_iter()
        .enumerate()
        .map(|(seq, a)| EventRecord {
            key: EventKey::new(
                day,
                6,
                partition_key_from_group(resolution.group_id),
                seq as u64,
            ),
            event: Event::StateTransition(StateTransitionEvent::WorkResolved {
                group_id: resolution.group_id,
                agent_id: a.agent_id,
                requested_harvest: a.requested_harvest,
                allocated_harvest: a.allocated_harvest,
            }),
        })
        .collect()
}

/// Constructs canonical Phase 6B FoodTransferred events from a completed targeted resolution.
pub fn events_from_targeted_resolution(
    day: u32,
    resolution: &SettlementTargetedResolution,
) -> Vec<EventRecord> {
    resolution
        .resolutions
        .iter()
        .enumerate()
        .map(|(seq, r)| {
            let (amount, success) = match r.outcome {
                TargetedOutcome::Applied { amount } => (amount, true),
                TargetedOutcome::TheftFailed
                | TargetedOutcome::ZeroTarget
                | TargetedOutcome::InitiatorIneligible
                | TargetedOutcome::TargetIneligible => (0.0, false),
            };

            EventRecord {
                key: EventKey::new(
                    day,
                    6,
                    partition_key_from_group(resolution.group_id),
                    seq as u64,
                ),
                event: Event::StateTransition(StateTransitionEvent::FoodTransferred {
                    group_id: resolution.group_id,
                    initiator_agent_id: r.initiator_agent_id,
                    target_agent_id: r.target_agent_id,
                    action_kind: r.action_kind,
                    amount,
                    success,
                }),
            }
        })
        .collect()
}
