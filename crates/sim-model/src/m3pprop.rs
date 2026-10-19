//! M3P population dynamics with the frozen M3 propagation overlay.

use crate::config::{
    PropagationConfig, SimConfig, SimulationConfig, SimulationProfile, SimulationSystems,
};
use crate::counterfactual::{ForcedActionDirective, PhaseComponentHashes, hash_propagation_rows};
use crate::events::{Event, EventKey, GLOBAL_PARTITION_KEY, ObservationEvent};
use crate::hashing::{
    CanonicalHash, CanonicalHashError, canonical_m3pprop_metrics_hash, canonical_m3pprop_state_hash,
};
#[cfg(test)]
use crate::m3p::run_m3p_reference_day_with_post_phase9b;
use crate::m3p::{
    M3pEvent, M3pEventRecord, M3pReferenceRecorderOverlay, M3pReferenceState, decode_m3p_snapshot,
    encode_m3p_snapshot, run_m3p_reference_day_with_post_phase9c_recorder,
    run_m3p_reference_day_with_post_phase9c_registered,
};
use crate::m3prop::M3PROP_SNAPSHOT_SCHEMA_VERSION;
use crate::propagation::{PropagationAgentState, PropagationDailyCounts, apply_phase9c_m3p};
use crate::relationship_graph::RelationshipGraph;
use crate::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext, M0RunError,
};
use crate::snapshot::{CanonicalSnapshot, SNAPSHOT_MAGIC, SnapshotError, SnapshotMetadata};
use crate::state::WorldState;
use crate::storage::SegmentedAgentStorage;
use sim_core::AgentId;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct M3pPropagationDailyMetrics {
    pub m3p_metrics: crate::m3p::M3pDailyMetrics,
    pub propagation: PropagationDailyCounts,
}

