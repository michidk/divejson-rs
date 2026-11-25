//! One device's record of a dive (§6.4a): its device (§6.4b), deco model (§6.4c), sampled
//! profile (§6.4), series (§6.5) and events (§6.6).

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::{DateTime, Extensions, StoredFile};

/// One device's record of one dive (§6.4a).
///
/// It must carry at least one of `device`, `profile`, `source_files` or a readout
/// (`surface_pressure`, `cns_start`, `cns_end`, `otu_start`, `otu_end`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Recording {
    /// What recorded it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    /// The mode this device ran in; absent is not open circuit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RecordingMode>,
    /// The decompression model the device ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deco_model: Option<DecoModel>,
    /// The water density the device was set to: a calibration, not a kind of water.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub salinity: Option<Salinity>,
    /// The device's own start; absent means the dive's `started_at`. Profile times are
    /// elapsed from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime>,
    /// Readout: ambient surface pressure in bar; 0.4–1.2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_pressure: Option<f64>,
    /// Readout: CNS percent at dive start; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cns_start: Option<f64>,
    /// Readout: CNS percent at dive end; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cns_end: Option<f64>,
    /// Readout: OTU at dive start; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otu_start: Option<f64>,
    /// Readout: OTU at dive end; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otu_end: Option<f64>,
    /// The original dive-computer files this recording was read from.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_files: Vec<StoredFile>,
    /// The sampled profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// The mode a device ran in (§6.4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RecordingMode {
    /// Open circuit.
    OpenCircuit,
    /// Closed-circuit rebreather.
    ClosedCircuit,
    /// Semi-closed rebreather.
    SemiClosed,
    /// Bottom timer.
    Gauge,
    /// Breath-hold.
    Freedive,
}

/// The water density a device was set to (§6.4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Salinity {
    /// Fresh water.
    Fresh,
    /// The EN 13319 convention.
    #[serde(rename = "en13319")]
    En13319,
    /// Sea water.
    Salt,
}

/// What recorded a dive (§6.4b). Every string is 1–64 code points (`firmware` 1–32); a
/// device with no members is not written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Device {
    /// The maker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    /// The product.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The device's own serial, opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    /// The firmware version it ran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub firmware: Option<String>,
    /// What the device calls itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The device's own dive counter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dive_number: Option<u64>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// The decompression model one device ran (§6.4c). `gf_low` and `gf_high` come as a pair,
/// low ≤ high; a model with no members is not written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct DecoModel {
    /// The model family.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<DecoAlgorithm>,
    /// The device's name for its model, 1–64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Whole percent, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gf_low: Option<u8>,
    /// Whole percent, 0–100.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gf_high: Option<u8>,
    /// On the device's own scale; may be negative.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conservatism: Option<i64>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

/// A deco model's family (§6.4c).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecoAlgorithm {
    /// Bühlmann.
    Buhlmann,
    /// RGBM.
    Rgbm,
}

/// The sampled record of one recording (§6.4).
///
/// Every channel is integer-scaled and sampled on its own millisecond axis; a gap in
/// `times` is a reading the device did not give, never a zero.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Profile {
    /// Milliseconds spanned by the samples; ≥ every channel's last time. Events may fall
    /// after it.
    pub duration: u64,
    /// Centimeters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depth: Option<Series>,
    /// Decompression ceiling in centimeters, present only while one existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ceiling: Option<Series>,
    /// Hundredths of a degree Celsius.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<Series>,
    /// One per cylinder the device monitored, in tenths of a bar.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pressures: Vec<PressureSeries>,
    /// No-decompression time in seconds; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ndl: Option<Series>,
    /// Time to surface in seconds; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts: Option<Series>,
    /// Computed ppO₂ in hundredths of a bar; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ppo2: Option<Series>,
    /// CNS in tenths of a percent; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cns: Option<Series>,
    /// Leading-tissue gradient factor in whole percent; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gradient_factor: Option<Series>,
    /// Surfacing gradient factor in whole percent; ≥ 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_gradient_factor: Option<Series>,
    /// Point events, in time order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<Event>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Profile {
    /// A profile spanning `duration` milliseconds with no channels.
    #[must_use]
    pub fn new(duration: u64) -> Self {
        Self {
            duration,
            ..Self::default()
        }
    }

    /// Every single-series channel that is present, with its member name.
    pub fn channels(&self) -> impl Iterator<Item = (&'static str, &Series)> {
        [
            ("depth", &self.depth),
            ("ceiling", &self.ceiling),
            ("temperature", &self.temperature),
            ("ndl", &self.ndl),
            ("tts", &self.tts),
            ("ppo2", &self.ppo2),
            ("cns", &self.cns),
            ("gradient_factor", &self.gradient_factor),
            ("surface_gradient_factor", &self.surface_gradient_factor),
        ]
        .into_iter()
        .filter_map(|(name, series)| series.as_ref().map(|series| (name, series)))
    }
}

