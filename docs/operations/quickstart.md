# HOTH quickstart

## Native offline simulation

Prepare the fixture catalog and mission, start the DIS receiver, and run HOTH from the repository root. [Manual build and run commands](../../README.md#manually-from-source)

The example emits 100 satellites for three seconds of simulation. The manifest contains everything runtime needs. Repeat the run from the same manifest to replay a given absolute UTC. Use external control for exact chosen timestamps. SIGTERM and Ctrl-C stop the process and drain the publisher queues. A new preparation requires a fresh manifest path.

## Native globe replay

In the same terminal as the manual build, use the prepared catalog at `$demo/catalog.sqlite` to create a 95-minute globe replay. The globe samples the manifest directly, so you can generate it at any time after preparing the catalog.

```sh
"$hoth" scenario prepare --database "$demo/catalog.sqlite" \
  --config tests/fixtures/scenarios/globe.yaml --output "$demo/globe.json"
cargo run --release --locked -p hoth --example orbit_globe -- \
  --manifest "$demo/globe.json" --output "$demo/orbit-globe.html"
printf '%s\n' "$demo/orbit-globe.html"
```

Open the printed HTML file in a browser to see all 100 satellites together. For browsers that restrict local file URLs, serve the demo directory on loopback and open `http://127.0.0.1:8020/orbit-globe.html`. [Globe controls](orbit-globe.md#inspect-an-orbit)

```sh
python3 -m http.server 8020 --bind 127.0.0.1 --directory "$demo"
```

Run the propagation reference suite directly from the repository root to compare the orbit and frame transforms against 86 reference states. [Reference data and tolerances](../../crates/hoth-orbits/tests/fixtures/vallado/README.md)

```sh
cargo test --locked -p hoth-orbits --test reference -- --nocapture
```

## Catalog preparation

Use `catalog import --format tle` for concatenated two-line or named three-line records, `--format omm-json` for a single object or array, or `--format omm-csv` for a table. Put permitted Space-Track historical archives and current CelesTrak GP exports through the same offline importer. Use `--source` and, where known, `--acquired-at` to preserve acquisition provenance. Import SATCAT plus optional numeric RCS with `catalog metadata --file satcat.json --source satcat`. Provider RCS size classes remain descriptive source data. Numeric square meters come from the RCS field or the explicit `rcs_m2` field.

```sh
hoth catalog bootstrap --years 5 --interval monthly --dry-run
hoth catalog snapshot --database data/hoth.db --start 2021-11 --end 2026-11 --max-age-days 17
hoth catalog validate --database data/hoth.db --start 2021-11 --end 2026-11
hoth catalog export --database data/hoth.db --output data/offline.sqlite
sha256sum data/offline.sqlite > data/offline.sqlite.sha256
```

Bootstrap uses an exclusive end and 60 completed months for five years. Explicit small groups can use the authenticated importer and its restart checkpoints. For larger populations use authorized bulk archives on disk. Keep the database between attempts. Snapshot and validation reports list empty or absent months within the requested range. Transfer the exported SQLite artifact, check the SHA-256 on receipt, and use it directly as `--database`. SQLite stores the schema version, provenance, snapshot membership and per-record hashes. Prepare the manifest before disconnected execution.

## Local integration

`bash scripts/e2e.sh` builds the image, runs the router, fixture setup, HOTH and verifier, and returns the verifier and runtime result. For an already built `hoth:local` image, use `HOTH_SKIP_BUILD=1 bash scripts/e2e.sh`. The router image must already be available for an air-gapped run. The Compose test uses the private subnet `172.29.186.0/24`. If it overlaps another local network, update the Compose addresses and the fixture destinations together.

On an invalid config or manifest, inspect the error path or checksum and prepare the manifest again. For zero UDP packets, confirm destination, socket and receiver startup order. For missing Zenoh messages, verify the router address and run key. For dropped batches or short counts, inspect per-publisher counters and receiver counts. High-rate UDP can lose packets even when the sender reports successful submissions.
