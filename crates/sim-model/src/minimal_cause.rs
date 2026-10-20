//! Deterministic minimal-cause search over M4-05 intervention batches.

use crate::config::{SimConfig, SimulationRunnerSelection};
use crate::counterfactual::{
    AgentField, AgentFieldValue, BranchDayRecord, BranchError, BranchIntervention,
    BranchInterventionBatchRequest, ConfigParameter, ConfigValue, IdentifiedIntervention,
    rebound_counterfactual_run, run_counterfactual_base_days, run_counterfactual_batch_branch_days,
    validate_identified_batch,
};
use crate::crn_efficiency::CrnOutcome;
use crate::m3p::decode_m3p_snapshot;
use crate::snapshot::{CanonicalSnapshot, decode_snapshot};
use crate::state::WorldState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sim_core::GroupId;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectComparison {
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MinimalCausePredicate {
    pub outcome: CrnOutcome,
    /// `None` means world scope. Settlement-scoped outcomes require a group.
    pub target_group_id: Option<GroupId>,
    pub comparison: EffectComparison,
    pub threshold: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinimalCauseGuarantee {
    Minimum,
    OneMinimal,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MinimalCauseStatus {
    Complete,
    Incomplete,
    NoCause,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MinimalCauseRequest {
    pub batch: BranchInterventionBatchRequest,
    pub candidates: Vec<IdentifiedIntervention>,
    pub predicate: MinimalCausePredicate,
    /// Paired CRN replicate IDs, normally 1 through 10. A single ID is exploratory.
    pub replicate_ids: Vec<u32>,
    pub exploratory_single_replicate: bool,
    pub max_branch_evaluations: u64,
    pub max_wall_time: Duration,
    pub evaluation_log_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinimalCauseResult {
    pub status: MinimalCauseStatus,
    pub cause_ids: Vec<String>,
    pub guarantee: MinimalCauseGuarantee,
    pub exploratory_single_replicate: bool,
    pub evaluation_count: u64,
    pub mean_effect: Option<f64>,
    pub predicate_value: bool,
    pub stop_reason: Option<String>,
    pub evaluation_log_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinimalCauseError {
    InvalidRequest(String),
    Branch(String),
    Outcome(String),
    Log(String),
    Recorded(String),
}

impl std::fmt::Display for MinimalCauseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "invalid minimal-cause request: {message}"),
            Self::Branch(message) => write!(f, "minimal-cause branch failed: {message}"),
            Self::Outcome(message) => write!(f, "minimal-cause outcome failed: {message}"),
            Self::Log(message) => write!(f, "minimal-cause log failed: {message}"),
            Self::Recorded(message) => {
                write!(f, "recorded minimal-cause evaluation failed: {message}")
            }
        }
    }
}

impl std::error::Error for MinimalCauseError {}

impl From<BranchError> for MinimalCauseError {
    fn from(error: BranchError) -> Self {
        Self::Branch(error.to_string())
    }
}

pub fn build_agent_field_candidates(
    world: &WorldState,
    agent_ids: &[sim_core::AgentId],
    target_group_id: Option<GroupId>,
    field: AgentField,
    values: &[AgentFieldValue],
) -> Result<Vec<IdentifiedIntervention>, MinimalCauseError> {
    let mut candidates = Vec::new();
    for &agent_id in agent_ids {
        let Some(agent) = world.agents.iter().find(|agent| agent.agent_id == agent_id) else {
            continue;
        };
        if target_group_id.is_some_and(|group_id| agent.group_id != group_id) {
            continue;
        }
        candidates.extend(
            values
                .iter()
                .copied()
                .map(|value| IdentifiedIntervention::agent_field_override(agent_id, field, value)),
        );
    }
    sort_candidates(candidates)
}

pub fn build_config_candidates(
    overrides: &[(ConfigParameter, ConfigValue)],
) -> Result<Vec<IdentifiedIntervention>, MinimalCauseError> {
    sort_candidates(
        overrides
            .iter()
            .copied()
            .map(|(parameter, value)| IdentifiedIntervention::config_override(parameter, value))
            .collect(),
    )
}

pub fn build_forced_action_candidates(
    agent_ids: &[sim_core::AgentId],
    day: u32,
    actions: &[crate::decision::Action],
) -> Result<Vec<IdentifiedIntervention>, MinimalCauseError> {
    sort_candidates(
        agent_ids
            .iter()
            .flat_map(|&agent_id| {
                actions
                    .iter()
                    .copied()
                    .map(move |action| IdentifiedIntervention::forced_action(agent_id, day, action))
            })
            .collect(),
    )
}

fn sort_candidates(
    mut candidates: Vec<IdentifiedIntervention>,
) -> Result<Vec<IdentifiedIntervention>, MinimalCauseError> {
    candidates.sort_by(|left, right| left.id.as_bytes().cmp(right.id.as_bytes()));
    if candidates.windows(2).any(|pair| pair[0].id == pair[1].id) {
        return Err(MinimalCauseError::InvalidRequest(
            "candidate builder produced duplicate intervention IDs".into(),
        ));
    }
    Ok(candidates)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LogRecord {
    Header {
        schema: u32,
        fingerprint: String,
    },
    Base {
        replicate_id: u32,
        outcome: Option<f64>,
        error: Option<String>,
    },
    Evaluation {
        subset_ids: Vec<String>,
        per_replicate_effects: Vec<f64>,
        mean_effect: Option<f64>,
        predicate_value: bool,
        branch_runs: u64,
        error: Option<String>,
        fatal_error: bool,
    },
    Result {
        max_branch_evaluations: u64,
        max_wall_time_ns: String,
        result: MinimalCauseResult,
    },
}

#[derive(Default)]
struct CachedLog {
    base: BTreeMap<u32, f64>,
    evaluations: BTreeMap<Vec<String>, CachedEvaluation>,
    base_errors: BTreeMap<u32, String>,
    evaluation_errors: BTreeMap<Vec<String>, String>,
    results: Vec<(u64, String, MinimalCauseResult)>,
    exists: bool,
}

struct CachedEvaluation {
    mean: Option<f64>,
    predicate: bool,
    branch_runs: u64,
}

/// Runs exhaustive minimum search for at most ten candidates and deterministic ddmin above ten.
pub fn search_minimal_cause(
    request: &MinimalCauseRequest,
) -> Result<MinimalCauseResult, MinimalCauseError> {
    validate_request(request)?;
    let fingerprint = request_fingerprint(request)?;
    let mut cache = load_cache(&request.evaluation_log_path, &fingerprint)?;
    let max_wall_time_ns = request.max_wall_time.as_nanos().to_string();
    if let Some((_, _, result)) = cache.results.iter().rev().find(|(evaluations, wall, _)| {
        *evaluations == request.max_branch_evaluations && *wall == max_wall_time_ns
    }) {
        return Ok(result.clone());
    }
    if !cache.exists {
        append_log(
            &request.evaluation_log_path,
            &LogRecord::Header {
                schema: 1,
                fingerprint,
            },
        )?;
        cache.exists = true;
    }

    let start = Instant::now();
    let mut evaluations = 0_u64;
    let mut bases = BTreeMap::new();
    let final_day = request
        .batch
        .snapshot
        .metadata
        .day
        .checked_add(request.batch.horizon_days.saturating_sub(1))
        .ok_or_else(|| MinimalCauseError::InvalidRequest("day overflow".into()))?;
    let snapshot_days = [final_day];
    for &replicate_id in &request.replicate_ids {
        if let Some(error) = cache.base_errors.get(&replicate_id) {
            return Err(MinimalCauseError::Recorded(error.clone()));
        }
        if let Some(value) = cache.base.get(&replicate_id) {
            if let Some(reason) = limit_reason(request, evaluations, start) {
                return finish_result(
                    request,
                    &mut cache,
                    Vec::new(),
                    MinimalCauseGuarantee::None,
                    evaluations,
                    None,
                    false,
                    Some(reason.into()),
                );
            }
            evaluations += 1;
            bases.insert(replicate_id, *value);
            continue;
        }
        if let Some(reason) = limit_reason(request, evaluations, start) {
            return finish_result(
                request,
                &mut cache,
                Vec::new(),
                MinimalCauseGuarantee::None,
                evaluations,
                None,
                false,
                Some(reason.into()),
            );
        }
        let outcome = (|| {
            let (config, context, snapshot) = rebound_counterfactual_run(
                &request.batch.snapshot,
                &request.batch.config,
                replicate_id,
            )?;
            let days = run_counterfactual_base_days(
                &snapshot,
                &config,
                &context,
                request.batch.horizon_days,
                &snapshot_days,
            )?;
            let record = days.last().ok_or_else(|| {
                MinimalCauseError::InvalidRequest("horizon must contain at least one day".into())
            })?;
            extract_one(
                record,
                &config,
                request.predicate.target_group_id,
                request.predicate.outcome,
            )
        })();
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                let message = error.to_string();
                append_log(
                    &request.evaluation_log_path,
                    &LogRecord::Base {
                        replicate_id,
                        outcome: None,
                        error: Some(message.clone()),
                    },
                )?;
                cache.base_errors.insert(replicate_id, message);
                return Err(error);
            }
        };
        append_log(
            &request.evaluation_log_path,
            &LogRecord::Base {
                replicate_id,
                outcome: Some(outcome),
                error: None,
            },
        )?;
        evaluations += 1;
        cache.base.insert(replicate_id, outcome);
        bases.insert(replicate_id, outcome);
    }

    let mut search_state = SearchState {
        request,
        candidates: {
            let mut candidates = request.candidates.clone();
            candidates.sort_by(|left, right| left.id.as_bytes().cmp(right.id.as_bytes()));
            candidates
        },
        cache: &mut cache,
        bases,
        start,
        evaluations,
        best: None,
    };
    let (cause, guarantee, mean_effect, predicate_value, stop_reason) =
        if request.candidates.len() <= 10 {
            search_state.exhaustive()?
        } else {
            search_state.ddmin()?
        };
    finish_result(
        request,
        search_state.cache,
        cause,
        guarantee,
        search_state.evaluations,
        mean_effect,
        predicate_value,
        stop_reason,
    )
}

struct SearchState<'a> {
    request: &'a MinimalCauseRequest,
    candidates: Vec<IdentifiedIntervention>,
    cache: &'a mut CachedLog,
    bases: BTreeMap<u32, f64>,
    start: Instant,
    evaluations: u64,
    best: Option<(Vec<IdentifiedIntervention>, f64)>,
}

