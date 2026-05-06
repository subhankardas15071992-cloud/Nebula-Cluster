use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use std::sync::Arc;

use nih_plug::prelude::*;
#[cfg(not(target_os = "windows"))]
use nih_plug_egui::{create_egui_editor, egui::Context, EguiState};
use parking_lot::Mutex;

pub mod analyzer;
pub mod dsp;
#[cfg(not(target_os = "windows"))]
mod gui;
pub mod model;
#[cfg(target_os = "windows")]
mod windows_editor;

use analyzer::SpectrumAnalyzer;
use dsp::{DspSettings, NebulaClusterDsp};
#[cfg(not(target_os = "windows"))]
use gui::{draw, GuiParams, MeterSnapshot, NebulaClusterGui};
use model::{format_value, parse_value, ControlId, Snapshot, ValueKind, CONTROL_COUNT};

const UNMAPPED_CC: i32 = -1;
pub const MIDI_WAITING_FOR_CONTROL: i32 = -2;

#[inline]
fn f32_to_u32(value: f32) -> u32 {
    value.to_bits()
}

#[inline]
fn u32_to_f32(value: u32) -> f32 {
    f32::from_bits(value)
}

pub struct MidiLearnShared {
    pub learning_target: AtomicI32,
    pub mappings: Mutex<HashMap<u8, u8>>,
    pub saved_mappings: Mutex<HashMap<u8, u8>>,
    pub midi_enabled: AtomicBool,
    pub cc_values: Vec<AtomicU32>,
    pub cc_dirty: Vec<AtomicBool>,
    cc_bindings: Vec<AtomicI32>,
    bindings_dirty: AtomicBool,
}

impl MidiLearnShared {
    fn new() -> Self {
        Self {
            learning_target: AtomicI32::new(UNMAPPED_CC),
            mappings: Mutex::new(HashMap::new()),
            saved_mappings: Mutex::new(HashMap::new()),
            midi_enabled: AtomicBool::new(true),
            cc_values: (0..128).map(|_| AtomicU32::new(0)).collect(),
            cc_dirty: (0..128).map(|_| AtomicBool::new(false)).collect(),
            cc_bindings: (0..128).map(|_| AtomicI32::new(UNMAPPED_CC)).collect(),
            bindings_dirty: AtomicBool::new(false),
        }
    }

    fn binding_for_cc(&self, cc: usize) -> Option<ControlId> {
        let binding = self.cc_bindings[cc.min(127)].load(Ordering::Acquire);
        if binding >= 0 {
            ControlId::from_index(binding as usize)
        } else {
            None
        }
    }

    fn learn_cc(&self, cc: u8, control: ControlId) {
        self.cc_bindings[cc as usize].store(control.index() as i32, Ordering::Release);
        self.bindings_dirty.store(true, Ordering::Release);
    }

    pub fn sync_mutex_from_atomic_if_needed(&self) {
        if !self.bindings_dirty.swap(false, Ordering::AcqRel) {
            return;
        }

        let mut mappings = self.mappings.lock();
        mappings.clear();
        for (cc, binding) in self.cc_bindings.iter().enumerate() {
            let value = binding.load(Ordering::Acquire);
            if value >= 0 {
                mappings.insert(cc as u8, value as u8);
            }
        }
    }

    pub fn sync_atomic_from_mutex(&self) {
        for binding in &self.cc_bindings {
            binding.store(UNMAPPED_CC, Ordering::Release);
        }

        let mappings = self.mappings.lock().clone();
        for (cc, control_index) in mappings {
            self.cc_bindings[cc as usize].store(control_index as i32, Ordering::Release);
        }
    }

    pub fn save_current_mapping(&self) {
        self.sync_mutex_from_atomic_if_needed();
        let current = self.mappings.lock().clone();
        *self.saved_mappings.lock() = current;
    }
}

#[derive(Params)]
pub struct NebulaClusterParams {
    #[cfg(not(target_os = "windows"))]
    #[persist = "editor-state"]
    pub editor_state: Arc<EguiState>,

