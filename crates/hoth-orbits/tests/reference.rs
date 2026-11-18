use chrono::TimeDelta;
use hoth_catalog::parse::parse_tle;
use hoth_domain::{
    orbit::{OrbitalElement, RecordProvenance},
    state::{EpochInitialization, SimulationUtc, TemeState, UtcInstant},
};
use hoth_orbits::{teme_to_ecef, EarthRotation, PropagationError, Propagator};
use std::collections::BTreeMap;

fn close(actual: [f64; 3], expected: [f64; 3], tolerance: f64) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!(
            (actual - expected).abs() < tolerance,
            "{actual} vs {expected}, tolerance {tolerance}"
        );
    }
}

fn verification_elements() -> BTreeMap<u32, OrbitalElement> {
    let lines: Vec<_> = include_str!("fixtures/vallado/verification.tle")
        .lines()
        .collect();
    let (records, rest) = lines.as_chunks::<2>();
    assert!(rest.is_empty());
    records
        .iter()
        .map(|[line1, line2]| {
            let element = parse_tle(
                format!("{line1}\n{line2}\n").as_bytes(),
                RecordProvenance {
                    source: "CelesTrak AIAA 2006-6753".into(),
                    record_id: line1[2..7].into(),
                },
            )
            .unwrap()
            .element;
            (element.norad_id.get(), element)
        })
        .collect()
}

fn norm_error(actual: [f64; 3], expected: [f64; 3], scale: f64) -> f64 {
    let [dx, dy, dz] = std::array::from_fn(|i| scale * (actual[i] - expected[i]));
    dx.hypot(dy).hypot(dz)
}

#[test]
fn vallado_reference_trajectories_in_teme_and_ecef() {
    const POSITION_LIMIT_M: f64 = 0.01;
    const VELOCITY_LIMIT_M_S: f64 = 0.0001;
    const SYNTHETIC_NORAD: u32 = 8195;
    let synthetic_epoch: UtcInstant = "2026-11-17T00:00:00Z".parse().unwrap();
    let synthetic = EpochInitialization::Synthetic {
        epoch_utc: synthetic_epoch,
    };

    let elements = verification_elements();
    let source_orbits: BTreeMap<_, _> = elements
        .iter()
        .map(|(&norad, element)| {
            (
                norad,
                Propagator::new(element, EpochInitialization::SourceEpoch).unwrap(),
            )
        })
        .collect();
    let synthetic_orbit = Propagator::new(&elements[&SYNTHETIC_NORAD], synthetic).unwrap();
    let mut cases = BTreeMap::new();
    let mut maxima = [0.0_f64; 4];

    for line in include_str!("fixtures/vallado/vectors.csv").lines().skip(1) {
        let mut fields = line.split(',');
        let mode = fields.next().unwrap();
        let norad: u32 = fields.next().unwrap().parse().unwrap();
        let offset: i64 = fields.next().unwrap().parse().unwrap();
        let at = SimulationUtc::new(fields.next().unwrap().parse().unwrap());
        let mut vectors = fields.map(|field| field.parse::<f64>().unwrap());
        let mut next_vector = || std::array::from_fn(|_| vectors.next().unwrap());
        let teme_position = next_vector();
        let teme_velocity = next_vector();
        let ecef_position = next_vector();
        let ecef_velocity = next_vector();
        assert!(
            vectors.next().is_none(),
            "unexpected vector fields for NORAD {norad}"
        );

        let source_epoch = elements[&norad].epoch_utc;
        let (orbit, epoch, initialization) = match mode {
            "source" => (
                &source_orbits[&norad],
                source_epoch,
                EpochInitialization::SourceEpoch,
            ),
            "synthetic" if norad == SYNTHETIC_NORAD => {
                (&synthetic_orbit, synthetic_epoch, synthetic)
            }
            _ => panic!("unexpected reference case {mode}/{norad}"),
        };
        assert_eq!(
            at.instant() - epoch,
            TimeDelta::seconds(offset),
            "NORAD {norad} {mode} epoch at offset {offset} s"
        );

        let teme = orbit.teme(at).unwrap();
        let canonical = orbit.state(EarthRotation::at(at)).unwrap();
        assert_eq!(teme.at_utc, at);
        assert_eq!(canonical.at_utc, at);
        assert_eq!(canonical.norad_id.get(), norad);
        assert_eq!(canonical.provenance.source_epoch_utc, source_epoch);
        assert_eq!(canonical.provenance.initialization, initialization);

        let errors = [
            norm_error(teme.position_km.xyz(), teme_position, 1000.0),
            norm_error(teme.velocity_km_s.xyz(), teme_velocity, 1000.0),
            norm_error(canonical.ecef.position_m.xyz(), ecef_position, 1.0),
            norm_error(canonical.ecef.velocity_m_per_s.xyz(), ecef_velocity, 1.0),
        ];
        for (i, (quantity, limit)) in [
            ("TEME position (m)", POSITION_LIMIT_M),
            ("TEME velocity (m/s)", VELOCITY_LIMIT_M_S),
            ("ECEF position (m)", POSITION_LIMIT_M),
            ("ECEF velocity (m/s)", VELOCITY_LIMIT_M_S),
        ]
        .into_iter()
        .enumerate()
        {
            assert!(
                errors[i] <= limit,
                "NORAD {norad} {mode} at {} ({offset:+} s): {quantity} error {:.9e} exceeds {limit:.9e}",
                at.instant(), errors[i]
            );
            maxima[i] = maxima[i].max(errors[i]);
        }
        *cases.entry((mode, norad)).or_default() += 1;
    }

    assert_eq!(
        cases,
        BTreeMap::from([
            (("source", 5), 13),
            (("source", 4632), 5),
            (("source", 8195), 25),
            (("source", 24208), 13),
            (("source", 28057), 25),
            (("synthetic", 8195), 5),
        ])
    );
    println!(
        "Maximum TEME error {:.9e} m and {:.9e} m/s; maximum ECEF error {:.9e} m and {:.9e} m/s",
        maxima[0], maxima[1], maxima[2], maxima[3]
    );
}

#[test]
fn rejects_invalid_and_subsurface_elements() {
    let mut element = verification_elements().remove(&5).unwrap();
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
