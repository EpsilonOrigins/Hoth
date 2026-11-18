//! Source orbital elements for initializing satellite propagation.

use crate::state::{NoradId, UtcInstant};

#[derive(Debug, Clone, PartialEq)]
pub struct OrbitalElement {
    pub norad_id: NoradId,
    /// Actual UTC epoch of the source record.
    pub epoch_utc: UtcInstant,
    pub mean_elements: MeanElements,
    pub raw_source: RawOrbitalSource,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeanElements {
    pub inclination_deg: f64,
    pub right_ascension_deg: f64,
    pub eccentricity: f64,
    pub argument_of_perigee_deg: f64,
    pub mean_anomaly_deg: f64,
    pub mean_motion_rev_day: f64,
    pub bstar: Option<f64>,
    /// GP coefficient as published by the source, in rev/day².
    pub mean_motion_dot: Option<f64>,
    /// GP coefficient as published by the source, in rev/day³.
    pub mean_motion_ddot: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementFormat {
    Tle,
    Omm,
}

impl ElementFormat {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tle => "tle",
            Self::Omm => "omm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOrbitalSource {
    pub format: ElementFormat,
    pub epoch_text: String,
    pub record: Vec<u8>,
    /// Lowercase SHA-256 digest of the original record bytes.
    pub checksum: String,
    pub provenance: RecordProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordProvenance {
    pub source: String,
    pub record_id: String,
}