    #[id = "input_level"]
    pub input_level: FloatParam,
    #[id = "input_pan"]
    pub input_pan: FloatParam,
    #[id = "output_level"]
    pub output_level: FloatParam,
    #[id = "output_pan"]
    pub output_pan: FloatParam,
    #[id = "global_mix"]
    pub global_mix: FloatParam,
    #[id = "oversampling"]
    pub oversampling: FloatParam,
    #[id = "global_phase"]
    pub global_phase: FloatParam,
    #[id = "fx_bypass"]
    pub fx_bypass: FloatParam,
    #[id = "distortion_enabled"]
    pub distortion_enabled: FloatParam,
    #[id = "dist_saturation"]
    pub dist_saturation: FloatParam,
    #[id = "harmonic_2"]
    pub harmonic_2: FloatParam,
    #[id = "harmonic_3"]
    pub harmonic_3: FloatParam,
    #[id = "harmonic_4"]
    pub harmonic_4: FloatParam,
    #[id = "harmonic_5"]
    pub harmonic_5: FloatParam,
    #[id = "harmonic_6"]
    pub harmonic_6: FloatParam,
    #[id = "harmonic_7"]
    pub harmonic_7: FloatParam,
    #[id = "dist_mix"]
    pub dist_mix: FloatParam,
    #[id = "dist_phase"]
    pub dist_phase: FloatParam,
    #[id = "dist_hpf"]
    pub dist_hpf: FloatParam,
    #[id = "dist_hp_slope"]
    pub dist_hp_slope: FloatParam,
    #[id = "dist_lpf"]
    pub dist_lpf: FloatParam,
    #[id = "dist_lp_slope"]
    pub dist_lp_slope: FloatParam,
    #[id = "filter_enabled"]
    pub filter_enabled: FloatParam,
    #[id = "filter_hpf"]
    pub filter_hpf: FloatParam,
    #[id = "filter_hp_slope"]
    pub filter_hp_slope: FloatParam,
    #[id = "filter_hp_res"]
    pub filter_hp_res: FloatParam,
    #[id = "filter_lpf"]
    pub filter_lpf: FloatParam,
    #[id = "filter_lp_slope"]
    pub filter_lp_slope: FloatParam,
    #[id = "filter_lp_res"]
    pub filter_lp_res: FloatParam,
    #[id = "compressor_enabled"]
    pub compressor_enabled: FloatParam,
    #[id = "comp_mode"]
    pub comp_mode: FloatParam,
    #[id = "comp_ratio"]
    pub comp_ratio: FloatParam,
    #[id = "comp_knee"]
    pub comp_knee: FloatParam,
    #[id = "comp_makeup"]
    pub comp_makeup: FloatParam,
    #[id = "comp_boost"]
    pub comp_boost: FloatParam,
    #[id = "comp_attack_threshold"]
    pub comp_attack_threshold: FloatParam,
    #[id = "comp_attack_ms"]
    pub comp_attack_ms: FloatParam,
    #[id = "comp_release_threshold"]
    pub comp_release_threshold: FloatParam,
    #[id = "comp_release_ms"]
    pub comp_release_ms: FloatParam,
    #[id = "comp_hold"]
    pub comp_hold: FloatParam,
}

