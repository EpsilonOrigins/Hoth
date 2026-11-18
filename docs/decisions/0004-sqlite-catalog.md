# ADR 0004 — SQLite catalog schema and migrations

## Status

Proposed for HOTH-006.

## Decision

Use strict SQLite tables with foreign keys and one membership per snapshot and NORAD ID. A composite foreign key ensures membership references an element for the same object. Store original source bytes and epoch text alongside normalized UTC for chronological lookup. Keep acquisitions separate from elements and object metadata so readers can trace source and acquisition time. Deduplicate raw payloads by NORAD ID, format, and checksum.

Embed numbered SQL migrations in `hoth-catalog`. Use `PRAGMA user_version` to record the latest applied migration. Apply pending migrations under one immediate transaction so schema changes and the version commit together. Append a migration for each schema change and verify upgrades against populated databases.

## Compatibility

This is the first persisted catalog schema. The bundled SQLite engine supports strict tables, and other readers need SQLite 3.37 or later. Scenario YAML continues to select a catalog by its target month. Orbital normalization, snapshot selection, and source ingestion have separate issue scopes. See the [schema reference](../architecture/catalog.md).