#[derive(Debug, Clone, PartialEq)]
pub struct M3pPropagationDayOutcome {
    pub executed_day: u32,
    pub metrics: Option<M3pPropagationDailyMetrics>,
    pub events: Vec<M3pEventRecord>,
    pub snapshot: Option<CanonicalSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct M3pPropagationReferenceState {
    base: M3pReferenceState,
    propagation: BTreeMap<AgentId, PropagationAgentState>,
    propagation_config: PropagationConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RestoredM3pPropagationSnapshot {
    pub metadata: SnapshotMetadata,
    pub state: M3pPropagationReferenceState,
}

impl M3pPropagationReferenceState {
    pub fn new(
        world: WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        validate_m3pprop_config(config)?;
        let base_config = m3p_base_config(config);
        let base = M3pReferenceState::new(world, &base_config, run_context)?;
        let propagation = base
            .world()
            .agents
            .iter()
            .map(|agent| (agent.agent_id, PropagationAgentState::default()))
            .collect();
        Self::from_parts(base, propagation, config)
    }

    pub fn from_parts(
        base: M3pReferenceState,
        propagation: BTreeMap<AgentId, PropagationAgentState>,
        config: &SimConfig,
    ) -> Result<Self, M0RunError> {
        validate_m3pprop_config(config)?;
        let base_config = m3p_base_config(config);
        let run_context = base.run_context();
        let base = M3pReferenceState::from_parts(
            base.world().clone(),
            base.relationship_graph().clone(),
            base.next_agent_id(),
            &base_config,
            &run_context,
        )?;
        validate_propagation_rows(base.world(), &propagation)?;
        Ok(Self {
            base,
            propagation,
            propagation_config: config.propagation.expect("validated propagation settings"),
        })
    }

    pub fn base_state(&self) -> &M3pReferenceState {
        &self.base
    }

    pub fn world(&self) -> &WorldState {
        self.base.world()
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        self.base.relationship_graph()
    }

    pub fn propagation(&self) -> &BTreeMap<AgentId, PropagationAgentState> {
        &self.propagation
    }

    pub fn state_hash(&self) -> Result<CanonicalHash, CanonicalHashError> {
        canonical_m3pprop_state_hash(self)
    }

    fn validate_for(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<(), M0RunError> {
        validate_m3pprop_config(config)?;
        if self.base.run_context() != *run_context {
            return Err(invariant(
                "M3P propagation run coordinates changed during a run",
            ));
        }
        if Some(self.propagation_config) != config.propagation {
            return Err(invariant(
                "M3P propagation configuration changed during a run",
            ));
        }
        self.base_state_valid(config, run_context)?;
        validate_propagation_rows(self.base.world(), &self.propagation)
    }

    fn base_state_valid(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<(), M0RunError> {
        M3pReferenceState::from_parts(
            self.base.world().clone(),
            self.base.relationship_graph().clone(),
            self.base.next_agent_id(),
            &m3p_base_config(config),
            run_context,
        )
        .map(|_| ())
    }
}

pub fn run_m3pprop_reference_day(
    state: &mut M3pPropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<M3pPropagationDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world().current_day.as_u32();
    let base_config = m3p_base_config(config);
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let mut propagation_counts = None;
    let outcome = {
        let M3pPropagationReferenceState {
            base,
            propagation,
            propagation_config,
        } = state;
        let mut apply_propagation = |world: &WorldState,
                                     graph: &RelationshipGraph,
                                     newborn_ids: &[AgentId],
                                     day: u32|
         -> Result<(), M0RunError> {
            sync_propagation_rows(world, propagation);
            propagation_counts = Some(
                apply_phase9c_m3p(
                    world,
                    graph,
                    propagation,
                    *propagation_config,
                    run_context.master_seed,
                    run_context.replicate_id,
                    day,
                    newborn_ids,
                )
                .map_err(invariant)?,
            );
            Ok(())
        };
        run_m3p_reference_day_with_post_phase9c_registered(
            base,
            &base_config,
            run_context,
            &base_options,
            &mut apply_propagation,
        )?
    };
    finish_m3pprop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts.expect("Phase 9c callback runs once per day"),
        outcome,
    )
}

pub(crate) fn run_m3pprop_reference_day_with_phase_recorder(
    state: &mut M3pPropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    forced_action: Option<&mut ForcedActionDirective>,
    records: &mut Vec<PhaseComponentHashes>,
) -> Result<M3pPropagationDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world().current_day.as_u32();
    let base_config = m3p_base_config(config);
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let (outcome, propagation_counts) = {
        let M3pPropagationReferenceState {
            base,
            propagation,
            propagation_config,
        } = state;
        let propagation_cell = RefCell::new(std::mem::take(propagation));
        let counts_cell = RefCell::new(None);
        let outcome = {
            let mut apply_propagation = |world: &WorldState,
                                         graph: &RelationshipGraph,
                                         newborn_ids: &[AgentId],
                                         day: u32|
             -> Result<(), M0RunError> {
                sync_propagation_rows(world, &mut propagation_cell.borrow_mut());
                let counts = apply_phase9c_m3p(
                    world,
                    graph,
                    &mut propagation_cell.borrow_mut(),
                    *propagation_config,
                    run_context.master_seed,
                    run_context.replicate_id,
                    day,
                    newborn_ids,
                )
                .map_err(invariant)?;
                *counts_cell.borrow_mut() = Some(counts);
                Ok(())
            };
            let mut overlay: &mut M3pReferenceRecorderOverlay<'_> = &mut |base_metrics, record| {
                record.propagation = Some(hash_propagation_rows(&propagation_cell.borrow()));
                if let (Some(base_metrics), Some(propagation_counts)) =
                    (base_metrics, *counts_cell.borrow())
                {
                    let metrics = M3pPropagationDailyMetrics {
                        m3p_metrics: base_metrics.clone(),
                        propagation: propagation_counts,
                    };
                    record.metrics_so_far = Some(
                        canonical_m3pprop_metrics_hash(std::slice::from_ref(&metrics)).map_err(
                            |error| {
                                invariant(format!(
                                    "phase recorder M3PPROP metrics hash failed: {error}"
                                ))
                            },
                        )?,
                    );
                }
                Ok(())
            };
            run_m3p_reference_day_with_post_phase9c_recorder(
                base,
                &base_config,
                run_context,
                &base_options,
                &mut apply_propagation,
                forced_action,
                records,
                &mut overlay,
            )?
        };
        let counts = counts_cell
            .into_inner()
            .expect("Phase 9c callback runs once per day");
        *propagation = propagation_cell.into_inner();
        (outcome, counts)
    };
    finish_m3pprop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts,
        outcome,
    )
}

