use serde::Deserialize;

/// The authority for advancing simulation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockMode {
    Standalone,
    External,
}
