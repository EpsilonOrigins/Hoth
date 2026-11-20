use hoth_config::RadarConfig;

pub struct Radar<'a> {
    config: &'a RadarConfig,
    origin: [f64; 3],
    sin_lat: f64,
    cos_lat: f64,
    sin_lon: f64,
    cos_lon: f64,
}

impl<'a> Radar<'a> {
    pub fn new(config: &'a RadarConfig) -> Self {
        let (sin_lat, cos_lat) = config.latitude_deg.to_radians().sin_cos();
        let (sin_lon, cos_lon) = config.longitude_deg.to_radians().sin_cos();
        let e_squared = 6.694_379_990_141_316_5e-3;
        let n = 6_378_137.0 / (1.0 - e_squared * sin_lat * sin_lat).sqrt();
        let a = config.altitude_m;
        let origin = [
            (n + a) * cos_lat * cos_lon,
            (n + a) * cos_lat * sin_lon,
            (n * (1.0 - e_squared) + a) * sin_lat,
        ];
        Self {
            config,
            origin,
            sin_lat,
            cos_lat,
            sin_lon,
            cos_lon,
        }
    }

    fn enu(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        let [x, y, z] = [x - self.origin[0], y - self.origin[1], z - self.origin[2]];
        let radial = self.cos_lon * x + self.sin_lon * y;
        [
            -self.sin_lon * x + self.cos_lon * y,
            -self.sin_lat * radial + self.cos_lat * z,
            self.cos_lat * radial + self.sin_lat * z,
        ]
    }

    /// Converts a local east/north/up position in meters into ECEF meters.
    pub fn ecef_position(&self, [east, north, up]: [f64; 3]) -> [f64; 3] {
        let radial = self.cos_lat * up - self.sin_lat * north;
        [
            self.origin[0] + self.cos_lon * radial - self.sin_lon * east,
            self.origin[1] + self.sin_lon * radial + self.cos_lon * east,
            self.origin[2] + self.sin_lat * up + self.cos_lat * north,
        ]
    }

    pub fn contains(&self, pos: [f64; 3]) -> bool {
        self.could_intersect(pos, 0.0)
    }

    /// Tests a ball enclosing a trajectory interval against each gate.
    pub fn could_intersect(&self, pos: [f64; 3], radius_m: f64) -> bool {
        let [east, north, up] = self.enu(pos);
        let c = self.config;
        let horizontal = east.hypot(north);
        let range = horizontal.hypot(up);
        if range + radius_m < c.range_min_m || range - radius_m > c.range_max_m {
            return false;
        }
        if range > radius_m {
            let elevation = up.atan2(horizontal).to_degrees();
            let delta = (radius_m / range).asin().to_degrees();
            if elevation + delta < c.elevation_min_deg || elevation - delta > c.elevation_max_deg {
                return false;
            }
        }
        if horizontal <= radius_m || (c.azimuth_min_deg == 0.0 && c.azimuth_max_deg == 360.0) {
            return true;
        }
        let azimuth = east.atan2(north).to_degrees().rem_euclid(360.0);
        let arc = (c.azimuth_max_deg - c.azimuth_min_deg).rem_euclid(360.0);
        let angle = (azimuth - c.azimuth_min_deg).rem_euclid(360.0);
        let delta = (radius_m / horizontal).asin().to_degrees();
        angle <= arc + delta || angle >= 360.0 - delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_elevation_and_wrapped_azimuth() {
        let config = RadarConfig {
            id: "equator".into(),
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            altitude_m: 0.0,
            azimuth_min_deg: 350.0,
            azimuth_max_deg: 20.0,
            elevation_min_deg: 5.0,
            elevation_max_deg: 90.0,
            range_min_m: 500.0,
            range_max_m: 2000.0,
        };
        let radar = Radar::new(&config);
        let local = |east, north, up: f64| [6_378_137.0 + up, east, north];
        assert!(radar.contains(local(0.0, 0.0, 1000.0)));
        assert!(radar.contains(local(-85.0, 1000.0, 500.0)));
        assert!(radar.contains(local(175.0, 1000.0, 500.0)));
        assert!(!radar.contains(local(0.0, -1000.0, 500.0)));
        assert!(!radar.contains(local(0.0, 1000.0, 0.0)));
        assert!(!radar.contains(local(0.0, 0.0, 10.0)));
        assert!(!radar.contains(local(0.0, 0.0, 2010.0)));
        assert!(radar.could_intersect(local(0.0, 0.0, 2010.0), 10.0));
        assert_eq!(
            radar.ecef_position([24.0, 30.0, 36.0]),
            local(24.0, 30.0, 36.0)
        );
    }
}