impl Default for NebulaClusterParams {
    fn default() -> Self {
        Self {
            #[cfg(not(target_os = "windows"))]
            editor_state: EguiState::from_size(1180, 760),
            input_level: make_param(ControlId::InputLevel),
            input_pan: make_param(ControlId::InputPan),
            output_level: make_param(ControlId::OutputLevel),
            output_pan: make_param(ControlId::OutputPan),
            global_mix: make_param(ControlId::GlobalMix),
            oversampling: make_param(ControlId::Oversampling),
            global_phase: make_param(ControlId::GlobalPhase),
            fx_bypass: make_param(ControlId::FxBypass),
            distortion_enabled: make_param(ControlId::DistortionEnabled),
            dist_saturation: make_param(ControlId::DistSaturation),
            harmonic_2: make_param(ControlId::Harmonic2),
            harmonic_3: make_param(ControlId::Harmonic3),
            harmonic_4: make_param(ControlId::Harmonic4),
            harmonic_5: make_param(ControlId::Harmonic5),
            harmonic_6: make_param(ControlId::Harmonic6),
            harmonic_7: make_param(ControlId::Harmonic7),
            dist_mix: make_param(ControlId::DistMix),
            dist_phase: make_param(ControlId::DistPhase),
            dist_hpf: make_param(ControlId::DistHpf),
            dist_hp_slope: make_param(ControlId::DistHpSlope),
            dist_lpf: make_param(ControlId::DistLpf),
            dist_lp_slope: make_param(ControlId::DistLpSlope),
            filter_enabled: make_param(ControlId::FilterEnabled),
            filter_hpf: make_param(ControlId::FilterHpf),
            filter_hp_slope: make_param(ControlId::FilterHpSlope),
            filter_hp_res: make_param(ControlId::FilterHpRes),
            filter_lpf: make_param(ControlId::FilterLpf),
            filter_lp_slope: make_param(ControlId::FilterLpSlope),
            filter_lp_res: make_param(ControlId::FilterLpRes),
            compressor_enabled: make_param(ControlId::CompressorEnabled),
            comp_mode: make_param(ControlId::CompMode),
            comp_ratio: make_param(ControlId::CompRatio),
            comp_knee: make_param(ControlId::CompKnee),
            comp_makeup: make_param(ControlId::CompMakeup),
            comp_boost: make_param(ControlId::CompBoost),
            comp_attack_threshold: make_param(ControlId::CompAttackThreshold),
            comp_attack_ms: make_param(ControlId::CompAttackMs),
            comp_release_threshold: make_param(ControlId::CompReleaseThreshold),
            comp_release_ms: make_param(ControlId::CompReleaseMs),
            comp_hold: make_param(ControlId::CompHold),
        }
    }
}

fn make_param(id: ControlId) -> FloatParam {
    let spec = id.spec();
    let mut param = FloatParam::new(
        spec.name,
        spec.default as f32,
        FloatRange::Linear {
            min: spec.min as f32,
            max: spec.max as f32,
        },
    )
    .with_step_size(spec.step as f32)
    .with_value_to_string(Arc::new(move |value| format_value(id, value as f64)))
    .with_string_to_value(Arc::new(move |input| {
        parse_value(id, input).map(|value| value as f32)
    }));

    if !matches!(spec.kind, ValueKind::Boolean | ValueKind::Choice(_)) {
        param = param.with_smoother(SmoothingStyle::Linear(10.0));
    }

    param
}

#[derive(Default)]
struct Meters {
    peak_bits: AtomicU32,
    reduction_bits: AtomicU32,
}

pub struct NebulaCluster {
    params: Arc<NebulaClusterParams>,
    sample_rate: f64,
    dsp: NebulaClusterDsp,
    os_dsp: NebulaClusterDsp,
    analyzer: SpectrumAnalyzer,
    meters: Arc<Meters>,
    midi_learn: Arc<MidiLearnShared>,
    oversampling_factor: usize,
    prev_l: f64,
    prev_r: f64,
}

impl Default for NebulaCluster {
    fn default() -> Self {
        let sample_rate = 44_100.0;
        Self {
            params: Arc::new(NebulaClusterParams::default()),
            sample_rate,
            dsp: NebulaClusterDsp::new(sample_rate),
            os_dsp: NebulaClusterDsp::new(sample_rate),
            analyzer: SpectrumAnalyzer::new(),
            meters: Arc::new(Meters {
                peak_bits: AtomicU32::new(f32_to_u32(-120.0)),
                reduction_bits: AtomicU32::new(f32_to_u32(0.0)),
            }),
            midi_learn: Arc::new(MidiLearnShared::new()),
            oversampling_factor: 1,
            prev_l: 0.0,
            prev_r: 0.0,
        }
    }
}

impl Drop for NebulaCluster {
    fn drop(&mut self) {
        self.midi_learn.save_current_mapping();
    }
}