type SearchProgress = (
    Vec<String>,
    MinimalCauseGuarantee,
    Option<f64>,
    bool,
    Option<String>,
);

impl SearchState<'_> {
    fn exhaustive(&mut self) -> Result<SearchProgress, MinimalCauseError> {
        for size in 0..=self.candidates.len() {
            let mut subsets = Vec::new();
            combinations(
                self.candidates.len(),
                size,
                0,
                &mut Vec::new(),
                &mut subsets,
            );
            for indices in subsets {
                let subset = indices
                    .into_iter()
                    .map(|index| self.candidates[index].clone())
                    .collect::<Vec<_>>();
                let (mean, matches, stop) = self.evaluate(&subset)?;
                if matches {
                    self.update_best(&subset, mean.unwrap_or(0.0));
                    return Ok((
                        ids(&subset),
                        if stop.is_some() {
                            MinimalCauseGuarantee::None
                        } else {
                            MinimalCauseGuarantee::Minimum
                        },
                        mean,
                        true,
                        stop,
                    ));
                }
                if let Some(reason) = stop {
                    let (best, best_mean) = self.best.clone().unwrap_or_default();
                    return Ok((
                        ids(&best),
                        MinimalCauseGuarantee::None,
                        Some(best_mean),
                        self.best.is_some(),
                        Some(reason),
                    ));
                }
            }
        }
        Ok((Vec::new(), MinimalCauseGuarantee::None, None, false, None))
    }

    fn ddmin(&mut self) -> Result<SearchProgress, MinimalCauseError> {
        let mut current = self.candidates.clone();
        let (full_mean, full_matches, stop) = self.evaluate(&current)?;
        if full_matches {
            self.update_best(&current, full_mean.unwrap_or(0.0));
        }
        if let Some(reason) = stop {
            let (best, best_mean) = self.best.clone().unwrap_or_default();
            return Ok((
                ids(&best),
                MinimalCauseGuarantee::None,
                self.best.as_ref().map(|_| best_mean).or(full_mean),
                self.best.is_some(),
                Some(reason),
            ));
        }
        if !full_matches {
            return Ok((
                Vec::new(),
                MinimalCauseGuarantee::None,
                full_mean,
                false,
                None,
            ));
        }
        let mut granularity = 2_usize;
        while current.len() >= 2 {
            let chunks = chunks(current.len(), granularity);
            let mut reduced = false;
            for (begin, end) in chunks {
                let candidate = current
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| *index < begin || *index >= end)
                    .map(|(_, item)| item.clone())
                    .collect::<Vec<_>>();
                let (mean, matches, stop) = self.evaluate(&candidate)?;
                if matches {
                    self.update_best(&candidate, mean.unwrap_or(0.0));
                    current = candidate;
                    granularity = granularity.saturating_sub(1).max(2).min(current.len());
                    reduced = true;
                    break;
                }
                if let Some(reason) = stop {
                    let (best, best_mean) = self.best.clone().unwrap_or_default();
                    return Ok((
                        ids(&best),
                        MinimalCauseGuarantee::None,
                        Some(best_mean),
                        self.best.is_some(),
                        Some(reason),
                    ));
                }
            }
            if reduced {
                continue;
            }
            if granularity >= current.len() {
                break;
            }
            granularity = (granularity * 2).min(current.len());
        }
        loop {
            let mut reduced = false;
            for index in 0..current.len() {
                let candidate = current
                    .iter()
                    .enumerate()
                    .filter(|(candidate_index, _)| *candidate_index != index)
                    .map(|(_, item)| item.clone())
                    .collect::<Vec<_>>();
                let (mean, matches, stop) = self.evaluate(&candidate)?;
                if matches {
                    self.update_best(&candidate, mean.unwrap_or(0.0));
                    current = candidate;
                    reduced = true;
                    break;
                }
                if let Some(reason) = stop {
                    let (best, best_mean) = self.best.clone().unwrap_or_default();
                    return Ok((
                        ids(&best),
                        MinimalCauseGuarantee::None,
                        Some(best_mean),
                        self.best.is_some(),
                        Some(reason),
                    ));
                }
            }
            if !reduced {
                break;
            }
        }
        let (best, mean) = self.best.clone().unwrap_or_default();
        Ok((
            ids(&best),
            MinimalCauseGuarantee::OneMinimal,
            Some(mean),
            self.best.is_some(),
            None,
        ))
    }

    fn evaluate(
        &mut self,
        subset: &[IdentifiedIntervention],
    ) -> Result<(Option<f64>, bool, Option<String>), MinimalCauseError> {
        let subset_ids = ids(subset);
        let replicate_count = self.request.replicate_ids.len() as u64;
        let previous_evaluations = self.evaluations;
        if self.start.elapsed() >= self.request.max_wall_time {
            return Ok((None, false, Some("max_wall_time".into())));
        }
        if let Some(error) = self.cache.evaluation_errors.get(&subset_ids) {
            return Err(MinimalCauseError::Recorded(error.clone()));
        }
        if let Some(evaluation) = self.cache.evaluations.get(&subset_ids) {
            if evaluation.branch_runs > 0 {
                if previous_evaluations.saturating_add(evaluation.branch_runs)
                    > self.request.max_branch_evaluations
                {
                    return Ok((None, false, Some("max_branch_evaluations".into())));
                }
                if let Some(reason) = limit_reason(self.request, previous_evaluations, self.start) {
                    return Ok((None, false, Some(reason.into())));
                }
                self.evaluations += evaluation.branch_runs;
            }
            return Ok((evaluation.mean, evaluation.predicate, None));
        }

        if has_duplicate_targets(subset) {
            let error = "invalid candidate subset: duplicate intervention target".to_owned();
            append_log(
                &self.request.evaluation_log_path,
                &LogRecord::Evaluation {
                    subset_ids: subset_ids.clone(),
                    per_replicate_effects: Vec::new(),
                    mean_effect: None,
                    predicate_value: false,
                    branch_runs: 0,
                    error: Some(error),
                    fatal_error: false,
                },
            )?;
            self.cache.evaluations.insert(
                subset_ids,
                CachedEvaluation {
                    mean: None,
                    predicate: false,
                    branch_runs: 0,
                },
            );
            return Ok((None, false, None));
        }
        if !subset.is_empty() {
            validate_identified_batch(subset).map_err(MinimalCauseError::from)?;
        }
        if previous_evaluations.saturating_add(replicate_count)
            > self.request.max_branch_evaluations
        {
            return Ok((None, false, Some("max_branch_evaluations".into())));
        }
        if let Some(reason) = limit_reason(self.request, previous_evaluations, self.start) {
            return Ok((None, false, Some(reason.into())));
        }
        self.evaluations += replicate_count;

        let evaluation = (|| {
            let mut effects = Vec::new();
            let final_day =
                self.request.batch.snapshot.metadata.day + self.request.batch.horizon_days - 1;
            for &replicate_id in &self.request.replicate_ids {
                let base = *self.bases.get(&replicate_id).ok_or_else(|| {
                    MinimalCauseError::InvalidRequest(format!(
                        "missing base cache for replicate {replicate_id}"
                    ))
                })?;
                let branch = if subset.is_empty() {
                    base
                } else {
                    let (config, context, snapshot) = rebound_counterfactual_run(
                        &self.request.batch.snapshot,
                        &self.request.batch.config,
                        replicate_id,
                    )?;
                    let batch = BranchInterventionBatchRequest {
                        snapshot,
                        config: config.clone(),
                        run_context: context,
                        interventions: subset.to_vec(),
                        horizon_days: self.request.batch.horizon_days,
                        snapshot_days: vec![final_day],
                    };
                    let (days, _) = run_counterfactual_batch_branch_days(&batch)?;
                    let record = days.last().ok_or_else(|| {
                        MinimalCauseError::InvalidRequest("empty branch horizon".into())
                    })?;
                    extract_one(
                        record,
                        &config,
                        self.request.predicate.target_group_id,
                        self.request.predicate.outcome,
                    )?
                };
                effects.push(branch - base);
            }
            Ok::<_, MinimalCauseError>(effects)
        })();
        let effects = match evaluation {
            Ok(effects) => effects,
            Err(error) => {
                let message = error.to_string();
                append_log(
                    &self.request.evaluation_log_path,
                    &LogRecord::Evaluation {
                        subset_ids: subset_ids.clone(),
                        per_replicate_effects: Vec::new(),
                        mean_effect: None,
                        predicate_value: false,
                        branch_runs: replicate_count,
                        error: Some(message.clone()),
                        fatal_error: true,
                    },
                )?;
                self.cache.evaluation_errors.insert(subset_ids, message);
                return Err(error);
            }
        };
        let mean =
            (!effects.is_empty()).then(|| effects.iter().sum::<f64>() / effects.len() as f64);
        let predicate_value =
            mean.is_some_and(|value| predicate_matches(&self.request.predicate, value));
        self.cache.evaluations.insert(
            subset_ids.clone(),
            CachedEvaluation {
                mean,
                predicate: predicate_value,
                branch_runs: replicate_count,
            },
        );
        append_log(
            &self.request.evaluation_log_path,
            &LogRecord::Evaluation {
                subset_ids,
                per_replicate_effects: effects,
                mean_effect: mean,
                predicate_value,
                branch_runs: replicate_count,
                error: None,
                fatal_error: false,
            },
        )?;
        Ok((mean, predicate_value, None))
    }

    fn update_best(&mut self, subset: &[IdentifiedIntervention], mean: f64) {
        let replace = self.best.as_ref().is_none_or(|(best, _)| {
            subset.len() < best.len() || (subset.len() == best.len() && ids(subset) < ids(best))
        });
        if replace {
            self.best = Some((subset.to_vec(), mean));
        }
    }
}

