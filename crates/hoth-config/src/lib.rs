//! Offline parsing and validation for HOTH scenario YAML.

use hoth_domain::state::SimulationUtc;
use serde::{de::Error as _, Deserialize, Deserializer};
use std::{collections::HashMap, error::Error, fmt, net::SocketAddr, num::NonZeroU64};
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
        deserialize_document(&document)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    Standalone,
    External,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublisherKind {
    Dis,
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
