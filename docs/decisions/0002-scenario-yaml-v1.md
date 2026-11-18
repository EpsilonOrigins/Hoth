# ADR 0002 — Scenario YAML v1

## Status

Proposed for HOTH-003.

## Decision

Use the existing example as the compatibility baseline for [scenario YAML v1](../interfaces/scenario-yaml.md). Require an explicit integer schema version and validate the field names, types, modes, and values during parsing. Reject unsupported versions before interpreting their fields. Strict field checking catches typos before scenario preparation.

Use the shared simulation UTC type for the start time and normalize explicit offsets to UTC. Keep a monthly snapshot key and a numeric DIS socket address in the configuration so validation stays independent of catalog availability and DNS. Identify duplicate publishers by type and socket address.

## Compatibility

The current example's field values retain their meanings. Future v1 additions must preserve support for existing valid files. The planned radar mode and its definitions will extend the schema after HOTH-018 defines the geometry.
