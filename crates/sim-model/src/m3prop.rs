use crate::config::{
    PropagationConfig, SimConfig, SimulationConfig, SimulationProfile, SimulationSystems,
};
use crate::counterfactual::{ForcedActionDirective, PhaseComponentHashes, hash_propagation_rows};
use crate::events::{Event, EventKey, EventRecord, GLOBAL_PARTITION_KEY, ObservationEvent};
use crate::hashing::{
    CanonicalHash, CanonicalHashError, canonical_m3prop_event_hash, canonical_m3prop_metrics_hash,
    canonical_m3prop_state_hash,
};
use crate::metrics::M3DailyMetrics;
use crate::propagation::{PropagationAgentState, PropagationDailyCounts, apply_phase9c};
use crate::relationship_graph::RelationshipGraph;
#[cfg(test)]
use crate::runner::run_m3_reference_day_with_post_phase9;
use crate::runner::{
    DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION, DayExecutionOptions, M0RunContext, M0RunError,
    M3ReferenceContext, M3ReferenceRecorderOverlay,
    run_m3_reference_day_with_post_phase9c_recorder,
    run_m3_reference_day_with_post_phase9c_registered,
};
use crate::snapshot::{
    CanonicalSnapshot, SNAPSHOT_MAGIC, SnapshotError, SnapshotMetadata, decode_snapshot,
    encode_snapshot,
};
use crate::state::WorldState;
use sim_core::AgentId;
use std::cell::RefCell;
use std::collections::BTreeMap;

