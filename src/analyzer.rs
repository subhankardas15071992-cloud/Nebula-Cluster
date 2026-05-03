use parking_lot::Mutex;
use rustfft::{num_complex::Complex, Fft, FftPlanner};
use std::f64::consts::PI;
use std::sync::Arc;

pub const FFT_SIZE: usize = 2048;
pub const FFT_HOP: usize = 512;
pub const NUM_BINS: usize = FFT_SIZE / 2 + 1;
pub const WAVEFORM_SIZE: usize = 512;

const MAG_SCALE: f64 = 4.0 / FFT_SIZE as f64;

#[derive(Clone)]
pub struct AnalyzerData {
    pub magnitudes_db: Vec<f32>,
    pub waveform: Vec<f32>,
    pub sample_rate: f64,
}

impl Default for AnalyzerData {
    fn default() -> Self {
        Self {
            magnitudes_db: vec![-120.0; NUM_BINS],
            waveform: vec![0.0; WAVEFORM_SIZE],
            sample_rate: 44_100.0,
        }
    }
}

pub struct SpectrumAnalyzer {
    ring_buffer: Vec<f64>,
    write_pos: usize,
    hop_counter: usize,
    waveform_counter: usize,
    waveform_pos: usize,
    waveform_scratch: Vec<f32>,
    window: Vec<f64>,
    fft: Arc<dyn Fft<f64>>,
    fft_scratch: Vec<Complex<f64>>,
    magnitude_scratch: Vec<f32>,
    shared: Arc<Mutex<AnalyzerData>>,
}

impl SpectrumAnalyzer {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f64>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);

        Self {
            ring_buffer: vec![0.0; FFT_SIZE],
            write_pos: 0,
            hop_counter: 0,
            waveform_counter: 0,
            waveform_pos: 0,
            waveform_scratch: vec![0.0; WAVEFORM_SIZE],
            window: hann_window(),
            fft,
            fft_scratch: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            magnitude_scratch: vec![-120.0; NUM_BINS],
            shared: Arc::new(Mutex::new(AnalyzerData::default())),
        }
    }

    pub fn shared(&self) -> Arc<Mutex<AnalyzerData>> {
        Arc::clone(&self.shared)
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if let Some(mut shared) = self.shared.try_lock() {
            shared.sample_rate = sample_rate;
        }
    }

    pub fn reset(&mut self) {
        self.ring_buffer.fill(0.0);
        self.fft_scratch.fill(Complex::new(0.0, 0.0));
        self.magnitude_scratch.fill(-120.0);
        self.waveform_scratch.fill(0.0);
        self.write_pos = 0;
        self.hop_counter = 0;
        self.waveform_counter = 0;
        self.waveform_pos = 0;
    }

    #[inline]
    pub fn push(&mut self, sample: f64) {
        let sample = sanitize(sample);
        self.ring_buffer[self.write_pos] = sample;
        self.write_pos = (self.write_pos + 1) % FFT_SIZE;

        self.waveform_counter += 1;
        if self.waveform_counter >= 4 {
            self.waveform_counter = 0;
            self.waveform_scratch[self.waveform_pos] = sample as f32;
            self.waveform_pos = (self.waveform_pos + 1) % WAVEFORM_SIZE;
        }

        self.hop_counter += 1;
        if self.hop_counter >= FFT_HOP {
            self.hop_counter = 0;
            self.compute_fft();
        }
    }

    fn compute_fft(&mut self) {
        for (index, scratch) in self.fft_scratch.iter_mut().enumerate() {
            let ring_index = (self.write_pos + index) % FFT_SIZE;
            *scratch = Complex::new(self.ring_buffer[ring_index] * self.window[index], 0.0);
        }

        self.fft.process(&mut self.fft_scratch);

        self.magnitude_scratch[0] = -120.0;
        for bin in 1..(NUM_BINS - 1) {
            let magnitude = self.fft_scratch[bin].norm() * MAG_SCALE;
            self.magnitude_scratch[bin] = magnitude_to_db(magnitude);
        }
        self.magnitude_scratch[NUM_BINS - 1] =
            magnitude_to_db(self.fft_scratch[NUM_BINS - 1].norm() * MAG_SCALE * 0.5);

        if let Some(mut shared) = self.shared.try_lock() {
            shared.magnitudes_db.clone_from(&self.magnitude_scratch);
            for index in 0..WAVEFORM_SIZE {
                let source = (self.waveform_pos + index) % WAVEFORM_SIZE;
                shared.waveform[index] = self.waveform_scratch[source];
            }
        }
    }
}

impl Default for SpectrumAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn hann_window() -> Vec<f64> {
    (0..FFT_SIZE)
        .map(|index| 0.5 * (1.0 - (2.0 * PI * index as f64 / (FFT_SIZE - 1) as f64).cos()))
        .collect()
}

#[inline]
fn magnitude_to_db(magnitude: f64) -> f32 {
    if magnitude <= 1.0e-12 {
        -120.0
    } else {
        (20.0 * magnitude.log10()).clamp(-120.0, 24.0) as f32
    }
}

#[inline]
fn sanitize(value: f64) -> f64 {
    if value.is_finite() && value.abs() >= 1.0e-30 {
        value.clamp(-16.0, 16.0)
    } else {
        0.0
    }
}
