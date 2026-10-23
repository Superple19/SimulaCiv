use rayon::ThreadPoolBuilder;
use serde_json::{Value, json};
use sim_core::{AgentId, GroupId};
use sim_model::counterfactual::{
    AgentField, AgentFieldValue, BranchAnalysisMode, BranchAnalysisOutput, BranchDayScanDigestMode,
    BranchExecutionOption, BranchIntervention, BranchRequest, ConfigParameter, ConfigValue,
    run_counterfactual_branch_with_analysis_mode, run_counterfactual_branch_with_direct_day_scan,
    run_counterfactual_branch_with_divergence_series,
};
use sim_model::crn_efficiency::{
    CrnEfficiencyRequest, run_crn_efficiency, run_crn_efficiency_parallel,
};
use sim_model::m3p::{M3pReferenceState, encode_m3p_snapshot};
use sim_model::minimal_cause::{
    EffectComparison, MinimalCausePredicate, MinimalCauseRequest, build_agent_field_candidates,
    search_minimal_cause, search_minimal_cause_parallel,
};
use sim_model::runner::{DEFAULT_CONFIG_VERSION, DEFAULT_MODEL_VERSION};
use sim_model::snapshot::SnapshotMetadata;
use sim_model::{M0RunContext, SimConfig, SimulationProfile, initialize_world};
use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FIXTURE: &str = include_str!("../tests/fixtures/m3p_propagation_sustained.toml");
const SEED: u64 = 81_985_529_216_486_895;
const REPLICATE: u32 = 7;
const GOLDEN_HORIZONS: [u32; 4] = [7, 30, 90, 365];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let only = std::env::args()
        .skip(1)
        .find_map(|arg| arg.strip_prefix("--only=").map(str::to_owned));
    let output = PathBuf::from(match only.as_deref() {
        Some("crn") => "docs/data/m4_07/crn_parallel.jsonl",
        Some("crn-parallel") => "docs/data/m4_07/crn_parallel_only.jsonl",
        Some("scan") => "docs/data/m4_07/day_scan.jsonl",
        Some("search") => "docs/data/m4_07/minimal_cause_final.jsonl",
        Some("cow") => "docs/data/m4_07/clone_share.jsonl",
        _ => "docs/data/m4_07/many_branch.jsonl",
    });
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(fs::File::create(&output)?);
    serde_json::to_writer(&mut writer, &header())?;
    writeln!(writer)?;
    writer.flush()?;

    if selected(&only, "crn") {
        measure_crn(&output, false)?;
    }
    if only.as_deref() == Some("crn-parallel") {
        measure_crn(&output, true)?;
    }
    if selected(&only, "scan") {
        measure_day_scans(&output)?;
    }
    if selected(&only, "search") {
        measure_minimal_cause(&output)?;
    }
    if selected(&only, "cow") {
        measure_clone_share(&output)?;
    }
    Ok(())
}

fn selected(only: &Option<String>, section: &str) -> bool {
    only.as_deref()
        .is_none_or(|value| value == "all" || value == section)
}

fn header() -> Value {
    json!({
        "kind": "header",
        "stage": "M4-07",
        "host_os": std::env::consts::OS,
        "host_arch": std::env::consts::ARCH,
        "logical_processors": std::thread::available_parallelism().ok().map(usize::from),
        "crn": {"population": 10_000, "settlements": 50, "k": 8, "p_local": 0.5,
                 "seed": SEED, "initial_replicate": REPLICATE, "replicates": 100,
                 "horizons": GOLDEN_HORIZONS},
        "pool_threads": 6,
        "scan_populations": [10_000, 100_000],
        "search": {"population": 2_000, "settlements": 3, "replicates": 10, "horizon": 90},
        "clone_share": {"populations": [10_000, 100_000], "horizons": [30, 90]}
    })
}

fn config_for(population: u64, settlements: u32) -> Result<SimConfig, Box<dyn std::error::Error>> {
    let mut config = SimConfig::parse_and_validate(FIXTURE)?;
    config.profile = SimulationProfile::M3p;
    config.simulation = None;
    config.propagation = None;
    config.world.initial_population = population;
    config.world.settlement_count = settlements;
    config.world.master_seed = SEED;
    config.world.replicate_id = REPLICATE;
    config.validate()?;
    Ok(config)
}

fn snapshot_for(
    config: &SimConfig,
    context: &M0RunContext,
) -> Result<sim_model::CanonicalSnapshot, Box<dyn std::error::Error>> {
    let world = initialize_world(config)?;
    let metadata = SnapshotMetadata::new(
        world.current_day.as_u32(),
        context.master_seed,
        context.replicate_id,
        DEFAULT_MODEL_VERSION,
        DEFAULT_CONFIG_VERSION,
    );
    let state = M3pReferenceState::new(world, config, context)?;
    Ok(encode_m3p_snapshot(&state, &metadata)?)
}