pub const M3PROP_SNAPSHOT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct M3PropDailyMetrics {
    pub m3_metrics: M3DailyMetrics,
    pub population: Option<M3PropPopulationMetrics>,
    pub propagation: PropagationDailyCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct M3PropPopulationMetrics {
    pub births: u64,
    pub removed: u64,
    pub replaced_edges: u64,
    pub live_population: u64,
    pub next_agent_id: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct M3PropDayOutcome {
    pub executed_day: u32,
    pub metrics: Option<M3PropDailyMetrics>,
    pub events: Vec<EventRecord>,
    pub snapshot: Option<CanonicalSnapshot>,
}

pub struct M3PropagationReferenceState {
    pub(crate) world: WorldState,
    pub(crate) reference: M3ReferenceContext,
    pub(crate) propagation: BTreeMap<AgentId, PropagationAgentState>,
    pub(crate) run_context: M0RunContext,
    relationship_degree_k: i64,
    p_local_bits: u32,
    propagation_config: PropagationConfig,
}

pub struct RestoredM3PropagationSnapshot {
    pub metadata: SnapshotMetadata,
    pub state: M3PropagationReferenceState,
}

impl M3PropagationReferenceState {
    pub fn new(
        world: WorldState,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        validate_m3prop_config(config)?;
        let base_config = m3_base_config(config);
        let reference = M3ReferenceContext::new(&world, &base_config, run_context)?;
        let propagation = world
            .agents
            .iter()
            .map(|agent| (agent.agent_id, PropagationAgentState::default()))
            .collect();
        Ok(Self {
            world,
            reference,
            propagation,
            run_context: *run_context,
            relationship_degree_k: config.interaction.relationship_degree_k.unwrap_or_default(),
            p_local_bits: config.interaction.p_local.unwrap_or_default().to_bits(),
            propagation_config: config.propagation.expect("validated propagation settings"),
        })
    }

    pub(crate) fn from_parts(
        world: WorldState,
        graph: crate::relationship_graph::RelationshipGraph,
        propagation: BTreeMap<AgentId, PropagationAgentState>,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<Self, M0RunError> {
        let mut state = Self::new(world, config, run_context)?;
        state.reference =
            crate::runner::M3ReferenceContext::from_relationship_graph(graph, run_context);
        state.propagation = propagation;
        state.validate_for(config, run_context)?;
        Ok(state)
    }

    pub fn world(&self) -> &WorldState {
        &self.world
    }

    pub fn world_mut(&mut self) -> &mut WorldState {
        &mut self.world
    }

    pub fn relationship_graph(&self) -> &RelationshipGraph {
        self.reference.relationship_graph()
    }

    pub fn propagation(&self) -> &BTreeMap<AgentId, PropagationAgentState> {
        &self.propagation
    }

    pub fn state_hash(&self) -> Result<CanonicalHash, CanonicalHashError> {
        canonical_m3prop_state_hash(self)
    }

    pub fn metrics_hash(
        metrics: &[M3PropDailyMetrics],
    ) -> Result<CanonicalHash, CanonicalHashError> {
        canonical_m3prop_metrics_hash(metrics)
    }

    pub fn events_hash(events: &[EventRecord]) -> Result<CanonicalHash, CanonicalHashError> {
        canonical_m3prop_event_hash(events)
    }

    fn validate_for(
        &self,
        config: &SimConfig,
        run_context: &M0RunContext,
    ) -> Result<(), M0RunError> {
        validate_m3prop_config(config)?;
        if self.run_context != *run_context {
            return Err(invariant(
                "M3 propagation run coordinates changed during a run",
            ));
        }
        if config.interaction.relationship_degree_k != Some(self.relationship_degree_k)
            || config.interaction.p_local.map(f32::to_bits) != Some(self.p_local_bits)
            || config.propagation != Some(self.propagation_config)
        {
            return Err(invariant(
                "M3 propagation configuration changed during a run",
            ));
        }
        let ids: Vec<_> = self
            .world
            .agents
            .iter()
            .map(|agent| agent.agent_id)
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        if ids.len() != sorted.len()
            || sorted.windows(2).any(|pair| pair[0] == pair[1])
            || self.reference.relationship_graph().source_ids() != sorted
            || self.propagation.keys().copied().ne(sorted.iter().copied())
        {
            return Err(invariant(
                "M3 propagation state does not match world AgentIds",
            ));
        }
        Ok(())
    }
}

pub fn run_m3prop_reference_day(
    state: &mut M3PropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<M3PropDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world.current_day.as_u32();
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let base_config = m3_base_config(config);
    let mut propagation_counts = None;
    let outcome = {
        let M3PropagationReferenceState {
            world,
            reference,
            propagation,
            propagation_config,
            ..
        } = state;
        let graph = reference.relationship_graph();
        run_m3_reference_day_with_post_phase9c_registered(
            world,
            &base_config,
            reference,
            &base_options,
            &mut |world, day| {
                propagation_counts = Some(
                    apply_phase9c(
                        world,
                        graph,
                        propagation,
                        *propagation_config,
                        run_context.master_seed,
                        run_context.replicate_id,
                        day,
                    )
                    .map_err(invariant)?,
                );
                Ok(())
            },
        )?
    };
    finish_m3prop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts.expect("Phase 9c callback runs once per day"),
        outcome,
    )
}

pub(crate) fn run_m3prop_reference_day_with_phase_recorder(
    state: &mut M3PropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    forced_action: Option<&mut ForcedActionDirective>,
    records: &mut Vec<PhaseComponentHashes>,
) -> Result<M3PropDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world.current_day.as_u32();
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let base_config = m3_base_config(config);
    let (outcome, propagation_counts) = {
        let M3PropagationReferenceState {
            world,
            reference,
            propagation,
            propagation_config,
            ..
        } = state;
        let graph = reference.relationship_graph();
        let propagation_cell = RefCell::new(std::mem::take(propagation));
        let counts_cell = RefCell::new(None);
        let outcome = {
            let mut apply_propagation = |world: &mut WorldState, day| {
                let counts = apply_phase9c(
                    world,
                    graph,
                    &mut propagation_cell.borrow_mut(),
                    *propagation_config,
                    run_context.master_seed,
                    run_context.replicate_id,
                    day,
                )
                .map_err(invariant)?;
                *counts_cell.borrow_mut() = Some(counts);
                Ok(())
            };
            let mut overlay: &mut M3ReferenceRecorderOverlay<'_> = &mut |base_metrics, record| {
                record.propagation = Some(hash_propagation_rows(&propagation_cell.borrow()));
                if let (Some(base_metrics), Some(propagation_counts)) =
                    (base_metrics, *counts_cell.borrow())
                {
                    let metrics = M3PropDailyMetrics {
                        m3_metrics: base_metrics.clone(),
                        population: None,
                        propagation: propagation_counts,
                    };
                    record.metrics_so_far = Some(
                        canonical_m3prop_metrics_hash(std::slice::from_ref(&metrics)).map_err(
                            |error| {
                                invariant(format!(
                                    "phase recorder M3PROP metrics hash failed: {error}"
                                ))
                            },
                        )?,
                    );
                }
                Ok(())
            };
            run_m3_reference_day_with_post_phase9c_recorder(
                world,
                &base_config,
                reference,
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
    finish_m3prop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts,
        outcome,
    )
}

#[cfg(test)]
fn run_m3prop_reference_day_legacy(
    state: &mut M3PropagationReferenceState,
    config: &SimConfig,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
) -> Result<M3PropDayOutcome, M0RunError> {
    state.validate_for(config, run_context)?;
    let executed_day = state.world.current_day.as_u32();
    let mut base_options = *options;
    base_options.snapshot_boundary = false;
    let base_config = m3_base_config(config);
    let mut propagation_counts = None;
    let outcome = {
        let M3PropagationReferenceState {
            world,
            reference,
            propagation,
            propagation_config,
            ..
        } = state;
        let graph = reference.relationship_graph();
        run_m3_reference_day_with_post_phase9(
            world,
            &base_config,
            reference,
            &base_options,
            |world, day| {
                propagation_counts = Some(
                    apply_phase9c(
                        world,
                        graph,
                        propagation,
                        *propagation_config,
                        run_context.master_seed,
                        run_context.replicate_id,
                        day,
                    )
                    .map_err(invariant)?,
                );
                Ok(())
            },
        )?
    };
    finish_m3prop_reference_day(
        state,
        run_context,
        options,
        executed_day,
        propagation_counts.expect("Phase 9c callback runs once per day"),
        outcome,
    )
}

fn finish_m3prop_reference_day(
    state: &M3PropagationReferenceState,
    run_context: &M0RunContext,
    options: &DayExecutionOptions,
    executed_day: u32,
    propagation_counts: PropagationDailyCounts,
    mut outcome: crate::runner::M3DayOutcome,
) -> Result<M3PropDayOutcome, M0RunError> {
    let metrics = outcome.metrics.map(|m3_metrics| M3PropDailyMetrics {
        m3_metrics,
        population: None,
        propagation: propagation_counts,
    });

    let snapshot = if options.snapshot_boundary {
        let metadata = SnapshotMetadata::new(
            state.world.current_day.as_u32(),
            run_context.master_seed,
            run_context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let snapshot = encode_m3prop_snapshot(state, &metadata)?;
        if options.events_enabled {
            outcome.events.push(EventRecord::new(
                EventKey::new(executed_day, 11, GLOBAL_PARTITION_KEY, 0),
                Event::Observation(ObservationEvent::SnapshotEmitted {
                    day: metadata.day,
                    master_seed: metadata.master_seed,
                    replicate_id: metadata.replicate_id,
                    schema_version: M3PROP_SNAPSHOT_SCHEMA_VERSION,
                }),
            ));
            outcome.events.sort_by_key(|event| event.key);
        }
        Some(snapshot)
    } else {
        None
    };

    Ok(M3PropDayOutcome {
        executed_day,
        metrics,
        events: outcome.events,
        snapshot,
    })
}

pub fn encode_m3prop_snapshot(
    state: &M3PropagationReferenceState,
    metadata: &SnapshotMetadata,
) -> Result<CanonicalSnapshot, SnapshotError> {
    if metadata.master_seed != state.run_context.master_seed
        || metadata.replicate_id != state.run_context.replicate_id
        || metadata.day != state.world.current_day.as_u32()
    {
        return Err(SnapshotError::InvalidState(
            "M3PROP snapshot metadata does not match run coordinates or state day".into(),
        ));
    }
    let mut agent_ids: Vec<_> = state
        .world
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    agent_ids.sort_unstable();
    if agent_ids.windows(2).any(|pair| pair[0] == pair[1])
        || state.reference.relationship_graph().source_ids() != agent_ids
        || state
            .propagation
            .keys()
            .copied()
            .ne(agent_ids.iter().copied())
        || state
            .propagation
            .values()
            .any(|row| !row.informed && row.informed_day != 0)
    {
        return Err(SnapshotError::InvalidState(
            "M3PROP propagation rows do not match canonical agent state".into(),
        ));
    }
    let base = encode_snapshot(&state.world, metadata)?;
    let base_len = u64::try_from(base.bytes.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let count =
        u64::try_from(state.propagation.len()).map_err(|_| SnapshotError::LengthOverflow)?;
    let config = state.propagation_config;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&SNAPSHOT_MAGIC);
    bytes.extend_from_slice(&M3PROP_SNAPSHOT_SCHEMA_VERSION.to_le_bytes());
    bytes.extend_from_slice(&base_len.to_le_bytes());
    bytes.extend_from_slice(&base.bytes);
    bytes.extend_from_slice(&[1, 0, 1]);
    bytes.extend_from_slice(&config.seed_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.transmit_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.forget_probability.to_bits().to_le_bytes());
    bytes.extend_from_slice(&config.max_hops.to_le_bytes());
    bytes.extend_from_slice(&count.to_le_bytes());
    for (&agent_id, agent_state) in &state.propagation {
        bytes.extend_from_slice(&agent_id.0.to_le_bytes());
        bytes.push(u8::from(agent_state.informed));
        bytes.extend_from_slice(&agent_state.informed_day.to_le_bytes());
    }
    Ok(CanonicalSnapshot::new(metadata.clone(), bytes))
}

pub fn decode_m3prop_snapshot(
    bytes: &[u8],
    config: &SimConfig,
) -> Result<RestoredM3PropagationSnapshot, SnapshotError> {
    validate_m3prop_config(config)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    let mut cursor = 0;
    let magic: [u8; 8] = read_snapshot_bytes(bytes, &mut cursor, 8)?
        .try_into()
        .map_err(|_| SnapshotError::UnexpectedEof)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SnapshotError::InvalidMagic(magic));
    }
    let schema = read_snapshot_u32(bytes, &mut cursor)?;
    if schema != M3PROP_SNAPSHOT_SCHEMA_VERSION {
        return Err(SnapshotError::UnsupportedVersion(schema));
    }
    let base_len = usize::try_from(read_snapshot_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let base = decode_snapshot(read_snapshot_bytes(bytes, &mut cursor, base_len)?)?;
    let flags = read_snapshot_bytes(bytes, &mut cursor, 3)?;
    if flags != [1, 0, 1] {
        return Err(SnapshotError::InvalidState(
            "M3PROP snapshot feature manifest does not match profile m3".into(),
        ));
    }
    let stored_config = PropagationConfig {
        seed_probability: f32::from_bits(read_snapshot_u32(bytes, &mut cursor)?),
        transmit_probability: f32::from_bits(read_snapshot_u32(bytes, &mut cursor)?),
        forget_probability: f32::from_bits(read_snapshot_u32(bytes, &mut cursor)?),
        max_hops: read_snapshot_u32(bytes, &mut cursor)?,
    };
    if Some(stored_config) != config.propagation {
        return Err(SnapshotError::InvalidState(
            "M3PROP snapshot parameters do not match configuration".into(),
        ));
    }
    let count = usize::try_from(read_snapshot_u64(bytes, &mut cursor)?)
        .map_err(|_| SnapshotError::LengthOverflow)?;
    let sorted_ids: Vec<_> = base
        .world
        .agents
        .iter()
        .map(|agent| agent.agent_id)
        .collect();
    if count != sorted_ids.len() {
        return Err(SnapshotError::InvalidState(
            "M3PROP propagation count does not match agents".into(),
        ));
    }
    let mut propagated = BTreeMap::new();
    for expected_id in sorted_ids {
        let agent_id = AgentId(read_snapshot_u32(bytes, &mut cursor)?);
        if agent_id != expected_id {
            return Err(SnapshotError::InvalidState(
                "M3PROP propagation rows are not in canonical AgentId order".into(),
            ));
        }
        let informed = match read_snapshot_bytes(bytes, &mut cursor, 1)?[0] {
            0 => false,
            1 => true,
            value => return Err(SnapshotError::InvalidBoolean(value)),
        };
        let informed_day = read_snapshot_u32(bytes, &mut cursor)?;
        if !informed && informed_day != 0 {
            return Err(SnapshotError::InvalidState(
                "uninformed M3PROP record has a nonzero informed_day".into(),
            ));
        }
        if informed && informed_day >= base.world.current_day.as_u32() {
            return Err(SnapshotError::InvalidState(
                "M3PROP informed_day is not before the snapshot resume day".into(),
            ));
        }
        propagated.insert(
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
    let run_context = M0RunContext::new(base.metadata.master_seed, base.metadata.replicate_id);
    let mut state = M3PropagationReferenceState::new(base.world, config, &run_context)
        .map_err(|error| SnapshotError::InvalidState(error.to_string()))?;
    state.propagation = propagated;
    Ok(RestoredM3PropagationSnapshot {
        metadata: base.metadata,
        state,
    })
}

pub(crate) fn m3_base_config(config: &SimConfig) -> SimConfig {
    let mut base = config.clone();
    base.simulation = Some(SimulationConfig {
        systems: SimulationSystems::for_profile(SimulationProfile::M3),
    });
    base.propagation = None;
    base
}

fn validate_m3prop_config(config: &SimConfig) -> Result<(), M0RunError> {
    if config.profile != SimulationProfile::M3 || !config.effective_systems().propagation {
        return Err(invariant(
            "M3 propagation reference runner requires profile m3 with propagation enabled",
        ));
    }
    config
        .validate()
        .map_err(|error| invariant(format!("invalid M3 propagation configuration: {error}")))
}

fn invariant(message: impl Into<String>) -> M0RunError {
    M0RunError::InvariantViolation(message.into())
}

fn read_snapshot_bytes<'a>(
    bytes: &'a [u8],
    cursor: &mut usize,
    length: usize,
) -> Result<&'a [u8], SnapshotError> {
    let end = cursor
        .checked_add(length)
        .ok_or(SnapshotError::ArithmeticOverflow)?;
    let result = bytes
        .get(*cursor..end)
        .ok_or(SnapshotError::UnexpectedEof)?;
    *cursor = end;
    Ok(result)
}

fn read_snapshot_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, SnapshotError> {
    Ok(u32::from_le_bytes(
        read_snapshot_bytes(bytes, cursor, 4)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

fn read_snapshot_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, SnapshotError> {
    Ok(u64::from_le_bytes(
        read_snapshot_bytes(bytes, cursor, 8)?
            .try_into()
            .map_err(|_| SnapshotError::UnexpectedEof)?,
    ))
}

#[cfg(test)]
mod registered_reference_parity_tests {
    use super::{
        M3PropagationReferenceState, run_m3prop_reference_day, run_m3prop_reference_day_legacy,
    };
    use crate::runner::{DayExecutionOptions, M0RunContext};
    use crate::{SimConfig, initialize_world};

    #[test]
    fn registered_m3prop_reference_matches_legacy_day_by_day_and_at_snapshots() {
        let config: SimConfig =
            toml::from_str(include_str!("../tests/fixtures/m3_propagation.toml")).unwrap();
        let run_context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let initial = initialize_world(&config).unwrap();
        let mut registered =
            M3PropagationReferenceState::new(initial.clone(), &config, &run_context).unwrap();
        let mut legacy = M3PropagationReferenceState::new(initial, &config, &run_context).unwrap();

        for day in 1..=500 {
            let options = DayExecutionOptions {
                metrics_enabled: true,
                events_enabled: true,
                snapshot_boundary: [100, 250, 500].contains(&day),
            };
            let actual =
                run_m3prop_reference_day(&mut registered, &config, &run_context, &options).unwrap();
            let expected =
                run_m3prop_reference_day_legacy(&mut legacy, &config, &run_context, &options)
                    .unwrap();
            assert_eq!(actual, expected, "M3PROP outcome differs on day {day}");
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
                registered.propagation(),
                legacy.propagation(),
                "propagation state differs on day {day}"
            );
        }
    }
}
