# HOTH — High-Throughput Orbit Telemetry Harness

HOTH generates reproducible orbital telemetry for local and distributed simulation. It prepares representative satellite populations from current and historical orbital elements, propagates absolute UTC states using SGP4, and publishes Earth-fixed kinematics over DIS 7 UDP and Protobuf/Zenoh. Historical elements describe the input population. Synthetic initialization creates plausible stimulus for a requested mission time.

## Run locally

### Docker Compose dashboard

```sh
docker compose up --build
```

Open `http://127.0.0.1:8080` for the live globe and DIS 7 wire monitor. The dashboard runs in its own container. Edit the mounted scenario to change radar fields of view and simulation start time. [Compose dashboard guide](docs/operations/dashboard.md)

### Docker package

Build the container and open the bundled globe at `http://localhost:8080`.

```sh
docker build -f deploy/Dockerfile -t hoth:local .
docker run --rm -p 127.0.0.1:8080:8080 hoth:local
```

The image also runs the CLI, the offline propagation reference suite, and the benchmark. Use `bash scripts/e2e.sh` to verify DIS and routed Zenoh telemetry with Docker Compose. [Container commands](docs/operations/container.md)

### Manually from source

Use the Rust toolchain in `rust-toolchain.toml` and a C compiler. The sample DIS receiver uses Python 3. From the repository root, build HOTH and prepare a fresh fixture mission.

```sh
cargo build --release --locked -p hoth
hoth="${CARGO_TARGET_DIR:-target}/release/hoth"
mkdir -p data
demo=$(mktemp -d data/manual.XXXXXX)

"$hoth" catalog import --database "$demo/catalog.sqlite" --format omm-json \
  --file tests/fixtures/catalog/population.json --source local-fixture \
  --acquired-at 2026-11-17T00:00:00Z
"$hoth" catalog snapshot --database "$demo/catalog.sqlite" --month 2021-11
"$hoth" catalog validate --database "$demo/catalog.sqlite"
"$hoth" scenario prepare --database "$demo/catalog.sqlite" \
  --config tests/fixtures/scenarios/standalone.yaml --output "$demo/mission.json"
```

Start the receiver in a second terminal at the repository root. It listens on UDP port 3000 and prints the first 100 decoded packets as JSON.

```sh
python3 scripts/dis_receive.py --count 100 --timeout 60
```

Back in the first terminal, run the three-second simulation.

```sh
"$hoth" run --manifest "$demo/mission.json"
```

The fixture publishes 100 satellites. Generate an interactive globe from the same catalog, or run the offline propagation reference suite. [Manual globe and validation commands](docs/operations/quickstart.md#native-globe-replay)

## Reference

- Replay satellite trajectories, follow live Zenoh telemetry, and compare reference orbits on the globe. [Orbit test harness](docs/operations/orbit-globe.md)
- Catalog storage, acquisition and export. [SQLite catalog](docs/architecture/catalog.md) · [Space-Track](docs/architecture/space-track.md)
- Configuration, initialization, frames and selection. [YAML v1](docs/interfaces/scenario-yaml.md) · [Orbit and manifest ADR](docs/decisions/0007-manifest-orbit-policy.md)
- Local controller and dual publishing. [Execution Fabric](docs/architecture/fabric-integration.md) · [Runtime ADR](docs/decisions/0008-runtime-and-control.md)
- Deployment and measured throughput. [Kubernetes](docs/operations/kubernetes.md) · [Benchmarks](docs/performance/benchmark-report.md)
- Recorded evidence and open acceptance criteria. [Baseline assessment](docs/implementation/assessment.md) · [Implementation progress](docs/implementation/progress.md)
