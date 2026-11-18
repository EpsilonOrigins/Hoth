# ADR 0005 — Orbital element normalization

## Status

Proposed for HOTH-007.

## Decision

Use one shared orbital-element type for TLE and GP OMM JSON or CSV. Store a typed UTC instant at the actual source epoch and retain the original epoch text, input bytes, format, provenance, and byte checksum. The parser returns field errors for invalid essential elements and warnings for missing optional drag fields and inferred OMM metadata.

Represent GP angular fields in degrees, mean motion in revolutions per day, and drag terms in the source's GP convention. UTC normalization uses exact integer nanoseconds for TLE fractional days and supports nanosecond OMM timestamps. Propagation and synthetic initialization belong to later issues.

## Compatibility

The normalized fields are an application-level contract. Existing SQLite columns store the normalized epoch and original source fields. Scenario YAML, satellite state, and the catalog schema version retain their existing representations. See the [parser contract](../interfaces/orbital-elements.md).
