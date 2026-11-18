# HOTH quickstart

## Native offline simulation

Install the Rust toolchain in `rust-toolchain.toml` and a C compiler. HOTH builds its own SQLite library.

```sh
cargo fetch --locked
cargo build --release --locked --offline -p hoth
hoth=target/release/hoth
mkdir -p data
"$hoth" catalog import --database data/fixture.sqlite --format omm-json \
  --file tests/fixtures/catalog/population.json --source fixture
"$hoth" catalog snapshot --database data/fixture.sqlite --month 2021-11
"$hoth" catalog validate --database data/fixture.sqlite
"$hoth" scenario prepare --database data/fixture.sqlite \
  --config tests/fixtures/scenarios/standalone.yaml --output data/mission.json
```

Start the receiver in another terminal before running HOTH.

```sh
python3 scripts/dis_receive.py --count 100
```

```sh
target/release/hoth run --manifest data/mission.json
```

The example emits 100 satellites for three seconds of simulation. The manifest contains everything runtime needs. Repeat the run from the same manifest to replay a given absolute UTC. Use external control for exact chosen timestamps. SIGTERM and Ctrl-C stop the process and drain the publisher queues. A new preparation requires a fresh manifest path.

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
