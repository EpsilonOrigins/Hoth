use hoth_domain::state::{
    EcefState, EpochInitialization, NoradId, PositionKilometers, PositionMeters, SatelliteState,
    SimulationUtc, StateProvenance, StateValidationError, TemeState, UtcInstant,
    VelocityKilometersPerSecond, VelocityMetersPerSecond,
};
use serde::{de::value::SeqDeserializer, Deserialize};
use serde_json::{json, Value};

const CANONICAL_STATE_JSON: &str = include_str!("fixtures/satellite-state.json");

fn fixture_state() -> Value {
    serde_json::from_str(CANONICAL_STATE_JSON).unwrap()
}

#[test]
fn canonical_state_round_trips_with_explicit_units_and_frame() {
    let state: SatelliteState = serde_json::from_str(CANONICAL_STATE_JSON).unwrap();

    assert_eq!(state.norad_id.get(), 100_001);
    assert_eq!(
        state.ecef.position_m.xyz(),
        [6_778_137.000_000_001, 1.0, -2.0]
    );
    assert_eq!(
        state.ecef.velocity_m_per_s.xyz(),
        [0.0, 7_500.123_456_789, -6.0]
    );
    assert_eq!(state.provenance.source(), "local_fixture");
    assert_eq!(state.provenance.source_record_id(), "fixture-1");
    assert_eq!(
        state.provenance.initialization,
        EpochInitialization::SourceEpoch
    );
    let serialized = serde_json::to_string_pretty(&state).unwrap();
    assert_eq!(serialized, CANONICAL_STATE_JSON.trim_end());
    assert_eq!(
        serde_json::from_str::<SatelliteState>(&serialized).unwrap(),
        state
    );
}

#[test]
fn teme_round_trips_and_is_rejected_as_canonical_ecef() {
    let at_utc = SimulationUtc::new("2026-11-18T05:00:00Z".parse().unwrap());
    let position = PositionKilometers::try_from([1.0, 2.0, 3.0]).unwrap();
    let velocity = VelocityKilometersPerSecond::try_from([4.0, 5.0, 6.0]).unwrap();
    let teme = TemeState::new(at_utc, position, velocity);

    let serialized = serde_json::to_value(teme).unwrap();
    assert_eq!(
        serialized,
        json!({
            "frame": "TEME",
            "at_utc": "2026-11-18T05:00:00Z",
            "position_km": [1.0, 2.0, 3.0],
            "velocity_km_s": [4.0, 5.0, 6.0]
        })
    );
    assert_eq!(
        serde_json::from_value::<TemeState>(serialized.clone()).unwrap(),
        teme
    );
    assert!(serde_json::from_value::<EcefState>(serialized.clone()).is_err());
    let mut mislabeled = serialized.clone();
    mislabeled["frame"] = json!("ECEF");
    assert!(serde_json::from_value::<TemeState>(mislabeled).is_err());
    let mut untimed = serialized.clone();
    untimed.as_object_mut().unwrap().remove("at_utc");
    assert!(serde_json::from_value::<TemeState>(untimed).is_err());

    let mut fixture = fixture_state();
    fixture["ecef"] = serialized;
    assert!(serde_json::from_value::<SatelliteState>(fixture).is_err());

    let mut fixture = fixture_state();
    fixture["ecef"]["frame"] = json!("TEME");
    assert!(serde_json::from_value::<SatelliteState>(fixture).is_err());

    let mut fixture = fixture_state();
    fixture["ecef"].as_object_mut().unwrap().remove("frame");
    assert!(serde_json::from_value::<SatelliteState>(fixture).is_err());
}

#[test]
fn rescales_position_and_velocity_with_known_vectors() {
    let position = PositionKilometers::try_from([6_778.125, -12.5, 0.0]).unwrap();
    let velocity = VelocityKilometersPerSecond::try_from([0.25, 7.5, -0.125]).unwrap();
    assert_eq!(
        position.to_meters().unwrap().xyz(),
        [6_778_125.0, -12_500.0, 0.0]
    );
    assert_eq!(
        velocity.to_meters_per_second().unwrap().xyz(),
        [250.0, 7_500.0, -125.0]
    );
}