#[cfg(test)]
fn run_m3pprop_reference_day_legacy(
    state: &mut M3pPropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<M3pPropagationDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world().current_day.as_u32();
    let base_config = m3p_base_config(config);
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let mut propagation_counts = None;
    let mut apply_propagation = |world: &WorldState,
                                 graph: &RelationshipGraph,
                                 newborn_ids: &[AgentId],
                                 day: u32|
     -> Result<(), M0RunError> {
        sync_propagation_rows(world, &mut state.propagation);
        propagation_counts = Some(
            apply_phase9c_m3p(
                world,
                graph,
                &mut state.propagation,
                state.propagation_config,
                run_context.master_seed,
                run_context.replicate_id,
                day,
                newborn_ids,
            )
            .map_err(invariant)?,
        );
        Ok(())
    };
    let outcome = run_m3p_reference_day_with_post_phase9b(
        &mut state.base,
        &base_config,
        run_context,
        &base_options,
        Some(&mut apply_propagation),
    )?;
    finish_m3pprop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts.expect("Phase 9c callback runs once per day"),
        outcome,
    )
}

fn finish_m3pprop_reference_day(
    state: &M3pPropagationReferenceState,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    executed_day: u32,
    propagation: PropagationDailyCounts,
    mut outcome: crate::m3p::M3pDayOutcome,
) -> Result<M3pPropagationDayOutcome, M0RunError> {
    let metrics = outcome
        .metrics
        .take()
        .map(|m3p_metrics| M3pPropagationDailyMetrics {
            m3p_metrics,
            propagation,
        });
    let snapshot = if options.snapshot_boundary {
        let metadata = SnapshotMetadata::new(
            state.world().current_day.as_u32(),
            run_context.master_seed,
            run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_m3pprop_snapshot(state, &metadata)?;
        if options.events_enabled {
            outcome.events.push(M3pEventRecord {
                key: EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                event: M3pEvent::Existing(Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: M3PROP_SNAPSHOT_SCHEMA_VERSION,
                })),
            });
            outcome.events.sort_by_key(|record| record.key);
        }
        Some(snapshot)
    } else {
        None
    };
    Ok(M3pPropagationDayOutcome {
        executed_day,
        metrics,
        events: outcome.events,
        snapshot,
    })
}

