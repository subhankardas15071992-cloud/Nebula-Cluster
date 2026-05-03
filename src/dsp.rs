use crate::model::{ControlId, Snapshot};
use std::f64::consts::PI;

pub const SAFETY_CEILING: f64 = 0.966_050_878_989_813_1;
const COMPRESSOR_NOISE_FLOOR_DB: f64 = -96.0;
const COMPRESSOR_NOISE_FADE_DB: f64 = 24.0;
const DEFIZZ_MIN_HZ: f64 = 13_500.0;
const DEFIZZ_MAX_HZ: f64 = 20_000.0;

#[inline]
pub fn db_to_gain(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

#[inline]
pub fn gain_to_db(gain: f64) -> f64 {
    if gain <= 1.0e-12 {
        -240.0
    } else {
        20.0 * gain.abs().log10()
    }
}

#[inline]
fn sanitize(value: f64) -> f64 {
    if value.is_finite() && value.abs() >= 1.0e-30 {
        value.clamp(-64.0, 64.0)
    } else {
        0.0
    }
}

#[inline]
fn safety_clip(value: f64) -> f64 {
    if value.is_finite() && value.abs() >= 1.0e-30 {
        value.clamp(-SAFETY_CEILING, SAFETY_CEILING)
    } else {
        0.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompressorMode {
    Downward,
    Upward,
    Boosting,
}

#[derive(Clone, Copy, Debug)]
pub struct DspSettings {
    pub input_level_db: f64,
    pub input_pan: f64,
    pub output_level_db: f64,
    pub output_pan: f64,
    pub global_mix: f64,
    pub global_phase: bool,
    pub fx_bypass: bool,
    pub distortion_enabled: bool,
    pub saturation: f64,
    pub harmonics: [f64; 6],
    pub dist_mix: f64,
    pub dist_phase: bool,
    pub dist_hpf_hz: f64,
    pub dist_hp_slope: f64,
    pub dist_lpf_hz: f64,
    pub dist_lp_slope: f64,
    pub filter_enabled: bool,
    pub filter_hpf_hz: f64,
    pub filter_hp_slope: f64,
    pub filter_hp_res: f64,
    pub filter_lpf_hz: f64,
    pub filter_lp_slope: f64,
    pub filter_lp_res: f64,
    pub compressor_enabled: bool,
    pub compressor_mode: CompressorMode,
    pub ratio: f64,
    pub knee_db: f64,
    pub makeup_db: f64,
    pub boost_db: f64,
    pub attack_threshold_db: f64,
    pub attack_ms: f64,
    pub release_threshold_db: f64,
    pub release_ms: f64,
    pub hold_ms: f64,
}

impl Default for DspSettings {
    fn default() -> Self {
        Self::from_snapshot(Snapshot::default())
    }
}

impl DspSettings {
    pub fn from_snapshot(snapshot: Snapshot) -> Self {
        let mode = match snapshot.choice(ControlId::CompMode) {
            1 => CompressorMode::Upward,
            2 => CompressorMode::Boosting,
            _ => CompressorMode::Downward,
        };

        Self {
            input_level_db: snapshot.get(ControlId::InputLevel),
            input_pan: snapshot.get(ControlId::InputPan),
            output_level_db: snapshot.get(ControlId::OutputLevel),
            output_pan: snapshot.get(ControlId::OutputPan),
            global_mix: snapshot.get(ControlId::GlobalMix),
            global_phase: snapshot.bool(ControlId::GlobalPhase),
            fx_bypass: snapshot.bool(ControlId::FxBypass),
            distortion_enabled: snapshot.bool(ControlId::DistortionEnabled),
            saturation: snapshot.get(ControlId::DistSaturation),
            harmonics: [
                snapshot.get(ControlId::Harmonic2),
                snapshot.get(ControlId::Harmonic3),
                snapshot.get(ControlId::Harmonic4),
                snapshot.get(ControlId::Harmonic5),
                snapshot.get(ControlId::Harmonic6),
                snapshot.get(ControlId::Harmonic7),
            ],
            dist_mix: snapshot.get(ControlId::DistMix),
            dist_phase: snapshot.bool(ControlId::DistPhase),
            dist_hpf_hz: snapshot.get(ControlId::DistHpf),
            dist_hp_slope: snapshot.get(ControlId::DistHpSlope),
            dist_lpf_hz: snapshot.get(ControlId::DistLpf),
            dist_lp_slope: snapshot.get(ControlId::DistLpSlope),
            filter_enabled: snapshot.bool(ControlId::FilterEnabled),
            filter_hpf_hz: snapshot.get(ControlId::FilterHpf),
            filter_hp_slope: snapshot.get(ControlId::FilterHpSlope),
            filter_hp_res: snapshot.get(ControlId::FilterHpRes),
            filter_lpf_hz: snapshot.get(ControlId::FilterLpf),
            filter_lp_slope: snapshot.get(ControlId::FilterLpSlope),
            filter_lp_res: snapshot.get(ControlId::FilterLpRes),
            compressor_enabled: snapshot.bool(ControlId::CompressorEnabled),
            compressor_mode: mode,
            ratio: snapshot.get(ControlId::CompRatio),
            knee_db: snapshot.get(ControlId::CompKnee),
            makeup_db: snapshot.get(ControlId::CompMakeup),
            boost_db: snapshot.get(ControlId::CompBoost),
            attack_threshold_db: snapshot.get(ControlId::CompAttackThreshold),
            attack_ms: snapshot.get(ControlId::CompAttackMs),
            release_threshold_db: snapshot.get(ControlId::CompReleaseThreshold),
            release_ms: snapshot.get(ControlId::CompReleaseMs),
            hold_ms: snapshot.get(ControlId::CompHold),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessReport {
    pub out_l: f64,
    pub out_r: f64,
    pub peak_db: f64,
    pub gain_reduction_db: f64,
}

#[derive(Clone, Copy, Debug)]
struct OnePoleSmoother {
    coeff: f64,
    current: f64,
}

impl OnePoleSmoother {
    fn new(sample_rate: f64, time_ms: f64, initial: f64) -> Self {
        let mut smoother = Self {
            coeff: 0.0,
            current: initial,
        };
        smoother.set_time(sample_rate, time_ms);
        smoother
    }

    fn set_time(&mut self, sample_rate: f64, time_ms: f64) {
        self.coeff = smoothing_coeff(time_ms, sample_rate);
    }

    fn reset(&mut self, value: f64) {
        self.current = value;
    }

    #[inline]
    fn next(&mut self, target: f64) -> f64 {
        self.current = target + self.coeff * (self.current - target);
        sanitize(self.current)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct OnePole {
    z: f64,
}

impl OnePole {
    #[inline]
    fn process(&mut self, input: f64, alpha: f64) -> f64 {
        self.z += alpha * (input - self.z);
        self.z = sanitize(self.z);
        self.z
    }

    fn reset(&mut self) {
        self.z = 0.0;
    }
}

#[derive(Clone, Debug)]
struct ComplementaryLowpass {
    sample_rate: f64,
    stages: [OnePole; 9],
}

impl ComplementaryLowpass {
    fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            stages: [OnePole::default(); 9],
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
    }

    fn reset(&mut self) {
        for stage in &mut self.stages {
            stage.reset();
        }
    }

    #[inline]
    fn lowpass(&mut self, input: f64, cutoff_hz: f64, slope_db_oct: f64) -> f64 {
        let slope = slope_db_oct.clamp(0.0, 100.0);
        if slope <= 1.0e-9 {
            return input;
        }

        let cutoff = cutoff_hz.clamp(1.0, self.sample_rate * 0.49);
        let alpha = 1.0 - (-2.0 * PI * cutoff / self.sample_rate).exp();
        let order = (slope / 12.0).clamp(0.0, self.stages.len() as f64 - 1.0);
        let order_floor = order.floor() as usize;
        let order_frac = order - order_floor as f64;

        let mut stage_input = input;
        let mut outputs = [input; 10];
        outputs[0] = input;
        for (index, stage) in self.stages.iter_mut().enumerate() {
            stage_input = stage.process(stage_input, alpha);
            outputs[index + 1] = stage_input;
        }

        let a = outputs[order_floor];
        let b = outputs[(order_floor + 1).min(self.stages.len())];
        sanitize(a + (b - a) * order_frac)
    }

    #[inline]
    fn highpass_path(&mut self, input: f64, cutoff_hz: f64, slope_db_oct: f64) -> (f64, f64) {
        let low = self.lowpass(input, cutoff_hz, slope_db_oct);
        let high = input - low;
        let amount = (slope_db_oct / 100.0).clamp(0.0, 1.0);
        let passed = high + low * (1.0 - amount);
        (sanitize(passed), sanitize(input - passed))
    }

    #[inline]
    fn lowpass_path(&mut self, input: f64, cutoff_hz: f64, slope_db_oct: f64) -> (f64, f64) {
        let low = self.lowpass(input, cutoff_hz, slope_db_oct);
        let high = input - low;
        let amount = (slope_db_oct / 100.0).clamp(0.0, 1.0);
        let passed = low + high * (1.0 - amount);
        (sanitize(passed), sanitize(input - passed))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BiquadCoeffs {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

#[derive(Clone, Copy, Debug, Default)]
struct BiquadState {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl BiquadCoeffs {
    fn bandpass(freq_hz: f64, q: f64, sample_rate: f64) -> Self {
        let freq = freq_hz.clamp(1.0, sample_rate * 0.49);
        let omega = 2.0 * PI * freq / sample_rate.max(1.0);
        let sin = omega.sin();
        let cos = omega.cos();
        let alpha = sin / (2.0 * q.max(1.0e-6));
        let a0 = 1.0 + alpha;

        Self {
            b0: alpha / a0,
            b1: 0.0,
            b2: -alpha / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    #[inline]
    fn process(self, state: &mut BiquadState, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * state.x1 + self.b2 * state.x2
            - self.a1 * state.y1
            - self.a2 * state.y2;

        state.x2 = state.x1;
        state.x1 = sanitize(input);
        state.y2 = state.y1;
        state.y1 = sanitize(output);
        state.y1
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct DcBlocker {
    x1: f64,
    y1: f64,
}

impl DcBlocker {
    #[inline]
    fn process(&mut self, input: f64) -> f64 {
        let output = input - self.x1 + 0.995 * self.y1;
        self.x1 = sanitize(input);
        self.y1 = sanitize(output);
        self.y1
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }
}

#[derive(Clone, Debug)]
struct ChannelDsp {
    sample_rate: f64,
    dist_hpf: ComplementaryLowpass,
    dist_lpf: ComplementaryLowpass,
    filter_hpf: ComplementaryLowpass,
    filter_lpf: ComplementaryLowpass,
    dist_dc: DcBlocker,
    dist_defizz: OnePole,
    filter_hp_peak: BiquadState,
    filter_lp_peak: BiquadState,
}

impl ChannelDsp {
    fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            dist_hpf: ComplementaryLowpass::new(sample_rate),
            dist_lpf: ComplementaryLowpass::new(sample_rate),
            filter_hpf: ComplementaryLowpass::new(sample_rate),
            filter_lpf: ComplementaryLowpass::new(sample_rate),
            dist_dc: DcBlocker::default(),
            dist_defizz: OnePole::default(),
            filter_hp_peak: BiquadState::default(),
            filter_lp_peak: BiquadState::default(),
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
        self.dist_hpf.set_sample_rate(sample_rate);
        self.dist_lpf.set_sample_rate(sample_rate);
        self.filter_hpf.set_sample_rate(sample_rate);
        self.filter_lpf.set_sample_rate(sample_rate);
    }

    fn reset(&mut self) {
        self.dist_hpf.reset();
        self.dist_lpf.reset();
        self.filter_hpf.reset();
        self.filter_lpf.reset();
        self.dist_dc.reset();
        self.dist_defizz.reset();
        self.filter_hp_peak = BiquadState::default();
        self.filter_lp_peak = BiquadState::default();
    }
}

#[derive(Clone, Debug)]
struct StereoCompressor {
    sample_rate: f64,
    envelope: f64,
    gain_smoother: OnePoleSmoother,
    hold_counter: usize,
}

impl StereoCompressor {
    fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            envelope: 0.0,
            gain_smoother: OnePoleSmoother::new(sample_rate, 5.0, 1.0),
            hold_counter: 0,
        }
    }

    fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
        self.gain_smoother.set_time(self.sample_rate, 5.0);
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
        self.gain_smoother.reset(1.0);
        self.hold_counter = 0;
    }

    fn process(&mut self, left: f64, right: f64, settings: &DspSettings) -> (f64, f64, f64) {
        let detector = left.abs().max(right.abs());
        let attack_coeff = smoothing_coeff(settings.attack_ms, self.sample_rate);
        let release_coeff = smoothing_coeff(settings.release_ms, self.sample_rate);

        let env_db = gain_to_db(self.envelope.max(1.0e-12));
        let release_gate_db = settings.attack_threshold_db + settings.release_threshold_db;
        let is_release = detector < self.envelope;
        let coeff = if detector > self.envelope {
            self.hold_counter = ms_to_samples(settings.hold_ms, self.sample_rate);
            attack_coeff
        } else if self.hold_counter > 0 {
            self.hold_counter -= 1;
            0.0
        } else if is_release && env_db <= release_gate_db {
            attack_coeff
        } else {
            release_coeff
        };

        self.envelope = detector + coeff * (self.envelope - detector);
        self.envelope = sanitize(self.envelope.max(0.0));

        let env_db = gain_to_db(self.envelope.max(1.0e-12));
        let mut target_gain_db = compressor_gain_db(env_db, settings) + settings.makeup_db;
        if matches!(
            settings.compressor_mode,
            CompressorMode::Upward | CompressorMode::Boosting
        ) && target_gain_db > 0.0
        {
            target_gain_db *= noise_floor_weight(env_db);
        }
        let target_gain = db_to_gain(target_gain_db);
        let smooth_gain = self.gain_smoother.next(target_gain);
        let reduction_db = gain_to_db(smooth_gain).min(0.0);
        (left * smooth_gain, right * smooth_gain, reduction_db)
    }
}

#[derive(Clone, Debug)]
struct StereoSafetyLimiter {
    sample_rate: f64,
    release_coeff: f64,
    gain: f64,
}

impl StereoSafetyLimiter {
    fn new(sample_rate: f64) -> Self {
        let mut limiter = Self {
            sample_rate: sample_rate.max(1.0),
            release_coeff: 0.0,
            gain: 1.0,
        };
        limiter.set_sample_rate(sample_rate);
        limiter
    }

    fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
        self.release_coeff = smoothing_coeff(55.0, self.sample_rate);
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }

    #[inline]
    fn process(&mut self, left: f64, right: f64) -> (f64, f64, f64) {
        let left = sanitize(left);
        let right = sanitize(right);
        let peak = left.abs().max(right.abs());
        let target_gain = if peak > SAFETY_CEILING {
            (SAFETY_CEILING / peak).clamp(0.0, 1.0)
        } else {
            1.0
        };

        if target_gain < self.gain {
            self.gain = target_gain;
        } else {
            self.gain = target_gain + self.release_coeff * (self.gain - target_gain);
        }

        self.gain = self.gain.clamp(0.0, 1.0);
        (
            safety_clip(left * self.gain),
            safety_clip(right * self.gain),
            gain_to_db(self.gain.max(1.0e-12)).min(0.0),
        )
    }
}

pub struct NebulaClusterDsp {
    sample_rate: f64,
    channels: [ChannelDsp; 2],
    compressor: StereoCompressor,
    safety_limiter: StereoSafetyLimiter,
    input_gain_l: OnePoleSmoother,
    input_gain_r: OnePoleSmoother,
    output_gain_l: OnePoleSmoother,
    output_gain_r: OnePoleSmoother,
    global_mix: OnePoleSmoother,
    bypass: OnePoleSmoother,
    filter_hp_peak_coeff: BiquadCoeffs,
    filter_lp_peak_coeff: BiquadCoeffs,
}

impl NebulaClusterDsp {
    pub fn new(sample_rate: f64) -> Self {
        let sample_rate = sample_rate.max(1.0);
        Self {
            sample_rate,
            channels: [ChannelDsp::new(sample_rate), ChannelDsp::new(sample_rate)],
            compressor: StereoCompressor::new(sample_rate),
            safety_limiter: StereoSafetyLimiter::new(sample_rate),
            input_gain_l: OnePoleSmoother::new(sample_rate, 15.0, 1.0),
            input_gain_r: OnePoleSmoother::new(sample_rate, 15.0, 1.0),
            output_gain_l: OnePoleSmoother::new(sample_rate, 15.0, 1.0),
            output_gain_r: OnePoleSmoother::new(sample_rate, 15.0, 1.0),
            global_mix: OnePoleSmoother::new(sample_rate, 12.0, 1.0),
            bypass: OnePoleSmoother::new(sample_rate, 12.0, 0.0),
            filter_hp_peak_coeff: BiquadCoeffs::default(),
            filter_lp_peak_coeff: BiquadCoeffs::default(),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate.max(1.0);
        for channel in &mut self.channels {
            channel.set_sample_rate(self.sample_rate);
        }
        self.compressor.set_sample_rate(self.sample_rate);
        self.safety_limiter.set_sample_rate(self.sample_rate);
        self.input_gain_l.set_time(self.sample_rate, 15.0);
        self.input_gain_r.set_time(self.sample_rate, 15.0);
        self.output_gain_l.set_time(self.sample_rate, 15.0);
        self.output_gain_r.set_time(self.sample_rate, 15.0);
        self.global_mix.set_time(self.sample_rate, 12.0);
        self.bypass.set_time(self.sample_rate, 12.0);
    }

    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.reset();
        }
        self.compressor.reset();
        self.safety_limiter.reset();
        self.input_gain_l.reset(1.0);
        self.input_gain_r.reset(1.0);
        self.output_gain_l.reset(1.0);
        self.output_gain_r.reset(1.0);
        self.global_mix.reset(1.0);
        self.bypass.reset(0.0);
    }

    pub fn prepare(&mut self, settings: &DspSettings) {
        self.filter_hp_peak_coeff =
            BiquadCoeffs::bandpass(settings.filter_hpf_hz, 0.85, self.sample_rate);
        self.filter_lp_peak_coeff =
            BiquadCoeffs::bandpass(settings.filter_lpf_hz, 0.85, self.sample_rate);
    }

    #[inline]
    pub fn process_frame(
        &mut self,
        input_l: f64,
        input_r: f64,
        settings: &DspSettings,
    ) -> ProcessReport {
        let dry_l = sanitize(input_l);
        let dry_r = sanitize(input_r);
        let input_gain = db_to_gain(settings.input_level_db);
        let (target_input_l, target_input_r) = pan_gains(settings.input_pan, input_gain);
        let x_l = dry_l * self.input_gain_l.next(target_input_l);
        let x_r = dry_r * self.input_gain_r.next(target_input_r);

        let mut wet_l = x_l;
        let mut wet_r = x_r;

        if settings.distortion_enabled {
            wet_l = process_distortion_channel(wet_l, &mut self.channels[0], settings);
            wet_r = process_distortion_channel(wet_r, &mut self.channels[1], settings);
        }

        if settings.filter_enabled {
            wet_l = self.process_filter_channel(wet_l, 0, settings);
            wet_r = self.process_filter_channel(wet_r, 1, settings);
        }

        let mut gain_reduction_db = 0.0;
        if settings.compressor_enabled {
            let compressed = self.compressor.process(wet_l, wet_r, settings);
            wet_l = compressed.0;
            wet_r = compressed.1;
            gain_reduction_db = compressed.2;
        }

        if settings.global_phase {
            wet_l = -wet_l;
            wet_r = -wet_r;
        }

        let mix = self.global_mix.next(settings.global_mix.clamp(0.0, 1.0));
        let active_l = x_l * (1.0 - mix) + wet_l * mix;
        let active_r = x_r * (1.0 - mix) + wet_r * mix;

        let output_gain = db_to_gain(settings.output_level_db);
        let (target_output_l, target_output_r) = pan_gains(settings.output_pan, output_gain);
        let active_l = active_l * self.output_gain_l.next(target_output_l);
        let active_r = active_r * self.output_gain_r.next(target_output_r);

        let bypass = self.bypass.next(if settings.fx_bypass { 1.0 } else { 0.0 });
        let out_l = active_l * (1.0 - bypass) + dry_l * bypass;
        let out_r = active_r * (1.0 - bypass) + dry_r * bypass;
        let safety = self.safety_limiter.process(out_l, out_r);
        let out_l = safety.0;
        let out_r = safety.1;
        gain_reduction_db = gain_reduction_db.min(safety.2);
        let peak_db = gain_to_db(out_l.abs().max(out_r.abs()).max(1.0e-12));

        ProcessReport {
            out_l,
            out_r,
            peak_db,
            gain_reduction_db,
        }
    }

    #[inline]
    fn process_filter_channel(
        &mut self,
        input: f64,
        channel_index: usize,
        settings: &DspSettings,
    ) -> f64 {
        let channel = &mut self.channels[channel_index];
        let hp_peak = self
            .filter_hp_peak_coeff
            .process(&mut channel.filter_hp_peak, input)
            * settings.filter_hp_res
            * 0.75;
        let (after_hpf, _) = channel.filter_hpf.highpass_path(
            input,
            settings.filter_hpf_hz,
            settings.filter_hp_slope,
        );
        let after_hpf = sanitize(after_hpf + hp_peak);
        let lp_peak = self
            .filter_lp_peak_coeff
            .process(&mut channel.filter_lp_peak, after_hpf)
            * settings.filter_lp_res
            * 0.75;
        let (after_lpf, _) = channel.filter_lpf.lowpass_path(
            after_hpf,
            settings.filter_lpf_hz,
            settings.filter_lp_slope,
        );
        sanitize(after_lpf + lp_peak)
    }
}

impl Default for NebulaClusterDsp {
    fn default() -> Self {
        Self::new(44_100.0)
    }
}

fn process_distortion_channel(input: f64, channel: &mut ChannelDsp, settings: &DspSettings) -> f64 {
    let (above_hpf, below_hpf) =
        channel
            .dist_hpf
            .highpass_path(input, settings.dist_hpf_hz, settings.dist_hp_slope);
    let (band, above_lpf) =
        channel
            .dist_lpf
            .lowpass_path(above_hpf, settings.dist_lpf_hz, settings.dist_lp_slope);
    let mut shaped = harmonic_shaper(band, settings.saturation, settings.harmonics);
    shaped = channel.dist_dc.process(shaped);
    shaped = process_defizz(shaped, channel, settings);
    if settings.dist_phase {
        shaped = -shaped;
    }
    let mix = settings.dist_mix.clamp(0.0, 1.0);
    sanitize(below_hpf + above_lpf + band * (1.0 - mix) + shaped * mix)
}

fn harmonic_shaper(input: f64, saturation: f64, harmonics: [f64; 6]) -> f64 {
    let saturation = saturation.clamp(0.0, 1.0);
    if saturation <= 1.0e-9 || harmonics.iter().all(|value| *value <= 1.0e-9) {
        return input;
    }

    let drive = 1.0 + saturation * 11.0;
    let x = (input * drive).tanh();
    let x2 = x * x;
    let x3 = x2 * x;
    let x4 = x2 * x2;
    let x5 = x4 * x;
    let x6 = x3 * x3;
    let x7 = x6 * x;

    let cheb = [
        2.0 * x2,
        4.0 * x3 - 3.0 * x,
        8.0 * x4 - 8.0 * x2,
        16.0 * x5 - 20.0 * x3 + 5.0 * x,
        32.0 * x6 - 48.0 * x4 + 18.0 * x2,
        64.0 * x7 - 112.0 * x5 + 56.0 * x3 - 7.0 * x,
    ];

    let harmonic_sum = harmonics
        .iter()
        .zip(cheb)
        .fold(0.0, |acc, (amount, basis)| {
            acc + amount.clamp(0.0, 1.0) * basis
        });
    let shaped = x + harmonic_sum * saturation * 0.075;
    let normalized = shaped.tanh() / drive.tanh().max(1.0e-6);
    sanitize(input * (1.0 - saturation) + normalized * saturation)
}

fn process_defizz(input: f64, channel: &mut ChannelDsp, settings: &DspSettings) -> f64 {
    let mix = settings.dist_mix.clamp(0.0, 1.0);
    let saturation = settings.saturation.clamp(0.0, 1.0);
    if mix <= 1.0e-9 || saturation <= 1.0e-9 {
        return input;
    }

    let high_order = (settings.harmonics[3] * 0.35
        + settings.harmonics[4] * 0.45
        + settings.harmonics[5] * 0.55)
        / 1.35;
    let intensity = (saturation * (0.55 + high_order.clamp(0.0, 1.0) * 0.45)).clamp(0.0, 1.0);
    let max_cutoff = (channel.sample_rate * 0.45).max(1.0);
    let min_cutoff = DEFIZZ_MIN_HZ.min(max_cutoff);
    let cutoff = (DEFIZZ_MAX_HZ - (DEFIZZ_MAX_HZ - DEFIZZ_MIN_HZ) * intensity.powf(0.85))
        .clamp(min_cutoff, max_cutoff);
    channel
        .dist_defizz
        .process(input, lowpass_alpha(cutoff, channel.sample_rate))
}

fn compressor_gain_db(env_db: f64, settings: &DspSettings) -> f64 {
    let ratio = settings.ratio.max(1.0);
    let knee = settings.knee_db.max(0.0);
    let threshold = settings.attack_threshold_db;
    match settings.compressor_mode {
        CompressorMode::Downward => {
            let over = soft_knee(env_db - threshold, knee);
            -over * (1.0 - 1.0 / ratio)
        }
        CompressorMode::Upward => {
            let under = soft_knee(threshold - env_db, knee);
            (under * (1.0 - 1.0 / ratio)).clamp(0.0, settings.boost_db.max(0.0))
        }
        CompressorMode::Boosting => {
            let boost = settings.boost_db.max(0.0);
            let computed_threshold = threshold - boost * 0.5;
            let over = soft_knee(env_db - computed_threshold, knee);
            (boost - over * (1.0 - 1.0 / ratio)).clamp(0.0, boost)
        }
    }
}

fn noise_floor_weight(env_db: f64) -> f64 {
    let unit = ((env_db - COMPRESSOR_NOISE_FLOOR_DB) / COMPRESSOR_NOISE_FADE_DB).clamp(0.0, 1.0);
    unit * unit * (3.0 - 2.0 * unit)
}

#[inline]
fn soft_knee(distance_db: f64, knee_db: f64) -> f64 {
    if knee_db <= 1.0e-9 {
        distance_db.max(0.0)
    } else if distance_db <= -knee_db * 0.5 {
        0.0
    } else if distance_db >= knee_db * 0.5 {
        distance_db
    } else {
        let x = distance_db + knee_db * 0.5;
        (x * x) / (2.0 * knee_db)
    }
}

#[inline]
fn pan_gains(pan: f64, gain: f64) -> (f64, f64) {
    let pan = pan.clamp(-1.0, 1.0);
    let left = if pan > 0.0 { 1.0 - pan } else { 1.0 };
    let right = if pan < 0.0 { 1.0 + pan } else { 1.0 };
    (gain * left, gain * right)
}

#[inline]
fn smoothing_coeff(time_ms: f64, sample_rate: f64) -> f64 {
    let time_ms = time_ms.max(0.001);
    (-1.0 / (time_ms * 0.001 * sample_rate.max(1.0))).exp()
}

#[inline]
fn lowpass_alpha(cutoff_hz: f64, sample_rate: f64) -> f64 {
    1.0 - (-2.0 * PI * cutoff_hz.clamp(1.0, sample_rate * 0.49) / sample_rate.max(1.0)).exp()
}

#[inline]
fn ms_to_samples(time_ms: f64, sample_rate: f64) -> usize {
    ((time_ms.max(0.0) * sample_rate.max(1.0)) * 0.001).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_transparent_after_reset() {
        let mut dsp = NebulaClusterDsp::new(48_000.0);
        let settings = DspSettings::default();
        dsp.prepare(&settings);
        dsp.reset();

        for index in 0..256 {
            let sample = (index as f64 * 0.03).sin() * 0.25;
            let out = dsp.process_frame(sample, -sample, &settings);
            assert!((out.out_l - sample).abs() < 1.0e-12);
            assert!((out.out_r + sample).abs() < 1.0e-12);
        }
    }
}
