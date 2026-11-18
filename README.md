# HOTH — High-Throughput Orbit Telemetry Harness

HOTH generates reproducible orbital telemetry for local and distributed simulation. It prepares representative satellite populations from current and historical orbital elements, propagates absolute UTC states using SGP4, and publishes Earth-fixed kinematics over DIS 7 UDP and Protobuf/Zenoh. Historical elements describe the input population. Synthetic initialization creates plausible stimulus for a requested mission time.

## Run locally

Build the Rust CLI and use the checked-in orbit catalog fixture and scenario. [Native quickstart](docs/operations/quickstart.md)

```sh
cargo build --release --locked -p hoth
bash scripts/e2e.sh
```

The script builds and runs the Docker Compose integration workflow, asserts DIS and routed Zenoh output from the same 100 satellites, drives mock lifecycle commands, and cleans up. Initial dependency and image fetches require package registry access. The simulation services use a private network and fixture data.

## Reference

- Inspect satellite trajectories on an offline globe. [Orbit test harness](docs/operations/orbit-globe.md)
- Catalog storage, acquisition and export. [SQLite catalog](docs/architecture/catalog.md) · [Space-Track](docs/architecture/space-track.md)
- Configuration, initialization, frames and selection. [YAML v1](docs/interfaces/scenario-yaml.md) · [Orbit and manifest ADR](docs/decisions/0007-manifest-orbit-policy.md)
- Local controller and dual publishing. [Execution Fabric](docs/architecture/fabric-integration.md) · [Runtime ADR](docs/decisions/0008-runtime-and-control.md)
- Deployment and measured throughput. [Kubernetes](docs/operations/kubernetes.md) · [Benchmarks](docs/performance/benchmark-report.md)
- Recorded evidence and open acceptance criteria. [Baseline assessment](docs/implementation/assessment.md) · [Implementation progress](docs/implementation/progress.md)