pub fn encode_m3pprop_snapshot(
    state: &M3pPropagationReferenceState,
    metadata: &SnapshotMetadata,
) -> Result<CanonicalSnapshot, SnapshotError> {
    if metadata.master_seed != state.base.run_context().master_seed
        || metadata.replicate_id != state.base.run_context().replicate_id
        || metadata.day != state.world().current_day.as_u32()
    {
        return Err(SnapshotError::InvalidState(
            "M3PROP M3P snapshot metadata does not match run coordinates or state day".into(),
        ));
    }
    validate_propagation_rows(state.world(), &state.propagation)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    let base = encode_m3p_snapshot(&state.base, metadata)?;
    let base_len = u64::try_from(base.bytes.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let count =
        u64::try_from(state.propagation.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SNAPSHOT_MAGIC);
    bytes.extend_from_slice(&M3PROP_SNAPSHOT_SCHEMA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&base_len.to_le_bytes());
    bytes.extend_from_slice(&base.bytes);
    bytes.extend_from_slice(&[1, 1, 1]);
    write_propagation_config(&mut bytes, state.propagation_config);
    bytes.extend_from_slice(&count.to_le_bytes());
    for (&agent_id, row) in &state.propagation {
        bytes.extend_from_slice(&agent_id.0.to_le_bytes());
        bytes.push(u8::from(row.informed));
        bytes.extend_from_slice(&row.informed_day.to_le_bytes());
    }
    Ok(CanonicalSnapshot::new(metadata.clone(), bytes))
}

pub fn decode_m3pprop_snapshot(
    bytes: &[u8],
    config: &SimConfig,
) -> Result<RestoredM3pPropagationSnapshot, SnapshotError> {
    validate_m3pprop_config(config)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    let mut cursor = 0usize;
    let magic: [u8; 8] = read_bytes(bytes, &mut cursor, 8)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SnapshotError::InvalidMagic(magic));
    }
    let schema = read_u32(bytes, &mut cursor)?;
    if schema != M3PROP_SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedVersion(schema));
    }
    let base_len = usize::try_from(read_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let base_bytes = read_bytes(bytes, &mut cursor, base_len)?;
    let base_config = m3p_base_config(config);
    let base = decode_m3p_snapshot(base_bytes, &base_config)?;
    if read_bytes(bytes, &mut cursor, 3)? != [1, 1, 1] {
        return Err(SnapshotError::InvalidState(
            "M3PROP snapshot feature manifest does not match profile m3p".into(),
        ));
    }
    let stored_config = read_propagation_config(bytes, &mut cursor)?;
    if Some(stored_config) != config.propagation {
        return Err(SnapshotError::InvalidState(
            "M3PROP snapshot parameters do not match configuration".into(),
        ));
    }
    let count = usize::try_from(read_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let expected_ids: Vec<_> = base
        .state
        .world()
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    if count != expected_ids.len() {
        return Err(SnapshotError::InvalidState(
            "M3PROP propagation count does not match M3P agents".into(),
        ));
    }
    let mut propagation = BTreeMap::new();
    for expected_id in expected_ids {
        let agent_id = AgentId(read_u32(bytes, &mut cursor)?);
        if agent_id != expected_id {
            return Err(SnapshotError::InvalidState(
                "M3PROP propagation rows are not in canonical AgentId order".into(),
            ));
        }
        let informed = match read_bytes(bytes, &mut cursor, 1)?[0] {
            0 => false,
            1 => true,
            value => return Err(SnapshotError::InvalidBoolean(value)),
        };
        let informed_day = read_u32(bytes, &mut cursor)?;
        if !informed && informed_day != 0 {
            return Err(SnapshotError::InvalidState(
                "uninformed M3PROP record has a nonzero informed_day".into(),
            ));
        }
        if informed && informed_day >= base.metadata.day {
            return Err(SnapshotError::InvalidState(
                "M3PROP informed_day is not before the snapshot resume day".into(),
            ));
        }
        propagation.insert(
            agent_id,
            PropagationAgentState {
                informed,
                informed_day,
            },
        );
    }
    if cursor != bytes.len() {
        return Err(SnapshotError::TrailingBytes(bytes.len() - cursor));
    }
    let state = M3pPropagationReferenceState::from_parts(base.state, propagation, config)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    Ok(RestoredM3pPropagationSnapshot {
        metadata: base.metadata,
        state,
    })
}

pub(crate) fn m3p_base_config(config: &SimConfig) -> SimConfig {
    let mut base = config.clone();
    let mut systems = SimulationSystems::for_profile(SimulationProfile::M3p);
    systems.propagation = false;
    base.simulation = Some(SimulationConfig { systems });
    base.propagation = None;
    base
}

pub(crate) fn validate_m3pprop_config(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile != SimulationProfile::M3p
        || !config.effective_systems().relationship_graph
        || !config.effective_systems().population_dynamics
        || !config.effective_systems().propagation
    {
        return Err(invariant(
            "M3P propagation runner requires profile m3p with population dynamics and propagation enabled",
        ));
    }
    config
        .validate()
        .map_err(|error| invariant(format!("invalid M3P propagation configuration: {error}")))
}

pub(crate) fn sync_propagation_rows(
    world: &WorldState,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
) {
    let mut ids: Vec<_> = world.agents.iter().map(|agent| agent.agent_id).collect();
    ids.sort_unstable();
    propagation.retain(|agent_id, _| ids.binary_search(agent_id).is_ok());
    for agent_id in ids {
        propagation.entry(agent_id).or_default();
    }
}

pub(crate) fn sync_propagation_storage_rows(
    storage: &SegmentedAgentStorage,
    propagation: &mut BTreeMap<AgentId, PropagationAgentState>,
) {
    let mut ids = storage.agent_ids().to_vec();
    ids.sort_unstable();
    propagation.retain(|agent_id, _| ids.binary_search(agent_id).is_ok());
    for agent_id in ids {
        propagation.entry(agent_id).or_default();
    }
}

fn validate_propagation_rows(
    world: &WorldState,
    propagation: &BTreeMap<AgentId, PropagationAgentState>,
) -> Result<(), M0RunError> {
    let mut ids: Vec<_> = world.agents.iter().map(|agent| agent.agent_id).collect();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) || propagation.keys().copied().ne(ids) {
        return Err(invariant(
            "M3PROP propagation rows do not match canonical M3P AgentIds",
        ));
    }
    if propagation.values().any(|row| {
        (!row.informed && row.informed_day != 0)
            || (row.informed && row.informed_day >= world.current_day.as_u32())
    }) {
        return Err(invariant(
            "uninformed M3PROP record has a nonzero informed_day",
        ));
    }
    Ok(())
}