impl Plugin for NebulaCluster {
    const NAME: &'static str = "Nebula Cluster";
    const VENDOR: &'static str = "Nebula Audio";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        aux_input_ports: &[],
        aux_output_ports: &[],
        names: PortNames {
            layout: Some("Stereo"),
            main_input: Some("Input"),
            main_output: Some("Output"),
            aux_inputs: &[],
            aux_outputs: &[],
        },
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    #[cfg(not(target_os = "windows"))]
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        let params = self.params.clone();
        let meters = self.meters.clone();
        let analyzer = self.analyzer.shared();
        let midi_learn = self.midi_learn.clone();

        create_egui_editor(
            self.params.editor_state.clone(),
            NebulaClusterGui::new(analyzer, midi_learn.clone()),
            |_ctx: &Context, _state: &mut NebulaClusterGui| {},
            move |ctx: &Context, setter: &ParamSetter, gui_state: &mut NebulaClusterGui| {
                midi_learn.sync_mutex_from_atomic_if_needed();
                apply_midi_cc_changes(&midi_learn, &params, setter);

                let gui_params = GuiParams {
                    snapshot: snapshot_from_params(&params),
                    meters: MeterSnapshot {
                        peak_db: u32_to_f32(meters.peak_bits.load(Ordering::Relaxed)),
                        gain_reduction_db: u32_to_f32(
                            meters.reduction_bits.load(Ordering::Relaxed),
                        ),
                    },
                };

                let changes = draw(ctx, &params.editor_state, gui_state, &gui_params);
                for change in changes.changes {
                    set_control(&params, setter, change.id, change.value);
                }
            },
        )
    }

    #[cfg(target_os = "windows")]
    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        windows_editor::create_editor(
            self.params.clone(),
            self.analyzer.shared(),
            self.meters.clone(),
            self.midi_learn.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate as f64;
        self.dsp = NebulaClusterDsp::new(self.sample_rate);
        self.os_dsp = NebulaClusterDsp::new(self.sample_rate);
        self.oversampling_factor = 1;
        self.prev_l = 0.0;
        self.prev_r = 0.0;
        self.analyzer.reset();
        self.analyzer.set_sample_rate(self.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.dsp.reset();
        self.os_dsp.reset();
        self.analyzer.reset();
        self.prev_l = 0.0;
        self.prev_r = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        while let Some(event) = context.next_event() {
            if let NoteEvent::MidiCC { cc, value, .. } = event {
                if !self.midi_learn.midi_enabled.load(Ordering::Relaxed) {
                    continue;
                }

                let cc_index = (cc as usize).min(127);
                self.midi_learn.cc_values[cc_index].store(f32_to_u32(value), Ordering::Relaxed);
                self.midi_learn.cc_dirty[cc_index].store(true, Ordering::Release);

                let target = self.midi_learn.learning_target.load(Ordering::Acquire);
                if target >= 0 {
                    self.midi_learn
                        .learning_target
                        .store(UNMAPPED_CC, Ordering::Release);
                    if let Some(control) = ControlId::from_index(target as usize) {
                        self.midi_learn.learn_cc(cc, control);
                    }
                }
            }
        }

        let snapshot = snapshot_from_params(&self.params);
        let settings = DspSettings::from_snapshot(snapshot);
        let requested_os = oversampling_factor(snapshot.choice(ControlId::Oversampling));
        if requested_os != self.oversampling_factor {
            self.oversampling_factor = requested_os;
            self.os_dsp = NebulaClusterDsp::new(self.sample_rate * requested_os as f64);
        }

        self.dsp.prepare(&settings);
        self.os_dsp.prepare(&settings);

        let samples = buffer.samples();
        let channels = buffer.as_slice();
        if channels.len() < 2 {
            return ProcessStatus::Normal;
        }

        let (left_slice, right_slice) = {
            let (left, right) = channels.split_at_mut(1);
            (&mut left[0], &mut right[0])
        };

        let mut peak_db = -120.0_f64;
        let mut reduction_db = 0.0_f64;

        for sample_index in 0..samples {
            let input_l = left_slice[sample_index] as f64;
            let input_r = right_slice[sample_index] as f64;
            let report = if self.oversampling_factor > 1 {
                process_oversampled(
                    &mut self.os_dsp,
                    self.prev_l,
                    self.prev_r,
                    input_l,
                    input_r,
                    self.oversampling_factor,
                    &settings,
                )
            } else {
                self.dsp.process_frame(input_l, input_r, &settings)
            };

            left_slice[sample_index] = report.out_l as f32;
            right_slice[sample_index] = report.out_r as f32;
            self.analyzer.push((report.out_l + report.out_r) * 0.5);
            peak_db = peak_db.max(report.peak_db);
            reduction_db = reduction_db.min(report.gain_reduction_db);
            self.prev_l = input_l;
            self.prev_r = input_r;
        }

        self.meters
            .peak_bits
            .store(f32_to_u32(peak_db as f32), Ordering::Relaxed);
        self.meters
            .reduction_bits
            .store(f32_to_u32(reduction_db as f32), Ordering::Relaxed);

        ProcessStatus::Normal
    }
}

