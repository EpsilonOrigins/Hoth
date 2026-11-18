//! Offline parsing and validation for HOTH scenario YAML.

pub use hoth_domain::clock::ClockMode;
use hoth_domain::state::SimulationUtc;
use serde::{de::Error as _, Deserialize, Deserializer};
use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    net::SocketAddr,
    num::NonZeroU64,
};
use yaml_serde::Value;

const SCHEMA_VERSION: i64 = 1;

/// A validated v1 scenario.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioConfig {
    #[serde(deserialize_with = "deserialize_version")]
    schema_version: i64,
    #[serde(deserialize_with = "deserialize_name")]
    pub name: String,
    pub time: ScenarioTime,
    pub catalog: CatalogConfig,
    pub selection: SelectionConfig,
    #[serde(default)]
    pub radars: Vec<RadarConfig>,
    #[serde(default)]
    pub dis: DisConfig,
    #[serde(deserialize_with = "deserialize_publishers")]
    pub publishers: Vec<PublisherConfig>,
}

impl ScenarioConfig {
    /// Parses one YAML document and validates its fields without reading a catalog
    /// or opening a network connection.
    pub fn from_yaml(input: &str) -> Result<Self, ScenarioError> {
        #[derive(Deserialize)]
        struct VersionHeader {
            #[serde(rename = "schema_version", deserialize_with = "deserialize_version")]
            _schema_version: i64,
        }

        let document: Value =
            yaml_serde::from_str(input).map_err(|error| ScenarioError(Box::new(error)))?;
        // Check the version before interpreting fields using the v1 schema.
        deserialize_document::<VersionHeader>(&document)?;
        let config: Self = deserialize_document(&document)?;
        config
            .validate()
            .map_err(|error| ScenarioError(error.into()))?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        use chrono::{Datelike, TimeDelta};
        let duration = i64::try_from(self.time.duration_seconds.get())
            .ok()
            .and_then(TimeDelta::try_seconds)
            .ok_or("time.duration_seconds overflows UTC")?;
        let start = self.time.start_utc.instant();
        let end = start
            .checked_add_signed(duration)
            .ok_or("time.duration_seconds overflows UTC")?;
        if start.year() < 1 || end.year() > 9999 || start.timestamp_subsec_nanos() >= 1_000_000_000
        {
            return Err("time must use four-digit UTC years and ordinary POSIX seconds".into());
        }
        if self.selection.mode == SelectionMode::RadarUnion && self.radars.is_empty() {
            return Err("selection.mode radar_union requires at least one entry in radars".into());
        }
        let mut ids = HashSet::new();
        for (i, radar) in self.radars.iter().enumerate() {
            for (field, value, min, max) in [
                ("latitude_deg", radar.latitude_deg, -90.0, 90.0),
                ("longitude_deg", radar.longitude_deg, -180.0, 180.0),
                ("altitude_m", radar.altitude_m, -1000.0, 100_000.0),
                ("azimuth_min_deg", radar.azimuth_min_deg, 0.0, 360.0),
                ("azimuth_max_deg", radar.azimuth_max_deg, 0.0, 360.0),
                ("elevation_min_deg", radar.elevation_min_deg, -90.0, 90.0),
                ("elevation_max_deg", radar.elevation_max_deg, -90.0, 90.0),
                ("range_min_m", radar.range_min_m, 0.0, f64::MAX),
                ("range_max_m", radar.range_max_m, 0.0, f64::MAX),
            ] {
                if !(min..=max).contains(&value) {
                    return Err(format!(
                        "radars[{i}].{field} must be between {min} and {max}"
                    ));
                }
            }
            if radar.id.trim().is_empty() || !ids.insert(&radar.id) {
                return Err(format!("radars[{i}].id must be nonblank and unique"));
            }
            if radar.elevation_min_deg > radar.elevation_max_deg
                || radar.range_min_m >= radar.range_max_m
            {
                return Err(format!(
                    "radars[{i}] has reversed elevation or range bounds"
                ));
            }
        }
        if self.dis.site == 0 || self.dis.application == 0 {
            return Err("dis.site and dis.application must be positive".into());
        }
        for (i, publisher) in self.publishers.iter().enumerate() {
            if publisher.destination.ip().is_unspecified()
                || publisher.multicast != publisher.destination.ip().is_multicast()
                || (publisher.multicast && publisher.kind != PublisherKind::Dis)
            {
                return Err(format!("publishers[{i}].destination requires a valid unicast address or DIS multicast opt-in"));
            }
            if !(0.001..=1000.0).contains(&publisher.rate_hz) {
                return Err(format!(
                    "publishers[{i}].rate_hz must be between 0.001 and 1000"
                ));
            }
        }
        Ok(())
    }

    pub fn end_utc(&self) -> SimulationUtc {
        let delta = chrono::TimeDelta::seconds(self.time.duration_seconds.get() as i64);
        SimulationUtc::new(self.time.start_utc.instant() + delta)
    }

