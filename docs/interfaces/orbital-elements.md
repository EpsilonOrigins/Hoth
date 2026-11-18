# Orbital element parsing

`hoth_catalog::parse` exposes `parse_tle`, `parse_omm_json`, and `parse_omm_csv`. Each accepts one record's bytes and a caller-supplied source and record ID. Each returns a shared `OrbitalElement` and any parser warnings. All three entry points operate on local bytes.

| Input | Accepted record |
| --- | --- |
| TLE | Two 69-column ASCII element lines, optionally preceded by a title. Both checksums and matching NORAD IDs are required. Numeric and Alpha-5 IDs are supported. |
| OMM JSON | One object containing GP mean-element fields. Numeric fields may be numbers or numeric strings. |
| OMM CSV | A header and one data row containing the same GP fields. The header is retained with the row so the stored CSV can be read on its own. |

The parsers require a positive NORAD ID, epoch, inclination, right ascension of the ascending node, eccentricity, argument of perigee, mean anomaly, and mean motion. Angles are in degrees and mean motion is in revolutions per day. Inclination must be between 0 and 180, eccentricity must be at least 0 and below 1, and mean motion must be positive. The other angles accept 0 through 360 degrees, with 360 normalized to 0. All numeric values must be finite.

Optional B* and the two mean-motion coefficients are preserved as published after decoding TLE notation. Missing coefficients remain absent and produce field-specific warnings. An OMM may declare Earth, TEME, UTC, and SGP4 metadata. Missing declarations generate warnings and use those GP defaults. Explicit alternative values raise an error.

## Epoch and provenance

TLE years 57 through 99 map to 1957 through 1999, and years 00 through 56 map to 2000 through 2056. Ordinal days start at 1. Each eight-digit fractional-day increment is exactly 864 microseconds, converted using integer nanoseconds. OMM calendar timestamps accept up to nine fractional second digits. A UTC offset is normalized to UTC. An omitted offset uses the OMM UTC time system.

The returned instant is the source epoch. The original epoch text and full input bytes are preserved with a lowercase SHA-256 digest of those bytes. Line endings and the optional TLE title remain part of the byte digest. For persistence, format the normalized epoch with nine fractional seconds and store the raw payload, source epoch text, and checksum separately. The catalog records acquisition time when the caller persists the source. See the [catalog schema](../architecture/catalog.md).

The [local fixtures](../../crates/hoth-catalog/tests/fixtures/orbits) include the ISS TLE from the Rust SGP4 parser example and matching JSON and CSV OMM test records. The test checks exact UTC equality, equal orbital fields at the supplied precision, and B* within an absolute tolerance of 1e-18. See the [TLE example](https://docs.rs/sgp4/2.4.0/sgp4/struct.Elements.html#method.from_tle).
