use chrono::TimeDelta;
use hoth_catalog::parse::parse_tle;
use hoth_domain::{
    orbit::RecordProvenance,
    state::{EpochInitialization, SimulationUtc, TemeState},
};
use hoth_orbits::{teme_to_ecef, PropagationError, Propagator};

fn close(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} vs {expected}, tolerance {tolerance}"
        );
    }
}

#[test]
fn vallado_wgs72_space_track_verification_case_00005() {
    // Vallado AIAA 2006-6753 SGP4-VER.TLE and tcppver.out, object 00005.
    let tle = b"1 00005U 58002B   00179.78495062  .00000023  00000-0  28098-4 0  4753\n2 00005  34.2682 348.7242 1859667 331.7664  19.3264 10.82419157413667\n";
    let mut element = parse_tle(
        tle,
        RecordProvenance {
            source: "AIAA 2006-6753".into(),
            record_id: "00005".into(),
        },
    )
    .unwrap()
    .element;
    let orbit = Propagator::new(&element, EpochInitialization::SourceEpoch).unwrap();
    for (minutes, position, velocity) in [
        (
            0,
            [7022.46529266, -1400.08296755, 0.03995155],
            [1.893841015, 6.405893759, 4.534807250],
        ),
        (
            360,
            [-7154.03120202, -3783.17682504, -3536.19412294],
            [4.741887409, -4.151817765, -2.093935425],
        ),
        (
            1440,
            [-938.55923943, -6268.18748831, -4294.02924751],
            [7.536105209, -0.427127707, 0.989878080],
        ),
    ] {
        let state = orbit
            .teme(SimulationUtc::new(
                element.epoch_utc + TimeDelta::minutes(minutes),
            ))
            .unwrap();
        close(state.position_km.xyz(), position, 1e-6);
        close(state.velocity_km_s.xyz(), velocity, 1e-8);
    }
    element.mean_elements.mean_motion_rev_day = f64::NAN;
    assert!(matches!(
        Propagator::new(&element, EpochInitialization::SourceEpoch),
        Err(PropagationError::Initialization(_))
    ));
    element.mean_elements.mean_motion_rev_day = 30.0;
    let decayed = Propagator::new(&element, EpochInitialization::SourceEpoch).unwrap();
    assert!(matches!(
        decayed.teme(SimulationUtc::new(element.epoch_utc)),
        Err(PropagationError::Decayed)
    ));
}

#[test]
fn erfa_gmst82_reference_position_and_rotating_velocity() {
    // Independent ERFA gmst82 and NumPy matrix/cross product. UT1=UTC, xp=yp=0.
    for line in include_str!("frames.csv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let (at, vectors) = line.split_once(',').unwrap();
        let at = SimulationUtc::new(at.parse().unwrap());
        let data: Vec<f64> = vectors
            .split(',')
            .map(|cell| cell.parse().unwrap())
            .collect();
        let state = TemeState::new(
            at,
            <[f64; 3]>::try_from(&data[..3])
                .unwrap()
                .try_into()
                .unwrap(),
            <[f64; 3]>::try_from(&data[3..6])
                .unwrap()
                .try_into()
                .unwrap(),
        );
        let ecef = teme_to_ecef(state).unwrap();
        close(ecef.position_m.xyz(), data[6..9].try_into().unwrap(), 0.002);
        close(
            ecef.velocity_m_per_s.xyz(),
            data[9..12].try_into().unwrap(),
            0.000_01,
        );
    }
}
