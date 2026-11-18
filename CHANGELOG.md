# Changelog

## 0.1.0 release candidate

- Offline TLE and OMM import, deterministic monthly snapshots, SATCAT/RCS enrichment, catalog validation and portable SQLite export.
- WGS-72 SGP4 propagation, Earth-fixed position and rotating-frame velocity, synthetic initialization provenance, and digest-verified mission manifests.
- Mission-wide fixed radar-union populations, standalone execution and mock external UTC/lifecycle control.
- DIS 7 UDP and Protobuf/Zenoh publishing with independent bounded worker queues.
- Local Compose telemetry verification, a Helm chart, operator workflows and a release benchmark harness.

Prepare a release after the native, Compose and Helm gates pass. Record throughput and any gaps from the benchmark. Check release archive integrity using its SHA-256 manifest and run the included binary against the offline fixture. Keep the compatibility policy and limitations with the artifact. [Implementation evidence](docs/implementation/progress.md)
