# Shared interfaces

## Satellite state

`hoth_domain::state::SatelliteState` carries a positive `u32` NORAD catalog ID, the `SimulationUtc` instant `at_utc`, ECEF kinematics, and orbital-element provenance. Canonical kinematics use meters and meters per second, and ECEF velocity is relative to the rotating ECEF axes. The SGP4-facing `TemeState` carries its own `at_utc` and uses kilometers and kilometers per second. Both frames use finite `f64` Cartesian components ordered `[x, y, z]`.

The required `frame` tag identifies the units and reference frame together. `ECEF` uses `position_m` and `velocity_m_per_s`. `TEME` uses `position_km` and `velocity_km_s`. The kilometer types scale each component by 1,000 when converting to meter units and reject overflow. The scaling preserves the reference frame. HOTH-014 owns the Earth-fixed transformation, including the Earth-rotation term for velocity.

Timestamps use RFC 3339 and serialize in UTC, including when the input has a numeric offset. `at_utc` is the absolute simulation instant of the vectors. The source record epoch is tracked independently.

Provenance records a nonblank source and record ID, the original `source_epoch_utc`, and the initialization kind (`source_epoch` or `synthetic`). A synthetic initialization carries a separate `epoch_utc`. An orientation policy can be defined when a publisher needs it.

The [canonical JSON fixture](../../crates/hoth-domain/tests/fixtures/satellite-state.json) shows the expected representation with illustrative coordinates.

## Planned interfaces

- OrbitalElement: original OMM/TLE record, actual epoch, source, acquisition time, checksum.
- ScenarioManifest: schema version; scenario clock policy; catalog snapshot and source records; fixed selected NORAD IDs; deterministic DIS Entity ID/type mapping; FOV selection metadata and provenance.
- SimulationClock: current absolute simulation UTC, rate, paused/running state, seek/replay events; external source adapter optional.
- Publisher: initialize, publish batch of canonical states at a simulation timestamp, flush/shutdown, report failures and backpressure.
- Frame conversions: TEME to ECEF with Earth-rotation velocity term; all frame assumptions explicit.
