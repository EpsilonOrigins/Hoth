# Shared interfaces (design contract; not yet implemented)

- SatelliteState: NORAD catalog ID; simulation UTC instant; ECEF position meters and velocity meters/second for DIS-ready canonical output; source/provenance; optional orientation policy.
- OrbitalElement: original OMM/TLE record, actual epoch, source, acquisition time, checksum.
- ScenarioManifest: schema version; scenario clock policy; catalog snapshot and source records; fixed selected NORAD IDs; deterministic DIS Entity ID/type mapping; FOV selection metadata and provenance.
- SimulationClock: current absolute simulation UTC, rate, paused/running state, seek/replay events; external source adapter optional.
- Publisher: initialize, publish batch of canonical states at a simulation timestamp, flush/shutdown, report failures and backpressure.
- Frame conversions: TEME to ECEF with Earth-rotation velocity term; all frame assumptions explicit.
These are design-level contracts. Issue HOTH-002 and HOTH-005 will finalize Rust types.