fn validate_request(request: &MinimalCauseRequest) -> Result<(), MinimalCauseError> {
    if !matches!(
        request.batch.config.runner_selection(),
        SimulationRunnerSelection::M3 | SimulationRunnerSelection::M3p
    ) {
        return Err(MinimalCauseError::InvalidRequest(
            "only serial m3 and m3p profiles are supported".into(),
        ));
    }
    if request.batch.horizon_days == 0
        || request.max_branch_evaluations == 0
        || request.max_wall_time.is_zero()
        || request.replicate_ids.is_empty()
        || !request.predicate.threshold.is_finite()
    {
        return Err(MinimalCauseError::InvalidRequest(
            "horizon, limits, candidates, replicates, and predicate must be non-empty/valid".into(),
        ));
    }
    if request.exploratory_single_replicate {
        if request.replicate_ids.len() != 1 {
            return Err(MinimalCauseError::InvalidRequest(
                "exploratory mode requires exactly one replicate".into(),
            ));
        }
    } else if request.replicate_ids != (1..=10).collect::<Vec<_>>() {
        return Err(MinimalCauseError::InvalidRequest(
            "paired CRN mode requires replicate IDs 1 through 10".into(),
        ));
    }
    if request
        .replicate_ids
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(MinimalCauseError::InvalidRequest(
            "replicate IDs must be unique and ascending".into(),
        ));
    }
    if !request.batch.interventions.is_empty() {
        return Err(MinimalCauseError::InvalidRequest(
            "batch.interventions must be empty; candidates define the searched intervention set"
                .into(),
        ));
    }
    if !request.batch.snapshot_days.is_empty() {
        return Err(MinimalCauseError::InvalidRequest(
            "batch.snapshot_days is set by the search horizon".into(),
        ));
    }
    if request.batch.snapshot.metadata.master_seed != request.batch.config.world.master_seed
        || request.batch.snapshot.metadata.replicate_id != request.batch.config.world.replicate_id
        || request.batch.run_context.master_seed != request.batch.config.world.master_seed
        || request.batch.run_context.replicate_id != request.batch.config.world.replicate_id
    {
        return Err(MinimalCauseError::InvalidRequest(
            "config, snapshot, and run context identities must match".into(),
        ));
    }
    if matches!(
        request.predicate.outcome,
        CrnOutcome::TargetSettlementAlivePopulation | CrnOutcome::TargetSettlementWealth
    ) && request.predicate.target_group_id.is_none()
    {
        return Err(MinimalCauseError::InvalidRequest(
            "settlement outcome requires target_group_id".into(),
        ));
    }
    let mut candidate_ids = BTreeSet::new();
    for candidate in &request.candidates {
        if candidate.id != candidate.canonical_id() || !candidate_ids.insert(candidate.id.as_str())
        {
            return Err(MinimalCauseError::InvalidRequest(
                "candidate IDs must be canonical and unique".into(),
            ));
        }
        match candidate.intervention {
            BranchIntervention::ForcedAction { .. }
                if candidate.day != Some(request.batch.snapshot.metadata.day) =>
            {
                return Err(MinimalCauseError::InvalidRequest(
                    "forced-action candidates must target the branch cursor day".into(),
                ));
            }
            BranchIntervention::ConfigOverride { .. }
            | BranchIntervention::AgentFieldOverride { .. }
                if candidate.day.is_some() =>
            {
                return Err(MinimalCauseError::InvalidRequest(
                    "only forced-action candidates may declare a day".into(),
                ));
            }
            _ => {}
        }
    }
    request
        .batch
        .config
        .validate()
        .map_err(|error| MinimalCauseError::InvalidRequest(error.to_string()))?;
    if let Some(group_id) = request.predicate.target_group_id {
        let world = decode_world(&request.batch.snapshot, &request.batch.config)?;
        if !world
            .settlements
            .iter()
            .any(|settlement| settlement.group_id == group_id)
        {
            return Err(MinimalCauseError::InvalidRequest(format!(
                "missing target settlement {group_id}"
            )));
        }
    }
    Ok(())
}

