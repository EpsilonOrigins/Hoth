# HOTH — High-Throughput Orbit Telemetry Harness

HOTH generates reproducible orbital telemetry for local and distributed simulation. It prepares representative satellite populations from current and historical orbital elements, propagates absolute UTC states using SGP4, and publishes Earth-fixed kinematics over DIS 7 UDP and Protobuf/Zenoh. Historical elements describe the input population. Synthetic initialization creates plausible stimulus for a requested mission time.

## Run locally

Build the container and open the bundled globe at `http://localhost:8080`.

```sh
docker build -f deploy/Dockerfile -t hoth:local .
docker run --rm -p 127.0.0.1:8080:8080 hoth:local
```

The image also runs the CLI, the offline propagation reference suite, and the benchmark. Use `bash scripts/e2e.sh` to verify DIS and routed Zenoh telemetry with Docker Compose. [Container commands](docs/operations/container.md) · [Native quickstart](docs/operations/quickstart.md)

## Reference

- Inspect satellite trajectories on an offline globe. [Orbit test harness](docs/operations/orbit-globe.md)
- Catalog storage, acquisition and export. [SQLite catalog](docs/architecture/catalog.md) · [Space-Track](docs/architecture/space-track.md)
- Configuration, initialization, frames and selection. [YAML v1](docs/interfaces/scenario-yaml.md) · [Orbit and manifest ADR](docs/decisions/0007-manifest-orbit-policy.md)
- Local controller and dual publishing. [Execution Fabric](docs/architecture/fabric-integration.md) · [Runtime ADR](docs/decisions/0008-runtime-and-control.md)
- Deployment and measured throughput. [Kubernetes](docs/operations/kubernetes.md) · [Benchmarks](docs/performance/benchmark-report.md)
- Recorded evidence and open acceptance criteria. [Baseline assessment](docs/implementation/assessment.md) · [Implementation progress](docs/implementation/progress.md)