fn write_propagation_config(bytes: &mut Vec<u8>, config: PropagationConfig) {
    bytes.extend_from_slice(&config.seed_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.transmit_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.forget_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.max_hops.to_le_bytes());
}

fn read_propagation_config(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<PropagationConfig, SnapshotError> {
    Ok(PropagationConfig {
        seed_probability: f32::from_bits(read_u32(bytes, cursor)?),
        transmit_probability: f32::from_bits(read_u32(bytes, cursor)?),
        forget_probability: f32::from_bits(read_u32(bytes, cursor)?),
        max_hops: read_u32(bytes, cursor)?,
    })
}

fn read_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    len: usize,
) -> Result<&'a [u8], SnapshotError> {
    let end = cursor
        .checked_add(len)
        .ok_or(SnapshotError::ArithmeticOverflow)?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or(SnapshotError::UnexpectedEof)?;
    *cursor = end;
    Ok(slice)
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, SnapshotError> {
    Ok(u32::from_le_bytes(
        read_bytes(bytes, cursor, 4)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SnapshotError> {
    Ok(u64::from_le_bytes(
        read_bytes(bytes, cursor, 8)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

fn invariant(message: impl Into<String>) -> M0RunError {
    M0RunError::InvariantViolation(message.into())
}

#[cfg(test)]
mod registered_reference_parity_tests {
    use super::{
        M3pPropagationReferenceState, run_m3pprop_reference_day, run_m3pprop_reference_day_legacy,
    };
    use crate::runner::{DayExecutionOptions, M0RunContext};
    use crate::{SimConfig, initialize_world};

    #[test]
    fn registered_m3pprop_reference_matches_legacy_day_by_day_and_at_snapshots() {
        let config: SimConfig = toml::from_str(include_str!(
            "../tests/fixtures/m3p_propagation_sustained.toml"
        ))
        .unwrap();
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let initial = initialize_world(&config).unwrap();
        let mut registered =
            M3pPropagationReferenceState::new(initial.clone(), &config, &run_context).unwrap();
        let mut legacy = M3pPropagationReferenceState::new(initial, &config, &run_context).unwrap();

        for day in 1..=300 {
            let options = DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: [100, 250, 300].contains(&day),
            };
            let actual =
                run_m3pprop_reference_day(&mut registered, &config, &run_context, &options)
                    .unwrap();
            let expected =
                run_m3pprop_reference_day_legacy(&mut legacy, &config, &run_context, &options)
                    .unwrap();
            assert_eq!(actual, expected, "M3PPROP outcome differs on day {day}");
            assert_eq!(
                registered.world(),
                legacy.world(),
                "world differs on day {day}"
            );
            assert_eq!(
                registered.relationship_graph().csr_hash(),
                legacy.relationship_graph().csr_hash(),
                "graph differs on day {day}"
            );
            assert_eq!(
                registered.base_state().next_agent_id(),
                legacy.base_state().next_agent_id(),
                "allocator differs on day {day}"
            );
            assert_eq!(
                registered.propagation(),
                legacy.propagation(),
                "propagation state differs on day {day}"
            );
        }
    }
}