fn decode_world(
    snapshot: &CanonicalSnapshot,
    config: &SimConfig,
) -> Result<WorldState, MinimalCauseError> {
    match config.runner_selection() {
        SimulationRunnerSelection::M3 => Ok(decode_snapshot(snapshot.as_ref())
            .map_err(|error| MinimalCauseError::Outcome(error.to_string()))?
            .world),
        SimulationRunnerSelection::M3p => Ok(decode_m3p_snapshot(snapshot.as_ref(), config)
            .map_err(|error| MinimalCauseError::Outcome(error.to_string()))?
            .state
            .world()
            .clone()),
        _ => Err(MinimalCauseError::InvalidRequest(
            "only m3 and m3p are supported".into(),
        )),
    }
}

fn extract_one(
    day: &BranchDayRecord,
    config: &SimConfig,
    scope: Option<GroupId>,
    outcome: CrnOutcome,
) -> Result<f64, MinimalCauseError> {
    let world = decode_world(
        day.snapshot
            .as_ref()
            .ok_or_else(|| MinimalCauseError::Outcome("final snapshot missing".into()))?,
        config,
    )?;
    let alive = world
        .agents
        .iter()
        .filter(|agent| agent.alive && scope.is_none_or(|group| agent.group_id == group))
        .count();
    match outcome {
        CrnOutcome::AlivePopulation => Ok(alive as f64),
        CrnOutcome::TotalWealth => {
            let agents = world.agents.iter().try_fold(0_i128, |sum, agent| {
                if scope.is_none_or(|group| agent.group_id == group) {
                    sum.checked_add(i128::from(agent.wealth))
                } else {
                    Some(sum)
                }
            });
            let settlements = world
                .settlements
                .iter()
                .try_fold(0_i128, |sum, settlement| {
                    if scope.is_none_or(|group| settlement.group_id == group) {
                        sum.checked_add(i128::from(settlement.treasury))
                    } else {
                        Some(sum)
                    }
                });
            let total = agents
                .and_then(|agent_total| {
                    settlements
                        .and_then(|settlement_total| agent_total.checked_add(settlement_total))
                })
                .ok_or_else(|| MinimalCauseError::Outcome("wealth accumulation overflow".into()))?;
            Ok(total as f64)
        }
        CrnOutcome::MeanHealthOfAlive => {
            if alive == 0 {
                return Err(MinimalCauseError::Outcome(
                    "mean health is undefined with no living agents".into(),
                ));
            }
            let total = world
                .agents
                .iter()
                .filter(|agent| agent.alive && scope.is_none_or(|group| agent.group_id == group))
                .map(|agent| f64::from(agent.health))
                .sum::<f64>();
            Ok(total / alive as f64)
        }
        CrnOutcome::TargetSettlementAlivePopulation => Ok(world
            .agents
            .iter()
            .filter(|agent| agent.alive && scope == Some(agent.group_id))
            .count() as f64),
        CrnOutcome::TargetSettlementWealth => {
            let group = scope.ok_or_else(|| {
                MinimalCauseError::InvalidRequest(
                    "settlement outcome requires a settlement scope".into(),
                )
            })?;
            if !world
                .settlements
                .iter()
                .any(|settlement| settlement.group_id == group)
            {
                return Err(MinimalCauseError::Outcome(format!(
                    "missing target settlement {group}"
                )));
            }
            let agents = world
                .agents
                .iter()
                .try_fold(0_i128, |sum, agent| {
                    if agent.group_id == group {
                        sum.checked_add(i128::from(agent.wealth))
                    } else {
                        Some(sum)
                    }
                })
                .ok_or_else(|| {
                    MinimalCauseError::Outcome("target wealth accumulation overflow".into())
                })?;
            let treasury = world
                .settlements
                .iter()
                .find(|settlement| settlement.group_id == group)
                .map(|settlement| i128::from(settlement.treasury))
                .unwrap_or_default();
            Ok(agents.checked_add(treasury).ok_or_else(|| {
                MinimalCauseError::Outcome("target wealth accumulation overflow".into())
            })? as f64)
        }
    }
}

