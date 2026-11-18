//! Transport-neutral satellite state types shared by HOTH components.

use serde::{Deserialize, Deserializer, Serialize};
use std::{error::Error, fmt, num::NonZeroU32};

/// An absolute UTC instant. Serde uses an RFC 3339 string and normalizes offsets to UTC.
pub type UtcInstant = chrono::DateTime<chrono::Utc>;

/// The absolute simulation time at which the position and velocity apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SimulationUtc(UtcInstant);

impl SimulationUtc {
    pub const fn new(instant: UtcInstant) -> Self {
        Self(instant)
    }

    pub const fn instant(self) -> UtcInstant {
        self.0
    }
}

/// A positive NORAD catalog number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoradId(NonZeroU32);

impl NoradId {
    pub const fn new(id: u32) -> Option<Self> {
        match NonZeroU32::new(id) {
            Some(id) => Some(Self(id)),
            None => None,
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateValidationError {
    NonFiniteComponent,
    BlankProvenance,
}

impl fmt::Display for StateValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFiniteComponent => "Cartesian vector components must be finite",
            Self::BlankProvenance => "provenance source and record ID must be nonblank",
        })
    }
}

impl Error for StateValidationError {}

/// A Cartesian position in meters, ordered [x, y, z].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]")]
pub struct PositionMeters([f64; 3]);

impl PositionMeters {
    pub const fn xyz(self) -> [f64; 3] {
        self.0
    }
}

impl TryFrom<[f64; 3]> for PositionMeters {
    type Error = StateValidationError;

    fn try_from(xyz: [f64; 3]) -> Result<Self, Self::Error> {
        finite_xyz(xyz).map(Self)
    }
}

/// A Cartesian velocity in meters per second, ordered [x, y, z].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]")]
pub struct VelocityMetersPerSecond([f64; 3]);

impl VelocityMetersPerSecond {
    pub const fn xyz(self) -> [f64; 3] {
        self.0
    }
}

impl TryFrom<[f64; 3]> for VelocityMetersPerSecond {
    type Error = StateValidationError;

    fn try_from(xyz: [f64; 3]) -> Result<Self, Self::Error> {
        finite_xyz(xyz).map(Self)
    }
}

/// A Cartesian position in kilometers, ordered [x, y, z].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]")]
pub struct PositionKilometers([f64; 3]);

impl PositionKilometers {
    pub const fn xyz(self) -> [f64; 3] {
        self.0
    }

    /// Rescales the components in the same frame. Returns an error on overflow.
    pub fn to_meters(self) -> Result<PositionMeters, StateValidationError> {
        PositionMeters::try_from(self.0.map(kilometers_to_meters))
    }
}

impl TryFrom<[f64; 3]> for PositionKilometers {
    type Error = StateValidationError;

    fn try_from(xyz: [f64; 3]) -> Result<Self, Self::Error> {
        finite_xyz(xyz).map(Self)
    }
}

/// A Cartesian velocity in kilometers per second, ordered [x, y, z].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]")]
pub struct VelocityKilometersPerSecond([f64; 3]);

impl VelocityKilometersPerSecond {
    pub const fn xyz(self) -> [f64; 3] {
        self.0
    }

    /// Rescales the components in the same frame. Returns an error on overflow.
    pub fn to_meters_per_second(self) -> Result<VelocityMetersPerSecond, StateValidationError> {
        VelocityMetersPerSecond::try_from(self.0.map(kilometers_to_meters))
    }
}

impl TryFrom<[f64; 3]> for VelocityKilometersPerSecond {
    type Error = StateValidationError;

    fn try_from(xyz: [f64; 3]) -> Result<Self, Self::Error> {
        finite_xyz(xyz).map(Self)
    }
}

fn kilometers_to_meters(component: f64) -> f64 {
    component * 1_000.0
}

fn finite_xyz(xyz: [f64; 3]) -> Result<[f64; 3], StateValidationError> {
    if xyz.iter().all(|component| component.is_finite()) {
        Ok(xyz)
    } else {
        Err(StateValidationError::NonFiniteComponent)
    }
}

/// Earth-centered, Earth-fixed kinematics. Velocity is relative to the rotating ECEF axes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EcefState {
    frame: EcefFrame,
    pub position_m: PositionMeters,
    pub velocity_m_per_s: VelocityMetersPerSecond,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
enum EcefFrame {
    #[serde(rename = "ECEF")]
    Ecef,
}

impl EcefState {
    pub const fn new(
        position_m: PositionMeters,
        velocity_m_per_s: VelocityMetersPerSecond,
    ) -> Self {
        Self {
            frame: EcefFrame::Ecef,
            position_m,
            velocity_m_per_s,
        }
    }
}

/// True equator, mean equinox kinematics in kilometers and kilometers per second.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TemeState {
    frame: TemeFrame,
    pub at_utc: SimulationUtc,
    pub position_km: PositionKilometers,
    pub velocity_km_s: VelocityKilometersPerSecond,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
enum TemeFrame {
    #[serde(rename = "TEME")]
    Teme,
}

impl TemeState {
    pub const fn new(
        at_utc: SimulationUtc,
        position_km: PositionKilometers,
        velocity_km_s: VelocityKilometersPerSecond,
    ) -> Self {
        Self {
            frame: TemeFrame::Teme,
            at_utc,
            position_km,
            velocity_km_s,
        }
    }
}

/// The epoch used for initialization. Synthetic initialization preserves the original source epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EpochInitialization {
    SourceEpoch,
    Synthetic { epoch_utc: UtcInstant },
}

/// Traceability to the orbital element record used to initialize the simulated state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateProvenance {
    #[serde(deserialize_with = "deserialize_nonblank")]
    source: String,
    #[serde(deserialize_with = "deserialize_nonblank")]
    source_record_id: String,
    pub source_epoch_utc: UtcInstant,
    pub initialization: EpochInitialization,
}

impl StateProvenance {
    pub fn new(
        source: String,
        source_record_id: String,
        source_epoch_utc: UtcInstant,
        initialization: EpochInitialization,
    ) -> Result<Self, StateValidationError> {
        Ok(Self {
            source: nonblank(source)?,
            source_record_id: nonblank(source_record_id)?,
            source_epoch_utc,
            initialization,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn source_record_id(&self) -> &str {
        &self.source_record_id
    }
}

fn nonblank(value: String) -> Result<String, StateValidationError> {
    if value.trim().is_empty() {
        Err(StateValidationError::BlankProvenance)
    } else {
        Ok(value)
    }
}

fn deserialize_nonblank<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    nonblank(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
}

/// Canonical state at a simulation UTC instant, ready for a publisher to consume.
///
/// ```compile_fail
/// use hoth_domain::state::{EcefState, TemeState};
/// fn publish(_: EcefState) {}
/// fn publish_teme(teme: TemeState) { publish(teme); }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SatelliteState {
    pub norad_id: NoradId,
    pub at_utc: SimulationUtc,
    pub ecef: EcefState,
    pub provenance: StateProvenance,
}
