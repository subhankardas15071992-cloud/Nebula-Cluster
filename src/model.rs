#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum ControlId {
    InputLevel = 0,
    InputPan,
    OutputLevel,
    OutputPan,
    GlobalMix,
    Oversampling,
    GlobalPhase,
    FxBypass,
    DistortionEnabled,
    DistSaturation,
    Harmonic2,
    Harmonic3,
    Harmonic4,
    Harmonic5,
    Harmonic6,
    Harmonic7,
    DistMix,
    DistPhase,
    DistHpf,
    DistHpSlope,
    DistLpf,
    DistLpSlope,
    FilterEnabled,
    FilterHpf,
    FilterHpSlope,
    FilterHpRes,
    FilterLpf,
    FilterLpSlope,
    FilterLpRes,
    CompressorEnabled,
    CompMode,
    CompRatio,
    CompKnee,
    CompMakeup,
    CompBoost,
    CompAttackThreshold,
    CompAttackMs,
    CompReleaseThreshold,
    CompReleaseMs,
    CompHold,
}

pub const CONTROL_COUNT: usize = 40;

pub const ALL_CONTROLS: [ControlId; CONTROL_COUNT] = [
    ControlId::InputLevel,
    ControlId::InputPan,
    ControlId::OutputLevel,
    ControlId::OutputPan,
    ControlId::GlobalMix,
    ControlId::Oversampling,
    ControlId::GlobalPhase,
    ControlId::FxBypass,
    ControlId::DistortionEnabled,
    ControlId::DistSaturation,
    ControlId::Harmonic2,
    ControlId::Harmonic3,
    ControlId::Harmonic4,
    ControlId::Harmonic5,
    ControlId::Harmonic6,
    ControlId::Harmonic7,
    ControlId::DistMix,
    ControlId::DistPhase,
    ControlId::DistHpf,
    ControlId::DistHpSlope,
    ControlId::DistLpf,
    ControlId::DistLpSlope,
    ControlId::FilterEnabled,
    ControlId::FilterHpf,
    ControlId::FilterHpSlope,
    ControlId::FilterHpRes,
    ControlId::FilterLpf,
    ControlId::FilterLpSlope,
    ControlId::FilterLpRes,
    ControlId::CompressorEnabled,
    ControlId::CompMode,
    ControlId::CompRatio,
    ControlId::CompKnee,
    ControlId::CompMakeup,
    ControlId::CompBoost,
    ControlId::CompAttackThreshold,
    ControlId::CompAttackMs,
    ControlId::CompReleaseThreshold,
    ControlId::CompReleaseMs,
    ControlId::CompHold,
];

pub const OVERSAMPLING_LABELS: [&str; 5] = ["Off", "2x", "4x", "6x", "8x"];
pub const COMPRESSOR_MODE_LABELS: [&str; 3] = ["Down", "Up", "Boost"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueKind {
    Decibel,
    Pan,
    Percent,
    Hertz,
    DbPerOct,
    Milliseconds,
    Ratio,
    Boolean,
    Choice(&'static [&'static str]),
}

#[derive(Clone, Copy, Debug)]
pub struct ControlSpec {
    pub id: ControlId,
    pub name: &'static str,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub step: f64,
    pub kind: ValueKind,
}

impl ControlSpec {
    pub fn clamp(self, value: f64) -> f64 {
        let stepped = if self.step > 0.0 {
            (value / self.step).round() * self.step
        } else {
            value
        };
        stepped.clamp(self.min, self.max)
    }

    pub fn value_from_unit(self, unit: f64) -> f64 {
        let unit = unit.clamp(0.0, 1.0);
        match self.kind {
            ValueKind::Hertz => {
                let min = self.min.max(1.0);
                let max = self.max.max(min + 1.0);
                min * (max / min).powf(unit)
            }
            _ => self.min + (self.max - self.min) * unit,
        }
        .clamp(self.min, self.max)
    }

    pub fn unit_from_value(self, value: f64) -> f64 {
        let value = value.clamp(self.min, self.max);
        match self.kind {
            ValueKind::Hertz => {
                let min = self.min.max(1.0);
                let max = self.max.max(min + 1.0);
                (value / min).ln() / (max / min).ln()
            }
            _ => (value - self.min) / (self.max - self.min),
        }
        .clamp(0.0, 1.0)
    }
}

