use nebula_cluster::dsp::{db_to_gain, gain_to_db, DspSettings, NebulaClusterDsp, SAFETY_CEILING};
use nebula_cluster::model::{ControlId, Snapshot, ALL_CONTROLS};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use std::f64::consts::PI;
use std::thread;
use std::time::Instant;

const SR: f64 = 48_000.0;

#[test]
fn audio_evaluation_null_spectral_transient_lufs() {
    let input = multi_source_signal(16_384, SR);
    let settings = DspSettings::default();
    let (left, right, _) = process_pair(&settings, &input, &input, SR);

    let null = max_abs_residual(&input, &left).max(max_abs_residual(&input, &right));
    let spectral_delta = spectral_rms_delta_db(&input, &left);
    let transient_delta = transient_peak_delta(&input, &left);
    let lufs_delta = (lufs_like(&input, SR) - lufs_like(&left, SR)).abs();

    println!(
        "audio_evaluation null={null:.3e} spectral_delta={spectral_delta:.6}dB transient_delta={transient_delta:.3e} lufs_delta={lufs_delta:.6}LU"
    );

    assert!(null < 1.0e-10);
    assert!(spectral_delta < 1.0e-6);
    assert!(transient_delta < 1.0e-10);
    assert!(lufs_delta < 1.0e-6);
}

#[test]
fn audio_evaluation_harmonic_musicality_and_golden_reference() {
    let input = sine(8192, SR, 220.0, 0.25);
    let mut snapshot = Snapshot::default();
    snapshot.set(ControlId::DistortionEnabled, 1.0);
    snapshot.set(ControlId::DistSaturation, 0.72);
    snapshot.set(ControlId::Harmonic2, 0.45);
    snapshot.set(ControlId::Harmonic3, 0.82);
    snapshot.set(ControlId::Harmonic4, 0.22);
    snapshot.set(ControlId::Harmonic5, 0.38);
    snapshot.set(ControlId::Harmonic6, 0.16);
    snapshot.set(ControlId::Harmonic7, 0.2);
    snapshot.set(ControlId::DistMix, 0.82);
    let settings = DspSettings::from_snapshot(snapshot);

    let (processed, _, _) = process_pair(&settings, &input, &input, SR);
    let harmonic_index = harmonic_musicality_index(&processed, SR, 220.0);
    let alias_rejection = non_harmonic_rejection_db(&processed, SR, 220.0);
    let golden = golden_reference_distance(&input, &processed, SR);

    println!(
        "audio_evaluation harmonic_index={harmonic_index:.3} nonharmonic_rejection={alias_rejection:.2}dB golden_distance={golden:.4}"
    );

    assert!(harmonic_index > 0.35);
    assert!(alias_rejection > 8.0);
    assert!(golden < 0.42);
}

#[test]
fn audio_evaluation_low_level_hiss_guard() {
    let mut rng = StdRng::seed_from_u64(0x4e43_4849_5353);
    let input: Vec<f64> = (0..16_384)
        .map(|_| rng.gen_range(-1.0..1.0) * db_to_gain(-108.0))
        .collect();

    for mode in [1.0, 2.0] {
        let mut snapshot = Snapshot::default();
        snapshot.set(ControlId::CompressorEnabled, 1.0);
        snapshot.set(ControlId::CompMode, mode);
        snapshot.set(ControlId::CompRatio, 20.0);
        snapshot.set(ControlId::CompBoost, 36.0);
        snapshot.set(ControlId::CompMakeup, 24.0);
        snapshot.set(ControlId::CompAttackThreshold, -80.0);
        snapshot.set(ControlId::CompAttackMs, 0.01);
        snapshot.set(ControlId::CompReleaseMs, 20.0);
        let settings = DspSettings::from_snapshot(snapshot);
        let (processed, _, _) = process_pair(&settings, &input, &input, SR);
        let output_rms = rms(&processed);

        println!(
            "audio_evaluation hiss_guard mode={mode:.0} output_rms_db={:.2}",
            gain_to_db(output_rms)
        );

        assert!(output_rms < db_to_gain(-92.0));
    }
}

