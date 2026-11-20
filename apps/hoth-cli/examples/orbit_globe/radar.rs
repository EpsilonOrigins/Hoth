use hoth_config::RadarConfig;
use hoth_scenario::radar::Radar;
use serde::Serialize;

/// ECEF wireframe of the configured azimuth, elevation and slant-range gates.
#[derive(Serialize)]
pub struct Overlay {
    id: String,
    origin: [f64; 3],
    lines: Vec<Vec<[f64; 3]>>,
}

impl Overlay {
    pub fn new(config: &RadarConfig) -> Self {
        let radar = Radar::new(config);
        let point = |azimuth: f64, elevation: f64, range: f64| {
            let (sin_az, cos_az) = azimuth.to_radians().sin_cos();
            let (sin_el, cos_el) = elevation.to_radians().sin_cos();
            radar.ecef_position([
                range * cos_el * sin_az,
                range * cos_el * cos_az,
                range * sin_el,
            ])
        };
        let full = config.azimuth_min_deg == 0.0 && config.azimuth_max_deg == 360.0;
        let azimuth = config.azimuth_min_deg;
        let sweep = if full {
            360.0
        } else {
            (config.azimuth_max_deg - azimuth).rem_euclid(360.0)
        };
        let elevations = [config.elevation_min_deg, config.elevation_max_deg];
        let ranges = [config.range_min_m, config.range_max_m];
        let spokes = if full {
            vec![0.0, 90.0, 180.0, 270.0]
        } else {
            vec![azimuth, azimuth + sweep]
        };
        let mut lines = Vec::new();
        for range in ranges.into_iter().filter(|&range| range > 0.0) {
            for elevation in elevations {
                lines.push(
                    (0..=72)
                        .map(|i| point(azimuth + sweep * f64::from(i) / 72.0, elevation, range))
                        .collect(),
                );
            }
            for &angle in &spokes {
                lines.push(
                    (0..=36)
                        .map(|i| {
                            let elevation = elevations[0]
                                + (elevations[1] - elevations[0]) * f64::from(i) / 36.0;
                            point(angle, elevation, range)
                        })
                        .collect(),
                );
            }
        }
        for angle in spokes {
            for elevation in elevations {
                lines.push(
                    ranges
                        .into_iter()
                        .map(|range| point(angle, elevation, range))
                        .collect(),
                );
            }
        }
        Self {
            id: config.id.clone(),
            origin: radar.ecef_position([0.0; 3]),
            lines,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_wraps_north_and_draws_both_range_boundaries() {
        let overlay = Overlay::new(&RadarConfig {
            id: "radar".into(),
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            altitude_m: 100.0,
            azimuth_min_deg: 350.0,
            azimuth_max_deg: 20.0,
            elevation_min_deg: 0.0,
            elevation_max_deg: 90.0,
            range_min_m: 100.0,
            range_max_m: 600.0,
        });
        assert_eq!(overlay.origin, [6_378_237.0, 0.0, 0.0]);
        assert!(overlay.lines[0][0][1] < 0.0);
        assert!(overlay.lines[0][72][1] > 0.0);
        assert_eq!(overlay.lines[1][0][0] - overlay.origin[0], 100.0);
        assert_eq!(overlay.lines[5][0][0] - overlay.origin[0], 600.0);
    }
}
