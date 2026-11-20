# Orbit test harness

Use the globe to inspect a prepared HOTH scenario and reproduce its canonical state at sampled simulation times. The report bundles the Earth texture, sampled states, and viewer into a single HTML file. Open it in a browser without internet access.

## Run the fixture

From the repository root:

```sh
bash scripts/orbit-globe.sh
```

Open the printed HTML path. The demo shows all 100 satellites together on a 95-minute timeline with 15-second sample intervals. The script uses a temporary catalog and manifest. A repeat run creates a fresh report path.

For browsers that restrict local file URLs, serve the report directory on loopback with `python3 -m http.server 8020 --bind 127.0.0.1 --directory data` and open the printed path under `http://127.0.0.1:8020`. For example, open `http://127.0.0.1:8020/orbit-globe.html` for `data/orbit-globe.html`.

## Use your own scenario

Prepare the manifest with `hoth scenario prepare`, then pass it to the same script:

```sh
bash scripts/orbit-globe.sh \
  --manifest data/mission.json --output data/my-orbit.html \
  --step-seconds 10 --norad 25544
```

Omit `--norad` to include every satellite in the manifest. Repeat it to export a subset. The report always samples the mission start and exact mission end. The default interval is 15 seconds and increases automatically for larger missions to fit within 500,000 total satellite samples. If you set an explicit interval and hit that limit, increase `--step-seconds` or omit it for automatic sampling. Each export writes to a new path.

## Inspect an orbit

- Scrub the UTC timeline to move all satellites together. Hollow markers indicate satellites behind Earth. Uncheck **Behind Earth** to hide those markers. The plotted count reflects the current view and any unavailable states.
- Drag to rotate the globe and use the wheel or zoom buttons to zoom. Use **Fit all satellites** to restore the scale for the full population after zooming.
- Select a NORAD ID to highlight its marker and inspect its trajectory. The centering button points the globe at that satellite. The orbit and ground-track checkboxes control its two paths. Dim orbit segments show parts of the trajectory behind Earth. Unavailable propagation samples appear as gaps.
- Read the canonical ECEF vectors and source epoch in the inspector. Download CSV for the selected satellite's full sampled series, with position in meters and velocity in meters per second.

HOTH propagates each sample with the same engine used for telemetry publishing. The display connects sample points and projects the ground track radially onto a spherical globe. The inspector computes geodetic coordinates with the WGS 84 ellipsoid. Synthetic initialization is marked in the source provenance. [Earth texture attribution](../../apps/hoth-cli/examples/orbit_globe/earth.LICENSE)

```sh
cargo test --locked -p hoth --example orbit_globe
```

## Validate the propagation

Run the offline Vallado dataset test to compare the same propagation engine used by the globe against independent TEME and ECEF vectors.

```sh
cargo test --locked -p hoth-orbits --test reference -- --nocapture
```

The test checks 86 timestamped states across five published orbit cases and a synthetic re-epoch case. [Reference data and tolerances](../../crates/hoth-orbits/tests/fixtures/vallado/README.md)
