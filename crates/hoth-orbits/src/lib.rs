//! Absolute-time SGP4 propagation and offline TEME to Earth-fixed conversion.

use hoth_domain::{
    orbit::OrbitalElement,
    state::{
        EcefState, EpochInitialization, NoradId, SatelliteState, SimulationUtc, StateProvenance,
        StateValidationError, TemeState, UtcInstant,
    },
};
use sgp4::{Constants, MinutesSinceEpoch, Orbit, WGS72};
use std::f64::consts::TAU;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PropagationError {
    #[error("SGP4 initialization failed: {0}")]
    Initialization(String),
    #[error("SGP4 propagation failed: {0}")]
    Prediction(#[from] sgp4::Error),
    #[error("predicted position is inside the SGP4 reference Earth")]
    Decayed,
    #[error(transparent)]
    State(#[from] StateValidationError),
}

/// Holds initialized SGP4 constants. Each prediction starts from this fixed epoch.
pub struct Propagator {
    constants: Constants,
    epoch: UtcInstant,
    norad_id: NoradId,
    provenance: StateProvenance,
}

impl Propagator {
    pub fn new(
        element: &OrbitalElement,
        initialization: EpochInitialization,
    ) -> Result<Self, PropagationError> {
        let epoch = match initialization {
            EpochInitialization::SourceEpoch => element.epoch_utc,
            EpochInitialization::Synthetic { epoch_utc } => epoch_utc,
        };
        let m = &element.mean_elements;
        let bstar = match initialization {
            EpochInitialization::SourceEpoch => m.bstar.unwrap_or(0.0),
            EpochInitialization::Synthetic { .. } => 0.0,
        };
        if ![
            m.inclination_deg,
            m.right_ascension_deg,
            m.eccentricity,
            m.argument_of_perigee_deg,
            m.mean_anomaly_deg,
            m.mean_motion_rev_day,
            bstar,
        ]
        .into_iter()
        .all(f64::is_finite)
            || !(0.0..=180.0).contains(&m.inclination_deg)
        {
            return Err(PropagationError::Initialization(
                "invalid mean elements".into(),
            ));
        }
        let orbit = Orbit::from_kozai_elements(
            &WGS72,
            m.inclination_deg.to_radians(),
            m.right_ascension_deg.to_radians(),
            m.eccentricity,
            m.argument_of_perigee_deg.to_radians(),
            m.mean_anomaly_deg.to_radians(),
            m.mean_motion_rev_day * TAU / 1440.0,
        )
        .map_err(|error| PropagationError::Initialization(error.to_string()))?;
        let constants = Constants::new(
            WGS72,
            sgp4::afspc_epoch_to_sidereal_time,
            sgp4::julian_years_since_j2000_afspc_compatibility_mode(&epoch.naive_utc()),
            bstar,
            orbit,
        )
        .map_err(|error| PropagationError::Initialization(error.to_string()))?;
        let raw = &element.raw_source;
        Ok(Self {
            constants,
            epoch,
            norad_id: element.norad_id,
            provenance: StateProvenance::new(
                raw.provenance.source.clone(),
                raw.provenance.record_id.clone(),
                element.epoch_utc,
                initialization,
            )?,
        })
    }

    pub fn norad_id(&self) -> NoradId {
        self.norad_id
    }

    pub fn teme(&self, at: SimulationUtc) -> Result<TemeState, PropagationError> {
        let elapsed = at.instant() - self.epoch;
        let solution = self
            .constants
            .propagate_afspc_compatibility_mode(MinutesSinceEpoch(
                elapsed.as_seconds_f64() / 60.0,
            ))?;
        if solution.position.iter().map(|x| x * x).sum::<f64>() < WGS72.ae.powi(2) {
            return Err(PropagationError::Decayed);
        }
        Ok(TemeState::new(
            at,
            solution.position.try_into()?,
            solution.velocity.try_into()?,
        ))
    }

    pub fn state(&self, frame: EarthRotation) -> Result<SatelliteState, PropagationError> {
        Ok(SatelliteState {
            norad_id: self.norad_id,
            at_utc: frame.at,
            ecef: frame.convert(self.teme(frame.at)?)?,
            provenance: self.provenance.clone(),
        })
    }
}

/// IAU 1982 GMST rotation with UT1 approximated by UTC and zero polar motion.
/// Compute once per batch since every object shares the same UTC.
#[derive(Clone, Copy)]
pub struct EarthRotation {
    at: SimulationUtc,
    sin: f64,
    cos: f64,
}

impl EarthRotation {
    pub fn at(at: SimulationUtc) -> Self {
        let date = at.instant();
        let unix_seconds = date.timestamp() - 946_728_000; // J2000 noon
        let days = unix_seconds.div_euclid(86_400) as f64;
        let fraction = (unix_seconds.rem_euclid(86_400) as f64
            + f64::from(date.timestamp_subsec_nanos()) * 1e-9)
            / 86_400.0;
        let t = (days + fraction) / 36_525.0;
        let seconds =
            67_310.548_41 + 86_400.0 * fraction + 8_640_184.812_866 * t + 0.093_104 * t * t
                - 6.2e-6 * t * t * t;
        let (sin, cos) = (seconds.rem_euclid(86_400.0) * TAU / 86_400.0).sin_cos();
        Self { at, sin, cos }
    }

    pub fn convert(self, state: TemeState) -> Result<EcefState, StateValidationError> {
        assert_eq!(
            state.at_utc, self.at,
            "frame rotation and TEME timestamps must match"
        );
        const OMEGA_RAD_S: f64 = 7.292_115_146_706_98e-5;
        let rotate = |[x, y, z]: [f64; 3]| {
            [
                1000.0 * (self.cos * x + self.sin * y),
                1000.0 * (-self.sin * x + self.cos * y),
                1000.0 * z,
            ]
        };
        let p = rotate(state.position_km.xyz());
        let mut v = rotate(state.velocity_km_s.xyz());
        v[0] += OMEGA_RAD_S * p[1];
        v[1] -= OMEGA_RAD_S * p[0];
        Ok(EcefState::new(p.try_into()?, v.try_into()?))
    }
}

pub fn teme_to_ecef(state: TemeState) -> Result<EcefState, StateValidationError> {
    EarthRotation::at(state.at_utc).convert(state)
}