    pub const fn schema_version(&self) -> i64 {
        self.schema_version
    }
}

/// Syntax or validation error with the YAML field path and location when available.
#[derive(Debug)]
pub struct ScenarioError(Box<dyn Error + Send + Sync>);

impl fmt::Display for ScenarioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Error for ScenarioError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioTime {
    #[serde(deserialize_with = "deserialize_start_utc")]
    pub start_utc: SimulationUtc,
    #[serde(deserialize_with = "deserialize_duration")]
    pub duration_seconds: NonZeroU64,
    pub clock: ClockMode,
    #[serde(deserialize_with = "deserialize_positive_rate")]
    pub rate: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogConfig {
    #[serde(deserialize_with = "deserialize_snapshot")]
    pub snapshot: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionConfig {
    pub mode: SelectionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    All,
    RadarUnion,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherConfig {
    #[serde(rename = "type")]
    pub kind: PublisherKind,
    #[serde(deserialize_with = "deserialize_destination")]
    pub destination: SocketAddr,
    #[serde(deserialize_with = "deserialize_positive_rate")]
    pub rate_hz: f64,
    #[serde(default)]
    pub multicast: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublisherKind {
    Dis,
    Zenoh,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RadarConfig {
    pub id: String,
    pub latitude_deg: f64,
    pub longitude_deg: f64,
    pub altitude_m: f64,
    pub azimuth_min_deg: f64,
    pub azimuth_max_deg: f64,
    pub elevation_min_deg: f64,
    pub elevation_max_deg: f64,
    #[serde(default)]
    pub range_min_m: f64,
    pub range_max_m: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DisConfig {
    pub exercise_id: u8,
    pub site: u16,
    pub application: u16,
}

impl Default for DisConfig {
    fn default() -> Self {
        Self {
            exercise_id: 1,
            site: 1,
            application: 1,
        }
    }
}

fn deserialize_document<'de, T: Deserialize<'de>>(
    document: &'de Value,
) -> Result<T, ScenarioError> {
    serde_path_to_error::deserialize(document).map_err(|error| ScenarioError(Box::new(error)))
}

fn deserialize_version<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    let version = i64::deserialize(deserializer)?;
    if version != SCHEMA_VERSION {
        return Err(D::Error::custom(format_args!(
            "unsupported schema version {version}; expected {SCHEMA_VERSION}"
        )));
    }
    Ok(version)
}

fn deserialize_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let name = String::deserialize(deserializer)?;
    if name.trim().is_empty() {
        return Err(D::Error::custom("scenario name must not be blank"));
    }
    Ok(name)
}

fn deserialize_start_utc<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<SimulationUtc, D::Error> {
    SimulationUtc::deserialize(deserializer)
        .map_err(|_| D::Error::custom("expected an RFC 3339 timestamp with Z or a numeric offset"))
}

fn deserialize_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<NonZeroU64, D::Error> {
    NonZeroU64::deserialize(deserializer)
        .map_err(|_| D::Error::custom("expected a positive whole number of seconds"))
}

fn deserialize_snapshot<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let snapshot = String::deserialize(deserializer)?;
    let valid = snapshot.split_once('-').is_some_and(|(year, month)| {
        year.len() == 4
            && year.bytes().all(|digit| digit.is_ascii_digit())
            && matches!(month.as_bytes(), [b'0', b'1'..=b'9'] | [b'1', b'0'..=b'2'])
    });
    if !valid {
        return Err(D::Error::custom(
            "expected a snapshot month in YYYY-MM format",
        ));
    }
    Ok(snapshot)
}

fn deserialize_positive_rate<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let rate = f64::deserialize(deserializer)?;
    if !rate.is_finite() || rate <= 0.0 {
        return Err(D::Error::custom(
            "rate must be finite and greater than zero",
        ));
    }
    Ok(rate)
}

fn deserialize_destination<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<SocketAddr, D::Error> {
    let value = String::deserialize(deserializer)?;
    let destination: SocketAddr = value.parse().map_err(|_| {
        D::Error::custom("expected an IP address and UDP port, e.g. 127.0.0.1:3000 or [::1]:3000")
    })?;
    if destination.port() == 0 {
        return Err(D::Error::custom(
            "destination UDP port must be greater than zero",
        ));
    }
    Ok(destination)
}

fn deserialize_publishers<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PublisherConfig>, D::Error> {
    let publishers = Vec::<PublisherConfig>::deserialize(deserializer)?;
    if publishers.is_empty() {
        return Err(D::Error::custom("expected at least one publisher"));
    }

    let mut seen = HashMap::new();
    for (index, publisher) in publishers.iter().enumerate() {
        if let Some(first_index) = seen.insert((publisher.kind, publisher.destination), index) {
            return Err(D::Error::custom(format_args!(
                "publishers[{index}] duplicates the type and destination of publishers[{first_index}]"
            )));
        }
    }
    Ok(publishers)
}