fn food_zero(agent_id: AgentId) -> BranchIntervention {
    BranchIntervention::AgentFieldOverride {
        agent_id,
        field: AgentField::Food,
        value: AgentFieldValue::Food(0.0),
    }
}

fn write_record(path: &Path, record: Value) -> Result<(), Box<dyn std::error::Error>> {
    let file = OpenOptions::new().append(true).open(path)?;
    let mut writer = BufWriter::new(file);
    serde_json::to_writer(&mut writer, &record)?;
    writeln!(writer)?;
    writer.flush()?;
    Ok(())
}

fn estimate_json(estimate: &sim_model::CrnOutcomeEstimate) -> Value {
    let variance_ratio = match estimate.variance_ratio {
        sim_model::CrnVarianceRatio::ExactPairedVarianceZero => {
            json!({"kind": "exact_paired_variance_zero"})
        }
        sim_model::CrnVarianceRatio::Estimated {
            ratio,
            ci95_lower,
            ci95_upper,
        } => json!({"kind": "estimated", "ratio": ratio,
                     "ci95_lower": ci95_lower, "ci95_upper": ci95_upper}),
    };
    json!({
        "horizon_days": estimate.horizon_days,
        "outcome": estimate.outcome.to_string(),
        "paired_mean_effect": estimate.paired_mean_effect,
        "paired_mean_ci95_lower": estimate.paired_mean_ci95_lower,
        "paired_mean_ci95_upper": estimate.paired_mean_ci95_upper,
        "paired_sample_variance": estimate.paired_sample_variance,
        "paired_estimator_variance": estimate.paired_estimator_variance,
        "independent_mean_effect": estimate.independent_mean_effect,
        "independent_base_sample_variance": estimate.independent_base_sample_variance,
        "independent_intervention_sample_variance": estimate.independent_intervention_sample_variance,
        "independent_estimator_variance": estimate.independent_estimator_variance,
        "variance_ratio": variance_ratio,
        "mean_paired_divergent_records": estimate.mean_paired_divergent_records,
        "mean_newborn_crn_loss": estimate.mean_newborn_crn_loss
    })
}

fn measure_crn(path: &Path, parallel_only: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = config_for(10_000, 50)?;
    let context = M0RunContext::new(SEED, REPLICATE);
    let snapshot = snapshot_for(&config, &context)?;
    let interventions = [
        ("food_0", food_zero(AgentId(0))),
        (
            "health_0.01",
            BranchIntervention::AgentFieldOverride {
                agent_id: AgentId(0),
                field: AgentField::Health,
                value: AgentFieldValue::Health(0.01),
            },
        ),
        (
            "base_metabolic_cost_plus_1pct",
            BranchIntervention::ConfigOverride {
                parameter: ConfigParameter::BaseMetabolicCost,
                value: ConfigValue::F32(1.01),
            },
        ),
    ];
    let pool = ThreadPoolBuilder::new().num_threads(6).build()?;
    for (index, (name, intervention)) in interventions.into_iter().enumerate() {
        let request = CrnEfficiencyRequest {
            config: config.clone(),
            branch_point: snapshot.clone(),
            intervention,
            target_group_id: GroupId(0),
            horizons: GOLDEN_HORIZONS.to_vec(),
            replicate_count: 100,
        };
        if parallel_only {
            let started = Instant::now();
            let parallel =
                run_crn_efficiency_parallel(&request, &pool, NonZeroUsize::new(1).unwrap())?;
            let parallel_ns = started.elapsed().as_nanos();
            eprintln!("M5-04 M4-07 parallel-only CRN {name}: {parallel_ns}ns");
            write_record(
                path,
                json!({
                    "kind": "crn_parallel_rerun",
                    "execution_mode": "parallel",
                    "intervention": name,
                    "replicate_count": 100,
                    "horizons": GOLDEN_HORIZONS,
                    "pool_threads": 6,
                    "chunk_size": 1,
                    "parallel_ns": parallel_ns,
                    "estimates": parallel.estimates.iter().map(estimate_json).collect::<Vec<_>>()
                }),
            )?;
            continue;
        }
        let first_parallel = index % 2 == 1;
        let (serial, serial_ns, parallel, parallel_ns) = if first_parallel {
            let started = Instant::now();
            let parallel =
                run_crn_efficiency_parallel(&request, &pool, NonZeroUsize::new(1).unwrap())?;
            let parallel_ns = started.elapsed().as_nanos();
            let started = Instant::now();
            let serial = run_crn_efficiency(&request)?;
            let serial_ns = started.elapsed().as_nanos();
            (serial, serial_ns, parallel, parallel_ns)
        } else {
            let started = Instant::now();
            let serial = run_crn_efficiency(&request)?;
            let serial_ns = started.elapsed().as_nanos();
            let started = Instant::now();
            let parallel =
                run_crn_efficiency_parallel(&request, &pool, NonZeroUsize::new(1).unwrap())?;
            let parallel_ns = started.elapsed().as_nanos();
            (serial, serial_ns, parallel, parallel_ns)
        };
        assert_eq!(serial, parallel, "M4-04 output changed for {name}");
        eprintln!("M4-07 CRN {name}: serial={serial_ns}ns parallel={parallel_ns}ns exact=true");
        write_record(
            path,
            json!({
                "kind": "crn_comparison",
                "intervention": name,
                "replicate_count": 100,
                "horizons": GOLDEN_HORIZONS,
                "order": if first_parallel {"parallel_then_serial"} else {"serial_then_parallel"},
                "serial_ns": serial_ns,
                "parallel_ns": parallel_ns,
                "parallel_speedup": serial_ns as f64 / parallel_ns as f64,
                "bit_identical": true,
                "estimates": serial.estimates.iter().map(estimate_json).collect::<Vec<_>>()
            }),
        )?;
    }
    Ok(())
}