pub const CONTROL_SPECS: [ControlSpec; CONTROL_COUNT] = [
    spec(
        ControlId::InputLevel,
        "Input Level",
        -100.0,
        100.0,
        0.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::InputPan,
        "Input Pan",
        -1.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Pan,
    ),
    spec(
        ControlId::OutputLevel,
        "Output Level",
        -100.0,
        100.0,
        0.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::OutputPan,
        "Output Pan",
        -1.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Pan,
    ),
    spec(
        ControlId::GlobalMix,
        "Mix",
        0.0,
        1.0,
        1.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Oversampling,
        "Oversampling",
        0.0,
        4.0,
        0.0,
        1.0,
        ValueKind::Choice(&OVERSAMPLING_LABELS),
    ),
    spec(
        ControlId::GlobalPhase,
        "Phase",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::FxBypass,
        "FX Bypass",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::DistortionEnabled,
        "Distortion",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::DistSaturation,
        "Saturation",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic2,
        "2nd Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic3,
        "3rd Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic4,
        "4th Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic5,
        "5th Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic6,
        "6th Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::Harmonic7,
        "7th Order",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::DistMix,
        "Dist Mix",
        0.0,
        1.0,
        1.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::DistPhase,
        "Dist Phase",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::DistHpf,
        "Dist HPF",
        20.0,
        20_000.0,
        20.0,
        1.0,
        ValueKind::Hertz,
    ),
    spec(
        ControlId::DistHpSlope,
        "Dist HPFS",
        0.0,
        100.0,
        0.0,
        0.1,
        ValueKind::DbPerOct,
    ),
    spec(
        ControlId::DistLpf,
        "Dist LPF",
        20.0,
        20_000.0,
        20_000.0,
        1.0,
        ValueKind::Hertz,
    ),
    spec(
        ControlId::DistLpSlope,
        "Dist LPFS",
        0.0,
        100.0,
        0.0,
        0.1,
        ValueKind::DbPerOct,
    ),
    spec(
        ControlId::FilterEnabled,
        "Filter",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::FilterHpf,
        "HPF",
        20.0,
        20_000.0,
        20.0,
        1.0,
        ValueKind::Hertz,
    ),
    spec(
        ControlId::FilterHpSlope,
        "HPFS",
        0.0,
        100.0,
        0.0,
        0.1,
        ValueKind::DbPerOct,
    ),
    spec(
        ControlId::FilterHpRes,
        "HPFR",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::FilterLpf,
        "LPF",
        20.0,
        20_000.0,
        20_000.0,
        1.0,
        ValueKind::Hertz,
    ),
    spec(
        ControlId::FilterLpSlope,
        "LPFS",
        0.0,
        100.0,
        0.0,
        0.1,
        ValueKind::DbPerOct,
    ),
    spec(
        ControlId::FilterLpRes,
        "LPFR",
        0.0,
        1.0,
        0.0,
        0.01,
        ValueKind::Percent,
    ),
    spec(
        ControlId::CompressorEnabled,
        "Compressor",
        0.0,
        1.0,
        0.0,
        1.0,
        ValueKind::Boolean,
    ),
    spec(
        ControlId::CompMode,
        "Mode",
        0.0,
        2.0,
        0.0,
        1.0,
        ValueKind::Choice(&COMPRESSOR_MODE_LABELS),
    ),
    spec(
        ControlId::CompRatio,
        "Ratio",
        1.0,
        20.0,
        2.0,
        0.01,
        ValueKind::Ratio,
    ),
    spec(
        ControlId::CompKnee,
        "Knee",
        0.0,
        36.0,
        6.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::CompMakeup,
        "Makeup",
        -24.0,
        24.0,
        0.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::CompBoost,
        "Boost",
        0.0,
        36.0,
        6.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::CompAttackThreshold,
        "Attack Thresh",
        -80.0,
        0.0,
        -18.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::CompAttackMs,
        "Attack",
        0.01,
        500.0,
        10.0,
        0.01,
        ValueKind::Milliseconds,
    ),
    spec(
        ControlId::CompReleaseThreshold,
        "Release Thresh",
        -60.0,
        0.0,
        -12.0,
        0.1,
        ValueKind::Decibel,
    ),
    spec(
        ControlId::CompReleaseMs,
        "Release",
        1.0,
        2_000.0,
        120.0,
        0.1,
        ValueKind::Milliseconds,
    ),
    spec(
        ControlId::CompHold,
        "Hold",
        0.0,
        500.0,
        0.0,
        0.1,
        ValueKind::Milliseconds,
    ),
];

