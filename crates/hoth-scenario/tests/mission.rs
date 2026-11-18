use chrono::TimeDelta;
use hoth_catalog::{offline, schema};
use hoth_dis::Registry;
use hoth_domain::state::{EpochInitialization, SimulationUtc};
use hoth_orbits::{EarthRotation, Propagator};
use hoth_scenario::{radar::Radar, Manifest};
use serde_json::{json, Value};

#[test]
fn narrow_later_passes_form_fixed_union_and_manifest_is_replayable() {
    let start = SimulationUtc::new("2026-11-17T00:00:00Z".parse().unwrap());
    let mut rows = Vec::new();
    for (id, phase) in [(70000, 0), (70001, 110), (70002, 230)] {
        rows.push(json!({
            "EPOCH": "2020-08-15T00:00:00Z", "NORAD_CAT_ID": id,
            "INCLINATION": 0, "RA_OF_ASC_NODE": 0, "ECCENTRICITY": 0.0002,
            "ARG_OF_PERICENTER": 0, "MEAN_ANOMALY": phase, "MEAN_MOTION": 15.4, "BSTAR": 0
        }));
    }
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    schema::migrate(&mut db).unwrap();
    offline::import_elements(
        &mut db,
        "omm-json",
        &serde_json::to_vec(&rows).unwrap(),
        "test",
        "2026-11-17T00:00:00Z",
    )
    .unwrap();
    offline::materialize(&mut db, "2020-08", 5).unwrap();
    let population = offline::snapshot(&db, "2020-08").unwrap().1;
    let mut yaml = r#"
schema_version: 1
name: disjoint-pass-test
catalog: {snapshot: "2020-08"}
time: {start_utc: "2026-11-17T00:00:00Z", duration_seconds: 600, clock: standalone, rate: 1}
selection: {mode: radar_union}
publishers: [{type: dis, destination: "127.0.0.1:3000", rate_hz: 25}]
radars:
"#
    .to_owned();
    for (index, millis) in [180_123, 417_369].into_iter().enumerate() {
        let orbit = Propagator::new(
            &population[index],
            EpochInitialization::Synthetic {
                epoch_utc: start.instant(),
            },
        )
        .unwrap();
        let at = SimulationUtc::new(start.instant() + TimeDelta::milliseconds(millis));
        let [x, y, _] = orbit
            .state(EarthRotation::at(at))
            .unwrap()
            .ecef
            .position_m
            .xyz();
        yaml.push_str(&format!(
            "  - {{id: radar-{index}, latitude_deg: 0, longitude_deg: {}, altitude_m: 0, azimuth_min_deg: 0, azimuth_max_deg: 360, elevation_min_deg: 89.9, elevation_max_deg: 90, range_max_m: 1000000}}\n",
            y.atan2(x).to_degrees()
        ));
    }
    let registry =
        Registry::from_yaml(include_str!("../../../config/dis/entity_types.yaml")).unwrap();
    let manifest = Manifest::prepare(&db, yaml.clone(), &registry).unwrap();
    let encoded = serde_json::to_vec(&manifest).unwrap();
    assert_eq!(
        encoded,
        serde_json::to_vec(&Manifest::prepare(&db, yaml, &registry).unwrap()).unwrap()
    );
    drop(db);
    let manifest = Manifest::decode(&encoded).unwrap();
    let config = manifest.config().unwrap();
    let radars: Vec<_> = config.radars.iter().map(Radar::new).collect();
    let ids: Vec<_> = manifest
        .members()
        .iter()
        .map(|item| item.norad_id())
        .collect();
    assert_eq!(ids, [70_000, 70_001]);
    for (index, item) in manifest.members().iter().enumerate() {
        let orbit = item.propagator().unwrap();
        let frame = EarthRotation::at(start);
        let before = orbit.state(frame).unwrap();
        assert!(!radars
            .iter()
            .any(|radar| radar.contains(before.ecef.position_m.xyz())));
        let at = SimulationUtc::new(
            start.instant() + TimeDelta::milliseconds([180_123, 417_369][index]),
        );
        let position = orbit
            .state(EarthRotation::at(at))
            .unwrap()
            .ecef
            .position_m
            .xyz();
        assert!(radars[index].contains(position));
        assert!(!radars[1 - index].contains(position));
        assert_eq!(before, orbit.state(frame).unwrap());
    }
    let mut tampered: Value = serde_json::from_slice(&encoded).unwrap();
    tampered["mission"]["snapshot_id"] = json!("another-snapshot");
    assert!(Manifest::decode(&serde_json::to_vec(&tampered).unwrap()).is_err());
}