fn measure_day_scans(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for population in [10_000, 100_000] {
        let settlements = population / 200;
        let config = config_for(population, settlements as u32)?;
        let context = M0RunContext::new(SEED, REPLICATE);
        let snapshot = snapshot_for(&config, &context)?;
        let request = BranchRequest {
            snapshot,
            config,
            run_context: context,
            intervention: food_zero(AgentId(0)),
            horizon_days: 30,
            execution: BranchExecutionOption::Serial,
            snapshot_days: Vec::new(),
        };
        let started = Instant::now();
        let sha = run_counterfactual_branch_with_divergence_series(&request)?;
        let sha_ns = started.elapsed().as_nanos();
        let started = Instant::now();
        let direct = run_counterfactual_branch_with_direct_day_scan(
            &request,
            BranchDayScanDigestMode::FirstDivergentDay,
        )?;
        let direct_ns = started.elapsed().as_nanos();
        assert_eq!(direct.first_divergence, sha.branch.first_divergence);
        assert_eq!(
            direct
                .daily_records
                .iter()
                .map(|record| &record.divergence)
                .collect::<Vec<_>>(),
            sha.daily_series.iter().collect::<Vec<_>>()
        );
        eprintln!("M4-07 day scan N={population}: SHA={sha_ns}ns direct={direct_ns}ns exact=true");
        write_record(
            path,
            json!({
                "kind": "day_scan",
                "population": population,
                "settlements": settlements,
                "horizon_days": 30,
                "sha_ns": sha_ns,
                "direct_ns": direct_ns,
                "direct_speedup": sha_ns as f64 / direct_ns as f64,
                "first_divergent_day": direct.first_divergence.as_ref().map(|value| value.day),
                "bit_identical": true
            }),
        )?;
    }
    Ok(())
}

