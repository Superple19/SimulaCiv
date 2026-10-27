#![allow(dead_code)]

pub const WARMUPS: usize = 2;
pub const SAMPLES: usize = 41;
pub const TARGET_SECONDS: f64 = 1.25;
pub const FLOOR_SECONDS: f64 = 1.0;
pub const MAX_RECALIBRATIONS: usize = 3;

#[derive(Debug)]
pub struct Recalibration {
    pub old_repetitions: Vec<usize>,
    pub new_repetitions: Vec<usize>,
}

#[derive(Debug)]
pub struct Series {
    pub repetitions_per_path: Vec<usize>,
    pub sample_seconds: Vec<f64>,
    pub path_samples_us: Vec<Vec<f64>>,
    pub recalibrations: Vec<Recalibration>,
}

pub fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

pub fn mad_over_median(values: &[f64]) -> f64 {
    let center = median(values);
    median(
        &values
            .iter()
            .map(|value| (value - center).abs())
            .collect::<Vec<_>>(),
    ) / center
}

pub fn config_hash(value: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn json_numbers(values: &[f64]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| format!("{value:.9}"))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn recalibrations_json(series: &Series) -> String {
    let rows = series
        .recalibrations
        .iter()
        .map(|row| {
            let old = row.old_repetitions.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
            let new = row.new_repetitions.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
            format!("{{\"old_repetitions\":[{old}],\"new_repetitions\":[{new}],\"reason\":\"sample_below_floor\"}}")
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{rows}]")
}

fn repetitions(target_seconds: f64, probe_seconds: &[f64]) -> usize {
    (target_seconds / median(probe_seconds)).ceil().max(1.0) as usize
}

fn larger_repetitions(old: usize) -> usize {
    ((old as f64 * 1.5).ceil() as usize).max(old + 1)
}

pub fn paired_series<F>(path_count: usize, mut operation: F) -> Series
where
    F: FnMut(usize, usize) -> (Vec<f64>, f64),
{
    let probes = (0..3)
        .map(|index| operation(index, 0).1)
        .collect::<Vec<_>>();
    let mut reps = vec![repetitions(TARGET_SECONDS, &probes)];
    let mut recalibrations = Vec::new();
    for recalibration in 0..=MAX_RECALIBRATIONS {
        for warmup in 0..WARMUPS {
            for repeat in 0..reps[0] {
                let _ = operation(warmup, repeat);
            }
        }
        let mut path_samples = vec![Vec::with_capacity(SAMPLES); path_count];
        let mut sample_seconds = Vec::with_capacity(SAMPLES);
        let mut failed = false;
        for sample in 0..SAMPLES {
            let mut sums = vec![0.0; path_count];
            let mut seconds = 0.0;
            for repeat in 0..reps[0] {
                let (paths, elapsed_seconds) = operation(sample, repeat);
                assert_eq!(paths.len(), path_count, "A-5 operation path count");
                for (sum, value) in sums.iter_mut().zip(paths) {
                    *sum += value;
                }
                seconds += elapsed_seconds;
            }
            if seconds < FLOOR_SECONDS {
                failed = true;
                break;
            }
            sample_seconds.push(seconds);
            for (path, sum) in path_samples.iter_mut().zip(sums) {
                path.push(sum / reps[0] as f64);
            }
        }
        if !failed {
            return Series {
                repetitions_per_path: reps,
                sample_seconds,
                path_samples_us: path_samples,
                recalibrations,
            };
        }
        if recalibration == MAX_RECALIBRATIONS {
            panic!("A-5 sample remained below the one-second floor after three recalibrations");
        }
        let old = reps.clone();
        reps = reps.into_iter().map(larger_repetitions).collect();
        recalibrations.push(Recalibration {
            old_repetitions: old,
            new_repetitions: reps.clone(),
        });
    }
    unreachable!()
}

pub fn noninterleaved_series<F>(path_count: usize, mut operation: F) -> Series
where
    F: FnMut(usize, usize, usize) -> (f64, f64),
{
    let probes = (0..path_count)
        .map(|path| {
            median(
                &(0..3)
                    .map(|probe| operation(path, probe, 0).1)
                    .collect::<Vec<_>>(),
            )
        })
        .sum::<f64>();
    let mut reps = vec![((TARGET_SECONDS / probes).ceil().max(1.0)) as usize];
    let mut recalibrations = Vec::new();
    for recalibration in 0..=MAX_RECALIBRATIONS {
        for path in 0..path_count {
            for warmup in 0..WARMUPS {
                for repeat in 0..reps[0] {
                    let _ = operation(path, warmup, repeat);
                }
            }
        }
        let mut path_samples = vec![Vec::with_capacity(SAMPLES); path_count];
        let mut path_seconds = vec![vec![0.0; SAMPLES]; path_count];
        let mut failed = false;
        for path in 0..path_count {
            for (sample, seconds_slot) in path_seconds[path].iter_mut().enumerate() {
                let mut sum = 0.0;
                let mut seconds = 0.0;
                for repeat in 0..reps[0] {
                    let (value, elapsed_seconds) = operation(path, sample, repeat);
                    sum += value;
                    seconds += elapsed_seconds;
                }
                *seconds_slot = seconds;
                path_samples[path].push(sum / reps[0] as f64);
            }
        }
        let sample_seconds = (0..SAMPLES)
            .map(|sample| path_seconds.iter().map(|path| path[sample]).sum::<f64>())
            .collect::<Vec<_>>();
        if sample_seconds
            .iter()
            .any(|seconds| *seconds < FLOOR_SECONDS)
        {
            failed = true;
        }
        if !failed {
            return Series {
                repetitions_per_path: reps,
                sample_seconds,
                path_samples_us: path_samples,
                recalibrations,
            };
        }
        if recalibration == MAX_RECALIBRATIONS {
            panic!("A-5 sample remained below the one-second floor after three recalibrations");
        }
        let old = reps.clone();
        reps = reps.into_iter().map(larger_repetitions).collect();
        recalibrations.push(Recalibration {
            old_repetitions: old,
            new_repetitions: reps.clone(),
        });
    }
    unreachable!()
}
