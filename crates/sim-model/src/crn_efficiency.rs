//! Deterministic replicate-set evaluation for the M4-04 CRN efficiency study.

use crate::config::{SimConfig, SimulationRunnerSelection};
use crate::counterfactual::{
    BranchError, BranchIntervention, BranchRequest,
    run_counterfactual_branch_with_divergence_series, run_counterfactual_intervention_only,
};
use crate::m3p::{M3pReferenceState, decode_m3p_snapshot, encode_m3p_snapshot};
use crate::m3pprop::{
    M3pPropagationReferenceState, decode_m3pprop_snapshot, encode_m3pprop_snapshot, m3p_base_config,
};
use crate::runner::M0RunContext;
use crate::snapshot::{CanonicalSnapshot, SnapshotError};
use crate::state::WorldState;
use rayon::ThreadPool;
use rayon::prelude::*;
use sim_core::GroupId;
use std::fmt;
use std::num::NonZeroUsize;

const MIN_REPLICATES: usize = 2;
const MAX_REPLICATES: usize = 100;
const INDEPENDENT_INTERVENTION_REPLICATE_BASE: u32 = 100_000;
const BOOTSTRAP_RESAMPLES: usize = 10_000;
const BOOTSTRAP_SEED: u64 = 0xA3_11_70;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReplicateIdSets {
    paired: Vec<u32>,
    independent_base: Vec<u32>,
    independent_intervention: Vec<u32>,
}

