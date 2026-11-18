# Orbit test harness

Use the globe to inspect a prepared HOTH scenario and reproduce its canonical state at sampled simulation times. The report bundles the Earth texture, sampled states, and viewer into a single HTML file. Open it in a browser without internet access.

## Run the fixture

From the repository root:

```sh
bash scripts/orbit-globe.sh
```

Open the printed HTML path. The default fixture includes 100 manifest members and the viewer shows four selected satellites for 95 minutes at 15-second sample intervals. The script uses a temporary catalog and manifest. A repeat run creates a fresh report path.

For browsers that restrict local file URLs, serve the report directory on loopback with `python3 -m http.server 8020 --bind 127.0.0.1 --directory data` and open `http://127.0.0.1:8020/orbit-globe.html`.

## Use your own scenario

Prepare the manifest with `hoth scenario prepare`, then pass it to the same script:

```sh
bash scripts/orbit-globe.sh \
  --manifest data/mission.json --output data/my-orbit.html \
  --step-seconds 10 --norad 25544
```

Repeat `--norad` to include multiple satellites. Omitting it includes every satellite in the manifest. The report always samples the mission start and exact mission end. For reports above 500,000 total satellite samples, increase `--step-seconds` or select fewer satellites. Each export writes to a new path.

## Inspect an orbit

- Select a NORAD ID and scrub the simulation UTC timeline. Use the arrow buttons for single-sample steps.
- Drag the globe to rotate it, use the mouse wheel or the zoom buttons to zoom, and use the centering button to find the selected satellite.
- Use the orbit and ground-track checkboxes to isolate the two paths. Dim orbit segments show parts of the trajectory behind Earth. Unavailable propagation samples appear as gaps.
- Read the canonical ECEF vectors and source epoch in the inspector. Download CSV for the selected satellite's full sampled series, with position in meters and velocity in meters per second.

HOTH propagates each sample with the same engine used for telemetry publishing. The display connects sample points and projects the ground track radially onto a spherical globe. The inspector computes geodetic coordinates with the WGS 84 ellipsoid. Synthetic initialization is marked in the source provenance. [Earth texture attribution](../../apps/hoth-cli/examples/orbit_globe/earth.LICENSE)

```sh
cargo test --locked -p hoth --example orbit_globe
```