fn measure_minimal_cause(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = config_for(2_000, 3)?;
    let context = M0RunContext::new(SEED, REPLICATE);
    let snapshot = snapshot_for(&config, &context)?;
    let world = initialize_world(&config)?;
    let mut agent_ids = world
        .agents
        .iter()
        .filter(|agent| agent.group_id == GroupId(0))
        .map(|agent| agent.agent_id)
        .collect::<Vec<_>>();
    agent_ids.sort_unstable();
    agent_ids.truncate(8);
    assert_eq!(
        agent_ids,
        (0..8).map(|index| AgentId(index * 3)).collect::<Vec<_>>()
    );
    let candidates = build_agent_field_candidates(
        &world,
        &agent_ids,
        Some(GroupId(0)),
        AgentField::Health,
        &[AgentFieldValue::Health(0.01)],
    )?;
    let serial_log = PathBuf::from("docs/data/m4_07/m4_05_search_serial_numeric8.jsonl");
    let parallel_log = PathBuf::from("docs/data/m4_07/m4_05_search_parallel_numeric8.jsonl");
    let request = |evaluation_log_path| MinimalCauseRequest {
        batch: sim_model::BranchInterventionBatchRequest {
            snapshot: snapshot.clone(),
            config: config.clone(),
            run_context: context,
            interventions: Vec::new(),
            horizon_days: 90,
            snapshot_days: Vec::new(),
        },
        candidates: candidates.clone(),
        predicate: MinimalCausePredicate {
            outcome: sim_model::CrnOutcome::TargetSettlementAlivePopulation,
            target_group_id: Some(GroupId(0)),
            comparison: EffectComparison::LessThanOrEqual,
            threshold: -1.0,
        },
        replicate_ids: (1..=10).collect(),
        exploratory_single_replicate: false,
        max_branch_evaluations: 5_000,
        max_wall_time: Duration::from_secs(7_200),
        evaluation_log_path,
    };
    let started = Instant::now();
    let serial = search_minimal_cause(&request(serial_log.clone()))?;
    let serial_ns = started.elapsed().as_nanos();
    let pool = ThreadPoolBuilder::new().num_threads(6).build()?;
    let started = Instant::now();
    let parallel = search_minimal_cause_parallel(
        &request(parallel_log.clone()),
        &pool,
        NonZeroUsize::new(1).unwrap(),
    )?;
    let parallel_ns = started.elapsed().as_nanos();
    assert_eq!(serial.status, parallel.status);
    assert_eq!(serial.cause_ids, parallel.cause_ids);
    assert_eq!(serial.guarantee, parallel.guarantee);
    assert_eq!(serial.evaluation_count, parallel.evaluation_count);
    assert_eq!(serial.mean_effect, parallel.mean_effect);
    assert_eq!(serial.predicate_value, parallel.predicate_value);
    let serial_log_text = fs::read_to_string(&serial_log)?;
    let parallel_log_text = fs::read_to_string(&parallel_log)?;
    assert_eq!(
        normalize_log(serial_log_text),
        normalize_log(parallel_log_text)
    );
    eprintln!("M4-07 M4-05 case (a): serial={serial_ns}ns parallel={parallel_ns}ns exact=true");
    write_record(
        path,
        json!({
            "kind": "minimal_cause",
            "serial_ns": serial_ns,
            "parallel_ns": parallel_ns,
            "parallel_speedup": serial_ns as f64 / parallel_ns as f64,
            "cause_ids": serial.cause_ids,
            "guarantee": serial.guarantee,
            "evaluation_count": serial.evaluation_count,
            "bit_identical": true,
            "serial_log": serial_log,
            "parallel_log": parallel_log
        }),
    )?;
    Ok(())
}

fn normalize_log(contents: String) -> Vec<Value> {
    contents
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).expect("valid JSONL row");
            if value["kind"] == "result" {
                value["result"]["evaluation_log_path"] = json!("<evaluation-log>");
            }
            value
        })
        .collect()
}

fn measure_clone_share(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for population in [10_000, 100_000] {
        for horizon in [30, 90] {
            let config = config_for(population, (population / 200) as u32)?;
            let context = M0RunContext::new(SEED, REPLICATE);
            let snapshot = snapshot_for(&config, &context)?;
            let request = BranchRequest {
                snapshot,
                config,
                run_context: context,
                intervention: food_zero(AgentId(0)),
                horizon_days: horizon,
                execution: BranchExecutionOption::Serial,
                snapshot_days: Vec::new(),
            };
            let mut samples = Vec::with_capacity(4);
            let mut reference_result = None;
            for _ in 0..4 {
                let started = Instant::now();
                let output = run_counterfactual_branch_with_analysis_mode(
                    &request,
                    BranchAnalysisMode::Full,
                )?;
                let total_ns = started.elapsed().as_nanos();
                let BranchAnalysisOutput::Full { result, costs } = output else {
                    unreachable!("full analysis returned disabled output")
                };
                if let Some(expected) = &reference_result {
                    assert_eq!(expected, &result, "branch output changed across samples");
                } else {
                    reference_result = Some(result);
                }
                samples.push(json!({
                    "total_ns": total_ns,
                    "snapshot_restore_ns": costs.snapshot_restore_ns,
                    "day_start_clone_ns": costs.day_start_clone_ns,
                    "restore_clone_share": (costs.snapshot_restore_ns + costs.day_start_clone_ns) as f64 / total_ns as f64
                }));
            }
            let samples = samples.into_iter().skip(1).collect::<Vec<_>>();
            eprintln!(
                "M4-07 CoW N={population} H={horizon}: {} measured samples",
                samples.len()
            );
            write_record(
                path,
                json!({
                    "kind": "clone_share",
                    "population": population,
                    "settlements": population / 200,
                    "horizon_days": horizon,
                    "warmups": 1,
                    "samples": samples
                }),
            )?;
        }
    }
    Ok(())
}