/// Two parallel integer arrays (§6.5): `times` in milliseconds from the recording's start,
/// strictly increasing, and `values` in the channel's scale. Both are always written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Series {
    /// Elapsed milliseconds.
    pub times: Vec<u64>,
    /// Readings in the channel's scale.
    pub values: Vec<i64>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Series {
    /// A series from its two arrays.
    #[must_use]
    pub fn new(times: Vec<u64>, values: Vec<i64>) -> Self {
        Self {
            times,
            values,
            extensions: Extensions::new(),
        }
    }
}

/// A [`Series`] of one cylinder's pressure in tenths of a bar, labelled with the device's
/// gas number (§6.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PressureSeries {
    /// Elapsed milliseconds.
    pub times: Vec<u64>,
    /// Tenths of a bar.
    pub values: Vec<i64>,
    /// Joins to the dive's `cylinders[].gas_number`.
    pub gas_number: u32,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl PressureSeries {
    /// A pressure series from its arrays and gas number.
    #[must_use]
    pub fn new(times: Vec<u64>, values: Vec<i64>, gas_number: u32) -> Self {
        Self {
            times,
            values,
            gas_number,
            extensions: Extensions::new(),
        }
    }
}

/// A point event on a recording's timeline (§6.6). An event without a `type` must carry a
/// non-empty `label`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Event {
    /// Elapsed milliseconds; may exceed the profile's `duration`.
    pub time: u64,
    /// What happened; absent means unclassified.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub event_type: Option<EventType>,
    /// On a gas switch, the gas switched to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gas_number: Option<u32>,
    /// The device's own wording, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Producer extensions.
    #[serde(default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl Event {
    /// A classified event.
    #[must_use]
    pub fn new(time: u64, event_type: EventType) -> Self {
        Self {
            time,
            event_type: Some(event_type),
            gas_number: None,
            label: None,
            extensions: Extensions::new(),
        }
    }

    /// An unclassified event carrying the device's wording.
    #[must_use]
    pub fn labelled(time: u64, label: impl Into<String>) -> Self {
        Self {
            time,
            event_type: None,
            gas_number: None,
            label: Some(label.into()),
            extensions: Extensions::new(),
        }
    }
}

/// An event's `type` (§6.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventType {
    /// The breathing gas changed.
    GasSwitch,
    /// A deep stop.
    DeepStop,
    /// A safety stop.
    SafetyStop,
    /// The diver marked this moment.
    Bookmark,
    /// The ascent was too fast.
    AscentRate,
    /// The device made a safety stop compulsory.
    SafetyStopMandatory,
    /// A required safety or deep stop was left early or not made.
    SafetyStopViolation,
    /// A required deep stop was left early or not made.
    DeepStopViolation,
    /// The diver went shallower than the ceiling.
    CeilingViolation,
    /// The no-decompression time ran out.
    NdlReached,
    /// The ppO₂ passed the device's limit.
    Ppo2High,
    /// The cylinder pressure passed the warning threshold.
    PressureLow,
    /// The depth passed a set limit.
    DepthAlarm,
}