#[test]
fn audio_evaluation_stereo_groove_temporal_dynamics() {
    let left = groove_signal(24_000, SR, 0.0);
    let right = groove_signal(24_000, SR, 0.37);
    let mut snapshot = musical_snapshot();
    snapshot.set(ControlId::FilterEnabled, 1.0);
    snapshot.set(ControlId::FilterHpf, 35.0);
    snapshot.set(ControlId::FilterHpSlope, 18.0);
    snapshot.set(ControlId::FilterLpf, 18_000.0);
    snapshot.set(ControlId::FilterLpSlope, 12.0);
    snapshot.set(ControlId::CompressorEnabled, 1.0);
    snapshot.set(ControlId::CompRatio, 2.6);
    snapshot.set(ControlId::CompAttackMs, 12.0);
    snapshot.set(ControlId::CompReleaseMs, 90.0);
    snapshot.set(ControlId::CompKnee, 9.0);
    let settings = DspSettings::from_snapshot(snapshot);

    let (out_l, out_r, reports) = process_pair(&settings, &left, &right, SR);
    let stereo = stereo_correlation(&out_l, &out_r);
    let smoothness = max_delta(&out_l);
    let groove_error = groove_lag_error(&left, &out_l);
    let lufs_error = (lufs_like(&left, SR) - lufs_like(&out_l, SR)).abs();
    let min_reduction = reports
        .iter()
        .map(|report| report.gain_reduction_db)
        .fold(0.0_f64, f64::min);

    println!(
        "audio_evaluation stereo_corr={stereo:.3} smoothness={smoothness:.4} groove_error={groove_error:.2} lufs_error={lufs_error:.3} min_gr={min_reduction:.2}dB"
    );

    assert!(stereo.abs() < 0.98);
    assert!(smoothness < 0.72);
    assert!(groove_error < 64.0);
    assert!(lufs_error < 8.0);
    assert!(min_reduction > -18.0);
}

#[test]
fn stress_buffer_timing_denormal_longrun_fuzz() {
    let mut rng = StdRng::seed_from_u64(0x4e43_5354_5245_5353);
    let mut snapshot = musical_snapshot();
    snapshot.set(ControlId::DistortionEnabled, 1.0);
    snapshot.set(ControlId::CompressorEnabled, 1.0);
    let settings = DspSettings::from_snapshot(snapshot);
    let buffer_sizes = [1_usize, 2, 3, 7, 16, 32, 64, 127, 128, 257, 512, 1024, 4096];

    let start = Instant::now();
    for size in buffer_sizes {
        let mut dsp = NebulaClusterDsp::new(SR);
        dsp.prepare(&settings);
        let left: Vec<f64> = (0..size).map(|_| rng.gen_range(-1.0..1.0) * 0.95).collect();
        let right: Vec<f64> = (0..size).map(|_| rng.gen_range(-1.0..1.0) * 0.95).collect();
        for (&l, &r) in left.iter().zip(&right) {
            let out = dsp.process_frame(l, r, &settings);
            assert!(out.out_l.is_finite());
            assert!(out.out_r.is_finite());
        }
    }
    let elapsed = start.elapsed();

    let tiny = vec![1.0e-310; 4096];
    let (denorm_l, denorm_r, _) = process_pair(&settings, &tiny, &tiny, SR);
    assert!(denorm_l
        .iter()
        .chain(&denorm_r)
        .all(|sample| sample.is_finite()));
    assert!(denorm_l
        .iter()
        .chain(&denorm_r)
        .all(|sample| sample.abs() < 1.0e-6));

    for _ in 0..128 {
        let random_settings = DspSettings::from_snapshot(random_snapshot(&mut rng));
        let l = rng.gen_range(-4.0..4.0);
        let r = rng.gen_range(-4.0..4.0);
        let mut dsp = NebulaClusterDsp::new(SR);
        dsp.prepare(&random_settings);
        let out = dsp.process_frame(l, r, &random_settings);
        assert!(out.out_l.is_finite());
        assert!(out.out_r.is_finite());
    }

    println!(
        "stress buffer_sweep_elapsed_ms={:.3}",
        elapsed.as_secs_f64() * 1000.0
    );
    assert!(elapsed.as_secs_f64() < 3.0);
}

