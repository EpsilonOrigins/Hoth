//! Offline scenario preparation, radar union selection and digested manifests.

pub mod radar;

use chrono::TimeDelta;
use hoth_catalog::offline::{parse_record, snapshot, CatalogResult};
use hoth_config::{ScenarioConfig, SelectionMode};
use hoth_dis::{Entity, EntityId, Registry};
use hoth_domain::{
    orbit::OrbitalElement,
    state::{EpochInitialization, SimulationUtc},
};
use hoth_orbits::{EarthRotation, PropagationError, Propagator};
use radar::Radar;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const VERSION: u8 = 1;
const COARSE_SECONDS: i64 = 30;
const MIN_INTERVAL_MS: i64 = 125;
const SPEED_BOUND_M_S: f64 = 25_000.0;
const MAX_SOURCE_AGE: TimeDelta = TimeDelta::days(7);

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    sha256: String,
    mission: Mission,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Mission {
    schema_version: u8,
    scenario_sha256: String,
    scenario_yaml: String,
    snapshot_id: String,
    selection_method: String,
    max_interval_ms: i64,
    min_interval_ms: i64,
    speed_bound_m_s: f64,
    selected: Vec<Member>,
    rejected: BTreeMap<u32, String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    element: OrbitalElement,
    initialization: EpochInitialization,
    entity: Entity,
}

impl Member {
    pub fn entity(&self) -> Entity {
        self.entity
    }
    pub fn norad_id(&self) -> u32 {
        self.element.norad_id.get()
    }
    pub fn propagator(&self) -> Result<Propagator, PropagationError> {
        Propagator::new(&self.element, self.initialization)
    }
}

impl Manifest {
    pub fn prepare(
        db: &Connection,
        scenario_yaml: String,
        registry: &Registry,
    ) -> CatalogResult<Self> {
        let config = ScenarioConfig::from_yaml(&scenario_yaml)?;
        let (snapshot_id, elements) = snapshot(db, &config.catalog.snapshot)?;
        let radars: Vec<_> = config.radars.iter().map(Radar::new).collect();
        let start = config.time.start_utc;
        let end = config.end_utc();
        let mut selected = Vec::new();
        let mut rejected = BTreeMap::new();
        for element in elements {
            let norad_id = element.norad_id.get();
            let initialization = initialization(&element, start, end);
            let evaluates = || {
                let propagator = Propagator::new(&element, initialization)?;
                propagator.teme(start)?;
                if config.selection.mode == SelectionMode::All {
                    return Ok(true);
                }
                intersects(&propagator, &radars, start, end)
            };
            match evaluates() {
                Ok(false) => continue,
                Err(error) => {
                    rejected.insert(norad_id, error.to_string());
                    continue;
                }
                Ok(true) => {}
            }
            let entity = Entity {
                id: EntityId {
                    site: config.dis.site,
                    application: config.dis.application,
                    entity: entity_number(selected.len())?,
                },
                entity_type: registry.lookup(norad_id),
            };
            selected.push(Member {
                element,
                initialization,
                entity,
            });
        }
        if selected.is_empty() {
            return Err(format!(
                "scenario selected no propagatable members ({} rejected)",
                rejected.len()
            )
            .into());
        }
        let mission = Mission {
            schema_version: VERSION,
            scenario_sha256: hex_digest(scenario_yaml.as_bytes()),
            scenario_yaml,
            snapshot_id,
            selection_method: match config.selection.mode {
                SelectionMode::All => "all",
                SelectionMode::RadarUnion => "bounded_radar_union_v1",
            }
            .into(),
            max_interval_ms: COARSE_SECONDS * 1000,
            min_interval_ms: MIN_INTERVAL_MS,
            speed_bound_m_s: SPEED_BOUND_M_S,
            selected,
            rejected,
        };
        Ok(Self {
            sha256: hex_digest(&serde_json::to_vec(&mission)?),
            mission,
        })
    }