impl ClapPlugin for NebulaCluster {
    const CLAP_ID: &'static str = "audio.nebula.cluster";
    const CLAP_DESCRIPTION: Option<&'static str> = Some(
        "Free open-source f64 multi-effect with distortion, filter, compression, and analyzer",
    );
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Distortion,
        ClapFeature::Filter,
        ClapFeature::Compressor,
        ClapFeature::Analyzer,
        ClapFeature::MultiEffects,
        ClapFeature::Mixing,
    ];
}

impl Vst3Plugin for NebulaCluster {
    const VST3_CLASS_ID: [u8; 16] = *b"NebulaClusterV30";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Filter,
        Vst3SubCategory::Dynamics,
        Vst3SubCategory::Analyzer,
    ];
}

nih_export_clap!(NebulaCluster);
nih_export_vst3!(NebulaCluster);
#[cfg(target_os = "macos")]
clap_wrapper::export_auv2!();

fn process_oversampled(
    dsp: &mut NebulaClusterDsp,
    prev_l: f64,
    prev_r: f64,
    input_l: f64,
    input_r: f64,
    factor: usize,
    settings: &DspSettings,
) -> dsp::ProcessReport {
    let mut out_l = 0.0;
    let mut out_r = 0.0;
    let mut peak_db: f64 = -120.0;
    let mut reduction_db: f64 = 0.0;
    for substep in 0..factor {
        let t = (substep + 1) as f64 / factor as f64;
        let interp_l = prev_l + (input_l - prev_l) * t;
        let interp_r = prev_r + (input_r - prev_r) * t;
        let report = dsp.process_frame(interp_l, interp_r, settings);
        out_l += report.out_l;
        out_r += report.out_r;
        peak_db = peak_db.max(report.peak_db);
        reduction_db = reduction_db.min(report.gain_reduction_db);
    }

    let inv = 1.0 / factor as f64;
    dsp::ProcessReport {
        out_l: out_l * inv,
        out_r: out_r * inv,
        peak_db,
        gain_reduction_db: reduction_db,
    }
}

fn apply_midi_cc_changes(
    midi_learn: &MidiLearnShared,
    params: &Arc<NebulaClusterParams>,
    setter: &ParamSetter,
) {
    if !midi_learn.midi_enabled.load(Ordering::Relaxed) {
        return;
    }

    for cc in 0..128 {
        if !midi_learn.cc_dirty[cc].swap(false, Ordering::AcqRel) {
            continue;
        }
        let Some(id) = midi_learn.binding_for_cc(cc) else {
            continue;
        };
        let raw = u32_to_f32(midi_learn.cc_values[cc].load(Ordering::Relaxed)) as f64;
        let value = id.spec().value_from_unit(raw);
        set_control(params, setter, id, value);
    }
}