#[test]
fn stress_extreme_gain_is_host_safe() {
    let mut snapshot = Snapshot::default();
    snapshot.set(ControlId::InputLevel, 100.0);
    snapshot.set(ControlId::OutputLevel, 100.0);
    snapshot.set(ControlId::DistortionEnabled, 1.0);
    snapshot.set(ControlId::DistSaturation, 1.0);
    snapshot.set(ControlId::Harmonic2, 1.0);
    snapshot.set(ControlId::Harmonic3, 1.0);
    snapshot.set(ControlId::Harmonic4, 1.0);
    snapshot.set(ControlId::Harmonic5, 1.0);
    snapshot.set(ControlId::Harmonic6, 1.0);
    snapshot.set(ControlId::Harmonic7, 1.0);
    snapshot.set(ControlId::DistMix, 1.0);
    snapshot.set(ControlId::FilterEnabled, 1.0);
    snapshot.set(ControlId::FilterHpf, 160.0);
    snapshot.set(ControlId::FilterHpSlope, 100.0);
    snapshot.set(ControlId::FilterHpRes, 1.0);
    snapshot.set(ControlId::FilterLpf, 18_000.0);
    snapshot.set(ControlId::FilterLpSlope, 100.0);
    snapshot.set(ControlId::FilterLpRes, 1.0);
    snapshot.set(ControlId::CompressorEnabled, 1.0);
    snapshot.set(ControlId::CompMode, 2.0);
    snapshot.set(ControlId::CompRatio, 20.0);
    snapshot.set(ControlId::CompMakeup, 24.0);
    snapshot.set(ControlId::CompBoost, 36.0);
    snapshot.set(ControlId::CompAttackThreshold, -80.0);
    snapshot.set(ControlId::CompAttackMs, 0.01);
    snapshot.set(ControlId::CompReleaseMs, 1.0);

    let settings = DspSettings::from_snapshot(snapshot);
    let mut dsp = NebulaClusterDsp::new(SR);
    dsp.prepare(&settings);

    for index in 0..8192 {
        let impulse = if index % 257 == 0 { 1.0 } else { 0.0 };
        let sine = (2.0 * PI * 977.0 * index as f64 / SR).sin() * 1.5;
        let report = dsp.process_frame(impulse + sine, -impulse + sine * 0.7, &settings);
        assert!(report.out_l.is_finite());
        assert!(report.out_r.is_finite());
        assert!(report.out_l.abs() <= SAFETY_CEILING + 1.0e-12);
        assert!(report.out_r.abs() <= SAFETY_CEILING + 1.0e-12);
    }

    snapshot.set(ControlId::FxBypass, 1.0);
    let bypass_settings = DspSettings::from_snapshot(snapshot);
    let mut bypass_dsp = NebulaClusterDsp::new(SR);
    bypass_dsp.prepare(&bypass_settings);
    for _ in 0..512 {
        let report = bypass_dsp.process_frame(2.5, -2.5, &bypass_settings);
        assert!(report.out_l.abs() <= SAFETY_CEILING + 1.0e-12);
        assert!(report.out_r.abs() <= SAFETY_CEILING + 1.0e-12);
    }
}

