# HOTH Agent Instructions

## Project contract
HOTH = High-Throughput Orbit Telemetry Harness. This is a satellite **stimulus generator**, not historical truth reconstruction. Preserve approved decisions in README and docs/architecture/overview.md. Do not change public interfaces, frame conventions, time semantics, or mission manifest schema without an ADR.

## Workflow
- Work on a topic branch; open a focused PR. Never push implementation directly to main.
- Respect issue dependencies; foundation interfaces must be merged before consumers are built.
- Rust stable pinned in rust-toolchain.toml. Run cargo fmt --all -- --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace before requesting review.
- Use fixtures/mocks for network-dependent tests. Never use live Space-Track or CelesTrak in CI.
- No secrets in source, logs, fixtures, images, or issue comments.
- Avoid speculative success claims: record what was actually run, results, and unverified behaviors.

## Contracts
- Canonical state: NORAD ID, simulation UTC timestamp, position/velocity with explicit units and reference frame, provenance; f64 for propagation.
- SGP4 TEME output is not ECEF; transform both position and velocity, including rotating-frame velocity term. Verify against independent reference vectors.
- Scenario YAML is versioned, validated, and backwards-compatible within a major version.
- Clock abstraction supports standalone and external authority, deterministic seek/pause/replay.
- Publisher abstraction is transport-neutral; DIS Entity IDs are separate from NORAD IDs; validate DIS 7 encoding and SISO entity type mappings.
- Scenario selection uses mission-wide UNION across all radar FOVs, fixed membership throughout a run. Radar detection is outside HOTH.
- Runtime is offline; catalog download is explicit and separate.
- Kubernetes multicast is not assumed; UDP unicast is baseline.
- Monthly catalog samples preserve actual OMM epochs; synthetic epoch initialization must be explicit and never represented as historical truth.

## PR checklist
Explain scope, dependency issues, test commands/results, compatibility effects, configuration changes, performance evidence where relevant, and remaining risks.