fn snapshot_from_params(params: &NebulaClusterParams) -> Snapshot {
    let mut values = [0.0; CONTROL_COUNT];
    values[ControlId::InputLevel.index()] = params.input_level.value() as f64;
    values[ControlId::InputPan.index()] = params.input_pan.value() as f64;
    values[ControlId::OutputLevel.index()] = params.output_level.value() as f64;
    values[ControlId::OutputPan.index()] = params.output_pan.value() as f64;
    values[ControlId::GlobalMix.index()] = params.global_mix.value() as f64;
    values[ControlId::Oversampling.index()] = params.oversampling.value() as f64;
    values[ControlId::GlobalPhase.index()] = params.global_phase.value() as f64;
    values[ControlId::FxBypass.index()] = params.fx_bypass.value() as f64;
    values[ControlId::DistortionEnabled.index()] = params.distortion_enabled.value() as f64;
    values[ControlId::DistSaturation.index()] = params.dist_saturation.value() as f64;
    values[ControlId::Harmonic2.index()] = params.harmonic_2.value() as f64;
    values[ControlId::Harmonic3.index()] = params.harmonic_3.value() as f64;
    values[ControlId::Harmonic4.index()] = params.harmonic_4.value() as f64;
    values[ControlId::Harmonic5.index()] = params.harmonic_5.value() as f64;
    values[ControlId::Harmonic6.index()] = params.harmonic_6.value() as f64;
    values[ControlId::Harmonic7.index()] = params.harmonic_7.value() as f64;
    values[ControlId::DistMix.index()] = params.dist_mix.value() as f64;
    values[ControlId::DistPhase.index()] = params.dist_phase.value() as f64;
    values[ControlId::DistHpf.index()] = params.dist_hpf.value() as f64;
    values[ControlId::DistHpSlope.index()] = params.dist_hp_slope.value() as f64;
    values[ControlId::DistLpf.index()] = params.dist_lpf.value() as f64;
    values[ControlId::DistLpSlope.index()] = params.dist_lp_slope.value() as f64;
    values[ControlId::FilterEnabled.index()] = params.filter_enabled.value() as f64;
    values[ControlId::FilterHpf.index()] = params.filter_hpf.value() as f64;
    values[ControlId::FilterHpSlope.index()] = params.filter_hp_slope.value() as f64;
    values[ControlId::FilterHpRes.index()] = params.filter_hp_res.value() as f64;
    values[ControlId::FilterLpf.index()] = params.filter_lpf.value() as f64;
    values[ControlId::FilterLpSlope.index()] = params.filter_lp_slope.value() as f64;
    values[ControlId::FilterLpRes.index()] = params.filter_lp_res.value() as f64;
    values[ControlId::CompressorEnabled.index()] = params.compressor_enabled.value() as f64;
    values[ControlId::CompMode.index()] = params.comp_mode.value() as f64;
    values[ControlId::CompRatio.index()] = params.comp_ratio.value() as f64;
    values[ControlId::CompKnee.index()] = params.comp_knee.value() as f64;
    values[ControlId::CompMakeup.index()] = params.comp_makeup.value() as f64;
    values[ControlId::CompBoost.index()] = params.comp_boost.value() as f64;
    values[ControlId::CompAttackThreshold.index()] = params.comp_attack_threshold.value() as f64;
    values[ControlId::CompAttackMs.index()] = params.comp_attack_ms.value() as f64;
    values[ControlId::CompReleaseThreshold.index()] = params.comp_release_threshold.value() as f64;
    values[ControlId::CompReleaseMs.index()] = params.comp_release_ms.value() as f64;
    values[ControlId::CompHold.index()] = params.comp_hold.value() as f64;
    Snapshot { values }
}

