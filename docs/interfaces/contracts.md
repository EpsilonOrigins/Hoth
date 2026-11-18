# Shared interfaces

## Satellite state

`hoth_domain::state::SatelliteState` carries a positive `u32` NORAD catalog ID, the `SimulationUtc` instant `at_utc`, ECEF kinematics, and orbital-element provenance. Canonical kinematics use meters and meters per second, and ECEF velocity is relative to the rotating ECEF axes. The SGP4-facing `TemeState` carries its own `at_utc` and uses kilometers and kilometers per second. Both frames use finite `f64` Cartesian components ordered `[x, y, z]`.

The required `frame` tag identifies the units and reference frame together. `ECEF` uses `position_m` and `velocity_m_per_s`. `TEME` uses `position_km` and `velocity_km_s`. The kilometer types scale each component by 1,000 when converting to meter units and reject overflow. The scaling preserves the reference frame. HOTH-014 owns the Earth-fixed transformation, including the Earth-rotation term for velocity.

Timestamps use RFC 3339 and serialize in UTC, including when the input has a numeric offset. `at_utc` is the absolute simulation instant of the vectors. The source record epoch is tracked independently.

Provenance records a nonblank source and record ID, the original `source_epoch_utc`, and the initialization kind (`source_epoch` or `synthetic`). A synthetic initialization carries a separate `epoch_utc`. An orientation policy can be defined when a publisher needs it.

The [canonical JSON fixture](../../crates/hoth-domain/tests/fixtures/satellite-state.json) shows the expected representation with illustrative coordinates.

## Scenario YAML

`hoth_config::ScenarioConfig::from_yaml` parses and validates the versioned configuration used for scenario preparation. See the [v1 fields and validation rules](scenario-yaml.md).

## Simulation clock

`hoth_clock::SimulationClock` returns the current `SimulationUtc`, authority mode, pause state, and standalone rate. Callers apply ordered `ClockEvent` values for pause, resume, seek, standalone rate changes, and externally supplied ticks. Consumers can sample a clock through a trait object. Propagation uses the reported absolute UTC so a seek can reproduce a previous state from the same inputs. See [ADR 0003](../decisions/0003-clock-and-publisher-ports.md).

## Publisher

`hoth_publisher::Publisher` provides synchronous initialization, batch submission, flush, and shutdown. The caller supplies one absolute UTC and an ordered slice of canonical states for that instant. Publishers preserve the slice order. The runner can pass the same slice to multiple publishers and can submit earlier timestamps after a seek.

Initialize each instance once before submitting or flushing. Success on submission acknowledges local acceptance, which may include bounded queuing. Backpressure means the entire batch was rejected and is safe to retry before submitting newer work. A sink failure may leave delivery partial or unknown and ends submissions to that instance. Flush drains accepted work to the sink. Shutdown releases resources, drops pending work, and can be called again. See [ADR 0003](../decisions/0003-clock-and-publisher-ports.md).

## Orbital elements

`hoth_domain::orbit::OrbitalElement` carries the NORAD ID, source UTC epoch, normalized mean elements, and original source record with provenance. See [supported input formats and validation rules](orbital-elements.md).

## Planned interfaces

- ScenarioManifest: schema version; scenario clock policy; catalog snapshot and source records; fixed selected NORAD IDs; deterministic DIS Entity ID/type mapping; FOV selection metadata and provenance.
- Frame conversions: TEME to ECEF with Earth-rotation velocity term; all frame assumptions explicit.