#[test]
fn stress_parameter_automation_thread_safety_reset_samplerate() {
    let mut dsp = NebulaClusterDsp::new(SR);
    let mut snapshot = musical_snapshot();
    let input = sine(12_000, SR, 440.0, 0.3);
    let mut out = Vec::with_capacity(input.len());

    for (index, sample) in input.iter().copied().enumerate() {
        let t = index as f64 / (input.len() - 1) as f64;
        snapshot.set(ControlId::GlobalMix, t);
        snapshot.set(ControlId::DistSaturation, t * 0.8);
        snapshot.set(ControlId::FilterLpf, 20_000.0 - 12_000.0 * t);
        let settings = DspSettings::from_snapshot(snapshot);
        dsp.prepare(&settings);
        let report = dsp.process_frame(sample, sample, &settings);
        assert!(report.out_l.is_finite());
        out.push(report.out_l);
    }
    assert!(max_delta(&out) < 0.42);

    dsp.reset();
    let default = DspSettings::default();
    dsp.prepare(&default);
    for sample in sine(1024, SR, 880.0, 0.2) {
        let report = dsp.process_frame(sample, -sample, &default);
        assert!((report.out_l - sample).abs() < 1.0e-10);
        assert!((report.out_r + sample).abs() < 1.0e-10);
    }

    for sample_rate in [44_100.0, 48_000.0, 88_200.0, 96_000.0, 192_000.0] {
        let mut rate_dsp = NebulaClusterDsp::new(sample_rate);
        let settings = DspSettings::from_snapshot(musical_snapshot());
        rate_dsp.prepare(&settings);
        for index in 0..2048 {
            let sample = (2.0 * PI * 1000.0 * index as f64 / sample_rate).sin() * 0.2;
            let report = rate_dsp.process_frame(sample, sample, &settings);
            assert!(report.out_l.is_finite());
            assert!(report.out_r.is_finite());
        }
    }

    let handles: Vec<_> = (0..4)
        .map(|thread_index| {
            thread::spawn(move || {
                let mut local = NebulaClusterDsp::new(SR);
                let settings = DspSettings::from_snapshot(musical_snapshot());
                local.prepare(&settings);
                for index in 0..4096 {
                    let sample = ((index + thread_index * 13) as f64 * 0.01).sin() * 0.5;
                    let report = local.process_frame(sample, -sample, &settings);
                    assert!(report.out_l.is_finite());
                    assert!(report.out_r.is_finite());
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("threaded DSP worker completed");
    }
}

fn process_pair(
    settings: &DspSettings,
    left: &[f64],
    right: &[f64],
    sample_rate: f64,
) -> (Vec<f64>, Vec<f64>, Vec<nebula_cluster::dsp::ProcessReport>) {
    let mut dsp = NebulaClusterDsp::new(sample_rate);
    dsp.prepare(settings);
    dsp.reset();
    let mut out_l = Vec::with_capacity(left.len());
    let mut out_r = Vec::with_capacity(left.len());
    let mut reports = Vec::with_capacity(left.len());
    for (&l, &r) in left.iter().zip(right) {
        let report = dsp.process_frame(l, r, settings);
        out_l.push(report.out_l);
        out_r.push(report.out_r);
        reports.push(report);
    }
    (out_l, out_r, reports)
}

fn musical_snapshot() -> Snapshot {
    let mut snapshot = Snapshot::default();
    snapshot.set(ControlId::DistortionEnabled, 1.0);
    snapshot.set(ControlId::DistSaturation, 0.28);
    snapshot.set(ControlId::Harmonic2, 0.18);
    snapshot.set(ControlId::Harmonic3, 0.32);
    snapshot.set(ControlId::Harmonic5, 0.12);
    snapshot.set(ControlId::DistMix, 0.35);
    snapshot
}

fn random_snapshot(rng: &mut StdRng) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for id in ALL_CONTROLS {
        let spec = id.spec();
        let value = match spec.kind {
            nebula_cluster::model::ValueKind::Boolean => {
                if rng.gen_bool(0.5) {
                    1.0
                } else {
                    0.0
                }
            }
            nebula_cluster::model::ValueKind::Choice(labels) => {
                rng.gen_range(0..labels.len()) as f64
            }
            _ => spec.value_from_unit(rng.gen()),
        };
        snapshot.set(id, value);
    }
    snapshot.set(ControlId::InputLevel, rng.gen_range(-24.0..12.0));
    snapshot.set(ControlId::OutputLevel, rng.gen_range(-24.0..6.0));
    snapshot
}

fn multi_source_signal(len: usize, sample_rate: f64) -> Vec<f64> {
    (0..len)
        .map(|index| {
            let t = index as f64 / sample_rate;
            let harmonic = (2.0 * PI * 110.0 * t).sin() * 0.21
                + (2.0 * PI * 440.0 * t).sin() * 0.11
                + (2.0 * PI * 1760.0 * t).sin() * 0.05;
            let transient = if index % 4096 < 18 {
                let local = (index % 4096) as f64;
                0.42 * (1.0 - local / 18.0).max(0.0)
            } else {
                0.0
            };
            harmonic + transient
        })
        .collect()
}

fn sine(len: usize, sample_rate: f64, freq: f64, amp: f64) -> Vec<f64> {
    (0..len)
        .map(|index| (2.0 * PI * freq * index as f64 / sample_rate).sin() * amp)
        .collect()
}

fn groove_signal(len: usize, sample_rate: f64, phase: f64) -> Vec<f64> {
    (0..len)
        .map(|index| {
            let t = index as f64 / sample_rate;
            let beat = ((t * 2.0 + phase).fract() * 48.0).floor() as i32;
            let hit = if matches!(beat, 0 | 12 | 24 | 36) {
                let local = ((t * 2.0 + phase).fract() * 48.0).fract();
                (-local * 24.0).exp() * 0.75
            } else {
                0.0
            };
            hit + (2.0 * PI * 55.0 * t).sin() * 0.18 + (2.0 * PI * 880.0 * t).sin() * 0.04
        })
        .collect()
}

fn max_abs_residual(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

fn spectral_rms_delta_db(a: &[f64], b: &[f64]) -> f64 {
    let mag_a = fft_magnitudes(a);
    let mag_b = fft_magnitudes(b);
    let sum = mag_a
        .iter()
        .zip(&mag_b)
        .map(|(x, y)| {
            let dx = gain_to_db(*x + 1.0e-12) - gain_to_db(*y + 1.0e-12);
            dx * dx
        })
        .sum::<f64>();
    (sum / mag_a.len() as f64).sqrt()
}

fn transient_peak_delta(a: &[f64], b: &[f64]) -> f64 {
    let peak_a = a.iter().map(|sample| sample.abs()).fold(0.0, f64::max);
    let peak_b = b.iter().map(|sample| sample.abs()).fold(0.0, f64::max);
    (peak_a - peak_b).abs()
}

fn fft_magnitudes(signal: &[f64]) -> Vec<f64> {
    let len = signal.len().next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(len);
    let mut buffer = vec![Complex::new(0.0, 0.0); len];
    for (index, sample) in signal.iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * PI * index as f64 / (signal.len() - 1) as f64).cos();
        buffer[index] = Complex::new(sample * window, 0.0);
    }
    fft.process(&mut buffer);
    buffer[..(len / 2)]
        .iter()
        .map(|value| value.norm() / len as f64)
        .collect()
}

fn harmonic_musicality_index(signal: &[f64], sample_rate: f64, fundamental: f64) -> f64 {
    let mags = fft_magnitudes(signal);
    let bin_hz = sample_rate / (mags.len() * 2) as f64;
    let mut harmonic = 0.0;
    let mut non_harmonic = 0.0;
    for (index, mag) in mags.iter().enumerate().skip(1) {
        let freq = index as f64 * bin_hz;
        let harmonic_number = (freq / fundamental).round();
        let harmonic_freq = harmonic_number * fundamental;
        if (2.0..=8.0).contains(&harmonic_number) && (freq - harmonic_freq).abs() < bin_hz * 1.5 {
            harmonic += mag * mag;
        } else if freq > fundamental * 1.3 && freq < fundamental * 9.0 {
            non_harmonic += mag * mag;
        }
    }
    harmonic / (non_harmonic + 1.0e-18)
}

fn non_harmonic_rejection_db(signal: &[f64], sample_rate: f64, fundamental: f64) -> f64 {
    gain_to_db(harmonic_musicality_index(signal, sample_rate, fundamental).sqrt())
}

fn golden_reference_distance(input: &[f64], processed: &[f64], sample_rate: f64) -> f64 {
    let input_lufs = lufs_like(input, sample_rate);
    let processed_lufs = lufs_like(processed, sample_rate);
    let spectral =
        spectral_centroid(processed, sample_rate) / spectral_centroid(input, sample_rate);
    let crest_delta = (crest_factor(input) - crest_factor(processed)).abs();
    ((processed_lufs - input_lufs).abs() / 18.0 + (spectral - 1.0).abs() + crest_delta / 12.0) / 3.0
}

fn lufs_like(signal: &[f64], sample_rate: f64) -> f64 {
    let mut hp = TestBiquad::highpass(38.0, 0.5, sample_rate);
    let mut shelf = TestBiquad::high_shelf(1681.974, 0.707, 4.0, sample_rate);
    let mut weighted = Vec::with_capacity(signal.len());
    for sample in signal {
        weighted.push(shelf.process(hp.process(*sample)));
    }
    let block = (sample_rate * 0.400) as usize;
    let mut energies = Vec::new();
    for chunk in weighted.chunks(block.max(1)) {
        let mean_square =
            chunk.iter().map(|sample| sample * sample).sum::<f64>() / chunk.len() as f64;
        let loudness = -0.691 + 10.0 * mean_square.max(1.0e-18).log10();
        if loudness > -70.0 {
            energies.push(mean_square);
        }
    }
    let gated = if energies.is_empty() {
        1.0e-18
    } else {
        energies.iter().sum::<f64>() / energies.len() as f64
    };
    -0.691 + 10.0 * gated.max(1.0e-18).log10()
}

fn spectral_centroid(signal: &[f64], sample_rate: f64) -> f64 {
    let mags = fft_magnitudes(signal);
    let bin_hz = sample_rate / (mags.len() * 2) as f64;
    let mut weighted = 0.0;
    let mut total = 0.0;
    for (index, mag) in mags.iter().enumerate().skip(1) {
        weighted += index as f64 * bin_hz * mag;
        total += mag;
    }
    weighted / total.max(1.0e-18)
}

fn crest_factor(signal: &[f64]) -> f64 {
    let peak = signal.iter().map(|sample| sample.abs()).fold(0.0, f64::max);
    gain_to_db(peak / rms(signal).max(1.0e-18))
}

fn rms(signal: &[f64]) -> f64 {
    (signal.iter().map(|sample| sample * sample).sum::<f64>() / signal.len() as f64).sqrt()
}

fn stereo_correlation(left: &[f64], right: &[f64]) -> f64 {
    let mut xy = 0.0;
    let mut xx = 0.0;
    let mut yy = 0.0;
    for (&l, &r) in left.iter().zip(right) {
        xy += l * r;
        xx += l * l;
        yy += r * r;
    }
    xy / (xx.sqrt() * yy.sqrt()).max(1.0e-18)
}

fn max_delta(signal: &[f64]) -> f64 {
    signal
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f64::max)
}

fn groove_lag_error(reference: &[f64], processed: &[f64]) -> f64 {
    let max_lag = 256_i32;
    let mut best_lag = 0_i32;
    let mut best_score = f64::MIN;
    for lag in -max_lag..=max_lag {
        let mut score = 0.0;
        for (index, sample) in reference.iter().enumerate() {
            let shifted = index as i32 + lag;
            if (0..processed.len() as i32).contains(&shifted) {
                score += sample.abs() * processed[shifted as usize].abs();
            }
        }
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    best_lag.abs() as f64
}

#[derive(Clone, Copy)]
struct TestBiquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl TestBiquad {
    fn highpass(freq: f64, q: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * freq / sample_rate;
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 + cos) * 0.5) / a0,
            b1: (-(1.0 + cos)) / a0,
            b2: ((1.0 + cos) * 0.5) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn high_shelf(freq: f64, q: f64, gain_db: f64, sample_rate: f64) -> Self {
        let omega = 2.0 * PI * freq / sample_rate;
        let sin = omega.sin();
        let cos = omega.cos();
        let a = 10.0_f64.powf(gain_db / 40.0);
        let alpha = sin / (2.0 * q);
        let beta = 2.0 * a.sqrt() * alpha;
        let a0 = (a + 1.0) - (a - 1.0) * cos + beta;
        Self {
            b0: a * ((a + 1.0) + (a - 1.0) * cos + beta) / a0,
            b1: -2.0 * a * ((a - 1.0) + (a + 1.0) * cos) / a0,
            b2: a * ((a + 1.0) + (a - 1.0) * cos - beta) / a0,
            a1: 2.0 * ((a - 1.0) - (a + 1.0) * cos) / a0,
            a2: ((a + 1.0) - (a - 1.0) * cos - beta) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}
