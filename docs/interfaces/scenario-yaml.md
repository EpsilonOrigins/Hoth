# Scenario YAML v1

`hoth_config::ScenarioConfig::from_yaml` reads a single YAML document and returns a typed configuration or an error. Validation errors identify the field, using paths such as `time.start_utc` and `publishers[0].rate_hz`. YAML syntax errors report line and column when available. Parsing requires no catalog files or network access.

All fields below are required. The validator rejects unknown fields and duplicate mapping keys.

| Field | Accepted value |
| --- | --- |
| `schema_version` | Integer `1`. The version is checked before applying the v1 field rules. |
| `name` | A string containing at least one non-whitespace character. |
| `time.start_utc` | RFC 3339 timestamp with `Z` or a numeric offset. The timestamp is normalized to UTC. |
| `time.duration_seconds` | A positive whole number of simulation seconds. |
| `time.clock` | `standalone` or `external`. |
| `time.rate` | A finite, positive simulation clock rate multiplier. |
| `catalog.snapshot` | A monthly catalog key in `YYYY-MM` form with month `01` through `12`. Scenario preparation resolves the key against the local catalog. |
| `selection.mode` | `all`, selecting the full snapshot. |
| `publishers` | A nonempty list of publisher mappings, kept in configuration order. |
| `publishers[].type` | `dis`. |
| `publishers[].destination` | An IPv4 or bracketed IPv6 address with a UDP port from 1 through 65535. |
| `publishers[].rate_hz` | A finite, positive frequency in hertz. |

Publishers with the same type and socket address are duplicates even if their rates differ or the addresses use different equivalent IPv6 spellings. Use distinct destinations for multiple DIS publishers.

See the [repository example](../../config/scenarios/example.yaml) for a standalone scenario, and the [external clock fixture](../../crates/hoth-config/tests/fixtures/external-clock.yaml) for flow mappings and multiple destinations. The clock mode selects the intended authority. External clock transport and control behavior are defined by the clock interface work.

## Radar field-of-view extension

Add `selection.mode: radar_union` and top-level `radars` after the geometry contract is approved in HOTH-018. The geometry contract will define the radar coordinates and field-of-view bounds. Scenario preparation will include an object if it enters any configured field of view at any time during the mission. That membership applies for the full run. The current parser rejects the reserved mode and radar definitions.

## Compatibility

The existing `config/scenarios/example.yaml` values are covered by a compatibility test. Keep existing valid v1 scenarios readable when adding capabilities. Each schema change requires an ADR and compatibility notes. See [ADR 0002](../decisions/0002-scenario-yaml-v1.md) for the initial schema decision.