const fn spec(
    id: ControlId,
    name: &'static str,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
    kind: ValueKind,
) -> ControlSpec {
    ControlSpec {
        id,
        name,
        min,
        max,
        default,
        step,
        kind,
    }
}

impl ControlId {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub fn from_index(index: usize) -> Option<Self> {
        ALL_CONTROLS.get(index).copied()
    }

    pub fn spec(self) -> ControlSpec {
        CONTROL_SPECS[self.index()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub values: [f64; CONTROL_COUNT],
}

impl Default for Snapshot {
    fn default() -> Self {
        let mut values = [0.0; CONTROL_COUNT];
        let mut index = 0;
        while index < CONTROL_COUNT {
            values[index] = CONTROL_SPECS[index].default;
            index += 1;
        }
        Self { values }
    }
}

impl Snapshot {
    pub fn get(self, id: ControlId) -> f64 {
        self.values[id.index()]
    }

    pub fn set(&mut self, id: ControlId, value: f64) {
        self.values[id.index()] = id.spec().clamp(value);
    }

    pub fn bool(self, id: ControlId) -> bool {
        self.get(id) >= 0.5
    }

    pub fn choice(self, id: ControlId) -> usize {
        self.get(id).round().max(0.0) as usize
    }
}

pub fn format_value(id: ControlId, value: f64) -> String {
    let spec = id.spec();
    let value = spec.clamp(value);
    match spec.kind {
        ValueKind::Decibel => format!("{value:.1} dB"),
        ValueKind::Pan => {
            if value.abs() < 0.005 {
                String::from("C")
            } else if value < 0.0 {
                format!("L {:.0}", value.abs() * 100.0)
            } else {
                format!("R {:.0}", value * 100.0)
            }
        }
        ValueKind::Percent => format!("{:.0}%", value * 100.0),
        ValueKind::Hertz => {
            if value >= 1000.0 {
                format!("{:.2} kHz", value / 1000.0)
            } else {
                format!("{value:.0} Hz")
            }
        }
        ValueKind::DbPerOct => format!("{value:.1} dB/oct"),
        ValueKind::Milliseconds => format!("{value:.2} ms"),
        ValueKind::Ratio => format!("{value:.2}:1"),
        ValueKind::Boolean => {
            if value >= 0.5 {
                String::from("On")
            } else {
                String::from("Off")
            }
        }
        ValueKind::Choice(labels) => {
            let index = value
                .round()
                .clamp(0.0, labels.len().saturating_sub(1) as f64) as usize;
            labels[index].to_string()
        }
    }
}

pub fn parse_value(id: ControlId, input: &str) -> Option<f64> {
    let spec = id.spec();
    let lowered = input.trim().to_ascii_lowercase();
    match spec.kind {
        ValueKind::Boolean => match lowered.as_str() {
            "on" | "true" | "1" => Some(1.0),
            "off" | "false" | "0" => Some(0.0),
            _ => None,
        },
        ValueKind::Choice(labels) => labels
            .iter()
            .position(|label| label.eq_ignore_ascii_case(input.trim()))
            .map(|index| index as f64)
            .or_else(|| numeric_prefix(&lowered)),
        ValueKind::Percent => {
            numeric_prefix(&lowered).map(|value| if value > 1.0 { value * 0.01 } else { value })
        }
        ValueKind::Hertz => numeric_prefix(&lowered).map(|value| {
            if lowered.contains("khz") {
                value * 1000.0
            } else {
                value
            }
        }),
        ValueKind::Pan => numeric_prefix(&lowered).map(|value| {
            if lowered.starts_with('l') || lowered.contains("left") {
                -(value.abs() / 100.0).clamp(0.0, 1.0)
            } else if lowered.starts_with('r') || lowered.contains("right") {
                (value.abs() / 100.0).clamp(0.0, 1.0)
            } else if value.abs() > 1.0 {
                (value / 100.0).clamp(-1.0, 1.0)
            } else {
                value
            }
        }),
        _ => numeric_prefix(&lowered),
    }
    .map(|value| spec.clamp(value))
}

fn numeric_prefix(input: &str) -> Option<f64> {
    let mut number = String::new();
    for ch in input.chars() {
        if ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.') {
            number.push(ch);
        } else if !number.is_empty() {
            break;
        }
    }
    number.parse().ok()
}