#[test]
fn unit_scaling_rejects_overflow() {
    let xyz = [f64::MAX, 0.0, 0.0];
    let position = PositionKilometers::try_from(xyz).unwrap();
    let velocity = VelocityKilometersPerSecond::try_from(xyz).unwrap();
    assert_eq!(
        position.to_meters(),
        Err(StateValidationError::NonFiniteComponent)
    );
    assert_eq!(
        velocity.to_meters_per_second(),
        Err(StateValidationError::NonFiniteComponent)
    );
}

#[test]
fn synthetic_initialization_preserves_source_epoch_and_normalizes_utc() {
    let mut fixture = fixture_state();
    fixture["at_utc"] = json!("2026-11-18T06:00:00.123456789+01:00");
    fixture["provenance"]["initialization"] = json!({
        "kind": "synthetic",
        "epoch_utc": "2026-11-18T04:00:00Z"
    });

    let state: SatelliteState = serde_json::from_value(fixture).unwrap();
    assert_eq!(
        state.at_utc.instant(),
        "2026-11-18T05:00:00.123456789Z"
            .parse::<UtcInstant>()
            .unwrap()
    );
    let source_epoch: UtcInstant = "2026-10-01T00:00:00Z".parse().unwrap();
    let synthetic_epoch: UtcInstant = "2026-11-18T04:00:00Z".parse().unwrap();
    assert_eq!(state.provenance.source_epoch_utc, source_epoch);
    assert_eq!(
        state.provenance.initialization,
        EpochInitialization::Synthetic {
            epoch_utc: synthetic_epoch
        }
    );

    let output = serde_json::to_value(state).unwrap();
    assert_eq!(output["at_utc"], "2026-11-18T05:00:00.123456789Z");
    assert_eq!(
        output["provenance"]["source_epoch_utc"],
        "2026-10-01T00:00:00Z"
    );
    assert_eq!(
        output["provenance"]["initialization"]["epoch_utc"],
        "2026-11-18T04:00:00Z"
    );
}

#[test]
fn constructors_and_serde_reject_nonfinite_components() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let xyz = [0.0, value, 0.0];
        assert_eq!(
            PositionMeters::try_from(xyz),
            Err(StateValidationError::NonFiniteComponent)
        );
        assert_eq!(
            VelocityMetersPerSecond::try_from(xyz),
            Err(StateValidationError::NonFiniteComponent)
        );
        assert_eq!(
            PositionKilometers::try_from(xyz),
            Err(StateValidationError::NonFiniteComponent)
        );
        assert_eq!(
            VelocityKilometersPerSecond::try_from(xyz),
            Err(StateValidationError::NonFiniteComponent)
        );

        let sequence = || SeqDeserializer::<_, serde::de::value::Error>::new(xyz.into_iter());
        assert!(PositionMeters::deserialize(sequence()).is_err());
        assert!(VelocityMetersPerSecond::deserialize(sequence()).is_err());
        assert!(PositionKilometers::deserialize(sequence()).is_err());
        assert!(VelocityKilometersPerSecond::deserialize(sequence()).is_err());
    }
}

#[test]
fn rejects_invalid_ids_timestamps_vectors_and_provenance() {
    assert!(NoradId::new(0).is_none());
    assert_eq!(NoradId::new(u32::MAX).unwrap().get(), u32::MAX);

    for (pointer, invalid) in [
        ("/norad_id", json!(0)),
        ("/norad_id", json!(-1)),
        ("/norad_id", json!(u64::from(u32::MAX) + 1)),
        ("/at_utc", json!("2026-11-18T05:00:00")),
        ("/at_utc", json!("2026-02-30T05:00:00Z")),
        ("/ecef/position_m", json!([1.0, 2.0])),
        ("/ecef/velocity_m_per_s", json!([0.0, null, 0.0])),
        ("/provenance/source", json!(" \t")),
        ("/provenance/source_record_id", json!("")),
        ("/provenance/initialization", json!({ "kind": "synthetic" })),
    ] {
        let mut fixture = fixture_state();
        *fixture.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            serde_json::from_value::<SatelliteState>(fixture).is_err(),
            "accepted invalid {pointer}"
        );
    }

    let epoch = "2026-10-01T00:00:00Z".parse().unwrap();
    let init = EpochInitialization::SourceEpoch;
    assert_eq!(
        StateProvenance::new("".into(), "record".into(), epoch, init),
        Err(StateValidationError::BlankProvenance)
    );
    assert_eq!(
        StateProvenance::new("fixture".into(), " ".into(), epoch, init),
        Err(StateValidationError::BlankProvenance)
    );
}
