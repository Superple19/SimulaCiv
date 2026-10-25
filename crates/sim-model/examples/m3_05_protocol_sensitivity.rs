//! Re-run the pre-registered M3-05 phase cells with the historical 2/7/3 protocol.

#[allow(dead_code)]
mod historical {
    include!("../benches/m3_scale_limit_bench.rs");

    use std::collections::BTreeSet;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const REPORT_REVISION: &str = "5df006c150179f67078588dfaf83a355a81d5f9f";
    const HISTORICAL_WARMUPS: usize = 2;
    const HISTORICAL_SAMPLES: usize = 7;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Cell {
        population: u64,
        degree: u32,
        horizon: u32,
    }

    fn now_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is after Unix epoch")
            .as_millis()
    }

    fn record(name: &str, fields: &str) {
        emit(&format!("RECON_{name}|{fields}"));
    }

    fn format_values(values: &[f64]) -> String {
        values
            .iter()
            .map(|value| format!("{value:.5}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    #[cfg(debug_assertions)]
    fn manual_step(
        manual: &mut ManualProfile,
        config: &SimConfig,
    ) -> (M3DayOutcome, ProfileSample) {
        manual.step(config, false)
    }

    #[cfg(not(debug_assertions))]
    fn manual_step(
        manual: &mut ManualProfile,
        config: &SimConfig,
    ) -> (M3DayOutcome, ProfileSample) {
        manual.step(config)
    }

    fn spec_cells(spec_path: &std::path::Path, smoke: bool) -> (BTreeSet<Cell>, String) {
        if smoke {
            return (
                BTreeSet::from([Cell {
                    population: 300,
                    degree: 8,
                    horizon: 0,
                }]),
                String::from("smoke-only-tiny-cell"),
            );
        }
        let text =
            std::fs::read_to_string(spec_path).expect("read pre-registered sensitivity spec");
        let parsed: toml::Value = toml::from_str(&text).expect("sensitivity spec parses");
        assert_eq!(
            parsed.get("status").and_then(toml::Value::as_str),
            Some("pre-registered"),
            "sensitivity cells must be registered before timing"
        );
        assert_eq!(
            parsed.get("report_revision").and_then(toml::Value::as_str),
            Some(REPORT_REVISION),
            "driver must run at the report revision"
        );
        let populations = parsed["populations"]
            .as_array()
            .expect("registered population list")
            .iter()
            .map(|value| value.as_integer().expect("N is integer") as u64)
            .collect::<Vec<_>>();
        let mut cells = BTreeSet::new();
        for row in parsed["first_break_change"]
            .as_array()
            .expect("registered first-break changes")
        {
            let degree = row["k"].as_integer().expect("k is integer") as u32;
            let horizon = row["day"].as_integer().expect("day is integer") as u32;
            for &population in &populations {
                cells.insert(Cell {
                    population,
                    degree,
                    horizon,
                });
            }
        }
        for row in parsed["not_reproduced_alpha"]
            .as_array()
            .expect("registered alpha claims")
        {
            let degree = row["k"].as_integer().expect("k is integer") as u32;
            let horizon = row["day"].as_integer().expect("day is integer") as u32;
            for field in ["from_N", "to_N"] {
                cells.insert(Cell {
                    population: row[field].as_integer().expect("N endpoint is integer") as u64,
                    degree,
                    horizon,
                });
            }
        }
        assert_eq!(
            cells.len(),
            parsed["unique_phase_population_series"]
                .as_integer()
                .expect("registered unique series count") as usize,
            "deduplicated historical protocol cell list matches preregistration"
        );
        let hash = <sha2::Sha256 as sha2::Digest>::digest(text.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        (cells, format!("sha256:{hash}"))
    }

    fn historical_profile_cell(
        checkpoint: &HybridWorldState,
        config: &SimConfig,
        series_id: &str,
        config_hash: &str,
        warmups: usize,
        samples: usize,
    ) {
        let mut phase_values: [Vec<f64>; PHASE_NAMES.len()] =
            std::array::from_fn(|_| Vec::with_capacity(samples * DAYS_PER_SAMPLE as usize));
        let mut stage_values: [Vec<f64>; P6B_STAGE_NAMES.len()] =
            std::array::from_fn(|_| Vec::with_capacity(samples * DAYS_PER_SAMPLE as usize));
        for sample_index in 0..warmups + samples {
            let sample_kind = if sample_index < warmups {
                "warmup"
            } else {
                "sample"
            };
            let started_at = now_ms();
            let started = Instant::now();
            let mut manual = ManualProfile::from_checkpoint(checkpoint, config);
            let mut runner_world = checkpoint.clone();
            let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
            let mut runner_context = M3StorageRunContext::new(&runner_world, config, &run_context)
                .expect("historical protocol runner context builds");
            let context_ready_at = now_ms();
            let mut runner_outcomes = Vec::with_capacity(DAYS_PER_SAMPLE as usize);
            let mut day_count = 0u64;

            for _ in 0..DAYS_PER_SAMPLE {
                let (manual_outcome, profile) = manual_step(&mut manual, config);
                let outcomes = run_m3_storage_days(
                    &mut runner_world,
                    config,
                    &mut runner_context,
                    1,
                    &options(),
                )
                .expect("historical protocol reference day succeeds");
                assert_eq!(outcomes.len(), 1);
                assert_eq!(
                    manual_outcome, outcomes[0],
                    "intent/resolution parity at every day"
                );
                let manual_hash = canonical_m3_state_hash_from_storage(
                    &manual.storage,
                    manual.world.current_day,
                    &manual.world.settlements,
                    &manual.graph,
                )
                .expect("manual profile state digest");
                let runner_hash = canonical_m3_state_hash_from_storage(
                    runner_world.storage().expect("runner storage exists"),
                    runner_world.world.current_day,
                    &runner_world.world.settlements,
                    runner_context.relationship_graph(),
                )
                .expect("runner state digest");
                assert_eq!(manual_hash, runner_hash, "state parity at every day");
                if sample_index >= warmups {
                    for (values, value) in phase_values.iter_mut().zip(profile.phase_us) {
                        values.push(value);
                    }
                    for (values, value) in stage_values.iter_mut().zip(profile.p6b_stage_us) {
                        values.push(value);
                    }
                }
                runner_outcomes.push(outcomes[0].clone());
                day_count += 1;
            }
            let digests = m3_day_digests(
                &runner_world,
                runner_context.relationship_graph(),
                &runner_outcomes,
            );
            record(
                "SAMPLE",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|kind={sample_kind}|sample_index={sample_index}|started_at_unix_ms={started_at}|context_ready_at_unix_ms={context_ready_at}|completed_at_unix_ms={}|elapsed_seconds={:.6}|days={day_count}",
                    now_ms(),
                    started.elapsed().as_secs_f64()
                ),
            );
            if sample_index + 1 == warmups + samples {
                record(
                    "PARITY",
                    &format!(
                        "series_id={series_id}|config_hash={config_hash}|state={}|metrics={}|events={}|equal=true",
                        digests.state.to_hex(),
                        digests.metrics.to_hex(),
                        digests.events.to_hex()
                    ),
                );
            }
        }

        let phases: [Summary; PHASE_NAMES.len()] = std::array::from_fn(|index| {
            let mut values = phase_values[index].clone();
            summarize(&mut values)
        });
        let stages: [Summary; P6B_STAGE_NAMES.len()] = std::array::from_fn(|index| {
            let mut values = stage_values[index].clone();
            summarize(&mut values)
        });
        for (index, name) in PHASE_NAMES.iter().enumerate() {
            record(
                "PHASE",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|phase={name}|median_us={:.5}|mad_us={:.5}|sample_values_us={}",
                    phases[index].median,
                    phases[index].mad,
                    format_values(&phase_values[index])
                ),
            );
        }
        for (index, name) in P6B_STAGE_NAMES.iter().enumerate() {
            record(
                "STAGE",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|stage={name}|median_us={:.5}|mad_us={:.5}|sample_values_us={}",
                    stages[index].median,
                    stages[index].mad,
                    format_values(&stage_values[index])
                ),
            );
        }
    }

    fn read_skips(text: &str) -> BTreeSet<String> {
        text.split(',')
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    }

    pub(super) fn run(spec_path: PathBuf, smoke: bool, skip_series: &str) {
        let (cells, spec_hash) = spec_cells(&spec_path, smoke);
        let skip_series = read_skips(skip_series);
        let total = cells.len();
        record(
            "HEADER",
            &format!(
                "stage=M3_05_PROTOCOL_SENSITIVITY|report_revision={REPORT_REVISION}|rust_toolchain=1.98.1|spec_hash={spec_hash}|protocol=historical_2_warmups_7_samples_3_days_no_process_controls|smoke={smoke}|planned_series={total}|timestamp_unix_ms={}",
                now_ms()
            ),
        );
        let run_started = Instant::now();
        let mut finished = 0usize;
        let mut total_elapsed = 0.0;
        for cell in cells {
            let series_id = format!(
                "phase:k{}:day{}:N{}",
                cell.degree, cell.horizon, cell.population
            );
            let series_id = if smoke {
                format!("smoke:{series_id}")
            } else {
                series_id
            };
            let config_started_at = now_ms();
            let config_started = Instant::now();
            let config = if smoke {
                let mut config = config_for(cell.population, cell.degree);
                config.world.settlement_count = 3;
                config.validate().expect("tiny smoke config validates");
                config
            } else {
                config_for(cell.population, cell.degree)
            };
            let config_hash = {
                let material = format!(
                    "{}|N={}|K={}|k={}|day={}|seed={MASTER_SEED}|replicate={REPLICATE_ID}|spec={spec_hash}",
                    M2_40_CONFIG,
                    cell.population,
                    config.world.settlement_count,
                    cell.degree,
                    cell.horizon
                );
                let digest = <sha2::Sha256 as sha2::Digest>::digest(material.as_bytes());
                format!(
                    "sha256:{}",
                    digest
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                )
            };
            if skip_series.contains(&series_id) {
                finished += 1;
                eprintln!(
                    "M3-05 protocol sensitivity {finished}/{total} already complete: {series_id}"
                );
                continue;
            }
            let series_started = config_started;
            let setup_started_at = config_started_at;
            let (base, initialization) = initialize(&config);
            let setup_completed_at = now_ms();
            record(
                "SERIES_START",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|N={}|K={}|k={}|day={}|setup_started_at_unix_ms={setup_started_at}|setup_completed_at_unix_ms={setup_completed_at}|initialization_seconds={:.6}|timestamp_unix_ms={}",
                    cell.population,
                    config.world.settlement_count,
                    cell.degree,
                    cell.horizon,
                    initialization.as_secs_f64(),
                    setup_completed_at
                ),
            );
            let checkpoint_started_at = now_ms();
            let checkpoint_timer = Instant::now();
            let checkpoint = build_checkpoint(&base, &config, cell.horizon);
            record(
                "CHECKPOINT",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|started_at_unix_ms={checkpoint_started_at}|completed_at_unix_ms={}|elapsed_seconds={:.6}|day={}",
                    now_ms(),
                    checkpoint_timer.elapsed().as_secs_f64(),
                    cell.horizon
                ),
            );
            record(
                "CALIBRATION",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|status=not_applicable_historical_protocol|timestamp_unix_ms={}",
                    now_ms()
                ),
            );
            historical_profile_cell(
                &checkpoint,
                &config,
                &series_id,
                &config_hash,
                if smoke { 1 } else { HISTORICAL_WARMUPS },
                if smoke { 1 } else { HISTORICAL_SAMPLES },
            );
            let elapsed = series_started.elapsed().as_secs_f64();
            record(
                "SERIES_COMPLETE",
                &format!(
                    "series_id={series_id}|config_hash={config_hash}|completed_at_unix_ms={}|elapsed_seconds={elapsed:.6}|phase_count={}|stage_count={}|parity=exact",
                    now_ms(),
                    PHASE_NAMES.len(),
                    P6B_STAGE_NAMES.len()
                ),
            );
            finished += 1;
            total_elapsed += elapsed;
            let mean = total_elapsed / finished as f64;
            eprintln!(
                "M3-05 protocol sensitivity {finished}/{total} elapsed={:.1}s estimated_remaining={:.1}s: {series_id}",
                run_started.elapsed().as_secs_f64(),
                mean * (total.saturating_sub(finished)) as f64
            );
        }
        record(
            "RUN_COMPLETE",
            &format!(
                "completed_series={finished}|planned_series={total}|timestamp_unix_ms={}|elapsed_seconds={:.6}",
                now_ms(),
                run_started.elapsed().as_secs_f64()
            ),
        );
    }
}

use std::path::PathBuf;

fn main() {
    let mut spec: Option<PathBuf> = None;
    let mut smoke = false;
    let mut skip_series = String::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--spec" => spec = args.next().map(PathBuf::from),
            "--smoke" => smoke = true,
            "--skip-series" => skip_series = args.next().expect("--skip-series needs ids"),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    historical::run(spec.expect("--spec is required"), smoke, &skip_series);
}