fn set_control(params: &Arc<NebulaClusterParams>, setter: &ParamSetter, id: ControlId, value: f64) {
    macro_rules! set {
        ($param:expr) => {{
            setter.begin_set_parameter(&$param);
            setter.set_parameter(&$param, value as f32);
            setter.end_set_parameter(&$param);
        }};
    }

    match id {
        ControlId::InputLevel => set!(params.input_level),
        ControlId::InputPan => set!(params.input_pan),
        ControlId::OutputLevel => set!(params.output_level),
        ControlId::OutputPan => set!(params.output_pan),
        ControlId::GlobalMix => set!(params.global_mix),
        ControlId::Oversampling => set!(params.oversampling),
        ControlId::GlobalPhase => set!(params.global_phase),
        ControlId::FxBypass => set!(params.fx_bypass),
        ControlId::DistortionEnabled => set!(params.distortion_enabled),
        ControlId::DistSaturation => set!(params.dist_saturation),
        ControlId::Harmonic2 => set!(params.harmonic_2),
        ControlId::Harmonic3 => set!(params.harmonic_3),
        ControlId::Harmonic4 => set!(params.harmonic_4),
        ControlId::Harmonic5 => set!(params.harmonic_5),
        ControlId::Harmonic6 => set!(params.harmonic_6),
        ControlId::Harmonic7 => set!(params.harmonic_7),
        ControlId::DistMix => set!(params.dist_mix),
        ControlId::DistPhase => set!(params.dist_phase),
        ControlId::DistHpf => set!(params.dist_hpf),
        ControlId::DistHpSlope => set!(params.dist_hp_slope),
        ControlId::DistLpf => set!(params.dist_lpf),
        ControlId::DistLpSlope => set!(params.dist_lp_slope),
        ControlId::FilterEnabled => set!(params.filter_enabled),
        ControlId::FilterHpf => set!(params.filter_hpf),
        ControlId::FilterHpSlope => set!(params.filter_hp_slope),
        ControlId::FilterHpRes => set!(params.filter_hp_res),
        ControlId::FilterLpf => set!(params.filter_lpf),
        ControlId::FilterLpSlope => set!(params.filter_lp_slope),
        ControlId::FilterLpRes => set!(params.filter_lp_res),
        ControlId::CompressorEnabled => set!(params.compressor_enabled),
        ControlId::CompMode => set!(params.comp_mode),
        ControlId::CompRatio => set!(params.comp_ratio),
        ControlId::CompKnee => set!(params.comp_knee),
        ControlId::CompMakeup => set!(params.comp_makeup),
        ControlId::CompBoost => set!(params.comp_boost),
        ControlId::CompAttackThreshold => set!(params.comp_attack_threshold),
        ControlId::CompAttackMs => set!(params.comp_attack_ms),
        ControlId::CompReleaseThreshold => set!(params.comp_release_threshold),
        ControlId::CompReleaseMs => set!(params.comp_release_ms),
        ControlId::CompHold => set!(params.comp_hold),
    }
}

fn oversampling_factor(selection: usize) -> usize {
    match selection {
        1 => 2,
        2 => 4,
        3 => 6,
        4 => 8,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_learn_syncs_audio_thread_binding_to_gui_mapping() {
        let midi_learn = MidiLearnShared::new();

        midi_learn.learn_cc(74, ControlId::DistSaturation);

        assert_eq!(
            midi_learn.binding_for_cc(74),
            Some(ControlId::DistSaturation)
        );
        midi_learn.sync_mutex_from_atomic_if_needed();
        assert_eq!(
            midi_learn.mappings.lock().get(&74).copied(),
            Some(ControlId::DistSaturation.index() as u8)
        );
    }

    #[test]
    fn midi_learn_syncs_gui_cleanup_to_atomic_bindings() {
        let midi_learn = MidiLearnShared::new();
        midi_learn.learn_cc(11, ControlId::GlobalMix);
        midi_learn.sync_mutex_from_atomic_if_needed();

        midi_learn.mappings.lock().clear();
        midi_learn.sync_atomic_from_mutex();

        assert_eq!(midi_learn.binding_for_cc(11), None);
    }

    #[test]
    fn midi_learn_save_current_mapping_updates_rollback_state() {
        let midi_learn = MidiLearnShared::new();

        midi_learn.learn_cc(21, ControlId::OutputLevel);
        midi_learn.save_current_mapping();

        assert_eq!(
            midi_learn.saved_mappings.lock().get(&21).copied(),
            Some(ControlId::OutputLevel.index() as u8)
        );
    }

    #[test]
    fn plugin_close_saves_current_midi_mapping_for_rollback() {
        let plugin = NebulaCluster::default();
        let midi_learn = Arc::clone(&plugin.midi_learn);

        midi_learn.learn_cc(22, ControlId::InputPan);
        drop(plugin);

        assert_eq!(
            midi_learn.saved_mappings.lock().get(&22).copied(),
            Some(ControlId::InputPan.index() as u8)
        );
    }
}
