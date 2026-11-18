# Architecture — High-Throughput Orbit Telemetry Harness

## Purpose
Generate plausible satellite kinematics to stimulate downstream simulation systems. Historical OMM/TLE catalogs provide representative orbital populations, **not** authoritative historical truth.

## Pipeline
1. Operator-run `hoth catalog bootstrap`: retrieve permitted historical GP records, sample monthly for five years, enrich SATCAT/RCS, persist the [SQLite catalog](catalog.md). Supports local file import and current CelesTrak refresh.
2. `hoth scenario prepare`: load versioned YAML; choose snapshot, initialize plausible orbits, evaluate radar FOV union over entire scenario, write immutable manifest containing object membership, source epochs, selected records, IDs, and provenance.
3. `hoth run`: load manifest offline; accept standalone or external clock, propagate using SGP4 at absolute UTC simulation time; convert TEME to ECEF; publish canonical state.
4. DIS 7 UDP publisher in v1; Zenoh/protobuf later via common publisher interface.

## Invariants
- Original orbital epochs never rewritten as historical facts. Synthetic initialization explicitly marked and reproducible.
- Fixed membership per mission; FOV is a preparation-time inclusion filter, not radar detection.
- No database query in the 25 Hz hot path.
- DIS Entity ID != NORAD catalog ID; deterministic mapping persisted in manifest.
- DIS Entity Type uses validated versioned SISO registry, not guessed codes.
- External clock transport details are an adapter concern; core clock is transport-neutral.
- Runtime may publish DIS and Zenoh concurrently once Zenoh is implemented.
- Default networking is UDP unicast; multicast requires verified CNI/network support.

## Deployment
Multi-stage Rust image; Kubernetes Helm chart for production; Docker Compose for local integration. Catalog imported before offline runtime; credentials not baked into image. Benchmark 15,000 objects at 25 Hz end-to-end and profile up to 50,000.

## Design risks / ADRs
- Synthetic epoch policy for distant source OMM epochs.
- FOV conservative sampling and false-negative risk.
- Independent validation of TEME/ECEF transformation and DIS wire format.
- Source availability, access terms, and catalog coverage.