fn replicate_id_sets(count: usize) -> ReplicateIdSets {
    let paired: Vec<_> = (1..=count as u32).collect();
    ReplicateIdSets {
        independent_base: paired.clone(),
        independent_intervention: (0..count)
            .map(|index| INDEPENDENT_INTERVENTION_REPLICATE_BASE + 1 + index as u32)
            .collect(),
        paired,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrnOutcome {
    AlivePopulation,
    TotalWealth,
    MeanHealthOfAlive,
    TargetSettlementAlivePopulation,
    TargetSettlementWealth,
}

impl CrnOutcome {
    pub const ALL: [Self; 5] = [
        Self::AlivePopulation,
        Self::TotalWealth,
        Self::MeanHealthOfAlive,
        Self::TargetSettlementAlivePopulation,
        Self::TargetSettlementWealth,
    ];
}

impl fmt::Display for CrnOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlivePopulation => f.write_str("alive_population"),
            Self::TotalWealth => f.write_str("total_wealth"),
            Self::MeanHealthOfAlive => f.write_str("mean_health_alive"),
            Self::TargetSettlementAlivePopulation => {
                f.write_str("target_settlement_alive_population")
            }
            Self::TargetSettlementWealth => f.write_str("target_settlement_wealth"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrnEfficiencyRequest {
    pub config: SimConfig,
    pub branch_point: CanonicalSnapshot,
    pub intervention: BranchIntervention,
    pub target_group_id: GroupId,
    pub horizons: Vec<u32>,
    /// CI uses 10; the report-only experiment uses 100.
    pub replicate_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CrnVarianceRatio {
    ExactPairedVarianceZero,
    Estimated {
        ratio: f64,
        ci95_lower: f64,
        ci95_upper: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrnOutcomeEstimate {
    pub horizon_days: u32,
    pub outcome: CrnOutcome,
    pub paired_replicates: usize,
    pub independent_base_replicates: usize,
    pub independent_intervention_replicates: usize,
    pub paired_mean_effect: f64,
    pub paired_mean_ci95_lower: f64,
    pub paired_mean_ci95_upper: f64,
    pub paired_sample_variance: f64,
    pub paired_estimator_variance: f64,
    pub independent_mean_effect: f64,
    pub independent_base_sample_variance: f64,
    pub independent_intervention_sample_variance: f64,
    pub independent_estimator_variance: f64,
    pub variance_ratio: CrnVarianceRatio,
    pub mean_paired_divergent_records: f64,
    pub mean_newborn_crn_loss: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CrnEfficiencyResult {
    pub estimates: Vec<CrnOutcomeEstimate>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CrnEfficiencyError {
    UnsupportedProfile,
    InvalidReplicateCount,
    InvalidHorizons,
    SnapshotIdentityMismatch,
    MissingTargetSettlement(GroupId),
    MissingDay(u32),
    MissingSnapshot(u32),
    UndefinedOutcome(CrnOutcome, u32),
    ArithmeticOverflow,
    Snapshot(SnapshotError),
    Branch(BranchError),
    InvalidConfig(String),
}

impl fmt::Display for CrnEfficiencyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProfile => f.write_str("M4-04 requires an M3P snapshot profile"),
            Self::InvalidReplicateCount => {
                write!(
                    f,
                    "replicate count must be in {MIN_REPLICATES}..={MAX_REPLICATES}"
                )
            }
            Self::InvalidHorizons => {
                f.write_str("horizons must be non-empty, positive, and strictly ascending")
            }
            Self::SnapshotIdentityMismatch => {
                f.write_str("config and branch-point snapshot identity do not match")
            }
            Self::MissingTargetSettlement(group_id) => {
                write!(f, "missing target settlement {group_id}")
            }
            Self::MissingDay(day) => write!(f, "branch result is missing day {day}"),
            Self::MissingSnapshot(day) => {
                write!(f, "branch result is missing snapshot for day {day}")
            }
            Self::UndefinedOutcome(outcome, day) => {
                write!(f, "outcome {outcome} is undefined on day {day}")
            }
            Self::ArithmeticOverflow => f.write_str("checked wealth accumulation overflowed"),
            Self::Snapshot(error) => write!(f, "snapshot error: {error}"),
            Self::Branch(error) => write!(f, "branch error: {error}"),
            Self::InvalidConfig(message) => write!(f, "invalid config: {message}"),
        }
    }
}

impl std::error::Error for CrnEfficiencyError {}

impl From<SnapshotError> for CrnEfficiencyError {
    fn from(value: SnapshotError) -> Self {
        Self::Snapshot(value)
    }
}

impl From<BranchError> for CrnEfficiencyError {
    fn from(value: BranchError) -> Self {
        Self::Branch(value)
    }
}

#[derive(Clone, Copy)]
struct PairedObservation {
    base: [f64; 5],
    intervention: [f64; 5],
    divergent_records: u64,
    newborn_crn_loss: u64,
}

fn ordered_replicate_map<T, F>(
    replicate_ids: &[u32],
    execution: Option<(&ThreadPool, NonZeroUsize)>,
    work: F,
) -> Result<Vec<T>, CrnEfficiencyError>
where
    T: Send,
    F: Fn(u32) -> Result<T, CrnEfficiencyError> + Sync,
{
    let results: Vec<_> = if let Some((pool, chunk_size)) = execution {
        pool.install(|| {
            replicate_ids
                .par_iter()
                .with_min_len(chunk_size.get())
                .map(|&replicate_id| work(replicate_id))
                .collect()
        })
    } else {
        replicate_ids
            .iter()
            .map(|&replicate_id| work(replicate_id))
            .collect()
    };
    results.into_iter().collect()
}

/// Runs paired and independent M3P replicate sets using the existing branch API.
/// The paired base runs also form the independent base sample prescribed by §13.
pub fn run_crn_efficiency(
    request: &CrnEfficiencyRequest,
) -> Result<CrnEfficiencyResult, CrnEfficiencyError> {
    run_crn_efficiency_with_execution(request, None)
}

pub fn run_crn_efficiency_parallel(
    request: &CrnEfficiencyRequest,
    pool: &ThreadPool,
    chunk_size: NonZeroUsize,
) -> Result<CrnEfficiencyResult, CrnEfficiencyError> {
    run_crn_efficiency_with_execution(request, Some((pool, chunk_size)))
}

fn run_crn_efficiency_with_execution(
    request: &CrnEfficiencyRequest,
    execution: Option<(&ThreadPool, NonZeroUsize)>,
) -> Result<CrnEfficiencyResult, CrnEfficiencyError> {
    validate_request(request)?;
    let start_day = request.branch_point.metadata.day;
    let horizon_days = *request
        .horizons
        .last()
        .expect("validated non-empty horizons");
    let snapshot_days = request
        .horizons
        .iter()
        .map(|horizon| {
            start_day
                .checked_add(*horizon - 1)
                .ok_or(CrnEfficiencyError::InvalidHorizons)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut paired = vec![
        Vec::<PairedObservation>::with_capacity(request.replicate_count);
        request.horizons.len()
    ];

    let replicate_ids = replicate_id_sets(request.replicate_count);
    debug_assert_eq!(replicate_ids.independent_base, replicate_ids.paired);
    let paired_by_replicate =
        ordered_replicate_map(&replicate_ids.paired, execution, |replicate| {
            let (config, context, snapshot) = rebound_run(request, replicate)?;
            let run = BranchRequest {
                config: config.clone(),
                run_context: context,
                snapshot,
                intervention: request.intervention.clone(),
                horizon_days,
                execution: crate::counterfactual::BranchExecutionOption::Serial,
                snapshot_days: snapshot_days.clone(),
            };
            let result = run_counterfactual_branch_with_divergence_series(&run)?;
            let mut observations = Vec::with_capacity(request.horizons.len());
            for horizon in &request.horizons {
                let day = start_day + *horizon - 1;
                let base_record = find_day(&result.branch.base_days, day)?;
                let intervention_record = find_day(&result.branch.branch_days, day)?;
                let base = extract_outcomes(
                    base_record,
                    &config,
                    request.target_group_id,
                    CrnOutcome::ALL,
                )?;
                let intervention = extract_outcomes(
                    intervention_record,
                    &config,
                    request.target_group_id,
                    CrnOutcome::ALL,
                )?;
                let divergence = result
                    .daily_series
                    .iter()
                    .find(|record| record.day == day)
                    .ok_or(CrnEfficiencyError::MissingDay(day))?;
                let newborn_crn_loss = result
                    .daily_series
                    .iter()
                    .filter(|record| record.day <= day)
                    .try_fold(0u64, |total, record| {
                        total.checked_add(record.newborn_crn_loss)
                    })
                    .ok_or(CrnEfficiencyError::ArithmeticOverflow)?;
                observations.push(PairedObservation {
                    base,
                    intervention,
                    divergent_records: divergence.changed_agent_records,
                    newborn_crn_loss,
                });
            }
            Ok(observations)
        })?;
    for observations in paired_by_replicate {
        for (horizon_index, observation) in observations.into_iter().enumerate() {
            paired[horizon_index].push(observation);
        }
    }

    let mut independent_intervention =
        vec![Vec::<[f64; 5]>::with_capacity(request.replicate_count); request.horizons.len()];
    let independent_by_replicate = ordered_replicate_map(
        &replicate_ids.independent_intervention,
        execution,
        |replicate| {
            let (config, context, snapshot) = rebound_run(request, replicate)?;
            let run = BranchRequest {
                config: config.clone(),
                run_context: context,
                snapshot,
                intervention: request.intervention.clone(),
                horizon_days,
                execution: crate::counterfactual::BranchExecutionOption::Serial,
                snapshot_days: snapshot_days.clone(),
            };
            let records = run_counterfactual_intervention_only(&run)?;
            let mut observations = Vec::with_capacity(request.horizons.len());
            for horizon in &request.horizons {
                let day = start_day + *horizon - 1;
                let record = find_day(&records, day)?;
                observations.push(extract_outcomes(
                    record,
                    &config,
                    request.target_group_id,
                    CrnOutcome::ALL,
                )?);
            }
            Ok(observations)
        },
    )?;
    for observations in independent_by_replicate {
        for (horizon_index, observation) in observations.into_iter().enumerate() {
            independent_intervention[horizon_index].push(observation);
        }
    }

    let mut estimates = Vec::with_capacity(request.horizons.len() * CrnOutcome::ALL.len());
    for (horizon_index, horizon) in request.horizons.iter().enumerate() {
        for (outcome_index, outcome) in CrnOutcome::ALL.into_iter().enumerate() {
            let observations = &paired[horizon_index];
            let paired_base: Vec<_> = observations
                .iter()
                .map(|sample| sample.base[outcome_index])
                .collect();
            let paired_treatment: Vec<_> = observations
                .iter()
                .map(|sample| sample.intervention[outcome_index])
                .collect();
            let paired_deltas: Vec<_> = paired_treatment
                .iter()
                .zip(&paired_base)
                .map(|(treated, base)| treated - base)
                .collect();
            let independent_base = paired_base.as_slice();
            let independent_treatment: Vec<_> = independent_intervention[horizon_index]
                .iter()
                .map(|values| values[outcome_index])
                .collect();
            let paired_sample_variance = sample_variance(&paired_deltas);
            let independent_base_sample_variance = sample_variance(independent_base);
            let independent_intervention_sample_variance = sample_variance(&independent_treatment);
            let independent_estimator_variance = (independent_base_sample_variance
                + independent_intervention_sample_variance)
                / request.replicate_count as f64;
            let paired_estimator_variance = paired_sample_variance / request.replicate_count as f64;
            let (paired_mean_ci95_lower, paired_mean_ci95_upper, variance_ratio) =
                bootstrap_intervals(
                    &paired_base,
                    &paired_deltas,
                    &independent_treatment,
                    paired_sample_variance == 0.0,
                );
            estimates.push(CrnOutcomeEstimate {
                horizon_days: *horizon,
                outcome,
                paired_replicates: request.replicate_count,
                independent_base_replicates: request.replicate_count,
                independent_intervention_replicates: request.replicate_count,
                paired_mean_effect: mean(&paired_deltas),
                paired_mean_ci95_lower,
                paired_mean_ci95_upper,
                paired_sample_variance,
                paired_estimator_variance,
                independent_mean_effect: mean(&independent_treatment) - mean(independent_base),
                independent_base_sample_variance,
                independent_intervention_sample_variance,
                independent_estimator_variance,
                variance_ratio,
                mean_paired_divergent_records: mean(
                    &observations
                        .iter()
                        .map(|sample| sample.divergent_records as f64)
                        .collect::<Vec<_>>(),
                ),
                mean_newborn_crn_loss: mean(
                    &observations
                        .iter()
                        .map(|sample| sample.newborn_crn_loss as f64)
                        .collect::<Vec<_>>(),
                ),
            });
        }
    }
    Ok(CrnEfficiencyResult { estimates })
}

fn validate_request(request: &CrnEfficiencyRequest) -> Result<(), CrnEfficiencyError> {
    if !(MIN_REPLICATES..=MAX_REPLICATES).contains(&request.replicate_count) {
        return Err(CrnEfficiencyError::InvalidReplicateCount);
    }
    if !matches!(
        request.config.runner_selection(),
        SimulationRunnerSelection::M3p | SimulationRunnerSelection::M3pPropagation
    ) {
        return Err(CrnEfficiencyError::UnsupportedProfile);
    }
    request
        .config
        .validate()
        .map_err(|error| CrnEfficiencyError::InvalidConfig(error.to_string()))?;
    if request.config.world.master_seed != request.branch_point.metadata.master_seed
        || request.config.world.replicate_id != request.branch_point.metadata.replicate_id
    {
        return Err(CrnEfficiencyError::SnapshotIdentityMismatch);
    }
    if request.horizons.is_empty()
        || request.horizons.contains(&0)
        || request.horizons.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(CrnEfficiencyError::InvalidHorizons);
    }
    let world = decode_world(&request.branch_point, &request.config)?;
    if !world
        .settlements
        .iter()
        .any(|settlement| settlement.group_id == request.target_group_id)
    {
        return Err(CrnEfficiencyError::MissingTargetSettlement(
            request.target_group_id,
        ));
    }
    Ok(())
}

fn rebound_run(
    request: &CrnEfficiencyRequest,
    replicate_id: u32,
) -> Result<(SimConfig, M0RunContext, CanonicalSnapshot), CrnEfficiencyError> {
    let mut config = request.config.clone();
    config.world.replicate_id = replicate_id;
    let context = M0RunContext::new(config.world.master_seed, replicate_id);
    let mut metadata = request.branch_point.metadata.clone();
    metadata.replicate_id = replicate_id;
    let snapshot = match config.runner_selection() {
        SimulationRunnerSelection::M3p => {
            let restored = decode_m3p_snapshot(&request.branch_point.bytes, &request.config)?;
            let old_state = restored.state;
            let state = M3pReferenceState::from_parts(
                old_state.world().clone(),
                old_state.relationship_graph().clone(),
                old_state.next_agent_id(),
                &config,
                &context,
            )
            .map_err(|error| CrnEfficiencyError::InvalidConfig(error.to_string()))?;
            encode_m3p_snapshot(&state, &metadata)?
        }
        SimulationRunnerSelection::M3pPropagation => {
            let restored = decode_m3pprop_snapshot(&request.branch_point.bytes, &request.config)?;
            let old_state = restored.state;
            let old_base = old_state.base_state();
            let base_config = m3p_base_config(&config);
            let base = M3pReferenceState::from_parts(
                old_base.world().clone(),
                old_base.relationship_graph().clone(),
                old_base.next_agent_id(),
                &base_config,
                &context,
            )
            .map_err(|error| CrnEfficiencyError::InvalidConfig(error.to_string()))?;
            let state = M3pPropagationReferenceState::from_parts(
                base,
                old_state.propagation().clone(),
                &config,
            )
            .map_err(|error| CrnEfficiencyError::InvalidConfig(error.to_string()))?;
            encode_m3pprop_snapshot(&state, &metadata)?
        }
        _ => return Err(CrnEfficiencyError::UnsupportedProfile),
    };
    Ok((config, context, snapshot))
}

fn decode_world(
    snapshot: &CanonicalSnapshot,
    config: &SimConfig,
) -> Result<WorldState, CrnEfficiencyError> {
    match config.runner_selection() {
        SimulationRunnerSelection::M3p => Ok(decode_m3p_snapshot(&snapshot.bytes, config)?
            .state
            .world()
            .clone()),
        SimulationRunnerSelection::M3pPropagation => {
            Ok(decode_m3pprop_snapshot(&snapshot.bytes, config)?
                .state
                .world()
                .clone())
        }
        _ => Err(CrnEfficiencyError::UnsupportedProfile),
    }
}

fn extract_outcomes(
    day: &crate::counterfactual::BranchDayRecord,
    config: &SimConfig,
    target_group_id: GroupId,
    outcomes: [CrnOutcome; 5],
) -> Result<[f64; 5], CrnEfficiencyError> {
    let snapshot = day
        .snapshot
        .as_ref()
        .ok_or(CrnEfficiencyError::MissingSnapshot(day.day))?;
    let world = decode_world(snapshot, config)?;
    let alive_population = world.agents.iter().filter(|agent| agent.alive).count() as u64;
    let total_wealth = world.agents.iter().try_fold(0i128, |total, agent| {
        total.checked_add(i128::from(agent.wealth))
    });
    let total_wealth = total_wealth
        .ok_or(CrnEfficiencyError::ArithmeticOverflow)?
        .checked_add(
            world
                .settlements
                .iter()
                .try_fold(0i128, |total, settlement| {
                    total.checked_add(i128::from(settlement.treasury))
                })
                .ok_or(CrnEfficiencyError::ArithmeticOverflow)?,
        )
        .ok_or(CrnEfficiencyError::ArithmeticOverflow)?;
    let alive_health_sum = world
        .agents
        .iter()
        .filter(|agent| agent.alive)
        .map(|agent| f64::from(agent.health))
        .sum::<f64>();
    let target_alive = world
        .agents
        .iter()
        .filter(|agent| agent.alive && agent.group_id == target_group_id)
        .count() as u64;
    let target_agent_wealth = world.agents.iter().try_fold(0i128, |total, agent| {
        if agent.group_id == target_group_id {
            total.checked_add(i128::from(agent.wealth))
        } else {
            Some(total)
        }
    });
    let target_treasury = world
        .settlements
        .iter()
        .find(|settlement| settlement.group_id == target_group_id)
        .map(|settlement| i128::from(settlement.treasury))
        .ok_or(CrnEfficiencyError::MissingTargetSettlement(target_group_id))?;
    let target_wealth = target_agent_wealth
        .ok_or(CrnEfficiencyError::ArithmeticOverflow)?
        .checked_add(target_treasury)
        .ok_or(CrnEfficiencyError::ArithmeticOverflow)?;
    if alive_population == 0 && outcomes.contains(&CrnOutcome::MeanHealthOfAlive) {
        return Err(CrnEfficiencyError::UndefinedOutcome(
            CrnOutcome::MeanHealthOfAlive,
            day.day,
        ));
    }
    let mean_health = if alive_population == 0 {
        f64::NAN
    } else {
        alive_health_sum / alive_population as f64
    };
    let values = outcomes.map(|outcome| match outcome {
        CrnOutcome::AlivePopulation => alive_population as f64,
        CrnOutcome::TotalWealth => total_wealth as f64,
        CrnOutcome::MeanHealthOfAlive => mean_health,
        CrnOutcome::TargetSettlementAlivePopulation => target_alive as f64,
        CrnOutcome::TargetSettlementWealth => target_wealth as f64,
    });
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CrnEfficiencyError::UndefinedOutcome(
            CrnOutcome::MeanHealthOfAlive,
            day.day,
        ));
    }
    Ok(values)
}

fn find_day(
    days: &[crate::counterfactual::BranchDayRecord],
    day: u32,
) -> Result<&crate::counterfactual::BranchDayRecord, CrnEfficiencyError> {
    days.iter()
        .find(|record| record.day == day)
        .ok_or(CrnEfficiencyError::MissingDay(day))
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn sample_variance(values: &[f64]) -> f64 {
    let average = mean(values);
    values
        .iter()
        .map(|value| {
            let difference = value - average;
            difference * difference
        })
        .sum::<f64>()
        / (values.len() - 1) as f64
}

fn percentile_95(values: &mut [f64]) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    (values[249], values[9_749])
}

fn bootstrap_intervals(
    paired_base: &[f64],
    paired_deltas: &[f64],
    independent_intervention: &[f64],
    observed_paired_variance_zero: bool,
) -> (f64, f64, CrnVarianceRatio) {
    let n = paired_deltas.len();
    let mut state = BOOTSTRAP_SEED;
    let mut means = Vec::with_capacity(BOOTSTRAP_RESAMPLES);
    let mut ratios = Vec::with_capacity(BOOTSTRAP_RESAMPLES);
    for _ in 0..BOOTSTRAP_RESAMPLES {
        let paired_indices: Vec<_> = (0..n).map(|_| next_u64(&mut state) as usize % n).collect();
        let independent_indices: Vec<_> =
            (0..n).map(|_| next_u64(&mut state) as usize % n).collect();
        let paired_sample: Vec<_> = paired_indices
            .iter()
            .map(|&index| paired_deltas[index])
            .collect();
        let paired_base_sample: Vec<_> = paired_indices
            .iter()
            .map(|&index| paired_base[index])
            .collect();
        let independent_intervention_sample: Vec<_> = independent_indices
            .iter()
            .map(|&index| independent_intervention[index])
            .collect();
        means.push(mean(&paired_sample));
        let paired_variance = sample_variance(&paired_sample);
        let independent_variance = (sample_variance(&paired_base_sample)
            + sample_variance(&independent_intervention_sample))
            / n as f64;
        ratios.push(if paired_variance == 0.0 {
            f64::INFINITY
        } else {
            independent_variance / (paired_variance / n as f64)
        });
    }
    let (mean_lower, mean_upper) = percentile_95(&mut means);
    if observed_paired_variance_zero {
        return (
            mean_lower,
            mean_upper,
            CrnVarianceRatio::ExactPairedVarianceZero,
        );
    }
    let (ratio_lower, ratio_upper) = percentile_95(&mut ratios);
    let mean_ratio = {
        let independent_variance =
            (sample_variance(paired_base) + sample_variance(independent_intervention)) / n as f64;
        let paired_variance = sample_variance(paired_deltas) / n as f64;
        independent_variance / paired_variance
    };
    (
        mean_lower,
        mean_upper,
        CrnVarianceRatio::Estimated {
            ratio: mean_ratio,
            ci95_lower: ratio_lower,
            ci95_upper: ratio_upper,
        },
    )
}

fn next_u64(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::counterfactual::run_counterfactual_branch;
    use crate::m3p::{M3pReferenceState, encode_m3p_snapshot};
    use crate::m3pprop::{
        M3pPropagationReferenceState, decode_m3pprop_snapshot, encode_m3pprop_snapshot,
    };
    use crate::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION};
    use crate::snapshot::SnapshotMetadata;
    use crate::{SimulationConfig, SimulationProfile, SimulationSystems, initialize_world};
    use rayon::ThreadPoolBuilder;

    fn small_request() -> CrnEfficiencyRequest {
        let mut config = SimConfig::parse_and_validate(include_str!(
            "../tests/fixtures/m3p_propagation_sustained.toml"
        ))
        .unwrap();
        config.world.initial_population = 300;
        config.world.settlement_count = 3;
        config.world.master_seed = 81_985_529_216_486_895;
        config.world.replicate_id = 7;
        config.profile = SimulationProfile::M3p;
        config.simulation = None;
        config.propagation = None;
        config.validate().unwrap();
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let world = initialize_world(&config).unwrap();
        let metadata = SnapshotMetadata::new(
            world.current_day.as_u32(),
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let state = M3pReferenceState::new(world, &config, &context).unwrap();
        CrnEfficiencyRequest {
            config,
            branch_point: encode_m3p_snapshot(&state, &metadata).unwrap(),
            intervention: BranchIntervention::AgentFieldOverride {
                agent_id: sim_core::AgentId(0),
                field: crate::counterfactual::AgentField::Food,
                value: crate::counterfactual::AgentFieldValue::Food(0.0),
            },
            target_group_id: GroupId(0),
            horizons: vec![7],
            replicate_count: 2,
        }
    }

    fn small_m3pprop_request() -> CrnEfficiencyRequest {
        let mut config = SimConfig::parse_and_validate(include_str!(
            "../tests/fixtures/m3p_propagation_sustained.toml"
        ))
        .unwrap();
        config.world.initial_population = 300;
        config.world.settlement_count = 3;
        config.world.master_seed = 81_985_529_216_486_895;
        config.world.replicate_id = 7;
        config.profile = SimulationProfile::M3p;
        config.simulation = Some(SimulationConfig {
            systems: SimulationSystems {
                relationship_graph: true,
                population_dynamics: true,
                propagation: true,
            },
        });
        config.validate().unwrap();
        let context = M0RunContext::new(config.world.master_seed, config.world.replicate_id);
        let world = initialize_world(&config).unwrap();
        let metadata = SnapshotMetadata::new(
            world.current_day.as_u32(),
            context.master_seed,
            context.replicate_id,
            DEFAULT_MODEL_VERSION,
            DEFAULT_CONFIG_VERSION,
        );
        let state = M3pPropagationReferenceState::new(world, &config, &context).unwrap();
        CrnEfficiencyRequest {
            config,
            branch_point: encode_m3pprop_snapshot(&state, &metadata).unwrap(),
            intervention: BranchIntervention::AgentFieldOverride {
                agent_id: sim_core::AgentId(0),
                field: crate::counterfactual::AgentField::Food,
                value: crate::counterfactual::AgentFieldValue::Food(0.0),
            },
            target_group_id: GroupId(0),
            horizons: vec![7],
            replicate_count: 2,
        }
    }

    #[test]
    fn replicate_sets_are_deterministic_and_independent_samples_are_disjoint() {
        let ids = replicate_id_sets(100);
        assert_eq!(ids.paired, (1..=100).collect::<Vec<_>>());
        assert_eq!(ids.independent_base, ids.paired);
        assert_eq!(
            ids.independent_intervention,
            (100_001..=100_100).collect::<Vec<_>>()
        );
        assert!(
            ids.independent_base
                .iter()
                .all(|id| !ids.independent_intervention.contains(id))
        );
    }

    #[test]
    fn exact_zero_variance_has_no_finite_ratio() {
        let base = [1.0, 2.0, 3.0, 4.0];
        let delta = [0.0; 4];
        let independent = [8.0, 9.0, 10.0, 11.0];
        let (lower, upper, ratio) = bootstrap_intervals(&base, &delta, &independent, true);
        assert_eq!((lower, upper), (0.0, 0.0));
        assert_eq!(ratio, CrnVarianceRatio::ExactPairedVarianceZero);
    }

    #[test]
    fn no_op_replicate_set_uses_exact_zero_variance_reporting() {
        let mut request = small_request();
        request.intervention = BranchIntervention::ConfigOverride {
            parameter: crate::counterfactual::ConfigParameter::BaseMetabolicCost,
            value: crate::counterfactual::ConfigValue::F32(
                request.config.environment.base_metabolic_cost,
            ),
        };
        let result = run_crn_efficiency(&request).unwrap();
        assert!(result.estimates.iter().all(|estimate| {
            estimate.paired_sample_variance == 0.0
                && estimate.variance_ratio == CrnVarianceRatio::ExactPairedVarianceZero
        }));
    }

    #[test]
    fn bootstrap_interval_is_reproducible() {
        let base = [1.0, 2.0, 4.0, 5.0, 8.0];
        let delta = [0.0, 1.0, -1.0, 3.0, 2.0];
        let independent = [2.0, 9.0, 3.0, 8.0, 1.0];
        assert_eq!(
            bootstrap_intervals(&base, &delta, &independent, false),
            bootstrap_intervals(&base, &delta, &independent, false)
        );
    }

    #[test]
    fn paired_api_output_matches_m4_02_branch_result() {
        let request = small_request();
        let (config, context, snapshot) = rebound_run(&request, 1).unwrap();
        let branch_request = BranchRequest {
            config,
            run_context: context,
            snapshot,
            intervention: request.intervention,
            horizon_days: 7,
            execution: crate::counterfactual::BranchExecutionOption::Serial,
            snapshot_days: vec![6],
        };
        let direct = run_counterfactual_branch(&branch_request).unwrap();
        let paired = run_counterfactual_branch_with_divergence_series(&branch_request).unwrap();
        assert_eq!(paired.branch, direct);
        assert_eq!(paired.daily_series.len(), 7);
    }

    #[test]
    fn parallel_replicate_sets_match_serial_and_independent_branch_only_records() {
        let request = small_request();
        let serial = run_crn_efficiency(&request).unwrap();
        let pool = ThreadPoolBuilder::new().num_threads(6).build().unwrap();
        let parallel =
            run_crn_efficiency_parallel(&request, &pool, NonZeroUsize::new(2).unwrap()).unwrap();
        assert_eq!(parallel, serial);

        let (config, context, snapshot) = rebound_run(&request, 100_001).unwrap();
        let branch_request = BranchRequest {
            config,
            run_context: context,
            snapshot,
            intervention: request.intervention,
            horizon_days: 7,
            execution: crate::counterfactual::BranchExecutionOption::Serial,
            snapshot_days: vec![6],
        };
        let full = run_counterfactual_branch(&branch_request).unwrap();
        let branch_only = run_counterfactual_intervention_only(&branch_request).unwrap();
        assert_eq!(branch_only, full.branch_days);
    }

    #[test]
    fn replicate_rebinding_preserves_the_branch_point_graph_and_world() {
        let request = small_request();
        let original = decode_m3p_snapshot(&request.branch_point.bytes, &request.config).unwrap();
        let (config, _, rebound) = rebound_run(&request, 2).unwrap();
        assert_eq!(rebound.metadata.replicate_id, 2);
        let rebound = decode_m3p_snapshot(&rebound.bytes, &config).unwrap();
        assert_eq!(
            original.state.relationship_graph().csr_hash(),
            rebound.state.relationship_graph().csr_hash()
        );
        assert_eq!(original.state.world(), rebound.state.world());
        assert_eq!(
            original.state.next_agent_id(),
            rebound.state.next_agent_id()
        );
    }

    #[test]
    fn m3p_propagation_rebinding_preserves_graph_and_propagation_rows() {
        let request = small_m3pprop_request();
        let original =
            decode_m3pprop_snapshot(&request.branch_point.bytes, &request.config).unwrap();
        let (config, _, rebound) = rebound_run(&request, 2).unwrap();
        let rebound = decode_m3pprop_snapshot(&rebound.bytes, &config).unwrap();
        assert_eq!(
            original.state.relationship_graph().csr_hash(),
            rebound.state.relationship_graph().csr_hash()
        );
        assert_eq!(original.state.world(), rebound.state.world());
        assert_eq!(original.state.propagation(), rebound.state.propagation());
        assert_eq!(
            original.state.base_state().next_agent_id(),
            rebound.state.base_state().next_agent_id()
        );
    }
}
