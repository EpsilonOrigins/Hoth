# Orbit test harness

Use the globe to inspect a prepared HOTH scenario and compare its Earth-fixed states against references. Run a self-contained HTML replay, serve high-resolution states from a local manifest, or follow live Zenoh telemetry.

## Run the fixture

From the repository root:

```sh
bash scripts/orbit-globe.sh
```

Open the printed HTML path. The demo shows all 100 satellites together on a 95-minute timeline with 15-second sample intervals and two radar fields of view. The script uses a temporary catalog and manifest. A repeat run creates a fresh report path.

For browsers that restrict local file URLs, serve the report directory on loopback with `python3 -m http.server 8020 --bind 127.0.0.1 --directory data` and open the printed path under `http://127.0.0.1:8020`. For example, open `http://127.0.0.1:8020/orbit-globe.html` for `data/orbit-globe.html`.

## Use your own scenario

Prepare the manifest with `hoth scenario prepare`, then pass it to the same script:

```sh
bash scripts/orbit-globe.sh \
  --manifest data/mission.json --output data/my-orbit.html \
  --step-seconds 10 --norad 25544
```

Omit `--norad` to include every satellite in the manifest. Repeat it to export a subset. The report always samples the mission start and exact mission end. The default interval is 15 seconds and increases automatically for larger missions to fit within 500,000 total satellite samples. If you set an explicit interval and hit that limit, increase `--step-seconds` or omit it for automatic sampling. Each export writes to a new path.

## Use high-resolution replay

Serve the same manifest on loopback to keep an exact interval for large populations.

```sh
bash scripts/orbit-globe.sh --manifest data/mission.json \
  --serve 127.0.0.1:8080 --step-ms 40
```

Open `http://127.0.0.1:8080`. The server propagates the population only for requested timestamps. The default interval is one second. Use `--step-ms 40` for a 25 Hz timeline, or `--step-seconds` for a whole-second interval. The timeline includes the exact mission end. Each step and CSV row uses the requested cadence. Playback can skip display frames to follow the selected speed. The orbit lines use a decimated set of points for display.

## Follow live telemetry

Start a local Zenoh router. The live fixture uses its host loopback address below. Use the catalog and `$demo` from the manual quickstart. [Manual setup](../../README.md#manually-from-source)

```sh
docker run --rm -p 127.0.0.1:17447:7447 \
  eclipse/zenoh:1.7.2 -l tcp/0.0.0.0:7447
```

```sh
"$hoth" scenario prepare --database "$demo/catalog.sqlite" \
  --config tests/fixtures/scenarios/globe-live.yaml --output "$demo/live.json"
bash scripts/orbit-globe.sh --manifest "$demo/live.json" --serve 127.0.0.1:8080 \
  --zenoh 127.0.0.1:17447 --run-id globe-demo
```

Open `http://127.0.0.1:8080`. In a separate terminal at the repository root, set `demo` to the directory containing the prepared `live.json`, then run HOTH with the same manifest and run ID.

```sh
hoth="${CARGO_TARGET_DIR:-target}/release/hoth"
"$hoth" run --manifest "$demo/live.json" --run-id globe-demo
```

The viewer follows the latest Protobuf batch on `hoth/v1/globe-demo/states`, including backward seeks and exact simulation UTC. Live mode retains one pending batch and refreshes the display about five times per second while keeping all available satellites in the batch. The path shown for the selected satellite is predicted from the manifest. Live mode is a monitor with no telemetry recording. Bind the HTTP viewer to loopback for local use.

## Inspect an orbit

- Scrub the UTC timeline to move all satellites together. Hollow markers indicate satellites behind Earth. Uncheck **Behind Earth** to hide those markers. The plotted count reflects the current view and any unavailable states.
- Drag to rotate the globe and use the wheel or zoom buttons to zoom. Use **Fit all satellites** to restore the scale for the full population after zooming.
- Select a NORAD ID to highlight its marker and inspect its trajectory. The centering button points the globe at that satellite. The orbit and ground-track checkboxes control its two paths. Dim orbit segments show parts of the trajectory behind Earth. Unavailable propagation samples appear as gaps.
- Toggle **Radar coverage** to show the configured sensor locations and wireframes bounded by azimuth, elevation, and slant range. Coverage is a geometry overlay for the manifest's fixed membership.
- Read the canonical ECEF vectors and source epoch in the inspector. In replay mode, download CSV for the selected satellite's full sampled series, with position in meters and velocity in meters per second.

HOTH propagates each sample with the same engine used for telemetry publishing. The display connects sample points and projects the ground track radially onto a spherical globe. The inspector computes geodetic coordinates with the WGS 84 ellipsoid. Synthetic initialization is marked in the source provenance. [Earth texture attribution](../../apps/hoth-cli/examples/orbit_globe/earth.LICENSE)

```sh
cargo test --locked -p hoth --example orbit_globe
```

## Validate the propagation

Generate a replay starting at the independent Vallado Vanguard 1 epoch.

```sh
bash scripts/orbit-globe.sh --reference-demo
```

Open the printed HTML and choose **Reference ECEF CSV**. Load the [Vallado vectors CSV](../../crates/hoth-orbits/tests/fixtures/vallado/vectors.csv). At the first sample for NORAD 5, the inspector shows the Euclidean position and velocity differences. Later matches occur every six hours in this one-day replay. The reference orbit connects the supplied samples and a square marks the matching reference position.

You can also load a HOTH state CSV with ECEF positions in meters and velocities in meters per second. The browser matches the selected NORAD ID and full UTC timestamp, including its fractional part. A sample without an exact match shows an unavailable comparison. The browser reads the file locally.

Run the offline Vallado dataset test to compare the same propagation engine used by the globe against independent TEME and ECEF vectors.

```sh
cargo test --locked -p hoth-orbits --test reference -- --nocapture
```

The test checks 86 timestamped states across five published orbit cases and a synthetic re-epoch case. [Reference data and tolerances](../../crates/hoth-orbits/tests/fixtures/vallado/README.md)
