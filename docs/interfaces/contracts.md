# Shared interfaces

## Satellite state

`hoth_core::SatelliteState` carries a positive `u32` NORAD catalog ID, a simulation UTC instant, ECEF kinematics, and source provenance. The `EcefState` and `TemeState` types each carry position in meters and velocity in meters per second, using finite `f64` components ordered `[x, y, z]`. The ECEF velocity is relative to the rotating ECEF axes. A propagator that emits TEME in kilometers and kilometers per second must scale its output to SI units at the boundary.

In Serde's human-readable representation, the kinematics object carries a required `frame` tag (`ECEF` or `TEME`) and the vectors `position_m` and `velocity_m_per_s`. The canonical state requires `ECEF`. UTC timestamps use RFC 3339 and serialize in UTC, including when the input has a numeric offset.

Provenance records a nonblank source and record ID, the original `source_epoch_utc`, and the initialization kind (`source_epoch` or `synthetic`). A synthetic initialization carries a separate `epoch_utc`. An orientation policy can be defined when a publisher needs it.

## Planned interfaces

- OrbitalElement: original OMM/TLE record, actual epoch, source, acquisition time, checksum.
- ScenarioManifest: schema version; scenario clock policy; catalog snapshot and source records; fixed selected NORAD IDs; deterministic DIS Entity ID/type mapping; FOV selection metadata and provenance.
- SimulationClock: current absolute simulation UTC, rate, paused/running state, seek/replay events; external source adapter optional.
- Publisher: initialize, publish batch of canonical states at a simulation timestamp, flush/shutdown, report failures and backpressure.
- Frame conversions: TEME to ECEF with Earth-rotation velocity term; all frame assumptions explicit.
