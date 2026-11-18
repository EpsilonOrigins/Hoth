# Scenario YAML v1

`hoth_config::ScenarioConfig::from_yaml` reads a single YAML document and returns a typed configuration or an error. Validation errors identify the field, using paths such as `time.start_utc` and `publishers[0].rate_hz`. YAML syntax errors report line and column when available. Parsing requires no catalog files or network access.

Fields without a default are required. The validator rejects unknown fields and duplicate mapping keys.

| Field | Accepted value |
| --- | --- |
| `schema_version` | Integer `1`. The version is checked before applying the v1 field rules. |
| `name` | A string containing at least one non-whitespace character. |
| `time.start_utc` | RFC 3339 timestamp with `Z` or a numeric offset. The timestamp is normalized to UTC. |
| `time.duration_seconds` | A positive whole number of simulation seconds. |
| `time.clock` | `standalone` or `external`. |
| `time.rate` | A finite, positive simulation clock rate multiplier. |
| `catalog.snapshot` | A monthly catalog key in `YYYY-MM` form with month `01` through `12`. Scenario preparation resolves the key against the local catalog. |
| `selection.mode` | `all` for the full snapshot or `radar_union` for any intersection during the mission. |
| `publishers` | A nonempty list of publisher mappings, kept in configuration order. |
| `publishers[].type` | `dis` or `zenoh`. |
| `publishers[].destination` | An IPv4 or bracketed IPv6 address and port from 1 through 65535. For Zenoh this is the TCP router. |
| `publishers[].rate_hz` | A frequency from 0.001 to 1000 Hz. |

Publishers with the same type and socket address are duplicates even if their rates differ or the addresses use different equivalent IPv6 spellings. Use distinct destinations for multiple DIS publishers.

See the [repository example](../../config/scenarios/example.yaml) for a standalone scenario, and the [external clock fixture](../../crates/hoth-config/tests/fixtures/external-clock.yaml) for flow mappings and multiple destinations. The clock mode selects the authority. The standalone clock uses the configured rate. External ticks specify exact UTC independently of the configured rate. See [ADR 0003](../decisions/0003-clock-and-publisher-ports.md).

## Radar field-of-view extension

Set `selection.mode: radar_union` and supply a nonempty `radars` list. Each fixed WGS-84 radar requires a unique `id`, `latitude_deg`, `longitude_deg`, `altitude_m`, `azimuth_min_deg`, `azimuth_max_deg`, `elevation_min_deg`, `elevation_max_deg`, and `range_max_m`. Optional `range_min_m` defaults to zero. Azimuth is clockwise from true north. Use 0–360 for all bearings or a decreasing interval such as 350–20 to cross north. Elevation is measured from the local tangent plane. Range is slant distance in meters. HOTH evaluates the union during preparation and uses that same membership for the entire run.

The optional top-level `dis` mapping accepts `exercise_id`, `site` and `application`, each defaulting to 1. Site and application IDs must be positive. Set a DIS publisher's optional `multicast: true` for an explicit multicast destination. The unicast default uses the host's routing table.

## Compatibility

The existing `config/scenarios/example.yaml` values are covered by a compatibility test. Keep existing valid v1 scenarios readable when adding capabilities. Each schema change requires an ADR and compatibility notes. See [ADR 0002](../decisions/0002-scenario-yaml-v1.md) for the initial schema decision.
