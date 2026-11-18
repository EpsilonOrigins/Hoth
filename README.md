# HOTH — High-Throughput Orbit Telemetry Harness

HOTH is an offline-capable Rust satellite simulation stimulus generator. It initializes plausible orbital populations from monthly historical orbital-element snapshots, propagates states using SGP4, and publishes kinematics via pluggable publishers. **HOTH does not reconstruct historical ground truth.**

## Approved baseline
- Private personal repository; Rust Cargo workspace.
- Five-year rolling catalog of monthly orbital snapshots, with authenticated Space-Track historical backfill and local import. CelesTrak supports current updates; no runtime web access.
- SQLite operational catalog; immutable, reproducible scenario manifests; versioned YAML scenarios.
- SGP4; explicitly documented TEME/ECEF coordinate and velocity transforms; f64 calculations.
- Simulation clock supports standalone and external authority, pause, seek, replay.
- Mission-wide union of radar FOVs selects a fixed satellite population; radar detection is out of scope.
- Pluggable publisher contract; DIS 7 Entity State over UDP in v1; Zenoh/protobuf after v1.
- Kubernetes production deployment via Helm; Docker Compose for integration testing.
- Benchmark 15,000 objects at 25 Hz and explore up to 50,000; performance is not presumed.

## Development
See [AGENTS.md](AGENTS.md), [architecture](docs/architecture/overview.md), and [agent execution plan](docs/architecture/agent-plan.md). The 32 planned issues are ordered by dependencies, with M0–M5 targeting v1 and M6 post-v1.

Build and inspect the CLI with `cargo run --locked -p hoth -- --help`. Use `--version` to print the package version.

This is a foundation scaffold; the presence of a CLI or CI configuration does not imply completed orbital or DIS functionality.
