#[cfg(not(debug_assertions))]
#[path = "common/process_controls.rs"]
mod historical_process_controls;

struct Args {
    smoke: bool,
}

fn args() -> Args {
    let mut smoke = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--smoke" => smoke = true,
            _ => panic!("unknown argument: {arg}"),
        }
    }
    Args { smoke }
}

#[allow(dead_code)]
mod scale {
    include!("../benches/m3_scale_limit_bench.rs");

    use std::process::Command;

    const A5_WARMUPS: usize = 2;
    const A5_SAMPLES: usize = 41;
    const A5_FLOOR_SECONDS: f64 = 1.0;
    const A5_PROBES: usize = 3;

    #[derive(Clone, Copy)]
    struct ProfileSummary {
        phases: [Summary; PHASE_NAMES.len()],
        stages: [Summary; P6B_STAGE_NAMES.len()],
    }

    #[derive(Clone, Copy)]
    struct PilotCell {
        population: u64,
        degree: u32,
        horizon: u32,
        serial: Summary,
        parallel: Summary,
        profile: ProfileSummary,
    }

    // The report-revision bench predates the extra scan flag in ManualProfile::step.
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

    fn median(values: &[f64]) -> f64 {
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2]
    }

    fn repetitions_for(probes: &[f64]) -> usize {
        (A5_FLOOR_SECONDS / median(probes)).ceil().max(1.0) as usize
    }

    fn config_for_cell(population: u64, settlements: u32, degree: u32) -> SimConfig {
        let mut config =
            SimConfig::parse_and_validate(M2_40_CONFIG).expect("M2-40 base config parses");
        config.profile = SimulationProfile::M3;
        config.world.initial_population = population;
        config.world.settlement_count = settlements;
        config.world.master_seed = MASTER_SEED;
        config.world.replicate_id = REPLICATE_ID;
        config.environment.base_metabolic_cost = EQUILIBRIUM_METABOLIC_COST;
        config.interaction.relationship_degree_k = Some(i64::from(degree));
        config.interaction.p_local = Some(P_LOCAL);
        config.validate().expect("M3-05 scale config validates");
        config
    }

    fn format_values(values: &[f64]) -> String {
        values
            .iter()
            .map(|value| format!("{value:.6}"))
            .collect::<Vec<_>>()
            .join(",")
    }

    fn setup_operation(
        config: &SimConfig,
        world: &HybridWorldState,
        phase: &str,
        repetitions: usize,
    ) -> (f64, f64) {
        let mut total_us = 0.0;
        for _ in 0..repetitions {
            let setup = measure_setup(config, world, Duration::ZERO);
            total_us += match phase {
                "graph" => micros(setup.graph),
                "projection" => micros(setup.projection),
                _ => unreachable!("known setup phase"),
            };
            std::hint::black_box((setup.graph_heap_bytes, setup.projection_heap_bytes));
        }
        (total_us / repetitions as f64, total_us / 1_000_000.0)
    }

    fn setup_series(
        config: &SimConfig,
        world: &HybridWorldState,
        phase: &str,
        warmups: usize,
        samples: usize,
        smoke: bool,
    ) -> Summary {
        let probes = (0..A5_PROBES)
            .map(|_| setup_operation(config, world, phase, 1).0 / 1_000_000.0)
            .collect::<Vec<_>>();
        let repetitions = if smoke { 1 } else { repetitions_for(&probes) };
        let mut values = Vec::with_capacity(samples);
        let mut elapsed = Vec::with_capacity(samples);
        for sample in 0..warmups + samples {
            let (value, seconds) = setup_operation(config, world, phase, repetitions);
            assert!(
                smoke || seconds >= A5_FLOOR_SECONDS,
                "A-5 setup {phase} sample {sample} shorter than one second (r={repetitions}, {seconds:.3}s)"
            );
            if sample >= warmups {
                values.push(value);
                elapsed.push(seconds);
            }
        }
        let mut sorted = values.clone();
        let result = summarize(&mut sorted);
        emit(&format!(
            "SCALE_SETUP_A5|N={}|K={}|k={}|phase={phase}|warmups={warmups}|samples={samples}|r={repetitions}|median_us={:.3}|mad_us={:.3}|sample_values_us={}|sample_seconds={}",
            config.world.initial_population,
            config.world.settlement_count,
            config.interaction.relationship_degree_k.unwrap(),
            result.median,
            result.mad,
            format_values(&values),
            format_values(&elapsed),
        ));
        result
    }

    fn profile_operation(
        checkpoint: &HybridWorldState,
        config: &SimConfig,
        repetitions: usize,
    ) -> (f64, ProfileSummary) {
        let started = Instant::now();
        let mut phase_values: [Vec<f64>; PHASE_NAMES.len()] = array::from_fn(|_| Vec::new());
        let mut stage_values: [Vec<f64>; P6B_STAGE_NAMES.len()] = array::from_fn(|_| Vec::new());
        for _ in 0..repetitions {
            let mut manual = ManualProfile::from_checkpoint(checkpoint, config);
            let mut runner_world = checkpoint.clone();
            let run_context = M0RunContext::new(MASTER_SEED, REPLICATE_ID);
            let mut runner_context = M3StorageRunContext::new(&runner_world, config, &run_context)
                .expect("profile oracle context builds");
            let mut runner_outcomes = Vec::with_capacity(DAYS_PER_SAMPLE as usize);
            for _ in 0..DAYS_PER_SAMPLE {
                let (manual_outcome, profile) = manual_step(&mut manual, config);
                let outcomes = run_m3_storage_days(
                    &mut runner_world,
                    config,
                    &mut runner_context,
                    1,
                    &options(),
                )
                .expect("profile oracle day succeeds");
                assert_eq!(outcomes.len(), 1);
                assert_eq!(manual_outcome, outcomes[0]);
                let manual_hash = canonical_m3_state_hash_from_storage(
                    &manual.storage,
                    manual.world.current_day,
                    &manual.world.settlements,
                    &manual.graph,
                )
                .expect("manual profile state hash");
                let runner_hash = canonical_m3_state_hash_from_storage(
                    runner_world.storage().expect("oracle storage exists"),
                    runner_world.world.current_day,
                    &runner_world.world.settlements,
                    runner_context.relationship_graph(),
                )
                .expect("runner profile state hash");
                assert_eq!(manual_hash, runner_hash);
                for (samples, value) in phase_values.iter_mut().zip(profile.phase_us) {
                    samples.push(value);
                }
                for (samples, value) in stage_values.iter_mut().zip(profile.p6b_stage_us) {
                    samples.push(value);
                }
                runner_outcomes.push(outcomes[0].clone());
            }
            let digests = m3_day_digests(
                &runner_world,
                runner_context.relationship_graph(),
                &runner_outcomes,
            );
            assert_ne!(digests.state.to_hex(), "");
        }
        let seconds = started.elapsed().as_secs_f64();
        (
            seconds,
            ProfileSummary {
                phases: phase_values.each_mut().map(|values| summarize(values)),
                stages: stage_values.each_mut().map(|values| summarize(values)),
            },
        )
    }

    fn profile_series(
        checkpoint: &HybridWorldState,
        config: &SimConfig,
        serial_median: f64,
        warmups: usize,
        samples: usize,
        smoke: bool,
    ) -> ProfileSummary {
        let probes = (0..A5_PROBES)
            .map(|_| profile_operation(checkpoint, config, 1).0)
            .collect::<Vec<_>>();
        let repetitions = if smoke { 1 } else { repetitions_for(&probes) };
        let mut phase_sample_values: [Vec<f64>; PHASE_NAMES.len()] =
            array::from_fn(|_| Vec::with_capacity(samples));
        let mut stage_sample_values: [Vec<f64>; P6B_STAGE_NAMES.len()] =
            array::from_fn(|_| Vec::with_capacity(samples));
        let mut sample_seconds = Vec::with_capacity(samples);
        for sample in 0..warmups + samples {
            let (seconds, profile) = profile_operation(checkpoint, config, repetitions);
            assert!(
                smoke || seconds >= A5_FLOOR_SECONDS,
                "A-5 phase profile sample {sample} shorter than one second (r={repetitions}, {seconds:.3}s)"
            );
            if sample >= warmups {
                for (values, summary) in phase_sample_values.iter_mut().zip(profile.phases) {
                    values.push(summary.median);
                }
                for (values, summary) in stage_sample_values.iter_mut().zip(profile.stages) {
                    values.push(summary.median);
                }
                sample_seconds.push(seconds);
            }
        }
        let phases = phase_sample_values
            .each_mut()
            .map(|values| summarize(values));
        let stages = stage_sample_values
            .each_mut()
            .map(|values| summarize(values));
        for (index, name) in PHASE_NAMES.iter().enumerate() {
            emit(&format!(
                "SCALE_PHASE|N={}|K={}|k={}|horizon={}|phase={name}|median_us={:.3}|mad_us={:.3}|share_of_serial_tick_pct={:.3}|repetitions_per_sample={repetitions}|sample_medians_us={}|sample_seconds={}",
                config.world.initial_population,
                config.world.settlement_count,
                config.interaction.relationship_degree_k.unwrap(),
                checkpoint.world.current_day.as_u32(),
                phases[index].median,
                phases[index].mad,
                phases[index].median / serial_median * 100.0,
                format_values(&phase_sample_values[index]),
                format_values(&sample_seconds),
            ));
        }
        for (index, name) in P6B_STAGE_NAMES.iter().enumerate() {
            emit(&format!(
                "SCALE_P6B_STAGE|N={}|K={}|k={}|horizon={}|stage={name}|median_us={:.3}|mad_us={:.3}|share_of_serial_tick_pct={:.3}|repetitions_per_sample={repetitions}|sample_medians_us={}|sample_seconds={}",
                config.world.initial_population,
                config.world.settlement_count,
                config.interaction.relationship_degree_k.unwrap(),
                checkpoint.world.current_day.as_u32(),
                stages[index].median,
                stages[index].mad,
                stages[index].median / serial_median * 100.0,
                format_values(&stage_sample_values[index]),
                format_values(&sample_seconds),
            ));
        }
        ProfileSummary { phases, stages }
    }

    fn paired_operation(
        checkpoint: &HybridWorldState,
        config: &SimConfig,
        pool: &rayon::ThreadPool,
        repetitions: usize,
        sample_index: usize,
    ) -> (f64, f64, f64, Digests) {
        let mut serial = 0.0;
        let mut parallel = 0.0;
        let mut seconds = 0.0;
        let mut digests: Option<Digests> = None;
        for repeat in 0..repetitions {
            let pair = paired_timing(
                checkpoint,
                config,
                pool,
                sample_index * repetitions + repeat,
            );
            serial += pair.serial;
            parallel += pair.parallel;
            seconds += (pair.serial + pair.parallel) * f64::from(DAYS_PER_SAMPLE) / 1_000_000.0;
            if let Some(previous) = digests {
                assert_eq!(previous.state, pair.digests.state);
                assert_eq!(previous.metrics, pair.digests.metrics);
                assert_eq!(previous.events, pair.digests.events);
            } else {
                digests = Some(pair.digests);
            }
        }
        (
            serial / repetitions as f64,
            parallel / repetitions as f64,
            seconds,
            digests.expect("paired operation always samples once"),
        )
    }

    fn paired_series(
        checkpoint: &HybridWorldState,
        config: &SimConfig,
        pool: &rayon::ThreadPool,
        warmups: usize,
        samples: usize,
        smoke: bool,
    ) -> (Summary, Summary, Digests) {
        let probes = (0..A5_PROBES)
            .map(|index| paired_operation(checkpoint, config, pool, 1, index).2)
            .collect::<Vec<_>>();
        let repetitions = if smoke { 1 } else { repetitions_for(&probes) };
        let mut serial_values = Vec::with_capacity(samples);
        let mut parallel_values = Vec::with_capacity(samples);
        let mut sample_seconds = Vec::with_capacity(samples);
        let mut expected: Option<Digests> = None;
        for sample in 0..warmups + samples {
            let (serial, parallel, seconds, digests) =
                paired_operation(checkpoint, config, pool, repetitions, sample + 10);
            assert!(
                smoke || seconds >= A5_FLOOR_SECONDS,
                "A-5 paired sample {sample} shorter than one second (r={repetitions}, {seconds:.3}s)"
            );
            if let Some(previous) = expected {
                assert_eq!(previous.state, digests.state);
                assert_eq!(previous.metrics, digests.metrics);
                assert_eq!(previous.events, digests.events);
            } else {
                expected = Some(digests);
            }
            if sample >= warmups {
                serial_values.push(serial);
                parallel_values.push(parallel);
                sample_seconds.push(seconds);
            }
        }
        let mut serial_sorted = serial_values.clone();
        let mut parallel_sorted = parallel_values.clone();
        let serial = summarize(&mut serial_sorted);
        let parallel = summarize(&mut parallel_sorted);
        emit(&format!(
            "SCALE_FULL_TICK|N={}|K={}|k={}|p_local={P_LOCAL}|horizon={}|serial_median_us={:.3}|serial_mad_us={:.3}|serial_us_per_agent={:.6}|parallel_median_us={:.3}|parallel_mad_us={:.3}|parallel_us_per_agent={:.6}|speedup={:.4}|warmups={warmups}|samples={samples}|r={repetitions}|serial_samples_us={}|parallel_samples_us={}|sample_seconds={}",
            config.world.initial_population,
            config.world.settlement_count,
            config.interaction.relationship_degree_k.unwrap(),
            checkpoint.world.current_day.as_u32(),
            serial.median,
            serial.mad,
            serial.median / config.world.initial_population as f64,
            parallel.median,
            parallel.mad,
            parallel.median / config.world.initial_population as f64,
            serial.median / parallel.median,
            format_values(&serial_values),
            format_values(&parallel_values),
            format_values(&sample_seconds),
        ));
        (
            serial,
            parallel,
            expected.expect("paired samples have digests"),
        )
    }

    fn host_ram_bytes() -> Option<u64> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory",
            ])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?.trim().parse().ok()
    }

    fn emit_host(smoke: bool) {
        let controls = if smoke {
            "not_applied_smoke".to_owned()
        } else {
            #[cfg(not(debug_assertions))]
            {
                let status = super::historical_process_controls::apply_high_priority_affinity();
                if cfg!(windows) {
                    assert!(status.priority_set && status.affinity_set);
                    "priority=high;affinity_mask=0x555".to_owned()
                } else {
                    "priority=not-applied;affinity_mask=not-applied".to_owned()
                }
            }
            #[cfg(debug_assertions)]
            {
                "priority=not-applied;affinity_mask=not-applied".to_owned()
            }
        };
        emit(&format!(
            "SCALE_HOST|os={}|arch={}|seed={MASTER_SEED}|replicate={REPLICATE_ID}|profile=equilibrium|metabolic_cost={EQUILIBRIUM_METABOLIC_COST}|p_local={P_LOCAL}|pool_threads={POOL_THREADS}|chunk={CHUNK_SIZE}|warmups={}|samples={}|days_per_sample={DAYS_PER_SAMPLE}|sample_floor_seconds={}|process_controls={controls}|host_ram_bytes={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            if smoke { 1 } else { A5_WARMUPS },
            if smoke { 2 } else { A5_SAMPLES },
            if smoke { 0.0 } else { A5_FLOOR_SECONDS },
            host_ram_bytes().map_or_else(|| "unavailable".to_owned(), |value| value.to_string()),
        ));
    }

    fn emit_alphas(cells: &[PilotCell]) -> Vec<(u32, u32, u64, u64, String, f64)> {
        let mut output = Vec::new();
        for degree in DEGREES {
            for horizon in CHECKPOINT_DAYS {
                let mut group = cells
                    .iter()
                    .filter(|cell| cell.degree == degree && cell.horizon == horizon)
                    .collect::<Vec<_>>();
                group.sort_by_key(|cell| cell.population);
                for pair in group.windows(2) {
                    let lower = pair[0];
                    let upper = pair[1];
                    let mut add = |phase: String, low: f64, high: f64| {
                        let value = alpha(lower.population, low, upper.population, high);
                        emit(&format!(
                            "SCALE_ALPHA|k={degree}|horizon={horizon}|phase={phase}|from_N={}|to_N={}|alpha={value:.5}",
                            lower.population, upper.population,
                        ));
                        output.push((
                            degree,
                            horizon,
                            lower.population,
                            upper.population,
                            phase,
                            value,
                        ));
                    };
                    add(
                        "full_tick".to_owned(),
                        lower.serial.median,
                        upper.serial.median,
                    );
                    for (index, name) in PHASE_NAMES.iter().enumerate() {
                        add(
                            (*name).to_owned(),
                            lower.profile.phases[index].median,
                            upper.profile.phases[index].median,
                        );
                    }
                    for (index, stage_name) in P6B_STAGE_NAMES.iter().enumerate() {
                        let low = lower.profile.stages[index].median;
                        let high = upper.profile.stages[index].median;
                        let phase = format!("P6B_{stage_name}");
                        if low == 0.0 || high == 0.0 {
                            emit(&format!(
                                "SCALE_ALPHA|k={degree}|horizon={horizon}|phase={phase}|from_N={}|to_N={}|alpha=not_applicable_serial_stage",
                                lower.population, upper.population
                            ));
                        } else {
                            add(phase, low, high);
                        }
                    }
                }
            }
        }
        output
    }

    fn emit_breaks(
        cells: &[PilotCell],
        setups: &[(u64, u32, SetupMeasurement, MemoryMeasurement)],
        alphas: &[(u32, u32, u64, u64, String, f64)],
        ram_bytes: Option<u64>,
    ) {
        let mut any = false;
        for (degree, horizon, from_n, to_n, phase, value) in alphas {
            if *value > 1.15 {
                any = true;
                emit(&format!(
                    "SCALE_FIRST_BREAK|criterion=phase_alpha_gt_1.15|k={degree}|horizon={horizon}|phase={phase}|from_N={from_n}|to_N={to_n}|alpha={value:.5}"
                ));
            }
        }
        for degree in DEGREES {
            for horizon in CHECKPOINT_DAYS {
                if let Some(baseline) = cells.iter().find(|cell| {
                    cell.population == 100_000 && cell.degree == degree && cell.horizon == horizon
                }) {
                    for cell in cells.iter().filter(|cell| {
                        cell.degree == degree
                            && cell.horizon == horizon
                            && cell.population > 100_000
                    }) {
                        let increase = (cell.serial.median / cell.population as f64)
                            / (baseline.serial.median / baseline.population as f64)
                            - 1.0;
                        if increase > 0.25 {
                            any = true;
                            emit(&format!(
                                "SCALE_FIRST_BREAK|criterion=per_agent_tick_gt_25pct|k={degree}|horizon={horizon}|N={}|increase_pct={:.3}",
                                cell.population,
                                increase * 100.0
                            ));
                        }
                    }
                }
            }
        }
        for (population, degree, setup, _) in setups {
            if let Some(day0) = cells.iter().find(|cell| {
                cell.population == *population && cell.degree == *degree && cell.horizon == 0
            }) {
                let ticks = micros(setup.graph + setup.projection) / day0.serial.median;
                if ticks > 10.0 {
                    any = true;
                    emit(&format!(
                        "SCALE_FIRST_BREAK|criterion=graph_projection_setup_gt_10_ticks|N={population}|k={degree}|tick_equivalents={ticks:.4}"
                    ));
                }
            }
        }
        if let Some(ram) = ram_bytes {
            for (population, degree, setup, memory) in setups {
                let serial = memory.soa_bytes
                    + memory.graph_csr_bytes
                    + memory.projection_bytes
                    + memory.membership_bytes
                    + memory.serial_scratch_bytes.max(0) as usize;
                let parallel = serial + memory.parallel_extra_scratch_bytes.max(0) as usize;
                let used = serial
                    .max(parallel)
                    .max(memory.os_peak_working_set_bytes.unwrap_or(0));
                if used as u64 > ram / 2 {
                    any = true;
                    emit(&format!(
                        "SCALE_FIRST_BREAK|criterion=memory_gt_50pct_host_ram|N={population}|k={degree}|used_bytes={used}|host_ram_bytes={ram}|graph_projection_us={:.3}",
                        micros(setup.graph + setup.projection)
                    ));
                }
            }
        }
        if !any {
            emit("SCALE_FIRST_BREAK|none_through_N=1000000|runtime_or_capacity_errors=none");
        }
    }

    pub(super) fn run(args: super::Args) {
        emit_host(args.smoke);
        let pool = ThreadPoolBuilder::new()
            .num_threads(POOL_THREADS)
            .build()
            .expect("fixed Rayon pool builds");
        let warmups = if args.smoke { 1 } else { A5_WARMUPS };
        let samples = if args.smoke { 2 } else { A5_SAMPLES };
        let cells = if args.smoke {
            vec![(300_u64, 3_u32, 8_u32, 0_u32)]
        } else {
            POPULATIONS
                .into_iter()
                .flat_map(|population| {
                    DEGREES.into_iter().flat_map(move |degree| {
                        CHECKPOINT_DAYS.into_iter().map(move |horizon| {
                            (population, (population / 200) as u32, degree, horizon)
                        })
                    })
                })
                .collect()
        };
        let configs = cells
            .iter()
            .map(|cell| (cell.0, cell.1, cell.2))
            .collect::<std::collections::BTreeSet<_>>();
        let ram = host_ram_bytes();
        let mut setups = Vec::new();
        let mut results = Vec::new();
        for (population, settlements, degree) in configs {
            let config = config_for_cell(population, settlements, degree);
            let (base, initialization) = initialize(&config);
            let probe = measure_setup(&config, &base, initialization);
            let graph = setup_series(&config, &base, "graph", warmups, samples, args.smoke);
            let projection =
                setup_series(&config, &base, "projection", warmups, samples, args.smoke);
            let setup = SetupMeasurement {
                initialization,
                graph: Duration::from_secs_f64(graph.median / 1_000_000.0),
                projection: Duration::from_secs_f64(projection.median / 1_000_000.0),
                graph_heap_bytes: probe.graph_heap_bytes,
                projection_heap_bytes: probe.projection_heap_bytes,
            };
            let memory = measure_memory(&base, &config, &pool, setup);
            report_setup(setup, population, degree, settlements);
            report_memory(memory, population, degree, settlements);
            setups.push((population, degree, setup, memory));
            for (_, _, _, horizon) in cells
                .iter()
                .filter(|cell| cell.0 == population && cell.1 == settlements && cell.2 == degree)
            {
                let checkpoint = build_checkpoint(&base, &config, *horizon);
                let (serial, parallel, digests) =
                    paired_series(&checkpoint, &config, &pool, warmups, samples, args.smoke);
                let profile = profile_series(
                    &checkpoint,
                    &config,
                    serial.median,
                    warmups,
                    samples,
                    args.smoke,
                );
                emit(&format!(
                    "SCALE_PARITY|N={population}|K={settlements}|k={degree}|horizon={horizon}|state={}|metrics={}|events={}|serial_parallel_equal=true",
                    digests.state.to_hex(),
                    digests.metrics.to_hex(),
                    digests.events.to_hex()
                ));
                results.push(PilotCell {
                    population,
                    degree,
                    horizon: *horizon,
                    serial,
                    parallel,
                    profile,
                });
            }
        }
        if !args.smoke {
            let alphas = emit_alphas(&results);
            emit_breaks(&results, &setups, &alphas, ram);
            emit(
                "SCALE_COMPLETE|largest_N=1000000|cells=16|parity=all_serial_parallel_final_digests_equal|a5_series=48",
            );
        }
    }
}

fn main() {
    scale::run(args());
}
