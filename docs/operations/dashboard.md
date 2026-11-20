# Compose engineering dashboard

Run from the repository root.

```sh
docker compose up --build
```

Open `http://127.0.0.1:8080`. The `hoth` service imports the orbital records and prepares an immutable mission manifest. The `dashboard` service reads that manifest from a read-only volume and opens its telemetry receivers before HOTH starts publishing. The browser HTTP port is bound to loopback.

## Edit the mounted inputs

The simulator reads [scenario.yaml](../../deploy/dashboard-inputs/scenario.yaml) from `/inputs/scenario.yaml`. Set `time.start_utc` to change the simulation epoch, or edit the site coordinates, azimuth, elevation, and ranges under `radars`. The demo displays the configured radar coverage with all satellites selected. Set `selection.mode: radar_union` to use the radars for mission-wide membership.

To use an input directory outside the checked-in example, copy the scenario to that directory and start Compose with `HOTH_INPUT_DIR` set to the directory path. Files must be readable by the container user, UID 10001.

```sh
mkdir -p local-inputs
cp deploy/dashboard-inputs/scenario.yaml local-inputs/scenario.yaml
cp tests/fixtures/catalog/population.json local-inputs/elements.json
export HOTH_INPUT_DIR="$PWD/local-inputs"
docker compose up -d --build --wait
```

The simulator imports `elements.json` when present and otherwise uses the bundled 100-object fixture. Set the `EPOCH` field in an OMM record to supply a source element epoch and set `catalog.snapshot` to the corresponding month. The requested simulation epoch comes from `time.start_utc`. The manifest preserves the source epoch and records its initialization policy. For other orbital input formats, set `HOTH_ELEMENTS_PATH` to the container path, such as `/inputs/elements.tle`, and `HOTH_ELEMENTS_FORMAT` to `tle`, `omm-json`, or `omm-csv`.

After saving input changes, recreate the two services to start a fresh mission with the new manifest. Reload the dashboard after Compose reports both services healthy.

```sh
docker compose up -d --no-build --force-recreate --wait hoth dashboard
```

Use `HOTH_DASHBOARD_PORT` to change the browser port. If the default Docker subnet conflicts with another network, set `HOTH_SIM_SUBNET`, `HOTH_DASHBOARD_IP`, and `HOTH_ZENOH_IP` together and adjust the two `publishers.destination` addresses in the scenario. Keep the DIS destination port at 3000 and the Zenoh port at 7447 for this Compose setup.

## Inspect telemetry

Select a NORAD catalog ID and open **Wire monitor**. The counters show accepted PDUs, distinct entities seen, and rejected datagrams. The selected message shows the DIS exercise and entity identity, sender, receive time, decoded fields, and the original 144-byte UDP payload. Use **Freeze packet** to inspect the payload while the globe and counters continue to update.

The PDU timestamp is a fraction of the simulation hour. **Received UTC** is the dashboard host's receive time. The globe uses simulation UTC from the live Zenoh batch. The monitor retains the latest matching HOTH DIS PDU for each manifest entity. A packet for a different exercise, identity, or marking is counted as ignored. Counters describe packets that reached the dashboard.

The demo runs for the duration in `time.duration_seconds`. After the simulator exits, the dashboard retains the last received packet for inspection. Use `docker compose down` to stop the services, or add `--volumes` to remove the disposable manifest volume.

## Verify the stack

```sh
bash scripts/dashboard-e2e.sh
```

This test starts the stack with a temporary input mount, checks live DIS and Zenoh payloads, edits the FOV and both simulation and source epochs, then repeats the checks after recreating the runtime and dashboard.