fn predicate_matches(predicate: &MinimalCausePredicate, mean_effect: f64) -> bool {
    match predicate.comparison {
        EffectComparison::LessThan => mean_effect < predicate.threshold,
        EffectComparison::LessThanOrEqual => mean_effect <= predicate.threshold,
        EffectComparison::GreaterThan => mean_effect > predicate.threshold,
        EffectComparison::GreaterThanOrEqual => mean_effect >= predicate.threshold,
    }
}

fn request_fingerprint(request: &MinimalCauseRequest) -> Result<String, MinimalCauseError> {
    let config = toml::to_string(&request.batch.config)
        .map_err(|error| MinimalCauseError::InvalidRequest(error.to_string()))?;
    let mut hash = Sha256::new();
    hash.update(b"SIMCIV_M4_05_REQUEST_V1\0");
    hash.update(config.as_bytes());
    hash.update(&request.batch.snapshot.bytes);
    hash.update(request.batch.snapshot.metadata.day.to_le_bytes());
    hash.update(request.batch.snapshot.metadata.master_seed.to_le_bytes());
    hash.update(request.batch.snapshot.metadata.replicate_id.to_le_bytes());
    hash.update(request.batch.run_context.master_seed.to_le_bytes());
    hash.update(request.batch.run_context.replicate_id.to_le_bytes());
    hash.update(request.batch.horizon_days.to_le_bytes());
    hash.update(request.predicate.outcome.to_string().as_bytes());
    hash.update([0]);
    hash.update(match request.predicate.comparison {
        EffectComparison::LessThan => b"<".as_slice(),
        EffectComparison::LessThanOrEqual => b"<=".as_slice(),
        EffectComparison::GreaterThan => b">".as_slice(),
        EffectComparison::GreaterThanOrEqual => b">=".as_slice(),
    });
    hash.update(
        request
            .predicate
            .target_group_id
            .map_or(u16::MAX, |id| id.0)
            .to_le_bytes(),
    );
    hash.update(request.predicate.threshold.to_bits().to_le_bytes());
    hash.update([u8::from(request.exploratory_single_replicate)]);
    for id in &request.replicate_ids {
        hash.update(id.to_le_bytes());
    }
    let mut candidates = request
        .candidates
        .iter()
        .map(|candidate| candidate.id.as_str())
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    for id in candidates {
        hash.update(id.as_bytes());
        hash.update([0]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn load_cache(path: &PathBuf, fingerprint: &str) -> Result<CachedLog, MinimalCauseError> {
    if !path.exists() {
        return Ok(CachedLog::default());
    }
    let content =
        fs::read_to_string(path).map_err(|error| MinimalCauseError::Log(error.to_string()))?;
    let mut cache = CachedLog {
        exists: true,
        ..CachedLog::default()
    };
    for (line_index, line) in content.lines().enumerate() {
        let record: LogRecord = serde_json::from_str(line)
            .map_err(|error| MinimalCauseError::Log(format!("line {}: {error}", line_index + 1)))?;
        match record {
            LogRecord::Header {
                schema: 1,
                fingerprint: actual,
            } if line_index == 0 && actual == fingerprint => {}
            LogRecord::Header { .. } => {
                return Err(MinimalCauseError::Log(
                    "request fingerprint or log schema mismatch".into(),
                ));
            }
            LogRecord::Base {
                replicate_id,
                outcome,
                error,
            } => {
                if let Some(outcome) = outcome {
                    cache.base.insert(replicate_id, outcome);
                }
                if let Some(error) = error {
                    cache.base_errors.insert(replicate_id, error);
                }
            }
            LogRecord::Evaluation {
                subset_ids,
                per_replicate_effects: _,
                mean_effect,
                predicate_value,
                branch_runs,
                error,
                fatal_error,
            } => {
                if fatal_error {
                    cache
                        .evaluation_errors
                        .insert(subset_ids, error.unwrap_or_default());
                } else {
                    cache.evaluations.insert(
                        subset_ids,
                        CachedEvaluation {
                            mean: mean_effect,
                            predicate: predicate_value,
                            branch_runs,
                        },
                    );
                }
            }
            LogRecord::Result {
                max_branch_evaluations,
                max_wall_time_ns,
                result,
            } => cache
                .results
                .push((max_branch_evaluations, max_wall_time_ns, result)),
        }
    }
    if content.lines().next().is_none() {
        return Err(MinimalCauseError::Log("evaluation log is empty".into()));
    }
    Ok(cache)
}

fn append_log(path: &PathBuf, record: &LogRecord) -> Result<(), MinimalCauseError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|error| MinimalCauseError::Log(error.to_string()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| MinimalCauseError::Log(error.to_string()))?;
    serde_json::to_writer(&mut file, record)
        .map_err(|error| MinimalCauseError::Log(error.to_string()))?;
    file.write_all(b"\n")
        .map_err(|error| MinimalCauseError::Log(error.to_string()))?;
    file.flush()
        .map_err(|error| MinimalCauseError::Log(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn finish_result(
    request: &MinimalCauseRequest,
    cache: &mut CachedLog,
    cause_ids: Vec<String>,
    guarantee: MinimalCauseGuarantee,
    evaluation_count: u64,
    mean_effect: Option<f64>,
    predicate_value: bool,
    stop_reason: Option<String>,
) -> Result<MinimalCauseResult, MinimalCauseError> {
    let status = if stop_reason.is_some() {
        MinimalCauseStatus::Incomplete
    } else if predicate_value {
        MinimalCauseStatus::Complete
    } else {
        MinimalCauseStatus::NoCause
    };
    let result = MinimalCauseResult {
        status,
        cause_ids,
        guarantee,
        exploratory_single_replicate: request.exploratory_single_replicate,
        evaluation_count,
        mean_effect,
        predicate_value,
        stop_reason,
        evaluation_log_path: request.evaluation_log_path.to_string_lossy().into_owned(),
    };
    let max_wall_time_ns = request.max_wall_time.as_nanos().to_string();
    append_log(
        &request.evaluation_log_path,
        &LogRecord::Result {
            max_branch_evaluations: request.max_branch_evaluations,
            max_wall_time_ns: max_wall_time_ns.clone(),
            result: result.clone(),
        },
    )?;
    cache.results.push((
        request.max_branch_evaluations,
        max_wall_time_ns,
        result.clone(),
    ));
    Ok(result)
}

fn limit_reason(request: &MinimalCauseRequest, used: u64, start: Instant) -> Option<&'static str> {
    if used >= request.max_branch_evaluations {
        Some("max_branch_evaluations")
    } else if start.elapsed() >= request.max_wall_time {
        Some("max_wall_time")
    } else {
        None
    }
}

fn has_duplicate_targets(subset: &[IdentifiedIntervention]) -> bool {
    let mut targets = BTreeSet::new();
    subset
        .iter()
        .any(|item| !targets.insert(item.duplicate_target_key()))
}

fn ids(subset: &[IdentifiedIntervention]) -> Vec<String> {
    let mut ids = subset
        .iter()
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    ids
}

pub(crate) fn combinations(
    count: usize,
    remaining: usize,
    start: usize,
    current: &mut Vec<usize>,
    output: &mut Vec<Vec<usize>>,
) {
    if remaining == 0 {
        output.push(current.clone());
        return;
    }
    for index in start..=count.saturating_sub(remaining) {
        current.push(index);
        combinations(count, remaining - 1, index + 1, current, output);
        current.pop();
    }
}

fn chunks(length: usize, count: usize) -> Vec<(usize, usize)> {
    let chunk_size = length.div_ceil(count).max(1);
    (0..length)
        .step_by(chunk_size)
        .map(|start| (start, (start + chunk_size).min(length)))
        .collect()
}

#[cfg(test)]
#[path = "minimal_cause_search_tests.rs"]
mod search_tests;