    pub fn decode(bytes: &[u8]) -> CatalogResult<Self> {
        let manifest: Self = serde_json::from_slice(bytes)?;
        let mission = &manifest.mission;
        if mission.schema_version != VERSION
            || hex_digest(&serde_json::to_vec(mission)?) != manifest.sha256
            || hex_digest(mission.scenario_yaml.as_bytes()) != mission.scenario_sha256
        {
            return Err("manifest version or digest verification failed".into());
        }
        let config = manifest.config()?;
        if mission.selected.is_empty() {
            return Err("manifest selected population is empty".into());
        }
        let mut previous = 0;
        for (index, member) in mission.selected.iter().enumerate() {
            let element = &member.element;
            let raw = &element.raw_source;
            let verified = parse_record(raw.format.as_str(), &raw.record, raw.provenance.clone())?;
            if verified.element != *element
                || previous >= member.norad_id()
                || member.entity.id.entity != entity_number(index)?
                || member.entity.id.site != config.dis.site
                || member.entity.id.application != config.dis.application
                || member.initialization
                    != initialization(element, config.time.start_utc, config.end_utc())
            {
                return Err(
                    format!("manifest record {} failed verification", member.norad_id()).into(),
                );
            }
            member.entity.entity_type.validate()?;
            previous = member.norad_id();
        }
        Ok(manifest)
    }

    pub fn config(&self) -> CatalogResult<ScenarioConfig> {
        Ok(ScenarioConfig::from_yaml(&self.mission.scenario_yaml)?)
    }

    pub fn members(&self) -> &[Member] {
        &self.mission.selected
    }

    pub fn digest(&self) -> &str {
        &self.sha256
    }

    pub fn rejected(&self) -> &BTreeMap<u32, String> {
        &self.mission.rejected
    }
}

fn entity_number(index: usize) -> Result<u16, &'static str> {
    u16::try_from(index + 1).map_err(|_| "DIS supports at most 65,535 members per site/application")
}

fn initialization(
    element: &OrbitalElement,
    start: SimulationUtc,
    end: SimulationUtc,
) -> EpochInitialization {
    if (start.instant() - element.epoch_utc).abs() > MAX_SOURCE_AGE
        || (end.instant() - element.epoch_utc).abs() > MAX_SOURCE_AGE
    {
        EpochInitialization::Synthetic {
            epoch_utc: start.instant(),
        }
    } else {
        EpochInitialization::SourceEpoch
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(hex, "{byte:02x}").expect("writing to a String");
    }
    hex
}

fn intersects(
    propagator: &Propagator,
    radars: &[Radar<'_>],
    start: SimulationUtc,
    end: SimulationUtc,
) -> Result<bool, PropagationError> {
    let mut left = start;
    while left < end {
        let right = SimulationUtc::new(
            (left.instant() + TimeDelta::seconds(COARSE_SECONDS)).min(end.instant()),
        );
        if interval_may_intersect(propagator, radars, left, right)? {
            return Ok(true);
        }
        left = right;
    }
    Ok(false)
}

fn interval_may_intersect(
    propagator: &Propagator,
    radars: &[Radar<'_>],
    left: SimulationUtc,
    right: SimulationUtc,
) -> Result<bool, PropagationError> {
    let length = right.instant() - left.instant();
    let midpoint = SimulationUtc::new(left.instant() + length / 2);
    let state = propagator.state(EarthRotation::at(midpoint))?;
    let pos = state.ecef.position_m.xyz();
    let speed = state
        .ecef
        .velocity_m_per_s
        .xyz()
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt();
    if speed > SPEED_BOUND_M_S || radars.iter().any(|radar| radar.contains(pos)) {
        return Ok(true);
    }
    let radius = SPEED_BOUND_M_S * length.as_seconds_f64() / 2.0 + 0.001;
    if !radars
        .iter()
        .any(|radar| radar.could_intersect(pos, radius))
    {
        return Ok(false);
    }
    if length <= TimeDelta::milliseconds(MIN_INTERVAL_MS) {
        return Ok(true);
    }
    Ok(interval_may_intersect(propagator, radars, left, midpoint)?
        || interval_may_intersect(propagator, radars, midpoint, right)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifty_thousand_ids_have_no_collisions_or_truncation() {
        let ids: std::collections::BTreeSet<_> =
            (0..50_000).map(|i| entity_number(i).unwrap()).collect();
        assert_eq!(ids.len(), 50_000);
        assert_eq!(ids.first(), Some(&1));
        assert_eq!(ids.last(), Some(&50_000));
        assert!(entity_number(65_535).is_err());
    }
}
