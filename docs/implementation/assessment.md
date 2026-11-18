# Implementation assessment

Baseline: main at `6021e74ca39ef4c7cc84e19d941f5161aec3c262`. Audited the 32 GitHub issue bodies, repository contracts, six ADRs, six workspace members, all three migrations, fixtures and CI/deployment files.

| Issue | Baseline implementation and evidence |
| --- | --- |
| 001 | Six crates and a CLI. Help exposed only the historical bootstrap. |
| 002 | Positive NORAD IDs, absolute UTC, validated finite TEME and ECEF types, provenance and fixture round trips. |
| 003 | Strict v1 YAML including numeric DIS destinations, all-object selection and rate/clock validation. No radar or Zenoh config. |
| 004 | Offline Cargo checks after dependency fetch, nonroot image, Compose scaffold smoke, empty Helm template set. See baseline check results below. |
| 005 | Transport-neutral clock and publisher ports with mock contract tests. Concrete clocks and sinks absent. |
| 006 | Strict SQLite tables, foreign keys, indexed epochs, transactional migration tests and portable file format. |
| 007 | TLE, OMM JSON and CSV single-record parsers, checksums, epoch preservation and negative fixtures. No general bulk import CLI. |
| 008 | Bounded Space-Track GP History acquisition, retries, page transactions and resumable scope checkpoints. Tests inject transport replies. Live account and large archive access remain unverified. |
| 009–012 | No materialized monthly selection, metadata refresh, complete bootstrap or offline export. |
| 013–016 | No SGP4, frame conversion, production clock or hot-path engine. |
| 017–020 | No catalog-to-manifest pipeline, radar geometry or selection. |
| 021–025 | No DIS encoder, identity registry, transport or runtime lifecycle. |
| 026–029 | Helm metadata only, smoke-only Compose, no pipeline benchmark or release runbook. |
| 030–032 | No Protobuf, Zenoh or FABRIC control adapter. |

The existing frame-safe domain types, strict config, SQLite constraints and clock/publisher ports are suitable. Preserve them. SQLite holds raw inputs and normalized UTC, so preparation should read and verify exact inputs once; execution receives a manifest and never opens the database. Historical queries currently target at most 50 explicit objects over 60 completed months and use an exclusive end. Keep that established acquisition contract and support broad populations via local archives. Avoid applying stale-element propagation over years. Validate the rotating-frame velocity term and DIS wire offsets independently. Mission sampling must account for narrow windows, with assumptions recorded. At target scale, UDP packet rate and downstream reception may dominate CPU-only SGP4 measurements.

Execution order:

1. Add offline ingest, snapshots, metadata, coverage and portable export alongside the existing authenticated importer.
2. Implement SGP4/frame conversion and concrete clocks, then radar geometry and manifest preparation.
3. Add DIS encoding, identities and transport, and connect the deterministic runtime and lifecycle.
4. Add the Protobuf/Zenoh and mock control adapters using the same canonical batches.
5. Verify native and container workflows, Helm and performance, then reconcile evidence per issue.

Baseline Rust 1.100.0: formatting, Clippy with warnings denied, and 52 tests passed. CLI help ran. Compose configuration rendered. The baseline image build failed during Cargo fetch because the managed proxy CA was absent from the builder trust store.

## Implementation follow-through

The implemented pipeline retained the approved type and storage boundaries and added a manifest-only runtime. See the [issue and verification record](progress.md) for acceptance evidence. The assumptions that matter most to downstream operators are the conservative radar speed bound, UTC-as-UT1 with zero pole motion, and UDP loss at high packet rates. The [benchmark report](../performance/benchmark-report.md) records the measured impact of batching, allocation reduction and receiver-buffer tuning.
